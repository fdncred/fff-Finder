//! Nushell integration that doesn't need GPUI: locating `nu`, running a
//! pipeline over the selected files, previewing structured files with
//! `open`, and NUON formatting. Terminals live in
//! `fff_Finder_core::terminals`.

use fff_Finder_core::{FileEntry, format};
use serde_json::Value as Json;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Locate `nu`: the configured path, `PATH`, then common install locations
/// (apps launched from Finder or a desktop menu get a minimal `PATH`).
pub fn find_nu(configured: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = configured.filter(|p| p.is_file()) {
        return Some(path.to_path_buf());
    }
    let exe = if cfg!(windows) { "nu.exe" } else { "nu" };
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(exe);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    let mut fallbacks: Vec<PathBuf> = Vec::new();
    if let Some(home) = dirs::home_dir() {
        fallbacks.push(home.join(".cargo/bin").join(exe));
        fallbacks.push(home.join(".local/bin").join(exe));
    }
    #[cfg(unix)]
    fallbacks.extend(
        [
            "/opt/homebrew/bin/nu",
            "/usr/local/bin/nu",
            "/usr/bin/nu",
            "/home/linuxbrew/.linuxbrew/bin/nu",
        ]
        .map(PathBuf::from),
    );
    #[cfg(windows)]
    if let Some(local) = dirs::data_local_dir() {
        fallbacks.push(local.join("Programs/nu/bin/nu.exe"));
    }
    fallbacks.into_iter().find(|p| p.is_file())
}

