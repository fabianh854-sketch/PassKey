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
//!
//! Die Datei gilt als fremde Eingabe: Der Leser prüft Größe, Kopf und jeden Eintrag, bevor er
//! etwas übernimmt, und greift nur über geprüfte Längen auf die Bytes zu.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand::rngs::SysRng;
use rand::TryRng;
use secrecy::{ExposeSecret, ExposeSecretMut, SecretBox, SecretString};
use zeroize::Zeroizing;

use crate::generator::{self, Options};

const MAGIC: &[u8; 8] = b"PASSKEY\0";
pub const VERSION: u8 = 1;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;
const HEADER_LEN: usize = 8 + 1 + 12 + SALT_LEN + NONCE_LEN;
const TAG_LEN: usize = 16;
const KEY_LEN: usize = 32;
/// Größere Dateien liest PassKey gar nicht erst ein. Ein Eintrag braucht unter 400 Byte,
/// das reicht also für über 20 000 Einträge.
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
/// Längstes Etikett in Zeichen.
pub const MAX_LABEL_CHARS: usize = 64;

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
        (1..=Self::MAX_P).contains(&self.p)
            && (1..=Self::MAX_T).contains(&self.t)
            && self.m_kib <= Self::MAX_M_KIB
            && self.p.checked_mul(8).is_some_and(|min| self.m_kib >= min)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Passwort falsch oder Datei verändert. AEAD kann beides nicht unterscheiden.
    WrongPassword,
    /// Kopf oder Inhalt passen nicht zum Format (fremde Datei, abgeschnitten, zu groß, neuere Version).
    Corrupt,
    /// Beim Anlegen liegt dort schon eine Datei. Sie wird nie überschrieben.
    Exists,
    /// Etikett oder Passwort verletzen die Regeln (Länge, Steuerzeichen).
    Invalid,
    Io(io::ErrorKind),
    /// Zufall, Schlüsselableitung oder Hintergrund-Thread ist gescheitert.
    Internal,
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e.kind())
    }
}

/// Text ohne Steuerzeichen mit `min..=max` Zeichen.
fn valid_text(s: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&s.chars().count()) && !s.chars().any(char::is_control)
}

/// Etikett eines Eintrags: höchstens [`MAX_LABEL_CHARS`] Zeichen, keine Steuerzeichen, auch leer.
/// Es verrät, wofür ein Passwort ist, und liegt deshalb ebenfalls geschützt im Speicher.
pub struct Label(SecretString);

impl TryFrom<String> for Label {
    type Error = Error;

    fn try_from(s: String) -> Result<Self, Error> {
        let ok = valid_text(&s, 0, MAX_LABEL_CHARS);
        // Zuerst einpacken: Auch ein abgelehnter Text wird beim Freigeben überschrieben.
        let secret = SecretString::from(s);
        if ok { Ok(Self(secret)) } else { Err(Error::Invalid) }
    }
}

impl Label {
    pub fn as_str(&self) -> &str {
        self.0.expose_secret()
    }
}

/// Ein gespeichertes Passwort. Nur über [`Entry::new`] zu bauen, das alle Regeln prüft.
pub struct Entry {
    label: Label,
    password: SecretString,
    /// Zeitpunkt des Speicherns in Unix-Sekunden (UTC).
    saved_at: i64,
    options: Options,
}

impl Entry {
    /// Das Passwort muss so lang sein, wie der Generator sie erzeugt (12 bis 32 Zeichen).
    pub fn new(
        label: Label,
        password: SecretString,
        saved_at: i64,
        options: Options,
    ) -> Result<Self, Error> {
        let ok = valid_text(
            password.expose_secret(),
            usize::from(generator::MIN_LEN),
            usize::from(generator::MAX_LEN),
        );
        if !ok {
            return Err(Error::Invalid);
        }
        Ok(Self {
            label,
            password,
            saved_at,
            options,
        })
    }

    pub fn label(&self) -> &str {
        self.label.as_str()
    }

    pub fn password(&self) -> &str {
        self.password.expose_secret()
    }

