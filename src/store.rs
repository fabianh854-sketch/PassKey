//! Verschlüsselte Verlaufsdatei (Standard: `%APPDATA%\PassKey\verlauf.pkv`).
//!
//! Argon2id leitet aus dem Master-Passwort einen 256-Bit-Schlüssel ab, XChaCha20-Poly1305
//! verschlüsselt den Inhalt. Es gibt keine Wiederherstellung: Ohne Passwort bleibt die Datei zu.
//!
//! Aufbau (Zahlen little-endian):
//!
//! | Byte | Länge | Inhalt                                   |
//! |------|-------|------------------------------------------|
//! | 0    | 8     | Kennung `PASSKEY\0`                      |
//! | 8    | 1     | Format-Version ([`VERSION`])             |
//! | 9    | 12    | Argon2id: Speicher (KiB), Durchläufe, Lanes |
//! | 21   | 16    | Salt                                     |
//! | 37   | 24    | Nonce                                    |
//! | 61   | …     | Geheimtext mit 16 Byte Tag               |
//!
//! Der Kopf (Byte 0 bis 60) geht als zusätzliche authentifizierte Daten in die Verschlüsselung
//! ein. Wer ein Byte darin ändert, lässt das Entschlüsseln scheitern, genau wie beim Geheimtext.
//!
//! Klartext: `u32` Anzahl, dann pro Eintrag `i64` Zeitpunkt (Unix-Sekunden), `u8` Länge,
//! `u8` Schalter-Bits, `u16` + Etikett (UTF-8), `u16` + Passwort (UTF-8).
//!
//! Geschrieben wird atomar: erst eine Temp-Datei daneben, dann Umbenennen. Ein Absturz
//! beim Speichern lässt die alte Datei heil.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand::Rng;
use zeroize::Zeroizing;

use crate::generator::Options;

const MAGIC: &[u8; 8] = b"PASSKEY\0";
pub const VERSION: u8 = 1;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;
const HEADER_LEN: usize = 8 + 1 + 12 + SALT_LEN + NONCE_LEN;
const TAG_LEN: usize = 16;
const KEY_LEN: usize = 32;

/// Kosten der Schlüsselableitung. Sie stehen im Kopf, damit spätere Versionen sie erhöhen
/// können, ohne alte Dateien unlesbar zu machen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KdfParams {
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
}

impl KdfParams {
    /// 64 MiB, 3 Durchläufe, 1 Lane (RFC 9106, zweite Empfehlung, auf einen Kern zugeschnitten).
    /// Mit optimiertem Argon2 dauert das Entsperren etwa eine halbe Sekunde.
    pub const DEFAULT: Self = Self { m_kib: 64 * 1024, t: 3, p: 1 };

    /// Obergrenzen beim Lesen. Argon2 läuft vor der Prüfung des Tags, ein manipulierter Kopf
    /// könnte sonst beliebig viel Speicher oder Zeit anfordern.
    const MAX_M_KIB: u32 = 1024 * 1024;
    const MAX_T: u32 = 64;
    const MAX_P: u32 = 16;

    fn plausible(&self) -> bool {
        self.p >= 1
            && self.p <= Self::MAX_P
            && self.t >= 1
            && self.t <= Self::MAX_T
            && self.m_kib >= 8 * self.p
            && self.m_kib <= Self::MAX_M_KIB
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Passwort falsch oder Datei verändert. AEAD kann beides nicht unterscheiden.
    WrongPassword,
    /// Kopf oder Inhalt passen nicht zum Format (fremde Datei, abgeschnitten, neuere Version).
    Corrupt,
    /// Beim Anlegen liegt dort schon eine Datei. Sie wird nie überschrieben.
    Exists,
    Io(io::ErrorKind),
    /// Schlüsselableitung oder Hintergrund-Thread ist gescheitert.
    Internal,
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e.kind())
    }
}

pub struct Entry {
    pub label: Zeroizing<String>,
    pub password: Zeroizing<String>,
    /// Zeitpunkt des Speicherns in Unix-Sekunden (UTC).
    pub saved_at: i64,
    pub options: Options,
}

/// Eine entsperrte Datei: Schlüssel und Einträge liegen im RAM, bis sie fallen gelassen wird.
/// `Zeroizing` überschreibt beides beim Freigeben.
pub struct Vault {
    path: PathBuf,
    params: KdfParams,
    salt: [u8; SALT_LEN],
    key: Zeroizing<[u8; KEY_LEN]>,
    entries: Vec<Entry>,
}

impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Nie Schlüssel oder Passwörter ausgeben.
        f.debug_struct("Vault")
            .field("path", &self.path)
            .field("entries", &self.entries.len())
            .finish_non_exhaustive()
    }
}

