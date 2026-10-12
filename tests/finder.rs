//! Browsing, searching, and every keyboard shortcut, driven through real key
//! presses in a headless window.

mod common;

use common::{Fixture, Ui, cmd};
use fff_finder_app::model::Location;
use fff_finder_core::ViewMode;
use gpui_kit::TestAppContext;
use gpui_kit::component::{ActiveTheme as _, WindowExt as _};

#[gpui_kit::test]
fn lists_navigates_and_goes_back(cx: &mut TestAppContext) {
    let fx = Fixture::new("nav");
    let mut ui = Ui::open(cx, &fx.dir);
    assert_eq!(
        ui.names(),
        ["Budget.csv", "notes.txt", "Projects", "Roadmap.md"]
    );

    ui.update(|f, window, cx| f.click_row(2, Default::default(), 2, window, cx));
    assert_eq!(ui.names(), ["atlas"]);

    ui.press(&cmd("["));
    assert_eq!(ui.folder().as_deref(), Some(fx.dir.as_path()));
    assert_eq!(
        ui.selected(),
        ["Projects"],
        "back selects the folder we left"
    );
    ui.press(&cmd("]"));
    assert_eq!(ui.folder(), Some(fx.path("Projects")));
    ui.press("alt-left");
    assert_eq!(ui.folder().as_deref(), Some(fx.dir.as_path()));
    ui.press("alt-right");
    ui.press(&cmd("up"));
    assert_eq!(
        ui.folder().as_deref(),
        Some(fx.dir.as_path()),
        "Cmd-Up goes to the parent"
    );
    assert_eq!(ui.selected(), ["Projects"]);

    // Cmd-Down, Cmd-O, and Return open the selection.
    ui.press(&cmd("down"));
    assert_eq!(ui.folder(), Some(fx.path("Projects")));
    ui.press(&cmd("up"));
    ui.press(&cmd("o"));
    assert_eq!(ui.folder(), Some(fx.path("Projects")));
    ui.press(&cmd("up"));
    ui.press("enter");
    assert_eq!(ui.folder(), Some(fx.path("Projects")));
}

#[gpui_kit::test]
fn go_shortcuts(cx: &mut TestAppContext) {
    let fx = Fixture::new("go-keys");
    let mut ui = Ui::open(cx, &fx.dir);
    ui.press(&cmd("shift-f"));
    assert_eq!(ui.read(|f| f.location().clone()), Location::Recents);
    ui.press(&cmd("shift-h"));
    assert_eq!(ui.folder(), dirs::home_dir());
}

#[gpui_kit::test]
fn searches_by_name_and_returns_to_browsing(cx: &mut TestAppContext) {
    let fx = Fixture::new("search");
    let mut ui = Ui::open(cx, &fx.dir);
    ui.press(&cmd("f"));
    assert!(
        ui.window(|window, cx| window.has_focused_input(cx)),
        "Cmd-F focuses search"
    );
    ui.type_text("atlas");
    let found = ui.names();
    assert!(
        found.contains(&"Project Atlas.txt".to_string()),
        "{found:?}"
    );
    assert!(
        found.contains(&"atlas".to_string()),
        "folders match too: {found:?}"
    );

    // Return moves into the results with the first one selected.
    ui.press("enter");
    assert!(ui.files_focused());
    assert_eq!(ui.selected().len(), 1);

    // Escape clears the selection, then the search.
    ui.press("escape");
    assert!(ui.selected().is_empty());
    ui.press("escape");
    assert_eq!(
        ui.names(),
        ["Budget.csv", "notes.txt", "Projects", "Roadmap.md"]
    );
}

#[gpui_kit::test]
fn arrow_keys_select_in_each_view(cx: &mut TestAppContext) {
    let fx = Fixture::new("arrows");
    let mut ui = Ui::open(cx, &fx.dir);

    ui.press(&cmd("2"));
    assert_eq!(ui.read(|f| f.effective_view()), ViewMode::List);
    ui.press("down");
    ui.press("shift-down");
    assert_eq!(ui.selected(), ["Budget.csv", "notes.txt"]);
    ui.press("up");
    assert_eq!(ui.selected(), ["Budget.csv"]);
    ui.press("end");
    assert_eq!(ui.selected(), ["Roadmap.md"]);
    ui.press("home");
    assert_eq!(ui.selected(), ["Budget.csv"]);
    ui.press("shift-down");
    ui.press("shift-down");
    ui.press("shift-up");
    assert_eq!(ui.selected(), ["Budget.csv", "notes.txt"]);
    ui.press(&cmd("a"));
    assert_eq!(ui.selected().len(), 4, "Cmd-A selects everything");
    ui.press("escape");
    assert!(ui.selected().is_empty());

    // Icons and the gallery move with Left and Right too.
    ui.press(&cmd("1"));
    assert_eq!(ui.read(|f| f.effective_view()), ViewMode::Icons);
    ui.press("right");
    ui.press("right");
    assert_eq!(ui.selected(), ["notes.txt"]);
    ui.press("shift-left");
    assert_eq!(ui.selected(), ["Budget.csv", "notes.txt"]);
    ui.press("shift-right");
    assert_eq!(ui.selected(), ["notes.txt"]);

    ui.press(&cmd("4"));
    assert_eq!(ui.read(|f| f.effective_view()), ViewMode::Gallery);
    ui.press("right");
    assert_eq!(ui.selected().len(), 1);

    // Columns: Right enters a folder, Left comes back.
    ui.press(&cmd("3"));
    assert_eq!(ui.read(|f| f.effective_view()), ViewMode::Columns);
    ui.update(|f, window, cx| f.select_in_column(0, 2, window, cx));
    assert_eq!(ui.read(|f| f.column_names_for_test()).len(), 2);
    ui.press("right");
    assert_eq!(ui.selected(), ["atlas"]);
    ui.press("left");
    assert_eq!(ui.selected(), ["Projects"]);
}

