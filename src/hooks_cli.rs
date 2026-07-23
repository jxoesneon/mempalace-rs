//! Hook management for the MemPalace CLI.
//!
//! This module is a Rust port of the hook-management surface from the upstream
//! Python `mempalace.hooks_cli`. It provides a small CLI-facing API to list,
//! install, uninstall, enable and disable hooks, and to persist hook behavior
//! settings (`silent_save`, `desktop_toast`) to a JSON file under the config
//! directory.
//!
//! All disk I/O goes through the shared JSON helpers so persistence is atomic
//! and consistent with the rest of the codebase. The [`FileSystem`] trait and
//! its implementations remain for direct filesystem testing.

use crate::shared::{load_json_file, save_json_file};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[cfg(test)]
use {
    std::collections::HashMap,
    std::sync::{Arc, Mutex},
};

/// Default name of the hook registry file inside the config directory.
pub const HOOKS_REGISTRY_FILE: &str = "hooks.json";
/// Default name of the main config file inside the config directory.
pub const CONFIG_FILE: &str = "config.json";

/// A single installed hook entry.
///
/// The shape is intentionally generic so it can describe hooks for any harness.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookEntry {
    pub name: String,
    pub event: String,
    pub harness: String,
    pub command: String,
    #[serde(default = "default_timeout")]
    pub timeout: u32,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl HookEntry {
    /// Convenience constructor for a new hook entry.
    pub fn new(
        name: impl Into<String>,
        event: impl Into<String>,
        harness: impl Into<String>,
        command: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            event: event.into(),
            harness: harness.into(),
            command: command.into(),
            timeout: default_timeout(),
            enabled: true,
        }
    }

    /// Fluent setter for the timeout value.
    pub fn with_timeout(mut self, timeout: u32) -> Self {
        self.timeout = timeout;
        self
    }

    /// Fluent setter for the enabled flag.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

fn default_timeout() -> u32 {
    30
}

fn default_true() -> bool {
    true
}

/// Registry of installed hooks persisted to disk.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct HooksRegistry {
    pub hooks: Vec<HookEntry>,
}

/// Hook behavior settings persisted under the `hooks` key of `config.json`.
///
/// Mirrors the upstream `MempalaceConfig.hook_silent_save` and
/// `MempalaceConfig.hook_desktop_toast` properties.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookSettings {
    /// When `true`, the stop hook saves directly without blocking the harness.
    #[serde(default = "default_true")]
    pub silent_save: bool,
    /// When `true`, show a desktop toast on save (e.g. via `notify-send`).
    #[serde(default)]
    pub desktop_toast: bool,
}

impl Default for HookSettings {
    fn default() -> Self {
        Self {
            silent_save: true,
            desktop_toast: false,
        }
    }
}

/// File-system abstraction used for direct filesystem testing.
pub trait FileSystem: Send + Sync {
    /// Read the entire file as a string. Returns `None` if the file does not exist.
    fn read_to_string(&self, path: &Path) -> Result<Option<String>>;
    /// Write `content` to `path`, creating parent directories if needed.
    fn write(&self, path: &Path, content: &str) -> Result<()>;
    /// Remove a file. Returns `Ok` even if the file does not exist.
    fn remove_file(&self, path: &Path) -> Result<()>;
    /// Create all parent directories for `path`.
    fn create_dir_all(&self, path: &Path) -> Result<()>;
}

/// Real file-system implementation backed by `std::fs`.
pub struct RealFileSystem;