/// Standardpfad: `%APPDATA%\PassKey\verlauf.pkv`.
pub fn default_path() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("PassKey").join("verlauf.pkv")
}

impl Vault {
    /// Legt eine neue, leere Datei an. Langsam (Argon2), daher im Hintergrund aufrufen.
    pub fn create(path: &Path, password: &str, params: KdfParams) -> Result<Self, Error> {
        if path.exists() {
            return Err(Error::Exists);
        }
        let mut salt = [0u8; SALT_LEN];
        rand::rng().fill_bytes(&mut salt);
        let key = derive_key(password, &salt, params)?;
        let vault = Vault {
            path: path.to_owned(),
            params,
            salt,
            key,
            entries: Vec::new(),
        };
        vault.write()?;
        Ok(vault)
    }

    /// Öffnet und entschlüsselt eine Datei. Langsam (Argon2), daher im Hintergrund aufrufen.
    pub fn open(path: &Path, password: &str) -> Result<Self, Error> {
        let bytes = fs::read(path)?;
        if bytes.len() < HEADER_LEN + TAG_LEN || &bytes[..8] != MAGIC {
            return Err(Error::Corrupt);
        }
        if bytes[8] != VERSION {
            return Err(Error::Corrupt);
        }
        let params = KdfParams {
            m_kib: u32_at(&bytes, 9),
            t: u32_at(&bytes, 13),
            p: u32_at(&bytes, 17),
        };
        if !params.plausible() {
            return Err(Error::Corrupt);
        }
        let salt: [u8; SALT_LEN] = bytes[21..37].try_into().expect("Länge geprüft");
        let nonce: [u8; NONCE_LEN] = bytes[37..HEADER_LEN].try_into().expect("Länge geprüft");

        let key = derive_key(password, &salt, params)?;
        let cipher = XChaCha20Poly1305::new(&(*key).into());
        let plain = cipher
            .decrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: &bytes[HEADER_LEN..],
                    aad: &bytes[..HEADER_LEN],
                },
            )
            .map(Zeroizing::new)
            .map_err(|_| Error::WrongPassword)?;
        let entries = decode(&plain).ok_or(Error::Corrupt)?;

        Ok(Vault {
            path: path.to_owned(),
            params,
            salt,
            key,
            entries,
        })
    }

    /// Gespeicherte Einträge, neueste zuerst.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Fügt einen Eintrag vorne an und schreibt die Datei. Scheitert das Schreiben, bleibt der
    /// Eintrag draußen: RAM und Datei zeigen immer denselben Stand.
    pub fn add(&mut self, entry: Entry) -> Result<(), Error> {
        self.entries.insert(0, entry);
        if let Err(e) = self.write() {
            self.entries.remove(0);
            return Err(e);
        }
        Ok(())
    }

    fn write(&self) -> Result<(), Error> {
        let mut nonce = [0u8; NONCE_LEN];
        rand::rng().fill_bytes(&mut nonce);

        let mut header = Vec::with_capacity(HEADER_LEN);
        header.extend_from_slice(MAGIC);
        header.push(VERSION);
        header.extend_from_slice(&self.params.m_kib.to_le_bytes());
        header.extend_from_slice(&self.params.t.to_le_bytes());
        header.extend_from_slice(&self.params.p.to_le_bytes());
        header.extend_from_slice(&self.salt);
        header.extend_from_slice(&nonce);
        debug_assert_eq!(header.len(), HEADER_LEN);

        let plain = encode(&self.entries);
        let cipher = XChaCha20Poly1305::new(&(*self.key).into());
        let sealed = cipher
            .encrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: &plain,
                    aad: &header,
                },
            )
            .map_err(|_| Error::Internal)?;

        let mut file = header;
        file.extend_from_slice(&sealed);
        write_atomic(&self.path, &file)?;
        Ok(())
    }
}

fn derive_key(
    password: &str,
    salt: &[u8; SALT_LEN],
    p: KdfParams,
) -> Result<Zeroizing<[u8; KEY_LEN]>, Error> {
    let params = Params::new(p.m_kib, p.t, p.p, Some(KEY_LEN)).map_err(|_| Error::Internal)?;
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, &mut *key)
        .map_err(|_| Error::Internal)?;
    Ok(key)
}

/// Temp-Datei schreiben, auf die Platte zwingen, dann über die alte Datei umbenennen.
/// `fs::rename` ersetzt unter Windows eine vorhandene Datei (MoveFileEx mit REPLACE_EXISTING).
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("pkv.tmp");
    let result = (|| {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().expect("Länge geprüft"))
}

// --- Klartext ---

const F_UPPER: u8 = 1;
const F_LOWER: u8 = 2;
const F_DIGITS: u8 = 4;
const F_SYMBOLS: u8 = 8;
const F_SIMILAR: u8 = 16;

