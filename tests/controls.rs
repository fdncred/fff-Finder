//! Every control in the window, driven with real clicks and key presses, with
//! an observable effect checked for each. A step that hangs fails the run
//! (see `common::HANG_LIMIT`).

mod common;

use common::{Fixture, Open, Ui, at, cmd, item_id};
use fff_finder_app::actions::*;
use fff_finder_app::finder::{PickerOptions, SearchScope};
use fff_finder_app::model::Location;
use fff_finder_app::system::SystemCall;
use fff_finder_core::terminals::{Launch, Terminal};
use fff_finder_core::{Appearance, KindFilter, SortKey, ViewMode};
use gpui_kit::Action as _;
use gpui_kit::TestAppContext;
use gpui_kit::component::ActiveTheme as _;
use std::path::PathBuf;

// ----- toolbar -------------------------------------------------------------------

#[gpui_kit::test]
fn view_buttons_switch_views(cx: &mut TestAppContext) {
    let fx = Fixture::new("view-buttons");
    let mut ui = Ui::open(cx, &fx.dir);
    for (id, mode) in [
        ("view-list", ViewMode::List),
        ("view-columns", ViewMode::Columns),
        ("view-gallery", ViewMode::Gallery),
        ("view-icons", ViewMode::Icons),
    ] {
        ui.click(id);
        assert_eq!(ui.read(|f| f.effective_view()), mode, "after clicking {id}");
    }
}

#[gpui_kit::test]
fn back_and_forward_buttons(cx: &mut TestAppContext) {
    let fx = Fixture::new("back-forward");
    let mut ui = Ui::open(cx, &fx.dir);
    // Buttons don't report a disabled state to tests; check that clicking
    // them does nothing instead.
    ui.click("back");
    ui.click("forward");
    assert_eq!(ui.folder().as_deref(), Some(fx.dir.as_path()));

    ui.double_click(item_id("tile", &fx.path("Projects")));
    assert_eq!(ui.names(), ["atlas"]);
    assert!(ui.read(|f| f.can_go_back()));

    ui.click("back");
    assert_eq!(ui.folder().as_deref(), Some(fx.dir.as_path()));
    assert_eq!(
        ui.selected(),
        ["Projects"],
        "back selects the folder we left"
    );
    ui.click("forward");
    assert_eq!(ui.folder(), Some(fx.path("Projects")));
}

#[gpui_kit::test]
fn sidebar_hide_and_show_buttons(cx: &mut TestAppContext) {
    let fx = Fixture::new("sidebar-toggle");
    let mut ui = Ui::open(cx, &fx.dir);
    assert!(!ui.exists("show-sidebar"));
    ui.click("hide-sidebar");
    assert!(!ui.read(|f| f.config().show_sidebar));
    assert!(!ui.exists("hide-sidebar"));
    ui.click("show-sidebar");
    assert!(ui.read(|f| f.config().show_sidebar));
}

#[gpui_kit::test]
fn sort_menu_items(cx: &mut TestAppContext) {
    let fx = Fixture::new("sort-menu");
    let mut ui = Ui::open(cx, &fx.dir);
    ui.click("sort");
    assert_eq!(
        ui.menu_labels(),
        [
            "Sort By",
            "Name",
            "Date Modified",
            "Size",
            "Kind",
            "Ascending",
            "Descending",
            "Show Hidden Files"
        ]
    );
    ui.click_menu_item("Size");
    assert!(!ui.menu_open(), "choosing an item closes the menu");
    assert_eq!(ui.read(|f| f.current_sort().key), SortKey::Size);

    ui.click("sort");
    ui.click_menu_item("Ascending");
    assert!(ui.read(|f| f.current_sort().ascending));
    ui.click("sort");
    ui.click_menu_item("Descending");
    assert!(!ui.read(|f| f.current_sort().ascending));

    assert!(!ui.names().contains(&".secret".to_string()));
    ui.click("sort");
    ui.click_menu_item("Show Hidden Files");
    assert!(ui.read(|f| f.config().show_hidden));
    assert!(ui.names().contains(&".secret".to_string()));
}

