//! Context-menu actions written in Nushell: the commands a user's
//! `actions.nu` module exports, run with the selected items as `$in`.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use fffinder_core::config;
use serde::Deserialize;

/// The starters written to `actions.nu` the first time.
pub const STARTERS: &str = include_str!("../assets/actions.nu");

/// Fingerprints of the starters earlier versions wrote. A file that still
/// matches one was never edited, so it gets the current starters.
const PREVIOUS_STARTERS: [u64; 1] = [0xb29c_c005_39a0_8637];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserAction {
    /// The command, as called: `count lines`.
    pub command: String,
    /// The menu label: `Count Lines`.
    pub label: String,
    pub description: String,
}

pub fn actions_path() -> PathBuf {
    config::config_dir().join("actions.nu")
}

/// Write the starter actions unless the file exists, and update starters an
/// earlier version wrote if they were never edited. Returns whether it wrote.
pub fn ensure_starters(path: &Path) -> std::io::Result<bool> {
    ensure(path, &PREVIOUS_STARTERS)
}

fn ensure(path: &Path, previous: &[u64]) -> std::io::Result<bool> {
    match std::fs::read_to_string(path) {
        Ok(text) if previous.contains(&fingerprint(&text)) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
        }
        // The user's own actions, or a file that can't be read: leave it.
        _ => return Ok(false),
    }
    std::fs::write(path, STARTERS)?;
    Ok(true)
}

/// FNV-1a of `text` with Windows line endings as `\n`, so a checkout with
/// either kind matches.
fn fingerprint(text: &str) -> u64 {
    text.replace("\r\n", "\n")
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
}

pub fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// A Nushell string literal for `text`.
fn quote(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| format!("{text:?}"))
}

/// The commands `path` exports, in file order.
pub fn discover(nu: &Path, path: &Path) -> Result<Vec<UserAction>, String> {
    let module = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let script = format!(
        "use {path}\n\
         let ids = scope modules | where name == {module} | get 0.commands.decl_id\n\
         scope commands | where decl_id in $ids | select name description decl_id \
         | sort-by decl_id | to json --raw",
        path = quote(&path.to_string_lossy()),
        module = quote(&module),
    );
    #[derive(Deserialize)]
    struct Row {
        name: String,
        description: String,
    }
    let dir = path.parent().unwrap_or(Path::new("."));
    let out = crate::nushell::run_raw(nu, &script, dir)?;
    let rows: Vec<Row> = serde_json::from_str(out.trim()).map_err(|e| e.to_string())?;
    let prefix = format!("{module} ");
    Ok(rows
        .into_iter()
        .map(|row| {
            let command = row
                .name
                .strip_prefix(&prefix)
                .unwrap_or(&row.name)
                .to_string();
            UserAction {
                label: title_case(&command),
                command,
                description: row.description,
            }
        })
        .collect())
}