/// Result of running Nushell, shaped for display.
#[derive(Debug, Clone, PartialEq)]
pub enum NuOutput {
    Table {
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Text(String),
    Empty,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NuRun {
    pub output: NuOutput,
    pub error: Option<String>,
    pub elapsed: std::time::Duration,
}

/// The selection as JSON records, the same shape `fff` returns in Nushell.
pub fn entries_json(entries: &[FileEntry]) -> String {
    let rows: Vec<Json> = entries
        .iter()
        .map(|e| {
            serde_json::json!({
                "name": e.name,
                "path": e.path.to_string_lossy(),
                "type": if e.is_dir { "dir" } else { "file" },
                "kind": e.kind_label(),
                "size": e.size,
                "modified": e.modified.map(format::iso_date),
            })
        })
        .collect();
    Json::Array(rows).to_string()
}

/// Definitions prepended to every script: `fff-render` turns values JSON
/// can't keep (file sizes, durations, dates) into Nushell's display text.
const PRELUDE: &str = r#"
def fff-cell [c: any] {
    match ($c | describe) {
        'filesize' | 'duration' => ($c | into string)
        'datetime' | 'date' => ($c | format date '%Y-%m-%d %H:%M:%S')
        _ => $c
    }
}
def fff-render [] {
    let v = $in
    let t = $v | describe
    if ($t | str starts-with 'table') {
        $v | update cells {|c| fff-cell $c }
    } else if ($t | str starts-with 'list') {
        $v | each {|c| fff-cell $c }
    } else if ($t | str starts-with 'record') {
        $v | items {|k, x| {key: $k, value: (fff-cell $x)} }
    } else {
        fff-cell $v
    }
}
"#;

/// Reads the entries from stdin as a table with sizes and dates turned back
/// into Nushell types.
const TYPED_INPUT: &str = "$in | from json \
    | update size {|r| if $r.size == null { null } else { $r.size | into filesize } } \
    | update modified {|r| if $r.modified == null { null } else { $r.modified | into datetime } }";

/// Longest a filter may run before it is stopped.
pub const FILTER_TIMEOUT: Duration = Duration::from_secs(10);

/// The paths of the entries a filter keeps. `filter` is a `where` condition
/// (`size > 1mb and kind == Folder`) or, starting with `|`, a pipeline whose
/// rows keep their `path` column.
pub fn filter_paths(
    nu: &Path,
    filter: &str,
    entries: &[FileEntry],
    cwd: &Path,
) -> Result<Vec<PathBuf>, String> {
    let filter = filter.trim();
    let stage = if filter.starts_with('|') {
        filter.to_string()
    } else {
        format!("| where {filter}")
    };
    let script = format!("{TYPED_INPUT}\n{stage}\n| get path | to json --raw");
    let out = exec(
        nu,
        &script,
        Some(entries_json(entries)),
        cwd,
        &[],
        Some(FILTER_TIMEOUT),
    )?;
    serde_json::from_str::<Vec<String>>(out.trim())
        .map(|paths| paths.into_iter().map(PathBuf::from).collect())
        .map_err(|_| "The filter must keep rows with a path column.".to_string())
}

/// The new names a rename closure gives the entries, in order. Each item has
/// the usual columns plus `stem`, `ext`, and a 1-based `index`; the closure
/// may take it as a parameter (`{|f| $f.name }`) or as `$in`. Text that
/// isn't a closure is treated as its body, with the item as `$f`.
pub fn rename_names(
    nu: &Path,
    closure: &str,
    entries: &[FileEntry],
    cwd: &Path,
) -> Result<Vec<String>, String> {
    let closure = closure.trim();
    let closure = if closure.starts_with('{') {
        closure.to_string()
    } else {
        format!("{{|f| {closure} }}")
    };
    let script = format!(
        "let rename = {closure}\n\
         {TYPED_INPUT} | enumerate | each {{|it| \
             let parts = $it.item.name | path parse; \
             $it.item | insert index ($it.index + 1) | insert stem $parts.stem \
                 | insert ext ($parts.extension? | default '') \
         }} | each {{|f| $f | do $rename $f | into string }} | to json --raw"
    );
    let out = exec(
        nu,
        &script,
        Some(entries_json(entries)),
        cwd,
        &[],
        Some(FILTER_TIMEOUT),
    )?;
    let names: Vec<String> = serde_json::from_str(out.trim())
        .map_err(|_| "The closure must return a name for each item.".to_string())?;
    if names.len() != entries.len() {
        return Err("The closure must return one name per item.".into());
    }
    Ok(names)
}

/// `source` highlighted by `nu-highlight` in a login shell, so the user's
/// `color_config` applies. Returns the ANSI-colored text.
pub fn highlight_nu(nu: &Path, source: &str) -> Result<String, String> {
    let child = Command::new(nu)
        .args([
            "--login",
            "--stdin",
            "-c",
            "$env.config.use_ansi_coloring = true; $in | nu-highlight",
        ])
        .env_remove("NO_COLOR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("could not run {}: {err}", nu.display()))?;
    let output = wait_for(child, Some(source.to_string()), Some(PREVIEW_TIMEOUT))
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "nu-highlight took too long".to_string())?;
    if !output.status.success() {
        return Err(strip_ansi(&String::from_utf8_lossy(&output.stderr)));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The comments in `nu-highlight` output. Nushell has no comment style, so
/// it leaves them plain: a comment is a `#` that starts a word and runs to the
/// end of its line without touching anything `styled`. Strings, raw strings
/// and interpolations that contain `#` are all styled, so they never match.
pub fn comment_ranges(
    text: &str,
    styled: &[std::ops::Range<usize>],
) -> Vec<std::ops::Range<usize>> {
    let bytes = text.as_bytes();
    let mut comments = Vec::new();
    let mut line_start = 0;
    for line in text.split_inclusive('\n') {
        let line_end = line_start + line.trim_end_matches(['\n', '\r']).len();
        let starts_word = |i: usize| {
            i == line_start
                || bytes[i - 1].is_ascii_whitespace()
                || b";|({[".contains(&bytes[i - 1])
        };
        let comment = (line_start..line_end)
            .filter(|&i| bytes[i] == b'#' && starts_word(i))
            .find(|&i| !styled.iter().any(|r| r.start < line_end && r.end > i));
        if let Some(start) = comment {
            comments.push(start..line_end);
        }
        line_start += line.len();
    }
    comments
}

/// Run a script with no input; returns its stdout or its error message.
pub fn run_raw(nu: &Path, script: &str, cwd: &Path) -> Result<String, String> {
    exec(nu, script, None, cwd, &[], Some(FILTER_TIMEOUT))
}

/// Run a script and return its stdout, or its error message.
fn exec(
    nu: &Path,
    script: &str,
    stdin: Option<String>,
    cwd: &Path,
    env: &[(&str, &std::ffi::OsStr)],
    timeout: Option<Duration>,
) -> Result<String, String> {
    let mut cmd = Command::new(nu);
    cmd.arg("--no-config-file");
    if stdin.is_some() {
        cmd.arg("--stdin");
    }
    cmd.arg("-c")
        .arg(script)
        .current_dir(cwd)
        .env("NO_COLOR", "1")
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in env {
        cmd.env(key, value);
    }
    let output = cmd
        .spawn()
        .and_then(|child| wait_for(child, stdin, timeout))
        .map_err(|err| format!("could not run {}: {err}", nu.display()))?
        .ok_or_else(|| {
            format!(
                "nu took longer than {} s and was stopped",
                timeout.unwrap_or_default().as_secs()
            )
        })?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));
        Err(if stderr.trim().is_empty() {
            format!("nu exited with {}", output.status)
        } else {
            stderr.trim().to_string()
        })
    }
}

/// Run a user pipeline with the entries as its input table.
///
/// The pipeline is the user's own code; the entries travel on stdin as JSON,
/// never spliced into the script.
pub fn run_pipeline(nu: &Path, pipeline: &str, entries: &[FileEntry], cwd: &Path) -> NuRun {
    let script = format!("{TYPED_INPUT} | do {{\n{pipeline}\n}} | fff-render | to json --raw");
    let script = format!("{PRELUDE}\n{script}");
    run_script(nu, &script, Some(entries_json(entries)), cwd, &[], None)
}

/// Longest a preview may take before it is abandoned and the file shows its icon.
pub const PREVIEW_TIMEOUT: Duration = Duration::from_secs(10);

/// Preview a structured file (CSV, JSON, TOML, YAML, SQLite, …) with `open`.
pub fn preview_file(nu: &Path, path: &Path) -> NuRun {
    const SCRIPT: &str = r#"
let data = open $env.FFF_PREVIEW_PATH
let type = $data | describe
let data = if ($type | str starts-with 'table') or ($type | str starts-with 'list') {
    $data | first 500
} else if ($type | str starts-with 'SQLiteDatabase') or ($type | str contains 'sqlite') {
    $data | schema | get tables | transpose table info | each {|t| {table: $t.table, columns: ($t.info.columns | length)} }
} else {
    $data
}
$data | fff-render | to json --raw
"#;
    let cwd = path.parent().unwrap_or(Path::new("."));
    run_script(
        nu,
        &format!("{PRELUDE}\n{SCRIPT}"),
        None,
        cwd,
        &[("FFF_PREVIEW_PATH", path.as_os_str())],
        Some(PREVIEW_TIMEOUT),
    )
}

/// File types `open` turns into structured data.
pub fn is_structured(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    matches!(
        ext.as_str(),
        "csv"
            | "tsv"
            | "json"
            | "jsonl"
            | "nuon"
            | "toml"
            | "yaml"
            | "yml"
            | "xml"
            | "ini"
            | "sqlite"
            | "sqlite3"
            | "db"
            | "xlsx"
            | "ods"
            | "eml"
            | "ics"
            | "vcf"
            | "msgpack"
            | "msgpackz"
            | "plist"
    )
}

fn run_script(
    nu: &Path,
    script: &str,
    stdin: Option<String>,
    cwd: &Path,
    env: &[(&str, &std::ffi::OsStr)],
    timeout: Option<Duration>,
) -> NuRun {
    let started = Instant::now();
    let mut cmd = Command::new(nu);
    cmd.arg("--no-config-file");
    if stdin.is_some() {
        cmd.arg("--stdin");
    }
    cmd.arg("-c")
        .arg(script)
        .current_dir(cwd)
        .env("NO_COLOR", "1")
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in env {
        cmd.env(key, value);
    }
    let result = cmd
        .spawn()
        .and_then(|child| wait_for(child, stdin, timeout));
    let elapsed = started.elapsed();
    match result {
        Err(err) => NuRun {
            output: NuOutput::Empty,
            error: Some(format!("could not run {}: {err}", nu.display())),
            elapsed,
        },
        Ok(None) => NuRun {
            output: NuOutput::Empty,
            error: Some(format!(
                "nu took longer than {} s and was stopped",
                timeout.unwrap_or_default().as_secs()
            )),
            elapsed,
        },
        Ok(Some(out)) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = strip_ansi(&String::from_utf8_lossy(&out.stderr));
            let output = parse_output(&stdout);
            let error = (!out.status.success()
                || (output == NuOutput::Empty && !stderr.trim().is_empty()))
            .then(|| {
                let msg = stderr.trim().to_string();
                if msg.is_empty() {
                    format!("nu exited with {}", out.status)
                } else {
                    msg
                }
            });
            NuRun {
                output,
                error,
                elapsed,
            }
        }
    }
}