#[gpui_kit::test]
fn clipboard_shortcuts(cx: &mut TestAppContext) {
    let fx = Fixture::new("clipboard");
    let mut ui = Ui::open(cx, &fx.dir);
    ui.press(&cmd("2"));
    ui.press("down");
    ui.press("shift-down");
    let both = format!(
        "{}\n{}",
        fx.path("Budget.csv").display(),
        fx.path("notes.txt").display()
    );
    ui.press(&cmd("c"));
    assert_eq!(ui.clipboard(), both);
    ui.cx
        .write_to_clipboard(gpui_kit::ClipboardItem::new_string(String::new()));
    ui.press(&cmd("alt-c"));
    assert_eq!(ui.clipboard(), both);
    ui.press(&cmd("shift-c"));
    assert!(
        ui.clipboard().starts_with('['),
        "NUON list: {}",
        ui.clipboard()
    );
    assert!(ui.clipboard().contains("notes.txt"));
}

#[gpui_kit::test]
fn window_shortcuts(cx: &mut TestAppContext) {
    let fx = Fixture::new("window-keys");
    let mut ui = Ui::open(cx, &fx.dir);

    ui.press(&cmd("shift-."));
    assert!(ui.read(|f| f.config().show_hidden));
    assert!(ui.names().contains(&".secret".to_string()));
    ui.press(&cmd("shift-."));
    assert!(!ui.names().contains(&".secret".to_string()));

    ui.press(&cmd("alt-s"));
    assert!(!ui.read(|f| f.config().show_sidebar));
    ui.press(&cmd("alt-s"));
    assert!(ui.read(|f| f.config().show_sidebar));

    let dark = ui.cx.update(|cx| cx.theme().is_dark());
    ui.press(&cmd("shift-d"));
    assert_ne!(ui.cx.update(|cx| cx.theme().is_dark()), dark);

    std::fs::write(fx.path("new.txt"), "x").unwrap();
    ui.press(&cmd("r"));
    assert!(
        ui.names().contains(&"new.txt".to_string()),
        "Cmd-R refreshes"
    );

    ui.press("ctrl-`");
    assert!(ui.read(|f| f.nu_panel_open()));
    ui.press("ctrl-`");
    assert!(!ui.read(|f| f.nu_panel_open()));

    ui.press(&cmd("/"));
    assert!(ui.dialog_open(), "Cmd-/ shows the search syntax");
    ui.press("escape");

    // Without nu this warns instead of opening a terminal.
    ui.press(&cmd("alt-t"));
    assert!(!ui.dialog_open());
}

#[gpui_kit::test]
fn file_shortcuts(cx: &mut TestAppContext) {
    let fx = Fixture::new("file-keys");
    let mut ui = Ui::open(cx, &fx.dir);
    ui.press(&cmd("shift-n"));
    assert!(fx.path("untitled folder").is_dir());
    assert!(ui.dialog_open(), "a new folder opens Rename");
    ui.type_text("Archive");
    ui.press("enter");
    assert!(!ui.dialog_open());
    assert!(fx.path("Archive").is_dir());
    assert_eq!(ui.selected(), ["Archive"]);

    ui.select("notes.txt");
    ui.press("f2");
    assert!(ui.dialog_open());
    ui.press("escape");
    for key in [cmd("backspace"), "delete".to_string()] {
        ui.press(&key);
        assert!(ui.dialog_open(), "{key} asks before trashing");
        ui.press("escape");
    }
    ui.press("space");
    assert!(ui.read(|f| f.quick_look_is_open()));
    ui.press(&cmd("y"));
    assert!(!ui.read(|f| f.quick_look_is_open()));

    ui.press(&cmd("w"));
    let open = ui.cx.update(|cx| cx.windows().len());
    assert_eq!(open, 0, "Cmd-W closes the window");
}

#[gpui_kit::test]
fn type_into_the_search_field_and_down_arrow(cx: &mut TestAppContext) {
    let fx = Fixture::new("search-down");
    let mut ui = Ui::open(cx, &fx.dir);
    ui.click("search");
    ui.type_text("roadmap");
    ui.press("down");
    assert!(ui.files_focused());
    assert_eq!(ui.selected(), ["Roadmap.md"]);
    // Keys typed in the field edit text instead of moving the selection.
    ui.click("search");
    ui.press("left");
    ui.press("backspace");
    assert_eq!(ui.read(|f| f.query().to_string()), "roadmp");
}
