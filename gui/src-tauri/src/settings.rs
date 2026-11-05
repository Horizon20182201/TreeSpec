//! GUI settings (design §6.8): a JSON file in the app's support directory.
//! The API key is never stored here; it lives in the Keychain (`secrets`).

use organizer::llm::provider::{LlmPolicy, ProviderConfig};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use ts_rs::TS;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct LlmSettings {
    pub base_url: String,
    pub model: String,
    pub local_only: bool,
    pub no_content: bool,
    pub disabled: bool,
}

impl LlmSettings {
    pub fn policy(&self) -> LlmPolicy {
        LlmPolicy {
            disabled: self.disabled,
            local_only: self.local_only,
            no_content: self.no_content,
        }
    }

    /// The provider configuration `chat` uses, with the same validation as the
    /// environment configuration (GUI design §9.3).
    pub fn provider_config(
        &self,
        api_key: Option<String>,
    ) -> organizer::error::Result<ProviderConfig> {
        let some = |s: &str| (!s.trim().is_empty()).then(|| s.trim().to_owned());
        ProviderConfig::from_settings(
            some(&self.base_url),
            some(&self.model),
            api_key,
            self.policy(),
        )
    }
}

/// Settings shown in the settings sheet.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct Preferences {
    pub llm: LlmSettings,
    /// App used by “用其他应用打开”, e.g. `/Applications/BBEdit.app`.
    pub editor_app: Option<String>,
    /// Rescan when a window becomes active (default off, design §3.5).
    pub scan_on_activate: bool,
    /// Reopen the last workspaces at launch (default on).
    pub reopen_last: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            llm: LlmSettings::default(),
            editor_app: None,
            scan_on_activate: false,
            reopen_last: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Recent {
    pub root: PathBuf,
    /// Workspace state label at the last refresh, e.g. “已同步”.
    pub state: String,
    /// Milliseconds since the Unix epoch.
    pub opened_ms: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    pub root: PathBuf,
    pub width: f64,
    pub height: f64,
    pub inspector: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub preferences: Preferences,
    pub recent: Vec<Recent>,
    /// Roots that were open at the last quit.
    pub open: Vec<PathBuf>,
    pub windows: Vec<WindowState>,
}

pub const MAX_RECENT: usize = 10;

pub fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_millis() as f64)
}

impl Settings {
    /// Missing file: defaults. Unreadable file: moved aside to `.bad` so a
    /// broken file is never silently overwritten, then defaults.
    pub fn load(path: &Path) -> Self {
        let Ok(text) = fs::read_to_string(path) else {
            return Self::default();
        };
        serde_json::from_str(&text).unwrap_or_else(|_| {
            let _ = fs::rename(path, path.with_extension("json.bad"));
            Self::default()
        })
    }

    /// Atomic replace; the file is private to the user.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| std::io::Error::other("no parent"))?;
        fs::create_dir_all(parent)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        file.write_all(serde_json::to_string_pretty(self)?.as_bytes())?;
        file.as_file().sync_all()?;
        file.persist(path).map_err(|e| e.error)?;
        Ok(())
    }

    pub fn touch_recent(&mut self, root: &Path, state: &str) {
        self.recent.retain(|r| r.root != root);
        self.recent.insert(
            0,
            Recent {
                root: root.to_owned(),
                state: state.to_owned(),
                opened_ms: now_ms(),
            },
        );
        self.recent.truncate(MAX_RECENT);
    }

    /// Updates the remembered state label without reordering.
    pub fn note_state(&mut self, root: &Path, state: &str) {
        if let Some(recent) = self.recent.iter_mut().find(|r| r.root == root) {
            state.clone_into(&mut recent.state);
        }
    }

    pub fn window(&self, root: &Path) -> Option<&WindowState> {
        self.windows.iter().find(|w| w.root == root)
    }

    pub fn set_window(&mut self, state: WindowState) {
        self.windows.retain(|w| w.root != state.root);
        self.windows.insert(0, state);
        self.windows.truncate(MAX_RECENT * 2);
    }
}
