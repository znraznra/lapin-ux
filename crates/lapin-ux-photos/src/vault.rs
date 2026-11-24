//! The Hidden vault: pictures kept encrypted on disk.
//!
//! Layout, in `~/.local/share/dev.lapinux.Photos/vault/`:
//!
//! * `vault.json`: a random salt, the Argon2id cost, and the vault key
//!   (a random 32-byte key) sealed with a key made from the password. A wrong
//!   password shows up as that seal failing to open.
//! * `<id>.bin`: one picture, `nonce (12) || ChaCha20-Poly1305 ciphertext`, with
//!   the id as associated data so files cannot be swapped around.
//! * `index.bin`: the list of pictures (id, original path, modified time),
//!   sealed the same way, so even the file names stay private.
//!
//! The vault key lives in memory only while the vault is open.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};

pub type Key = [u8; 32];

/// How much work turning a password into a key takes.
#[derive(Clone, Copy, Debug)]
pub struct Cost {
    /// Memory in KiB.
    pub m: u32,
    pub t: u32,
    pub p: u32,
}

impl Cost {
    /// OWASP's minimum for Argon2id: 19 MiB, two passes, one lane.
    pub const DEFAULT: Cost = Cost { m: 19_456, t: 2, p: 1 };
}

#[derive(Debug)]
pub enum Error {
    WrongPassword,
    /// A vault file is damaged or was changed.
    Corrupt,
    Io(std::io::Error),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::WrongPassword => f.write_str("That is not the password."),
            Error::Corrupt => f.write_str("The vault is damaged."),
            Error::Io(e) => write!(f, "{e}"),
        }
    }
}

type Result<T> = std::result::Result<T, Error>;

/// One picture in the vault.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    /// Where it came from, and where it goes back to.
    pub path: PathBuf,
    /// Seconds since the epoch.
    pub modified: u64,
}

const KEY_AAD: &[u8] = b"lapin-ux-photos vault key v1";
const INDEX_AAD: &[u8] = b"lapin-ux-photos vault index v1";

// ---------------------------------------------------------------- state

static KEY: Mutex<Option<Key>> = Mutex::new(None);
static NAMES: Mutex<Option<HashMap<PathBuf, String>>> = Mutex::new(None);

/// Remember (or forget) the vault key while the vault is open.
pub fn set_key(key: Option<Key>) {
    *KEY.lock().unwrap_or_else(|e| e.into_inner()) = key;
}

pub fn key() -> Option<Key> {
    *KEY.lock().unwrap_or_else(|e| e.into_inner())
}

/// The vault's own folder.
pub fn default_dir() -> Option<&'static PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| Some(dirs::data_dir()?.join(crate::APP_ID).join("vault"))).as_ref()
}

/// Where the encrypted file of an entry lives. It is the entry's identity in the app.
pub fn file_of(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.bin"))
}

/// The file names the app shows for the vault's files.
pub fn set_names(entries: &[Entry]) {
    let Some(dir) = default_dir() else { return };
    let map = entries
        .iter()
        .filter_map(|e| Some((file_of(dir, &e.id), e.path.file_name()?.to_string_lossy().into_owned())))
        .collect();
    *NAMES.lock().unwrap_or_else(|e| e.into_inner()) = Some(map);
}

/// The original name of a vault file (`None` for any other path).
pub fn original_name(path: &Path) -> Option<String> {
    NAMES.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.get(path).cloned()
}

/// The decrypted bytes of a vault file, if `path` is one and the vault is open.
pub fn read_path(path: &Path) -> Option<Vec<u8>> {
    let dir = default_dir()?;
    if path.parent()? != dir.as_path() {
        return None;
    }
    let id = path.file_stem()?.to_str()?;
    read(dir, &key()?, id).ok()
}

pub fn is_vault_path(path: &Path) -> bool {
    default_dir().is_some_and(|d| path.parent() == Some(d.as_path())) && path.extension().is_some_and(|e| e == "bin")
}

// --------------------------------------------------------------- crypto

fn cipher(key: &Key) -> ChaCha20Poly1305 {
    ChaCha20Poly1305::new(&(*key).into())
}

fn random<const N: usize>() -> std::io::Result<[u8; N]> {
    let mut b = [0u8; N];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
    Ok(b)
}

/// `nonce || ciphertext`.
fn seal(key: &Key, aad: &[u8], plain: &[u8]) -> Result<Vec<u8>> {
    let nonce: [u8; 12] = random()?;
    let ct = cipher(key).encrypt(Nonce::from_slice(&nonce), Payload { msg: plain, aad }).map_err(|_| Error::Corrupt)?;
    let mut out = nonce.to_vec();
    out.extend_from_slice(&ct);
    Ok(out)
}