#[gpui_kit::test]
fn nushell_menu_items(cx: &mut TestAppContext) {
    let fx = Fixture::new("nushell-menu");
    let mut ui = Ui::open(cx, &fx.dir);
    // Disabled without a selection: clicking it copies nothing.
    ui.click("nushell");
    ui.click_menu_item("Copy as Nushell List");
    assert_eq!(ui.clipboard(), "");
    ui.press("escape");

    ui.click("nushell");
    ui.click_menu_item("Run a Pipeline…");
    assert!(ui.read(|f| f.nu_panel_open()));
    ui.click("nushell");
    ui.click_menu_item("Hide Pipeline Panel");
    assert!(!ui.read(|f| f.nu_panel_open()));

    ui.select("Roadmap.md");
    ui.click("nushell");
    ui.click_menu_item("Copy as Nushell List");
    assert!(ui.clipboard().contains("Roadmap.md"), "{}", ui.clipboard());

    // Without nu this reports a problem instead of opening a terminal.
    ui.click("nushell");
    ui.click_menu_item("Open Nushell Here");
    assert!(!ui.menu_open());
}

#[gpui_kit::test]
fn actions_menu_items(cx: &mut TestAppContext) {
    let fx = Fixture::new("actions-menu");
    let mut ui = Ui::open(cx, &fx.dir);
    // These need a selection, so they're disabled and do nothing yet.
    for needs_selection in ["Quick Look", "Rename…", "Move to Trash"] {
        ui.click("actions");
        ui.click_menu_item(needs_selection);
        assert!(!ui.dialog_open(), "{needs_selection} without a selection");
        ui.press("escape");
    }

    ui.click("actions");
    ui.click_menu_item("Add Folder to Sidebar");
    assert!(ui.read(|f| f.favorites().iter().any(|p| p.path == fx.dir)));

    ui.select("Roadmap.md");
    ui.click("actions");
    ui.click_menu_item("Copy Path");
    assert_eq!(ui.clipboard(), fx.path("Roadmap.md").display().to_string());

    ui.click("actions");
    ui.click_menu_item(fff_finder_app::finder::reveal_label());
    assert_eq!(
        ui.system_calls(),
        [SystemCall::Reveal(fx.path("Roadmap.md"))]
    );

    for (item, close) in [
        ("Quick Look", "dialog-close"),
        ("Rename…", "rename-cancel"),
        ("Move to Trash", "cancel"),
        ("Go to Folder…", "goto-cancel"),
        ("Search Syntax…", "help-done"),
        ("Settings…", "settings-done"),
    ] {
        ui.click("actions");
        ui.click_menu_item(item);
        assert!(ui.dialog_open(), "{item} opens a dialog");
        if close == "dialog-close" {
            ui.close_dialog();
        } else {
            ui.click(close);
        }
        assert!(!ui.dialog_open(), "{close} closes the {item} dialog");
        assert!(
            ui.files_focused(),
            "focus returns to the files after {item}"
        );
    }
    assert!(fx.path("Roadmap.md").exists(), "cancel kept the file");

    ui.click("actions");
    ui.click_menu_item("New Folder");
    assert!(fx.path("untitled folder").is_dir());
    assert!(ui.dialog_open(), "a new folder opens Rename");
    ui.click("rename-cancel");
    assert!(ui.names().contains(&"untitled folder".to_string()));
}

