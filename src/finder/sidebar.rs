//! The floating sidebar: Recents, Favorites, Locations, and index status.

use std::path::PathBuf;

use fff_finder_core::places::{Place, PlaceIcon};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::toolbar::{TOOLBAR_HEIGHT, pill, pill_button, window_drag_area};
use super::views::DraggedFiles;
use super::{Finder, group_digits};
use crate::actions::*;
use crate::model::Location;
use crate::theme::palette;

pub const SIDEBAR_WIDTH: f32 = 212.;

pub fn place_icon(icon: PlaceIcon) -> Icon {
    match icon {
        PlaceIcon::Applications => Icon::new(Lucide::AppWindow),
        PlaceIcon::Desktop => Icon::new(Lucide::Monitor),
        PlaceIcon::Documents => Icon::new(IconName::FileText),
        PlaceIcon::Downloads => Icon::new(Lucide::Download),
        PlaceIcon::Pictures => Icon::new(Lucide::Image),
        PlaceIcon::Music => Icon::new(Lucide::Music),
        PlaceIcon::Movies => Icon::new(Lucide::Film),
        PlaceIcon::Home => Icon::new(Lucide::House),
        PlaceIcon::Cloud => Icon::new(Lucide::Cloud),
        PlaceIcon::Drive => Icon::new(IconName::HardDrive),
        PlaceIcon::External => Icon::new(Lucide::Usb),
        PlaceIcon::Folder => Icon::new(IconName::Folder),
    }
}