fn open(key: &Key, aad: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 12 + 16 {
        return None;
    }
    let (nonce, ct) = data.split_at(12);
    cipher(key).decrypt(Nonce::from_slice(nonce), Payload { msg: ct, aad }).ok()
}

fn derive(password: &str, salt: &[u8], cost: Cost) -> Result<Key> {
    let params = Params::new(cost.m, cost.t, cost.p, Some(32)).map_err(|_| Error::Corrupt)?;
    let mut out = [0u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, &mut out)
        .map_err(|_| Error::Corrupt)?;
    Ok(out)
}

pub fn to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn from_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 || !s.is_ascii() {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

// -------------------------------------------------------------- storage

/// Write through a temporary file in the same folder, so a crash never leaves half a file.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().ok_or_else(|| std::io::Error::other("no folder"))?;
    let name = path.file_name().and_then(|n| n.to_str()).ok_or_else(|| std::io::Error::other("no name"))?;
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let result = (|| {
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn exists(dir: &Path) -> bool {
    dir.join("vault.json").is_file()
}

fn write_key_file(dir: &Path, password: &str, cost: Cost, key: &Key) -> Result<()> {
    let salt: [u8; 16] = random()?;
    let kek = derive(password, &salt, cost)?;
    let sealed = seal(&kek, KEY_AAD, key)?;
    let json = serde_json::json!({
        "v": 1,
        "salt": to_hex(&salt),
        "m": cost.m,
        "t": cost.t,
        "p": cost.p,
        "key": to_hex(&sealed),
    });
    write_atomic(&dir.join("vault.json"), &serde_json::to_vec(&json).map_err(std::io::Error::other)?)?;
    Ok(())
}

/// Make a new, empty vault and return its key.
pub fn create(dir: &Path, password: &str, cost: Cost) -> Result<Key> {
    if exists(dir) {
        return Err(Error::Io(std::io::Error::from(std::io::ErrorKind::AlreadyExists)));
    }
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    let key: Key = random()?;
    write_key_file(dir, password, cost, &key)?;
    save_index(dir, &key, &[])?;
    Ok(key)
}

/// Open the vault with its password.
pub fn unlock(dir: &Path, password: &str) -> Result<Key> {
    let text = std::fs::read_to_string(dir.join("vault.json"))?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|_| Error::Corrupt)?;
    let num = |k: &str| v.get(k).and_then(|x| x.as_u64()).and_then(|x| u32::try_from(x).ok());
    let cost = Cost { m: num("m").ok_or(Error::Corrupt)?, t: num("t").ok_or(Error::Corrupt)?, p: num("p").ok_or(Error::Corrupt)? };
    // A changed file must not be able to ask for gigabytes.
    if cost.m > 1 << 20 || cost.t > 20 || cost.p > 16 {
        return Err(Error::Corrupt);
    }
    let salt = from_hex(v.get("salt").and_then(|s| s.as_str()).ok_or(Error::Corrupt)?).ok_or(Error::Corrupt)?;
    let sealed = from_hex(v.get("key").and_then(|s| s.as_str()).ok_or(Error::Corrupt)?).ok_or(Error::Corrupt)?;
    let kek = derive(password, &salt, cost)?;
    let key = open(&kek, KEY_AAD, &sealed).ok_or(Error::WrongPassword)?;
    key.try_into().map_err(|_| Error::Corrupt)
}

/// Seal the same vault key with a new password. The pictures are not touched.
pub fn change_password(dir: &Path, key: &Key, new_password: &str, cost: Cost) -> Result<()> {
    write_key_file(dir, new_password, cost, key)
}

pub fn load_index(dir: &Path, key: &Key) -> Result<Vec<Entry>> {
    let data = match std::fs::read(dir.join("index.bin")) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let plain = open(key, INDEX_AAD, &data).ok_or(Error::Corrupt)?;
    let list: Vec<serde_json::Value> = serde_json::from_slice(&plain).map_err(|_| Error::Corrupt)?;
    list.iter()
        .map(|e| {
            let id = e.get("id")?.as_str()?.to_string();
            let path = PathBuf::from(std::ffi::OsString::from_vec(from_hex(e.get("path")?.as_str()?)?));
            let modified = e.get("modified")?.as_u64()?;
            valid_id(&id).then_some(Entry { id, path, modified })
        })
        .collect::<Option<Vec<_>>>()
        .ok_or(Error::Corrupt)
}

pub fn save_index(dir: &Path, key: &Key, entries: &[Entry]) -> Result<()> {
    let list: Vec<_> = entries
        .iter()
        .map(|e| serde_json::json!({ "id": e.id, "path": to_hex(e.path.as_os_str().as_bytes()), "modified": e.modified }))
        .collect();
    let plain = serde_json::to_vec(&list).map_err(std::io::Error::other)?;
    write_atomic(&dir.join("index.bin"), &seal(key, INDEX_AAD, &plain)?)?;
    Ok(())
}

/// Decrypt one picture.
pub fn read(dir: &Path, key: &Key, id: &str) -> Result<Vec<u8>> {
    if !valid_id(id) {
        return Err(Error::Corrupt);
    }
    let data = std::fs::read(file_of(dir, id))?;
    open(key, id.as_bytes(), &data).ok_or(Error::Corrupt)
}

/// Encrypt `path` into the vault, then remove the original. `index` is saved
/// before the original goes, so a crash can leave a copy but never lose a picture.
pub fn add(dir: &Path, key: &Key, index: &mut Vec<Entry>, path: &Path) -> Result<Entry> {
    let bytes = std::fs::read(path)?;
    let modified = std::fs::metadata(path)?
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    let id = to_hex(&random::<16>()?);
    write_atomic(&file_of(dir, &id), &seal(key, id.as_bytes(), &bytes)?)?;
    let entry = Entry { id, path: path.to_path_buf(), modified };
    index.push(entry.clone());
    let undo = |index: &mut Vec<Entry>| {
        index.pop();
        let _ = std::fs::remove_file(file_of(dir, &entry.id));
    };
    if let Err(e) = save_index(dir, key, index) {
        undo(index);
        return Err(e);
    }
    if let Err(e) = std::fs::remove_file(path) {
        undo(index);
        let _ = save_index(dir, key, index);
        return Err(e.into());
    }
    Ok(entry)
}

/// A name that is free: the original, or "name (restored).ext", "(restored 2)" and so on.
fn free_target(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let stem = path.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (1u32..)
        .map(|n| {
            let tag = if n == 1 { "restored".to_string() } else { format!("restored {n}") };
            path.with_file_name(format!("{stem} ({tag}){ext}"))
        })
        .find(|p| !p.exists())
        .unwrap_or_else(|| path.to_path_buf())
}

/// Decrypt a picture back to where it came from (or beside it, if the name is taken)
/// and drop it from the vault. Returns where it went.
pub fn restore(dir: &Path, key: &Key, index: &mut Vec<Entry>, id: &str) -> Result<PathBuf> {
    let pos = index.iter().position(|e| e.id == id).ok_or(Error::Corrupt)?;
    let bytes = read(dir, key, id)?;
    let wanted = index[pos].path.clone();
    if let Some(parent) = wanted.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // create_new: never write over a file that appeared in the meantime.
    let mut target = free_target(&wanted);
    let mut file = loop {
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&target) {
            Ok(f) => break f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => target = free_target(&wanted),
            Err(e) => return Err(e.into()),
        }
    };
    let written = file.write_all(&bytes).and_then(|()| file.sync_all());
    if let Err(e) = written {
        let _ = std::fs::remove_file(&target);
        return Err(e.into());
    }
    let entry = index.remove(pos);
    if let Err(e) = save_index(dir, key, index) {
        index.insert(pos, entry);
        return Err(e);
    }
    let _ = std::fs::remove_file(file_of(dir, id));
    Ok(target)
}

/// Delete the vault folder (only after every picture has been restored).
pub fn destroy(dir: &Path) -> std::io::Result<()> {
    match std::fs::remove_dir_all(dir) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHEAP: Cost = Cost { m: 8, t: 1, p: 1 };

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lapin-vault-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn hex_round_trip() {
        assert_eq!(from_hex(&to_hex(&[0, 1, 254, 255])), Some(vec![0, 1, 254, 255]));
        assert_eq!(from_hex("abc"), None);
        assert_eq!(from_hex("zz"), None);
    }

    #[test]
    fn create_then_unlock() {
        let base = scratch("unlock");
        let dir = base.join("vault");
        let key = create(&dir, "secret", CHEAP).unwrap();
        assert!(exists(&dir));
        assert_eq!(unlock(&dir, "secret").unwrap(), key);
        assert!(matches!(unlock(&dir, "Secret"), Err(Error::WrongPassword)));
        assert!(matches!(unlock(&dir, ""), Err(Error::WrongPassword)));
        assert!(matches!(create(&dir, "again", CHEAP), Err(Error::Io(_))));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn change_password_keeps_the_key() {
        let base = scratch("change");
        let dir = base.join("vault");
        let key = create(&dir, "old", CHEAP).unwrap();
        change_password(&dir, &key, "new", CHEAP).unwrap();
        assert!(matches!(unlock(&dir, "old"), Err(Error::WrongPassword)));
        assert_eq!(unlock(&dir, "new").unwrap(), key);
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn add_hides_the_bytes_and_restore_returns_them() {
        let base = scratch("roundtrip");
        let dir = base.join("vault");
        let key = create(&dir, "pw", CHEAP).unwrap();
        let photo = base.join("pics").join("cat.jpg");
        std::fs::create_dir_all(photo.parent().unwrap()).unwrap();
        let bytes: Vec<u8> = (0..5000u32).map(|i| (i * 7 % 251) as u8).collect();
        std::fs::write(&photo, &bytes).unwrap();

        let mut index = load_index(&dir, &key).unwrap();
        let entry = add(&dir, &key, &mut index, &photo).unwrap();
        assert!(!photo.exists(), "the original is removed");
        let stored = std::fs::read(file_of(&dir, &entry.id)).unwrap();
        assert!(stored.len() > bytes.len());
        assert!(!stored.windows(64).any(|w| bytes.windows(64).any(|b| b == w)), "no plain bytes on disk");
        assert_eq!(read(&dir, &key, &entry.id).unwrap(), bytes);

        // The index survives a reopen and is not readable without the key.
        let again = load_index(&dir, &key).unwrap();
        assert_eq!(again, vec![entry.clone()]);
        let raw = std::fs::read(dir.join("index.bin")).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("cat"));

        let back = restore(&dir, &key, &mut index, &entry.id).unwrap();
        assert_eq!(back, photo);
        assert_eq!(std::fs::read(&photo).unwrap(), bytes);
        assert!(index.is_empty() && load_index(&dir, &key).unwrap().is_empty());
        assert!(!file_of(&dir, &entry.id).exists());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn restore_never_overwrites() {
        let base = scratch("collide");
        let dir = base.join("vault");
        let key = create(&dir, "pw", CHEAP).unwrap();
        let photo = base.join("a.png");
        std::fs::write(&photo, b"first").unwrap();
        let mut index = Vec::new();
        let e = add(&dir, &key, &mut index, &photo).unwrap();
        std::fs::write(&photo, b"newer file with the same name").unwrap();
        let back = restore(&dir, &key, &mut index, &e.id).unwrap();
        assert_eq!(back, base.join("a (restored).png"));
        assert_eq!(std::fs::read(&photo).unwrap(), b"newer file with the same name");
        assert_eq!(std::fs::read(&back).unwrap(), b"first");
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_changed_or_swapped_file_is_refused() {
        let base = scratch("tamper");
        let dir = base.join("vault");
        let key = create(&dir, "pw", CHEAP).unwrap();
        let (a, b) = (base.join("a.png"), base.join("b.png"));
        std::fs::write(&a, b"aaaaaaaaaaaaaaaa").unwrap();
        std::fs::write(&b, b"bbbbbbbbbbbbbbbb").unwrap();
        let mut index = Vec::new();
        let ea = add(&dir, &key, &mut index, &a).unwrap();
        let eb = add(&dir, &key, &mut index, &b).unwrap();

        // Swapping two sealed files fails because the id is part of the seal.
        let (fa, fb) = (file_of(&dir, &ea.id), file_of(&dir, &eb.id));
        let (da, db) = (std::fs::read(&fa).unwrap(), std::fs::read(&fb).unwrap());
        std::fs::write(&fa, &db).unwrap();
        std::fs::write(&fb, &da).unwrap();
        assert!(matches!(read(&dir, &key, &ea.id), Err(Error::Corrupt)));
        std::fs::write(&fa, &da).unwrap();
        std::fs::write(&fb, &db).unwrap();
        assert!(read(&dir, &key, &ea.id).is_ok());

        // One flipped bit is caught.
        let mut flipped = da.clone();
        let last = flipped.len() - 1;
        flipped[last] ^= 1;
        std::fs::write(&fa, &flipped).unwrap();
        assert!(matches!(read(&dir, &key, &ea.id), Err(Error::Corrupt)));

        // A changed index is caught, and a wrong key cannot read it.
        let mut raw = std::fs::read(dir.join("index.bin")).unwrap();
        raw[20] ^= 1;
        std::fs::write(dir.join("index.bin"), &raw).unwrap();
        assert!(matches!(load_index(&dir, &key), Err(Error::Corrupt)));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_huge_cost_in_a_changed_file_is_refused() {
        let base = scratch("cost");
        let dir = base.join("vault");
        create(&dir, "pw", CHEAP).unwrap();
        let text = std::fs::read_to_string(dir.join("vault.json")).unwrap();
        let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
        v["m"] = serde_json::json!(4_000_000_000u64);
        std::fs::write(dir.join("vault.json"), serde_json::to_vec(&v).unwrap()).unwrap();
        assert!(matches!(unlock(&dir, "pw"), Err(Error::Corrupt)));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn ids_cannot_leave_the_folder() {
        let base = scratch("ids");
        let dir = base.join("vault");
        let key = create(&dir, "pw", CHEAP).unwrap();
        assert!(matches!(read(&dir, &key, "../../etc/passwd"), Err(Error::Corrupt)));
        let _ = std::fs::remove_dir_all(base);
    }
}
