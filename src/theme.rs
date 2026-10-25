//! Kiesel-Tokens als Rust-Konstanten.
//!
//! Die Werte stammen 1:1 aus `project/tokens.json` des Kiesel-Design-Systems.
//! Farben gibt es in zwei Themes (hell/dunkel), alles andere ist themenunabhängig.

use iced::font::{Family, Weight};
use iced::theme::Palette;
use iced::{Color, Font, Theme};

/// Fensterbreite in Pixeln. Die Tab-Leiste berechnet daraus die Breite ihrer Tabs.
pub const WINDOW_W: f32 = 400.0;

// --- Abstände (space-1 … space-8) -------------------------------------------
pub const SPACE_1: f32 = 4.0;
pub const SPACE_2: f32 = 8.0;
pub const SPACE_3: f32 = 12.0;
pub const SPACE_4: f32 = 16.0;
pub const SPACE_5: f32 = 24.0;

// --- Radien ------------------------------------------------------------------
pub const RADIUS_SM: f32 = 6.0;
pub const RADIUS_MD: f32 = 10.0;
pub const RADIUS_LG: f32 = 16.0;

// --- Schriften ---------------------------------------------------------------
// Inter für Text, Inter Tight (nur 600) für Überschriften. Die Dateien liegen
// in `assets/fonts` und werden ins Binary eingebettet, damit nichts nachgeladen wird.
pub const FONT_FILES: [&[u8]; 4] = [
    include_bytes!("../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../assets/fonts/InterTight-SemiBold.ttf"),
];

pub const INTER: Font = Font {
    family: Family::Name("Inter"),
    weight: Weight::Normal,
    ..Font::DEFAULT
};
pub const INTER_MEDIUM: Font = Font {
    weight: Weight::Medium,
    ..INTER
};
pub const INTER_SEMIBOLD: Font = Font {
    weight: Weight::Semibold,
    ..INTER
};
pub const DISPLAY: Font = Font {
    family: Family::Name("Inter Tight"),
    weight: Weight::Semibold,
    ..Font::DEFAULT
};

// --- Farben ------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    pub mode: Mode,
    pub paper: Color,
    pub surface: Color,
    pub fog: Color,
    pub line: Color,
    pub control_border: Color,
    pub ink: Color,
    pub ink_muted: Color,
    pub accent: Color,
    pub on_accent: Color,
    pub accent_text: Color,
    pub sage: Color,
    pub on_sage: Color,
    pub moss: Color,
    pub clay: Color,
    pub on_clay: Color,
    pub clay_mark: Color,
}

