//! File previews for the gallery, the column view, and Quick Look. Structured
//! files (CSV, JSON, TOML, YAML, SQLite, …) are opened with Nushell and shown
//! as tables.

use super::views::file_icon;
use super::{AppState, Finder};
use crate::actions::{
    PreviewTextBigger, PreviewTextReset, PreviewTextSmaller, QUICK_LOOK, QuickLook,
};
use crate::nushell::{self, NuOutput};
use crate::theme::palette;
use crate::theme::ui;
use fff_Finder_core::config::{DEFAULT_PREVIEW_FONT_SIZE, FONT_SIZE_RANGE};
use fff_Finder_core::{FileEntry, FileKind, format};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::io::Read as _;
use std::path::Path;

const TEXT_LIMIT: usize = 64 * 1024;
const TABLE_ROWS: usize = 200;

#[derive(Clone)]
pub enum Preview {
    Loading,
    Image,
    Text {
        text: SharedString,
        truncated: bool,
    },
    /// Source colored the way the user's Nushell colors it.
    Code {
        text: SharedString,
        spans: Vec<(std::ops::Range<usize>, HighlightStyle)>,
        /// Comments, which `nu-highlight` leaves plain; drawn in the theme's
        /// secondary text color.
        comments: Vec<std::ops::Range<usize>>,
        truncated: bool,
    },
    Table {
        columns: Vec<SharedString>,
        rows: Vec<Vec<SharedString>>,
        via_nu: bool,
    },
    Folder {
        items: usize,
    },
    Unavailable,
}

impl Preview {
    /// Whether the preview shows text, so its size can be changed.
    fn has_text(&self) -> bool {
        matches!(
            self,
            Preview::Text { .. } | Preview::Code { .. } | Preview::Table { .. }
        )
    }
}

fn load_preview(entry: &FileEntry, nu: Option<&Path>) -> Preview {
    if entry.is_dir {
        let items = std::fs::read_dir(&entry.path)
            .map(|d| d.count())
            .unwrap_or(0);
        return Preview::Folder { items };
    }
    let is_nu = entry
        .path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("nu"));
    if let Some(nu) = nu
        && is_nu
        && let Some((source, truncated)) = read_text(&entry.path)
        && let Ok(colored) = nushell::highlight_nu(nu, &source)
    {
        let (text, spans) = crate::ansi::parse(&colored);
        let styled: Vec<_> = spans.iter().map(|(range, _)| range.clone()).collect();
        return Preview::Code {
            comments: nushell::comment_ranges(&text, &styled),
            text: text.into(),
            spans: spans
                .into_iter()
                .map(|(range, style)| (range, style.to_highlight()))
                .collect(),
            truncated,
        };
    }
    if let Some(nu) = nu
        && nushell::is_structured(&entry.path)
        && entry.size.unwrap_or(0) < 50_000_000
    {
        let run = nushell::preview_file(nu, &entry.path);
        match run.output {
            NuOutput::Table { columns, rows } => {
                return Preview::Table {
                    columns: columns.into_iter().map(Into::into).collect(),
                    rows: rows
                        .into_iter()
                        .take(TABLE_ROWS)
                        .map(|r| r.into_iter().map(Into::into).collect())
                        .collect(),
                    via_nu: true,
                };
            }
            NuOutput::Text(text) => {
                return Preview::Text {
                    truncated: text.len() > TEXT_LIMIT,
                    text: text.chars().take(TEXT_LIMIT).collect::<String>().into(),
                };
            }
            NuOutput::Empty => {}
        }
    }
    if matches!(entry.kind, FileKind::Image) {
        return Preview::Image;
    }
    if matches!(
        entry.kind,
        FileKind::Text | FileKind::Code | FileKind::Data | FileKind::Other | FileKind::Executable
    ) && let Some((text, truncated)) = read_text(&entry.path)
    {
        return Preview::Text {
            text: text.into(),
            truncated,
        };
    }
    Preview::Unavailable
}