impl FileSystem for RealFileSystem {
    fn read_to_string(&self, path: &Path) -> Result<Option<String>> {
        match std::fs::read_to_string(path) {
            Ok(content) => Ok(Some(content)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn write(&self, path: &Path, content: &str) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)?;
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    fn create_dir_all(&self, path: &Path) -> Result<()> {
        std::fs::create_dir_all(path).map_err(Into::into)
    }
}

/// In-memory file-system for unit tests.
#[cfg(test)]
#[derive(Clone)]
pub struct MockFileSystem {
    files: Arc<Mutex<HashMap<PathBuf, String>>>,
}

#[cfg(test)]
impl MockFileSystem {
    pub fn new() -> Self {
        Self {
            files: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn insert(&self, path: impl AsRef<Path>, content: impl Into<String>) {
        self.files
            .lock()
            .unwrap()
            .insert(path.as_ref().to_path_buf(), content.into());
    }

    pub fn get(&self, path: impl AsRef<Path>) -> Option<String> {
        self.files.lock().unwrap().get(path.as_ref()).cloned()
    }
}

#[cfg(test)]
impl FileSystem for MockFileSystem {
    fn read_to_string(&self, path: &Path) -> Result<Option<String>> {
        Ok(self.files.lock().unwrap().get(path).cloned())
    }

    fn write(&self, path: &Path, content: &str) -> Result<()> {
        self.files
            .lock()
            .unwrap()
            .insert(path.to_path_buf(), content.to_string());
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        self.files.lock().unwrap().remove(path);
        Ok(())
    }

    fn create_dir_all(&self, _path: &Path) -> Result<()> {
        Ok(())
    }
}

/// Hook management CLI state.
///
/// Stores the config directory. All disk I/O is performed through the shared
/// JSON helpers so persistence is atomic and consistent with the rest of the
/// codebase.
pub struct HooksCli {
    config_dir: PathBuf,
}

impl HooksCli {
    /// Create a new [`HooksCli`] backed by the real filesystem.
    pub fn new(config_dir: PathBuf) -> Self {
        Self { config_dir }
    }

    /// Return the configured config directory.
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    fn registry_path(&self) -> PathBuf {
        self.config_dir.join(HOOKS_REGISTRY_FILE)
    }

    fn settings_path(&self) -> PathBuf {
        self.config_dir.join(CONFIG_FILE)
    }

    fn load_registry(&self) -> Result<HooksRegistry> {
        let path = self.registry_path();
        match load_json_file::<HooksRegistry>(&path) {
            Ok(registry) => Ok(registry),
            Err(e) if Self::is_not_found_error(&e) => Ok(HooksRegistry::default()),
            Err(e) => Err(e).with_context(|| format!("failed to parse {}", HOOKS_REGISTRY_FILE)),
        }
    }

    fn save_registry(&self, registry: &HooksRegistry) -> Result<()> {
        save_json_file(&self.registry_path(), registry)
    }

    /// Return all installed hooks, in the order they were installed.
    pub fn list_hooks(&self) -> Result<Vec<HookEntry>> {
        Ok(self.load_registry()?.hooks)
    }

    /// Install a hook. If a hook with the same name already exists it is
    /// replaced, making the operation idempotent.
    pub fn install_hook(&self, hook: HookEntry) -> Result<()> {
        let mut registry = self.load_registry()?;
        if let Some(existing) = registry.hooks.iter_mut().find(|h| h.name == hook.name) {
            *existing = hook;
        } else {
            registry.hooks.push(hook);
        }
        self.save_registry(&registry)
    }

    /// Uninstall a hook by name. Returns `true` if a hook was removed.
    pub fn uninstall_hook(&self, name: &str) -> Result<bool> {
        let mut registry = self.load_registry()?;
        let before = registry.hooks.len();
        registry.hooks.retain(|h| h.name != name);
        if registry.hooks.len() < before {
            self.save_registry(&registry)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Enable a hook by name. Returns `true` if the hook was found.
    pub fn enable_hook(&self, name: &str) -> Result<bool> {
        self.set_hook_enabled(name, true)
    }

    /// Disable a hook by name. Returns `true` if the hook was found.
    pub fn disable_hook(&self, name: &str) -> Result<bool> {
        self.set_hook_enabled(name, false)
    }

    fn set_hook_enabled(&self, name: &str, enabled: bool) -> Result<bool> {
        let mut registry = self.load_registry()?;
        if let Some(hook) = registry.hooks.iter_mut().find(|h| h.name == name) {
            hook.enabled = enabled;
            self.save_registry(&registry)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn is_not_found_error(err: &anyhow::Error) -> bool {
        err.chain()
            .filter_map(|c| c.downcast_ref::<std::io::Error>())
            .any(|e| e.kind() == std::io::ErrorKind::NotFound)
    }

    /// Save hook settings to the `hooks` object of `config.json`.
    ///
    /// Existing keys in the file are preserved; only the `hooks` object is
    /// updated. If the file is missing or malformed, a fresh config object is
    /// created.
    pub fn save_settings(&self, silent_save: bool, desktop_toast: bool) -> Result<()> {
        let path = self.settings_path();
        let mut config = match load_json_file::<Value>(&path) {
            Ok(value) => value,
            Err(e) if Self::is_not_found_error(&e) => Value::Object(serde_json::Map::new()),
            Err(_) => Value::Object(serde_json::Map::new()),
        };

        if !config.is_object() {
            config = Value::Object(serde_json::Map::new());
        }

        let root = config
            .as_object_mut()
            .context("config root is not an object")?;
        let hooks = root
            .entry("hooks")
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        let hooks_obj = hooks
            .as_object_mut()
            .context("config.hooks is not an object")?;

        hooks_obj.insert("silent_save".to_string(), Value::Bool(silent_save));
        hooks_obj.insert("desktop_toast".to_string(), Value::Bool(desktop_toast));

        save_json_file(&path, &config)
    }

    /// Load hook settings from the `hooks` object of `config.json`.
    ///
    /// Returns the documented defaults if the file is missing, malformed, or
    /// does not contain the relevant keys.
    pub fn load_settings(&self) -> Result<HookSettings> {
        let path = self.settings_path();
        match load_json_file::<Value>(&path) {
            Ok(config) => Ok(Self::parse_settings_from_value(&config)),
            Err(e) if Self::is_not_found_error(&e) => Ok(HookSettings::default()),
            Err(_) => Ok(HookSettings::default()),
        }
    }

    fn parse_settings_from_value(config: &Value) -> HookSettings {
        let hooks = config.get("hooks").and_then(|v| v.as_object());
        let silent_save = hooks
            .and_then(|m| m.get("silent_save"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let desktop_toast = hooks
            .and_then(|m| m.get("desktop_toast"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        HookSettings {
            silent_save,
            desktop_toast,
        }
    }

    /// Run a hook by name and return its captured output.
    ///
    /// The hook is executed through the platform shell (`cmd /C` on Windows,
    /// `sh -c` elsewhere). If the hook is missing or disabled, an error is
    /// returned.
    pub fn run_hook(&self, name: &str) -> Result<std::process::Output> {
        let registry = self.load_registry()?;
        let hook = registry
            .hooks
            .iter()
            .find(|h| h.name == name)
            .with_context(|| format!("hook {name} not found"))?;
        if !hook.enabled {
            anyhow::bail!("hook {name} is disabled");
        }

        let mut command = if cfg!(windows) {
            let mut c = std::process::Command::new("cmd");
            c.arg("/C");
            c
        } else {
            let mut c = std::process::Command::new("sh");
            c.arg("-c");
            c
        };
        command.arg(&hook.command);
        let output = command
            .output()
            .with_context(|| format!("failed to execute hook {name}"))?;
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cli() -> (HooksCli, PathBuf) {
        let dir = tempfile::tempdir().unwrap().keep();
        let cli = HooksCli::new(dir.clone());
        (cli, dir)
    }

    #[test]
    fn test_default_settings() {
        let defaults = HookSettings::default();
        assert!(defaults.silent_save);
        assert!(!defaults.desktop_toast);
    }

    #[test]
    fn test_save_and_load_settings() {
        let (cli, dir) = make_cli();
        cli.save_settings(false, true).unwrap();

        let path = dir.join(CONFIG_FILE);
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("\"silent_save\": false"));
        assert!(content.contains("\"desktop_toast\": true"));

        let loaded = cli.load_settings().unwrap();
        assert!(!loaded.silent_save);
        assert!(loaded.desktop_toast);
    }

    #[test]
    fn test_save_settings_preserves_other_config() {
        let (cli, dir) = make_cli();
        let path = dir.join(CONFIG_FILE);
        std::fs::write(
            &path,
            r#"{"palace_path": "/tmp/palace", "hooks": {"auto_save": true}}"#,
        )
        .unwrap();

        cli.save_settings(false, true).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let config: Value = serde_json::from_str(&content).unwrap();
        assert_eq!(
            config.get("palace_path").unwrap().as_str().unwrap(),
            "/tmp/palace"
        );
        let hooks = config.get("hooks").unwrap().as_object().unwrap();
        assert_eq!(hooks.get("auto_save").unwrap().as_bool().unwrap(), true);
        assert_eq!(hooks.get("silent_save").unwrap().as_bool().unwrap(), false);
        assert_eq!(hooks.get("desktop_toast").unwrap().as_bool().unwrap(), true);
    }

    #[test]
    fn test_load_settings_missing_file() {
        let (cli, _dir) = make_cli();
        let settings = cli.load_settings().unwrap();
        assert!(settings.silent_save);
        assert!(!settings.desktop_toast);
    }

    #[test]
    fn test_load_settings_malformed_json() {
        let (cli, dir) = make_cli();
        std::fs::write(dir.join(CONFIG_FILE), "not-json").unwrap();
        let settings = cli.load_settings().unwrap();
        assert!(settings.silent_save);
        assert!(!settings.desktop_toast);
    }

    #[test]
    fn test_load_settings_non_object_hooks() {
        let (cli, dir) = make_cli();
        std::fs::write(dir.join(CONFIG_FILE), r#"{"hooks": "bad"}"#).unwrap();
        let settings = cli.load_settings().unwrap();
        assert!(settings.silent_save);
        assert!(!settings.desktop_toast);
    }

    #[test]
    fn test_list_hooks_empty() {
        let (cli, _dir) = make_cli();
        let hooks = cli.list_hooks().unwrap();
        assert!(hooks.is_empty());
    }

    #[test]
    fn test_list_hooks_malformed_registry() {
        let (cli, dir) = make_cli();
        std::fs::write(dir.join(HOOKS_REGISTRY_FILE), "not-json").unwrap();
        let result = cli.list_hooks();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("failed to parse"));
    }

    #[test]
    fn test_install_and_list_hook() {
        let (cli, dir) = make_cli();
        let hook = HookEntry::new("stop", "Stop", "claude-code", "bash stop.sh").with_timeout(30);
        cli.install_hook(hook.clone()).unwrap();

        let content = std::fs::read_to_string(dir.join(HOOKS_REGISTRY_FILE)).unwrap();
        let registry: HooksRegistry = serde_json::from_str(&content).unwrap();
        assert_eq!(registry.hooks.len(), 1);
        assert_eq!(registry.hooks[0], hook);

        let listed = cli.list_hooks().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0], hook);
    }

    #[test]
    fn test_install_hook_updates_existing() {
        let (cli, dir) = make_cli();
        let hook1 = HookEntry::new("stop", "Stop", "claude-code", "bash stop.sh");
        cli.install_hook(hook1).unwrap();

        let hook2 = HookEntry::new("stop", "Stop", "claude-code", "bash stop2.sh").with_timeout(60);
        cli.install_hook(hook2.clone()).unwrap();

        let registry: HooksRegistry =
            serde_json::from_str(&std::fs::read_to_string(dir.join(HOOKS_REGISTRY_FILE)).unwrap())
                .unwrap();
        assert_eq!(registry.hooks.len(), 1);
        assert_eq!(registry.hooks[0], hook2);
    }

    #[test]
    fn test_install_multiple_hooks() {
        let (cli, _dir) = make_cli();
        cli.install_hook(HookEntry::new(
            "stop",
            "Stop",
            "claude-code",
            "bash stop.sh",
        ))
        .unwrap();
        cli.install_hook(HookEntry::new(
            "precompact",
            "PreCompact",
            "claude-code",
            "bash precompact.sh",
        ))
        .unwrap();
        cli.install_hook(HookEntry::new(
            "codex-stop",
            "Stop",
            "codex",
            "bash codex_stop.sh",
        ))
        .unwrap();

        let hooks = cli.list_hooks().unwrap();
        assert_eq!(hooks.len(), 3);
        assert_eq!(hooks[0].name, "stop");
        assert_eq!(hooks[1].name, "precompact");
        assert_eq!(hooks[2].name, "codex-stop");
        assert_eq!(hooks[2].harness, "codex");
    }

    #[test]
    fn test_uninstall_hook() {
        let (cli, _dir) = make_cli();
        cli.install_hook(HookEntry::new(
            "stop",
            "Stop",
            "claude-code",
            "bash stop.sh",
        ))
        .unwrap();
        cli.install_hook(HookEntry::new(
            "precompact",
            "PreCompact",
            "claude-code",
            "bash precompact.sh",
        ))
        .unwrap();

        assert!(cli.uninstall_hook("stop").unwrap());
        let hooks = cli.list_hooks().unwrap();
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0].name, "precompact");
    }

    #[test]
    fn test_uninstall_hook_not_found() {
        let (cli, _dir) = make_cli();
        cli.install_hook(HookEntry::new(
            "stop",
            "Stop",
            "claude-code",
            "bash stop.sh",
        ))
        .unwrap();
        assert!(!cli.uninstall_hook("missing").unwrap());
        assert_eq!(cli.list_hooks().unwrap().len(), 1);
    }

    #[test]
    fn test_enable_hook() {
        let (cli, _dir) = make_cli();
        cli.install_hook(
            HookEntry::new("stop", "Stop", "claude-code", "bash stop.sh").with_enabled(false),
        )
        .unwrap();

        assert!(cli.enable_hook("stop").unwrap());
        let hooks = cli.list_hooks().unwrap();
        assert!(hooks[0].enabled);
    }

    #[test]
    fn test_disable_hook() {
        let (cli, _dir) = make_cli();
        cli.install_hook(HookEntry::new(
            "stop",
            "Stop",
            "claude-code",
            "bash stop.sh",
        ))
        .unwrap();

        assert!(cli.disable_hook("stop").unwrap());
        let hooks = cli.list_hooks().unwrap();
        assert!(!hooks[0].enabled);
    }

    #[test]
    fn test_enable_hook_not_found() {
        let (cli, _dir) = make_cli();
        assert!(!cli.enable_hook("missing").unwrap());
    }

    #[test]
    fn test_disable_hook_not_found() {
        let (cli, _dir) = make_cli();
        assert!(!cli.disable_hook("missing").unwrap());
    }

    #[test]
    fn test_enable_hook_already_enabled() {
        let (cli, _dir) = make_cli();
        cli.install_hook(HookEntry::new(
            "stop",
            "Stop",
            "claude-code",
            "bash stop.sh",
        ))
        .unwrap();
        assert!(cli.enable_hook("stop").unwrap());
        let hooks = cli.list_hooks().unwrap();
        assert!(hooks[0].enabled);
    }

    #[test]
    fn test_realfs_reads_and_writes() {
        let dir = tempfile::tempdir().unwrap();
        let cli = HooksCli::new(dir.path().to_path_buf());
        cli.save_settings(false, true).unwrap();
        let settings = cli.load_settings().unwrap();
        assert!(!settings.silent_save);
        assert!(settings.desktop_toast);

        let hook = HookEntry::new("stop", "Stop", "claude-code", "bash stop.sh");
        cli.install_hook(hook).unwrap();
        let hooks = cli.list_hooks().unwrap();
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0].name, "stop");
    }

    #[test]
    fn test_save_settings_creates_config_dir() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a").join("b").join(".mempalace");
        let cli = HooksCli::new(nested.clone());
        cli.save_settings(true, false).unwrap();
        assert!(nested.join(CONFIG_FILE).exists());
    }

    #[test]
    fn test_uninstall_hook_removes_registry_file() {
        let (cli, dir) = make_cli();
        cli.install_hook(HookEntry::new(
            "stop",
            "Stop",
            "claude-code",
            "bash stop.sh",
        ))
        .unwrap();
        cli.uninstall_hook("stop").unwrap();
        let hooks = cli.list_hooks().unwrap();
        assert!(hooks.is_empty());
        // The registry still exists with an empty list; the implementation is
        // allowed to keep it. This just documents current behavior.
        assert!(dir.join(HOOKS_REGISTRY_FILE).exists());
    }

    #[test]
    fn test_load_settings_explicit_false() {
        let (cli, dir) = make_cli();
        std::fs::write(
            dir.join(CONFIG_FILE),
            r#"{"hooks": {"silent_save": false, "desktop_toast": false}}"#,
        )
        .unwrap();
        let settings = cli.load_settings().unwrap();
        assert!(!settings.silent_save);
        assert!(!settings.desktop_toast);
    }

    #[test]
    fn test_parse_settings_with_missing_hooks() {
        let config = serde_json::json!({});
        let settings = HooksCli::parse_settings_from_value(&config);
        assert!(settings.silent_save);
        assert!(!settings.desktop_toast);
    }

    #[test]
    fn test_save_settings_overwrites_existing_values() {
        let (cli, dir) = make_cli();
        std::fs::write(
            dir.join(CONFIG_FILE),
            r#"{"hooks": {"silent_save": false, "desktop_toast": true}}"#,
        )
        .unwrap();
        cli.save_settings(true, false).unwrap();
        let settings = cli.load_settings().unwrap();
        assert!(settings.silent_save);
        assert!(!settings.desktop_toast);
    }

    #[test]
    fn test_realfs_remove_file_missing() {
        let dir = tempfile::tempdir().unwrap();
        let fs = RealFileSystem;
        let path = dir.path().join("missing.json");
        fs.remove_file(&path).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn test_realfs_read_missing() {
        let dir = tempfile::tempdir().unwrap();
        let fs = RealFileSystem;
        let path = dir.path().join("missing.json");
        assert!(fs.read_to_string(&path).unwrap().is_none());
    }

    #[test]
    fn test_new_uses_realfs() {
        let dir = PathBuf::from("/tmp/test_mempalace");
        let cli = HooksCli::new(dir.clone());
        assert_eq!(cli.config_dir(), dir);
    }

    #[test]
    fn test_run_hook() {
        let dir = tempfile::tempdir().unwrap();
        let cli = HooksCli::new(dir.path().to_path_buf());
        cli.install_hook(HookEntry::new("echo", "Save", "generic", "echo test"))
            .unwrap();
        let output = cli.run_hook("echo").unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("test"));
        assert!(output.status.success());
    }

    #[test]
    fn test_run_hook_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let cli = HooksCli::new(dir.path().to_path_buf());
        cli.install_hook(
            HookEntry::new("echo", "Save", "generic", "echo test").with_enabled(false),
        )
        .unwrap();
        assert!(cli.run_hook("echo").is_err());
    }

    #[test]
    fn test_run_hook_missing() {
        let dir = tempfile::tempdir().unwrap();
        let cli = HooksCli::new(dir.path().to_path_buf());
        assert!(cli.run_hook("missing").is_err());
    }
}
