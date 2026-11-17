//! Plugins au format Claude Code (ex. claude-mem) : on en lit les **skills** (`skills/` et
//! champ `skills` de plugin.json) et les **serveurs MCP** (`.mcp.json` ou `mcpServers`
//! dans `.claude-plugin/plugin.json`). Les hooks (`hooks/hooks.json`), commandes
//! (`commands/`) et agents (`agents/`) des plugins ne sont PAS gérés.
//!
//! Modèle (comme la doc Claude Code) :
//! - une **marketplace** = un dossier/dépôt avec `.claude-plugin/marketplace.json`
//!   (champ `name`, liste `plugins` avec `"source": "./…"`) ;
//! - un **plugin** s'identifie par `nom@marketplace` et peut être activé/désactivé
//!   (état dans `<config>/plugins-state.json`).
//!
//! Plugins lus :
//! 1. ceux de `~/.claude/plugins/installed_plugins.json` (sauf `enabledPlugins: false`
//!    dans `~/.claude/settings.json`) ;
//! 2. chaque sous-dossier de `<config de l'app>/plugins` (ajoutés par `/plugin marketplace add`
//!    ou `/plugin install`), sauf les plugins désactivés.
//!
//! `${CLAUDE_PLUGIN_ROOT}` est remplacé par le dossier du plugin.