/// The start of a file as text, or `None` when it looks binary.
pub(super) fn read_text(path: &Path) -> Option<(String, bool)> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; TEXT_LIMIT + 1];
    let mut len = 0;
    while len < buf.len() {
        match file.read(&mut buf[len..]) {
            Ok(0) => break,
            Ok(n) => len += n,
            Err(_) => return None,
        }
    }
    buf.truncate(len);
    if buf.iter().take(8192).any(|&b| b == 0) {
        return None;
    }
    let truncated = buf.len() > TEXT_LIMIT;
    buf.truncate(TEXT_LIMIT);
    let text = String::from_utf8_lossy(&buf).into_owned();
    // Lossy decoding of a binary file produces many replacement characters.
    let bad = text.chars().take(4096).filter(|&c| c == '\u{FFFD}').count();
    (bad < 16).then_some((text, truncated))
}

impl Finder {
    /// Start loading the preview for `entry` unless it is cached or loading.
    pub fn ensure_preview(&mut self, entry: &FileEntry, cx: &mut Context<Self>) {
        let path = entry.path.clone();
        if self.previews.contains_key(&path) || self.preview_tasks.contains_key(&path) {
            return;
        }
        if self.previews.len() > 256 {
            self.previews.clear();
        }
        let nu = cx.global::<AppState>().nu.clone();
        let entry = entry.clone();
        let task = cx.spawn(async move |this, cx| {
            let preview = cx
                .background_spawn({
                    let entry = entry.clone();
                    async move { load_preview(&entry, nu.as_deref()) }
                })
                .await;
            this.update(cx, |this, cx| {
                this.preview_tasks.remove(&entry.path);
                this.previews.insert(entry.path.clone(), preview);
                cx.notify();
            })
            .ok();
            cx.refresh();
        });
        self.preview_tasks.insert(path, task);
    }

    pub fn preview_of(&self, entry: &FileEntry) -> Preview {
        self.previews
            .get(&entry.path)
            .cloned()
            .unwrap_or(Preview::Loading)
    }

    pub fn quick_look(&mut self, _: &QuickLook, window: &mut Window, cx: &mut Context<Self>) {
        if self.quick_look_open {
            self.quick_look_open = false;
            // The flag can outlive the dialog; only close what is open.
            if window.has_active_dialog(cx) {
                window.close_dialog(cx);
                return;
            }
        }
        let Some(entry) = self.primary_entry() else {
            return;
        };
        self.focus_before_dialog(window, cx);
        self.ensure_preview(&entry, cx);
        self.quick_look_open = true;
        let view = cx.entity();
        let focus = self.quick_look_focus.clone();
        let first = std::cell::Cell::new(true);
        window.open_dialog(cx, move |dialog, window, cx| {
            // Focus the preview so Space (bound in "QuickLook") closes it again.
            if first.replace(false) {
                focus.focus(window, cx);
            }
            let finder = view.read(cx);
            let Some(entry) = finder.primary_entry() else {
                return dialog;
            };
            let open_path = entry.path.clone();
            let open_view = view.clone();
            let close_view = view.clone();
            let text_size = finder
                .preview_of(&entry)
                .has_text()
                .then(|| finder.config.preview_font_size());
            dialog
                .title(
                    h_flex()
                        .gap(ui(8.))
                        .child(file_icon(&entry, ui(20.), false, &palette(cx)))
                        .child(div().truncate().child(entry.name.clone())),
                )
                .w(ui(860.))
                .child(
                    div()
                        .track_focus(&focus)
                        .key_context(crate::actions::QUICK_LOOK)
                        .on_action({
                            let view = view.clone();
                            move |_: &QuickLook, window, cx| {
                                window.close_dialog(cx);
                                view.update(cx, |this, _| this.quick_look_open = false);
                            }
                        })
                        .on_action(text_size_handler::<PreviewTextBigger>(&view, Some(1.)))
                        .on_action(text_size_handler::<PreviewTextSmaller>(&view, Some(-1.)))
                        .on_action(text_size_handler::<PreviewTextReset>(&view, None))
                        .h(ui(540.))
                        .child(render_preview_large(finder, &entry, cx)),
                )
                .footer(
                    DialogFooter::new()
                        .child(h_flex().flex_1().when_some(text_size, |this, size| {
                            this.child(text_size_buttons(&view, size))
                        }))
                        .child(
                            Button::new("ql-reveal")
                                .label(super::menus::reveal_label())
                                .on_click({
                                    let path = entry.path.clone();
                                    move |_, _, cx| crate::system::reveal(&path, cx)
                                }),
                        )
                        .child(Button::new("ql-open").primary().label("Open").on_click(
                            move |_, window, cx| {
                                // `close_dialog` doesn't run `on_close`.
                                window.close_dialog(cx);
                                open_view.update(cx, |this, cx| {
                                    this.quick_look_open = false;
                                    // A file no app claims opens in an editor.
                                    this.open_files(vec![open_path.clone()], window, cx);
                                });
                            },
                        )),
                )
                .on_close(move |_, _, cx| {
                    close_view.update(cx, |this, _| this.quick_look_open = false);
                })
        });
    }
}