impl Finder {
    pub(super) fn render_sidebar(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let (bg, border) = (theme.sidebar, theme.sidebar_border);
        let pal = palette(cx);
        let current = self.history.current().clone();

        let mut list = v_flex()
            .id("sidebar-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(8.))
            .pb(px(8.))
            .child(self.sidebar_row(
                "recents",
                Icon::new(Lucide::Clock),
                "Recents".into(),
                current == Location::Recents,
                Location::Recents,
                None,
                cx,
            ));

        list = list.child(section_heading("Favorites", pal.sidebar_heading));
        let favorites = self.favorites.clone();
        for (ix, place) in favorites.into_iter().enumerate() {
            let active = current.folder() == Some(place.path.as_path());
            list = list.child(self.sidebar_row(
                ("fav", ix),
                place_icon(place.icon),
                place.name.clone().into(),
                active,
                Location::Folder(place.path.clone()),
                Some((place, true)),
                cx,
            ));
        }

        list = list.child(section_heading("Locations", pal.sidebar_heading));
        let locations = self.locations.clone();
        for (ix, place) in locations.into_iter().enumerate() {
            let active = current.folder() == Some(place.path.as_path());
            list = list.child(self.sidebar_row(
                ("loc", ix),
                place_icon(place.icon),
                place.name.clone().into(),
                active,
                Location::Folder(place.path.clone()),
                Some((place, false)),
                cx,
            ));
        }

        let entity = cx.entity();
        div()
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .flex_shrink_0()
            .p(px(8.))
            .pr(px(0.))
            .child(
                v_flex()
                    .id("sidebar")
                    .size_full()
                    .rounded(px(14.))
                    .bg(bg)
                    .border_1()
                    .border_color(border)
                    .overflow_hidden()
                    // Drop folders here to add them to Favorites.
                    .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                        for path in paths.paths().iter().filter(|p| p.is_dir()) {
                            this.add_favorite(path.clone(), cx);
                        }
                    }))
                    .on_drop(cx.listener(|this, files: &DraggedFiles, _, cx| {
                        for path in files.0.iter().filter(|p| p.is_dir()) {
                            this.add_favorite(path.clone(), cx);
                        }
                    }))
                    .child(window_drag_area(
                        h_flex()
                            .h(px(TOOLBAR_HEIGHT - 8.))
                            .flex_shrink_0()
                            .justify_end()
                            .px(px(6.))
                            .child(
                                pill(cx)
                                    .bg(gpui_kit::transparent_black())
                                    .border_color(gpui_kit::transparent_black())
                                    .child(
                                        pill_button("hide-sidebar", IconName::PanelLeft, cx)
                                            .tooltip("Hide Sidebar")
                                            .on_click(move |_, window, cx| {
                                                let _ = &entity;
                                                window.dispatch_action(Box::new(ToggleSidebar), cx)
                                            }),
                                    ),
                            ),
                    ))
                    .child(list)
                    .child(self.render_index_footer(cx)),
            )
    }

    #[allow(clippy::too_many_arguments)]
    fn sidebar_row(
        &self,
        id: impl Into<ElementId>,
        icon: Icon,
        label: SharedString,
        active: bool,
        target: Location,
        place: Option<(Place, bool)>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let pal = palette(cx);
        let view = cx.entity();
        let row = h_flex()
            .id(id)
            .h(px(28.))
            .px(px(8.))
            .gap(px(8.))
            .rounded(px(7.))
            .text_size(px(13.))
            .text_color(theme.sidebar_foreground)
            .when(active, |this| this.bg(theme.sidebar_accent))
            .when(!active, |this| this.hover(|s| s.bg(theme.list_hover)))
            .child(icon.with_size(px(16.)).text_color(pal.sidebar_icon))
            .child(div().flex_1().truncate().child(label))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.navigate(target.clone(), window, cx);
            }));
        let Some((place, is_favorite)) = place else {
            return row.into_any_element();
        };
        let path: PathBuf = place.path.clone();
        row.context_menu(move |menu, window, _| {
            let open_nu = path.clone();
            let reveal = path.clone();
            let remove = path.clone();
            let mut menu = menu
                .item(
                    PopupMenuItem::new("Open Nushell Here")
                        .icon(IconName::SquareTerminal)
                        .on_click(window.listener_for(
                            &view,
                            move |this: &mut Finder, _: &ClickEvent, window, cx| {
                                this.open_terminal_at(&open_nu, window, cx)
                            },
                        )),
                )
                .item(
                    PopupMenuItem::new(super::menus::reveal_label())
                        .icon(IconName::ExternalLink)
                        .on_click(move |_, _, cx| cx.reveal_path(&reveal)),
                );
            if is_favorite {
                menu = menu
                    .separator()
                    .item(
                        PopupMenuItem::new("Remove from Sidebar").on_click(window.listener_for(
                            &view,
                            move |this: &mut Finder, _: &ClickEvent, _, cx| {
                                this.remove_favorite(&remove, cx)
                            },
                        )),
                    );
            }
            menu
        })
        .into_any_element()
    }

    fn render_index_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let (icon, text) = match self.indexing_progress() {
            Some(n) => (
                Spinner::new().xsmall().color(muted).into_any_element(),
                format!("Indexing… {}", group_digits(n)),
            ),
            None => (
                Icon::new(Lucide::ScanSearch)
                    .with_size(px(14.))
                    .text_color(muted)
                    .into_any_element(),
                format!("{} files indexed", group_digits(self.indexed_files())),
            ),
        };
        h_flex()
            .id("index-footer")
            .h(px(34.))
            .flex_shrink_0()
            .px(px(14.))
            .gap(px(8.))
            .border_t_1()
            .border_color(theme.sidebar_border)
            .text_size(px(11.))
            .text_color(muted)
            .child(icon)
            .child(div().flex_1().truncate().child(text))
            .child(
                div()
                    .id("index-settings")
                    .p(px(3.))
                    .rounded(px(5.))
                    .hover(|s| s.bg(theme.list_hover))
                    .child(
                        Icon::new(IconName::Settings)
                            .with_size(px(14.))
                            .text_color(muted),
                    )
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(ShowSettings), cx)),
            )
    }
}

fn section_heading(label: &'static str, color: Hsla) -> impl IntoElement {
    div()
        .pt(px(14.))
        .pb(px(4.))
        .px(px(8.))
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(color)
        .child(label)
}
