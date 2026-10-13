//! Open the real window (real platform, display link, and menus) on your real
//! index, replay steps through in-process mouse and keyboard events, and print
//! the window's state after each one. The watchdog reports a hang with thread
//! stacks. Settings are never written.
//!
//! ```text
//! cargo run --example drive -- key:cmd-, click:show-hidden key:escape \
//!     key:cmd-shift-g type:~/src key:enter click:view-gallery wait:2000
//! ```
//!
//! Steps: `key:<keys>` presses keys (`cmd-shift-g`, `enter`), `click:<id>`
//! clicks an element by ID, `type:<text>` types into the focused field, and
//! `wait:<ms>` pauses.

use fff_Finder::finder::AppState;
use fff_Finder::model::Location;
use fff_Finder_core::{Config, Engine, EngineOptions};
use gpui_kit::component::WindowExt as _;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::*;
use std::time::Duration;

fn main() {
    let steps: Vec<String> = std::env::args().skip(1).collect();
    let config = Config::load();
    let engine = Engine::new(EngineOptions {
        roots: config.search_roots.clone(),
        db_dir: Some(fff_Finder_core::config::data_dir().join("gui")),
        watch: true,
        search: Default::default(),
    });
    let nu = fff_Finder::nushell::find_nu(config.nu_path.as_deref());
    let start = fff_Finder::start_location(None, &config);

    gpui_kit::application()
        .with_assets(fff_Finder::assets::AppAssets)
        .run(move |cx: &mut App| {
            fff_Finder::init(cx, AppState { engine, nu, terminals: None }, config.appearance);
            fff_Finder::watchdog::start(cx);
            cx.on_action(|_: &fff_Finder::actions::Quit, cx: &mut App| cx.quit());
            let Some((handle, view)) = fff_Finder::open_window(start, None, None, cx) else {
                return;
            };
            view.update(cx, |f, _| f.set_persist(false));
            cx.activate(true);

            cx.spawn(async move |cx| {
                let executor = cx.background_executor().clone();
                // Give the first scan a head start, as a person would.
                executor.timer(Duration::from_secs(5)).await;
                for step in steps {
                    let (kind, arg) = step.split_once(':').unwrap_or((step.as_str(), ""));
                    if kind == "wait" {
                        let ms = arg.parse().unwrap_or(1000);
                        executor.timer(Duration::from_millis(ms)).await;
                        continue;
                    }
                    let arg = arg.to_string();
                    cx.update_window(handle, |_, window, cx| match kind {
                        "key" => window.press(&arg, cx),
                        "click" => window.click(SharedString::from(arg), cx),
                        "type" => window.input(&arg, cx),
                        other => eprintln!("unknown step kind {other:?}"),
                    })
                    .ok();
                    executor.timer(Duration::from_millis(800)).await;
                    let state = cx
                        .update_window(handle, |_, window, cx| {
                            let dialog = window.has_active_dialog(cx);
                            let f = view.read(cx);
                            let place = match f.location() {
                                Location::Folder(p) => p.display().to_string(),
                                Location::Recents => "Recents".into(),
                                Location::Results { title, .. } => title.clone(),
                            };
                            format!(
                                "view={:?} hidden={} dialog={} files-focused={} rows={} selected={} in {place}",
                                f.effective_view(),
                                f.config().show_hidden,
                                dialog,
                                f.files_focused(window),
                                f.rows_for_test().len(),
                                f.selected_entries().len(),
                            )
                        })
                        .unwrap_or_else(|_| "window closed".into());
                    println!("{step:<28} {state}");
                }
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
}
