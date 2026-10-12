//! `nu_plugin_fff`: fffFinder's search engine as Nushell commands.
//!
//! The plugin keeps its indexes in memory between calls (garbage collection
//! is disabled once an index exists), so repeated searches are instant.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use chrono::{DateTime, FixedOffset, Local};
use fff_finder_core::{
    Config, Engine, EngineOptions, FileEntry, GrepRequest, KindFilter, SearchRequest, config,
};
use nu_plugin::{
    EngineInterface, EvaluatedCall, MsgPackSerializer, Plugin, PluginCommand, serve_plugin,
};
use nu_protocol::{
    Category, Example, LabeledError, PipelineData, Record, Signature, Span, SyntaxShape, Type,
    Value, record,
};

pub struct FffPlugin {
    engine: OnceLock<Arc<Engine>>,
}

impl FffPlugin {
    /// One engine per plugin process. It starts with no roots: plain searches
    /// index the current directory on demand, `--all` adds the configured roots.
    fn engine(&self, engine: &EngineInterface) -> Arc<Engine> {
        let _ = engine.set_gc_disabled(true);
        self.engine
            .get_or_init(|| {
                Engine::new(EngineOptions {
                    roots: Vec::new(),
                    db_dir: Some(config::data_dir().join("plugin")),
                    watch: true,
                })
            })
            .clone()
    }
}

impl Plugin for FffPlugin {
    fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").into()
    }

    fn commands(&self) -> Vec<Box<dyn PluginCommand<Plugin = Self>>> {
        vec![
            Box::new(FffSearch),
            Box::new(FffGrep),
            Box::new(FffStatus),
            Box::new(FffPick),
            Box::new(FffGui),
        ]
    }
}

/// Resolve the scope of a search: `--root`, the configured roots (`--all`),
/// or the caller's current directory.
fn scope(
    plugin: &FffPlugin,
    engine: &EngineInterface,
    call: &EvaluatedCall,
) -> Result<(Arc<Engine>, Option<PathBuf>), LabeledError> {
    let fff = plugin.engine(engine);
    let cwd = PathBuf::from(engine.get_current_dir()?);
    if call.has_flag("all")? {
        let roots = Config::load().search_roots;
        if fff.roots() != roots {
            fff.set_roots(&roots);
        }
        return Ok((fff, None));
    }
    let root = match call.get_flag::<String>("root")? {
        Some(root) => {
            let path = PathBuf::from(&root);
            let path = if path.is_absolute() {
                path
            } else {
                cwd.join(path)
            };
            if !path.is_dir() {
                let span = call.get_flag_span("root").unwrap_or(call.head);
                return Err(LabeledError::new("Not a directory")
                    .with_label(format!("{} is not a folder", path.display()), span));
            }
            path
        }
        None => cwd,
    };
    Ok((fff, Some(root)))
}

/// Wait for the index, polling for Ctrl-C.
fn wait_ready(
    fff: &Engine,
    scope: Option<&Path>,
    engine: &EngineInterface,
    span: Span,
) -> Result<(), LabeledError> {
    while !fff.prepare(scope, Duration::from_millis(100)) {
        engine.signals().check(&span)?;
    }
    Ok(())
}

fn date_value(time: std::time::SystemTime, span: Span) -> Value {
    let local: DateTime<Local> = time.into();
    let fixed: DateTime<FixedOffset> = local.fixed_offset();
    Value::date(fixed, span)
}

fn entry_record(entry: &FileEntry, span: Span) -> Record {
    record! {
        "name" => Value::string(&entry.name, span),
        "path" => Value::string(entry.path.to_string_lossy(), span),
        "type" => Value::string(if entry.is_dir { "dir" } else { "file" }, span),
        "kind" => Value::string(entry.kind_label(), span),
        "size" => entry.size.map_or(Value::nothing(span), |s| Value::filesize(s as i64, span)),
        "modified" => entry.modified.map_or(Value::nothing(span), |t| date_value(t, span)),
    }
}

pub struct FffSearch;

impl PluginCommand for FffSearch {
    type Plugin = FffPlugin;

    fn name(&self) -> &str {
        "fff"
    }

    fn description(&self) -> &str {
        "Fuzzy-find files and folders with fff-search."
    }

