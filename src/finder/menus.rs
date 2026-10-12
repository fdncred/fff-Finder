//! Context menus for items and for empty space.

use super::Finder;
use crate::actions::*;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::IconName;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::path::{Path, PathBuf};

pub fn reveal_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Show in Finder"
    } else if cfg!(windows) {
        "Show in Explorer"
    } else {
        "Show in File Manager"
    }
}

/// Right-click on one or more selected items.
pub fn item_menu(
    view: &Entity<Finder>,
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let finder = view.read(cx);
    let entries = finder.selected_entries();
    let Some(first) = entries.first().cloned() else {
        return menu;
    };
    let single = entries.len() == 1;
    let searching = finder.is_searching() || finder.current_folder().is_none();
    let folder_for_nu: Option<PathBuf> = if first.is_dir {
        Some(first.path.clone())
    } else {
        first.parent().map(Path::to_path_buf)
    };
    let is_favorite = finder.config.favorites.contains(&first.path);
    let picker = finder.picker.is_some();

    let enclosing = first.path.clone();
    let nu_dir = folder_for_nu.clone();
    let favorite = first.path.clone();

    menu.when(picker, |menu| {
        menu.item(
            PopupMenuItem::new(if single { "Choose" } else { "Choose Items" })
                .action(Box::new(Choose)),
        )
        .separator()
    })
    .item(
        PopupMenuItem::new("Open")
            .icon(IconName::FolderOpen)
            .action(Box::new(OpenSelected)),
    )
    .item(
        PopupMenuItem::new("Quick Look")
            .icon(IconName::Eye)
            .action(Box::new(QuickLook)),
    )
    .when(searching && single, |menu| {
        menu.item(
            PopupMenuItem::new("Show in Enclosing Folder")
                .icon(IconName::Folder)
                .on_click(window.listener_for(
                    view,
                    move |this: &mut Finder, _: &ClickEvent, window, cx| {
                        this.reveal_in_place(&enclosing, window, cx)
                    },
                )),
        )
    })
    .item(
        PopupMenuItem::new(reveal_label())
            .icon(IconName::ExternalLink)
            .action(Box::new(RevealInFileManager)),
    )
    .separator()
    .item(
        PopupMenuItem::new(if single { "Copy Path" } else { "Copy Paths" })
            .icon(IconName::Copy)
            .action(Box::new(CopyPath)),
    )
    .item(
        PopupMenuItem::new("Copy as Nushell List")
            .icon(Lucide::ClipboardCopy)
            .action(Box::new(CopyAsNuon)),
    )
    .when_some(nu_dir, |menu, dir| {
        menu.item(
            PopupMenuItem::new("Open Nushell Here")
                .icon(IconName::SquareTerminal)
                .on_click(window.listener_for(
                    view,
                    move |this: &mut Finder, _: &ClickEvent, window, cx| {
                        this.open_terminal_at(&dir, window, cx)
                    },
                )),
        )
    })
    .item(
        PopupMenuItem::new("Run Nushell Pipeline…")
            .icon(Lucide::Terminal)
            .on_click(window.listener_for(
                view,
                |this: &mut Finder, _: &ClickEvent, window, cx| {
                    if !this.nu_open {
                        this.toggle_nu_panel(&ToggleNuPanel, window, cx);
                    }
                },
            )),
    )
    .separator()
    .when(single, |menu| {
        menu.item(
            PopupMenuItem::new("Rename…")
                .icon(Lucide::Pencil)
                .action(Box::new(Rename)),
        )
    })
    .when(single && first.is_navigable() && !is_favorite, |menu| {
        menu.item(
            PopupMenuItem::new("Add to Sidebar")
                .icon(IconName::Star)
                .on_click(window.listener_for(
                    view,
                    move |this: &mut Finder, _: &ClickEvent, _, cx| {
                        this.add_favorite(favorite.clone(), cx)
                    },
                )),
        )
    })
    .separator()
    .item(
        PopupMenuItem::new("Move to Trash")
            .icon(Lucide::Trash)
            .action(Box::new(MoveToTrash)),
    )
    .min_w(px(230.))
}

/// Right-click on empty space in a view.
pub fn background_menu(
    view: &Entity<Finder>,
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    // Menus don't stop propagation, so a right-click on an item reaches this
    // menu too. An empty menu is never shown.
    let position = window.mouse_position();
    if view.update(cx, |f, _| f.take_item_menu_click(position)) {
        return menu;
    }
    let finder = view.read(cx);
    let folder = finder.current_folder().map(Path::to_path_buf);
    let hidden = finder.config.show_hidden;
    let is_favorite = folder
        .as_ref()
        .is_some_and(|f| finder.config.favorites.contains(f));
    let nu_folder = folder.clone();
    menu.when(folder.is_some(), |menu| {
        menu.item(
            PopupMenuItem::new("New Folder")
                .icon(Lucide::FolderPlus)
                .action(Box::new(NewFolder)),
        )
    })
    .when_some(nu_folder, |menu, dir| {
        menu.item(
            PopupMenuItem::new("Open Nushell Here")
                .icon(IconName::SquareTerminal)
                .on_click(window.listener_for(
                    view,
                    move |this: &mut Finder, _: &ClickEvent, window, cx| {
                        this.open_terminal_at(&dir, window, cx)
                    },
                )),
        )
    })
    .item(
        PopupMenuItem::new("Run Nushell Pipeline…")
            .icon(Lucide::Terminal)
            .action(Box::new(ToggleNuPanel)),
    )
    .separator()
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
    .item(
        PopupMenuItem::new("Show Hidden Files")
            .checked(hidden)
            .action(Box::new(ToggleHidden)),
    )
    .item(
        PopupMenuItem::new("Refresh")
            .icon(IconName::RefreshCw)
            .action(Box::new(Refresh)),
    )
    .min_w(px(220.))
}
