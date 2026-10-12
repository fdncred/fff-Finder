//! Settings shared by the GUI and the Nushell plugin, stored as JSON in the
//! platform config directory (`~/Library/Application Support/fffFinder` on
//! macOS, `~/.config/fffFinder` on Linux, `%APPDATA%\fffFinder` on Windows).

use crate::kind::KindFilter;
use crate::listing::SortOrder;
use crate::terminals::{CUSTOM, Platform, TerminalSetting};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const APP_NAME: &str = "fffFinder";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ViewMode {
    #[default]
    Icons,
    List,
    Columns,
    Gallery,
}

impl ViewMode {
    pub const ALL: [ViewMode; 4] = [
        ViewMode::Icons,
        ViewMode::List,
        ViewMode::Columns,
        ViewMode::Gallery,
    ];

    pub fn title(self) -> &'static str {
        match self {
            ViewMode::Icons => "Icons",
            ViewMode::List => "List",
            ViewMode::Columns => "Columns",
            ViewMode::Gallery => "Gallery",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Folders indexed for "This Computer" searches.
    pub search_roots: Vec<PathBuf>,
    /// Sidebar favorites, in display order.
    pub favorites: Vec<PathBuf>,
    pub show_hidden: bool,
    pub view_mode: ViewMode,
    pub sort: SortOrder,
    pub icon_size: f32,
    pub show_sidebar: bool,
    pub appearance: Appearance,
    pub kind_filter: KindFilter,
    /// Maximum results shown for a search.
    pub result_limit: usize,
    /// The terminal "Open Nushell Here" uses, per operating system (`macos`,
    /// `linux`, `windows`), so one settings file can carry a choice for each.
    /// A missing entry picks one automatically.
    pub terminals: BTreeMap<String, TerminalSetting>,
    /// The older single terminal command; moved into `terminals` on load.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal: Option<String>,
    /// Path to `nu`; `None` searches `PATH`.
    pub nu_path: Option<PathBuf>,
    pub last_folder: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            search_roots: default_search_roots(),
            favorites: crate::places::default_favorites()
                .into_iter()
                .map(|p| p.path)
                .collect(),
            show_hidden: false,
            view_mode: ViewMode::Icons,
            sort: SortOrder::default(),
            icon_size: 64.0,
            show_sidebar: true,
            appearance: Appearance::System,
            kind_filter: KindFilter::Any,
            result_limit: 500,
            terminals: BTreeMap::new(),
            terminal: None,
            nu_path: None,
            last_folder: None,
        }
    }
}

/// Home, plus the system application folder on macOS.
pub fn default_search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = dirs::home_dir() {
        roots.push(home);
    }
    #[cfg(target_os = "macos")]
    roots.push(PathBuf::from("/Applications"));
    roots
}

/// Settings directory; `FFFFINDER_CONFIG_DIR` overrides it (used by tests).
pub fn config_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("FFFFINDER_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    dirs::config_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(APP_NAME)
}

/// Where frecency and query-history databases live.
pub fn data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(APP_NAME)
}

pub fn config_path() -> PathBuf {
    config_dir().join("settings.json")
}

impl Config {
    /// Load settings; missing or malformed files fall back to defaults.
    pub fn load() -> Config {
        Self::load_from(&config_path())
    }

    pub fn load_from(path: &Path) -> Config {
        let mut config: Config = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        if let Some(command) = config.terminal.take().filter(|c| !c.trim().is_empty()) {
            config
                .terminals
                .entry(Platform::current().key().into())
                .or_insert(TerminalSetting {
                    app: Some(CUSTOM.into()),
                    command: Some(command),
                });
        }
        config
    }

    /// This operating system's terminal setting.
    pub fn terminal_setting(&self) -> TerminalSetting {
        self.terminals
            .get(Platform::current().key())
            .cloned()
            .unwrap_or_default()
    }

    pub fn set_terminal_setting(&mut self, setting: TerminalSetting) {
        let key = Platform::current().key().to_string();
        if setting == TerminalSetting::default() {
            self.terminals.remove(&key);
        } else {
            self.terminals.insert(key, setting);
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&config_path())
    }

    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        // Write then rename, so a crash never leaves a truncated file.
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let path = std::env::temp_dir().join(format!("fff-finder-cfg-{}.json", std::process::id()));
        let mut cfg = Config {
            show_hidden: true,
            view_mode: ViewMode::Gallery,
            ..Default::default()
        };
        cfg.terminals.insert(
            "windows".into(),
            TerminalSetting {
                app: Some("windows-terminal".into()),
                command: None,
            },
        );
        cfg.save_to(&path).unwrap();
        assert_eq!(Config::load_from(&path), cfg);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn keeps_a_terminal_per_system_and_moves_the_old_command() {
        let path =
            std::env::temp_dir().join(format!("fff-finder-cfg3-{}.json", std::process::id()));
        std::fs::write(
            &path,
            r#"{"terminal": "foot -D {dir} {nu}", "terminals": {"plan9": {"app": "rio"}}}"#,
        )
        .unwrap();
        let mut cfg = Config::load_from(&path);
        assert_eq!(cfg.terminal, None);
        assert_eq!(
            cfg.terminal_setting(),
            TerminalSetting {
                app: Some(CUSTOM.into()),
                command: Some("foot -D {dir} {nu}".into())
            }
        );
        cfg.set_terminal_setting(TerminalSetting {
            app: Some("ghostty".into()),
            command: Some("foot -D {dir} {nu}".into()),
        });
        cfg.save_to(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("\"terminal\""), "{text}");
        assert!(text.contains("\"plan9\""), "other systems are kept: {text}");
        assert_eq!(
            Config::load_from(&path).terminal_setting().app.as_deref(),
            Some("ghostty")
        );
        cfg.set_terminal_setting(TerminalSetting::default());
        assert!(!cfg.terminals.contains_key(Platform::current().key()));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn tolerates_partial_files() {
        let path =
            std::env::temp_dir().join(format!("fff-finder-cfg2-{}.json", std::process::id()));
        std::fs::write(&path, r#"{"show_hidden": true}"#).unwrap();
        let cfg = Config::load_from(&path);
        assert!(cfg.show_hidden);
        assert_eq!(cfg.result_limit, 500);
        std::fs::remove_file(path).unwrap();
    }
}
