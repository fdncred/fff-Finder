//! File previews for the gallery, the column view, and Quick Look. Structured
//! files (CSV, JSON, TOML, YAML, SQLite, …) are opened with Nushell and shown
//! as tables.

use std::io::Read as _;
use std::path::Path;

use fff_finder_core::{FileEntry, FileKind, format};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::views::file_icon;
use super::{AppState, Finder};
use crate::actions::QuickLook;
use crate::nushell::{self, NuOutput};
use crate::theme::palette;

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

fn load_preview(entry: &FileEntry, nu: Option<&Path>) -> Preview {
    if entry.is_dir {
        let items = std::fs::read_dir(&entry.path)
            .map(|d| d.count())
            .unwrap_or(0);
        return Preview::Folder { items };
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
fn read_text(path: &Path) -> Option<(String, bool)> {
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
            window.close_dialog(cx);
            self.quick_look_open = false;
            return;
        }
        let Some(entry) = self.primary_entry() else {
            return;
        };
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
            let close_view = view.clone();
            dialog
                .title(
                    h_flex()
                        .gap(px(8.))
                        .child(file_icon(&entry, px(20.), false, &palette(cx)))
                        .child(div().truncate().child(entry.name.clone())),
                )
                .w(px(860.))
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
                        .h(px(540.))
                        .child(render_preview_large(finder, &entry, cx)),
                )
                .footer(
                    DialogFooter::new()
                        .child(
                            Button::new("ql-reveal")
                                .label(super::menus::reveal_label())
                                .on_click({
                                    let path = entry.path.clone();
                                    move |_, _, cx| cx.reveal_path(&path)
                                }),
                        )
                        .child(Button::new("ql-open").primary().label("Open").on_click(
                            move |_, window, cx| {
                                cx.open_with_system(&open_path);
                                window.close_dialog(cx);
                            },
                        )),
                )
                .on_close(move |_, _, cx| {
                    close_view.update(cx, |this, _| this.quick_look_open = false);
                })
        });
    }
}

/// A big preview with a caption: the gallery's main area and Quick Look.
pub fn render_preview_large(finder: &Finder, entry: &FileEntry, cx: &App) -> AnyElement {
    let theme = cx.theme();
    v_flex()
        .size_full()
        .gap(px(10.))
        .child(
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .rounded(px(8.))
                .overflow_hidden()
                .child(preview_body(finder, entry, px(220.), cx)),
        )
        .child(
            h_flex()
                .justify_center()
                .gap(px(8.))
                .text_size(px(12.))
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
            .gap(px(8.))
            .text_size(px(11.))
            .child(div().w(px(70.)).text_color(muted).child(label))
            .child(div().flex_1().truncate().child(value))
    };
    v_flex()
        .size_full()
        .gap(px(12.))
        .child(
            div()
                .h(px(240.))
                .w_full()
                .rounded(px(8.))
                .overflow_hidden()
                .child(preview_body(finder, entry, px(120.), cx)),
        )
        .child(
            div()
                .text_size(px(13.))
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

fn preview_body(finder: &Finder, entry: &FileEntry, icon: Pixels, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let pal = palette(cx);
    let centered_icon = |extra: Option<String>| {
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap(px(10.))
            .child(file_icon(entry, icon, false, &pal))
            .when_some(extra, |this, text| {
                this.child(
                    div()
                        .text_size(px(12.))
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
                        move || img(asset).size(px(160.)).into_any_element()
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
            .p(px(12.))
            .overflow_y_scrollbar()
            .font_family(theme.mono_font_family.clone())
            .text_size(px(11.5))
            .child(text)
            .when(truncated, |this| {
                this.child(
                    div()
                        .pt(px(8.))
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
            let width = px((ncols as f32 * 140.).max(300.));
            v_flex()
                .size_full()
                .bg(theme.background)
                .border_1()
                .border_color(border)
                .rounded(px(8.))
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
                                .text_size(px(11.5))
                                .child(
                                    h_flex()
                                        .bg(header_bg)
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .children(columns.iter().map(|c| table_cell(c.clone()))),
                                )
                                .children(rows.iter().enumerate().map(|(ix, row)| {
                                    h_flex()
                                        .when(ix % 2 == 1, |this| this.bg(pal.stripe))
                                        .children(row.iter().map(|c| table_cell(c.clone())))
                                })),
                        ),
                )
                .when(via_nu, |this| {
                    this.child(
                        h_flex()
                            .px(px(10.))
                            .py(px(4.))
                            .border_t_1()
                            .border_color(border)
                            .text_size(px(10.5))
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

fn table_cell(text: SharedString) -> impl IntoElement {
    div()
        .w(px(140.))
        .flex_shrink_0()
        .px(px(8.))
        .py(px(3.))
        .truncate()
        .child(text)
}