#[gpui_kit::test]
fn every_menu_bar_action_has_a_handler(cx: &mut TestAppContext) {
    let fx = Fixture::new("menu-bar");
    let mut ui = Ui::open(cx, &fx.dir);
    ui.select("Roadmap.md");
    let actions: Vec<Box<dyn gpui_kit::Action>> = vec![
        ShowSettings.boxed_clone(),
        Rescan.boxed_clone(),
        NewFolder.boxed_clone(),
        OpenSelected.boxed_clone(),
        QuickLook.boxed_clone(),
        Rename.boxed_clone(),
        MoveToTrash.boxed_clone(),
        CloseWindow.boxed_clone(),
        CopyPath.boxed_clone(),
        CopyAsNuon.boxed_clone(),
        SelectAll.boxed_clone(),
        FocusSearch.boxed_clone(),
        SearchHelp.boxed_clone(),
        ViewIcons.boxed_clone(),
        ViewList.boxed_clone(),
        ViewColumns.boxed_clone(),
        ViewGallery.boxed_clone(),
        ToggleSidebar.boxed_clone(),
        ToggleHidden.boxed_clone(),
        ToggleAppearance.boxed_clone(),
        Refresh.boxed_clone(),
        GoBack.boxed_clone(),
        GoForward.boxed_clone(),
        GoUp.boxed_clone(),
        GoRecents.boxed_clone(),
        GoHome.boxed_clone(),
        GoToFolder.boxed_clone(),
        ToggleNuPanel.boxed_clone(),
        OpenInNushell.boxed_clone(),
    ];
    // Menus send actions to whatever has focus: the files, or a text field.
    for focus in ["files", "search"] {
        if focus == "search" {
            ui.click("search");
        }
        let missing: Vec<&str> = ui.window(|window, cx| {
            actions
                .iter()
                .filter(|a| !window.is_action_available(a.as_ref(), cx))
                .map(|a| a.name())
                .collect()
        });
        assert!(
            missing.is_empty(),
            "with focus in {focus}, no handler for {missing:?}"
        );
    }
}

// ----- search ----------------------------------------------------------------------

#[gpui_kit::test]
fn search_field_typing_escape_and_down_arrow(cx: &mut TestAppContext) {
    let fx = Fixture::new("search-field");
    let mut ui = Ui::open(cx, &fx.dir);
    ui.click("search");
    ui.type_text("atlas");
    assert_eq!(ui.read(|f| f.query().to_string()), "atlas");
    assert!(
        ui.names().contains(&"Project Atlas.txt".to_string()),
        "{:?}",
        ui.names()
    );
    assert!(ui.exists("scope-computer"), "the scope bar appears");

    ui.press("down");
    assert!(ui.files_focused(), "Down moves into the results");
    assert_eq!(ui.selected().len(), 1);

    ui.click("search");
    ui.press("escape");
    assert_eq!(ui.read(|f| f.query().to_string()), "");
    assert!(!ui.exists("scope-computer"));
    assert_eq!(ui.names().len(), 4);
}

#[gpui_kit::test]
fn scope_bar_controls(cx: &mut TestAppContext) {
    let fx = Fixture::new("scope-bar");
    let mut ui = Ui::open(cx, &fx.dir);
    ui.update(|f, _, cx| f.set_query("atlas".into(), true, cx));

    ui.click("scope-folder");
    assert_eq!(ui.read(|f| f.search_scope()), SearchScope::Folder);
    ui.click("scope-computer");
    assert_eq!(ui.read(|f| f.search_scope()), SearchScope::Computer);

    ui.click("by-contents");
    assert!(ui.read(|f| f.searches_contents()));
    ui.click("kind");
    assert!(!ui.menu_open(), "Kind is off for a contents search");
    ui.click("by-name");
    assert!(!ui.read(|f| f.searches_contents()));

    ui.click("kind");
    let labels: Vec<String> = KindFilter::ALL
        .iter()
        .map(|k| k.label().to_string())
        .collect();
    assert_eq!(ui.menu_labels(), labels);
    ui.click_menu_item("Folder");
    assert_eq!(ui.read(|f| f.config().kind_filter), KindFilter::Folder);
    assert_eq!(ui.names(), ["atlas"], "only folders match");
    ui.click("kind");
    ui.click_menu_item("Any Kind");
    assert_eq!(ui.read(|f| f.config().kind_filter), KindFilter::Any);

    ui.click("search-help");
    assert!(ui.dialog_open());
    ui.click("help-done");
    assert!(!ui.dialog_open());
}

// ----- sidebar, path bar, status bar ---------------------------------------------

