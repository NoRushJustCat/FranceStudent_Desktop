//! Édition depuis l'interface : skills (locaux et du compte) et serveurs MCP.
//!
//! - Skills locaux : lecture / écriture / création / suppression de `SKILL.md`.
//!   Seuls les skills trouvés par `skills::list` (donc directement dans un des
//!   dossiers de skills connus) peuvent être modifiés ou supprimés.
//! - Skills du compte : endpoints `PATCH` / `DELETE /skills/{id}` SUPPOSÉS.
//! - MCP : `mcp.json` est modifié en JSON brut, donc les champs inconnus sont conservés.
//!   Un serveur est soit local (`command` + `args`), soit HTTP (`url`, `token` optionnel).
//! - MCP : `mcp_call` appelle un outil à la demande de l'utilisateur (panneau « Appeler l'outil »).

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{json, Map, Value};
use tauri::{AppHandle, State};

use crate::mcp::{self, ServerConfig};
use crate::{client, config_dir, skills, AppState};

const CONFIG_FILE: &str = "mcp.json";

// ---------- Skills locaux ----------

fn root_of(workdir: Option<String>) -> Option<PathBuf> {
    workdir.filter(|w| !w.trim().is_empty()).map(PathBuf::from)
}

fn find_skill(
    app: &AppHandle,
    workdir: Option<String>,
    name: &str,
) -> Result<skills::SkillInfo, String> {
    let cfg = config_dir(app)?;
    let root = root_of(workdir);
    skills::list(&cfg, root.as_deref())
        .into_iter()
        .find(|s| s.name == name)
        .ok_or_else(|| format!("skill inconnu : {name}"))
}

fn valid_skill_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 64
        && n.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Texte brut du `SKILL.md`.
#[tauri::command]
pub fn skill_read(app: AppHandle, name: String, workdir: Option<String>) -> Result<String, String> {
    let info = find_skill(&app, workdir, &name)?;
    fs::read_to_string(Path::new(&info.path).join("SKILL.md")).map_err(|e| e.to_string())
}

/// Remplace le `SKILL.md` d'un skill existant.
#[tauri::command]
pub fn skill_write(
    app: AppHandle,
    name: String,
    workdir: Option<String>,
    text: String,
) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("contenu vide".to_string());
    }
    let info = find_skill(&app, workdir, &name)?;
    if info.source == "plugin" {
 return Err("skill fourni par un plugin : lecture seule".to_string());
    }
    fs::write(Path::new(&info.path).join("SKILL.md"), text).map_err(|e| e.to_string())
}

/// Crée un skill dans `<config de l'app>/skills/<nom>/SKILL.md`.
#[tauri::command]
pub fn skill_create(
    app: AppHandle,
    name: String,
    description: String,
    body: String,
    workdir: Option<String>,
) -> Result<(), String> {
    let name = name.trim().to_string();
    if !valid_skill_name(&name) {
        return Err("nom invalide : lettres, chiffres, « - » et « _ » (64 max)".to_string());
    }
    if body.trim().is_empty() {
        return Err("les instructions sont vides".to_string());
    }
    let cfg = config_dir(&app)?;
    let root = root_of(workdir);
    if skills::list(&cfg, root.as_deref())
        .iter()
        .any(|s| s.name.eq_ignore_ascii_case(&name))
    {
        return Err(format!("un skill « {name} » existe déjà"));
    }
    let dir = cfg.join("skills").join(&name);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let desc = description.split_whitespace().collect::<Vec<_>>().join(" ");
    let desc_block = if desc.is_empty() {
        String::new()
    } else {
        format!("description: >\n  {desc}\n")
    };
    let text = format!("---\nname: {name}\n{desc_block}---\n\n{}\n", body.trim());
    fs::write(dir.join("SKILL.md"), text).map_err(|e| e.to_string())
}

/// Supprime le dossier du skill (définitif).
#[tauri::command]
pub fn skill_delete(app: AppHandle, name: String, workdir: Option<String>) -> Result<(), String> {
    let info = find_skill(&app, workdir, &name)?;
    if info.source == "plugin" {
        return Err("skill fourni par un plugin : désinstalle le plugin pour le retirer".to_string());
    }
    let dir = PathBuf::from(&info.path);
    if !dir.join("SKILL.md").is_file() {
        return Err("SKILL.md introuvable : rien supprimé".to_string());
    }
    fs::remove_dir_all(&dir).map_err(|e| format!("suppression de {} : {e}", dir.display()))
}

// ---------- Skills du compte (endpoints supposés) ----------

fn safe_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.contains(['/', '\\', '?', '#']) {
        Err("identifiant de skill invalide".to_string())
    } else {
        Ok(())
    }
}

