use crate::{crypto, otp::Algorithm};
use anyhow::{bail, Context, Result};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;
use zeroize::Zeroize;

const SALT_LENGTH: usize = 16;
/// Current on-disk VaultData schema version. Phase 2.3 added per-entry usage
/// metadata (added_at / last_used_at / use_count), which is not representable
/// in bincode without a versioned layout; the bump forces a one-shot upgrade
/// of every existing v1 vault on next unlock.
pub const CURRENT_VAULT_VERSION: u32 = 2;

#[derive(Clone, Deserialize, Serialize)]
pub struct Account {
    pub id: Uuid,
    pub issuer: String,
    pub name: String,
    secret: String,
    pub digits: u8,
    pub period: u32,
    pub algorithm: Algorithm,
    #[serde(default)]
    pub added_at: u64,
    #[serde(default)]
    pub last_used_at: Option<u64>,
    #[serde(default)]
    pub use_count: u64,
}

impl Account {
    pub fn new(
        issuer: String,
        name: String,
        secret: String,
        digits: u8,
        period: u32,
        algorithm: Algorithm,
    ) -> Result<Self> {
        let mut secret = secret;
        let normalized = match crate::otp::normalize_secret(&secret) {
            Ok(value) => value,
            Err(error) => {
                secret.zeroize();
                return Err(error);
            }
        };
        secret.zeroize();
        let account = Self {
            id: Uuid::new_v4(),
            issuer,
            name,
            secret: normalized,
            digits,
            period,
            algorithm,
            added_at: current_timestamp(),
            last_used_at: None,
            use_count: 0,
        };
        account.validate()?;
        Ok(account)
    }

    #[cfg(feature = "gtk")]
    pub fn secret(&self) -> &str {
        &self.secret
    }

    /// Record that the account was just used (code copied or displayed in the
    /// detail view). Updates `last_used_at` and bumps `use_count`. Callers are
    /// responsible for debouncing so a single visible period is not counted
    /// multiple times in a row.
    pub fn record_use(&mut self) {
        self.use_count = self.use_count.saturating_add(1);
        self.last_used_at = Some(current_timestamp());
    }

    /// Copy the stable identity and usage metadata from another account.
    /// Used by the edit flow so that re-importing the same entry does not
    /// change its id (used for selection persistence) or wipe its history.
    pub fn inherit_identity_from(&mut self, other: &Account) {
        self.id = other.id;
        self.added_at = other.added_at;
        self.last_used_at = other.last_used_at;
        self.use_count = other.use_count;
    }

    pub fn validate(&self) -> Result<()> {
        if self.issuer.trim().is_empty() || self.name.trim().is_empty() {
            bail!("Issuer and account name are required");
        }
        if !(6..=8).contains(&self.digits) {
            bail!("Digits must be between 6 and 8");
        }
        if self.period == 0 {
            bail!("Period must be greater than zero");
        }
        crate::otp::normalize_secret(&self.secret)?;
        Ok(())
    }
}

impl Drop for Account {
    fn drop(&mut self) {
        self.secret.zeroize();
    }
}

#[derive(Deserialize, Serialize)]
pub struct VaultData {
    pub version: u32,
    pub created_at: u64,
    pub updated_at: u64,
    pub accounts: Vec<Account>,
}

/// On-disk layout of an account prior to phase 2.3 (no usage metadata).
/// Kept around as a deserialization-only schema for the v1 → v2 migration
/// triggered by `VaultStore::unlock`. Must mirror the historical field set
/// exactly — adding fields here breaks compat with existing vaults.
#[derive(Deserialize, Serialize)]
struct AccountV1 {
    pub id: Uuid,
    pub issuer: String,
    pub name: String,
    secret: String,
    pub digits: u8,
    pub period: u32,
    pub algorithm: Algorithm,
}

/// On-disk layout of `VaultData` prior to phase 2.3. Mirrors the historical
/// field set (no per-entry usage metadata). Used only by the v1 → v2
/// migration path.
#[derive(Deserialize, Serialize)]
#[allow(dead_code)]
struct VaultDataV1 {
    pub version: u32,
    pub created_at: u64,
    pub updated_at: u64,
    pub accounts: Vec<AccountV1>,
}