use std::collections::{BTreeMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use serde_json::{Map, Value};
use tauri::AppHandle;

use crate::config_dir;
use crate::mcp::ServerConfig;

fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

fn read_text(p: &Path) -> Option<String> {
    fs::read_to_string(p)
        .ok()
        .map(|t| t.trim_start_matches('\u{feff}').to_string())
}

/// Lit un JSON en remplaçant `${CLAUDE_PLUGIN_ROOT}` (chemin échappé pour JSON).
fn read_json(p: &Path, root: Option<&Path>) -> Option<Value> {
    let mut text = read_text(p)?;
    if let Some(r) = root {
        let esc = serde_json::to_string(&r.to_string_lossy()).ok()?;
        text = text.replace("${CLAUDE_PLUGIN_ROOT}", esc.trim_matches('"'));
    }
    serde_json::from_str(&text).ok()
}

fn dir_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn app_entries(config: &Path) -> Vec<PathBuf> {
    let Ok(rd) = fs::read_dir(config.join("plugins")) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = rd
        .flatten()
 .map(|e| e.path())
 .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs
}

fn claude_roots() -> Vec<PathBuf> {
    let Some(claude) = home().map(|h| h.join(".claude")) else {
        return Vec::new();
    };
    let disabled: HashSet<String> = read_json(&claude.join("settings.json"), None)
        .and_then(|v| v.get("enabledPlugins").and_then(Value::as_object).cloned())
 .map(|m| {
     m.into_iter()
  .filter(|(_, v)| v.as_bool() == Some(false))
         .map(|(k, _)| k)
         .collect()
        })
        .unwrap_or_default();
    let installed = read_json(&claude.join("plugins").join("installed_plugins.json"), None);
    let mut out = Vec::new();
    if let Some(map) = installed
 .as_ref()
        .and_then(|v| v.get("plugins"))
        .and_then(Value::as_object)
    {
 for (key, entries) in map {
     if disabled.contains(key) {
         continue;
     }
            // v2 : tableau d'installations ; v1 : un seul objet.
     let list: Vec<&Value> = match entries {
         Value::Array(a) => a.iter().collect(),
         o => vec![o],
     };
     for e in list {
         if let Some(p) = e.get("installPath").and_then(Value::as_str) {
      out.push(PathBuf::from(p));
  }
     }
 }
    }
    out
}

/// Un plugin identifié : `nom@marketplace`.
#[derive(Clone)]
struct Entry {
    name: String,
    /// Nom de la marketplace (champ `name` de marketplace.json, sinon nom du dossier).
    market: String,
    /// Nom du dossier sous `<config>/plugins` ("" pour `~/.claude`).
    store: String,
    path: PathBuf,
}

impl Entry {
    fn key(&self) -> String {
        format!("{}@{}", self.name, self.market)
    }
}

/// Un dossier installé -> ses plugins (ceux que déclare sa marketplace, sinon lui-même).
fn expand(dir: &Path, store: &str) -> Vec<Entry> {
    let market = read_json(&dir.join(".claude-plugin").join("marketplace.json"), None);
    let mname = market
 .as_ref()
        .and_then(|m| m.get("name"))
        .and_then(Value::as_str)
        .filter(|n| !n.is_empty())
        .unwrap_or(store)
        .to_string();
    let mut out: Vec<Entry> = Vec::new();
    if let Some(list) = market
        .as_ref()
 .and_then(|m| m.get("plugins"))
 .and_then(Value::as_array)
    {
        for p in list {
     let Some(rel) = p.get("source").and_then(Value::as_str) else {
  continue;
     };
     if !rel.starts_with("./") || rel.split(['/', '\\']).any(|s| s == "..") || rel.contains(':') {
         continue;
     }
            let d = dir.join(&rel[2..]);
            if !d.is_dir() {
         continue;
     }
     let name = p
         .get("name")
  .and_then(Value::as_str)
  .map(String::from)
  .unwrap_or_else(|| dir_name(&d));
            out.push(Entry {
         name,
  market: mname.clone(),
         store: store.to_string(),
         path: d,
     });
        }
    }
    if out.is_empty() {
 let name = read_json(&dir.join(".claude-plugin").join("plugin.json"), None)
     .and_then(|m| m.get("name").and_then(Value::as_str).map(String::from))
     .unwrap_or_else(|| store.to_string());
        out.push(Entry {
     name,
     market: mname,
     store: store.to_string(),
     path: dir.to_path_buf(),
        });
    }
    out
}

fn app_plugins(config: &Path) -> Vec<Entry> {
    app_entries(config)
 .into_iter()
 .flat_map(|d| {
     let store = dir_name(&d);
            expand(&d, &store)
        })
        .collect()
}

// ---------- État activé / désactivé ----------

fn disabled(config: &Path) -> HashSet<String> {
    read_json(&config.join("plugins-state.json"), None)
 .and_then(|v| v.get("disabled").and_then(Value::as_array).cloned())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
 .unwrap_or_default()
}

fn save_disabled(config: &Path, set: &HashSet<String>) -> Result<(), String> {
    let mut list: Vec<&String> = set.iter().collect();
    list.sort();
    let text = serde_json::to_string_pretty(&serde_json::json!({ "disabled": list }))
        .map_err(|e| e.to_string())?;
    fs::create_dir_all(config).map_err(|e| e.to_string())?;
    fs::write(config.join("plugins-state.json"), text).map_err(|e| e.to_string())
}

/// Dossiers racines des plugins actifs.
pub fn roots(config: &Path) -> Vec<PathBuf> {
    let off = disabled(config);
    let mut out: Vec<PathBuf> = Vec::new();
    let app = app_plugins(config)
        .into_iter()
 .filter(|e| !off.contains(&e.key()))
        .map(|e| e.path);
    for p in claude_roots().into_iter().chain(app) {
 if p.is_dir() && !out.contains(&p) {
     out.push(p);
        }
    }
    out
}

/// `nom@marketplace` (marketplace = son nom ou son dossier), ou `nom` s'il est unique.
fn resolve(config: &Path, spec: &str) -> Result<Entry, String> {
    let spec = spec.trim();
    if spec.is_empty() {
 return Err("plugin manquant : `nom@marketplace`".to_string());
    }
    let all = app_plugins(config);
    let found: Vec<Entry> = match spec.split_once('@') {
 Some((n, m)) => all
     .into_iter()
     .filter(|e| e.name == n && (e.market == m || e.store == m))
            .collect(),
        None => all.into_iter().filter(|e| e.name == spec).collect(),
    };
    match found.len() {
        0 => Err(format!("plugin introuvable : {spec} (/plugin list)")),
        1 => Ok(found[0].clone()),
        _ => Err(format!(
     "« {spec} » est ambigu : {}",
     found.iter().map(Entry::key).collect::<Vec<_>>().join(", ")
        )),
    }
}

/// Dossiers installés dont le nom de dossier ou de marketplace vaut `name` (tous si `None`).
fn market_dirs(config: &Path, name: Option<&str>) -> Vec<PathBuf> {
    app_entries(config)
        .into_iter()
        .filter(|d| match name {
            None => true,
     Some(n) => {
  let store = dir_name(d);
  store == n || expand(d, &store).iter().any(|e| e.market == n)
     }
        })
 .collect()
}

/// Dossiers de skills d'un plugin : `skills/` + chemin(s) du champ `skills` de plugin.json
/// (chaîne ou tableau, relatifs au plugin, sans `..`).
pub fn skill_dirs(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![root.join("skills")];
    let manifest = read_json(&root.join(".claude-plugin").join("plugin.json"), None);
    let list: Vec<String> = match manifest.as_ref().and_then(|m| m.get("skills")) {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) => a.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
 _ => Vec::new(),
    };
    for rel in list {
 let rel = rel.trim_start_matches("./");
        let bad = rel.split(['/', '\\']).any(|p| p == "..")
     || rel.starts_with(['/', '\\'])
     || rel.contains(':');
        if rel.is_empty() || bad {
     continue;
        }
        let d = root.join(rel);
 if !out.contains(&d) {
     out.push(d);
        }
    }
    out
}

