//! Paired devices, their grants and the one-time pairing code.
//!
//! Deny by default: a new device can watch, nothing else. Input and tab
//! actions are granted per device by the person at the desktop. Tokens are
//! stored hashed and compared in constant time; the plain token exists only
//! in the device's own cookie.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::watch;

/// A paired device can always watch. These three are opt-in.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct Grant {
    pub input: bool,
    pub tabs: bool,
    #[serde(default)]
    pub app: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub grant: Grant,
    pub created_at: u64,
    pub last_seen: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Device {
    id: String,
    name: String,
    token_sha256: String,
    grant: Grant,
    created_at: u64,
    last_seen: u64,
}

impl Device {
    fn info(&self) -> DeviceInfo {
        DeviceInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            grant: self.grant,
            created_at: self.created_at,
            last_seen: self.last_seen,
        }
    }
}

#[derive(Default, Deserialize, Serialize)]
struct File {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    devices: Vec<Device>,
}

/// A paired device, as one request sees it.
#[derive(Clone, Debug)]
pub struct Caller {
    pub id: String,
    pub name: String,
    pub grant: Grant,
}

/// The device list, loaded once from app data and written on every change.
pub struct DeviceStore {
    path: Option<PathBuf>,
    devices: Mutex<Vec<Device>>,
    /// Bumped on every grant change and revocation, so open streams notice.
    revision: watch::Sender<u64>,
    _keepalive: watch::Receiver<u64>,
}

impl DeviceStore {
    /// `None` keeps the list in memory only (tests).
    pub fn load(path: Option<PathBuf>) -> Self {
        let devices = path
            .as_deref()
            .and_then(|path| fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<File>(&bytes).ok())
            .map(|file| file.devices)
            .unwrap_or_default();
        let (revision, keepalive) = watch::channel(0u64);
        Self {
            path,
            devices: Mutex::new(devices),
            revision,
            _keepalive: keepalive,
        }
    }

    pub fn memory() -> Self {
        Self::load(None)
    }

    /// The token in a device's cookie, if the device is still paired.
    pub fn authenticate(&self, token: &str) -> Option<Caller> {
        let hash = hash_token(token);
        let now = now();
        let mut devices = self.devices.lock().unwrap();
        let device = devices
            .iter_mut()
            .find(|device| constant_time_eq(&device.token_sha256, &hash))?;
        device.last_seen = now;
        Some(Caller {
            id: device.id.clone(),
            name: device.name.clone(),
            grant: device.grant,
        })
    }

    /// A pairing succeeded: the device gets a fresh token, which is only ever
    /// returned here so it can be set as the cookie.
    pub fn add(&self, name: &str) -> (DeviceInfo, String) {
        let token = random_hex(32);
        let device = Device {
            id: random_hex(8),
            name: name.trim().chars().take(64).collect(),
            token_sha256: hash_token(&token),
            grant: Grant::default(),
            created_at: now(),
            last_seen: now(),
        };
        let info = device.info();
        self.devices.lock().unwrap().push(device);
        self.save();
        (info, token)
    }

    pub fn list(&self) -> Vec<DeviceInfo> {
        let mut list: Vec<_> = self
            .devices
            .lock()
            .unwrap()
            .iter()
            .map(Device::info)
            .collect();
        list.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        list
    }

    pub fn update(&self, id: &str, grant: Grant) -> bool {
        let mut devices = self.devices.lock().unwrap();
        let Some(device) = devices.iter_mut().find(|device| device.id == id) else {
            return false;
        };
        if device.grant == grant {
            return true;
        }
        device.grant = grant;
        drop(devices);
        self.save();
        self.revision.send_modify(|version| *version += 1);
        true
    }

    /// A revoked device's streams end at once: the revision changes and each
    /// stream re-checks that its device still exists.
    pub fn revoke(&self, id: &str) -> bool {
        let removed = {
            let mut devices = self.devices.lock().unwrap();
            let before = devices.len();
            devices.retain(|device| device.id != id);
            before != devices.len()
        };
        if removed {
            self.save();
            self.revision.send_modify(|version| *version += 1);
        }
        removed
    }

    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.revision.subscribe()
    }

    pub fn len(&self) -> usize {
        self.devices.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.devices.lock().unwrap().is_empty()
    }

    /// Whether a device is still paired; open streams end when it is not.
    pub fn contains(&self, id: &str) -> bool {
        self.devices
            .lock()
            .unwrap()
            .iter()
            .any(|device| device.id == id)
    }

    /// Whether a device is still paired and holds the `app` grant.
    pub fn has_app(&self, id: &str) -> bool {
        self.devices
            .lock()
            .unwrap()
            .iter()
            .any(|device| device.id == id && device.grant.app)
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        let file = File {
            version: 1,
            devices: self.devices.lock().unwrap().clone(),
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let write = || -> std::io::Result<()> {
            let bytes = serde_json::to_vec_pretty(&file).unwrap_or_default();
            write_private(path, &bytes)
        };
        if let Err(error) = write() {
            log::warn!("could not save remote devices: {error}");
        }
    }
}