impl From<VaultDataV1> for VaultData {
    fn from(v1: VaultDataV1) -> Self {
        Self {
            version: CURRENT_VAULT_VERSION,
            created_at: v1.created_at,
            updated_at: v1.updated_at,
            accounts: v1
                .accounts
                .into_iter()
                .map(|account| Account {
                    id: account.id,
                    issuer: account.issuer,
                    name: account.name,
                    secret: account.secret,
                    digits: account.digits,
                    period: account.period,
                    algorithm: account.algorithm,
                    // Migrated entries have no recorded add date. Use the
                    // vault creation timestamp as a best-effort hint; the UI
                    // hides the field when it is exactly zero, and downstream
                    // bumps can refine this default.
                    added_at: v1.created_at,
                    last_used_at: None,
                    use_count: 0,
                })
                .collect(),
        }
    }
}

impl VaultData {
    pub fn new() -> Self {
        let timestamp = current_timestamp();
        Self {
            version: CURRENT_VAULT_VERSION,
            created_at: timestamp,
            updated_at: timestamp,
            accounts: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != CURRENT_VAULT_VERSION {
            bail!(
                "Unsupported data version {} (expected {})",
                self.version,
                CURRENT_VAULT_VERSION
            );
        }
        for account in &self.accounts {
            account.validate()?;
        }
        Ok(())
    }

    pub fn add_account(&mut self, account: Account) -> Result<Uuid> {
        account.validate()?;
        let id = account.id;
        self.accounts.push(account);
        self.updated_at = current_timestamp();
        Ok(id)
    }

    pub fn remove_account(&mut self, id: Uuid) -> Option<Account> {
        if let Some(position) = self.accounts.iter().position(|account| account.id == id) {
            self.updated_at = current_timestamp();
            Some(self.accounts.remove(position))
        } else {
            None
        }
    }

    pub fn reorder_account(&mut self, id: Uuid, new_position: usize) -> Option<usize> {
        let current = self.accounts.iter().position(|account| account.id == id)?;
        if current == new_position {
            return Some(current);
        }
        let account = self.accounts.remove(current);
        let target = new_position.min(self.accounts.len());
        let adjusted = if new_position > current {
            target
        } else {
            target
        };
        self.accounts.insert(adjusted, account);
        self.updated_at = current_timestamp();
        Some(adjusted)
    }

    #[cfg(feature = "gtk")]
    pub fn account(&self, id: Uuid) -> Option<&Account> {
        self.accounts.iter().find(|account| account.id == id)
    }
}

impl Default for VaultData {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Vault {
    data: VaultData,
    key: [u8; 32],
    salt: [u8; SALT_LENGTH],
    store: VaultStore,
    #[allow(dead_code)]
    lock: Option<VaultLock>,
}

impl Vault {
    pub fn data(&self) -> &VaultData {
        &self.data
    }

    pub fn data_mut(&mut self) -> &mut VaultData {
        &mut self.data
    }

    #[cfg(feature = "gtk")]
    #[allow(dead_code)]
    pub fn store(&self) -> &VaultStore {
        &self.store
    }

    pub fn save(&self) -> Result<()> {
        self.data.validate()?;
        let mut plaintext =
            bincode::serialize(&self.data).context("Unable to serialize entries")?;
        let encoded = crypto::seal_with_key_and_salt(&plaintext, &self.key, &self.salt)?;
        plaintext.zeroize();
        backup_previous(self.store.path())?;
        write_atomic(self.store.path(), &encoded)
    }

    pub fn change_password(&mut self, old_password: &str, new_password: &str) -> Result<()> {
        validate_password(new_password)?;
        let (rederived_key, _, _) =
            crypto::open_with_key_and_salt(&self.store.read_encrypted()?, old_password)?;
        if !constant_time_eq(&rederived_key, &self.key) {
            let mut to_wipe = rederived_key;
            to_wipe.zeroize();
            bail!("Old password is incorrect");
        }
        let mut to_wipe = rederived_key;
        to_wipe.zeroize();
        let mut new_salt = [0_u8; SALT_LENGTH];
        OsRng.fill_bytes(&mut new_salt);
        let mut new_key = crypto::derive_production_key(new_password, &new_salt)?;
        let mut plaintext =
            bincode::serialize(&self.data).context("Unable to serialize the vault")?;
        let encoded = crypto::seal_with_key_and_salt(&plaintext, &new_key, &new_salt)?;
        plaintext.zeroize();
        backup_previous(self.store.path())?;
        let result = write_atomic(self.store.path(), &encoded);
        match result {
            Ok(()) => {
                self.key.zeroize();
                self.salt.zeroize();
                self.key = new_key;
                self.salt = new_salt;
                Ok(())
            }
            Err(error) => {
                new_key.zeroize();
                new_salt.zeroize();
                Err(error)
            }
        }
    }