/// `{"mcpServers": {...}}`, `{"servers": {...}}` ou directement `{"nom": {...}}`.
fn table(v: &Value) -> Option<&Map<String, Value>> {
    let o = v.as_object()?;
    match o.get("mcpServers").or_else(|| o.get("servers")) {
 Some(x) => x.as_object(),
 None => Some(o),
    }
}

fn servers_of(root: &Path) -> Vec<(String, ServerConfig)> {
    let mut sources: Vec<Value> = Vec::new();
    let manifest = read_json(&root.join(".claude-plugin").join("plugin.json"), Some(root));
    match manifest.as_ref().and_then(|m| m.get("mcpServers")) {
        Some(Value::String(rel)) if !rel.contains("..") => {
     if let Some(v) = read_json(&root.join(rel.trim_start_matches("./")), Some(root)) {
         sources.push(v);
     }
        }
 Some(v @ Value::Object(_)) => sources.push(v.clone()),
        _ => {}
    }
    if let Some(v) = read_json(&root.join(".mcp.json"), Some(root)) {
 sources.push(v);
    }
    let mut out = Vec::new();
    for v in &sources {
        for (name, c) in table(v).into_iter().flatten() {
            if let Ok(cfg) = serde_json::from_value::<ServerConfig>(c.clone()) {
  if cfg.is_valid() {
      out.push((name.clone(), cfg));
         }
     }
        }
    }
    out
}

/// Serveurs MCP déclarés par les plugins actifs (le premier nom rencontré gagne).
pub fn mcp_servers(config: &Path) -> BTreeMap<String, ServerConfig> {
    let mut out = BTreeMap::new();
    for root in roots(config) {
 for (name, cfg) in servers_of(&root) {
     out.entry(name).or_insert(cfg);
        }
    }
    out
}

// ---------- Gestion : /plugin ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    pub name: String,
    /// Nom de la marketplace.
    pub market: String,
    /// Dossier d'installation (pour supprimer / mettre à jour la marketplace).
    pub store: String,
    /// `app` (géré par l'app) ou `claude` (~/.claude, lecture seule).
    pub source: String,
    pub enabled: bool,
    pub path: String,
    pub skills: usize,
    pub mcp: usize,
    pub hooks: bool,
}

fn count_skills(root: &Path) -> usize {
    skill_dirs(root)
 .iter()
        .filter_map(|d| fs::read_dir(d).ok())
 .map(|rd| {
     rd.flatten()
  .filter(|e| e.path().join("SKILL.md").is_file())
         .count()
        })
 .sum()
}

fn info_of(e: &Entry, source: &str, enabled: bool) -> PluginInfo {
    PluginInfo {
        name: e.name.clone(),
        market: e.market.clone(),
        store: e.store.clone(),
        source: source.to_string(),
        enabled,
        path: e.path.to_string_lossy().into_owned(),
 skills: count_skills(&e.path),
        mcp: servers_of(&e.path).len(),
        hooks: e.path.join("hooks").join("hooks.json").is_file(),
    }
}

fn list_all(config: &Path) -> Vec<PluginInfo> {
    let mut out = Vec::new();
    for d in claude_roots() {
        if d.is_dir() {
     let name = read_json(&d.join(".claude-plugin").join("plugin.json"), None)
  .and_then(|m| m.get("name").and_then(Value::as_str).map(String::from))
         .unwrap_or_else(|| dir_name(&d));
     let e = Entry {
  name,
         market: "claude".to_string(),
         store: String::new(),
  path: d,
     };
     out.push(info_of(&e, "claude", true));
        }
    }
    let off = disabled(config);
    for e in app_plugins(config) {
 let on = !off.contains(&e.key());
        out.push(info_of(&e, "app", on));
    }
    out
}