#[gpui_kit::test]
fn sidebar_rows_and_menus(cx: &mut TestAppContext) {
    let fx = Fixture::new("sidebar");
    let mut ui = Ui::open(cx, &fx.dir);
    ui.click("recents");
    assert_eq!(ui.read(|f| f.location().clone()), Location::Recents);

    let projects = fx.path("Projects");
    ui.update(|f, _, cx| f.add_favorite(projects.clone(), cx));
    let ix = ui.read(|f| {
        f.favorites()
            .iter()
            .position(|p| p.path == projects)
            .unwrap()
    });
    ui.click(("fav", ix));
    assert_eq!(ui.folder(), Some(projects.clone()));

    if ui.exists(("loc", 0usize)) {
        ui.click(("loc", 0usize));
        assert!(ui.folder().is_some(), "a location opens a folder");
    }

    ui.right_click(("fav", ix));
    assert!(ui.menu_open());
    assert!(ui.menu_labels().contains(&"Open Nushell Here".to_string()));
    ui.click_menu_item("Remove from Sidebar");
    assert!(!ui.read(|f| f.favorites().iter().any(|p| p.path == projects)));

    ui.click("index-settings");
    assert!(ui.dialog_open(), "the gear opens Settings");
}

#[gpui_kit::test]
fn path_bar_crumbs_navigate(cx: &mut TestAppContext) {
    let fx = Fixture::new("crumbs");
    let mut ui = Ui::open(cx, &fx.dir);
    let atlas = fx.path("Projects/atlas");
    ui.go_to(&atlas);
    let crumbs = fff_finder_core::places::breadcrumbs(&atlas).len();
    ui.click(("crumb", crumbs - 2));
    assert_eq!(ui.folder(), Some(fx.path("Projects")));
}

#[gpui_kit::test]
fn status_bar_slider_resizes_icons(cx: &mut TestAppContext) {
    let fx = Fixture::new("slider");
    let mut ui = Ui::open(cx, &fx.dir);
    let before = ui.read(|f| f.icon_size_for_test());
    let bar = ui.snapshot("slider-bar-container").bounds();
    ui.click_at(
        "slider-bar-container",
        at(
            f32::from(bar.size.width) - 1.,
            f32::from(bar.size.height) / 2.,
        ),
    );
    let after = ui.read(|f| f.icon_size_for_test());
    assert!(after > before, "icon size {before} -> {after}");
    ui.click("view-list");
    assert!(
        !ui.exists("slider-bar-container"),
        "the slider is only for icons"
    );
}

// ----- views -----------------------------------------------------------------------

#[gpui_kit::test]
fn list_headers_sort(cx: &mut TestAppContext) {
    let fx = Fixture::new("headers");
    let mut ui = Ui::open(cx, &fx.dir);
    ui.click("view-list");
    for (header, key) in [
        ("Date Modified", SortKey::Modified),
        ("Size", SortKey::Size),
        ("Kind", SortKey::Kind),
        ("Name", SortKey::Name),
    ] {
        ui.click(header);
        assert_eq!(ui.read(|f| f.current_sort().key), key, "{header}");
    }
    let ascending = ui.read(|f| f.current_sort().ascending);
    ui.click("Name");
    assert_ne!(
        ui.read(|f| f.current_sort().ascending),
        ascending,
        "a second click flips"
    );
}

#[gpui_kit::test]
fn items_select_and_open_in_every_view(cx: &mut TestAppContext) {
    let fx = Fixture::new("items");
    let mut ui = Ui::open(cx, &fx.dir);
    let roadmap = fx.path("Roadmap.md");

    ui.click(item_id("tile", &roadmap));
    assert_eq!(ui.selected(), ["Roadmap.md"], "icons");

    ui.click("view-list");
    ui.click(item_id("row", &fx.path("notes.txt")));
    assert_eq!(ui.selected(), ["notes.txt"], "list");

    ui.click("view-gallery");
    ui.click(item_id("strip", &roadmap));
    assert_eq!(ui.selected(), ["Roadmap.md"], "gallery");
    assert!(
        ui.read(|f| f.has_preview_for_test(&roadmap)),
        "gallery loads the preview"
    );

    ui.click("view-columns");
    ui.click(item_id("crow", &fx.path("Projects")));
    assert_eq!(
        ui.read(|f| f.column_names_for_test()),
        vec![
            vec!["Budget.csv", "notes.txt", "Projects", "Roadmap.md"],
            vec!["atlas"]
        ]
    );
    ui.click(item_id("crow", &fx.path("Projects/atlas")));
    assert_eq!(ui.read(|f| f.column_names_for_test()).len(), 3);

    ui.click("view-list");
    ui.double_click(item_id("row", &fx.path("Projects")));
    assert_eq!(
        ui.folder(),
        Some(fx.path("Projects")),
        "double-click opens a folder"
    );
}

