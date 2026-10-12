//! Theme installation and the few app colors the kit theme has no slot for.

use fff_finder_core::Appearance;
use gpui_kit::component::{ActiveTheme as _, Theme, ThemeMode, ThemeRegistry};
use gpui_kit::{Anchor, App, Hsla, Rgba, Window, px, rgba};

pub fn install(cx: &mut App) {
    if let Err(err) = ThemeRegistry::global_mut(cx).load_themes_from_str(crate::assets::THEMES) {
        log::warn!("theme: {err}");
    }
    let registry = ThemeRegistry::global(cx);
    let light = registry.themes().get("fffFinder Light").cloned();
    let dark = registry.themes().get("fffFinder Dark").cloned();
    Theme::update(cx, |theme| {
        if let Some(light) = &light {
            theme.light_theme = light.clone();
        }
        if let Some(dark) = &dark {
            theme.dark_theme = dark.clone();
        }
        // Toasts default to the top right, over the search field and the
        // toolbar menus; put them above the path and status bars instead.
        theme.notification.placement = Anchor::BottomRight;
        theme.notification.margins.bottom = px(68.);
    });
}

/// Apply the configured appearance; `System` follows the window.
pub fn apply(appearance: Appearance, window: Option<&mut Window>, cx: &mut App) {
    match appearance {
        Appearance::System => Theme::sync_system_appearance(window, cx),
        Appearance::Light => Theme::change(ThemeMode::Light, None, cx),
        Appearance::Dark => Theme::change(ThemeMode::Dark, None, cx),
    }
}

fn c(hex: u32) -> Hsla {
    let rgba: Rgba = rgba(hex);
    rgba.into()
}

/// Finder-specific surfaces, matched to the bundled light/dark themes.
#[derive(Clone, Copy)]
pub struct Palette {
    pub stripe: Hsla,
    pub pill: Hsla,
    pub pill_border: Hsla,
    pub pill_hover: Hsla,
    pub pill_selected: Hsla,
    pub selection: Hsla,
    pub selection_inactive: Hsla,
    pub selection_text: Hsla,
    pub tile_selected: Hsla,
    pub sidebar_icon: Hsla,
    pub sidebar_heading: Hsla,
    pub faint: Hsla,
    pub highlight: Hsla,
    pub badge_text: Hsla,
}

pub fn palette(cx: &App) -> Palette {
    if cx.theme().is_dark() {
        Palette {
            stripe: c(0xFFFFFF0A),
            pill: c(0x2D3E4CFF),
            pill_border: c(0xFFFFFF14),
            pill_hover: c(0x364959FF),
            pill_selected: c(0x4F79ADFF),
            selection: c(0x0058D0FF),
            selection_inactive: c(0x3A4554FF),
            selection_text: c(0xFFFFFFFF),
            tile_selected: c(0xFFFFFF1F),
            sidebar_icon: c(0x3D8EF0FF),
            sidebar_heading: c(0x8E99A7FF),
            faint: c(0x6B7685FF),
            highlight: c(0xFFD60A2E),
            badge_text: c(0x5B6573FF),
        }
    } else {
        Palette {
            stripe: c(0xF4F5F7FF),
            pill: c(0xFFFFFFFF),
            pill_border: c(0x0000001A),
            pill_hover: c(0xF2F2F4FF),
            pill_selected: c(0xD7E6FBFF),
            selection: c(0x0063E1FF),
            selection_inactive: c(0xDCDCE1FF),
            selection_text: c(0xFFFFFFFF),
            tile_selected: c(0x0000001A),
            sidebar_icon: c(0x007AFFFF),
            sidebar_heading: c(0x8A8A8EFF),
            faint: c(0xA1A1A6FF),
            highlight: c(0xFFCC0040),
            badge_text: c(0x6E6E73FF),
        }
    }
}