fn hex(v: u32) -> Color {
    Color::from_rgb8((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

impl Tokens {
    pub fn light() -> Self {
        Self {
            mode: Mode::Light,
            paper: hex(0xF4F5F4),
            surface: hex(0xFFFFFF),
            fog: hex(0xE4E8E9),
            line: hex(0xDFE3E4),
            control_border: hex(0x7F8A92),
            ink: hex(0x22272B),
            ink_muted: hex(0x566068),
            accent: hex(0x5B6670),
            on_accent: hex(0xFFFFFF),
            accent_text: hex(0x4A5560),
            sage: hex(0xD5E0D6),
            on_sage: hex(0x1F3A2C),
            moss: hex(0x4F6F5C),
            clay: hex(0xF3E1D9),
            on_clay: hex(0x7A3419),
            clay_mark: hex(0x9A4A2E),
        }
    }

    pub fn dark() -> Self {
        Self {
            mode: Mode::Dark,
            paper: hex(0x15191C),
            surface: hex(0x1E2327),
            fog: hex(0x262D32),
            line: hex(0x2E363C),
            control_border: hex(0x6B7780),
            ink: hex(0xE8ECEE),
            ink_muted: hex(0x9CA8B0),
            accent: hex(0xA9B8C3),
            on_accent: hex(0x15191C),
            accent_text: hex(0xA9B8C3),
            sage: hex(0x22382C),
            on_sage: hex(0xBFDCCB),
            moss: hex(0x7FA891),
            clay: hex(0x3A2620),
            on_clay: hex(0xF0B8A3),
            clay_mark: hex(0xE59A7F),
        }
    }

    /// Das iced-Theme, das Standard-Widgets (Scrollleiste, Auswahl) mit den Kiesel-Farben versorgt.
    pub fn iced_theme(&self) -> Theme {
        let name = match self.mode {
            Mode::Light => "Kiesel Hell",
            Mode::Dark => "Kiesel Dunkel",
        };
        Theme::custom(
            name,
            Palette {
                background: self.paper,
                text: self.ink,
                primary: self.accent,
                success: self.moss,
                warning: self.accent,
                danger: self.clay_mark,
            },
        )
    }
}

// --- Farbhelfer --------------------------------------------------------------

pub fn alpha(c: Color, a: f32) -> Color {
    Color { a: c.a * a, ..c }
}

/// Legt `over` mit dem Anteil `amount` über `base` (wie die Hover-Überlagerung in Kiesel).
pub fn mix(base: Color, over: Color, amount: f32) -> Color {
    Color {
        r: base.r + (over.r - base.r) * amount,
        g: base.g + (over.g - base.g) * amount,
        b: base.b + (over.b - base.b) * amount,
        a: base.a,
    }
}

// Kontrast-Hilfen: nur für den Test, der die Kiesel-Paare prüft.
#[cfg(test)]
fn linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
fn luminance(c: Color) -> f32 {
    0.2126 * linear(c.r) + 0.7152 * linear(c.g) + 0.0722 * linear(c.b)
}

/// WCAG-Kontrastverhältnis zweier Farben.
#[cfg(test)]
pub fn contrast(a: Color, b: Color) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(name: &str, mode: &str, fg: Color, bg: Color, min: f32, failures: &mut Vec<String>) {
        let ratio = contrast(fg, bg);
        if ratio < min {
            failures.push(format!("{mode}: {name} = {ratio:.2}:1 (erwartet {min}:1)"));
        }
    }

    #[test]
    fn kiesel_pairs_meet_contrast() {
        let mut failures = Vec::new();
        for (mode, t) in [("hell", Tokens::light()), ("dunkel", Tokens::dark())] {
            // Text: mindestens 4,5:1
            for (bg_name, bg) in [("paper", t.paper), ("surface", t.surface), ("fog", t.fog)] {
                check(&format!("ink auf {bg_name}"), mode, t.ink, bg, 4.5, &mut failures);
                check(&format!("ink-muted auf {bg_name}"), mode, t.ink_muted, bg, 4.5, &mut failures);
            }
            check("on-accent auf accent", mode, t.on_accent, t.accent, 4.5, &mut failures);
            check("accent-text auf paper", mode, t.accent_text, t.paper, 4.5, &mut failures);
            check("accent-text auf surface", mode, t.accent_text, t.surface, 4.5, &mut failures);
            check("on-sage auf sage", mode, t.on_sage, t.sage, 4.5, &mut failures);
            check("on-clay auf clay", mode, t.on_clay, t.clay, 4.5, &mut failures);
            for (bg_name, bg) in [("paper", t.paper), ("surface", t.surface), ("clay", t.clay)] {
                check(&format!("clay-mark auf {bg_name}"), mode, t.clay_mark, bg, 4.5, &mut failures);
            }
            // Ränder, Fokusring, Marker: mindestens 3:1
            for (bg_name, bg) in [("paper", t.paper), ("surface", t.surface)] {
                check(&format!("control-border auf {bg_name}"), mode, t.control_border, bg, 3.0, &mut failures);
                check(&format!("accent (Fokusring) auf {bg_name}"), mode, t.accent, bg, 3.0, &mut failures);
            }
            check("moss auf sage", mode, t.moss, t.sage, 3.0, &mut failures);
        }
        assert!(failures.is_empty(), "Kontrast zu niedrig:\n{}", failures.join("\n"));
    }
}