#[gpui_kit::test]
fn gallery_previews_an_item_with_nothing_selected(cx: &mut TestAppContext) {
    let fx = Fixture::new("gallery");
    let mut ui = Ui::open(cx, &fx.dir);
    assert!(ui.selected().is_empty());
    ui.click("view-gallery");
    assert_eq!(
        ui.selected(),
        ["Budget.csv"],
        "the gallery selects the first item"
    );
    assert!(
        ui.read(|f| f.has_preview_for_test(&fx.path("Budget.csv"))),
        "the preview finished loading instead of spinning"
    );
    // Escape clears the selection; the shown item still gets its preview.
    ui.press("escape");
    assert!(ui.read(|f| f.has_preview_for_test(&fx.path("Budget.csv"))));
}

#[gpui_kit::test]
fn context_menus_on_items_and_empty_space(cx: &mut TestAppContext) {
    let fx = Fixture::new("context-menus");
    let mut ui = Ui::open(cx, &fx.dir);
    ui.click("view-list");
    ui.right_click(item_id("row", &fx.path("notes.txt")));
    assert_eq!(ui.selected(), ["notes.txt"]);
    assert!(
        ui.menu_labels()
            .contains(&"Copy as Nushell List".to_string())
    );
    ui.click_menu_item("Copy Path");
    assert_eq!(ui.clipboard(), fx.path("notes.txt").display().to_string());

    ui.right_click(item_id("row", &fx.path("notes.txt")));
    ui.click_menu_item("Run Nushell Pipeline…");
    assert!(ui.read(|f| f.nu_panel_open()));
    ui.dispatch(ToggleNuPanel);

    // Empty space below the rows.
    let content = ui.snapshot("content").bounds();
    let below = at(40., f32::from(content.size.height) - 30.);
    ui.right_click_at("content", below);
    assert!(ui.menu_open(), "the background menu opens");
    assert!(!ui.menu_labels().contains(&"Copy Path".to_string()));
    ui.click_menu_item("Show Hidden Files");
    assert!(ui.read(|f| f.config().show_hidden));

    ui.right_click_at("content", below);
    ui.click_menu_item("Refresh");
    ui.right_click_at("content", below);
    ui.click_menu_item("New Folder");
    assert!(fx.path("untitled folder").is_dir());
    ui.click("rename-cancel");
}

// ----- dialogs ---------------------------------------------------------------------

#[gpui_kit::test]
fn settings_dialog_controls(cx: &mut TestAppContext) {
    let fx = Fixture::new("settings");
    let mut ui = Ui::open(cx, &fx.dir);
    ui.press(&cmd(","));
    assert!(ui.dialog_open());

    // The switch used to dispatch an action the dialog's focus could not
    // deliver, so clicking it did nothing.
    assert_eq!(ui.snapshot("show-hidden").checked(), Some(false));
    ui.click("show-hidden");
    assert!(ui.read(|f| f.config().show_hidden));
    assert_eq!(ui.snapshot("show-hidden").checked(), Some(true));
    assert!(ui.names().contains(&".secret".to_string()));
    ui.click("show-hidden");
    assert!(!ui.read(|f| f.config().show_hidden));

    for (id, appearance, dark) in [
        ("ap-dark", Appearance::Dark, true),
        ("ap-light", Appearance::Light, false),
        ("ap-system", Appearance::System, false),
    ] {
        ui.click(id);
        assert_eq!(ui.read(|f| f.config().appearance), appearance, "{id}");
        if appearance != Appearance::System {
            assert_eq!(ui.cx.update(|cx| cx.theme().is_dark()), dark, "{id}");
        }
    }

    ui.click("rescan");
    assert!(ui.dialog_open(), "Rescan keeps Settings open");

    // Add Folder… asks for folders with the system picker.
    ui.click("add-root");
    assert!(ui.cx.did_prompt_for_paths());
    let projects = fx.path("Projects");
    ui.cx
        .simulate_path_prompt_response(|_| Some(vec![projects.clone()]));
    ui.settle();
    assert_eq!(
        ui.read(|f| f.config().search_roots.clone()),
        vec![fx.dir.clone(), projects.clone()]
    );
    ui.click(("remove-root", 1usize));
    assert_eq!(
        ui.read(|f| f.config().search_roots.clone()),
        vec![fx.dir.clone()]
    );

    // These would index the whole disk or the home folder; check the
    // setting, then put the fixture back before the engine starts on it.
    let disk = if cfg!(windows) { "C:\\" } else { "/" };
    ui.click_without_settling("add-disk");
    assert_eq!(
        ui.read(|f| f.config().search_roots.clone()),
        vec![std::path::PathBuf::from(disk)]
    );
    let root = fx.dir.clone();
    ui.update(|f, _, cx| f.set_search_roots(vec![root.clone()], cx));
    ui.click_without_settling("reset-roots");
    assert_eq!(
        ui.read(|f| f.config().search_roots.clone()),
        fff_finder_core::config::default_search_roots()
    );
    ui.update(|f, _, cx| f.set_search_roots(vec![root.clone()], cx));

    ui.click("settings-done");
    assert!(!ui.dialog_open());
    assert!(ui.files_focused(), "Done returns focus to the files");

    ui.press(&cmd(","));
    ui.press("escape");
    assert!(!ui.dialog_open(), "Escape closes Settings");
    ui.press(&cmd(","));
    ui.close_dialog();
    assert!(!ui.dialog_open(), "the close button closes Settings");
}

