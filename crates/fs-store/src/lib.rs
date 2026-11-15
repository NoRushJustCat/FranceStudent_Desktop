//! Stockage local : réglages (config.json) et clé API (coffre du système).
//! SQLite (chats locaux, compteur de requêtes) viendra plus tard.

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const DEFAULT_BASE_URL: &str = "https://api.francestudent.org";

const SERVICE: &str = "fs-code";
const ACCOUNT: &str = "api-key";
const CONFIG_FILE: &str = "config.json";

/// Réglages non secrets, dans `config.json` (dossier de config de l'app).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub base_url: String,
    pub last_agent_id: Option<i64>,
    /// Dernier dossier de travail du mode Code.
    pub last_workdir: Option<String>,
    /// Accepter sans demander les modifs de fichiers (jamais `run_command`).
    pub auto_accept: bool,
    /// Mode économie : sorties d'outils courtes, moins d'étapes, compaction auto.
    pub eco: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            last_agent_id: Some(-2001),
            last_workdir: None,
            auto_accept: false,
            eco: false,
        }
    }
}

/// Lit `config.json` ; valeurs par défaut si absent ou invalide.
pub fn load(dir: &Path) -> Settings {
    let mut s: Settings = fs::read_to_string(dir.join(CONFIG_FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    if s.base_url.trim().is_empty() {
        s.base_url = DEFAULT_BASE_URL.to_string();
    }
    s
}

pub fn save(dir: &Path, settings: &Settings) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let text = serde_json::to_string_pretty(settings).map_err(io::Error::other)?;
    fs::write(dir.join(CONFIG_FILE), text)
}

fn entry() -> Result<keyring::Entry, keyring::Error> {
    keyring::Entry::new(SERVICE, ACCOUNT)
}

/// Clé API : coffre du système d'abord, variable `FS_KEY`, puis session FranceStudent active.
pub fn get_key() -> Option<String> {
    let stored = entry()
        .ok()
        .and_then(|e| e.get_password().ok())
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty());
    stored
        .or_else(|| {
            std::env::var("FS_KEY")
                .ok()
                .map(|k| k.trim().to_string())
                .filter(|k| !k.is_empty())
        })
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .map(std::path::PathBuf::from)
                .map(|p| p.join(".gemini/config/plugins/eco-quota/fs_token.txt"))
                .and_then(|p| fs::read_to_string(p).ok())
                .map(|k| k.trim().to_string())
                .filter(|k| !k.is_empty())
        })
}

pub fn set_key(key: &str) -> Result<(), String> {
    entry()
        .map_err(|e| e.to_string())?
        .set_password(key.trim())
        .map_err(|e| e.to_string())
}

pub fn clear_key() {
    if let Ok(e) = entry() {
        let _ = e.delete_credential();
    }
}
