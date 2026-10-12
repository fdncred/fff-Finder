//! Dialogs: settings, Go to Folder, rename, Move to Trash, and search syntax.

use std::path::{Path, PathBuf};

use fff_finder_core::{Appearance, FileEntry, config};
use gpui_kit::component::button::{Button, ButtonGroup, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::dialog::{DialogClose, DialogFooter};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Selectable as _, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{AppState, Finder, group_digits};
use crate::actions::*;
use crate::model::Location;

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
        let terminal = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(default_terminal_hint())
                .default_value(self.config.terminal.clone().unwrap_or_default())
        });
        self.dialog_sub = Some(
            cx.subscribe(&terminal, |this, input, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    let value = input.read(cx).value().trim().to_string();
                    this.config.terminal = (!value.is_empty()).then_some(value);
                    this.schedule_save(cx);
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
                                        .on_click(|_, window, cx| window.dispatch_action(Box::new(Rescan), cx)),
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
                                        .on_change(|_, window, cx| window.dispatch_action(Box::new(ToggleHidden), cx)),
                                ),
                        )
                        .child(section("NUSHELL", cx))
                        .child(
                            div().text_size(px(12.)).child(match &nu {
                                Some(path) => format!("Using {}", path.display()),
                                None => "nu was not found on PATH or in the usual install locations.".to_string(),
                            }),
                        )
                        .child(
                            v_flex()
                                .gap(px(4.))
                                .child(div().text_size(px(12.)).text_color(muted).child(
                                    "Terminal command for “Open Nushell Here” ({dir} and {nu} are replaced). Leave empty for the default.",
                                ))
                                .child(Input::new(&terminal).small().font_family(theme.mono_font_family.clone())),
                        )
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
        let go = {
            let view = view.clone();
            let input = input.clone();
            move |window: &mut Window, cx: &mut App| {
                let text = input.read(cx).value().to_string();
                view.update(cx, |this, cx| {
                    let path = resolve_path(&text, this.current_folder());
                    if path.is_dir() {
                        this.navigate(Location::Folder(path), window, cx);
                    } else if path.exists() {
                        this.reveal_in_place(&path, window, cx);
                    } else {
                        window.push_notification(
                            (
                                gpui_kit::component::notification::NotificationType::Warning,
                                format!("{} doesn’t exist", path.display()),
                            ),
                            cx,
                        );
                        return;
                    }
                    window.close_dialog(cx);
                });
            }
        };
        let on_enter = go.clone();
        self.dialog_sub = Some(cx.subscribe_in(
            &input,
            window,
            move |_, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    on_enter(window, cx);
                }
            },
        ));
        let first = std::cell::Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            if first.replace(false) {
                input.update(cx, |s, cx| {
                    s.focus(window, cx);
                    s.select_all(window, cx);
                });
            }
            let go = go.clone();
            dialog
                .title("Go to Folder")
                .w(px(480.))
                .child(Input::new(&input).cleanable(true))
                .footer(
                    DialogFooter::new()
                        .child(DialogClose::new().child(Button::new("goto-cancel").label("Cancel")))
                        .child(
                            Button::new("goto-go")
                                .primary()
                                .label("Go")
                                .on_click(move |_, window, cx| go(window, cx)),
                        ),
                )
        });
    }

    pub(super) fn open_rename_dialog(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = fff_finder_core::entry::display_name(&path);
        let stem_len = if path.is_dir() {
            name.len()
        } else {
            name.rfind('.').filter(|&i| i > 0).unwrap_or(name.len())
        };
        let input = cx.new(|cx| InputState::new(window, cx).default_value(name.clone()));
        let view = cx.entity();
        let apply = {
            let view = view.clone();
            let input = input.clone();
            let path = path.clone();
            move |window: &mut Window, cx: &mut App| {
                let new_name = input.read(cx).value().trim().to_string();
                if new_name.is_empty() || new_name.contains(['/', '\\']) {
                    return;
                }
                let target = path.with_file_name(&new_name);
                if target == path {
                    window.close_dialog(cx);
                    return;
                }
                if target.exists() {
                    window.push_notification(
                        (
                            gpui_kit::component::notification::NotificationType::Warning,
                            format!("“{new_name}” already exists"),
                        ),
                        cx,
                    );
                    return;
                }
                match std::fs::rename(&path, &target) {
                    Ok(()) => {
                        view.update(cx, |this, cx| {
                            this.previews.remove(&path);
                            for row in this.results.iter_mut().filter(|r| r.entry.path == path) {
                                if let Some(entry) = FileEntry::from_path(&target) {
                                    *row = super::Row::new(entry, Vec::new(), None);
                                }
                            }
                            this.load_listing(Some(target.clone()), true, cx);
                        });
                        window.close_dialog(cx);
                    }
                    Err(err) => window.push_notification(
                        (
                            gpui_kit::component::notification::NotificationType::Error,
                            format!("Couldn’t rename: {err}"),
                        ),
                        cx,
                    ),
                }
            }
        };
        let on_enter = apply.clone();
        self.dialog_sub = Some(cx.subscribe_in(
            &input,
            window,
            move |_, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    on_enter(window, cx);
                }
            },
        ));
        let first = std::cell::Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            if first.replace(false) {
                input.update(cx, |s, cx| {
                    s.focus(window, cx);
                    s.set_selected_range(0..stem_len, cx);
                });
            }
            let apply = apply.clone();
            dialog
                .title("Rename")
                .w(px(440.))
                .child(Input::new(&input))
                .footer(
                    DialogFooter::new()
                        .child(
                            DialogClose::new().child(Button::new("rename-cancel").label("Cancel")),
                        )
                        .child(
                            Button::new("rename-ok")
                                .primary()
                                .label("Rename")
                                .on_click(move |_, window, cx| apply(window, cx)),
                        ),
                )
        });
    }

    pub(super) fn confirm_trash(
        &mut self,
        entries: Vec<FileEntry>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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

fn default_terminal_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        "Terminal.app — e.g. open -na Ghostty --args --working-directory={dir} -e {nu}"
    } else if cfg!(windows) {
        "Windows Terminal — e.g. wt.exe -d {dir} {nu}"
    } else {
        "auto-detect — e.g. wezterm start --cwd {dir} -- {nu}"
    }
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
