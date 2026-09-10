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

#[derive(Clone, Deserialize, Serialize)]
pub struct Account {
    pub id: Uuid,
    pub issuer: String,
    pub name: String,
    secret: String,
    pub digits: u8,
    pub period: u32,
    pub algorithm: Algorithm,
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
        };
        account.validate()?;
        Ok(account)
    }

    #[cfg(feature = "gtk")]
    pub fn secret(&self) -> &str {
        &self.secret
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

impl VaultData {
    pub fn new() -> Self {
        let timestamp = current_timestamp();
        Self {
            version: 1,
            created_at: timestamp,
            updated_at: timestamp,
            accounts: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            bail!("Unsupported data version");
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
}

impl Vault {
    pub fn data(&self) -> &VaultData {
        &self.data
    }

    pub fn data_mut(&mut self) -> &mut VaultData {
        &mut self.data
    }

    pub fn save(&self) -> Result<()> {
        self.data.validate()?;
        let mut plaintext =
            bincode::serialize(&self.data).context("Unable to serialize entries")?;
        let encoded = crypto::seal_with_key_and_salt(&plaintext, &self.key, &self.salt)?;
        plaintext.zeroize();
        write_atomic(self.store.path(), &encoded)
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
    #[cfg(feature = "gtk")]
    pub fn new() -> Result<Self> {
        let data_dir = dirs::data_dir().context("Unable to determine the data directory")?;
        let directory = data_dir.join("notp");
        fs::create_dir_all(&directory).context("Unable to create the data directory")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        }
        Ok(Self {
            path: directory.join("vault.notp"),
        })
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
        let mut key = crypto::derive_production_key(password, &salt)?;
        let encoded = crypto::seal_with_key_and_salt(&plaintext, &key, &salt)?;
        plaintext.zeroize();
        key.zeroize();
        write_atomic(&self.path, &encoded)?;
        Ok(Vault {
            data,
            key,
            salt,
            store: self.clone(),
        })
    }

    pub fn unlock(&self, password: &str) -> Result<Vault> {
        if !self.exists() {
            bail!("Vault does not exist");
        }
        let encoded = fs::read(&self.path).context("Unable to read the vault")?;
        let (key, salt, mut plaintext) = crypto::open_with_key_and_salt(&encoded, password)?;
        let data: VaultData =
            match bincode::deserialize(&plaintext).context("Vault is incompatible or corrupted") {
                Ok(data) => data,
                Err(error) => {
                    plaintext.zeroize();
                    return Err(error);
                }
            };
        plaintext.zeroize();
        data.validate().context("Vault data is invalid")?;
        Ok(Vault {
            data,
            key,
            salt,
            store: self.clone(),
        })
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

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
}
