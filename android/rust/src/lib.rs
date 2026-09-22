//! UniFFI surface for the Android app.
//!
//! Wraps the core `notp` crate without exposing the desktop password-based
//! envelope: every entry point operates on a pre-derived 32-byte key, which
//! the Android side derives from a recovery key or unwraps from a Keystore
//! key that requires user authentication. The crate uses the v3 envelope
//! (`NOTP3` magic) so a file written by the mobile app cannot be opened on
//! the desktop binary and vice versa.

use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use notp::{
    otp::{self, Algorithm},
    qr_import,
    settings::{AppSettings as CoreAppSettings, Theme as CoreTheme},
    storage::{self, Account as CoreAccount, EnvelopeFormat, Vault, VaultStore},
};
use uuid::Uuid;

uniffi::setup_scaffolding!();

const VAULT_FORMAT_VERSION: u32 = 3;

/// Errors surfaced across the FFI boundary. `Anyhow` is not FFI-safe, so we
/// funnel every error through a typed enum and carry a fallback message for
/// unexpected failures.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum VaultError {
    #[error("The vault was created with a different envelope and cannot be opened here")]
    WrongEnvelope,
    #[error("The secret is not valid Base32")]
    InvalidSecret,
    #[error("The vault file is corrupted")]
    CorruptedVault,
    #[error("A vault already exists at this path")]
    AlreadyExists,
    #[error("No vault exists at this path")]
    NotFound,
    #[error("{message}")]
    Other { message: String },
}