fn flags(o: &Options) -> u8 {
    [
        (o.upper, F_UPPER),
        (o.lower, F_LOWER),
        (o.digits, F_DIGITS),
        (o.symbols, F_SYMBOLS),
        (o.avoid_similar, F_SIMILAR),
    ]
    .into_iter()
    .filter(|(on, _)| *on)
    .fold(0, |acc, (_, bit)| acc | bit)
}

fn encode(entries: &[Entry]) -> Zeroizing<Vec<u8>> {
    // Genau passend reservieren: Ein wachsender Vec ließe beim Umlagern ungelöschte Kopien zurück.
    let size = 4 + entries
        .iter()
        .map(|e| 8 + 1 + 1 + 2 + e.label.len() + 2 + e.password.len())
        .sum::<usize>();
    let mut out = Zeroizing::new(Vec::with_capacity(size));
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for e in entries {
        out.extend_from_slice(&e.saved_at.to_le_bytes());
        out.push(e.options.length);
        out.push(flags(&e.options));
        for s in [e.label.as_str(), e.password.as_str()] {
            // Etikett und Passwort sind kurz (Eingabefeld, höchstens 32 Zeichen).
            let len = s.len().min(u16::MAX as usize);
            out.extend_from_slice(&(len as u16).to_le_bytes());
            out.extend_from_slice(&s.as_bytes()[..len]);
        }
    }
    debug_assert_eq!(out.len(), size);
    out
}