    fn extra_description(&self) -> &str {
        "Searches the current directory by default. Words are fuzzy and typo tolerant. \
Query syntax: `*.rs` (extension), `src/**/*.rs` (glob), `/dir/` (path segment), \
`git:modified`, and `!` to negate any of them."
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["find", "fuzzy", "fzf", "files", "search", "locate"]
    }

    fn signature(&self) -> Signature {
        Signature::build(self.name())
            .rest("query", SyntaxShape::String, "Search words and filters.")
            .named(
                "root",
                SyntaxShape::Directory,
                "Folder to search instead of the current directory.",
                Some('r'),
            )
            .switch(
                "all",
                "Search every configured root (this computer).",
                Some('a'),
            )
            .named(
                "kind",
                SyntaxShape::String,
                "Only this kind: folder, document, image, movie, audio, code, archive.",
                Some('k'),
            )
            .named(
                "limit",
                SyntaxShape::Int,
                "Maximum number of results (default 50).",
                Some('l'),
            )
            .switch("hidden", "Include hidden files.", Some('H'))
            .switch("score", "Add the ranking score column.", Some('s'))
            .input_output_type(Type::Nothing, Type::table())
            .category(Category::FileSystem)
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![
            Example {
                example: "fff main rs",
                description: "Find files matching 'main' and 'rs' under the current directory.",
                result: None,
            },
            Example {
                example: "fff --all budget --kind document",
                description: "Search the whole computer for documents named like 'budget'.",
                result: None,
            },
            Example {
                example: "fff *.toml !target | get path",
                description: "List TOML files outside of target folders.",
                result: None,
            },
        ]
    }

    fn run(
        &self,
        plugin: &FffPlugin,
        engine: &EngineInterface,
        call: &EvaluatedCall,
        _input: PipelineData,
    ) -> Result<PipelineData, LabeledError> {
        let span = call.head;
        let query = call.rest::<String>(0)?.join(" ");
        let kind = match call.get_flag::<String>("kind")? {
            Some(name) => KindFilter::from_name(&name).ok_or_else(|| {
                LabeledError::new("Unknown kind").with_label(
                    "expected folder, document, image, movie, audio, code, or archive",
                    call.get_flag_span("kind").unwrap_or(span),
                )
            })?,
            None => KindFilter::Any,
        };
        let limit = call.get_flag::<i64>("limit")?.unwrap_or(50).max(1) as usize;
        let with_score = call.has_flag("score")?;
        let (fff, scope) = scope(plugin, engine, call)?;
        wait_ready(&fff, scope.as_deref(), engine, span)?;
        let response = fff.search(&SearchRequest {
            query,
            scope,
            kind,
            limit,
            show_hidden: call.has_flag("hidden")?,
        });
        let rows = response
            .hits
            .iter()
            .map(|hit| {
                let mut row = entry_record(&hit.entry, span);
                if let Some(git) = &hit.git_status {
                    row.push("git", Value::string(git, span));
                }
                if with_score {
                    row.push("score", Value::int(hit.score as i64, span));
                }
                Value::record(row, span)
            })
            .collect();
        Ok(PipelineData::value(Value::list(rows, span), None))
    }
}

pub struct FffGrep;

impl PluginCommand for FffGrep {
    type Plugin = FffPlugin;

    fn name(&self) -> &str {
        "fff grep"
    }

    fn description(&self) -> &str {
        "Search file contents with fff's SIMD grep."
    }

    fn extra_description(&self) -> &str {
        "Plain text by default (smart case). Add filters such as `*.rs` or `/src/` after the pattern to narrow the files searched."
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["rg", "ripgrep", "content", "text"]
    }

    fn signature(&self) -> Signature {
        Signature::build(self.name())
            .rest(
                "pattern",
                SyntaxShape::String,
                "Text to find, optionally followed by file filters.",
            )
            .named(
                "root",
                SyntaxShape::Directory,
                "Folder to search instead of the current directory.",
                Some('r'),
            )
            .switch("all", "Search every configured root.", Some('a'))
            .switch(
                "regex",
                "Treat the pattern as a regular expression.",
                Some('e'),
            )
            .named(
                "limit",
                SyntaxShape::Int,
                "Maximum number of matching lines (default 200).",
                Some('l'),
            )
            .input_output_type(Type::Nothing, Type::table())
            .category(Category::FileSystem)
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![Example {
            example: "fff grep TODO *.rs | group-by path",
            description: "Find TODOs in Rust files, grouped by file.",
            result: None,
        }]
    }

