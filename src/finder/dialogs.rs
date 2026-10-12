//! Dialogs: settings, Go to Folder, rename, Move to Trash, and search syntax.

use super::{AppState, Finder, group_digits};
use crate::actions::*;
use crate::model::Location;
use fff_finder_core::terminals::{self, CUSTOM, Platform, Terminal};
use fff_finder_core::{Appearance, FileEntry, config};
use gpui_kit::component::button::{Button, ButtonGroup, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::dialog::{DialogClose, DialogFooter};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Selectable as _, Sizable as _,
    WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::path::{Path, PathBuf};

/// Expand `~` and make relative paths relative to `base`.
fn resolve_path(text: &str, base: Option<&Path>) -> PathBuf {
    let text = text.trim();
    let expanded = if text == "~" {
        dirs::home_dir().unwrap_or_default()
    } else if let Some(rest) = text.strip_prefix("~/").or_else(|| text.strip_prefix("~\\")) {
        dirs::home_dir().unwrap_or_default().join(rest)
    } else {
        PathBuf::from(text)
    };
    if expanded.is_absolute() {
        expanded
    } else {
        base.map(|b| b.join(&expanded)).unwrap_or(expanded)
    }
}

impl Finder {
    /// A dialog returns focus to whatever had it when it opened. From a menu
    /// that is the menu's button, which leaves the keyboard dead in the file
    /// views, so focus the files first.
    pub(super) fn focus_before_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !window.has_focused_input(cx) {
            self.focus_files(window, cx);
        }
    }
}

fn section(title: &'static str, cx: &App) -> impl IntoElement {
    div()
        .pt(px(6.))
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.theme().muted_foreground)
        .child(title)
}