    pub fn saved_at(&self) -> i64 {
        self.saved_at
    }

    #[cfg(test)]
    pub fn options(&self) -> Options {
        self.options
    }
}

/// Eine entsperrte Datei: Schlüssel und Einträge liegen im RAM, bis sie fallen gelassen wird.
/// `SecretBox` und `SecretString` überschreiben beides beim Freigeben.
pub struct Vault {
    path: PathBuf,
    params: KdfParams,
    salt: [u8; SALT_LEN],
    key: SecretBox<[u8; KEY_LEN]>,
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

/// Zufall direkt vom Betriebssystem (unter Windows `ProcessPrng`), ohne Zwischen-Generator.
fn os_random<const N: usize>() -> Result<[u8; N], Error> {
    let mut bytes = [0u8; N];
    SysRng.try_fill_bytes(&mut bytes).map_err(|_| Error::Internal)?;
    Ok(bytes)
}

struct Header {
    params: KdfParams,
    salt: [u8; SALT_LEN],
    nonce: [u8; NONCE_LEN],
}

fn parse_header(bytes: &[u8]) -> Option<Header> {
    let mut r = Reader { b: bytes, at: 0 };
    if r.take(8)? != MAGIC || r.array::<1>()? != [VERSION] {
        return None;
    }
    let params = KdfParams {
        m_kib: r.u32()?,
        t: r.u32()?,
        p: r.u32()?,
    };
    let salt = r.array()?;
    let nonce = r.array()?;
    debug_assert_eq!(r.at, HEADER_LEN);
    params.plausible().then_some(Header { params, salt, nonce })
}

/// Liest höchstens [`MAX_FILE_BYTES`]. Eine größere Datei wird gar nicht erst ganz geladen.
fn read_limited(path: &Path) -> Result<Vec<u8>, Error> {
    let file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).map_or(true, |n| n > MAX_FILE_BYTES) {
        return Err(Error::Corrupt);
    }
    Ok(bytes)
}