/// Writes owner-only on Unix; other platforms use the account's defaults.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(bytes)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
    }
    #[cfg(not(unix))]
    {
        fs::write(path, bytes)
    }
}

fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (a, b) in left.bytes().zip(right.bytes()) {
        difference |= a ^ b;
    }
    difference == 0
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

fn random_hex(bytes: usize) -> String {
    let mut buffer = vec![0u8; bytes];
    if getrandom::fill(&mut buffer).is_err() {
        // A machine without entropy cannot pair safely; use the time so the
        // token is at least unique, and log loudly.
        log::error!("getrandom failed for a remote token; falling back to time");
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        for (index, byte) in buffer.iter_mut().enumerate() {
            *byte = (seed >> (index % 16 * 8)) as u8 ^ index as u8;
        }
    }
    buffer.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// `A-Z2-9` without the characters people confuse: no I, O, 0 or 1.
const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
pub const CODE_LEN: usize = 6;
pub const PAIRING_TTL: Duration = Duration::from_secs(10 * 60);
pub const PAIRING_ATTEMPTS: u32 = 5;

#[derive(Debug, PartialEq, Eq)]
pub enum PairError {
    NotStarted,
    Expired,
    TooManyAttempts,
    Wrong,
}

impl PairError {
    pub fn message(self) -> &'static str {
        match self {
            Self::NotStarted => "No pairing is in progress.",
            Self::Expired => "That pairing code expired. Start pairing again.",
            Self::TooManyAttempts => "Too many attempts. Start pairing again.",
            Self::Wrong => "That code is not right.",
        }
    }
}

struct Pending {
    code: String,
    expires: Instant,
    attempts: u32,
}

/// One code at a time: starting again replaces the previous one.
#[derive(Default)]
pub struct Pairing {
    pending: Mutex<Option<Pending>>,
}

impl Pairing {
    pub fn start(&self) -> String {
        let code = self.new_code();
        *self.pending.lock().unwrap() = Some(Pending {
            code: code.clone(),
            expires: Instant::now() + PAIRING_TTL,
            attempts: 0,
        });
        code
    }

    /// Answers a code once, and only while it is fresh and under its attempt
    /// limit.
    pub fn consume(&self, code: &str) -> Result<(), PairError> {
        let mut pending = self.pending.lock().unwrap();
        let Some(entry) = pending.as_mut() else {
            return Err(PairError::NotStarted);
        };
        if entry.expires <= Instant::now() {
            *pending = None;
            return Err(PairError::Expired);
        }
        if entry.attempts >= PAIRING_ATTEMPTS {
            *pending = None;
            return Err(PairError::TooManyAttempts);
        }
        entry.attempts += 1;
        let entered = normalize_code(code);
        if !constant_time_eq(&entry.code, &entered) {
            return Err(PairError::Wrong);
        }
        *pending = None;
        Ok(())
    }

    #[cfg(test)]
    fn expire(&self) {
        if let Some(entry) = self.pending.lock().unwrap().as_mut() {
            entry.expires = Instant::now() - Duration::from_secs(1);
        }
    }

    fn new_code(&self) -> String {
        let mut bytes = [0u8; CODE_LEN];
        if getrandom::fill(&mut bytes).is_err() {
            log::error!("getrandom failed for a pairing code; falling back to time");
            let seed = now();
            for (index, byte) in bytes.iter_mut().enumerate() {
                *byte = (seed >> (index * 8)) as u8;
            }
        }
        bytes
            .iter()
            .map(|byte| CODE_ALPHABET[*byte as usize % CODE_ALPHABET.len()] as char)
            .collect()
    }
}

/// People type codes in either case, sometimes with spaces or dashes.
fn normalize_code(code: &str) -> String {
    code.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .take(CODE_LEN)
        .collect()
}