impl VaultError {
    fn from_anyhow(error: uniffi::deps::anyhow::Error) -> Self {
        let message = format!("{error:#}");
        if message.contains("already exists") {
            return Self::AlreadyExists;
        }
        if message.contains("does not exist") {
            return Self::NotFound;
        }
        if message.contains("Android") || message.contains("desktop") {
            return Self::WrongEnvelope;
        }
        if message.contains("Secret") || message.contains("Base32") {
            return Self::InvalidSecret;
        }
        if message.contains("corrupt") || message.contains("deserialize") {
            return Self::CorruptedVault;
        }
        Self::Other { message }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum AlgorithmDto {
    Sha1,
    Sha256,
    Sha512,
}

impl From<Algorithm> for AlgorithmDto {
    fn from(algorithm: Algorithm) -> Self {
        match algorithm {
            Algorithm::Sha1 => Self::Sha1,
            Algorithm::Sha256 => Self::Sha256,
            Algorithm::Sha512 => Self::Sha512,
        }
    }
}

impl From<AlgorithmDto> for Algorithm {
    fn from(algorithm: AlgorithmDto) -> Self {
        match algorithm {
            AlgorithmDto::Sha1 => Self::Sha1,
            AlgorithmDto::Sha256 => Self::Sha256,
            AlgorithmDto::Sha512 => Self::Sha512,
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct OtpParamsDto {
    pub issuer: String,
    pub label: String,
    pub secret: String,
    pub digits: u8,
    pub period: u32,
    pub algorithm: AlgorithmDto,
}

impl From<qr_import::OtpParams> for OtpParamsDto {
    fn from(params: qr_import::OtpParams) -> Self {
        Self {
            issuer: params.issuer,
            label: params.label,
            secret: params.secret,
            digits: params.digits,
            period: params.period,
            algorithm: params.algorithm.into(),
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct AccountDto {
    pub id: String,
    pub issuer: String,
    pub name: String,
    pub digits: u8,
    pub period: u32,
    pub algorithm: AlgorithmDto,
    pub added_at: u64,
    pub last_used_at: Option<u64>,
    pub use_count: u64,
}

impl From<&CoreAccount> for AccountDto {
    fn from(account: &CoreAccount) -> Self {
        Self {
            id: account.id.to_string(),
            issuer: account.issuer.clone(),
            name: account.name.clone(),
            digits: account.digits,
            period: account.period,
            algorithm: account.algorithm.into(),
            added_at: account.added_at,
            last_used_at: account.last_used_at,
            use_count: account.use_count,
        }
    }
}

#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum Theme {
    System,
    Light,
    Dark,
}

impl From<CoreTheme> for Theme {
    fn from(theme: CoreTheme) -> Self {
        match theme {
            CoreTheme::System => Self::System,
            CoreTheme::Light => Self::Light,
            CoreTheme::Dark => Self::Dark,
        }
    }
}

impl From<Theme> for CoreTheme {
    fn from(theme: Theme) -> Self {
        match theme {
            Theme::System => Self::System,
            Theme::Light => Self::Light,
            Theme::Dark => Self::Dark,
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct AppSettingsDto {
    pub auto_lock_seconds: u64,
    pub clipboard_clear_seconds: u64,
    pub theme: Theme,
}

/// Live vault held by the Android side. Wraps the in-memory `Vault` behind
/// a `Mutex` so the UI can call mutating methods from coroutines without
/// taking ownership.
#[derive(uniffi::Object)]
pub struct NotpVault {
    inner: Mutex<Vault>,
}

#[uniffi::export]
impl NotpVault {
    pub fn data(&self) -> Result<Vec<AccountDto>, VaultError> {
        let guard = self.inner.lock().map_err(|error| VaultError::Other {
            message: format!("Vault lock poisoned: {error}"),
        })?;
        Ok(guard.data().accounts.iter().map(AccountDto::from).collect())
    }

    pub fn save(&self) -> Result<(), VaultError> {
        let guard = self.inner.lock().map_err(|error| VaultError::Other {
            message: format!("Vault lock poisoned: {error}"),
        })?;
        guard.save().map_err(VaultError::from_anyhow)
    }

    pub fn add_account(
        &self,
        issuer: String,
        name: String,
        secret: String,
        digits: u8,
        period: u32,
        algorithm: AlgorithmDto,
    ) -> Result<String, VaultError> {
        let mut guard = self.inner.lock().map_err(|error| VaultError::Other {
            message: format!("Vault lock poisoned: {error}"),
        })?;
        let account = CoreAccount::new(issuer, name, secret, digits, period, algorithm.into())
            .map_err(VaultError::from_anyhow)?;
        let id = account.id;
        guard
            .data_mut()
            .add_account(account)
            .map_err(VaultError::from_anyhow)?;
        guard.save().map_err(VaultError::from_anyhow)?;
        Ok(id.to_string())
    }

    pub fn replace_account(
        &self,
        id: String,
        issuer: String,
        name: String,
        secret: String,
        digits: u8,
        period: u32,
        algorithm: AlgorithmDto,
    ) -> Result<(), VaultError> {
        let mut guard = self.inner.lock().map_err(|error| VaultError::Other {
            message: format!("Vault lock poisoned: {error}"),
        })?;
        let parsed = Uuid::parse_str(&id).map_err(|error| VaultError::Other {
            message: format!("Invalid account id: {error}"),
        })?;
        let position = guard
            .data()
            .accounts
            .iter()
            .position(|account| account.id == parsed)
            .ok_or(VaultError::NotFound)?;
        let mut new_account =
            CoreAccount::new(issuer, name, secret, digits, period, algorithm.into())
                .map_err(VaultError::from_anyhow)?;
        let previous = guard.data().accounts[position].clone();
        new_account.inherit_identity_from(&previous);
        guard.data_mut().accounts[position] = new_account;
        guard.save().map_err(VaultError::from_anyhow)?;
        Ok(())
    }

    pub fn remove_account(&self, id: String) -> Result<bool, VaultError> {
        let mut guard = self.inner.lock().map_err(|error| VaultError::Other {
            message: format!("Vault lock poisoned: {error}"),
        })?;
        let parsed = Uuid::parse_str(&id).map_err(|error| VaultError::Other {
            message: format!("Invalid account id: {error}"),
        })?;
        let removed = guard
            .remove_account(parsed)
            .map_err(VaultError::from_anyhow)?;
        Ok(removed.is_some())
    }

    pub fn reorder_account(&self, id: String, new_position: u32) -> Result<u32, VaultError> {
        let mut guard = self.inner.lock().map_err(|error| VaultError::Other {
            message: format!("Vault lock poisoned: {error}"),
        })?;
        let parsed = Uuid::parse_str(&id).map_err(|error| VaultError::Other {
            message: format!("Invalid account id: {error}"),
        })?;
        let new_index = guard
            .reorder_account(parsed, new_position as usize)
            .map_err(VaultError::from_anyhow)?;
        match new_index {
            Some(position) => Ok(position as u32),
            None => Err(VaultError::NotFound),
        }
    }

    pub fn record_use(&self, id: String) -> Result<(), VaultError> {
        let mut guard = self.inner.lock().map_err(|error| VaultError::Other {
            message: format!("Vault lock poisoned: {error}"),
        })?;
        let parsed = Uuid::parse_str(&id).map_err(|error| VaultError::Other {
            message: format!("Invalid account id: {error}"),
        })?;
        let account = guard
            .data_mut()
            .accounts
            .iter_mut()
            .find(|account| account.id == parsed)
            .ok_or(VaultError::NotFound)?;
        account.record_use();
        guard.save().map_err(VaultError::from_anyhow)?;
        Ok(())
    }

    pub fn generate_code(&self, id: String, timestamp: u64) -> Result<String, VaultError> {
        let guard = self.inner.lock().map_err(|error| VaultError::Other {
            message: format!("Vault lock poisoned: {error}"),
        })?;
        let parsed = Uuid::parse_str(&id).map_err(|error| VaultError::Other {
            message: format!("Invalid account id: {error}"),
        })?;
        let account = guard
            .data()
            .accounts
            .iter()
            .find(|account| account.id == parsed)
            .ok_or(VaultError::NotFound)?;
        otp::generate_code(
            account.secret(),
            timestamp,
            account.digits,
            account.period,
            account.algorithm,
        )
        .map_err(VaultError::from_anyhow)
    }

    pub fn reveal_secret(&self, id: String) -> Result<String, VaultError> {
        let guard = self.inner.lock().map_err(|error| VaultError::Other {
            message: format!("Vault lock poisoned: {error}"),
        })?;
        let parsed = Uuid::parse_str(&id).map_err(|error| VaultError::Other {
            message: format!("Invalid account id: {error}"),
        })?;
        let account = guard
            .data()
            .accounts
            .iter()
            .find(|account| account.id == parsed)
            .ok_or(VaultError::NotFound)?;
        Ok(account.secret().to_string())
    }
}

#[uniffi::export]
pub fn default_vault_path() -> Result<String, VaultError> {
    let path = VaultStore::default_path().map_err(VaultError::from_anyhow)?;
    Ok(path.to_string_lossy().into_owned())
}

/// Create a new v3 vault sealed with the supplied raw 32-byte key.
#[uniffi::export]
pub fn vault_create_with_key(
    path: String,
    key_bytes: Vec<u8>,
) -> Result<Arc<NotpVault>, VaultError> {
    if key_bytes.len() != 32 {
        return Err(VaultError::Other {
            message: format!("Vault key must be 32 bytes, got {}", key_bytes.len()),
        });
    }
    let mut key = [0_u8; 32];
    key.copy_from_slice(&key_bytes);
    let store = VaultStore::for_path(&path).map_err(VaultError::from_anyhow)?;
    let vault = store
        .create_with_key(&key)
        .map_err(VaultError::from_anyhow)?;
    Ok(Arc::new(NotpVault {
        inner: Mutex::new(vault),
    }))
}

/// Open an existing v3 vault with the supplied raw 32-byte key.
#[uniffi::export]
pub fn vault_unlock_with_key(
    path: String,
    key_bytes: Vec<u8>,
) -> Result<Arc<NotpVault>, VaultError> {
    if key_bytes.len() != 32 {
        return Err(VaultError::Other {
            message: format!("Vault key must be 32 bytes, got {}", key_bytes.len()),
        });
    }
    let mut key = [0_u8; 32];
    key.copy_from_slice(&key_bytes);
    let store = VaultStore::for_path(&path).map_err(VaultError::from_anyhow)?;
    if !store.exists() {
        return Err(VaultError::NotFound);
    }
    let vault = store
        .unlock_with_key(&key)
        .map_err(VaultError::from_anyhow)?;
    Ok(Arc::new(NotpVault {
        inner: Mutex::new(vault),
    }))
}

/// Inspect the envelope of a vault file without unlocking it. Returns the
/// raw format discriminator so the UI can refuse cross-platform files before
/// prompting the user for a key.
#[derive(Debug, uniffi::Enum)]
pub enum VaultEnvelope {
    V1,
    V3,
    Unknown,
}

#[uniffi::export]
pub fn peek_vault_envelope(path: String) -> Result<VaultEnvelope, VaultError> {
    let bytes = std::fs::read(&path).map_err(|error| VaultError::Other {
        message: format!("Unable to read {path}: {error}"),
    })?;
    Ok(match storage::peek_envelope(&bytes) {
        Some(EnvelopeFormat::V1) => VaultEnvelope::V1,
        Some(EnvelopeFormat::V3) => VaultEnvelope::V3,
        None => VaultEnvelope::Unknown,
    })
}

/// Turn a recovery key string into the raw 32-byte vault key it represents.
/// The input uses the `V3-XXXXX-XXXXX-...` format with CRC32 checksum
/// verification; the prefix is included so future envelopes can ship their
/// own recovery key format without confusion.
#[uniffi::export]
pub fn recovery_key_to_bytes(words: String) -> Result<Vec<u8>, VaultError> {
    recovery_key_internal::parse(&words)
        .map(|bytes| bytes.to_vec())
        .map_err(|message| VaultError::Other { message })
}

/// Render a 32-byte vault key as a recovery key string with checksum and
/// grouping. Accepts a `Vec<u8>` of exactly 32 bytes — UniFFI's fixed-size
/// byte array support is too recent to rely on, so the size is checked at
/// runtime.
#[uniffi::export]
pub fn recovery_key_from_bytes(bytes: Vec<u8>) -> Result<String, VaultError> {
    if bytes.len() != 32 {
        return Err(VaultError::Other {
            message: format!("Recovery key material must be 32 bytes, got {}", bytes.len()),
        });
    }
    let mut key = [0_u8; 32];
    key.copy_from_slice(&bytes);
    Ok(recovery_key_internal::format(&key))
}

#[uniffi::export]
pub fn generate_code(
    secret: String,
    timestamp: u64,
    digits: u8,
    period: u32,
    algorithm: AlgorithmDto,
) -> Result<String, VaultError> {
    otp::generate_code(&secret, timestamp, digits, period, algorithm.into())
        .map_err(VaultError::from_anyhow)
}

#[uniffi::export]
pub fn remaining_seconds(timestamp: u64, period: u32) -> u64 {
    otp::remaining_seconds(timestamp, period)
}

#[uniffi::export]
pub fn current_system_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// Decode a raw otpauth URI into a single `OtpParamsDto`. Used for manual
/// entry fallbacks and as a helper when a QR payload is captured as text.
#[uniffi::export]
pub fn parse_otpauth_uri(uri: String) -> Result<OtpParamsDto, VaultError> {
    qr_import::parse_otpauth(&uri)
        .map(OtpParamsDto::from)
        .map_err(VaultError::from_anyhow)
}

/// Decode raw bytes (typically a JPEG/PNG frame from the camera) into one or
/// more `OtpParamsDto` entries. Returns an empty list if no QR code is
/// detected — callers should treat this as a transient "no result yet"
/// signal rather than an error.
#[uniffi::export]
pub fn decode_qr_from_bytes(bytes: Vec<u8>) -> Result<Vec<OtpParamsDto>, VaultError> {
    qr_import::decode_qr_from_bytes(&bytes)
        .map(|entries| entries.into_iter().map(OtpParamsDto::from).collect())
        .map_err(VaultError::from_anyhow)
}

/// Decode a payload string (already extracted from a QR code) into one or
/// more `OtpParamsDto` entries.
#[uniffi::export]
pub fn decode_qr_payload(payload: String) -> Result<Vec<OtpParamsDto>, VaultError> {
    qr_import::decode_qr_payload(&payload)
        .map(|entries| entries.into_iter().map(OtpParamsDto::from).collect())
        .map_err(VaultError::from_anyhow)
}

#[uniffi::export]
pub fn settings_load() -> Result<AppSettingsDto, VaultError> {
    let settings = CoreAppSettings::load().map_err(VaultError::from_anyhow)?;
    Ok(AppSettingsDto {
        auto_lock_seconds: settings.auto_lock_seconds,
        clipboard_clear_seconds: settings.clipboard_clear_seconds,
        theme: settings.theme.into(),
    })
}

#[uniffi::export]
pub fn settings_save(dto: AppSettingsDto) -> Result<(), VaultError> {
    let settings = CoreAppSettings {
        last_vault_path: None,
        auto_lock_seconds: dto.auto_lock_seconds,
        clipboard_clear_seconds: dto.clipboard_clear_seconds,
        theme: dto.theme.into(),
    }
    .normalized();
    settings.save().map_err(VaultError::from_anyhow)
}

/// Centralized build information exposed to the Android side so the About
/// screen has a single source of truth instead of env! macros scattered
/// across the Kotlin tree.
#[derive(Debug, uniffi::Record)]
pub struct VersionInfo {
    pub version: String,
    pub git_tag: String,
    pub vault_format_version: u32,
}

#[uniffi::export]
pub fn notp_version_info() -> VersionInfo {
    VersionInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        git_tag: env!("NOTP_GIT_TAG").to_string(),
        vault_format_version: VAULT_FORMAT_VERSION,
    }
}

mod recovery_key_internal {
    //! Recovery key encoding for the v3 envelope.
    //!
    //! Format: `V3-<base32 groups of 5 separated by `-`>-<4-hex CRC32>`.
    //! The version prefix is appended to the front so a future envelope can
    //! ship a different recovery key shape without collision. The CRC32
    //! covers the raw 32-byte key.

    const PREFIX: &str = "V3";
    const GROUP_SIZE: usize = 5;
    const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

    pub fn format(bytes: &[u8; 32]) -> String {
        let encoded = base32_encode(bytes);
        let mut grouped = String::with_capacity(encoded.len() + encoded.len() / GROUP_SIZE);
        for (index, chunk) in encoded
            .as_bytes()
            .chunks(GROUP_SIZE)
            .enumerate()
        {
            if index > 0 {
                grouped.push('-');
            }
            grouped.push_str(std::str::from_utf8(chunk).unwrap_or(""));
        }
        let checksum = crc32(bytes);
        format!("{PREFIX}-{grouped}-{checksum:08X}")
    }

    pub fn parse(input: &str) -> Result<[u8; 32], String> {
        let trimmed = input.trim().replace(' ', "");
        let parts: Vec<&str> = trimmed.split('-').collect();
        if parts.len() < 3 {
            return Err("Recovery key has the wrong shape".to_string());
        }
        let prefix = parts[0];
        if prefix != PREFIX {
            return Err(format!(
                "Recovery key version {prefix:?} is not supported (expected {PREFIX:?})"
            ));
        }
        let checksum_str = parts[parts.len() - 1];
        if checksum_str.len() != 8 {
            return Err("Recovery key checksum must be 8 hex characters".to_string());
        }
        let expected_checksum = u32::from_str_radix(checksum_str, 16)
            .map_err(|_| "Recovery key checksum is not valid hex".to_string())?;
        let body: String = parts[1..parts.len() - 1].join("");
        let bytes = base32_decode(&body)?;
        if bytes.len() != 32 {
            return Err(format!(
                "Recovery key must decode to 32 bytes, got {}",
                bytes.len()
            ));
        }
        let mut out = [0_u8; 32];
        out.copy_from_slice(&bytes);
        if crc32(&out) != expected_checksum {
            return Err("Recovery key checksum mismatch".to_string());
        }
        Ok(out)
    }

    fn base32_encode(input: &[u8]) -> String {
        let mut output = String::with_capacity(((input.len() + 4) / 5) * 8);
        let mut buffer: u32 = 0;
        let mut bits: u32 = 0;
        for byte in input {
            buffer = (buffer << 8) | u32::from(*byte);
            bits += 8;
            while bits >= 5 {
                bits -= 5;
                output.push(ALPHABET[((buffer >> bits) & 0x1f) as usize] as char);
            }
        }
        if bits > 0 {
            output.push(ALPHABET[((buffer << (5 - bits)) & 0x1f) as usize] as char);
        }
        output
    }

    fn base32_decode(input: &str) -> Result<Vec<u8>, String> {
        let mut output = Vec::new();
        let mut buffer = 0_u32;
        let mut bits = 0_u32;
        for character in input.chars() {
            let value = match character {
                'A'..='Z' => u32::from(character as u8 - b'A'),
                '2'..='7' => u32::from(character as u8 - b'2') + 26,
                _ => return Err(format!("Invalid base32 character {character:?}")),
            };
            buffer = (buffer << 5) | value;
            bits += 5;
            if bits >= 8 {
                bits -= 8;
                output.push(((buffer >> bits) & 0xff) as u8);
                buffer &= (1_u32 << bits) - 1;
            }
        }
        if bits >= 5 {
            return Err("Invalid base32 length".to_string());
        }
        if bits > 0 && (buffer & ((1_u32 << bits) - 1)) != 0 {
            return Err("Invalid base32 padding bits".to_string());
        }
        Ok(output)
    }

    fn crc32(bytes: &[u8]) -> u32 {
        let mut table: [u32; 256] = [0; 256];
        for (i, entry) in table.iter_mut().enumerate() {
            let mut value = i as u32;
            for _ in 0..8 {
                value = if value & 1 != 0 {
                    0xedb8_8320 ^ (value >> 1)
                } else {
                    value >> 1
                };
            }
            *entry = value;
        }
        let mut crc: u32 = 0xffff_ffff;
        for byte in bytes {
            let index = ((crc ^ u32::from(*byte)) & 0xff) as usize;
            crc = (crc >> 8) ^ table[index];
        }
        crc ^ 0xffff_ffff
    }
}

// Make the std types UniFFI is unaware of (Arc, Box, etc.) usable from the
// generated Kotlin code without further annotations.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_key_round_trip() {
        let key = [42_u8; 32];
        let formatted = recovery_key_from_bytes(key.to_vec()).unwrap();
        assert!(formatted.starts_with("V3-"));
        assert!(formatted.ends_with(&format!("-{:08X}", recovery_key_checksum(&key))));
        let recovered = recovery_key_to_bytes(formatted.clone()).unwrap();
        assert_eq!(recovered, key.to_vec());
    }

    #[test]
    fn recovery_key_rejects_bad_checksum() {
        let key = [7_u8; 32];
        let mut formatted = recovery_key_from_bytes(key.to_vec()).unwrap();
        // Flip the last hex character of the checksum.
        let last = formatted.pop().unwrap();
        let replacement = if last == '0' { '1' } else { '0' };
        formatted.push(replacement);
        assert!(recovery_key_to_bytes(formatted).is_err());
    }

    #[test]
    fn recovery_key_rejects_wrong_size() {
        let bad = vec![0_u8; 31];
        assert!(recovery_key_from_bytes(bad).is_err());
    }

    fn recovery_key_checksum(bytes: &[u8]) -> u32 {
        let mut table: [u32; 256] = [0; 256];
        for (i, entry) in table.iter_mut().enumerate() {
            let mut value = i as u32;
            for _ in 0..8 {
                value = if value & 1 != 0 {
                    0xedb8_8320 ^ (value >> 1)
                } else {
                    value >> 1
                };
            }
            *entry = value;
        }
        let mut crc: u32 = 0xffff_ffff;
        for byte in bytes {
            let index = ((crc ^ u32::from(*byte)) & 0xff) as usize;
            crc = (crc >> 8) ^ table[index];
        }
        crc ^ 0xffff_ffff
    }
}