#[gpui_kit::test]
fn settings_terminal_choice(cx: &mut TestAppContext) {
    let fx = Fixture::new("terminal-choice");
    let ghostty = Terminal {
        id: "ghostty".into(),
        name: "Ghostty".into(),
        location: Some("/Applications/Ghostty.app".into()),
        launch: Launch::Ghostty {
            bundle: "/Applications/Ghostty.app".into(),
        },
    };
    let other = Terminal {
        id: "terminal".into(),
        name: "Terminal".into(),
        location: Some("/usr/bin/term".into()),
        launch: Launch::Exec {
            program: "/usr/bin/term".into(),
            args: vec!["--cwd".into(), "{dir}".into(), "{nu}".into()],
        },
    };
    let nu = PathBuf::from("/opt/nu/bin/nu");
    let mut ui = Ui::open_with(
        cx,
        &fx.dir,
        Open {
            nu: Some(nu.clone()),
            terminals: vec![ghostty, other],
            ..Default::default()
        },
    );
    let dir = fx.dir.display().to_string();
    let last_launch = |ui: &Ui| match ui.system_calls().last() {
        Some(SystemCall::Terminal { program, args }) => {
            (program.display().to_string(), args.clone())
        }
        other => panic!("no terminal was started: {other:?}"),
    };
    let label = |ui: &mut Ui| ui.snapshot("terminal-app").label().map(str::to_string);

    ui.press(&cmd(","));
    assert_eq!(label(&mut ui).as_deref(), Some("Automatic (Ghostty)"));
    ui.click("terminal-app");
    assert_eq!(
        ui.menu_labels(),
        [
            "Automatic (Ghostty)",
            "Ghostty",
            "Terminal",
            "Custom Command",
            "Detect Again"
        ]
    );
    ui.click_menu_item("Terminal");
    assert_eq!(
        ui.read(|f| f.terminal_setting().app).as_deref(),
        Some("terminal")
    );
    assert_eq!(label(&mut ui).as_deref(), Some("Terminal"));
    ui.click("try-terminal");
    assert_eq!(
        last_launch(&ui),
        (
            "/usr/bin/term".into(),
            vec!["--cwd".into(), dir.clone(), nu.display().to_string()]
        )
    );

    // A custom command shows its field; words are split like a shell would.
    assert!(!ui.exists("terminal-input"));
    ui.click("terminal-app");
    ui.click_menu_item("Custom Command");
    ui.click("terminal-input");
    ui.type_text("foot -D {dir} {nu}");
    assert_eq!(
        ui.read(|f| f.terminal_setting().command).as_deref(),
        Some("foot -D {dir} {nu}")
    );
    ui.click("try-terminal");
    assert_eq!(
        last_launch(&ui),
        (
            "foot".into(),
            vec!["-D".into(), dir.clone(), nu.display().to_string()]
        )
    );

    // Automatic picks Ghostty, which goes through AppleScript; the custom
    // command is kept for later.
    ui.click("terminal-app");
    ui.click_menu_item("Automatic (Ghostty)");
    assert!(!ui.exists("terminal-input"));
    assert_eq!(ui.read(|f| f.terminal_setting().app), None);
    assert!(ui.read(|f| f.terminal_setting().command).is_some());
    ui.click("try-terminal");
    assert_eq!(last_launch(&ui).0, "/usr/bin/osascript");

    // A terminal that isn't installed any more falls back to Automatic.
    ui.update(|f, _, cx| f.set_terminal_app(Some("kitty".into()), cx));
    assert_eq!(label(&mut ui).as_deref(), Some("kitty (not installed)"));
    ui.click("try-terminal");
    assert_eq!(last_launch(&ui).0, "/usr/bin/osascript");

    ui.click("terminal-app");
    ui.click_menu_item("Detect Again");
    assert_eq!(ui.read(|f| f.terminals().map(<[Terminal]>::len)), Some(2));

    // The menu items and the shortcut use the same choice.
    ui.click("settings-done");
    ui.update(|f, _, cx| f.set_terminal_app(Some("terminal".into()), cx));
    ui.press(&cmd("alt-t"));
    assert_eq!(last_launch(&ui).1[1], dir);
    ui.click("nushell");
    ui.click_menu_item("Open Nushell Here");
    assert_eq!(last_launch(&ui).1[1], dir);
    let calls = ui.system_calls().len();
    assert_eq!(calls, 6, "one launch per request: {:?}", ui.system_calls());
}