/// `DELETE /skills/{id}` (supposé).
#[tauri::command]
pub async fn account_skill_delete(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    safe_id(&id)?;
    let c = client(&app, &state).await?;
    c.send_json("DELETE", &format!("/skills/{id}"), None)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// `POST /skills/{id}` (SkillUpdateDto) avec le JSON fourni.
#[tauri::command]
pub async fn account_skill_update(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    body: Value,
) -> Result<Value, String> {
    safe_id(&id)?;
    let c = client(&app, &state).await?;
    c.send_json("POST", &format!("/skills/{id}"), Some(body))
        .await
        .map_err(|e| e.to_string())
}

// ---------- MCP : édition de mcp.json ----------

fn read_root(dir: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(dir.join(CONFIG_FILE)).unwrap_or_default();
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(&text).map_err(|e| format!("mcp.json invalide : {e}"))
}

fn write_root(dir: &Path, root: &Value) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut text = serde_json::to_string_pretty(root).map_err(|e| e.to_string())?;
    text.push('\n');
    fs::write(dir.join(CONFIG_FILE), text).map_err(|e| e.to_string())
}

fn servers_mut(root: &mut Value) -> Result<&mut Map<String, Value>, String> {
    let obj = root
        .as_object_mut()
        .ok_or_else(|| "mcp.json : un objet JSON est attendu".to_string())?;
    obj.entry("mcpServers")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| "mcp.json : « mcpServers » doit être un objet".to_string())
}

/// Écrit `v` sous `key`, ou retire la clé si `v` est `None`.
fn set_or_remove(obj: &mut Map<String, Value>, key: &str, v: Option<Value>) {
    match v {
        Some(v) => {
            obj.insert(key.to_string(), v);
        }
        None => {
            obj.remove(key);
        }
    }
}

/// Déconnecte un serveur et retire ses autorisations de session.
async fn drop_server(state: &AppState, name: &str) {
    state.mcp.disconnect(name).await;
    if let Ok(mut up) = state.mcp_up.lock() {
        up.remove(name);
    }
    let prefix = mcp::exposed_name(name, "");
    if let Ok(mut t) = state.trusted.lock() {
        t.retain(|n| !n.starts_with(&prefix));
    }
}

/// Configuration d'un serveur (pour le formulaire de modification).
#[tauri::command]
pub fn mcp_get_server(app: AppHandle, name: String) -> Result<ServerConfig, String> {
    mcp::load_config(&config_dir(&app)?)?
        .get(&name)
        .cloned()
        .ok_or_else(|| format!("serveur inconnu : {name}"))
}

/// Ajoute (`old_name` absent) ou modifie (`old_name` = nom actuel) un serveur.
/// Le serveur concerné est déconnecté : il faut le reconnecter pour appliquer les changements.
/// Il faut une `command` (serveur local) ou une `url` (serveur HTTP).
#[tauri::command]
pub async fn mcp_upsert(
    app: AppHandle,
    state: State<'_, AppState>,
    old_name: Option<String>,
    name: String,
    config: ServerConfig,
) -> Result<(), String> {
    let name = name.trim().to_string();
    if name.is_empty() || name.chars().any(|c| c.is_control()) {
        return Err("nom de serveur invalide".to_string());
    }
    let command = config.command.trim().to_string();
    let url = config
        .url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);
    let kind = config
        .kind
        .as_deref()
        .map(|k| k.trim().to_ascii_lowercase())
        .filter(|k| !k.is_empty() && k != "stdio");
    if let Some(k) = &kind {
        if !(k == "sse" || k.contains("http")) {
            return Err(format!("type de serveur inconnu : {k}"));
        }
        if url.is_none() {
            return Err("URL manquante pour un serveur distant".to_string());
        }
    } else if command.is_empty() && url.is_none() {
        return Err("commande ou URL manquante".to_string());
    }
    if let Some(u) = &url {
        if !(u.starts_with("http://") || u.starts_with("https://")) {
            return Err("l'URL doit commencer par http:// ou https://".to_string());
        }
    }
    let token = config
        .token
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);
    let args: Vec<String> = config
        .args
        .iter()
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty())
        .collect();

    let dir = config_dir(&app)?;
    let editing = old_name.is_some();
    let renamed = old_name.as_deref().is_some_and(|o| o != name);
    {
        let mut root = read_root(&dir)?;
        let servers = servers_mut(&mut root)?;
        if servers.contains_key(&name) && (!editing || renamed) {
            return Err(format!("un serveur « {name} » existe déjà"));
        }
        // On repart de l'entrée existante : les champs inconnus sont conservés.
        let base = old_name
            .as_deref()
            .and_then(|o| servers.get(o))
            .cloned()
            .unwrap_or_else(|| json!({}));
        if renamed {
            servers.remove(old_name.as_deref().unwrap_or_default());
        }
        let mut obj = match base {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        set_or_remove(
            &mut obj,
            "command",
            (!command.is_empty()).then(|| json!(command)),
        );
        set_or_remove(&mut obj, "args", (!args.is_empty()).then(|| json!(args)));
        set_or_remove(
            &mut obj,
            "env",
            (!config.env.is_empty()).then(|| json!(config.env)),
        );
        set_or_remove(
            &mut obj,
            "cwd",
            config
                .cwd
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|c| json!(c)),
        );
        set_or_remove(&mut obj, "url", url.map(|u| json!(u)));
        set_or_remove(&mut obj, "token", token.map(|t| json!(t)));
        set_or_remove(
            &mut obj,
            "headers",
            (!config.headers.is_empty()).then(|| json!(config.headers)),
        );
        // Alias d'autres clients : remplacés par les clés canoniques ci-dessus.
        for alias in ["serverUrl", "httpUrl", "transport"] {
            obj.remove(alias);
        }
        set_or_remove(&mut obj, "type", kind.map(|k| json!(k)));
        set_or_remove(&mut obj, "disabled", config.disabled.then(|| json!(true)));
        servers.insert(name.clone(), Value::Object(obj));
        write_root(&dir, &root)?;
    }
    if let Some(o) = &old_name {
        drop_server(&state, o).await;
    }
    drop_server(&state, &name).await;
    Ok(())
}