/// Steps the preview text size by `steps` points, or resets it for `None`.
fn step_text_size(view: &Entity<Finder>, steps: Option<f32>, window: &mut Window, cx: &mut App) {
    view.update(cx, |this, cx| match steps {
        Some(steps) => this.step_preview_font_size(steps, cx),
        None => this.set_preview_font_size(None, cx),
    });
    // The dialog is drawn by the window's root, not by the Finder view.
    window.refresh();
}

fn text_size_handler<A: Action>(
    view: &Entity<Finder>,
    steps: Option<f32>,
) -> impl Fn(&A, &mut Window, &mut App) + 'static {
    let view = view.clone();
    move |_, window, cx| step_text_size(&view, steps, window, cx)
}

/// Quick Look's smaller/bigger text buttons, and the size, which resets it.
fn text_size_buttons(view: &Entity<Finder>, size: f32) -> impl IntoElement {
    let (min, max) = FONT_SIZE_RANGE;
    let view = view.clone();
    size_stepper(
        "ql-text",
        format!("{size} pt"),
        [size <= min, size >= max, size == DEFAULT_PREVIEW_FONT_SIZE],
        Some([&PreviewTextSmaller, &PreviewTextBigger, &PreviewTextReset]),
        move |steps, window, cx| step_text_size(&view, steps, window, cx),
    )
}

/// A small "A" (smaller), a big "A" (bigger), and the current size, which
/// resets it; `step` gets -1, +1, or `None`. The buttons are `{id}-smaller`,
/// `{id}-bigger` and `{id}-reset`; `disabled` follows the same order, and
/// `keys` are Quick Look actions whose shortcuts the tooltips show.
pub(super) fn size_stepper(
    id: &str,
    label: String,
    disabled: [bool; 3],
    keys: Option<[&dyn Action; 3]>,
    step: impl Fn(Option<f32>, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let step = std::rc::Rc::new(step);
    let tips = ["Smaller Text", "Bigger Text", "Actual Size"];
    let buttons = [
        ("smaller", Some(-1.)),
        ("bigger", Some(1.)),
        ("reset", None),
    ];
    h_flex()
        .gap(ui(2.))
        .children(buttons.into_iter().enumerate().map(|(ix, (name, steps))| {
            let step = step.clone();
            let button = Button::new(SharedString::from(format!("{id}-{name}")))
                .ghost()
                .small()
                .disabled(disabled[ix])
                .on_click(move |_, window, cx| step(steps, window, cx));
            let button = match ix {
                0 => button.child(div().text_size(ui(11.)).child("A")),
                1 => button.child(div().text_size(ui(16.)).child("A")),
                _ => button.label(label.clone()),
            };
            match keys {
                Some(keys) => button.tooltip_with_action(tips[ix], keys[ix], Some(QUICK_LOOK)),
                None => button.tooltip(tips[ix]),
            }
        }))
}

/// A big preview with a caption: the gallery's main area and Quick Look.
pub fn render_preview_large(finder: &Finder, entry: &FileEntry, cx: &App) -> AnyElement {
    let theme = cx.theme();
    v_flex()
        .size_full()
        .gap(ui(10.))
        .child(
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .rounded(ui(8.))
                .overflow_hidden()
                .child(preview_body(
                    finder,
                    entry,
                    ui(220.),
                    finder.config.preview_font_size(),
                    cx,
                )),
        )
        .child(
            h_flex()
                .justify_center()
                .gap(ui(8.))
                .text_size(ui(12.))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(entry.name.clone()),
                )
                .child(
                    div()
                        .text_color(theme.muted_foreground)
                        .child(info_line(entry)),
                ),
        )
        .into_any_element()
}

