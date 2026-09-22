use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const SETTINGS_FILE: &str = "settings.json";

pub const DEFAULT_AUTO_LOCK_SECONDS: u64 = 60;
pub const DEFAULT_CLIPBOARD_CLEAR_SECONDS: u64 = 30;
pub const MIN_AUTO_LOCK_SECONDS: u64 = 5;
pub const MAX_AUTO_LOCK_SECONDS: u64 = 3_600;
pub const MIN_CLIPBOARD_CLEAR_SECONDS: u64 = 0;
pub const MAX_CLIPBOARD_CLEAR_SECONDS: u64 = 600;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AppSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_vault_path: Option<PathBuf>,
    #[serde(default = "default_auto_lock")]
    pub auto_lock_seconds: u64,
    #[serde(default = "default_clipboard_clear")]
    pub clipboard_clear_seconds: u64,
    #[serde(default)]
    pub theme: Theme,
}

fn default_auto_lock() -> u64 {
    DEFAULT_AUTO_LOCK_SECONDS
}

fn default_clipboard_clear() -> u64 {
    DEFAULT_CLIPBOARD_CLEAR_SECONDS
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            last_vault_path: None,
            auto_lock_seconds: DEFAULT_AUTO_LOCK_SECONDS,
            clipboard_clear_seconds: DEFAULT_CLIPBOARD_CLEAR_SECONDS,
            theme: Theme::System,
        }
    }
}

impl AppSettings {
    pub fn normalized(mut self) -> Self {
        self.auto_lock_seconds = self
            .auto_lock_seconds
            .clamp(MIN_AUTO_LOCK_SECONDS, MAX_AUTO_LOCK_SECONDS);
        self.clipboard_clear_seconds = self
            .clipboard_clear_seconds
            .clamp(MIN_CLIPBOARD_CLEAR_SECONDS, MAX_CLIPBOARD_CLEAR_SECONDS);
        self
    }
}

impl AppSettings {
    pub fn load() -> Result<Self> {
        let path = settings_path()?;
        if !path.is_file() {
            return Ok(Self::default());
        }
        let raw = fs::read(&path).context("Unable to read settings")?;
        let settings: Self = serde_json::from_slice(&raw).context("Settings are corrupted")?;
        Ok(settings.normalized())
    }

    pub fn save(&self) -> Result<()> {
        let path = settings_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("Unable to create the settings directory")?;
        }
        let raw = serde_json::to_vec_pretty(self).context("Unable to encode settings")?;
        atomic_write(&path, &raw).context("Unable to write settings")?;
        Ok(())
    }
}

fn settings_path() -> Result<PathBuf> {
    let dir = settings_dir().context("Unable to determine the settings directory")?;
    Ok(dir.join("notp").join(SETTINGS_FILE))
}

fn settings_dir() -> Result<PathBuf> {
    dirs::state_dir()
        .or_else(dirs::config_dir)
        .context("Unable to determine the settings directory")
}

fn atomic_write(path: &std::path::Path, content: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid settings path")
    })?;
    let temporary = parent.join(format!(
        ".settings-{}-{}.tmp",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(content)?;
        file.sync_all()?;
    }
    let result = fs::rename(&temporary, path);
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // XDG_STATE_HOME is process-global, so these tests must run serially.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_tempdir<T>(f: impl FnOnce(&std::path::Path) -> T) -> T {
        let dir = tempfile::tempdir().unwrap();
        f(dir.path())
    }

    fn lock_env() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn loads_defaults_when_missing() {
        let _env = lock_env();
        with_tempdir(|dir| {
            std::env::set_var("XDG_STATE_HOME", dir);
            let settings = AppSettings::load().unwrap();
            assert!(settings.last_vault_path.is_none());
            assert_eq!(settings.auto_lock_seconds, DEFAULT_AUTO_LOCK_SECONDS);
            assert_eq!(
                settings.clipboard_clear_seconds,
                DEFAULT_CLIPBOARD_CLEAR_SECONDS
            );
            assert_eq!(settings.theme, Theme::System);
        });
    }

    #[test]
    fn round_trips_vault_path() {
        let _env = lock_env();
        with_tempdir(|dir| {
            std::env::set_var("XDG_STATE_HOME", dir);
            let mut settings = AppSettings::default();
            let path = PathBuf::from("/tmp/some/vault.notp");
            settings.last_vault_path = Some(path.clone());
            settings.save().unwrap();
            let loaded = AppSettings::load().unwrap();
            assert_eq!(loaded.last_vault_path, Some(path));
        });
    }

    #[test]
    fn clamps_out_of_range_values_on_load() {
        let _env = lock_env();
        with_tempdir(|dir| {
            std::env::set_var("XDG_STATE_HOME", dir);
            std::fs::create_dir_all(dir.join("notp")).unwrap();
            std::fs::write(
                dir.join("notp").join(SETTINGS_FILE),
                r#"{"auto_lock_seconds":1,"clipboard_clear_seconds":100000,"theme":"dark"}"#,
            )
            .unwrap();
            let loaded = AppSettings::load().unwrap();
            assert_eq!(loaded.auto_lock_seconds, MIN_AUTO_LOCK_SECONDS);
            assert_eq!(loaded.clipboard_clear_seconds, MAX_CLIPBOARD_CLEAR_SECONDS);
            assert_eq!(loaded.theme, Theme::Dark);
        });
    }
}