/// Retire un serveur de `mcp.json` (après l'avoir déconnecté).
#[tauri::command]
pub async fn mcp_remove(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> Result<(), String> {
    drop_server(&state, &name).await;
    let dir = config_dir(&app)?;
    let mut root = read_root(&dir)?;
    let removed = servers_mut(&mut root)?.remove(&name).is_some();
    if !removed {
        return Err(format!("serveur inconnu : {name}"));
    }
    write_root(&dir, &root)
}

/// Importe du JSON au format Claude Desktop (`{"mcpServers": {...}}`) ou une simple
/// table `{"nom": {"command": ...}}` / `{"nom": {"url": ...}}`.
/// Refuse tout (rien écrit) si un nom existe déjà.
#[tauri::command]
pub fn mcp_import(app: AppHandle, text: String) -> Result<Vec<String>, String> {
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("JSON invalide : {e}"))?;
    let obj = v
        .as_object()
        .ok_or_else(|| "un objet JSON est attendu".to_string())?;
    if obj.contains_key("command") || obj.contains_key("url") {
        return Err(
            "il manque le nom du serveur : {\"nom\": {\"command\": …}} ou {\"nom\": {\"url\": …}}"
                .to_string(),
        );
    }
    let found: Map<String, Value> = match obj
        .get("mcpServers")
        .or_else(|| obj.get("servers"))
        .and_then(Value::as_object)
    {
        Some(m) => m.clone(),
        None => obj.clone(),
    };
    if found.is_empty() {
        return Err("aucun serveur trouvé".to_string());
    }
    for (n, c) in &found {
        let cfg: ServerConfig =
            serde_json::from_value(c.clone()).map_err(|e| format!("« {n} » : {e}"))?;
        if !cfg.is_valid() {
            return Err(format!("« {n} » : `command` ou `url` manquant"));
        }
    }
    let dir = config_dir(&app)?;
    let mut root = read_root(&dir)?;
    let servers = servers_mut(&mut root)?;
    let dup: Vec<String> = found
        .keys()
        .filter(|k| servers.contains_key(k.as_str()))
        .cloned()
        .collect();
    if !dup.is_empty() {
        return Err(format!("déjà présents : {}", dup.join(", ")));
    }
    let names: Vec<String> = found.keys().cloned().collect();
    for (k, v) in found {
        servers.insert(k, v);
    }
    write_root(&dir, &root)?;
    Ok(names)
}

// ---------- MCP : appel manuel d'un outil ----------

/// Résultat d'un appel d'outil MCP, tel qu'affiché dans le panneau.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpCallResult {
    text: String,
    is_error: bool,
}

/// Appelle un outil d'un serveur MCP **connecté**, sur demande explicite de l'utilisateur.
/// `args` : objet JSON des arguments de l'outil.
#[tauri::command]
pub async fn mcp_call(
    state: State<'_, AppState>,
    server: String,
    tool: String,
    args: Value,
) -> Result<McpCallResult, String> {
    let (text, is_error) = state.mcp.call_tool(&server, &tool, args).await?;
    Ok(McpCallResult { text, is_error })
}