#[gpui_kit::test]
fn go_to_folder_dialog(cx: &mut TestAppContext) {
    let fx = Fixture::new("goto");
    let mut ui = Ui::open(cx, &fx.dir);
    ui.press(&cmd("shift-g"));
    assert!(ui.dialog_open());
    // The field starts selected, so typing replaces it; Return goes.
    ui.type_text("Projects");
    ui.press("enter");
    assert!(!ui.dialog_open());
    assert_eq!(ui.folder(), Some(fx.path("Projects")));

    ui.press(&cmd("shift-g"));
    ui.type_text("does-not-exist");
    ui.click("goto-go");
    assert!(ui.dialog_open(), "a missing folder keeps the dialog open");
    ui.click("goto-cancel");
    assert!(!ui.dialog_open());

    ui.press(&cmd("shift-g"));
    ui.type_text(&fx.path("Roadmap.md").display().to_string());
    ui.click("goto-go");
    assert_eq!(ui.folder().as_deref(), Some(fx.dir.as_path()));
    assert_eq!(
        ui.selected(),
        ["Roadmap.md"],
        "a file opens its folder and selects it"
    );
}

#[gpui_kit::test]
fn rename_dialog(cx: &mut TestAppContext) {
    let fx = Fixture::new("rename");
    let mut ui = Ui::open(cx, &fx.dir);
    ui.select("Roadmap.md");
    ui.press("f2");
    assert!(ui.dialog_open());
    // Only the name before the extension is selected.
    ui.type_text("Plan");
    ui.click("rename-ok");
    assert!(!ui.dialog_open());
    assert!(fx.path("Plan.md").exists() && !fx.path("Roadmap.md").exists());
    assert!(ui.names().contains(&"Plan.md".to_string()));

    ui.select("Plan.md");
    ui.press("f2");
    ui.type_text("Later");
    ui.click("rename-cancel");
    assert!(fx.path("Plan.md").exists(), "Cancel leaves the name alone");

    ui.select("Plan.md");
    ui.press("f2");
    ui.press(&cmd("a"));
    ui.type_text("notes.txt");
    ui.press("enter");
    assert!(
        ui.dialog_open(),
        "renaming onto an existing name is refused"
    );
    ui.press("escape");
    assert!(fx.path("Plan.md").exists());
}

