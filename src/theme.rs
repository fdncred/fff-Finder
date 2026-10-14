//! Theme installation and the few app colors the kit theme has no slot for.

use fffinder_core::Appearance;
use gpui_kit::component::{ActiveTheme as _, Theme, ThemeMode, ThemeRegistry};
use gpui_kit::{Anchor, App, Global, Hsla, Pixels, Rgba, Window, px, rgba};
use std::cell::Cell;

thread_local! {
    /// Settings › Text size; windows render on one thread.
    static UI_SCALE: Cell<f32> = const { Cell::new(1.0) };
    /// The theme's own font sizes (sans, mono), before scaling.
    static BASE_FONTS: Cell<Option<(Pixels, Pixels)>> = const { Cell::new(None) };
    /// The sizes last set here, to tell them from a theme's own.
    static SCALED_FONTS: Cell<Option<(Pixels, Pixels)>> = const { Cell::new(None) };
}

/// `n` pixels at the window's text size: the app's layout and text use this
/// instead of `px`, so Settings › Text size scales everything together.
pub fn ui(n: f32) -> Pixels {
    px(n * ui_scale())
}

/// Settings › Text size as a factor; 1.0 is normal.
pub fn ui_scale() -> f32 {
    UI_SCALE.with(Cell::get)
}

/// Scale every window's text and controls. The kit's own controls are sized
/// from the theme's font size, so that is scaled to match.
pub fn set_ui_scale(scale: f32, cx: &mut App) {
    UI_SCALE.with(|s| s.set(scale));
    scale_theme_fonts(cx);
}

fn scale_theme_fonts(cx: &mut App) {
    let (sans, mono) = BASE_FONTS.with(Cell::get).unwrap_or_else(|| {
        let theme = cx.theme();
        (theme.font_size, theme.mono_font_size)
    });
    let scale = ui_scale();
    BASE_FONTS.with(|b| b.set(Some((sans, mono))));
    SCALED_FONTS.with(|f| f.set(Some((sans * scale, mono * scale))));
    Theme::update(cx, |theme| {
        theme.font_size = sans * scale;
        theme.mono_font_size = mono * scale;
        // Keep toasts above the path and status bars, which scale too.
        theme.notification.margins.bottom = ui(68.);
    });
}

/// The accent and selection colors chosen in macOS System Settings.
pub struct SystemAccent(pub crate::macos::SystemColors);

impl Global for SystemAccent {}

/// `0xRRGGBBAA` as `#RRGGBB`.
fn hex6(rgba: u32) -> String {
    format!("#{:06X}", rgba >> 8)
}

/// The bundled themes use `#007AFF` (with any alpha) for the accent and
/// Finder's default selection colors; swap in the ones macOS reports. Named
/// blues, info, and links keep their colors, as they do in macOS.
fn with_system_colors(themes: &str, colors: crate::macos::SystemColors) -> String {
    let Ok(mut json) = serde_json::from_str::<serde_json::Value>(themes) else {
        return themes.to_string();
    };
    let accent = hex6(colors.accent);
    let themes_list = json.get_mut("themes").and_then(|t| t.as_array_mut());
    for theme in themes_list.into_iter().flatten() {
        let dark = theme.get("mode").and_then(|m| m.as_str()) == Some("dark");
        let selection = hex6(if dark {
            colors.selection_dark
        } else {
            colors.selection_light
        });
        let Some(map) = theme.get_mut("colors").and_then(|c| c.as_object_mut()) else {
            continue;
        };
        for (key, value) in map.iter_mut() {
            if ["base.", "info.", "link."]
                .iter()
                .any(|p| key.starts_with(p))
            {
                continue;
            }
            let Some(hex) = value.as_str() else { continue };
            let new = if key == "table.active.background" {
                selection.clone()
            } else if let Some(alpha) = hex.strip_prefix("#007AFF") {
                format!("{accent}{alpha}")
            } else {
                continue;
            };
            *value = new.into();
        }
    }
    json.to_string()
}