fn title_case(command: &str) -> String {
    command
        .split([' ', '-', '_'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            chars
                .next()
                .map(|c| c.to_uppercase().chain(chars).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The pipeline that runs `action` from `path`, as shown in the panel.
pub fn invocation(path: &Path, action: &UserAction) -> String {
    // `use` is a statement of its own, so the items are passed on with `$in`.
    format!(
        "use {} *; $in | {}",
        quote(&path.to_string_lossy()),
        action.command
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_read_like_menu_items() {
        assert_eq!(title_case("count lines"), "Count Lines");
        assert_eq!(title_case("find-duplicates"), "Find Duplicates");
        assert_eq!(title_case("archive"), "Archive");
    }

    /// Runs only where `nu` is installed.
    #[test]
    fn finds_the_starters() {
        let Some(nu) = crate::nushell::find_nu(None) else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("fff-actions-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("actions.nu");
        assert!(ensure_starters(&path).unwrap());
        assert!(!ensure_starters(&path).unwrap(), "an existing file is kept");
        let actions = discover(&nu, &path).unwrap();
        let labels: Vec<&str> = actions.iter().map(|a| a.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Count Lines",
                "Total Size",
                "Markdown Links",
                "Find Duplicates",
                "Archive"
            ]
        );
        assert_eq!(actions[0].command, "count lines");
        assert!(actions[0].description.starts_with("Count the lines"));
        std::fs::write(&path, "export def broken [ {").unwrap();
        assert!(discover(&nu, &path).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unedited_old_starters_are_updated() {
        let dir = std::env::temp_dir().join(format!("fff-upgrade-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("actions.nu");
        let old = "# old starters\nexport def x [] {}\n";
        let previous = [fingerprint(old)];

        std::fs::write(&path, old.replace('\n', "\r\n")).unwrap();
        assert!(ensure(&path, &previous).unwrap(), "unedited, so updated");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), STARTERS);
        assert!(!ensure(&path, &previous).unwrap(), "current starters stay");

        let edited = format!("{old}export def mine [] {{}}\n");
        std::fs::write(&path, &edited).unwrap();
        assert!(!ensure(&path, &previous).unwrap(), "edits are kept");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), edited);

        // Otherwise every launch would rewrite the file and reload it.
        assert!(!PREVIOUS_STARTERS.contains(&fingerprint(STARTERS)));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Every starter, run the way the menu runs it, on the selections that
    /// broke earlier versions. Runs only where `nu` is installed.
    #[test]
    fn starters_run() {
        use crate::nushell::NuOutput;
        use fffinder_core::FileEntry;
        let Some(nu) = crate::nushell::find_nu(None) else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("fff-starters-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for folder in ["one", "two", "Photos [2024]"] {
            std::fs::create_dir_all(dir.join(folder)).unwrap();
        }
        let files: [(&str, &[u8]); 7] = [
            ("one/a.txt", b"one\ntwo\n"),
            ("two/b.txt", b"one\ntwo\n"),
            ("one/-dash.txt", b"three\n"),
            ("one/notes (1) [draft]#2.md", b"# hi\nthere\nmore"),
            ("one/image.bin", b"PNG\0\0\x01\n\n\n"),
            ("Photos [2024]/c.txt", b"one\ntwo\n"),
            ("Photos [2024]/empty.txt", b""),
        ];
        for (name, bytes) in files {
            std::fs::write(dir.join(name), bytes).unwrap();
        }
        std::fs::write(dir.join("Photos [2024]/empty2.txt"), b"").unwrap();
        let path = dir.join("actions.nu");
        ensure_starters(&path).unwrap();
        let actions = discover(&nu, &path).unwrap();
        let run = |command: &str, names: &[&str]| {
            let entries: Vec<FileEntry> = names
                .iter()
                .map(|n| FileEntry::from_path(dir.join(n)).unwrap())
                .collect();
            let action = actions.iter().find(|a| a.command == command).unwrap();
            let run = crate::nushell::run_pipeline(&nu, &invocation(&path, action), &entries, &dir);
            assert_eq!(run.error, None, "{command} on {names:?}");
            run.output
        };
        let table = |output: NuOutput| match output {
            NuOutput::Table { columns, rows } => (columns, rows),
            other => panic!("expected a table, got {other:?}"),
        };
        let column = |output: NuOutput, name: &str| {
            let (columns, rows) = table(output);
            let ix = columns.iter().position(|c| c == name).unwrap();
            rows.into_iter().map(|r| r[ix].clone()).collect::<Vec<_>>()
        };

        // Binary files and folders have no lines.
        let (_, rows) = table(run(
            "count lines",
            &[
                "one/a.txt",
                "one/notes (1) [draft]#2.md",
                "one/image.bin",
                "one",
            ],
        ));
        assert_eq!(
            rows,
            [["notes (1) [draft]#2.md", "3"], ["a.txt", "2"]].map(|r| r.map(String::from))
        );

        // Only folders, and nothing at all, add up to zero.
        let (_, rows) = table(run("total size", &["one", "two"]));
        assert_eq!(rows[0], ["files", "0"]);
        assert_eq!(rows[1], ["size", "0 B"]);
        assert_eq!(rows[2], ["folders", "2"]);
        let (_, rows) = table(run("total size", &["one/a.txt", "two/b.txt"]));
        assert_eq!(rows[1], ["size", "16 B"]);

        let NuOutput::Text(links) = run("markdown links", &["one/notes (1) [draft]#2.md"]) else {
            panic!("expected text");
        };
        // Windows paths become file:///C:/Users/..., with forward slashes.
        let mut url = dir
            .join("one/notes%20%281%29%20%5Bdraft%5D%232.md")
            .display()
            .to_string()
            .replace('\\', "/");
        if !url.starts_with('/') {
            url.insert(0, '/');
        }
        assert_eq!(
            links,
            format!(r"- [notes (1) \[draft\]#2.md](file://{url})")
        );

        // A [ ] folder is searched, and empty files aren't duplicates.
        let paths = column(
            run("find duplicates", &["Photos [2024]", "one/a.txt", "two"]),
            "path",
        );
        let mut names: Vec<&str> = paths
            .iter()
            .filter_map(|p| p.rsplit(['/', '\\']).next())
            .collect();
        names.sort();
        assert_eq!(names, ["a.txt", "b.txt", "c.txt"]);

        // Items from two folders, one named like an option.
        let archive = column(run("archive", &["one/-dash.txt", "two/b.txt"]), "value");
        let listing = std::process::Command::new("tar")
            .arg("-tzf")
            .arg(&archive[0])
            .output()
            .unwrap();
        let listing = String::from_utf8_lossy(&listing.stdout);
        assert!(
            listing.contains("-dash.txt") && listing.contains("b.txt"),
            "{listing}"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
