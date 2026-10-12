//! fffFinder: a Finder-style file search app on fff-search and GPUI Kit.
//! The binary is a thin wrapper around [`run`]; the library also backs the
//! headless screenshot tool in `examples/screenshot.rs`.

pub mod actions;
pub mod args;
pub mod assets;
pub mod finder;
pub mod model;
pub mod nushell;
pub mod picker;
pub mod theme;

use std::path::PathBuf;

use fff_finder_core::{Config, Engine, EngineOptions, config};
use gpui_kit::*;

use crate::finder::{AppState, Finder, PickerOptions};
use crate::model::Location;

/// Start indexing, open the first window, and run until it closes.
pub fn run(args: args::Args) {
    let config = Config::load();
    let start = start_location(args.path.as_ref(), &config);
    let picker = args.pick.then_some(PickerOptions {
        multiple: args.multiple,
    });
    // Indexing starts right away, in the background.
    let engine = Engine::new(EngineOptions {
        roots: config.search_roots.clone(),
        db_dir: Some(config::data_dir().join("gui")),
        watch: true,
    });
    let nu = nushell::find_nu(config.nu_path.as_deref());

    gpui_kit::application()
        .with_assets(assets::AppAssets)
        .with_quit_mode(QuitMode::LastWindowClosed)
        .run(move |cx: &mut App| {
            init(cx, AppState { engine, nu }, config.appearance);
            cx.on_action(move |_: &actions::Quit, cx: &mut App| {
                if picker.is_some() {
                    picker::cancel();
                }
                cx.quit();
            });
            cx.on_action(|_: &actions::NewWindow, cx: &mut App| {
                let config = Config::load();
                open_window(start_location(None, &config), None, None, cx);
            });
            open_window(start, args.query.clone(), picker, cx);
            cx.activate(true);
        });
}

/// Kit, theme, shared state, key bindings, and menus.
pub fn init(cx: &mut App, state: AppState, appearance: fff_finder_core::Appearance) {
    gpui_kit::init(cx);
    theme::install(cx);
    theme::apply(appearance, None, cx);
    cx.set_global(state);
    actions::bind_keys(cx);
    actions::set_menus(cx);
}

/// The folder to open: the argument, the last folder, or home.
pub fn start_location(arg: Option<&PathBuf>, config: &Config) -> Location {
    if let Some(path) = arg {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.clone());
        if path.is_dir() {
            return Location::Folder(path);
        }
        if let Some(parent) = path.parent().filter(|p| p.is_dir()) {
            return Location::Folder(parent.to_path_buf());
        }
    }
    config
        .last_folder
        .clone()
        .filter(|p| p.is_dir())
        .or_else(dirs::home_dir)
        .map(Location::Folder)
        .unwrap_or(Location::Recents)
}

pub fn open_window(
    start: Location,
    query: Option<String>,
    picker: Option<PickerOptions>,
    cx: &mut App,
) {
    let mac = cfg!(target_os = "macos");
    let title: SharedString = match picker {
        Some(PickerOptions { multiple: true }) => "Choose Files".into(),
        Some(_) => "Choose a File".into(),
        None => "fffFinder".into(),
    };
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(1100.), px(700.)), cx)),
        window_min_size: Some(size(px(850.), px(470.))),
        titlebar: Some(TitlebarOptions {
            title: Some(title.clone()),
            appears_transparent: mac,
            traffic_light_position: mac.then(|| point(px(18.), px(18.))),
        }),
        app_owns_titlebar_drag: mac,
        app_id: Some("fffFinder".into()),
        ..Default::default()
    };
    let result = gpui_kit::open_window(options, cx, move |window, cx| {
        window.set_window_title(&title);
        if picker.is_some() {
            window.on_window_should_close(cx, |_, _| picker::cancel());
        }
        let view = cx.new(|cx| Finder::new(start, query, picker, window, cx));
        let focus = view.read(cx).files_focus_handle();
        focus.focus(window, cx);
        view
    });
    if let Err(err) = result {
        eprintln!("fffFinder: could not open a window: {err}");
    }
}