pub fn install(cx: &mut App) {
    let system = crate::macos::system_colors();
    let themes = match system {
        Some(colors) => with_system_colors(crate::assets::THEMES, colors),
        None => crate::assets::THEMES.to_string(),
    };
    if let Err(err) = ThemeRegistry::global_mut(cx).load_themes_from_str(&themes) {
        log::warn!("theme: {err}");
    }
    if let Some(colors) = system {
        cx.set_global(SystemAccent(colors));
    }
    let registry = ThemeRegistry::global(cx);
    let light = registry.themes().get("fffinder Light").cloned();
    let dark = registry.themes().get("fffinder Dark").cloned();
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
        theme.notification.margins.bottom = ui(68.);
    });
}

/// Apply the configured appearance; `System` follows the window.
pub fn apply(appearance: Appearance, window: Option<&mut Window>, cx: &mut App) {
    match appearance {
        Appearance::System => Theme::sync_system_appearance(window, cx),
        Appearance::Light => Theme::change(ThemeMode::Light, None, cx),
        Appearance::Dark => Theme::change(ThemeMode::Dark, None, cx),
    }
    // A size the theme set itself becomes the base; one still at what was
    // set here was not reloaded, so its base stays. Sans and mono separately,
    // since a theme file may set only one.
    let theme = cx.theme();
    let now = (theme.font_size, theme.mono_font_size);
    let base = match (BASE_FONTS.with(Cell::get), SCALED_FONTS.with(Cell::get)) {
        (Some(base), Some(scaled)) => (
            if now.0 == scaled.0 { base.0 } else { now.0 },
            if now.1 == scaled.1 { base.1 } else { now.1 },
        ),
        _ => now,
    };
    BASE_FONTS.with(|b| b.set(Some(base)));
    scale_theme_fonts(cx);
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
    let system = cx.try_global::<SystemAccent>().map(|a| a.0);
    let accent = system.map_or(c(0x007AFFFF), |s| c(s.accent | 0xFF));
    if cx.theme().is_dark() {
        Palette {
            stripe: c(0xFFFFFF0C),
            pill: c(0xFFFFFF14),
            pill_border: c(0xFFFFFF1A),
            pill_hover: c(0xFFFFFF1F),
            pill_selected: c(0xFFFFFF29),
            selection: system.map_or(c(0x0059D1FF), |s| c(s.selection_dark | 0xFF)),
            selection_inactive: c(0x464646FF),
            selection_text: c(0xFFFFFFFF),
            tile_selected: c(0xFFFFFF1F),
            sidebar_icon: accent,
            sidebar_heading: c(0xFFFFFF59),
            faint: c(0xFFFFFF3F),
            highlight: c(0xFFD60A40),
            badge_text: c(0xFFFFFF8C),
        }
    } else {
        Palette {
            stripe: c(0xF4F5F5FF),
            pill: c(0xFFFFFFFF),
            pill_border: c(0x00000017),
            pill_hover: c(0x0000000A),
            pill_selected: c(0x00000014),
            selection: system.map_or(c(0x0064E1FF), |s| c(s.selection_light | 0xFF)),
            selection_inactive: c(0xDCDCDCFF),
            selection_text: c(0xFFFFFFFF),
            tile_selected: c(0x00000017),
            sidebar_icon: accent,
            sidebar_heading: c(0x0000004D),
            faint: c(0x00000042),
            highlight: c(0xFFCC0059),
            badge_text: c(0x00000080),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::with_system_colors;
    use crate::macos::SystemColors;

    #[test]
    fn system_accent_replaces_only_accent_colors() {
        let pink = SystemColors {
            accent: 0xFF2D55FF,
            selection_light: 0xE0234AFF,
            selection_dark: 0xC8183CFF,
        };
        let themes = with_system_colors(crate::assets::THEMES, pink);
        let json: serde_json::Value = serde_json::from_str(&themes).unwrap();
        let color = |theme: usize, key: &str| {
            json["themes"][theme]["colors"][key]
                .as_str()
                .unwrap()
                .to_string()
        };
        for theme in [0, 1] {
            assert_eq!(color(theme, "primary.background"), "#FF2D55");
            assert_eq!(color(theme, "base.blue"), "#007AFF");
            assert_eq!(color(theme, "info.background"), "#007AFF");
        }
        assert_eq!(color(0, "list.active.background"), "#FF2D551F");
        assert_eq!(color(0, "table.active.background"), "#E0234A");
        assert_eq!(color(1, "table.active.background"), "#C8183C");
        assert_eq!(color(0, "link.hover.foreground"), "#007AFF");
    }
}
