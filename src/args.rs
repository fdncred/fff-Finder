//! Command line: `fffFinder [PATH] [--query TEXT] [--pick] [--multiple]`.
//!
//! `--pick` turns the window into a chooser for scripts (used by `fff pick`):
//! choosing prints a JSON array of absolute paths and exits 0; cancelling exits 1.

use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Args {
    pub path: Option<PathBuf>,
    pub query: Option<String>,
    pub pick: bool,
    pub multiple: bool,
}

pub const USAGE: &str = "\
fffFinder - fast Finder-style file search

Usage: fffFinder [PATH] [OPTIONS]

Arguments:
  [PATH]             Folder to open (default: last folder or home)

Options:
  -q, --query TEXT   Start with this search
      --pick         Choose files and print them as JSON to stdout
      --multiple     With --pick, allow choosing several items
  -h, --help         Show this help
  -V, --version      Show the version
";

#[derive(Debug, PartialEq)]
pub enum Parsed {
    Run(Args),
    Help,
    Version,
}

pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Parsed, String> {
    let mut out = Args::default();
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        let text = arg.to_string_lossy();
        match text.as_ref() {
            "-h" | "--help" => return Ok(Parsed::Help),
            "-V" | "--version" => return Ok(Parsed::Version),
            "--pick" => out.pick = true,
            "--multiple" | "-m" => out.multiple = true,
            "-q" | "--query" => {
                let value = iter.next().ok_or("--query needs a value")?;
                out.query = Some(value.to_string_lossy().into_owned());
            }
            _ if text.starts_with("--query=") => {
                out.query = Some(text["--query=".len()..].to_string());
            }
            // macOS passes a process serial number when launched from Finder.
            _ if text.starts_with("-psn_") => {}
            _ if text.starts_with('-') && text.len() > 1 => {
                return Err(format!("unknown option: {text}"));
            }
            _ => {
                if out.path.is_some() {
                    return Err(format!("unexpected argument: {text}"));
                }
                out.path = Some(PathBuf::from(arg));
            }
        }
    }
    Ok(Parsed::Run(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> Result<Parsed, String> {
        parse(args.iter().map(OsString::from))
    }

    #[test]
    fn parses_pick_contract() {
        let parsed = run(&["--pick", "/tmp", "--multiple", "--query", "main rs"]).unwrap();
        assert_eq!(
            parsed,
            Parsed::Run(Args {
                path: Some("/tmp".into()),
                query: Some("main rs".into()),
                pick: true,
                multiple: true,
            })
        );
    }

    #[test]
    fn rejects_unknown() {
        assert!(run(&["--nope"]).is_err());
        assert!(run(&["a", "b"]).is_err());
        assert!(run(&["--query"]).is_err());
        assert_eq!(run(&["-h"]).unwrap(), Parsed::Help);
        assert_eq!(run(&["-psn_0_123"]).unwrap(), Parsed::Run(Args::default()));
    }
}
