//! Print AppKit's semantic colors in light and dark mode (macOS only), to
//! check the bundled themes against the system.

#[cfg(target_os = "macos")]
fn main() {
    use objc2_app_kit::{
        NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSColor, NSColorSpace,
    };
    use objc2_foundation::NSArray;
    type Getter = fn() -> objc2::rc::Retained<NSColor>;
    let colors: &[(&str, Getter)] = &[
        ("windowBackground", NSColor::windowBackgroundColor),
        ("underPageBackground", NSColor::underPageBackgroundColor),
        ("controlBackground", NSColor::controlBackgroundColor),
        ("textBackground", NSColor::textBackgroundColor),
        ("label", NSColor::labelColor),
        ("secondaryLabel", NSColor::secondaryLabelColor),
        ("tertiaryLabel", NSColor::tertiaryLabelColor),
        ("quaternaryLabel", NSColor::quaternaryLabelColor),
        ("separator", NSColor::separatorColor),
        ("grid", NSColor::gridColor),
        (
            "selectedContentBackground",
            NSColor::selectedContentBackgroundColor,
        ),
        (
            "unemphasizedSelectedContentBackground",
            NSColor::unemphasizedSelectedContentBackgroundColor,
        ),
        (
            "selectedTextBackground",
            NSColor::selectedTextBackgroundColor,
        ),
        ("controlAccent", NSColor::controlAccentColor),
        ("systemBlue", NSColor::systemBlueColor),
        ("link", NSColor::linkColor),
        (
            "keyboardFocusIndicator",
            NSColor::keyboardFocusIndicatorColor,
        ),
        ("control", NSColor::controlColor),
        ("controlText", NSColor::controlTextColor),
        ("systemGray", NSColor::systemGrayColor),
        ("systemRed", NSColor::systemRedColor),
        ("systemGreen", NSColor::systemGreenColor),
        ("systemOrange", NSColor::systemOrangeColor),
        ("systemYellow", NSColor::systemYellowColor),
        ("findHighlight", NSColor::findHighlightColor),
        ("headerText", NSColor::headerTextColor),
        (
            "alternateSelectedControlText",
            NSColor::alternateSelectedControlTextColor,
        ),
        ("systemFill", NSColor::systemFillColor),
        ("secondarySystemFill", NSColor::secondarySystemFillColor),
        ("tertiarySystemFill", NSColor::tertiarySystemFillColor),
        ("quaternarySystemFill", NSColor::quaternarySystemFillColor),
    ];
    for (mode, name) in [
        ("light", unsafe { NSAppearanceNameAqua }),
        ("dark", unsafe { NSAppearanceNameDarkAqua }),
    ] {
        let appearance = NSAppearance::appearanceNamed(name).unwrap();
        let block = block2::RcBlock::new(|| {
            let srgb = NSColorSpace::sRGBColorSpace();
            let hex = |c: &NSColor| {
                let c = c.colorUsingColorSpace(&srgb).unwrap();
                let (r, g, b, a) = (
                    c.redComponent(),
                    c.greenComponent(),
                    c.blueComponent(),
                    c.alphaComponent(),
                );
                format!(
                    "#{:02X}{:02X}{:02X}{:02X}",
                    (r * 255.).round() as u8,
                    (g * 255.).round() as u8,
                    (b * 255.).round() as u8,
                    (a * 255.).round() as u8
                )
            };
            for (label, get) in colors {
                println!("{mode:5} {label:38} {}", hex(&get()));
            }
            let alternating: objc2::rc::Retained<NSArray<NSColor>> =
                NSColor::alternatingContentBackgroundColors();
            for (i, c) in alternating.iter().enumerate() {
                println!(
                    "{mode:5} alternatingContentBackground[{i}]        {}",
                    hex(&c)
                );
            }
        });
        appearance.performAsCurrentDrawingAppearance(&block);
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("macOS only");
}