impl Finder {
    pub(super) fn show_settings(
        &mut self,
        _: &ShowSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_before_dialog(window, cx);
        // Pick up terminals installed since the last look.
        self.detect_terminals(cx);
        let terminal = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(custom_terminal_hint())
                .default_value(self.terminal_setting().command.unwrap_or_default())
        });
        self.dialog_sub = Some(
            cx.subscribe(&terminal, |this, input, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    let value = input.read(cx).value().trim().to_string();
                    this.set_terminal_command(value, cx);
                }
            }),
        );
        let view = cx.entity();
        window.open_dialog(cx, move |dialog, window, cx| {
            let finder = view.read(cx);
            let theme = cx.theme();
            let muted = theme.muted_foreground;
            let border = theme.border;
            let roots = finder.config.search_roots.clone();
            let status = finder.index_status.clone();
            let appearance = finder.config.appearance;
            let hidden = finder.config.show_hidden;
            let nu = cx.global::<AppState>().nu.clone();
            let plugin = std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|d| d.join(if cfg!(windows) { "nu_plugin_fff.exe" } else { "nu_plugin_fff" })))
                .filter(|p| p.is_file());

            let mut root_list = v_flex()
                .border_1()
                .border_color(border)
                .rounded(px(8.))
                .overflow_hidden();
            for (ix, root) in roots.iter().enumerate() {
                let files = status.iter().find(|s| &s.path == root).map(|s| {
                    if s.scanning {
                        format!("indexing… {}", group_digits(s.indexed_files))
                    } else {
                        format!("{} files", group_digits(s.indexed_files))
                    }
                });
                let remove = root.clone();
                root_list = root_list.child(
                    h_flex()
                        .px(px(10.))
                        .py(px(6.))
                        .gap(px(8.))
                        .when(ix > 0, |this| this.border_t_1().border_color(border))
                        .child(Icon::new(IconName::Folder).with_size(px(15.)).text_color(theme.primary))
                        .child(div().flex_1().truncate().text_size(px(12.)).child(super::tildify(root)))
                        .when_some(files, |this, f| this.child(div().text_size(px(11.)).text_color(muted).child(f)))
                        .child(
                            Button::new(("remove-root", ix))
                                .ghost()
                                .xsmall()
                                .icon(IconName::Close)
                                .tooltip("Stop indexing this folder")
                                .on_click(window.listener_for(&view, move |this: &mut Finder, _: &ClickEvent, _, cx| {
                                    let roots: Vec<PathBuf> =
                                        this.config.search_roots.iter().filter(|r| **r != remove).cloned().collect();
                                    this.set_search_roots(roots, cx);
                                })),
                        ),
                );
            }
            if roots.is_empty() {
                root_list = root_list.child(
                    div()
                        .p(px(10.))
                        .text_size(px(12.))
                        .text_color(muted)
                        .child("No folders are indexed. “This Computer” searches return nothing."),
                );
            }

            let disk_root = if cfg!(windows) { PathBuf::from("C:\\") } else { PathBuf::from("/") };
            let has_disk = roots.contains(&disk_root);

            dialog
                .title("Settings")
                .w(px(600.))
                .child(
                    v_flex()
                        .gap(px(10.))
                        .child(section("SEARCH LOCATIONS", cx))
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(muted)
                                .child("“This Computer” searches these folders. Indexes stay live while fffFinder runs."),
                        )
                        .child(root_list)
                        .child(
                            h_flex()
                                .gap(px(8.))
                                .child(Button::new("add-root").small().label("Add Folder…").on_click(
                                    window.listener_for(&view, |this: &mut Finder, _: &ClickEvent, _, cx| {
                                        this.prompt_add_root(cx)
                                    }),
                                ))
                                .when(!has_disk, |this| {
                                    this.child(
                                        Button::new("add-disk")
                                            .small()
                                            .label("Index Entire Disk")
                                            .tooltip("Search everything; the first scan takes longer and uses more memory")
                                            .on_click(window.listener_for(&view, move |this: &mut Finder, _: &ClickEvent, _, cx| {
                                                this.set_search_roots(vec![disk_root.clone()], cx)
                                            })),
                                    )
                                })
                                .child(Button::new("reset-roots").small().ghost().label("Reset").on_click(
                                    window.listener_for(&view, |this: &mut Finder, _: &ClickEvent, _, cx| {
                                        this.set_search_roots(config::default_search_roots(), cx)
                                    }),
                                ))
                                .child(div().flex_1())
                                .child(
                                    Button::new("rescan")
                                        .small()
                                        .icon(IconName::RefreshCw)
                                        .label("Rescan")
                                        .on_click(window.listener_for(&view, |this: &mut Finder, _: &ClickEvent, window, cx| {
                                            this.rescan(&Rescan, window, cx)
                                        })),
                                ),
                        )
                        .child(section("APPEARANCE", cx))
                        .child(
                            h_flex()
                                .gap(px(12.))
                                .child(
                                    ButtonGroup::new("appearance")
                                        .small()
                                        .outline()
                                        .child(Button::new("ap-system").label("System").selected(appearance == Appearance::System))
                                        .child(Button::new("ap-light").label("Light").selected(appearance == Appearance::Light))
                                        .child(Button::new("ap-dark").label("Dark").selected(appearance == Appearance::Dark))
                                        .on_click(window.listener_for(&view, |this: &mut Finder, ixs: &Vec<usize>, window, cx| {
                                            let appearance = match ixs.first() {
                                                Some(1) => Appearance::Light,
                                                Some(2) => Appearance::Dark,
                                                _ => Appearance::System,
                                            };
                                            this.set_appearance(appearance, window, cx);
                                        })),
                                )
                                .child(
                                    Switch::new("show-hidden")
                                        .small()
                                        .label("Show hidden files")
                                        .checked(hidden)
                                        // The dialog sits outside the Finder view, so actions
                                        // dispatched from here never reach it; call it directly.
                                        .on_change(window.listener_for(&view, |this: &mut Finder, show: &bool, _, cx| {
                                            this.set_show_hidden(*show, cx)
                                        })),
                                ),
                        )
                        .child(section("NUSHELL", cx))
                        .child(
                            div().text_size(px(12.)).child(match &nu {
                                Some(path) => format!("Using {}", path.display()),
                                None => "nu was not found on PATH or in the usual install locations.".to_string(),
                            }),
                        )
                        .child(terminal_controls(finder, &view, &terminal, window, cx))
                        .child(
                            div()
                                .p(px(10.))
                                .rounded(px(8.))
                                .bg(theme.muted)
                                .text_size(px(11.5))
                                .font_family(theme.mono_font_family.clone())
                                .child(match &plugin {
                                    Some(p) => format!("plugin add {}\nplugin use fff\nfff --all budget --kind document", p.display()),
                                    None => "nu scripts/install.nu   # installs fffFinder + nu_plugin_fff".to_string(),
                                }),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(muted)
                                .child(format!("Settings file: {}", config::config_path().display())),
                        ),
                )
                .footer(
                    DialogFooter::new().child(DialogClose::new().child(Button::new("settings-done").primary().label("Done"))),
                )
        });
    }

    fn prompt_add_root(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Index".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                this.update(cx, |this, cx| {
                    let mut roots = this.config.search_roots.clone();
                    roots.extend(paths);
                    roots.dedup();
                    this.set_search_roots(roots, cx);
                })
                .ok();
            }
        })
        .detach();
    }

    pub(super) fn go_to_folder(
        &mut self,
        _: &GoToFolder,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_before_dialog(window, cx);
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("~/Documents, /usr/local, C:\\Users…")
                .default_value(
                    self.current_folder()
                        .map(super::tildify)
                        .unwrap_or_default(),
                )
        });
        let view = cx.entity();
        let first = std::cell::Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            if first.replace(false) {
                input.update(cx, |s, cx| {
                    s.focus(window, cx);
                    s.select_all(window, cx);
                });
            }
            let (ok_input, go_input) = (input.clone(), input.clone());
            let ok_view = view.clone();
            dialog
                .title("Go to Folder")
                .w(px(480.))
                .child(Input::new(&input).id("goto-input").cleanable(true))
                // Return confirms the dialog; it closes only when the path exists.
                .on_ok(move |_, window, cx| {
                    ok_view.update(cx, |this, cx| this.submit_go_to(&ok_input, window, cx))
                })
                .footer(
                    DialogFooter::new()
                        .child(DialogClose::new().child(Button::new("goto-cancel").label("Cancel")))
                        .child(Button::new("goto-go").primary().label("Go").on_click(
                            window.listener_for(
                                &view,
                                move |this: &mut Finder, _: &ClickEvent, window, cx| {
                                    if this.submit_go_to(&go_input, window, cx) {
                                        window.close_dialog(cx);
                                        this.focus_files(window, cx);
                                    }
                                },
                            ),
                        )),
                )
        });
    }

    /// Open the folder (or reveal the file) typed in Go to Folder. Returns
    /// whether the dialog should close.
    fn submit_go_to(
        &mut self,
        input: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let text = input.read(cx).value().to_string();
        let path = resolve_path(&text, self.current_folder());
        if path.is_dir() {
            self.navigate(Location::Folder(path), window, cx);
        } else if path.exists() {
            self.reveal_in_place(&path, window, cx);
        } else {
            window.push_notification(
                (
                    gpui_kit::component::notification::NotificationType::Warning,
                    format!("{} doesn’t exist", path.display()),
                ),
                cx,
            );
            return false;
        }
        true
    }

    pub(super) fn open_rename_dialog(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_before_dialog(window, cx);
        let name = fff_finder_core::entry::display_name(&path);
        let stem_len = if path.is_dir() {
            name.len()
        } else {
            name.rfind('.').filter(|&i| i > 0).unwrap_or(name.len())
        };
        let input = cx.new(|cx| InputState::new(window, cx).default_value(name.clone()));
        let view = cx.entity();
        let first = std::cell::Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            if first.replace(false) {
                input.update(cx, |s, cx| {
                    s.focus(window, cx);
                    s.set_selected_range(0..stem_len, cx);
                });
            }
            let (ok_input, button_input) = (input.clone(), input.clone());
            let (ok_path, button_path) = (path.clone(), path.clone());
            let ok_view = view.clone();
            dialog
                .title("Rename")
                .w(px(440.))
                .child(Input::new(&input).id("rename-input"))
                .on_ok(move |_, window, cx| {
                    ok_view.update(cx, |this, cx| {
                        this.submit_rename(&ok_path, &ok_input, window, cx)
                    })
                })
                .footer(
                    DialogFooter::new()
                        .child(
                            DialogClose::new().child(Button::new("rename-cancel").label("Cancel")),
                        )
                        .child(Button::new("rename-ok").primary().label("Rename").on_click(
                            window.listener_for(
                                &view,
                                move |this: &mut Finder, _: &ClickEvent, window, cx| {
                                    if this.submit_rename(&button_path, &button_input, window, cx) {
                                        window.close_dialog(cx);
                                        this.focus_files(window, cx);
                                    }
                                },
                            ),
                        )),
                )
        });
    }

    /// Rename `path` to the typed name. Returns whether the dialog should close.
    fn submit_rename(
        &mut self,
        path: &Path,
        input: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let new_name = input.read(cx).value().trim().to_string();
        let mut notify = |kind, message: String| {
            window.push_notification((kind, message), cx);
        };
        use gpui_kit::component::notification::NotificationType::{Error, Warning};
        if new_name.is_empty() {
            return false;
        }
        if new_name.contains(['/', '\\']) {
            notify(Warning, "A name can’t contain “/” or “\\”".into());
            return false;
        }
        let target = path.with_file_name(&new_name);
        if target == path {
            return true;
        }
        if target.exists() {
            notify(Warning, format!("“{new_name}” already exists"));
            return false;
        }
        if let Err(err) = std::fs::rename(path, &target) {
            notify(Error, format!("Couldn’t rename: {err}"));
            return false;
        }
        self.previews.remove(path);
        for row in self.results.iter_mut().filter(|r| r.entry.path == path) {
            if let Some(entry) = FileEntry::from_path(&target) {
                *row = super::Row::new(entry, Vec::new(), None);
            }
        }
        self.load_listing(Some(target), true, cx);
        true
    }

    pub(super) fn confirm_trash(
        &mut self,
        entries: Vec<FileEntry>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_before_dialog(window, cx);
        let count = entries.len();
        let title = if count == 1 {
            format!("Move “{}” to the Trash?", entries[0].name)
        } else {
            format!("Move {count} items to the Trash?")
        };
        let paths: Vec<PathBuf> = entries.into_iter().map(|e| e.path).collect();
        let view = cx.entity();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let paths = paths.clone();
            let view = view.clone();
            alert
                .confirm()
                .title(title.clone())
                .description("You can restore items from the Trash.")
                .ok_text("Move to Trash")
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, _, cx| {
                    view.update(cx, |this, cx| this.trash_now(paths.clone(), cx));
                    true
                })
        });
    }

    pub(super) fn search_help(
        &mut self,
        _: &SearchHelp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_before_dialog(window, cx);
        const ROWS: [(&str, &str); 10] = [
            (
                "budget report",
                "Fuzzy words, typo tolerant; every word must match",
            ),
            ("*.pdf  *.md", "Extensions (several are ORed)"),
            (
                "src/**/*.rs",
                "Glob against the path inside the search location",
            ),
            ("/notes/ todo", "Only paths inside a folder named “notes”"),
            ("main src/", "Folder filter written at the end"),
            (
                "git:modified",
                "Git status: modified, untracked, staged, clean",
            ),
            (
                "!*.log  !test",
                "Exclude an extension, glob, folder, or text",
            ),
            ("main.rs:42", "Line suffix, ignored for matching"),
            ("Kind menu", "Restrict to folders, documents, images, …"),
            ("Contents", "Search inside files instead of names"),
        ];
        window.open_dialog(cx, move |dialog, _, cx| {
            let theme = cx.theme();
            dialog
                .title("Search Syntax")
                .w(px(560.))
                .child(
                    v_flex()
                        .gap(px(2.))
                        .text_size(px(12.))
                        .children(ROWS.iter().map(|(syntax, meaning)| {
                            h_flex()
                                .gap(px(14.))
                                .py(px(3.))
                                .child(
                                    div()
                                        .w(px(150.))
                                        .font_family(theme.mono_font_family.clone())
                                        .text_color(theme.primary)
                                        .child(*syntax),
                                )
                                .child(div().flex_1().child(*meaning))
                        }))
                        .child(
                            div()
                                .pt(px(10.))
                                .text_color(theme.muted_foreground)
                                .child("Searches run 50 ms after you stop typing; Return searches now and Down Arrow moves into the results. Hidden files are only indexed inside git repositories."),
                        ),
                )
                .footer(DialogFooter::new().child(DialogClose::new().child(Button::new("help-done").primary().label("Done"))))
        });
    }
}

