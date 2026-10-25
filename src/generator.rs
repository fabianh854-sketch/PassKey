//! Passworterzeugung. Reine Logik, keine GUI.

use rand::seq::{IndexedRandom, SliceRandom};
use rand::CryptoRng;
use secrecy::SecretString;
use zeroize::Zeroizing;

pub const MIN_LEN: u8 = 12;
pub const MAX_LEN: u8 = 32;
pub const DEFAULT_LEN: u8 = 20;

const UPPER: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const LOWER: &str = "abcdefghijklmnopqrstuvwxyz";
const DIGITS: &str = "0123456789";
const SYMBOLS: &str = "!#$%&*+-=?@^_~";
/// Zeichen, die leicht verwechselt werden.
const SIMILAR: &str = "0O1lI|";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub length: u8,
    pub upper: bool,
    pub lower: bool,
    pub digits: bool,
    pub symbols: bool,
    pub avoid_similar: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            length: DEFAULT_LEN,
            upper: true,
            lower: true,
            digits: true,
            symbols: true,
            avoid_similar: false,
        }
    }
}

impl Options {
    /// Die gewählten Zeichenarten als Zeichenlisten (ohne verwechselbare Zeichen, falls gewünscht).
    fn pools(&self) -> Vec<Vec<char>> {
        [
            (self.upper, UPPER),
            (self.lower, LOWER),
            (self.digits, DIGITS),
            (self.symbols, SYMBOLS),
        ]
        .into_iter()
        .filter(|(on, _)| *on)
        .map(|(_, set)| {
            set.chars()
                .filter(|c| !(self.avoid_similar && SIMILAR.contains(*c)))
                .collect()
        })
        .collect()
    }

    pub fn has_class(&self) -> bool {
        self.upper || self.lower || self.digits || self.symbols
    }

    pub fn length(&self) -> usize {
        usize::from(self.length.clamp(MIN_LEN, MAX_LEN))
    }

    /// Schätzung der Stärke in Bit: Länge × log2(Größe des Zeichenvorrats).
    pub fn entropy_bits(&self) -> f32 {
        let pool: usize = self.pools().iter().map(Vec::len).sum();
        if pool < 2 {
            return 0.0;
        }
        self.length() as f32 * (pool as f32).log2()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strength {
    Weak,
    Fair,
    Strong,
    VeryStrong,
}

impl Strength {
    pub fn from_bits(bits: f32) -> Self {
        match bits {
            b if b < 50.0 => Self::Weak,
            b if b < 80.0 => Self::Fair,
            b if b < 110.0 => Self::Strong,
            _ => Self::VeryStrong,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Weak => "Schwach",
            Self::Fair => "Solide",
            Self::Strong => "Stark",
            Self::VeryStrong => "Sehr stark",
        }
    }
}

/// Erzeugt ein Passwort. Aus jeder gewählten Zeichenart kommt mindestens ein Zeichen vor.
/// `None`, wenn keine Zeichenart gewählt ist.
///
/// `CryptoRng` lässt nur kryptografisch sichere Generatoren zu (in der App: `SysRng`).
pub fn generate<R: CryptoRng + ?Sized>(opts: &Options, rng: &mut R) -> Option<SecretString> {
    let pools = opts.pools();
    if pools.is_empty() {
        return None;
    }
    let all: Vec<char> = pools.iter().flatten().copied().collect();
    let len = opts.length();

    // Beide Puffer vorab passend groß: Ein wachsender Vec ließe beim Umlagern ungelöschte Kopien zurück.
    let mut chars: Zeroizing<Vec<char>> = Zeroizing::new(Vec::with_capacity(len));
    chars.extend(pools.iter().filter_map(|p| p.choose(rng).copied()));
    while chars.len() < len {
        chars.push(*all.choose(rng)?);
    }
    chars.shuffle(rng);

    // Alle Zeichen sind ASCII, ein Zeichen ist also ein Byte.
    let mut password = String::with_capacity(len);
    password.extend(chars.iter());
    Some(SecretString::from(password))
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;

    fn opts(length: u8) -> Options {
        Options {
            length,
            ..Options::default()
        }
    }

    #[test]
    fn length_stays_between_12_and_32() {
        let mut rng = rand::rng();
        for length in 0..=255u8 {
            let pw = generate(&opts(length), &mut rng).unwrap().expose_secret().to_owned();
            let n = pw.chars().count();
            assert!(
                (12..=32).contains(&n),
                "Länge {length} ergab {n} Zeichen"
            );
            assert_eq!(n, usize::from(length.clamp(12, 32)));
        }
        assert_eq!(generate(&opts(12), &mut rng).unwrap().expose_secret().to_owned().chars().count(), 12);
        assert_eq!(generate(&opts(32), &mut rng).unwrap().expose_secret().to_owned().chars().count(), 32);
    }

    #[test]
    fn every_selected_class_appears() {
        let mut rng = rand::rng();
        for length in MIN_LEN..=MAX_LEN {
            for _ in 0..50 {
                let pw = generate(&opts(length), &mut rng).unwrap().expose_secret().to_owned();
                assert!(pw.chars().any(|c| UPPER.contains(c)), "{}", pw);
                assert!(pw.chars().any(|c| LOWER.contains(c)), "{}", pw);
                assert!(pw.chars().any(|c| DIGITS.contains(c)), "{}", pw);
                assert!(pw.chars().any(|c| SYMBOLS.contains(c)), "{}", pw);
            }
        }
    }

    #[test]
    fn only_selected_classes_are_used() {
        let mut rng = rand::rng();
        let o = Options {
            symbols: false,
            digits: false,
            ..Options::default()
        };
        for _ in 0..200 {
            let pw = generate(&o, &mut rng).unwrap().expose_secret().to_owned();
            assert!(pw.chars().all(|c| c.is_ascii_alphabetic()), "{}", pw);
        }
    }

    #[test]
    fn similar_characters_are_avoided() {
        let mut rng = rand::rng();
        let o = Options {
            avoid_similar: true,
            ..Options::default()
        };
        for _ in 0..500 {
            let pw = generate(&o, &mut rng).unwrap().expose_secret().to_owned();
            assert!(!pw.chars().any(|c| SIMILAR.contains(c)), "{}", pw);
        }
    }

    #[test]
    fn no_class_means_no_password() {
        let mut rng = rand::rng();
        let o = Options {
            upper: false,
            lower: false,
            digits: false,
            symbols: false,
            ..Options::default()
        };
        assert!(generate(&o, &mut rng).is_none());
        assert_eq!(o.entropy_bits(), 0.0);
    }

    #[test]
    fn passwords_differ() {
        let mut rng = rand::rng();
        let a = generate(&Options::default(), &mut rng).unwrap().expose_secret().to_owned();
        let b = generate(&Options::default(), &mut rng).unwrap().expose_secret().to_owned();
        assert_ne!(a, b);
    }

    #[test]
    fn strength_grows_with_length() {
        let weak = Options {
            length: 12,
            upper: false,
            lower: false,
            symbols: false,
            ..Options::default()
        };
        assert_eq!(Strength::from_bits(weak.entropy_bits()), Strength::Weak);
        assert_eq!(
            Strength::from_bits(opts(32).entropy_bits()),
            Strength::VeryStrong
        );
        assert!(opts(32).entropy_bits() > opts(12).entropy_bits());
    }
}