/// Feed stdin, collect output, and kill the child if it outlives `timeout`
/// (`Ok(None)`). Pipes are drained on threads so a large output can't
/// deadlock against a full pipe.
fn wait_for(
    mut child: std::process::Child,
    stdin: Option<String>,
    timeout: Option<Duration>,
) -> std::io::Result<Option<std::process::Output>> {
    if let Some(input) = stdin
        && let Some(mut pipe) = child.stdin.take()
    {
        std::thread::spawn(move || {
            let _ = pipe.write_all(input.as_bytes());
        });
    }
    let Some(timeout) = timeout else {
        return child.wait_with_output().map(Some);
    };
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut buf);
            }
            buf
        })
    };
    let stdout = drain(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let stderr = drain(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    Ok(Some(std::process::Output {
        status,
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    }))
}

/// Turn `to json` output into a table or text.
pub fn parse_output(stdout: &str) -> NuOutput {
    let text = stdout.trim();
    if text.is_empty() {
        return NuOutput::Empty;
    }
    let Ok(json) = serde_json::from_str::<Json>(text) else {
        return NuOutput::Text(text.to_string());
    };
    match json {
        Json::Null => NuOutput::Empty,
        Json::String(s) => NuOutput::Text(s),
        Json::Array(items) => {
            if items.is_empty() {
                return NuOutput::Empty;
            }
            let mut columns: Vec<String> = Vec::new();
            let all_records = items.iter().all(Json::is_object);
            if all_records {
                for item in &items {
                    if let Json::Object(map) = item {
                        for key in map.keys() {
                            if !columns.contains(key) {
                                columns.push(key.clone());
                            }
                        }
                    }
                }
                let rows = items
                    .iter()
                    .map(|item| {
                        columns
                            .iter()
                            .map(|c| item.get(c).map(cell).unwrap_or_default())
                            .collect()
                    })
                    .collect();
                NuOutput::Table { columns, rows }
            } else {
                NuOutput::Table {
                    columns: vec!["value".into()],
                    rows: items.iter().map(|v| vec![cell(v)]).collect(),
                }
            }
        }
        Json::Object(map) => NuOutput::Table {
            columns: vec!["key".into(), "value".into()],
            rows: map.iter().map(|(k, v)| vec![k.clone(), cell(v)]).collect(),
        },
        other => NuOutput::Text(cell(&other)),
    }
}

/// One table cell, abbreviated like Nushell does for nested data.
fn cell(value: &Json) -> String {
    match value {
        Json::Null => String::new(),
        Json::Bool(b) => b.to_string(),
        Json::Number(n) => n.to_string(),
        Json::String(s) => s.clone(),
        Json::Array(items) => format!("[list {} item{}]", items.len(), plural(items.len())),
        Json::Object(map) => format!("{{record {} field{}}}", map.len(), plural(map.len())),
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// Paths as NUON: a quoted string for one path, a list for several.
pub fn to_nuon(paths: &[PathBuf]) -> String {
    let quoted: Vec<String> = paths
        .iter()
        .map(|p| Json::String(p.to_string_lossy().into_owned()).to_string())
        .collect();
    match quoted.as_slice() {
        [one] => one.clone(),
        many => format!("[{}]", many.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_comments_nu_highlight_left_plain() {
        let text = "# top\nlet x = 1 # trailing\n\"a # b\" #c\nls#not\n  # indented\r\n";
        let at = |needle: &str| {
            let start = text.find(needle).unwrap();
            start..start + needle.len()
        };
        // What nu-highlight colors: `let`, `x`, `1`, the string, and `ls#not`.
        let styled = [
            at("let"),
            at("x ="),
            at("1 "),
            at("\"a # b\""),
            at("ls#not"),
        ];
        let found: Vec<&str> = comment_ranges(text, &styled)
            .into_iter()
            .map(|r| &text[r])
            .collect();
        assert_eq!(found, ["# top", "# trailing", "#c", "# indented"]);
        assert!(comment_ranges("", &[]).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_slow_child_is_stopped_at_the_timeout() {
        let child = Command::new("sleep")
            .arg("5")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let started = Instant::now();
        let out = wait_for(child, None, Some(Duration::from_millis(200))).unwrap();
        assert!(out.is_none());
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn parses_tables() {
        let out = parse_output(r#"[{"a":1,"b":"x"},{"a":2,"c":[1,2]}]"#);
        assert_eq!(
            out,
            NuOutput::Table {
                columns: vec!["a".into(), "b".into(), "c".into()],
                rows: vec![
                    vec!["1".into(), "x".into(), "".into()],
                    vec!["2".into(), "".into(), "[list 2 items]".into()],
                ],
            }
        );
        assert_eq!(parse_output("\"hi\""), NuOutput::Text("hi".into()));
        assert_eq!(parse_output("[]"), NuOutput::Empty);
        assert!(matches!(parse_output(r#"{"k":1}"#), NuOutput::Table { .. }));
    }

    #[test]
    fn nuon_quoting() {
        assert_eq!(to_nuon(&["/a b".into()]), r#""/a b""#);
        assert_eq!(
            to_nuon(&["/a".into(), "/b\"c".into()]),
            r#"["/a", "/b\"c"]"#
        );
    }

    #[test]
    fn strips_ansi() {
        assert_eq!(strip_ansi("\u{1b}[31mError\u{1b}[0m: x"), "Error: x");
    }

    /// Runs only where `nu` is installed.
    #[test]
    fn filters_with_real_nu() {
        let Some(nu) = find_nu(None) else { return };
        let dir = std::env::temp_dir().join(format!("fff-nu-filter-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let small = dir.join("small.txt");
        let big = dir.join("big.log");
        std::fs::write(&small, "x").unwrap();
        std::fs::write(&big, "y".repeat(5000)).unwrap();
        let entries: Vec<FileEntry> = [&small, &big]
            .iter()
            .map(|p| FileEntry::from_path(p).unwrap())
            .collect();

        assert_eq!(
            filter_paths(&nu, "size > 1kb", &entries, &dir).unwrap(),
            std::slice::from_ref(&big)
        );
        assert_eq!(
            filter_paths(
                &nu,
                "name =~ 'small' and modified > 2020-01-01",
                &entries,
                &dir
            )
            .unwrap(),
            std::slice::from_ref(&small)
        );
        assert_eq!(
            filter_paths(&nu, "| sort-by size --reverse | first 1", &entries, &dir).unwrap(),
            std::slice::from_ref(&big)
        );
        assert!(
            filter_paths(&nu, "size > 1tb", &entries, &dir)
                .unwrap()
                .is_empty()
        );
        let typo = filter_paths(&nu, "sise > 1kb", &entries, &dir).unwrap_err();
        assert!(typo.contains("sise"), "{typo}");
        let dropped = filter_paths(&nu, "| select name", &entries, &dir).unwrap_err();
        assert!(dropped.contains("path"), "{dropped}");

        let names = |closure: &str| rename_names(&nu, closure, &entries, &dir);
        assert_eq!(
            names("{|f| $f.name | str upcase }").unwrap(),
            ["SMALL.TXT", "BIG.LOG"]
        );
        assert_eq!(names("{ $in.stem }").unwrap(), ["small", "big"]);
        assert_eq!(
            names("$\"($f.index) ($f.stem).($f.ext)\"").unwrap(),
            ["1 small.txt", "2 big.log"]
        );
        assert!(names("{|f| $f.nope }").unwrap_err().contains("nope"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Runs only where `nu` is installed.
    #[test]
    fn pipeline_and_preview_with_real_nu() {
        let Some(nu) = find_nu(None) else { return };
        let dir = std::env::temp_dir().join(format!("fff-nu-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let csv = dir.join("people.csv");
        std::fs::write(&csv, "name,age\nana,31\nbo,42\n").unwrap();
        let entries = vec![FileEntry::from_path(&csv).unwrap()];

        let run = run_pipeline(&nu, "select name size", &entries, &dir);
        assert_eq!(run.error, None, "{run:?}");
        let NuOutput::Table { columns, rows } = run.output else {
            panic!("expected table: {run:?}")
        };
        assert_eq!(columns, ["name", "size"]);
        assert_eq!(rows[0][0], "people.csv");
        // File sizes come back formatted the way Nushell shows them.
        assert_eq!(rows[0][1], "22 B");

        let run = preview_file(&nu, &csv);
        assert_eq!(run.error, None, "{run:?}");
        assert_eq!(
            run.output,
            NuOutput::Table {
                columns: vec!["name".into(), "age".into()],
                rows: vec![
                    vec!["ana".into(), "31".into()],
                    vec!["bo".into(), "42".into()]
                ],
            }
        );

        let run = run_pipeline(&nu, "this-command-does-not-exist", &entries, &dir);
        assert!(run.error.is_some());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