fn custom_terminal_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        "e.g. open -na WezTerm --args start --cwd {dir} -- {nu}"
    } else if cfg!(windows) {
        "e.g. wt.exe -d {dir} {nu}"
    } else {
        "e.g. foot --working-directory={dir} {nu}"
    }
}

/// The "Open Nushell Here" terminal: a menu of the terminals found, a button
/// to try it, and the command field when the choice is a custom command.
fn terminal_controls(
    finder: &Finder,
    view: &Entity<Finder>,
    input: &Entity<InputState>,
    window: &mut Window,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let setting = finder.terminal_setting();
    let found = finder.terminals();
    let env_terminal = std::env::var("TERMINAL").ok();
    let automatic = found.and_then(|f| terminals::automatic(f, env_terminal.as_deref()));
    let custom = setting.app.as_deref() == Some(CUSTOM);
    let chosen = setting.app.as_deref().filter(|id| *id != CUSTOM);
    let shown = match chosen {
        Some(id) => found.and_then(|f| f.iter().find(|t| t.id == id)),
        None => automatic,
    };
    let label: SharedString = if found.is_none() {
        "Looking for terminals…".into()
    } else if custom {
        "Custom Command".into()
    } else {
        match (chosen, shown) {
            (None, Some(t)) => format!("Automatic ({})", t.name).into(),
            (None, None) => "Automatic (none found)".into(),
            (Some(_), Some(t)) => t.name.clone().into(),
            (Some(id), None) => format!("{id} (not installed)").into(),
        }
    };
    let detail = if custom {
        "Runs the command below.".to_string()
    } else if let Some(t) = shown {
        t.location
            .as_deref()
            .map(super::tildify)
            .unwrap_or_else(|| "Opens nu in a new console window.".into())
    } else if chosen.is_some() {
        "Not installed here, so Automatic is used.".to_string()
    } else if found.is_some() {
        "No terminal was found; use a custom command.".to_string()
    } else {
        String::new()
    };
    let detail = format!("{detail} Saved for {}.", Platform::current().label());

    let menu_view = view.clone();
    let picker = Button::new("terminal-app")
        .small()
        .outline()
        .label(label)
        .dropdown_caret(true)
        .disabled(found.is_none())
        .dropdown_menu(move |menu, window, cx| {
            let finder = menu_view.read(cx);
            let current = finder.terminal_setting().app;
            let found = finder
                .terminals()
                .map(<[Terminal]>::to_vec)
                .unwrap_or_default();
            let env_terminal = std::env::var("TERMINAL").ok();
            let automatic = terminals::automatic(&found, env_terminal.as_deref())
                .map_or_else(|| "none found".to_string(), |t| t.name.clone());
            let pick = |app: Option<String>| {
                window.listener_for(
                    &menu_view,
                    move |this: &mut Finder, _: &ClickEvent, _, cx| {
                        this.set_terminal_app(app.clone(), cx)
                    },
                )
            };
            let mut menu = menu
                .item(
                    PopupMenuItem::new(format!("Automatic ({automatic})"))
                        .checked(current.is_none())
                        .on_click(pick(None)),
                )
                .separator();
            for t in &found {
                menu = menu.item(
                    PopupMenuItem::new(t.name.clone())
                        .checked(current.as_deref() == Some(t.id.as_str()))
                        .on_click(pick(Some(t.id.clone()))),
                );
            }
            menu.separator()
                .item(
                    PopupMenuItem::new("Custom Command")
                        .checked(current.as_deref() == Some(CUSTOM))
                        .on_click(pick(Some(CUSTOM.into()))),
                )
                .separator()
                .item(PopupMenuItem::new("Detect Again").on_click(
                    window.listener_for(&menu_view, |this: &mut Finder, _: &ClickEvent, _, cx| {
                        this.detect_terminals(cx)
                    }),
                ))
                .min_w(px(240.))
        });

    v_flex()
        .gap(px(6.))
        .child(
            h_flex()
                .gap(px(8.))
                .child(div().text_size(px(12.)).child("Open Nushell Here in"))
                .child(picker)
                .child(
                    Button::new("try-terminal")
                        .small()
                        .ghost()
                        .label("Try It")
                        .tooltip("Open Nushell in the current folder now")
                        .on_click(window.listener_for(
                            view,
                            |this: &mut Finder, _: &ClickEvent, window, cx| {
                                let dir = this
                                    .current_folder()
                                    .map(Path::to_path_buf)
                                    .or_else(dirs::home_dir);
                                if let Some(dir) = dir {
                                    this.open_terminal_at(&dir, window, cx);
                                }
                            },
                        )),
                ),
        )
        .child(div().text_size(px(11.)).text_color(muted).child(detail))
        .when(custom, |this| {
            this.child(
                v_flex()
                    .gap(px(4.))
                    .child(
                        Input::new(input)
                            .id("terminal-input")
                            .small()
                            .font_family(theme.mono_font_family.clone()),
                    )
                    .child(div().text_size(px(11.)).text_color(muted).child(
                        "{dir} becomes the folder and {nu} the path to nu. Quote words that contain spaces.",
                    )),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::resolve_path;
    use std::path::{Path, PathBuf};

    #[test]
    fn resolves_paths() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(resolve_path("~", None), home);
        assert_eq!(resolve_path("~/x", None), home.join("x"));
        assert_eq!(
            resolve_path("sub", Some(Path::new("/base"))),
            PathBuf::from("/base/sub")
        );
        assert_eq!(
            resolve_path(" /abs ", Some(Path::new("/base"))),
            PathBuf::from("/abs")
        );
    }
}
