//! The unified toolbar: title, back/forward, view switcher, sort and action
//! menus, Nushell menu, and the search field.

use super::Finder;
use crate::actions::*;
use crate::theme::palette;
use crate::theme::ui;
use fff_Finder_core::{SortKey, ViewMode};
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonRounded, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Selectable as _, Sizable as _, h_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

pub const TOOLBAR_HEIGHT: f32 = 52.;
/// Room for the macOS traffic lights when they sit over this view.
pub const TRAFFIC_LIGHTS: f32 = 78.;

/// A rounded group of toolbar buttons.
pub fn pill(cx: &App) -> Div {
    let pal = palette(cx);
    h_flex()
        .h(ui(34.))
        .p(ui(2.))
        .gap(ui(2.))
        .rounded_full()
        .bg(pal.pill)
        .border_1()
        .border_color(pal.pill_border)
        .flex_shrink_0()
        // Clicks on controls must not start a window drag.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

pub fn pill_button(id: impl Into<ElementId>, icon: impl Into<Icon>, cx: &App) -> Button {
    Button::new(id)
        .ghost()
        .icon(icon)
        .rounded(ButtonRounded::Size(ui(15.)))
        .w(ui(36.))
        .h(ui(28.))
        .text_color(cx.theme().foreground)
}

/// Start a window drag (or zoom on double-click) from empty toolbar space.
pub fn window_drag_area<E: InteractiveElement>(el: E) -> E {
    if cfg!(target_os = "macos") {
        el.on_mouse_down(MouseButton::Left, |event, window, _| {
            if event.click_count >= 2 {
                window.titlebar_double_click();
            } else {
                window.start_window_move();
            }
        })
    } else {
        el
    }
}

impl Finder {
    pub(super) fn render_toolbar(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let pal = palette(cx);
        let view = cx.entity();
        let mode = self.config.view_mode;
        let traffic = cfg!(target_os = "macos") && !self.config.show_sidebar;
        let can_back = self.history.can_back();
        let can_forward = self.history.can_forward();

        let nav = pill(cx)
            .child(
                pill_button("back", IconName::ChevronLeft, cx)
                    .tooltip("Back")
                    .disabled(!can_back)
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.go_back(&GoBack, window, cx)
                    })),
            )
            .child(
                pill_button("forward", IconName::ChevronRight, cx)
                    .tooltip("Forward")
                    .disabled(!can_forward)
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.go_forward(&GoForward, window, cx)
                    })),
            );

        let view_button = |id: &'static str,
                           icon: Lucide,
                           tip: &'static str,
                           m: ViewMode,
                           cx: &mut Context<Self>| {
            pill_button(id, icon, cx)
                .tooltip(tip)
                .selected(mode == m)
                .when(mode == m, |b| b.bg(pal.pill_selected))
                .on_click(cx.listener(move |this, _, _, cx| this.set_view_mode(m, cx)))
        };
        let views = pill(cx)
            .child(view_button(
                "view-icons",
                Lucide::LayoutGrid,
                "as Icons",
                ViewMode::Icons,
                cx,
            ))
            .child(view_button(
                "view-list",
                Lucide::List,
                "as List",
                ViewMode::List,
                cx,
            ))
            .child(view_button(
                "view-columns",
                Lucide::Columns3,
                "as Columns",
                ViewMode::Columns,
                cx,
            ))
            .child(view_button(
                "view-gallery",
                Lucide::GalleryHorizontalEnd,
                "as Gallery",
                ViewMode::Gallery,
                cx,
            ));

        let sort_view = view.clone();
        let sort = pill(cx).child(
            pill_button("sort", Lucide::ArrowUpDown, cx)
                .w(ui(48.))
                .dropdown_caret(true)
                .tooltip("Sort and View Options")
                .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, window, cx| {
                    sort_menu(&sort_view, menu, window, cx)
                }),
        );

        let nu_view = view.clone();
        let nushell = pill(cx).child(
            pill_button("nushell", IconName::SquareTerminal, cx)
                .w(ui(48.))
                .dropdown_caret(true)
                .tooltip("Nushell")
                .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, window, cx| {
                    nushell_menu(&nu_view, menu, window, cx)
                }),
        );

        let actions_view = view.clone();
        let actions = pill(cx).child(
            pill_button("actions", IconName::Ellipsis, cx)
                .w(ui(48.))
                .dropdown_caret(true)
                .tooltip("Actions")
                .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, window, cx| {
                    actions_menu(&actions_view, menu, window, cx)
                }),
        );

        let search = h_flex()
            .w(ui(250.))
            .flex_shrink_0()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            // Down arrow moves from the search field into the results.
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "down" && !this.rows.is_empty() {
                    this.focus_files(window, cx);
                    if this.selection.is_empty() {
                        this.selection.select_only(0);
                        this.scroll_to(0);
                        this.selection_changed(cx);
                    }
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .child(
                Input::new(&self.search_input)
                    .id("search")
                    .prefix(
                        Icon::new(IconName::Search)
                            .small()
                            .text_color(cx.theme().muted_foreground),
                    )
                    .cleanable(true)
                    .h(ui(34.))
                    .rounded_full()
                    .bg(pal.pill)
                    .border_color(pal.pill_border)
                    .w_full(),
            );

        let title = self.title();
        window_drag_area(
            h_flex()
                .id("toolbar")
                .h(ui(TOOLBAR_HEIGHT))
                .w_full()
                .flex_shrink_0()
                // macOS draws the window buttons at a fixed size.
                .pl(if traffic { px(TRAFFIC_LIGHTS) } else { ui(12.) })
                .pr(ui(10.))
                .gap(ui(8.))
                .when(!self.config.show_sidebar, |this| {
                    this.child(
                        pill(cx).child(
                            pill_button("show-sidebar", IconName::PanelLeft, cx)
                                .tooltip("Show Sidebar")
                                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                    this.toggle_sidebar(&ToggleSidebar, window, cx)
                                })),
                        ),
                    )
                })
                .child(
                    div()
                        .flex_1()
                        .min_w(ui(60.))
                        .pl(ui(4.))
                        .text_size(ui(15.))
                        .font_weight(FontWeight::BOLD)
                        .truncate()
                        .child(title),
                )
                .child(nav)
                .child(views)
                .child(sort)
                .child(nushell)
                .child(actions)
                .child(search),
        )
    }
}

