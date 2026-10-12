//! Nushell integration that doesn't need GPUI: locating `nu`, running a
//! pipeline over the selected files, previewing structured files with
//! `open`, opening a terminal running `nu`, and NUON formatting.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use fff_finder_core::{FileEntry, format};
use serde_json::Value as Json;

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

/// Run a user pipeline with the entries as its input table.
///
/// The pipeline is the user's own code; the entries travel on stdin as JSON,
/// never spliced into the script.
pub fn run_pipeline(nu: &Path, pipeline: &str, entries: &[FileEntry], cwd: &Path) -> NuRun {
    // `($in | from json)` converts sizes and dates back into Nushell types.
    let script = format!(
        "$in | from json | update size {{|r| if $r.size == null {{ null }} else {{ $r.size | into filesize }} }} \
         | update modified {{|r| if $r.modified == null {{ null }} else {{ $r.modified | into datetime }} }} \
         | do {{\n{pipeline}\n}} | fff-render | to json --raw"
    );
    let script = format!("{PRELUDE}\n{script}");
    run_script(nu, &script, Some(entries_json(entries)), cwd, &[])
}

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
) -> NuRun {
    let started = std::time::Instant::now();
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
    let result = cmd.spawn().and_then(|mut child| {
        if let Some(input) = stdin
            && let Some(mut pipe) = child.stdin.take()
        {
            // A writer thread avoids a deadlock when the output is large.
            std::thread::spawn(move || {
                let _ = pipe.write_all(input.as_bytes());
            });
        }
        child.wait_with_output()
    });
    let elapsed = started.elapsed();
    match result {
        Err(err) => NuRun {
            output: NuOutput::Empty,
            error: Some(format!("could not run {}: {err}", nu.display())),
            elapsed,
        },
        Ok(out) => {
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

/// Open a terminal window running `nu` in `dir`.
///
/// `template` (from settings) overrides the default; `{dir}` and `{nu}` are
/// replaced and the result is split on whitespace into program and arguments.
pub fn open_terminal(nu: &Path, dir: &Path, template: Option<&str>) -> std::io::Result<()> {
    if let Some(template) = template.filter(|t| !t.trim().is_empty()) {
        let parts: Vec<String> = template
            .split_whitespace()
            .map(|p| {
                p.replace("{dir}", &dir.to_string_lossy())
                    .replace("{nu}", &nu.to_string_lossy())
            })
            .collect();
        let (program, args) = parts
            .split_first()
            .ok_or(std::io::ErrorKind::InvalidInput)?;
        return Command::new(program)
            .args(args)
            .current_dir(dir)
            .spawn()
            .map(drop);
    }
    open_default_terminal(nu, dir)
}

#[cfg(target_os = "macos")]
fn open_default_terminal(nu: &Path, dir: &Path) -> std::io::Result<()> {
    // A `.command` file opens in Terminal.app and runs with the folder as cwd.
    let script = std::env::temp_dir().join(format!("fffFinder-nu-{}.command", std::process::id()));
    let quote = |p: &Path| format!("'{}'", p.to_string_lossy().replace('\'', r"'\''"));
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\ncd {} || exit 1\nrm -f {}\nexec {} --login\n",
            quote(dir),
            quote(&script),
            quote(nu)
        ),
    )?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))?;
    Command::new("open")
        .arg("-a")
        .arg("Terminal")
        .arg(&script)
        .spawn()
        .map(drop)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_default_terminal(nu: &Path, dir: &Path) -> std::io::Result<()> {
    let nu = nu.to_string_lossy().into_owned();
    let dir_s = dir.to_string_lossy().into_owned();
    let candidates: Vec<(&str, Vec<String>)> = vec![
        (
            "wezterm",
            vec![
                "start".into(),
                "--cwd".into(),
                dir_s.clone(),
                "--".into(),
                nu.clone(),
            ],
        ),
        ("kitty", vec!["-d".into(), dir_s.clone(), nu.clone()]),
        (
            "alacritty",
            vec![
                "--working-directory".into(),
                dir_s.clone(),
                "-e".into(),
                nu.clone(),
            ],
        ),
        (
            "ghostty",
            vec![
                format!("--working-directory={dir_s}"),
                "-e".into(),
                nu.clone(),
            ],
        ),
        (
            "gnome-terminal",
            vec![
                format!("--working-directory={dir_s}"),
                "--".into(),
                nu.clone(),
            ],
        ),
        (
            "konsole",
            vec!["--workdir".into(), dir_s.clone(), "-e".into(), nu.clone()],
        ),
        (
            "xfce4-terminal",
            vec![
                format!("--working-directory={dir_s}"),
                "-x".into(),
                nu.clone(),
            ],
        ),
        ("x-terminal-emulator", vec!["-e".into(), nu.clone()]),
        ("xterm", vec!["-e".into(), nu.clone()]),
    ];
    let path = std::env::var_os("PATH").unwrap_or_default();
    for (program, args) in candidates {
        if std::env::split_paths(&path).any(|d| d.join(program).is_file()) {
            return Command::new(program)
                .args(args)
                .current_dir(dir)
                .spawn()
                .map(drop);
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "no terminal emulator found; set one in Settings",
    ))
}

#[cfg(windows)]
fn open_default_terminal(nu: &Path, dir: &Path) -> std::io::Result<()> {
    // Windows Terminal when available, else a new console window.
    let wt = Command::new("wt.exe")
        .arg("-d")
        .arg(dir)
        .arg(nu)
        .current_dir(dir)
        .spawn();
    if wt.is_ok() {
        return Ok(());
    }
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    Command::new(nu)
        .current_dir(dir)
        .creation_flags(CREATE_NEW_CONSOLE)
        .spawn()
        .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

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