#[tauri::command]
pub fn plugin_list(app: AppHandle) -> Result<Vec<PluginInfo>, String> {
    Ok(list_all(&config_dir(&app)?))
}

enum Source {
    Git { url: String, name: String },
    Local { path: PathBuf, name: String },
}

fn valid_part(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 100
        && !s.starts_with('.')
        && s.chars()
     .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn parse_source(raw: &str) -> Result<Source, String> {
    let s = raw.trim().trim_matches('"');
    if s.is_empty() {
        return Err("source manquante : `owner/repo`, URL https:// ou dossier local".to_string());
    }
    if let Some(rest) = s.strip_prefix("https://") {
        let clean = rest.trim_end_matches('/').trim_end_matches(".git");
 let name = clean.rsplit('/').next().unwrap_or_default().to_string();
        if !clean.contains('/') || !valid_part(&name) || s.chars().any(|c| c.is_whitespace()) {
     return Err("URL invalide (attendu : https://hôte/chemin/dépôt)".to_string());
        }
 return Ok(Source::Git {
     url: s.to_string(),
     name,
 });
    }
    let p = Path::new(s);
    if p.is_dir() {
        let name = dir_name(p);
        if !valid_part(&name) {
            return Err(format!("nom de dossier invalide : {name}"));
        }
 return Ok(Source::Local {
     path: p.to_path_buf(),
            name,
 });
    }
    let parts: Vec<&str> = s.split('/').collect();
    if parts.len() == 2 && parts.iter().all(|x| valid_part(x)) {
        let repo = parts[1].trim_end_matches(".git").to_string();
        return Ok(Source::Git {
            url: format!("https://github.com/{}/{}.git", parts[0], repo),
            name: repo,
        });
    }
    Err("source non reconnue : `owner/repo`, URL https:// ou dossier local existant".to_string())
}

fn looks_like_plugin(dir: &Path) -> bool {
    let cp = dir.join(".claude-plugin");
    cp.join("plugin.json").is_file()
 || cp.join("marketplace.json").is_file()
        || dir.join(".mcp.json").is_file()
 || dir.join("skills").is_dir()
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for e in fs::read_dir(from).map_err(|e| e.to_string())?.flatten() {
 let name = e.file_name();
 let Ok(ft) = e.file_type() else { continue };
 if ft.is_symlink() || name == ".git" {
     continue;
        }
 let dest = to.join(&name);
 if ft.is_dir() {
     copy_dir(&e.path(), &dest)?;
        } else {
     fs::copy(e.path(), &dest).map_err(|e| e.to_string())?;
 }
    }
    Ok(())
}

/// Lance `git <args>` (sans invite interactive, 3 min max) ; renvoie la sortie standard.
async fn git(args: Vec<OsString>, what: &str) -> Result<String, String> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(&args)
 .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null())
 .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let out = tokio::time::timeout(Duration::from_secs(180), cmd.output())
        .await
        .map_err(|_| format!("git {what} : délai dépassé (3 min)"))?
 .map_err(|e| format!("git introuvable ou non lançable : {e}"))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).trim().to_string());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("échec");
    Err(format!("git {what} : {}", last.trim()))
}

