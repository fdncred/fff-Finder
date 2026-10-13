//! Terminal colors to styled text: the xterm palette, and a parser for the
//! SGR escapes `nu-highlight` writes.

use std::ops::Range;

use gpui_kit::{FontStyle, FontWeight, HighlightStyle, Hsla, Rgba, UnderlineStyle, px};

/// The standard xterm 256-color palette.
pub fn xterm_rgb(n: u8) -> (u8, u8, u8) {
    const BASE: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (205, 0, 0),
        (0, 205, 0),
        (205, 205, 0),
        (0, 0, 238),
        (205, 0, 205),
        (0, 205, 205),
        (229, 229, 229),
        (127, 127, 127),
        (255, 0, 0),
        (0, 255, 0),
        (255, 255, 0),
        (92, 92, 255),
        (255, 0, 255),
        (0, 255, 255),
        (255, 255, 255),
    ];
    match n {
        0..=15 => BASE[n as usize],
        16..=231 => {
            let i = n - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            (level(i / 36), level((i / 6) % 6), level(i % 6))
        }
        _ => {
            let v = 8 + (n - 232) * 10;
            (v, v, v)
        }
    }
}

pub fn rgb(r: u8, g: u8, b: u8) -> Hsla {
    Rgba {
        r: r as f32 / 255.,
        g: g as f32 / 255.,
        b: b as f32 / 255.,
        a: 1.,
    }
    .into()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Style {
    pub fg: Option<(u8, u8, u8)>,
    pub bg: Option<(u8, u8, u8)>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
}

impl Style {
    pub fn is_plain(&self) -> bool {
        *self == Style::default()
    }

    pub fn to_highlight(self) -> HighlightStyle {
        HighlightStyle {
            color: self.fg.map(|(r, g, b)| rgb(r, g, b)),
            background_color: self.bg.map(|(r, g, b)| rgb(r, g, b)),
            font_weight: self.bold.then_some(FontWeight::BOLD),
            font_style: self.italic.then_some(FontStyle::Italic),
            fade_out: self.dim.then_some(0.35),
            underline: self.underline.then(|| UnderlineStyle {
                thickness: px(1.),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// Apply one SGR sequence's parameters.
    fn apply(&mut self, params: &[u16]) {
        if params.is_empty() {
            *self = Style::default();
            return;
        }
        let mut i = 0;
        while i < params.len() {
            let p = params[i];
            match p {
                0 => *self = Style::default(),
                1 => self.bold = true,
                2 => self.dim = true,
                3 => self.italic = true,
                4 => self.underline = true,
                22 => {
                    self.bold = false;
                    self.dim = false;
                }
                23 => self.italic = false,
                24 => self.underline = false,
                30..=37 => self.fg = Some(xterm_rgb((p - 30) as u8)),
                90..=97 => self.fg = Some(xterm_rgb((p - 90 + 8) as u8)),
                39 => self.fg = None,
                40..=47 => self.bg = Some(xterm_rgb((p - 40) as u8)),
                100..=107 => self.bg = Some(xterm_rgb((p - 100 + 8) as u8)),
                49 => self.bg = None,
                38 | 48 => {
                    let color = match params.get(i + 1) {
                        Some(5) => {
                            let c = params.get(i + 2).map(|n| xterm_rgb(*n as u8));
                            i += 2;
                            c
                        }
                        Some(2) => {
                            let c = match (params.get(i + 2), params.get(i + 3), params.get(i + 4))
                            {
                                (Some(r), Some(g), Some(b)) => Some((*r as u8, *g as u8, *b as u8)),
                                _ => None,
                            };
                            i += 4;
                            c
                        }
                        _ => None,
                    };
                    if p == 38 {
                        self.fg = color;
                    } else {
                        self.bg = color;
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }
}

/// Text without escape sequences, and the styled byte ranges in it.
pub fn parse(input: &str) -> (String, Vec<(Range<usize>, Style)>) {
    let mut text = String::with_capacity(input.len());
    let mut spans: Vec<(Range<usize>, Style)> = Vec::new();
    let mut style = Style::default();
    let mut start = 0;
    let mut chars = input.char_indices().peekable();
    let mut flush = |text: &str, start: &mut usize, style: Style| {
        if *start < text.len() && !style.is_plain() {
            spans.push((*start..text.len(), style));
        }
        *start = text.len();
    };
    while let Some((_, c)) = chars.next() {
        if c != '\u{1b}' {
            text.push(c);
            continue;
        }
        match chars.peek().map(|(_, c)| *c) {
            // CSI: ESC [ params final-byte
            Some('[') => {
                chars.next();
                let mut seq = String::new();
                let mut fin = None;
                for (_, c) in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        fin = Some(c);
                        break;
                    }
                    seq.push(c);
                }
                if fin == Some('m') {
                    flush(&text, &mut start, style);
                    let params: Vec<u16> = if seq.is_empty() {
                        Vec::new()
                    } else {
                        seq.split([';', ':'])
                            .map(|p| p.parse().unwrap_or(0))
                            .collect()
                    };
                    style.apply(&params);
                }
            }
            // OSC: ESC ] … BEL or ESC \
            Some(']') => {
                chars.next();
                while let Some((_, c)) = chars.next() {
                    if c == '\u{7}' {
                        break;
                    }
                    if c == '\u{1b}' {
                        chars.next();
                        break;
                    }
                }
            }
            _ => {
                chars.next();
            }
        }
    }
    flush(&text, &mut start, style);
    (text, spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nu_highlight_output() {
        let (text, spans) = parse(
            "\u{1b}[1;36mdef\u{1b}[0m \u{1b}[38;2;255;176;0m{\u{1b}[0m x \u{1b}[38;5;48mok\u{1b}[39m",
        );
        assert_eq!(text, "def { x ok");
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].0, 0..3);
        assert!(spans[0].1.bold);
        assert_eq!(spans[0].1.fg, Some(xterm_rgb(6)));
        assert_eq!(
            spans[1],
            (
                4..5,
                Style {
                    fg: Some((255, 176, 0)),
                    ..Default::default()
                }
            )
        );
        assert_eq!(spans[2].0, 8..10);
        assert_eq!(spans[2].1.fg, Some(xterm_rgb(48)));
        // Other escapes are dropped.
        assert_eq!(parse("a\u{1b}]8;;http://x\u{7}b\u{1b}[2Kc").0, "abc");
        assert_eq!(xterm_rgb(196), (255, 0, 0));
        assert_eq!(xterm_rgb(244), (128, 128, 128));
    }
}