    pub fn remove_account(&mut self, id: Uuid) -> Result<Option<Account>> {
        let account = self.data.remove_account(id);
        match account {
            Some(account) => match self.save() {
                Ok(()) => Ok(Some(account)),
                Err(error) => {
                    self.data.add_account(account)?;
                    Err(error)
                }
            },
            None => Ok(None),
        }
    }

    pub fn reorder_account(&mut self, id: Uuid, new_position: usize) -> Result<Option<usize>> {
        let original_order = self
            .data
            .accounts
            .iter()
            .map(|account| account.id)
            .collect::<Vec<_>>();
        let new_index = self.data.reorder_account(id, new_position);
        let Some(new_index) = new_index else {
            return Ok(None);
        };
        if original_order
            .iter()
            .position(|candidate| *candidate == id)
            .map_or(false, |current| current == new_index)
        {
            return Ok(Some(new_index));
        }
        match self.save() {
            Ok(()) => Ok(Some(new_index)),
            Err(error) => {
                self.data.accounts.sort_by_key(|account| {
                    original_order
                        .iter()
                        .position(|candidate| *candidate == account.id)
                        .unwrap_or(usize::MAX)
                });
                self.data.updated_at = current_timestamp();
                Err(error)
            }
        }
    }
}

impl Drop for Vault {
    fn drop(&mut self) {
        for account in &mut self.data.accounts {
            account.secret.zeroize();
        }
        self.key.zeroize();
        self.salt.zeroize();
    }
}

#[derive(Clone)]
pub struct VaultStore {
    path: PathBuf,
}

impl VaultStore {
    pub fn for_path(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("Unable to create the vault directory")?;
        }
        Ok(Self { path })
    }

    pub fn default_path() -> Result<PathBuf> {
        let data_dir = dirs::data_dir().context("Unable to determine the data directory")?;
        Ok(data_dir.join("notp").join("vault.notp"))
    }