/// Ajoute une marketplace / un plugin dans `<config>/plugins/<nom>` : `owner/repo` (GitHub),
/// URL https:// (git clone --depth 1) ou dossier local (copié). Refuse si le nom existe
/// déjà ou si le contenu ne ressemble pas à un plugin. `install = false` : tout est ajouté
/// mais désactivé (à activer avec `plugin_set_enabled`) ; défaut : tout est activé.
#[tauri::command]
pub async fn plugin_add(
    app: AppHandle,
    source: String,
    install: Option<bool>,
) -> Result<Vec<PluginInfo>, String> {
    let src = parse_source(&source)?;
    let name = match &src {
        Source::Git { name, .. } | Source::Local { name, .. } => name.clone(),
    };
    let config = config_dir(&app)?;
    let dest = config.join("plugins").join(&name);
    if dest.exists() {
        return Err(format!("« {name} » est déjà installé (/plugin remove {name})"));
    }
    fs::create_dir_all(dest.parent().unwrap_or(Path::new("."))).map_err(|e| e.to_string())?;
    let res = match &src {
        Source::Git { url, .. } => {
     git(
         vec![
      "clone".into(),
      "--depth".into(),
      "1".into(),
      "--".into(),
      url.into(),
      dest.clone().into_os_string(),
         ],
         "clone",
     )
     .await
     .map(|_| ())
        }
        Source::Local { path, .. } => copy_dir(path, &dest),
    };
    if let Err(e) = res {
        let _ = fs::remove_dir_all(&dest);
 return Err(e);
    }
    if !looks_like_plugin(&dest) {
        let _ = fs::remove_dir_all(&dest);
        return Err(
     "ce dépôt ne ressemble pas à un plugin (ni .claude-plugin/, ni .mcp.json, ni skills/)"
         .to_string(),
        );
    }
    let entries = expand(&dest, &name);
    let install = install.unwrap_or(true);
    let mut off = disabled(&config);
    for e in &entries {
        if install {
     off.remove(&e.key());
        } else {
     off.insert(e.key());
        }
    }
    save_disabled(&config, &off)?;
    Ok(entries.iter().map(|e| info_of(e, "app", install)).collect())
}

/// Active ou désactive un plugin (`nom@marketplace`) installé par l'app.
#[tauri::command]
pub fn plugin_set_enabled(app: AppHandle, spec: String, enabled: bool) -> Result<PluginInfo, String> {
    let config = config_dir(&app)?;
    let e = resolve(&config, &spec)?;
    let mut off = disabled(&config);
    if enabled {
 off.remove(&e.key());
    } else {
        off.insert(e.key());
    }
    save_disabled(&config, &off)?;
    Ok(info_of(&e, "app", enabled))
}

/// Supprime une marketplace / un plugin installé par l'app (par nom de dossier ou de
/// marketplace ; jamais ceux de `~/.claude`).
#[tauri::command]
pub fn plugin_remove(app: AppHandle, name: String) -> Result<(), String> {
    let name = name.trim();
    if !valid_part(name) {
        return Err("nom invalide".to_string());
    }
    let config = config_dir(&app)?;
    let Some(dir) = market_dirs(&config, Some(name)).into_iter().next() else {
        return Err(format!("inconnu : {name} (/plugin marketplace list)"));
    };
    let store = dir_name(&dir);
    let keys: Vec<String> = expand(&dir, &store).iter().map(Entry::key).collect();
    fs::remove_dir_all(&dir).map_err(|e| format!("suppression de {} : {e}", dir.display()))?;
    let mut off = disabled(&config);
    for k in keys {
        off.remove(&k);
    }
    save_disabled(&config, &off)
}