/// A compact card for the column view's last column.
pub fn render_preview_card(finder: &Finder, entry: &FileEntry, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let row = |label: &'static str, value: String| {
        h_flex()
            .gap(ui(8.))
            .text_size(ui(11.))
            .child(div().w(ui(70.)).text_color(muted).child(label))
            .child(div().flex_1().truncate().child(value))
    };
    v_flex()
        .size_full()
        .gap(ui(12.))
        .child(
            div()
                .h(ui(240.))
                .w_full()
                .rounded(ui(8.))
                .overflow_hidden()
                .child(preview_body(
                    finder,
                    entry,
                    ui(120.),
                    DEFAULT_PREVIEW_FONT_SIZE,
                    cx,
                )),
        )
        .child(
            div()
                .text_size(ui(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(entry.name.clone()),
        )
        .child(row("Kind", entry.kind_label()))
        .when_some(entry.size, |this, size| {
            this.child(row("Size", format::human_size(size)))
        })
        .when_some(entry.modified, |this, t| {
            this.child(row("Modified", format::human_date(t)))
        })
        .child(row(
            "Where",
            entry.parent().map(super::tildify).unwrap_or_default(),
        ))
        .into_any_element()
}

fn info_line(entry: &FileEntry) -> String {
    let mut parts = vec![entry.kind_label()];
    if let Some(size) = entry.size {
        parts.push(format::human_size(size));
    }
    if let Some(time) = entry.modified {
        parts.push(format::human_date(time));
    }
    parts.join(" · ")
}

/// The preview itself; `text_size` is in points.
fn preview_body(
    finder: &Finder,
    entry: &FileEntry,
    icon: Pixels,
    text_size: f32,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let pal = palette(cx);
    let centered_icon = |extra: Option<String>| {
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap(ui(10.))
            .child(file_icon(entry, icon, false, &pal))
            .when_some(extra, |this, text| {
                this.child(
                    div()
                        .text_size(ui(12.))
                        .text_color(theme.muted_foreground)
                        .child(text),
                )
            })
            .into_any_element()
    };
    let is_image = entry.kind == FileKind::Image;
    match finder.preview_of(entry) {
        _ if is_image => div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                img(entry.path.clone())
                    .size_full()
                    .object_fit(ObjectFit::Contain)
                    .with_fallback({
                        let asset = entry.kind.icon_asset();
                        move || img(asset).size(ui(160.)).into_any_element()
                    }),
            )
            .into_any_element(),
        Preview::Loading => v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .child(Spinner::new().small())
            .into_any_element(),
        Preview::Image | Preview::Unavailable => centered_icon(None),
        Preview::Folder { items } => centered_icon(Some(if items == 1 {
            "1 item".into()
        } else {
            format!("{items} items")
        })),
        Preview::Text { text, truncated } => div()
            .id(("text-preview", super::views::path_id(&entry.path)))
            .size_full()
            .bg(theme.muted)
            .p(ui(12.))
            .overflow_y_scrollbar()
            .font_family(theme.mono_font_family.clone())
            .text_size(ui(text_size))
            .child(text)
            .when(truncated, |this| {
                this.child(
                    div()
                        .pt(ui(8.))
                        .text_color(theme.muted_foreground)
                        .child("… (preview truncated)"),
                )
            })
            .into_any_element(),
        Preview::Code {
            text,
            mut spans,
            comments,
            truncated,
        } => div()
            .id(("code-preview", super::views::path_id(&entry.path)))
            .size_full()
            .bg(theme.muted)
            .p(ui(12.))
            .overflow_y_scrollbar()
            .font_family(theme.mono_font_family.clone())
            .text_size(ui(text_size))
            .child({
                let comment = HighlightStyle {
                    color: Some(theme.muted_foreground),
                    ..Default::default()
                };
                spans.extend(comments.into_iter().map(|range| (range, comment)));
                spans.sort_by_key(|(range, _)| range.start);
                StyledText::new(text).with_highlights(spans)
            })
            .when(truncated, |this| {
                this.child(
                    div()
                        .pt(ui(8.))
                        .text_color(theme.muted_foreground)
                        .child("… (preview truncated)"),
                )
            })
            .into_any_element(),
        Preview::Table {
            columns,
            rows,
            via_nu,
        } => {
            let header_bg = theme.muted;
            let border = theme.border;
            let ncols = columns.len().max(1);
            // Columns widen with the text.
            let cell = 140. * text_size / DEFAULT_PREVIEW_FONT_SIZE;
            let width = ui((ncols as f32 * cell).max(300.));
            v_flex()
                .size_full()
                .bg(theme.background)
                .border_1()
                .border_color(border)
                .rounded(ui(8.))
                .overflow_hidden()
                .child(
                    div()
                        .id(("table-preview", super::views::path_id(&entry.path)))
                        .flex_1()
                        .min_h_0()
                        .overflow_scrollbar()
                        .child(
                            v_flex()
                                .min_w(width)
                                .text_size(ui(text_size))
                                .child(
                                    h_flex()
                                        .bg(header_bg)
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .children(
                                            columns.iter().map(|c| table_cell(c.clone(), cell)),
                                        ),
                                )
                                .children(rows.iter().enumerate().map(|(ix, row)| {
                                    h_flex()
                                        .when(ix % 2 == 1, |this| this.bg(pal.stripe))
                                        .children(row.iter().map(|c| table_cell(c.clone(), cell)))
                                })),
                        ),
                )
                .when(via_nu, |this| {
                    this.child(
                        h_flex()
                            .px(ui(10.))
                            .py(ui(4.))
                            .border_t_1()
                            .border_color(border)
                            .text_size(ui(10.5))
                            .text_color(theme.muted_foreground)
                            .child(format!(
                                "Opened with Nushell · {} row{}{}",
                                rows.len(),
                                if rows.len() == 1 { "" } else { "s" },
                                if rows.len() >= TABLE_ROWS {
                                    " shown"
                                } else {
                                    ""
                                }
                            )),
                    )
                })
                .into_any_element()
        }
    }
}