    fn run(
        &self,
        plugin: &FffPlugin,
        engine: &EngineInterface,
        call: &EvaluatedCall,
        _input: PipelineData,
    ) -> Result<PipelineData, LabeledError> {
        let span = call.head;
        let pattern = call.rest::<String>(0)?.join(" ");
        if pattern.trim().is_empty() {
            return Err(
                LabeledError::new("Missing pattern").with_label("give text to search for", span)
            );
        }
        let limit = call.get_flag::<i64>("limit")?.unwrap_or(200).max(1) as usize;
        let (fff, scope) = scope(plugin, engine, call)?;
        wait_ready(&fff, scope.as_deref(), engine, span)?;
        let response = fff.grep(&GrepRequest {
            pattern,
            scope,
            regex: call.has_flag("regex")?,
            limit,
        });
        let rows = response
            .hits
            .iter()
            .map(|hit| {
                Value::record(
                    record! {
                        "path" => Value::string(hit.path.to_string_lossy(), span),
                        "line" => Value::int(hit.line as i64, span),
                        "column" => Value::int(hit.column as i64 + 1, span),
                        "text" => Value::string(&hit.text, span),
                    },
                    span,
                )
            })
            .collect();
        Ok(PipelineData::value(Value::list(rows, span), None))
    }
}

pub struct FffStatus;

impl PluginCommand for FffStatus {
    type Plugin = FffPlugin;

    fn name(&self) -> &str {
        "fff status"
    }

    fn description(&self) -> &str {
        "Show the plugin's indexes and the shared fffFinder settings."
    }

    fn signature(&self) -> Signature {
        Signature::build(self.name())
            .input_output_type(Type::Nothing, Type::record())
            .category(Category::FileSystem)
    }

    fn run(
        &self,
        plugin: &FffPlugin,
        engine: &EngineInterface,
        call: &EvaluatedCall,
        _input: PipelineData,
    ) -> Result<PipelineData, LabeledError> {
        let span = call.head;
        let fff = plugin.engine(engine);
        let config = Config::load();
        let roots = fff
            .status()
            .into_iter()
            .map(|s| {
                Value::record(
                    record! {
                        "path" => Value::string(s.path.to_string_lossy(), span),
                        "files" => Value::int(s.indexed_files as i64, span),
                        "scanning" => Value::bool(s.scanning, span),
                        "git" => Value::bool(s.git_repo, span),
                    },
                    span,
                )
            })
            .collect();
        let configured = config
            .search_roots
            .iter()
            .map(|p| Value::string(p.to_string_lossy(), span))
            .collect();
        let value = Value::record(
            record! {
                "indexes" => Value::list(roots, span),
                "configured_roots" => Value::list(configured, span),
                "settings" => Value::string(config::config_path().to_string_lossy(), span),
                "gui" => gui_binary().map_or(Value::nothing(span), |p| Value::string(p.to_string_lossy(), span)),
            },
            span,
        );
        Ok(PipelineData::value(value, None))
    }
}

/// The GUI binary: next to this plugin, or on `PATH`.
fn gui_binary() -> Option<PathBuf> {
    let exe_name = if cfg!(windows) {
        "fffFinder.exe"
    } else {
        "fffFinder"
    };
    if let Ok(me) = std::env::current_exe()
        && let Some(dir) = me.parent()
    {
        let sibling = dir.join(exe_name);
        if sibling.is_file() {
            return Some(sibling);
        }
        // Inside a macOS app bundle the GUI lives in Contents/MacOS.
        let bundled = dir.join("../MacOS").join(exe_name);
        if bundled.is_file() {
            return Some(bundled);
        }
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(exe_name))
        .find(|p| p.is_file())
        .or_else(|| {
            let app = PathBuf::from("/Applications/fffFinder.app/Contents/MacOS").join(exe_name);
            app.is_file().then_some(app)
        })
}

fn require_gui(span: Span) -> Result<PathBuf, LabeledError> {
    gui_binary().ok_or_else(|| {
        LabeledError::new("fffFinder not found")
            .with_label("could not find the fffFinder GUI", span)
            .with_help("install it next to nu_plugin_fff or on PATH (see scripts/install.nu)")
    })
}

pub struct FffPick;

impl PluginCommand for FffPick {
    type Plugin = FffPlugin;

    fn name(&self) -> &str {
        "fff pick"
    }

    fn description(&self) -> &str {
        "Open the fffFinder window to choose files, and return the chosen paths."
    }

