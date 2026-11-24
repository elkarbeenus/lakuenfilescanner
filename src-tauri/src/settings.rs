use crate::{paths, util};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct WhitelistEntry {
    pub sha256: String,
    pub name: String,
    pub signature: String,
    pub added: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub language: String,
    pub theme: String,
    pub threads: usize,
    pub max_file_mb: u64,
    pub scan_archives: bool,
    pub follow_symlinks: bool,
    pub skip_hidden: bool,
    pub heuristics: bool,
    pub detect_pua: bool,
    pub skip_unchanged: bool,
    pub auto_update_signatures: bool,
    pub preload_engine: bool,
    pub check_app_updates: bool,
    pub notify_on_finish: bool,
    pub exclusions: Vec<String>,
    pub excluded_extensions: Vec<String>,
    pub whitelist: Vec<WhitelistEntry>,
    pub recent_targets: Vec<String>,
    pub onboarding_done: bool,
    pub last_signature_update: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            language: "de".into(),
            theme: "midnight".into(),
            threads: 0,
            max_file_mb: 1024,
            scan_archives: true,
            follow_symlinks: false,
            skip_hidden: false,
            heuristics: true,
            detect_pua: false,
            skip_unchanged: true,
            auto_update_signatures: true,
            preload_engine: true,
            check_app_updates: true,
            notify_on_finish: true,
            exclusions: vec!["/proc".into(), "/sys".into(), "~/.local/share/lakuenfilescanner".into()],
            excluded_extensions: vec![],
            whitelist: vec![],
            recent_targets: vec![],
            onboarding_done: false,
            last_signature_update: 0,
        }
    }
}

impl Settings {
    fn file() -> std::path::PathBuf {
        paths::config_dir().join("settings.json")
    }

    pub fn load() -> Settings {
        util::read_json(&Self::file()).unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        util::write_json(&Self::file(), self)
    }

    pub fn effective_threads(&self) -> usize {
        if self.threads > 0 {
            self.threads.min(64)
        } else {
            std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 32)
        }
    }

    /// Settings that are baked into the clamd config.
    pub fn engine_key(&self) -> (usize, u64, bool, bool) {
        (self.effective_threads(), self.max_file_mb, self.scan_archives, self.detect_pua)
    }

    pub fn push_recent(&mut self, targets: &[String]) {
        for t in targets.iter().rev() {
            self.recent_targets.retain(|x| x != t);
            self.recent_targets.insert(0, t.clone());
        }
        self.recent_targets.truncate(8);
    }
}
