//! Drive the real browser window headlessly: listing, navigation, search,
//! keyboard selection, the column view, and copying paths.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use fff_finder_app::actions::{CopyPath, ExtendDown, MoveDown};
use fff_finder_app::finder::{AppState, Finder};
use fff_finder_app::model::Location;
use fff_finder_core::{Appearance, Engine, EngineOptions, ViewMode};
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext, WindowOptions};

struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Fixture {
        let dir = std::env::temp_dir().join(format!("fff-finder-ui-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Projects/atlas")).unwrap();
        std::fs::write(dir.join("Budget.csv"), "a,b\n1,2\n").unwrap();
        std::fs::write(dir.join("Roadmap.md"), "# Roadmap\n").unwrap();
        std::fs::write(dir.join("Projects/atlas/Project Atlas.txt"), "atlas").unwrap();
        isolate_settings();
        Fixture { dir }
    }
}

/// Point settings at one empty temp folder, once, before any window opens.
fn isolate_settings() {
    static CONFIG: OnceLock<PathBuf> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let config =
            std::env::temp_dir().join(format!("fff-finder-ui-config-{}", std::process::id()));
        std::fs::create_dir_all(&config).unwrap();
        // SAFETY: runs once, inside the OnceLock, before any test reads the variable.
        unsafe { std::env::set_var("FFFFINDER_CONFIG_DIR", &config) };
        config
    });
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn open(cx: &mut TestAppContext, dir: &Path) -> (AnyWindowHandle, Entity<Finder>) {
    let engine = Engine::new(EngineOptions {
        roots: vec![dir.to_path_buf()],
        db_dir: None,
        watch: false,
    });
    assert!(engine.wait_ready(Duration::from_secs(20)));
    cx.update(|cx| fff_finder_app::init(cx, AppState { engine, nu: None }, Appearance::Light));
    let start = Location::Folder(dir.to_path_buf());
    let (handle, view) = cx
        .update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, move |window, cx| {
                cx.new(|cx| Finder::new(start, None, None, window, cx))
            })
        })
        .unwrap();
    view.update(cx, |f, _| f.set_persist(false));
    settle(cx);
    (handle, view)
}

fn settle(cx: &mut TestAppContext) {
    for _ in 0..5 {
        cx.run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(100));
    }
    cx.run_until_parked();
}

fn names(view: &Entity<Finder>, cx: &TestAppContext) -> Vec<String> {
    view.read_with(cx, |f, _| {
        f.rows_for_test()
            .iter()
            .map(|r| r.entry.name.clone())
            .collect()
    })
}

#[gpui_kit::test]
fn lists_navigates_and_goes_back(cx: &mut TestAppContext) {
    let fx = Fixture::new("nav");
    let (handle, view) = open(cx, &fx.dir);
    assert_eq!(names(&view, cx), ["Budget.csv", "Projects", "Roadmap.md"]);

    // Double-click a folder to open it.
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |f, cx| {
            f.click_row(1, Default::default(), 2, window, cx)
        });
    })
    .unwrap();
    settle(cx);
    assert_eq!(names(&view, cx), ["atlas"]);

    // Back returns to the parent with the folder we left selected.
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |f, cx| {
            f.go_back(&fff_finder_app::actions::GoBack, window, cx)
        });
    })
    .unwrap();
    settle(cx);
    let selected = view.read_with(cx, |f, _| f.selected_entries());
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].name, "Projects");
}

#[gpui_kit::test]
fn searches_by_name_and_returns_to_browsing(cx: &mut TestAppContext) {
    let fx = Fixture::new("search");
    let (_, view) = open(cx, &fx.dir);
    view.update(cx, |f, cx| f.set_query("atlas".into(), true, cx));
    settle(cx);
    let found = names(&view, cx);
    assert!(
        found.contains(&"Project Atlas.txt".to_string()),
        "{found:?}"
    );
    assert!(
        found.contains(&"atlas".to_string()),
        "folders match too: {found:?}"
    );

    view.update(cx, |f, cx| f.set_query(String::new(), true, cx));
    settle(cx);
    assert_eq!(names(&view, cx), ["Budget.csv", "Projects", "Roadmap.md"]);
}

#[gpui_kit::test]
fn keyboard_selection_and_copy_path(cx: &mut TestAppContext) {
    let fx = Fixture::new("keys");
    let (handle, view) = open(cx, &fx.dir);
    // In the icon grid Down moves a whole row; the list moves one item.
    view.update(cx, |f, cx| f.set_view_mode(ViewMode::List, cx));
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |f, cx| f.focus_files(window, cx))
    })
    .unwrap();
    cx.dispatch_action(handle, MoveDown);
    cx.dispatch_action(handle, ExtendDown);
    settle(cx);
    let selected: Vec<String> = view.read_with(cx, |f, _| {
        f.selected_entries().into_iter().map(|e| e.name).collect()
    });
    assert_eq!(selected, ["Budget.csv", "Projects"]);

    cx.dispatch_action(handle, CopyPath);
    settle(cx);
    let copied = cx
        .read_from_clipboard()
        .and_then(|c| c.text())
        .unwrap_or_default();
    assert_eq!(
        copied,
        format!(
            "{}\n{}",
            fx.dir.join("Budget.csv").display(),
            fx.dir.join("Projects").display()
        )
    );
}

#[gpui_kit::test]
fn column_view_opens_folders_to_the_right(cx: &mut TestAppContext) {
    let fx = Fixture::new("columns");
    let (handle, view) = open(cx, &fx.dir);
    view.update(cx, |f, cx| f.set_view_mode(ViewMode::Columns, cx));
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |f, cx| f.select_in_column(0, 1, window, cx));
    })
    .unwrap();
    settle(cx);
    let columns = view.read_with(cx, |f, _| f.column_names_for_test());
    assert_eq!(
        columns,
        vec![vec!["Budget.csv", "Projects", "Roadmap.md"], vec!["atlas"]]
    );
}

#[gpui_kit::test]
fn toolbar_dropdown_opens_a_menu(cx: &mut TestAppContext) {
    let fx = Fixture::new("dropdown");
    let (handle, _) = open(cx, &fx.dir);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("popup-menu").is_none());
        window.click("sort", cx);
        window.render_frame(cx);
        window.render_frame(cx);
        assert!(
            window.try_find("popup-menu").is_some(),
            "sort menu did not open"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn right_click_selects_the_row_and_opens_its_menu(cx: &mut TestAppContext) {
    let fx = Fixture::new("context");
    let (handle, view) = open(cx, &fx.dir);
    view.update(cx, |f, cx| f.set_view_mode(ViewMode::List, cx));
    settle(cx);
    let row = (
        "row",
        fff_finder_app::finder::path_id(&fx.dir.join("Roadmap.md")),
    );
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.right_click(row, cx);
    })
    .unwrap();
    // The menu is built on the frame after the click.
    settle(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("popup-menu").is_some(),
            "context menu did not open"
        );
    })
    .unwrap();
    let selected: Vec<String> = view.read_with(cx, |f, _| {
        f.selected_entries().into_iter().map(|e| e.name).collect()
    });
    assert_eq!(selected, ["Roadmap.md"]);
}
