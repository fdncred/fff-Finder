//! The Nushell pipeline panel: run a pipeline over the selected files (or
//! everything shown) and see the result as a table.

use super::Finder;
use crate::actions::ToggleNuPanel;
use crate::nushell::NuOutput;
use crate::theme::palette;
use crate::theme::ui;
use gpui_kit::TestSupportExt as _;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::ops::Range;

const EXAMPLES: [&str; 4] = [
    "where size > 1mb | sort-by size --reverse",
    "group-by kind | transpose kind files | update files { length }",
    "get path | each { open --raw $in | lines | length } | math sum",
    "where type == file | get name | str join (char nl)",
];

impl Finder {
    pub(super) fn render_nu_panel(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme().clone();
        let border = theme.border;
        let muted = theme.muted_foreground;
        // Settings › Nushell text size; 12 is the default.
        let size = self.config.nu_font_size();
        let input_count = {
            let n = self.selected_entries().len();
            if n == 0 { self.rows.len() } else { n }
        };
        let scope_label = if self.selected_entries().is_empty() {
            format!("$in: all {input_count} shown items")
        } else {
            format!(
                "$in: {input_count} selected item{}",
                if input_count == 1 { "" } else { "s" }
            )
        };
        let can_clear = self.nu_result.is_some()
            || self.nu_running
            || !self.nu_input.read(cx).value().is_empty();
        let header = h_flex()
            .h(ui(40.))
            .flex_shrink_0()
            .px(ui(12.))
            .gap(ui(8.))
            .child(
                Icon::new(Lucide::Terminal)
                    .with_size(ui(15.))
                    .text_color(theme.primary),
            )
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(ui(12.))
                    .child("Nushell"),
            )
            .child(
                Input::new(&self.nu_input)
                    .id("nu-input")
                    .small()
                    .flex_1()
                    .font_family(theme.mono_font_family.clone())
                    .text_size(ui(size)),
            )
            .child(
                Button::new("nu-run")
                    .small()
                    .primary()
                    .label("Run")
                    .loading(self.nu_running)
                    .on_click(cx.listener(|this, _, window, cx| this.run_nu_pipeline(window, cx))),
            )
            .child(
                Button::new("nu-clear")
                    .small()
                    .label("Clear")
                    .tooltip("Clear the pipeline and its result")
                    .disabled(!can_clear)
                    .on_click(cx.listener(|this, _, window, cx| this.clear_nu(window, cx))),
            )
            .child(
                Button::new("nu-close")
                    .small()
                    .ghost()
                    .icon(IconName::Close)
                    .tooltip("Close (Ctrl-`)")
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.toggle_nu_panel(&ToggleNuPanel, window, cx)
                    })),
            );

        let body: AnyElement = match &self.nu_result {
            _ if self.nu_running => v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(Spinner::new().small())
                .into_any_element(),
            None => {
                let view = cx.entity();
                v_flex()
                    .size_full()
                    .p(ui(12.))
                    .gap(ui(6.))
                    .text_size(ui(size))
                    .text_color(muted)
                    .child(format!(
                        "{scope_label}. Each item has name, path, type, kind, size, and modified. Try:"
                    ))
                    .children(EXAMPLES.iter().enumerate().map(|(ix, example)| {
                        let view = view.clone();
                        div()
                            .id(("nu-example", ix))
                            .font_family(theme.mono_font_family.clone())
                            .text_color(theme.link)
                            .cursor_pointer()
                            .child(*example)
                            .on_click(move |_, window, cx| {
                                view.update(cx, |this, cx| {
                                    this.nu_input.update(cx, |s, cx| s.set_value(*example, window, cx));
                                    this.run_nu_pipeline(window, cx);
                                });
                            })
                            .test_support()
                    }))
                    .into_any_element()
            }
            Some(run) => {
                let mut col = v_flex().size_full();
                if let Some(error) = &run.error {
                    col = col.child(
                        div()
                            .id("nu-error-scroll")
                            .flex_shrink_0()
                            .max_h(ui(140.))
                            .overflow_y_scrollbar()
                            .m(ui(8.))
                            .p(ui(8.))
                            .rounded(ui(6.))
                            .bg(theme.danger.opacity(0.12))
                            .text_color(theme.danger)
                            .font_family(theme.mono_font_family.clone())
                            .text_size(ui(size - 0.5))
                            // The scrollbar wrapper hides its own element from UI
                            // tests, so the text inside is the observed one.
                            .child(div().id("nu-error").test_support().child(error.clone())),
                    );
                }
                col = col.child(match &run.output {
                    NuOutput::Empty if run.error.is_none() => div()
                        .p(ui(12.))
                        .text_size(ui(size))
                        .text_color(muted)
                        .child("The pipeline returned nothing.")
                        .into_any_element(),
                    NuOutput::Empty => div().into_any_element(),
                    NuOutput::Text(text) => div()
                        .id("nu-text-scroll")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scrollbar()
                        .p(ui(12.))
                        .font_family(theme.mono_font_family.clone())
                        .text_size(ui(size))
                        .child(div().id("nu-text").test_support().child(text.clone()))
                        .into_any_element(),
                    NuOutput::Table { columns, rows } => {
                        self.render_nu_table(columns, rows.len(), size, cx)
                    }
                });
                col.child(
                    h_flex()
                        .h(ui(size + 10.))
                        .flex_shrink_0()
                        .px(ui(12.))
                        .text_size(ui(size - 1.5))
                        .text_color(muted)
                        .child(format!(
                            "{scope_label} · {:.0} ms",
                            run.elapsed.as_secs_f64() * 1000.
                        )),
                )
                .into_any_element()
            }
        };

        v_flex()
            // Taller for bigger text, up to half again, and never more than
            // half the window.
            .h(ui(260. * (size / 12.).clamp(1., 1.5)))
            .max_h(relative(0.5))
            .flex_shrink_0()
            .border_t_1()
            .border_color(border)
            .bg(theme.background)
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .border_t_1()
                    .border_color(border)
                    .child(body),
            )
    }

    fn render_nu_table(
        &self,
        columns: &[String],
        len: usize,
        size: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let ncols = columns.len().max(1);
        // Column widths grow with the text.
        let (index_w, min_w) = (ui(44. * size / 12.), ui(90. * size / 12.));
        let header = h_flex()
            .flex_shrink_0()
            .w_full()
            .h(ui(size + 12.))
            .bg(theme.muted)
            .text_size(ui(size - 0.5))
            .font_weight(FontWeight::SEMIBOLD)
            .child(
                div()
                    .w(index_w)
                    .px(ui(8.))
                    .text_color(theme.muted_foreground)
                    .child("#"),
            )
            .children(columns.iter().map(|c| {
                div()
                    .flex_1()
                    .flex_basis(ui(0.))
                    .min_w(min_w)
                    .px(ui(8.))
                    .truncate()
                    .child(c.clone())
            }));
        let list = uniform_list(
            "nu-rows",
            len,
            cx.processor(move |this: &mut Self, range: Range<usize>, _, cx| {
                let pal = palette(cx);
                let muted = cx.theme().muted_foreground;
                let Some(crate::nushell::NuRun {
                    output: NuOutput::Table { rows, .. },
                    ..
                }) = &this.nu_result
                else {
                    return Vec::new();
                };
                range
                    .filter_map(|ix| rows.get(ix).map(|row| (ix, row)))
                    .map(|(ix, row)| {
                        h_flex()
                            .h(ui(size + 10.))
                            .w_full()
                            .text_size(ui(size - 0.5))
                            .when(ix % 2 == 1, |this| this.bg(pal.stripe))
                            .child(
                                div()
                                    .w(index_w)
                                    .px(ui(8.))
                                    .text_color(muted)
                                    .child(ix.to_string()),
                            )
                            .children((0..ncols).map(|c| {
                                div()
                                    .flex_1()
                                    .flex_basis(ui(0.))
                                    .min_w(min_w)
                                    .px(ui(8.))
                                    .truncate()
                                    .child(row.get(c).cloned().unwrap_or_default())
                            }))
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .flex_1()
        .min_h_0();
        v_flex()
            .flex_1()
            .min_h_0()
            .child(header)
            .child(list)
            .into_any_element()
    }
}