fn decode(b: &[u8]) -> Option<Vec<Entry>> {
    let mut r = Reader { b, at: 0 };
    let count = u32::from_le_bytes(r.take(4)?.try_into().ok()?) as usize;
    // Jeder Eintrag braucht mindestens 14 Byte: schützt vor einer riesigen Reservierung.
    if count > b.len() / 14 {
        return None;
    }
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let saved_at = i64::from_le_bytes(r.take(8)?.try_into().ok()?);
        let length = r.take(1)?[0];
        let f = r.take(1)?[0];
        let label = r.string()?;
        let password = r.string()?;
        entries.push(Entry {
            label,
            password,
            saved_at,
            options: Options {
                length,
                upper: f & F_UPPER != 0,
                lower: f & F_LOWER != 0,
                digits: f & F_DIGITS != 0,
                symbols: f & F_SYMBOLS != 0,
                avoid_similar: f & F_SIMILAR != 0,
            },
        });
    }
    (r.at == b.len()).then_some(entries)
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
    }

    fn string(&mut self) -> Option<Zeroizing<String>> {
        let len = u16::from_le_bytes(self.take(2)?.try_into().ok()?) as usize;
        let s = std::str::from_utf8(self.take(len)?).ok()?;
        Some(Zeroizing::new(s.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Billige Parameter, damit die Tests schnell laufen. Das Format ist dasselbe.
    const FAST: KdfParams = KdfParams { m_kib: 64, t: 1, p: 1 };
    const PW: &str = "richtig-langes-passwort";

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "passkey-test-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir.join("verlauf.pkv")
    }

    fn entry(label: &str, pw: &str, at: i64) -> Entry {
        Entry {
            label: Zeroizing::new(label.into()),
            password: Zeroizing::new(pw.into()),
            saved_at: at,
            options: Options {
                length: 24,
                symbols: false,
                avoid_similar: true,
                ..Options::default()
            },
        }
    }

    fn filled(name: &str) -> PathBuf {
        let path = temp_path(name);
        let mut v = Vault::create(&path, PW, FAST).unwrap();
        v.add(entry("Bank", "Tz4rM9xQe2LwKc7HaPq3", 1_760_000_000)).unwrap();
        v.add(entry("Ümlaut-Shop ✓", "pV8nB3wYd6Fs2jRu4kLm", 1_760_100_000)).unwrap();
        v.add(entry("", "Xk7Hc2Wm9Qa5Lt3zZz9a", -5)).unwrap();
        path
    }

    #[test]
    fn roundtrip_keeps_all_fields_and_order() {
        let path = filled("roundtrip");
        let v = Vault::open(&path, PW).unwrap();
        let e = v.entries();
        assert_eq!(e.len(), 3);
        assert_eq!(e[0].label.as_str(), "");
        assert_eq!(e[0].saved_at, -5);
        assert_eq!(e[1].label.as_str(), "Ümlaut-Shop ✓");
        assert_eq!(e[2].password.as_str(), "Tz4rM9xQe2LwKc7HaPq3");
        assert_eq!(e[2].options, entry("", "", 0).options);
    }

    #[test]
    fn wrong_password_is_rejected() {
        let path = filled("wrong");
        assert_eq!(
            Vault::open(&path, "falsches-passwort").unwrap_err(),
            Error::WrongPassword
        );
    }

    #[test]
    fn any_changed_byte_is_detected() {
        let path = filled("tamper");
        let original = fs::read(&path).unwrap();
        // Ein Byte in Salt, Nonce, Geheimtext und Tag. Die Parameter im Kopf prüft der nächste Test.
        for at in [21, 40, HEADER_LEN + 3, original.len() - 1] {
            let mut bytes = original.clone();
            bytes[at] ^= 0x01;
            fs::write(&path, &bytes).unwrap();
            assert_eq!(Vault::open(&path, PW).unwrap_err(), Error::WrongPassword, "Byte {at}");
        }
    }

    #[test]
    fn changed_kdf_params_fail_too() {
        let path = filled("params");
        let mut bytes = fs::read(&path).unwrap();
        bytes[13] = 2; // Durchläufe 1 → 2: anderer Schlüssel und anderer Kopf
        fs::write(&path, &bytes).unwrap();
        assert_eq!(Vault::open(&path, PW).unwrap_err(), Error::WrongPassword);
    }

    #[test]
    fn absurd_kdf_params_are_refused_before_hashing() {
        let path = filled("absurd");
        let mut bytes = fs::read(&path).unwrap();
        bytes[9..13].copy_from_slice(&u32::MAX.to_le_bytes()); // 4 TiB Speicher
        fs::write(&path, &bytes).unwrap();
        assert_eq!(Vault::open(&path, PW).unwrap_err(), Error::Corrupt);
    }

    #[test]
    fn foreign_truncated_or_newer_files_are_corrupt() {
        let path = filled("corrupt");
        let original = fs::read(&path).unwrap();

        fs::write(&path, b"keine passkey-datei, nur text").unwrap();
        assert_eq!(Vault::open(&path, PW).unwrap_err(), Error::Corrupt);

        fs::write(&path, &original[..HEADER_LEN + 4]).unwrap();
        assert_eq!(Vault::open(&path, PW).unwrap_err(), Error::Corrupt);

        let mut newer = original.clone();
        newer[8] = VERSION + 1;
        fs::write(&path, &newer).unwrap();
        assert_eq!(Vault::open(&path, PW).unwrap_err(), Error::Corrupt);
    }

    #[test]
    fn create_never_overwrites() {
        let path = filled("exists");
        assert_eq!(
            Vault::create(&path, "anderes-passwort-xyz", FAST).unwrap_err(),
            Error::Exists
        );
        assert_eq!(Vault::open(&path, PW).unwrap().entries().len(), 3);
    }

    #[test]
    fn every_save_uses_a_fresh_nonce_and_leaves_no_temp_file() {
        let path = temp_path("nonce");
        let mut v = Vault::create(&path, PW, FAST).unwrap();
        let first = fs::read(&path).unwrap();
        v.add(entry("a", "b", 1)).unwrap();
        let second = fs::read(&path).unwrap();
        assert_ne!(first[37..HEADER_LEN], second[37..HEADER_LEN]);
        assert_eq!(first[21..37], second[21..37], "Salt bleibt");
        assert!(!path.with_extension("pkv.tmp").exists());
    }

    #[test]
    fn plaintext_never_appears_in_the_file() {
        let path = filled("plain");
        let bytes = fs::read(&path).unwrap();
        for needle in [&b"Tz4rM9xQe2LwKc7HaPq3"[..], b"Bank"] {
            assert!(!bytes.windows(needle.len()).any(|w| w == needle));
        }
    }

    #[test]
    fn failed_write_keeps_ram_and_file_in_step() {
        let path = filled("rollback");
        let mut v = Vault::open(&path, PW).unwrap();
        // Ein Ordner mit dem Namen der Temp-Datei lässt das Schreiben scheitern.
        fs::create_dir(path.with_extension("pkv.tmp")).unwrap();
        assert!(v.add(entry("neu", "neu", 9)).is_err());
        assert_eq!(v.entries().len(), 3);
        fs::remove_dir(path.with_extension("pkv.tmp")).unwrap();
        assert_eq!(Vault::open(&path, PW).unwrap().entries().len(), 3);
    }

    #[test]
    fn decode_rejects_trailing_or_missing_bytes() {
        let entries = vec![entry("x", "y", 1)];
        let plain = encode(&entries);
        assert!(decode(&plain).is_some());
        assert!(decode(&plain[..plain.len() - 1]).is_none());
        let mut longer = plain.to_vec();
        longer.push(0);
        assert!(decode(&longer).is_none());
        // Behauptete Anzahl viel größer als die Daten.
        assert!(decode(&[0xff, 0xff, 0xff, 0x7f]).is_none());
    }
}