    fn extra_description(&self) -> &str {
        "Press Return (or double-click) to choose, Escape to cancel. Returns nothing when cancelled."
    }

    fn signature(&self) -> Signature {
        Signature::build(self.name())
            .rest("query", SyntaxShape::String, "Initial search text.")
            .named(
                "root",
                SyntaxShape::Directory,
                "Folder to open instead of the current directory.",
                Some('r'),
            )
            .switch(
                "multiple",
                "Allow choosing several items; always returns a list.",
                Some('m'),
            )
            .input_output_types(vec![
                (Type::Nothing, Type::String),
                (Type::Nothing, Type::List(Box::new(Type::String))),
                (Type::Nothing, Type::Nothing),
            ])
            .category(Category::FileSystem)
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![
            Example {
                example: "open (fff pick *.csv)",
                description: "Pick a CSV file and open it as a table.",
                result: None,
            },
            Example {
                example: "fff pick --multiple | each { ls -l $in }",
                description: "Pick several files and list their details.",
                result: None,
            },
        ]
    }

    fn run(
        &self,
        _plugin: &FffPlugin,
        engine: &EngineInterface,
        call: &EvaluatedCall,
        _input: PipelineData,
    ) -> Result<PipelineData, LabeledError> {
        let span = call.head;
        let gui = require_gui(span)?;
        let cwd = PathBuf::from(engine.get_current_dir()?);
        let root = call
            .get_flag::<String>("root")?
            .map(|r| cwd.join(r))
            .unwrap_or(cwd.clone());
        let multiple = call.has_flag("multiple")?;
        let query = call.rest::<String>(0)?.join(" ");

        let mut cmd = Command::new(&gui);
        cmd.arg("--pick").arg(&root).current_dir(&cwd);
        if multiple {
            cmd.arg("--multiple");
        }
        if !query.is_empty() {
            cmd.arg("--query").arg(&query);
        }
        let output = cmd
            .stdin(Stdio::null())
            .stderr(Stdio::inherit())
            .output()
            .map_err(|e| {
                LabeledError::new(format!("failed to run fffFinder: {e}")).with_label("here", span)
            })?;
        if !output.status.success() {
            // Cancelled.
            return Ok(PipelineData::value(Value::nothing(span), None));
        }
        let paths: Vec<String> = serde_json::from_slice(&output.stdout).map_err(|e| {
            LabeledError::new(format!("unexpected output from fffFinder: {e}"))
                .with_label("here", span)
        })?;
        let value = if multiple {
            Value::list(
                paths.into_iter().map(|p| Value::string(p, span)).collect(),
                span,
            )
        } else {
            paths
                .into_iter()
                .next()
                .map_or(Value::nothing(span), |p| Value::string(p, span))
        };
        Ok(PipelineData::value(value, None))
    }
}

pub struct FffGui;

impl PluginCommand for FffGui {
    type Plugin = FffPlugin;

    fn name(&self) -> &str {
        "fff gui"
    }

    fn description(&self) -> &str {
        "Open the fffFinder window at a folder, optionally searching."
    }

    fn signature(&self) -> Signature {
        Signature::build(self.name())
            .optional(
                "path",
                SyntaxShape::Directory,
                "Folder to open (default: current directory).",
            )
            .named("query", SyntaxShape::String, "Start a search.", Some('q'))
            .input_output_type(Type::Nothing, Type::Nothing)
            .category(Category::FileSystem)
    }

    fn run(
        &self,
        _plugin: &FffPlugin,
        engine: &EngineInterface,
        call: &EvaluatedCall,
        _input: PipelineData,
    ) -> Result<PipelineData, LabeledError> {
        let span = call.head;
        let gui = require_gui(span)?;
        let cwd = PathBuf::from(engine.get_current_dir()?);
        let path = call.opt::<String>(0)?.map(|p| cwd.join(p)).unwrap_or(cwd);
        let mut cmd = Command::new(gui);
        cmd.arg(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(query) = call.get_flag::<String>("query")? {
            cmd.arg("--query").arg(query);
        }
        cmd.spawn().map_err(|e| {
            LabeledError::new(format!("failed to start fffFinder: {e}")).with_label("here", span)
        })?;
        Ok(PipelineData::value(Value::nothing(span), None))
    }
}

fn main() {
    serve_plugin(
        &FffPlugin {
            engine: OnceLock::new(),
        },
        MsgPackSerializer,
    );
}