    #[cfg(test)]
    fn from_path(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn exists(&self) -> bool {
        self.path.is_file()
    }

    pub fn create(&self, password: &str) -> Result<Vault> {
        if self.exists() {
            bail!("Vault already exists");
        }
        validate_password(password)?;

        let data = VaultData::new();
        let mut plaintext = bincode::serialize(&data).context("Unable to serialize the vault")?;
        let mut salt = [0_u8; SALT_LENGTH];
        OsRng.fill_bytes(&mut salt);
        let key = crypto::derive_production_key(password, &salt)?;
        let encoded = crypto::seal_with_key_and_salt(&plaintext, &key, &salt)?;
        plaintext.zeroize();
        write_atomic(&self.path, &encoded)?;
        Ok(Vault {
            data,
            key,
            salt,
            store: self.clone(),
            lock: None,
        })
    }

    pub fn unlock(&self, password: &str) -> Result<Vault> {
        if !self.exists() {
            bail!("Vault does not exist");
        }
        let lock = VaultLock::acquire(&self.path)
            .context("Vault is already locked by another instance")?;
        let encoded = fs::read(&self.path).context("Unable to read the vault")?;
        let (key, salt, mut plaintext) = crypto::open_with_key_and_salt(&encoded, password)?;
        // bincode is not self-describing: `#[serde(default)]` is silently
        // ignored on a shorter payload, which would otherwise break every
        // vault written before phase 2.3. Peek the schema version byte and
        // route to the matching decoder; missing entries in a legacy layout
        // are filled by the per-version migration below.
        let on_disk_version = peek_varint_u32(&plaintext)
            .context("Unable to read the vault schema version")?;
        let data: VaultData = match on_disk_version {
            1 => {
                let v1: VaultDataV1 = match bincode::deserialize(&plaintext)
                    .context("Unable to decode a v1 vault") {
                    Ok(v1) => v1,
                    Err(error) => {
                        plaintext.zeroize();
                        return Err(error);
                    }
                };
                plaintext.zeroize();
                let mut data: VaultData = v1.into();
                data.updated_at = current_timestamp();
                data
            }
            CURRENT_VAULT_VERSION => {
                let data = match bincode::deserialize(&plaintext)
                    .context("Vault is incompatible or corrupted") {
                    Ok(data) => data,
                    Err(error) => {
                        plaintext.zeroize();
                        return Err(error);
                    }
                };
                plaintext.zeroize();
                data
            }
            other => {
                plaintext.zeroize();
                bail!(
                    "Unsupported vault data version {} (expected {} or 1)",
                    other,
                    CURRENT_VAULT_VERSION
                );
            }
        };
        data.validate().context("Vault data is invalid")?;
        Ok(Vault {
            data,
            key,
            salt,
            store: self.clone(),
            lock: Some(lock),
        })
    }

    fn read_encrypted(&self) -> Result<Vec<u8>> {
        fs::read(&self.path).context("Unable to read the vault")
    }
}

/// Cooperative exclusive lock on the vault file (single-instance guarantee).
/// On non-Unix platforms this is a no-op (returns `Ok`).
#[cfg(unix)]
struct VaultLock {
    _file: std::fs::File,
}

#[cfg(unix)]
impl VaultLock {
    fn acquire(path: &Path) -> Result<Self> {
        use std::io::ErrorKind;
        use std::os::unix::io::AsRawFd;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)
            .with_context(|| format!("Unable to open {}", path.display()))?;
        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if result != 0 {
            let err = std::io::Error::last_os_error();
            // On some FS the lock may be advisory; surface a clean message either way.
            return Err(anyhow::Error::new(match err.kind() {
                ErrorKind::WouldBlock | ErrorKind::AlreadyExists => err,
                _ => err,
            })
            .context("Vault is already locked by another instance"));
        }
        Ok(Self { _file: file })
    }
}

#[cfg(not(unix))]
struct VaultLock;

#[cfg(not(unix))]
impl VaultLock {
    fn acquire(_path: &Path) -> Result<Self> {
        Ok(Self)
    }
}

fn backup_previous(path: &Path) -> Result<()> {
    if !path.is_file() {
        return Ok(());
    }
    let backup_path = path.with_extension("notp.bak");
    if backup_path.exists() {
        fs::remove_file(&backup_path)
            .with_context(|| format!("Unable to remove {}", backup_path.display()))?;
    }
    fs::copy(path, &backup_path).with_context(|| {
        format!(
            "Unable to write backup {}",
            backup_path.display()
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = std::fs::Permissions::from_mode(0o600);
        let _ = std::fs::set_permissions(&backup_path, permissions);
    }
    Ok(())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

pub fn validate_password(password: &str) -> Result<()> {
    if password.chars().count() < 8 {
        bail!("Master password must contain at least 8 characters");
    }
    Ok(())
}

fn write_atomic(path: &Path, content: &[u8]) -> Result<()> {
    let parent = path.parent().context("Invalid vault path")?;
    fs::create_dir_all(parent).context("Unable to create the vault directory")?;
    let temporary = parent.join(format!(
        ".notp-{}-{}.tmp",
        std::process::id(),
        Uuid::new_v4()
    ));
    let result = write_file(&temporary, content).and_then(|_| {
        #[cfg(unix)]
        {
            fs::rename(&temporary, path).context("Unable to replace the vault")
        }
        #[cfg(not(unix))]
        {
            if path.exists() {
                fs::remove_file(path)?;
            }
            fs::rename(&temporary, path).context("Unable to replace the vault")
        }
    });
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn write_file(path: &Path, content: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("Unable to open {}", path.display()))?;
    file.write_all(content)
        .with_context(|| format!("Unable to write {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("Unable to synchronize {}", path.display()))
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// Decode the leading unsigned LEB128 varint (as written by `bincode`'s
/// default config) without consuming the input. Used by `VaultStore::unlock`
/// to choose the matching schema decoder for the on-disk `VaultData`.
fn peek_varint_u32(bytes: &[u8]) -> Option<u32> {
    let mut result: u32 = 0;
    let mut shift: u32 = 0;
    for &byte in bytes.iter().take(5) {
        result |= u32::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(result);
        }
        shift += 7;
        if shift >= 32 {
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn peek_varint_u32_handles_small_values() {
        // bincode's default config encodes a u32 ≤ 0x7f as a single byte; our
        // current versions (1 and 2) live in that range, so the peek must
        // round-trip them without consuming the trailing bytes.
        assert_eq!(peek_varint_u32(&[0x01, 0xff]), Some(1));
        assert_eq!(peek_varint_u32(&[0x02, 0x00, 0xab]), Some(2));
        assert_eq!(peek_varint_u32(&[]), None);
        // 5 bytes with the continuation bit still set → truncated varint.
        assert_eq!(peek_varint_u32(&[0x80; 5]), None);
    }

    #[test]
    fn unlocks_legacy_v1_vault_and_migrates() {
        // Phase 2.3 retrocompat: a vault written by a build prior to the
        // usage-metadata fields must still unlock. The in-memory data must
        // be tagged v2, every account must carry zeroed metadata, and a
        // subsequent save must round-trip through the v2 decoder.
        let directory = tempdir().unwrap();
        let vault_path = directory.path().join("vault.notp");
        let store = VaultStore::from_path(vault_path.clone());

        // Hand-craft a v1 VaultData and seal it with the production envelope.
        let legacy = VaultDataV1 {
            version: 1,
            created_at: 1_700_000_000,
            updated_at: 1_700_000_500,
            accounts: vec![
                AccountV1 {
                    id: Uuid::new_v4(),
                    issuer: "Legacy".to_string(),
                    name: "alice@example.com".to_string(),
                    secret: "JBSWY3DPEHPK3PXP".to_string(),
                    digits: 6,
                    period: 30,
                    algorithm: Algorithm::Sha1,
                },
                AccountV1 {
                    id: Uuid::new_v4(),
                    issuer: "Legacy".to_string(),
                    name: "bob@example.com".to_string(),
                    secret: "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ".to_string(),
                    digits: 8,
                    period: 60,
                    algorithm: Algorithm::Sha256,
                },
            ],
        };
        let plaintext = bincode::serialize(&legacy).unwrap();
        let mut salt = [0_u8; SALT_LENGTH];
        OsRng.fill_bytes(&mut salt);
        let key = crypto::derive_production_key("legacy master phrase", &salt).unwrap();
        let encoded = crypto::seal_with_key_and_salt(&plaintext, &key, &salt).unwrap();
        std::fs::write(&vault_path, &encoded).unwrap();

        let vault = store
            .unlock("legacy master phrase")
            .expect("v1 vault must unlock");
        assert_eq!(vault.data().version, CURRENT_VAULT_VERSION);
        assert_eq!(vault.data().created_at, 1_700_000_000);
        assert_eq!(vault.data().accounts.len(), 2);
        assert_eq!(vault.data().accounts[0].issuer, "Legacy");
        assert_eq!(vault.data().accounts[0].name, "alice@example.com");
        assert_eq!(vault.data().accounts[0].digits, 6);
        assert_eq!(vault.data().accounts[0].period, 30);
        assert_eq!(vault.data().accounts[0].algorithm, Algorithm::Sha1);
        // Migrated entries have no recorded history.
        assert_eq!(vault.data().accounts[0].last_used_at, None);
        assert_eq!(vault.data().accounts[0].use_count, 0);
        assert_eq!(
            vault.data().accounts[0].added_at, 1_700_000_000,
            "migrated entries inherit the vault creation timestamp"
        );

        // Saving the migrated vault must produce a file that the v2 decoder
        // (and only the v2 decoder) can read back.
        vault.save().expect("saving the migrated vault must succeed");
        drop(vault);

        let reopened = store.unlock("legacy master phrase").unwrap();
        assert_eq!(reopened.data().version, CURRENT_VAULT_VERSION);
        assert_eq!(reopened.data().accounts.len(), 2);
    }

    #[test]
    fn account_new_zeroizes_secret_on_failure() {
        // Phase 1.2 audit: when normalize_secret fails the caller's secret
        // string must be wiped before the error propagates. The function
        // cannot expose the zeroized bytes, but it must at least return an
        // error and not panic, which exercises the `secret.zeroize()` branch.
        let result = Account::new(
            "Example".to_string(),
            "alice@example.com".to_string(),
            "not a valid base32 secret !!!".to_string(),
            6,
            30,
            Algorithm::Sha1,
        );
        assert!(result.is_err());
    }

    #[test]
    fn account_metadata_round_trip() {
        // Phase 2.3: added_at, last_used_at, use_count must survive a save /
        // unlock cycle and stay accurate after multiple record_use() calls.
        let directory = tempdir().unwrap();
        let store = VaultStore::from_path(directory.path().join("vault.notp"));
        let mut vault = store.create("correct horse battery staple").unwrap();
        let mut account = Account::new(
            "Example".to_string(),
            "alice@example.com".to_string(),
            "JBSWY3DPEHPK3PXP".to_string(),
            6,
            30,
            Algorithm::Sha1,
        )
        .unwrap();
        let original_added_at = account.added_at;
        assert!(original_added_at > 0, "added_at must be set on creation");
        assert_eq!(account.last_used_at, None);
        assert_eq!(account.use_count, 0);

        account.record_use();
        account.record_use();
        assert_eq!(account.use_count, 2);
        assert!(account.last_used_at.is_some());

        let id = account.id;
        vault.data_mut().add_account(account).unwrap();
        vault.save().unwrap();

        let reopened = store.unlock("correct horse battery staple").unwrap();
        let stored = reopened
            .data()
            .accounts
            .iter()
            .find(|account| account.id == id)
            .expect("account must persist after a round trip");
        assert_eq!(stored.added_at, original_added_at);
        assert_eq!(stored.use_count, 2);
        assert!(stored.last_used_at.is_some());
    }

    #[test]
    fn account_edit_preserves_identity_and_metadata() {
        // Phase 2.3: replacing an account in place (the edit flow) must keep
        // its id (so the selection stays consistent), added_at and usage
        // counters intact.
        let mut original = Account::new(
            "Example".to_string(),
            "alice@example.com".to_string(),
            "JBSWY3DPEHPK3PXP".to_string(),
            6,
            30,
            Algorithm::Sha1,
        )
        .unwrap();
        original.record_use();
        original.record_use();
        let original_id = original.id;
        let added_at = original.added_at;
        let last_used_at = original.last_used_at;

        // Simulate the edit flow: build a fresh one (mirrors what the dialog
        // returns) and inherit the stable identity before overwriting.
        let mut replacement = Account::new(
            "Example".to_string(),
            "alice@example.com".to_string(),
            "JBSWY3DPEHPK3PXP".to_string(),
            8,
            30,
            Algorithm::Sha256,
        )
        .unwrap();
        replacement.inherit_identity_from(&original);

        assert_eq!(replacement.id, original_id);
        assert_eq!(replacement.added_at, added_at);
        assert_eq!(replacement.last_used_at, last_used_at);
        assert_eq!(replacement.use_count, 2);
        assert_eq!(replacement.digits, 8);
        assert_eq!(replacement.algorithm, Algorithm::Sha256);
    }

    #[test]
    fn creates_and_unlocks_a_vault() {
        let directory = tempdir().unwrap();
        let store = VaultStore::from_path(directory.path().join("vault.notp"));
        let account = Account::new(
            "Example".to_string(),
            "alice@example.com".to_string(),
            "JBSWY3DPEHPK3PXP".to_string(),
            6,
            30,
            Algorithm::Sha1,
        )
        .unwrap();
        store.create("correct horse battery staple").unwrap();
        let mut vault = store.unlock("correct horse battery staple").unwrap();
        vault.data_mut().add_account(account).unwrap();
        vault.save().unwrap();
        assert!(vault.remove_account(Uuid::nil()).unwrap().is_none());
        assert!(store.unlock("wrong password").is_err());
        let reopened = store.unlock("correct horse battery staple").unwrap();
        assert_eq!(reopened.data().accounts.len(), 1);
    }

    #[test]
    fn change_password_reencrypts_vault() {
        let directory = tempdir().unwrap();
        let store = VaultStore::from_path(directory.path().join("vault.notp"));
        let mut vault = store
            .create("correct horse battery staple")
            .unwrap();
        let account = Account::new(
            "Example".to_string(),
            "alice@example.com".to_string(),
            "JBSWY3DPEHPK3PXP".to_string(),
            6,
            30,
            Algorithm::Sha1,
        )
        .unwrap();
        vault.data_mut().add_account(account).unwrap();
        vault.save().unwrap();

        vault
            .change_password("correct horse battery staple", "new pass phrase!")
            .unwrap();

        assert!(store.unlock("correct horse battery staple").is_err());
        let reopened = store.unlock("new pass phrase!").unwrap();
        assert_eq!(reopened.data().accounts.len(), 1);
    }

    #[test]
    fn change_password_rejects_wrong_old() {
        let directory = tempdir().unwrap();
        let store = VaultStore::from_path(directory.path().join("vault.notp"));
        let mut vault = store.create("right password").unwrap();
        assert!(vault
            .change_password("wrong password", "new password")
            .is_err());
    }

    #[test]
    fn rejects_short_new_password() {
        let directory = tempdir().unwrap();
        let store = VaultStore::from_path(directory.path().join("vault.notp"));
        let mut vault = store.create("right password").unwrap();
        assert!(vault.change_password("right password", "short").is_err());
    }

    #[test]
    fn save_writes_backup() {
        let directory = tempdir().unwrap();
        let vault_path = directory.path().join("vault.notp");
        let store = VaultStore::from_path(vault_path.clone());
        let mut vault = store.create("right password").unwrap();
        let first = Account::new(
            "Example".to_string(),
            "alice@example.com".to_string(),
            "JBSWY3DPEHPK3PXP".to_string(),
            6,
            30,
            Algorithm::Sha1,
        )
        .unwrap();
        vault.data_mut().add_account(first).unwrap();
        vault.save().unwrap();

        let second = Account::new(
            "Example".to_string(),
            "bob@example.com".to_string(),
            "JBSWY3DPEHPK3PXP".to_string(),
            6,
            30,
            Algorithm::Sha1,
        )
        .unwrap();
        vault.data_mut().add_account(second).unwrap();
        vault.save().unwrap();

        let backup = vault_path.with_extension("notp.bak");
        assert!(backup.is_file(), "backup file must be created on save");
        let backup_store = VaultStore::from_path(backup.clone());
        let backup_vault = backup_store.unlock("right password").unwrap();
        assert_eq!(
            backup_vault.data().accounts.len(),
            1,
            "backup must reflect the previous vault state (one account)"
        );

        let current = store.unlock("right password").unwrap();
        assert_eq!(current.data().accounts.len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn flock_blocks_second_unlock() {
        let directory = tempdir().unwrap();
        let store = VaultStore::from_path(directory.path().join("vault.notp"));
        store.create("right password").unwrap();
        let _locked = store.unlock("right password").unwrap();
        let second = store.unlock("right password");
        assert!(second.is_err(), "second unlock on the same vault must fail");
    }

    #[test]
    fn reorder_account_persists_across_unlocks() {
        let directory = tempdir().unwrap();
        let store = VaultStore::from_path(directory.path().join("vault.notp"));
        store.create("correct horse battery staple").unwrap();
        let mut vault = store.unlock("correct horse battery staple").unwrap();
        let first = Account::new(
            "Example".to_string(),
            "alice@example.com".to_string(),
            "JBSWY3DPEHPK3PXP".to_string(),
            6,
            30,
            Algorithm::Sha1,
        )
        .unwrap();
        let first_id = vault.data_mut().add_account(first).unwrap();
        let second = Account::new(
            "Example".to_string(),
            "bob@example.com".to_string(),
            "JBSWY3DPEHPK3PXP".to_string(),
            6,
            30,
            Algorithm::Sha1,
        )
        .unwrap();
        let second_id = vault.data_mut().add_account(second).unwrap();
        vault.save().unwrap();

        assert_eq!(
            vault.reorder_account(first_id, 0).unwrap(),
            Some(0)
        );
        assert_eq!(
            vault.reorder_account(first_id, 5).unwrap(),
            Some(1)
        );
        assert_eq!(
            vault.reorder_account(second_id, 0).unwrap(),
            Some(0)
        );

        let reopened = store.unlock("correct horse battery staple").unwrap();
        let order = reopened
            .data()
            .accounts
            .iter()
            .map(|account| account.id)
            .collect::<Vec<_>>();
        assert_eq!(order, vec![second_id, first_id]);
    }
}