/// `git pull --ff-only` sur les marketplaces clonées (toutes, ou celle nommée).
/// Les copies locales (sans `.git`) sont ignorées.
#[tauri::command]
pub async fn plugin_update(app: AppHandle, name: Option<String>) -> Result<String, String> {
    let config = config_dir(&app)?;
    let name = name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    if let Some(n) = &name {
 if !valid_part(n) {
     return Err("nom invalide".to_string());
 }
    }
    let dirs: Vec<PathBuf> = market_dirs(&config, name.as_deref())
        .into_iter()
        .filter(|d| d.join(".git").exists())
        .collect();
    if dirs.is_empty() {
 return Ok("rien à mettre à jour (les copies de dossiers locaux sont ignorées)".to_string());
    }
    let mut lines = Vec::new();
    for d in dirs {
        let label = dir_name(&d);
        let args: Vec<OsString> = vec![
     "-C".into(),
     d.clone().into_os_string(),
            "pull".into(),
     "--ff-only".into(),
        ];
        match git(args, "pull").await {
     Ok(out) if out.contains("Already up to date") => lines.push(format!("{label} : à jour")),
     Ok(_) => lines.push(format!("{label} : mis à jour")),
     Err(e) => lines.push(format!("{label} : {e}")),
        }
    }
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_plugin_mcp_and_expands_root() {
        let dir = std::env::temp_dir().join(format!("fs-plugin-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".claude-plugin")).unwrap();
 fs::write(
     dir.join(".mcp.json"),
     r#"{"mcpServers":{"mem":{"command":"node","args":["${CLAUDE_PLUGIN_ROOT}/s.cjs"]}}}"#,
        )
        .unwrap();
 fs::write(
     dir.join(".claude-plugin").join("plugin.json"),
            r#"{"name":"x","mcpServers":{"web":{"type":"http","url":"https://e.x/mcp"}}}"#,
 )
        .unwrap();
        let s = servers_of(&dir);
        let _ = fs::remove_dir_all(&dir);
        let mem = &s.iter().find(|(n, _)| n == "mem").expect("mem").1;
        assert!(mem.args[0].ends_with("s.cjs") && !mem.args[0].contains("${"));
        assert!(mem.args[0].starts_with(&*dir.to_string_lossy()));
        assert!(s.iter().any(|(n, _)| n == "web"));
    }

    #[test]
    fn parses_sources() {
        match parse_source("thedotmack/claude-mem").unwrap() {
     Source::Git { url, name } => {
  assert_eq!(url, "https://github.com/thedotmack/claude-mem.git");
  assert_eq!(name, "claude-mem");
            }
     _ => panic!("git attendu"),
        }
 match parse_source("https://github.com/a/b.git").unwrap() {
            Source::Git { name, .. } => assert_eq!(name, "b"),
     _ => panic!("git attendu"),
        }
        assert!(parse_source("").is_err());
 assert!(parse_source("--upload-pack=x/y").is_err());
        assert!(parse_source("ext::sh -c evil").is_err());
 assert!(parse_source("https://x").is_err());
 assert!(parse_source("a/../b").is_err());
    }

    #[test]
    fn skill_dirs_follow_manifest_and_reject_escapes() {
 let dir = std::env::temp_dir().join(format!("fs-skilldirs-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".claude-plugin")).unwrap();
        fs::write(
            dir.join(".claude-plugin").join("plugin.json"),
     r#"{"skills":["./.claude/skills/","../evil","C:\\x"]}"#,
        )
        .unwrap();
        let d = skill_dirs(&dir);
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(d.len(), 2);
        assert!(d[1].ends_with("skills") && d[1].to_string_lossy().contains(".claude"));
    }

    #[test]
    fn marketplace_expands_to_local_plugins() {
 let dir = std::env::temp_dir().join(format!("fs-market-test-{}", std::process::id()));
 let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".claude-plugin")).unwrap();
 fs::create_dir_all(dir.join("plugin")).unwrap();
        fs::write(
     dir.join(".claude-plugin").join("marketplace.json"),
     r#"{"plugins":[{"name":"p","source":"./plugin"},{"name":"bad","source":"./../x"}]}"#,
        )
 .unwrap();
        let r = expand(&dir, "store");
        let _ = fs::remove_dir_all(&dir);
 assert_eq!(r.len(), 1);
        assert!(r[0].path.ends_with("plugin"));
 assert_eq!(r[0].key(), "p@store");
    }

    #[test]
    fn resolves_and_disables_plugins() {
        let cfg = std::env::temp_dir().join(format!("fs-resolve-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&cfg);
 let mk = cfg.join("plugins").join("m");
        fs::create_dir_all(mk.join(".claude-plugin")).unwrap();
 fs::create_dir_all(mk.join("p")).unwrap();
        fs::write(
     mk.join(".claude-plugin").join("marketplace.json"),
     r#"{"name":"mk","plugins":[{"name":"pl","source":"./p"}]}"#,
        )
 .unwrap();
        let path = mk.join("p");
 let ok = |s: &str| resolve(&cfg, s).map(|e| e.key());
 assert_eq!(ok("pl@mk").unwrap(), "pl@mk");
 assert_eq!(ok("pl@m").unwrap(), "pl@mk");
        assert_eq!(ok("pl").unwrap(), "pl@mk");
        assert!(ok("x@mk").is_err() && ok("pl@zz").is_err() && ok("").is_err());
 assert!(roots(&cfg).contains(&path));
        let mut off = HashSet::new();
        off.insert("pl@mk".to_string());
        save_disabled(&cfg, &off).unwrap();
 let (is_off, still_listed) = (disabled(&cfg).contains("pl@mk"), roots(&cfg).contains(&path));
 let listed = list_all(&cfg).iter().any(|i| i.name == "pl" && !i.enabled);
 let _ = fs::remove_dir_all(&cfg);
        assert!(is_off && !still_listed && listed);
    }
}