#[gpui_kit::test]
fn trash_confirmation_cancel(cx: &mut TestAppContext) {
    // Confirming would move a file into the real Trash, so only Cancel and
    // Escape are exercised here.
    let fx = Fixture::new("trash");
    let mut ui = Ui::open(cx, &fx.dir);
    ui.select("notes.txt");
    ui.press(&cmd("backspace"));
    assert!(ui.dialog_open());
    ui.click("cancel");
    assert!(!ui.dialog_open());
    ui.press(&cmd("backspace"));
    ui.press("escape");
    assert!(!ui.dialog_open());
    assert!(fx.path("notes.txt").exists());
}

#[gpui_kit::test]
fn quick_look_buttons_and_keys(cx: &mut TestAppContext) {
    let fx = Fixture::new("quick-look");
    let mut ui = Ui::open(cx, &fx.dir);
    ui.select("Roadmap.md");
    ui.press("space");
    assert!(ui.read(|f| f.quick_look_is_open()) && ui.dialog_open());
    assert!(ui.read(|f| f.has_preview_for_test(&fx.path("Roadmap.md"))));
    ui.press("space");
    assert!(!ui.dialog_open(), "Space closes it again");
    assert!(!ui.read(|f| f.quick_look_is_open()));

    ui.press("space");
    ui.click("ql-reveal");
    assert!(ui.dialog_open(), "Show in Finder keeps the preview open");
    ui.click("ql-open");
    assert!(!ui.dialog_open() && !ui.read(|f| f.quick_look_is_open()));
    let roadmap = fx.path("Roadmap.md");
    assert_eq!(
        ui.system_calls(),
        [
            SystemCall::Reveal(roadmap.clone()),
            SystemCall::Open(roadmap)
        ]
    );
    ui.press("space");
    assert!(ui.dialog_open(), "one Space reopens it after Open");
    ui.press("space");

    ui.press(&cmd("y"));
    ui.close_dialog();
    assert!(
        !ui.read(|f| f.quick_look_is_open()),
        "the close button resets the state"
    );
    ui.press("space");
    assert!(ui.dialog_open(), "and Space opens it again");
}

// ----- Nushell panel -----------------------------------------------------------------

#[gpui_kit::test]
fn nushell_panel_controls(cx: &mut TestAppContext) {
    let fx = Fixture::new("nu-panel");
    let nu = fff_finder_app::nushell::find_nu(None);
    let have_nu = nu.is_some();
    let mut ui = Ui::open_with(
        cx,
        &fx.dir,
        Open {
            nu,
            ..Default::default()
        },
    );
    ui.press("ctrl-`");
    assert!(ui.read(|f| f.nu_panel_open()));

    // An example fills the field and runs it.
    ui.click(("nu-example", 0usize));
    assert!(!ui.read(|f| f.nu_busy()));
    assert_eq!(
        ui.exists("nu-error"),
        !have_nu,
        "an error shows only without nu"
    );

    ui.click("nu-input");
    ui.press(&cmd("a"));
    ui.type_text("length");
    ui.press("enter");
    assert!(!ui.read(|f| f.nu_busy()));
    assert_eq!(ui.exists("nu-error"), !have_nu);
    if have_nu {
        assert!(ui.exists("nu-text"), "`length` prints a number");
    }

    ui.click("nu-run");
    assert!(!ui.read(|f| f.nu_busy()));
    ui.click("nu-close");
    assert!(!ui.read(|f| f.nu_panel_open()));
    assert!(ui.files_focused());
}

// ----- picker ------------------------------------------------------------------------

#[gpui_kit::test]
fn picker_bar_enables_choose_with_a_selection(cx: &mut TestAppContext) {
    // Choose and Cancel end the process, so only their state is checked.
    let fx = Fixture::new("picker");
    let mut ui = Ui::open_with(
        cx,
        &fx.dir,
        Open {
            picker: Some(PickerOptions { multiple: true }),
            ..Default::default()
        },
    );
    assert!(ui.exists("picker-cancel"));
    // With nothing selected Choose does nothing (with a selection it would
    // print the paths and exit).
    ui.click("picker-choose");
    ui.select("Roadmap.md");
    assert_eq!(ui.snapshot("picker-choose").label(), Some("Choose"));
    ui.select_also("notes.txt");
    assert_eq!(ui.snapshot("picker-choose").label(), Some("Choose 2 Items"));
}