fn sort_menu(
    view: &Entity<Finder>,
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let finder = view.read(cx);
    let sort = finder.current_sort();
    let searching = finder.is_searching();
    let hidden = finder.config.show_hidden;
    let mut keys: Vec<SortKey> = Vec::new();
    if searching {
        keys.push(SortKey::Relevance);
    }
    keys.extend(SortKey::BROWSE);
    let mut menu = menu.label("Sort By");
    for key in keys {
        menu = menu.item(
            PopupMenuItem::new(key.label())
                .checked(sort.key == key)
                .on_click(
                    window.listener_for(view, move |this: &mut Finder, _: &ClickEvent, _, cx| {
                        this.set_sort(key, cx)
                    }),
                ),
        );
    }
    menu.separator()
        .item(
            PopupMenuItem::new("Ascending")
                .checked(sort.ascending)
                .disabled(sort.key == SortKey::Relevance)
                .on_click(
                    window.listener_for(view, |this: &mut Finder, _: &ClickEvent, _, cx| {
                        this.set_ascending(true, cx)
                    }),
                ),
        )
        .item(
            PopupMenuItem::new("Descending")
                .checked(!sort.ascending)
                .disabled(sort.key == SortKey::Relevance)
                .on_click(
                    window.listener_for(view, |this: &mut Finder, _: &ClickEvent, _, cx| {
                        this.set_ascending(false, cx)
                    }),
                ),
        )
        .separator()
        .item(
            PopupMenuItem::new("Show Hidden Files")
                .checked(hidden)
                .action(Box::new(ToggleHidden)),
        )
        .min_w(ui(200.))
}

fn nushell_menu(
    view: &Entity<Finder>,
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let finder = view.read(cx);
    let has_selection = !finder.selected_entries().is_empty();
    let panel_open = finder.nu_open;
    let menu = menu
        .item(
            PopupMenuItem::new("Open Nushell Here")
                .icon(IconName::SquareTerminal)
                .action(Box::new(OpenInNushell)),
        )
        .item(
            PopupMenuItem::new(if panel_open {
                "Hide Pipeline Panel"
            } else {
                "Run a Pipeline…"
            })
            .icon(Lucide::Terminal)
            .action(Box::new(ToggleNuPanel)),
        );
    super::menus::actions_submenu(view, menu, window, cx)
        .separator()
        .item(
            PopupMenuItem::new("Copy as Nushell List")
                .icon(IconName::Copy)
                .disabled(!has_selection)
                .action(Box::new(CopyAsNuon)),
        )
        .min_w(ui(220.))
}

fn actions_menu(
    view: &Entity<Finder>,
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let finder = view.read(cx);
    let has_selection = !finder.selected_entries().is_empty();
    let in_folder = finder.current_folder().is_some();
    let folder = finder.current_folder().map(|p| p.to_path_buf());
    let is_favorite = folder
        .as_ref()
        .is_some_and(|f| finder.config.favorites.contains(f));
    menu.item(
        PopupMenuItem::new("New Folder")
            .icon(Lucide::FolderPlus)
            .disabled(!in_folder)
            .action(Box::new(NewFolder)),
    )
    .item(
        PopupMenuItem::new("Quick Look")
            .icon(IconName::Eye)
            .disabled(!has_selection)
            .action(Box::new(QuickLook)),
    )
    .item(
        PopupMenuItem::new("Rename…")
            .icon(Lucide::Pencil)
            .disabled(!has_selection)
            .action(Box::new(Rename)),
    )
    .separator()
    .item(
        PopupMenuItem::new("Copy Path")
            .icon(IconName::Copy)
            .disabled(!has_selection)
            .action(Box::new(CopyPath)),
    )
    .item(
        PopupMenuItem::new(super::menus::reveal_label())
            .icon(IconName::ExternalLink)
            .disabled(!has_selection)
            .action(Box::new(RevealInFileManager)),
    )
    .when_some(folder.filter(|_| !is_favorite), |menu, folder| {
        menu.item(
            PopupMenuItem::new("Add Folder to Sidebar")
                .icon(IconName::Star)
                .on_click(window.listener_for(
                    view,
                    move |this: &mut Finder, _: &ClickEvent, _, cx| {
                        this.add_favorite(folder.clone(), cx)
                    },
                )),
        )
    })
    .separator()
    .item(
        PopupMenuItem::new("Move to Trash")
            .icon(Lucide::Trash)
            .disabled(!has_selection)
            .action(Box::new(MoveToTrash)),
    )
    .separator()
    .item(
        PopupMenuItem::new("Filter with Nushell…")
            .icon(Lucide::Funnel)
            .action(Box::new(ToggleFilter)),
    )
    .item(PopupMenuItem::new("Go to Folder…").action(Box::new(GoToFolder)))
    .item(PopupMenuItem::new("Search Syntax…").action(Box::new(SearchHelp)))
    .item(
        PopupMenuItem::new("Settings…")
            .icon(IconName::Settings)
            .action(Box::new(ShowSettings)),
    )
    .min_w(ui(220.))
}