/// Where `remote-devices.json` lives.
pub fn store_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    use tauri::Manager;
    app.path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join("remote-devices.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> DeviceStore {
        DeviceStore::memory()
    }

    #[test]
    fn a_new_device_can_only_watch_until_granted() {
        let store = store();
        let (info, token) = store.add("phone");
        assert_eq!(info.grant, Grant::default());
        let caller = store.authenticate(&token).expect("paired");
        assert_eq!(caller.name, "phone");
        assert!(!caller.grant.input);
        assert!(!caller.grant.tabs);
        assert!(!caller.grant.app);
        assert!(store.authenticate("not-the-token").is_none());

        assert!(store.update(
            &info.id,
            Grant {
                input: true,
                tabs: false,
                app: false,
            }
        ));
        assert!(store.authenticate(&token).unwrap().grant.input);
        assert!(!store.authenticate(&token).unwrap().grant.tabs);
        assert!(!store.authenticate(&token).unwrap().grant.app);
    }

    #[test]
    fn app_grant_defaults_false_and_loads_from_old_device_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("remote-devices.json");
        let old_json = r#"{
            "version": 1,
            "devices": [{
                "id": "dev1",
                "name": "old-phone",
                "tokenSha256": "deadbeef",
                "grant": { "input": true, "tabs": true },
                "createdAt": 1,
                "lastSeen": 2
            }]
        }"#;
        fs::write(&path, old_json).unwrap();
        let store = DeviceStore::load(Some(path));
        let list = store.list();
        assert_eq!(list.len(), 1);
        assert!(list[0].grant.input);
        assert!(list[0].grant.tabs);
        assert!(!list[0].grant.app);
        assert!(!store.has_app("dev1"));

        let revisions = store.subscribe();
        assert!(store.update(
            "dev1",
            Grant {
                input: true,
                tabs: true,
                app: true,
            }
        ));
        assert!(store.has_app("dev1"));
        assert!(revisions.has_changed().unwrap());
    }

    #[test]
    fn revoking_a_device_invalidates_its_token_and_opens_a_revision() {
        let store = store();
        let (info, token) = store.add("phone");
        let revisions = store.subscribe();
        assert!(store.authenticate(&token).is_some());
        assert!(store.revoke(&info.id));
        assert!(store.authenticate(&token).is_none());
        assert!(store.list().is_empty());
        assert!(revisions.has_changed().unwrap());
        assert!(!store.revoke(&info.id), "revoking twice is a no-op");
    }

    #[test]
    fn the_saved_list_reloads_with_hashed_tokens_only() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("remote-devices.json");
        let store = DeviceStore::load(Some(path.clone()));
        let (info, token) = store.add("laptop");
        let reloaded = DeviceStore::load(Some(path.clone()));
        assert_eq!(reloaded.list().len(), 1);
        assert!(reloaded.authenticate(&token).is_some());
        let saved = fs::read_to_string(&path).unwrap();
        assert!(!saved.contains(&token), "the plain token is never stored");
        assert!(saved.contains(&info.id));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn a_pairing_code_works_once_and_expires() {
        let pairing = Pairing::default();
        let code = pairing.start();
        assert_eq!(code.len(), CODE_LEN);
        assert!(code.chars().all(|c| CODE_ALPHABET.contains(&(c as u8))));
        assert_eq!(pairing.consume(&code.to_ascii_lowercase()), Ok(()));
        assert_eq!(pairing.consume(&code), Err(PairError::NotStarted));

        let code = pairing.start();
        pairing.expire();
        assert_eq!(pairing.consume(&code), Err(PairError::Expired));
        assert_eq!(pairing.consume(&code), Err(PairError::NotStarted));
    }

    #[test]
    fn five_wrong_attempts_end_the_pairing() {
        let pairing = Pairing::default();
        let code = pairing.start();
        for _ in 0..PAIRING_ATTEMPTS {
            assert_eq!(pairing.consume("AAAAAA"), Err(PairError::Wrong));
        }
        assert_eq!(pairing.consume(&code), Err(PairError::TooManyAttempts));
        assert_eq!(pairing.consume(&code), Err(PairError::NotStarted));
    }

    #[test]
    fn wrong_attempts_count_across_all_codes_and_end_them_all() {
        let pairing = Pairing::default();
        let first = pairing.start();
        let second = pairing.start();
        for _ in 0..PAIRING_ATTEMPTS {
            assert_eq!(pairing.consume("AAAAAA"), Err(PairError::Wrong));
        }
        assert_eq!(pairing.consume(&first), Err(PairError::TooManyAttempts));
        assert_eq!(pairing.consume(&second), Err(PairError::NotStarted));
    }

    #[test]
    fn only_the_newest_code_stays_open() {
        let pairing = Pairing::default();
        let oldest = pairing.start();
        let newest = pairing.start();
        assert_eq!(pairing.consume(&oldest), Err(PairError::Wrong));
        assert_eq!(pairing.consume(&newest), Ok(()));
    }

    #[test]
    fn typed_codes_ignore_case_spaces_and_dashes() {
        let pairing = Pairing::default();
        let code = pairing.start();
        let typed = format!("{} - {}", &code[..3], &code[3..]).to_ascii_lowercase();
        assert_eq!(pairing.consume(&typed), Ok(()));
    }
}