impl Vault {
    /// Legt eine neue, leere Datei an. Langsam (Argon2), daher im Hintergrund aufrufen.
    pub fn create(path: &Path, password: &str, params: KdfParams) -> Result<Self, Error> {
        if path.exists() {
            return Err(Error::Exists);
        }
        let salt = os_random()?;
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
        let bytes = read_limited(path)?;
        let header = parse_header(&bytes).ok_or(Error::Corrupt)?;
        let (aad, sealed) = bytes.split_at_checked(HEADER_LEN).ok_or(Error::Corrupt)?;
        if sealed.len() < TAG_LEN {
            return Err(Error::Corrupt);
        }

        let key = derive_key(password, &header.salt, header.params)?;
        let cipher = XChaCha20Poly1305::new(&(*key.expose_secret()).into());
        let plain = cipher
            .decrypt(&XNonce::from(header.nonce), Payload { msg: sealed, aad })
            .map(Zeroizing::new)
            .map_err(|_| Error::WrongPassword)?;
        let entries = decode(&plain).ok_or(Error::Corrupt)?;

        Ok(Vault {
            path: path.to_owned(),
            params: header.params,
            salt: header.salt,
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
        let nonce = os_random::<NONCE_LEN>()?;

        let mut header = Vec::with_capacity(HEADER_LEN);
        header.extend_from_slice(MAGIC);
        header.push(VERSION);
        header.extend_from_slice(&self.params.m_kib.to_le_bytes());
        header.extend_from_slice(&self.params.t.to_le_bytes());
        header.extend_from_slice(&self.params.p.to_le_bytes());
        header.extend_from_slice(&self.salt);
        header.extend_from_slice(&nonce);
        debug_assert_eq!(header.len(), HEADER_LEN);

        let plain = encode(&self.entries)?;
        let cipher = XChaCha20Poly1305::new(&(*self.key.expose_secret()).into());
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
) -> Result<SecretBox<[u8; KEY_LEN]>, Error> {
    let params = Params::new(p.m_kib, p.t, p.p, Some(KEY_LEN)).map_err(|_| Error::Internal)?;
    // Der Schlüssel entsteht direkt im geschützten Speicher, ohne Kopie auf dem Stack.
    let mut key = SecretBox::new(Box::new([0u8; KEY_LEN]));
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, key.expose_secret_mut())
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

// --- Klartext ---

const F_UPPER: u8 = 1;
const F_LOWER: u8 = 2;
const F_DIGITS: u8 = 4;
const F_SYMBOLS: u8 = 8;
const F_SIMILAR: u8 = 16;
/// Feste Bytes pro Eintrag: Zeitpunkt, Länge, Schalter, zwei Längenangaben.
const ENTRY_FIXED: usize = 8 + 1 + 1 + 2 + 2;

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

fn encode(entries: &[Entry]) -> Result<Zeroizing<Vec<u8>>, Error> {
    // Genau passend reservieren: Ein wachsender Vec ließe beim Umlagern ungelöschte Kopien zurück.
    let size = entries.iter().try_fold(4usize, |acc, e| {
        acc.checked_add(ENTRY_FIXED)?
            .checked_add(e.label().len())?
            .checked_add(e.password().len())
    });
    let mut out = Zeroizing::new(Vec::with_capacity(size.ok_or(Error::Invalid)?));
    let count = u32::try_from(entries.len()).map_err(|_| Error::Invalid)?;
    out.extend_from_slice(&count.to_le_bytes());
    for e in entries {
        out.extend_from_slice(&e.saved_at.to_le_bytes());
        out.push(e.options.length);
        out.push(flags(&e.options));
        for s in [e.label(), e.password()] {
            // `Label` und `Entry::new` halten beide Texte weit unter 64 KiB.
            let len = u16::try_from(s.len()).map_err(|_| Error::Invalid)?;
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(s.as_bytes());
        }
    }
    debug_assert_eq!(Some(out.len()), size);
    Ok(out)
}

fn decode(b: &[u8]) -> Option<Vec<Entry>> {
    let mut r = Reader { b, at: 0 };
    let count = usize::try_from(r.u32()?).ok()?;
    // Jeder Eintrag braucht mindestens ENTRY_FIXED Byte: schützt vor einer riesigen Reservierung.
    if count > b.len() / ENTRY_FIXED {
        return None;
    }
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let saved_at = i64::from_le_bytes(r.array()?);
        let [length] = r.array()?;
        let [f] = r.array()?;
        let label = Label::try_from(r.string()?).ok()?;
        let password = SecretString::from(r.string()?);
        let options = Options {
            length,
            upper: f & F_UPPER != 0,
            lower: f & F_LOWER != 0,
            digits: f & F_DIGITS != 0,
            symbols: f & F_SYMBOLS != 0,
            avoid_similar: f & F_SIMILAR != 0,
        };
        entries.push(Entry::new(label, password, saved_at, options).ok()?);
    }
    (r.at == b.len()).then_some(entries)
}

/// Liest der Reihe nach aus einem Byte-Slice. Jeder Zugriff prüft die Grenzen und gibt sonst `None`.
struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let s = self.b.get(self.at..end)?;
        self.at = end;
        Some(s)
    }

    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.array()?))
    }

    /// `u16` Länge, dann so viele Byte UTF-8.
    fn string(&mut self) -> Option<String> {
        let len = usize::from(u16::from_le_bytes(self.array()?));
        let s = std::str::from_utf8(self.take(len)?).ok()?;
        Some(s.to_owned())
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

    fn opts() -> Options {
        Options {
            length: 24,
            symbols: false,
            avoid_similar: true,
            ..Options::default()
        }
    }

    fn entry(label: &str, pw: &str, at: i64) -> Entry {
        Entry::new(
            Label::try_from(label.to_owned()).unwrap(),
            SecretString::from(pw),
            at,
            opts(),
        )
        .unwrap()
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
        assert_eq!(e[0].label(), "");
        assert_eq!(e[0].saved_at(), -5);
        assert_eq!(e[1].label(), "Ümlaut-Shop ✓");
        assert_eq!(e[2].password(), "Tz4rM9xQe2LwKc7HaPq3");
        assert_eq!(e[2].options(), opts());
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

        fs::write(&path, &original[..HEADER_LEN - 1]).unwrap();
        assert_eq!(Vault::open(&path, PW).unwrap_err(), Error::Corrupt);

        fs::write(&path, b"").unwrap();
        assert_eq!(Vault::open(&path, PW).unwrap_err(), Error::Corrupt);

        let mut newer = original.clone();
        newer[8] = VERSION + 1;
        fs::write(&path, &newer).unwrap();
        assert_eq!(Vault::open(&path, PW).unwrap_err(), Error::Corrupt);
    }

    #[test]
    fn oversized_files_are_not_read() {
        let path = filled("huge");
        let mut bytes = fs::read(&path).unwrap();
        bytes.resize(usize::try_from(MAX_FILE_BYTES).unwrap() + 1, 0);
        fs::write(&path, &bytes).unwrap();
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
        v.add(entry("a", "Xk7Hc2Wm9Qa5Lt3zZz9a", 1)).unwrap();
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
        assert!(v.add(entry("neu", "Xk7Hc2Wm9Qa5Lt3zZz9a", 9)).is_err());
        assert_eq!(v.entries().len(), 3);
        fs::remove_dir(path.with_extension("pkv.tmp")).unwrap();
        assert_eq!(Vault::open(&path, PW).unwrap().entries().len(), 3);
    }

    #[test]
    fn labels_are_validated() {
        assert!(Label::try_from(String::new()).is_ok());
        assert!(Label::try_from("ä".repeat(MAX_LABEL_CHARS)).is_ok());
        assert_eq!(
            Label::try_from("a".repeat(MAX_LABEL_CHARS + 1)).err(),
            Some(Error::Invalid)
        );
        assert_eq!(Label::try_from("Bank\nX".to_owned()).err(), Some(Error::Invalid));
        assert_eq!(Label::try_from("Bank\u{0}".to_owned()).err(), Some(Error::Invalid));
    }

    #[test]
    fn entries_need_a_generator_sized_password() {
        let label = || Label::try_from(String::new()).unwrap();
        let make = |pw: &str| Entry::new(label(), SecretString::from(pw), 0, opts()).err();
        assert_eq!(make("zu-kurz"), Some(Error::Invalid));
        assert_eq!(make(&"x".repeat(33)), Some(Error::Invalid));
        assert_eq!(make("zwölf-zeichen\t"), Some(Error::Invalid));
        assert_eq!(make(&"x".repeat(12)), None);
        assert_eq!(make(&"x".repeat(32)), None);
    }

    #[test]
    fn decode_rejects_trailing_or_missing_bytes() {
        let entries = vec![entry("x", "Xk7Hc2Wm9Qa5Lt3zZz9a", 1)];
        let plain = encode(&entries).unwrap();
        assert!(decode(&plain).is_some());
        assert!(decode(&plain[..plain.len() - 1]).is_none());
        let mut longer = plain.to_vec();
        longer.push(0);
        assert!(decode(&longer).is_none());
        // Behauptete Anzahl viel größer als die Daten.
        assert!(decode(&[0xff, 0xff, 0xff, 0x7f]).is_none());
    }

    #[test]
    fn decode_rejects_entries_that_break_the_rules() {
        // Ein Eintrag mit 2-Zeichen-Passwort: Ein korrekt verschlüsselter, aber unsinniger Inhalt
        // wird nicht übernommen.
        let mut plain = 1u32.to_le_bytes().to_vec();
        plain.extend_from_slice(&0i64.to_le_bytes());
        plain.extend_from_slice(&[20, F_LOWER]);
        plain.extend_from_slice(&0u16.to_le_bytes());
        plain.extend_from_slice(&2u16.to_le_bytes());
        plain.extend_from_slice(b"ab");
        assert!(decode(&plain).is_none());
    }
}