fn table_cell(text: SharedString, width: f32) -> impl IntoElement {
    div()
        .w(ui(width))
        .flex_shrink_0()
        .px(ui(8.))
        .py(ui(3.))
        .truncate()
        .child(text)
}

#[cfg(test)]
mod tests {
    // Not `super::*`: GPUI's `test` attribute would replace the standard one.
    use super::{Preview, load_preview};
    use crate::nushell;
    use fff_Finder_core::FileEntry;

    /// Runs only where `nu` is installed.
    #[test]
    fn nushell_scripts_are_highlighted() {
        let Some(nu) = nushell::find_nu(None) else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("fff-highlight-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("demo.nu");
        std::fs::write(
            &script,
            "# Say hi\ndef greet [name: string] {\n  $\"hi ($name) #1\"\n}\n",
        )
        .unwrap();
        let entry = FileEntry::from_path(&script).unwrap();
        match load_preview(&entry, Some(&nu)) {
            Preview::Code {
                text,
                spans,
                comments,
                ..
            } => {
                assert!(text.starts_with("# Say hi"), "{text}");
                assert_eq!(comments.len(), 1, "the comment is found");
                assert_eq!(&text[comments[0].clone()], "# Say hi");
                assert!(!text.contains('\u{1b}'));
                assert!(!spans.is_empty(), "nu-highlight colored something");
            }
            _ => panic!("expected highlighted code"),
        }
        let plain = dir.join("notes.txt");
        std::fs::write(&plain, "def greet").unwrap();
        let entry = FileEntry::from_path(&plain).unwrap();
        assert!(matches!(
            load_preview(&entry, Some(&nu)),
            Preview::Text { .. }
        ));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
