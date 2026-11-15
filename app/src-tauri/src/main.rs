// Pas de console supplémentaire sous Windows en release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bridge;
mod ext;
mod mcp;
mod skills;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use fs_api::{Agent, ChatEvent, Client, StreamOptions, ToolCall, ToolResult};
use fs_tools::Risk;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::oneshot;

/// Nombre maximal d'allers-retours outils pour un seul message utilisateur.
const MAX_STEPS: usize = 30;
/// Idem en mode économie.
const MAX_STEPS_ECO: usize = 10;
/// Taille maximale (caractères) d'une sortie d'outil envoyée au modèle en mode économie.
const ECO_OUTPUT_CAP: usize = 8_000;
/// En dessous de ce nombre de tokens de prompt, le mode économie ne regarde même pas la compaction.
const ECO_COMPACT_MIN: u64 = 30_000;
/// Seuil de compaction automatique si la taille de contexte de l'agent est inconnue.
const ECO_COMPACT_DEFAULT: u64 = 60_000;

/// Compteur pour les identifiants de génération quand le front n'en fournit pas.
static RUN_SEQ: AtomicU64 = AtomicU64::new(1);

/// Confirmation d'outil en attente : (identifiant de la génération, canal de réponse).
type Pending = (String, oneshot::Sender<bool>);

/// Client HTTP mis en cache (vidé quand l'URL ou la clé changent),
/// confirmations d'outils en attente, générations en cours (une par chat, en parallèle),
/// MCP et autorisations de session.
#[derive(Default)]
struct AppState {
    client: Mutex<Option<Client>>,
    /// Confirmations en attente, par `call_id`.
    pending: Mutex<HashMap<String, Pending>>,
    /// Générations en cours : drapeau d'annulation par identifiant de génération.
    runs: Mutex<HashMap<String, Arc<AtomicBool>>>,
    mcp: mcp::Hub,
    /// Serveurs MCP actuellement connectés.
    mcp_up: Mutex<HashSet<String>>,
    /// Outils MCP « toujours autoriser » pour cette session (jamais enregistré sur disque).
    trusted: Mutex<HashSet<String>>,
    /// Pont MCP (serveur local + tunnel + enregistrement France Student).
    bridge: bridge::Bridge,
}

fn config_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_config_dir().map_err(|e| e.to_string())
}

static ECO_TOKEN_CACHE: OnceLock<Mutex<Option<(String, u64)>>> = OnceLock::new();
static ACTIVE_CLIENT_KEY: OnceLock<Mutex<Option<String>>> = OnceLock::new();

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn b64url_decode(input: &str) -> Option<Vec<u8>> {
    let mut acc: u32 = 0;
    let mut bits: u8 = 0;
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    for b in input.bytes() {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            b'=' => break,
            _ => return None,
        };
        acc = (acc << 6) | (v as u32);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

fn jwt_exp(tok: &str) -> Option<u64> {
    let payload = tok.split('.').nth(1)?;
    let bytes = b64url_decode(payload)?;
    let v: Value = serde_json::from_slice(&bytes).ok()?;
    v.get("exp").and_then(|x| x.as_u64())
}

fn eco_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join(".gemini/config/plugins/eco-quota"))
}

fn has_eco_credentials() -> bool {
    eco_dir()
        .map(|d| d.join("fs_credentials.json").exists())
        .unwrap_or(false)
}

async fn refresh_eco_token() -> Option<(String, u64)> {
    let dir = eco_dir()?;
    let raw = tokio::fs::read_to_string(dir.join("fs_credentials.json")).await.ok()?;
    let creds: Value = serde_json::from_str(&raw).ok()?;
    let session_token = creds.get("sessionToken")?.as_str()?.trim().to_string();
    let device_id = creds.get("deviceId")?.as_str()?.trim().to_string();
    if session_token.is_empty() || device_id.is_empty() {
        return None;
    }

    let cookie = format!("next-auth.session-token={session_token}; francestudent-device-id={device_id}");
    let http = reqwest::Client::new();
    let resp = http
        .get("https://ia.francestudent.org/api/auth/api-token")
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:157.0) Gecko/20100101 Firefox/157.0")
        .header("Accept", "*/*")
        .header("Origin", "https://ia.francestudent.org")
        .header("Referer", "https://ia.francestudent.org/")
        .header("francestudent-client-platform", "web_fs_ai")
        .header("francestudent-client-version", "4005601")
        .header("francestudent-device-id", &device_id)
        .header("Cookie", &cookie)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let data: Value = resp.json().await.ok()?;
    let tok = data.get("accessToken")?.as_str()?.trim().to_string();
    if tok.is_empty() {
        return None;
    }
    let expires_in = data.get("expiresIn").and_then(|v| v.as_u64()).unwrap_or(300);
    let _ = tokio::fs::write(dir.join("fs_token.txt"), &tok).await;
    Some((tok, now_secs() + expires_in))
}

async fn resolve_api_key() -> Option<String> {
    let stored = fs_store::get_key();
    if let Some(ref k) = stored {
        if !k.starts_with("eyJ") {
            return Some(k.clone());
        }
        if jwt_exp(k).is_some_and(|exp| now_secs() + 30 < exp) {
            return Some(k.clone());
        }
    }

    let cache = ECO_TOKEN_CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(guard) = cache.lock() {
        if let Some((ref tok, exp)) = *guard {
            if now_secs() + 30 < exp {
                return Some(tok.clone());
            }
        }
    }

    if let Some((tok, exp)) = refresh_eco_token().await {
        if let Ok(mut guard) = cache.lock() {
            *guard = Some((tok.clone(), exp));
        }
        return Some(tok);
    }

    stored
}

async fn client(app: &AppHandle, state: &State<'_, AppState>) -> Result<Client, String> {
    let key = resolve_api_key()
        .await
        .ok_or("Clé API manquante : ouvre les réglages.")?;
    let active = ACTIVE_CLIENT_KEY.get_or_init(|| Mutex::new(None));
    let same_key = active
        .lock()
        .map(|g| g.as_deref() == Some(key.as_str()))
        .unwrap_or(false);

    let mut slot = state
        .client
        .lock()
        .map_err(|_| "état interne verrouillé".to_string())?;
    if same_key {
        if let Some(c) = slot.as_ref() {
            return Ok(c.clone());
        }
    }
    let settings = fs_store::load(&config_dir(app)?);
    let c = Client::new(&settings.base_url, &key).map_err(|e| e.to_string())?;
    *slot = Some(c.clone());
    if let Ok(mut g) = active.lock() {
        *g = Some(key);
    }
    Ok(c)
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max).collect();
    t.push('…');
    t
}

// ---------- Réglages ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsView {
    base_url: String,
    has_key: bool,
    last_agent_id: Option<i64>,
    last_workdir: Option<String>,
    auto_accept: bool,
    eco: bool,
}

#[tauri::command]
fn get_settings(app: AppHandle) -> Result<SettingsView, String> {
    let s = fs_store::load(&config_dir(&app)?);
    Ok(SettingsView {
        base_url: s.base_url,
        has_key: fs_store::get_key().is_some() || has_eco_credentials(),
        last_agent_id: s.last_agent_id,
        last_workdir: s.last_workdir,
        auto_accept: s.auto_accept,
        eco: s.eco,
    })
}

/// Chaque champ est optionnel : seuls ceux fournis sont modifiés.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    base_url: Option<String>,
    api_key: Option<String>,
    last_agent_id: Option<i64>,
    last_workdir: Option<String>,
    auto_accept: Option<bool>,
    eco: Option<bool>,
) -> Result<(), String> {
    let dir = config_dir(&app)?;
    let mut s = fs_store::load(&dir);
    let mut reset_client = false;

    if let Some(u) = base_url
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty())
    {
        if u != s.base_url {
            s.base_url = u;
            reset_client = true;
        }
    }
    if let Some(id) = last_agent_id {
        s.last_agent_id = Some(id);
    }
    if let Some(w) = last_workdir.filter(|w| !w.trim().is_empty()) {
        s.last_workdir = Some(w);
    }
    if let Some(a) = auto_accept {
        s.auto_accept = a;
    }
    if let Some(e) = eco {
        s.eco = e;
    }
    fs_store::save(&dir, &s).map_err(|e| e.to_string())?;

    if let Some(k) = api_key.filter(|k| !k.trim().is_empty()) {
        fs_store::set_key(&k)?;
        reset_client = true;
    }
    if reset_client {
        *state
            .client
            .lock()
            .map_err(|_| "état interne verrouillé".to_string())? = None;
    }
    Ok(())
}

/// Sélecteur de dossier natif. `None` si l'utilisateur annule.
#[tauri::command]
async fn pick_folder(app: AppHandle) -> Result<Option<String>, String> {
    let picked = tokio::task::spawn_blocking(move || app.dialog().file().blocking_pick_folder())
        .await
        .map_err(|e| e.to_string())?;
    match picked {
        Some(p) => {
            let path = p.into_path().map_err(|e| e.to_string())?;
            Ok(Some(path.to_string_lossy().into_owned()))
        }
        None => Ok(None),
    }
}

// ---------- Lectures et actions simples ----------

#[tauri::command]
async fn list_agents(app: AppHandle, state: State<'_, AppState>) -> Result<Vec<Agent>, String> {
    let c = client(&app, &state).await?;
    c.agents().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_usage(app: AppHandle, state: State<'_, AppState>) -> Result<Value, String> {
    let c = client(&app, &state).await?;
    c.usage().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_chats(
    app: AppHandle,
    state: State<'_, AppState>,
    limit: Option<u32>,
    cursor: Option<String>,
) -> Result<Value, String> {
    let c = client(&app, &state).await?;
    c.list_chats(limit.unwrap_or(40), cursor.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_chat(
    app: AppHandle,
    state: State<'_, AppState>,
    chat_id: String,
) -> Result<Value, String> {
    let c = client(&app, &state).await?;
    c.get_chat(&chat_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_chat(
    app: AppHandle,
    state: State<'_, AppState>,
    chat_id: String,
) -> Result<(), String> {
    let c = client(&app, &state).await?;
    c.delete_chat(&chat_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn rename_chat(
    app: AppHandle,
    state: State<'_, AppState>,
    chat_id: String,
    title: String,
) -> Result<(), String> {
    let c = client(&app, &state).await?;
    c.rename_chat(&chat_id, title.trim())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn pin_chat(
    app: AppHandle,
    state: State<'_, AppState>,
    chat_id: String,
    pinned: bool,
) -> Result<(), String> {
    let c = client(&app, &state).await?;
    c.pin_chat(&chat_id, pinned).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn archive_chat(
    app: AppHandle,
    state: State<'_, AppState>,
    chat_id: String,
) -> Result<(), String> {
    let c = client(&app, &state).await?;
    c.archive_chat(&chat_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn search_chats(
    app: AppHandle,
    state: State<'_, AppState>,
    query: String,
    limit: Option<u32>,
) -> Result<Value, String> {
    let c = client(&app, &state).await?;
    c.search_chats(query.trim(), limit.unwrap_or(20))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn fork_edit_message(
    app: AppHandle,
    state: State<'_, AppState>,
    chat_id: String,
    message_id: String,
    content: String,
) -> Result<Value, String> {
    let c = client(&app, &state).await?;
    c.fork_edit_message(&chat_id, &message_id, &content)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_saved_messages(
    app: AppHandle,
    state: State<'_, AppState>,
    limit: Option<u32>,
    cursor: Option<String>,
) -> Result<Value, String> {
    let c = client(&app, &state).await?;
    c.list_saved_messages(limit.unwrap_or(50), cursor.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn save_message(
    app: AppHandle,
    state: State<'_, AppState>,
    chat_id: String,
    message_id: String,
) -> Result<Value, String> {
    let c = client(&app, &state).await?;
    c.save_message(&chat_id, &message_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn unsave_message(
    app: AppHandle,
    state: State<'_, AppState>,
    chat_id: String,
    message_id: String,
) -> Result<(), String> {
    let c = client(&app, &state).await?;
    c.unsave_message(&chat_id, &message_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_archived_chats(
    app: AppHandle,
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> Result<Value, String> {
    let c = client(&app, &state).await?;
    c.list_archived_chats(limit.unwrap_or(40))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn restore_chat(
    app: AppHandle,
    state: State<'_, AppState>,
    chat_id: String,
) -> Result<(), String> {
    let c = client(&app, &state).await?;
    c.restore_chat(&chat_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_notification_prefs(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let c = client(&app, &state).await?;
    c.get_notification_preferences()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn update_notification_prefs(
    app: AppHandle,
    state: State<'_, AppState>,
    prefs: Value,
) -> Result<Value, String> {
    let c = client(&app, &state).await?;
    c.update_notification_preferences(prefs)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn upload_attachment(
    app: AppHandle,
    state: State<'_, AppState>,
    kind: String,
    filename: String,
    content_type: String,
    bytes: Vec<u8>,
) -> Result<Value, String> {
    let c = client(&app, &state).await?;
    if kind == "image" {
        c.upload_image(&filename, &content_type, bytes)
            .await
            .map_err(|e| e.to_string())
    } else {
        c.upload_file_v2(&filename, &content_type, bytes)
            .await
            .map_err(|e| e.to_string())
    }
}

#[tauri::command]
fn open_external(url: String) -> Result<(), String> {
    let trimmed = url.trim();
    if !trimmed.starts_with("https://") && !trimmed.starts_with("http://") {
        return Err("Seules les URL http(s) peuvent être ouvertes.".to_string());
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(trimmed)
            .spawn()
            .map_err(|e| format!("Impossible d'ouvrir le lien : {e}"))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(trimmed)
            .spawn()
            .map_err(|e| format!("Impossible d'ouvrir le lien : {e}"))?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(trimmed)
            .spawn()
            .map_err(|e| format!("Impossible d'ouvrir le lien : {e}"))?;
    }
    Ok(())
}

#[tauri::command]
async fn get_context(
    app: AppHandle,
    state: State<'_, AppState>,
    chat_id: String,
) -> Result<Value, String> {
    let c = client(&app, &state).await?;
    c.context(&chat_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn compact_chat(
    app: AppHandle,
    state: State<'_, AppState>,
    chat_id: String,
) -> Result<Value, String> {
    let c = client(&app, &state).await?;
    c.compact(&chat_id).await.map_err(|e| e.to_string())
}

/// Annule UNE génération (celle de `run_id`) : drapeau local, confirmations d'outils en
/// attente de cette génération, puis annulation côté serveur si le chat existe déjà.
/// Les autres générations en cours ne sont pas touchées.
#[tauri::command]
async fn cancel_chat(
    app: AppHandle,
    state: State<'_, AppState>,
    chat_id: Option<String>,
    run_id: Option<String>,
) -> Result<(), String> {
    if let Some(run) = run_id.as_deref() {
        if let Ok(runs) = state.runs.lock() {
            if let Some(flag) = runs.get(run) {
                flag.store(true, Ordering::SeqCst);
            }
        }
        if let Ok(mut map) = state.pending.lock() {
            // les receveurs voient un refus
            map.retain(|_, (r, _)| r != run);
        }
    }
    if let Some(id) = chat_id {
        let c = client(&app, &state).await?;
        let _ = c.cancel(&id).await; // idempotent ; rien à annuler n'est pas une erreur
    }
    Ok(())
}

/// Réponse de l'utilisateur à une demande de confirmation d'outil.
#[tauri::command]
fn resolve_tool(state: State<'_, AppState>, call_id: String, allow: bool) {
    if let Ok(mut map) = state.pending.lock() {
        if let Some((_, tx)) = map.remove(&call_id) {
            let _ = tx.send(allow);
        }
    }
}

// ---------- Skills ----------

/// Skills locaux (projet, utilisateur, app). `workdir` = dossier du mode Code, s'il y en a un.
#[tauri::command]
fn list_skills(app: AppHandle, workdir: Option<String>) -> Result<Vec<skills::SkillInfo>, String> {
    let dir = config_dir(&app)?;
    let root = workdir.filter(|w| !w.trim().is_empty()).map(PathBuf::from);
    Ok(skills::list(&dir, root.as_deref()))
}

/// Skills du compte France Student. Endpoint supposé (`GET /skills`) : à confirmer.
#[tauri::command]
async fn account_skills(app: AppHandle, state: State<'_, AppState>) -> Result<Value, String> {
    let c = client(&app, &state).await?;
    c.get_json("/skills").await.map_err(|e| e.to_string())
}

// ---------- MCP local ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct McpToolView {
    name: String,
    exposed_name: String,
    description: String,
    read_only: bool,
    trusted: bool,
    schema: Value,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct McpServerView {
    name: String,
    command: String,
    args: Vec<String>,
    url: Option<String>,
    connected: bool,
    tools: Vec<McpToolView>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct McpStatus {
    servers: Vec<McpServerView>,
    /// Erreur de lecture de mcp.json, s'il y en a une.
    error: Option<String>,
    /// Désormais false : le modèle peut utiliser les outils MCP via fs-mcp ou blocs mcp-call.
    custom_rejected: bool,
}

#[tauri::command]
async fn mcp_status(app: AppHandle, state: State<'_, AppState>) -> Result<McpStatus, String> {
    let dir = config_dir(&app)?;
    let cfg = mcp::Hub::load_all_servers(&dir, None);
    let up: HashSet<String> = state.mcp_up.lock().map(|g| g.clone()).unwrap_or_default();
    let trusted: HashSet<String> = state.trusted.lock().map(|g| g.clone()).unwrap_or_default();
    let exposed = state.mcp.exposed().await;
    let servers = cfg
        .into_iter()
        .map(|(name, c)| {
            let tools = exposed
                .iter()
                .filter(|e| e.server == name)
                .map(|e| McpToolView {
                    name: e.tool.name.clone(),
                    exposed_name: e.name.clone(),
                    description: clip(&e.tool.description, 200),
                    read_only: e.tool.read_only,
                    trusted: trusted.contains(&e.name),
                    schema: e.tool.input_schema.clone(),
                })
                .collect();
            McpServerView {
                url: c.endpoint().map(|(u, _)| u),
                connected: up.contains(&name),
                name,
                command: c.command,
                args: c.args,
                tools,
            }
        })
        .collect();
    Ok(McpStatus {
        servers,
        error: None,
        custom_rejected: false,
    })
}

/// Texte brut de mcp.json (exemple si le fichier n'existe pas encore).
#[tauri::command]
fn mcp_get_config(app: AppHandle) -> Result<String, String> {
    Ok(mcp::config_text(&config_dir(&app)?))
}

#[tauri::command]
fn mcp_save_config(app: AppHandle, text: String) -> Result<(), String> {
    mcp::save_config_text(&config_dir(&app)?, &text)
}

/// Lance le serveur (demande explicite) et renvoie le nombre d'outils trouvés.
#[tauri::command]
async fn mcp_connect(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> Result<usize, String> {
    let cfg = mcp::load_config(&config_dir(&app)?)?;
    let server = cfg
        .get(&name)
        .ok_or_else(|| format!("serveur inconnu : {name}"))?;
    let tools = state.mcp.connect(&name, server).await?;
    if let Ok(mut up) = state.mcp_up.lock() {
        up.insert(name);
    }
    Ok(tools.len())
}

#[tauri::command]
async fn mcp_disconnect(state: State<'_, AppState>, name: String) -> Result<(), String> {
    state.mcp.disconnect(&name).await;
    if let Ok(mut up) = state.mcp_up.lock() {
        up.remove(&name);
    }
    let prefix = mcp::exposed_name(&name, "");
    if let Ok(mut t) = state.trusted.lock() {
        t.retain(|n| !n.starts_with(&prefix));
    }
    Ok(())
}

/// « Toujours autoriser » (ou retirer) un outil MCP, pour cette session seulement.
#[tauri::command]
fn trust_tool(state: State<'_, AppState>, name: String, on: bool) {
    if let Ok(mut t) = state.trusted.lock() {
        if on && name.starts_with("mcp__") {
            t.insert(name);
        } else {
            t.remove(&name);
        }
    }
}

// ---------- Envoi d'un message (streaming + boucle d'outils) ----------

/// Événements envoyés au front.
#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum UiEvent {
    Opened {
        chat_id: String,
    },
    Title {
        chat_id: String,
        title: String,
    },
    Delta {
        text: String,
    },
    Reasoning {
        text: String,
    },
    /// Le modèle demande un outil. Si `needs_confirm`, le front doit
    /// répondre via `resolve_tool`. `origin` : toujours `local` (fichiers/commande).
    ToolCall {
        call_id: String,
        name: String,
        origin: String,
        args: Value,
        needs_confirm: bool,
    },
    ToolResult {
        call_id: String,
        output: String,
        is_error: bool,
        denied: bool,
    },
    /// Information (skills joints, compaction auto…).
    Notice {
        text: String,
    },
    /// Fin réelle de la réponse. `prompt_tokens` = taille du contexte au dernier appel ;
    /// les `total_*` et `api_calls` cumulent tous les appels du message (outils compris).
    Done {
        chat_id: String,
        message_id: String,
        prompt_tokens: Option<u64>,
        output_tokens: Option<u64>,
        total_prompt_tokens: u64,
        total_output_tokens: u64,
        api_calls: u32,
        eco: bool,
    },
    Failed {
        code: String,
        message: String,
        retryable: bool,
    },
    Cancelled,
}

/// Attend la réponse de l'utilisateur pour un outil. `run` = génération concernée
/// (permet d'annuler les confirmations d'un seul chat).
async fn ask(state: &State<'_, AppState>, run: &str, call_id: &str) -> bool {
    let (tx, rx) = oneshot::channel();
    if let Ok(mut map) = state.pending.lock() {
        map.insert(call_id.to_string(), (run.to_string(), tx));
    }
    rx.await.unwrap_or(false)
}

/// Version courte d'une sortie d'outil pour l'affichage (le modèle reçoit la sortie complète
/// ou, en mode économie, une version plafonnée).
fn preview(s: &str) -> String {
    if s.chars().count() > 4000 {
        let mut t: String = s.chars().take(4000).collect();
        t.push_str("\n[…]");
        t
    } else {
        s.to_string()
    }
}

/// Arguments pour l'affichage : ajoute les anciens noms (`old_string`, `new_string`,
/// `pattern`) à côté des noms du serveur (`old_text`, `new_text`, `query`), pour que
/// l'interface affiche les deux sans changement.
fn display_args(args: &Value) -> Value {
    let mut a = args.clone();
    if let Some(o) = a.as_object_mut() {
        for (from, to) in [
            ("old_text", "old_string"),
            ("new_text", "new_string"),
            ("query", "pattern"),
        ] {
            if let Some(v) = o.get(from).cloned() {
                o.entry(to).or_insert(v);
            }
        }
    }
    a
}

/// Message d'erreur lisible pour l'utilisateur.
fn explain(e: fs_api::Error, code_mode: bool) -> String {
    if let fs_api::Error::Status { status, body } = &e {
        if code_mode && *status == 403 {
            return format!(
                "Mode Code refusé (HTTP 403) : le flag `2026_11_client_tools` n'est probablement pas activé pour ton compte. Détail : {}",
                clip(body, 300)
            );
        }
        if code_mode && *status == 400 && body.contains("not supported by this model") {
            return "Ce modèle ne gère pas les outils client (Gemini). Choisis un modèle OpenAI ou Claude pour le mode Code.".to_string();
        }
    }
    e.to_string()
}

/// Mode économie : compacte le contexte quand il devient gros.
async fn eco_compact(
    c: &Client,
    chat_id: &str,
    agent_id: i64,
    prompt_tokens: u64,
    on_event: &Channel<UiEvent>,
) {
    if prompt_tokens < ECO_COMPACT_MIN {
        return;
    }
    let window = c
        .agents()
        .await
        .ok()
        .and_then(|a| a.into_iter().find(|x| x.id == agent_id))
        .and_then(|x| x.context_window);
    let threshold = window.map(|w| w / 2).unwrap_or(ECO_COMPACT_DEFAULT);
    if prompt_tokens < threshold {
        return;
    }
    let text = match c.compact(chat_id).await {
        Ok(_) => format!("Mode éco : contexte compacté (il pesait {prompt_tokens} tokens)."),
        Err(e) => format!("Mode éco : compaction auto impossible ({e})."),
    };
    let _ = on_event.send(UiEvent::Notice { text });
}

/// `chat_id` absent => nouveau chat ; présent => réponse dans ce chat
/// (l'agent peut changer d'un message à l'autre).
/// `workdir` présent => mode Code : les 6 outils du serveur (fichiers + commande) sont
/// proposés au modèle et exécutés ici, dans ce dossier uniquement.
/// `eco` absent => valeur des réglages.
/// `run_id` : identifiant de cette génération (choisi par le front) ; permet d'en annuler
/// une seule alors que plusieurs chats génèrent en parallèle.
///
/// Les générations sont indépendantes : changer de chat dans l'interface ne coupe rien,
/// les événements continuent d'arriver sur le même canal.
///
/// Les skills passent par `$nom` dans le message (instructions jointes au texte) :
/// l'API n'accepte pas d'outils personnalisés, donc ni `load_skill` ni outils MCP.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn send_message(
    app: AppHandle,
    state: State<'_, AppState>,
    chat_id: Option<String>,
    agent_id: i64,
    content: String,
    workdir: Option<String>,
    auto_accept: bool,
    eco: Option<bool>,
    run_id: Option<String>,
    images: Option<Vec<String>>,
    files: Option<Vec<String>>,
    enable_web_search: Option<bool>,
    on_event: Channel<UiEvent>,
) -> Result<(), String> {
    let run = run_id
        .filter(|r| !r.is_empty())
        .unwrap_or_else(|| format!("run-{}", RUN_SEQ.fetch_add(1, Ordering::SeqCst)));
    let flag = Arc::new(AtomicBool::new(false));
    if let Ok(mut m) = state.runs.lock() {
        m.insert(run.clone(), flag.clone());
    }

    let opts = StreamOptions {
        images: images.unwrap_or_default(),
        files: files.unwrap_or_default(),
        enable_web_search: enable_web_search.unwrap_or(false),
    };

    let result = run_message(
        &app,
        &state,
        chat_id,
        agent_id,
        content,
        workdir,
        auto_accept,
        eco,
        opts,
        &run,
        &flag,
        &on_event,
    )
    .await;

    if let Ok(mut m) = state.runs.lock() {
        m.remove(&run);
    }
    if let Ok(mut p) = state.pending.lock() {
        p.retain(|_, (r, _)| r != &run);
    }
    result
}

#[allow(clippy::too_many_arguments)]
async fn run_message(
    app: &AppHandle,
    state: &State<'_, AppState>,
    chat_id: Option<String>,
    agent_id: i64,
    content: String,
    workdir: Option<String>,
    auto_accept: bool,
    eco: Option<bool>,
    opts: StreamOptions,
    run: &str,
    flag: &AtomicBool,
    on_event: &Channel<UiEvent>,
) -> Result<(), String> {
    let c = client(app, state).await?;

    let dir = config_dir(app)?;
    let eco = eco.unwrap_or_else(|| fs_store::load(&dir).eco);
    let max_steps = if eco { MAX_STEPS_ECO } else { MAX_STEPS };
    let out_cap = if eco {
        ECO_OUTPUT_CAP
    } else {
        fs_tools::MAX_OUTPUT
    };
    let skill_cap = if eco { 6_000 } else { 20_000 };

    let root = workdir.filter(|w| !w.trim().is_empty()).map(PathBuf::from);
    let skill_list = skills::list(&dir, root.as_deref());

    // Outils : uniquement le catalogue fixe du serveur, et seulement en mode Code.
    let tools: Vec<Value> = if root.is_some() {
        fs_tools::NAMES.iter().map(|n| json!(n)).collect()
    } else {
        Vec::new()
    };

    // `$nom-de-skill` dans le message : les instructions sont jointes au message.
    let mut content = content;
    let (expanded, used) = skills::expand_mentions(&skill_list, &content, skill_cap);
    if !used.is_empty() {
        content = expanded;
        let _ = on_event.send(UiEvent::Notice {
            text: format!("Skills joints au message : {}.", used.join(", ")),
        });
    }

    let mut chat = chat_id;
    let mut results: Vec<ToolResult> = Vec::new();
    let mut total_prompt: u64 = 0;
    let mut total_output: u64 = 0;
    let mut api_calls: u32 = 0;
    let mut seen_ev: HashSet<String> = HashSet::new();

    for _ in 0..max_steps {
        let target = chat.clone();
        let mut calls: Vec<ToolCall> = Vec::new();
        let mut done: Option<(String, String, Option<u64>, Option<u64>)> = None;
        let mut stop = false;
        let mut assistant_text = String::new();

        let handler = |ev: ChatEvent| match ev {
            ChatEvent::Opened { chat_id } => {
                chat = Some(chat_id.clone());
                let _ = on_event.send(UiEvent::Opened { chat_id });
            }
            ChatEvent::Title { chat_id, title } => {
                let _ = on_event.send(UiEvent::Title { chat_id, title });
            }
            ChatEvent::Delta(text) => {
                assistant_text.push_str(&text);
                let _ = on_event.send(UiEvent::Delta { text });
            }
            ChatEvent::ReasoningDelta(text) => {
                let _ = on_event.send(UiEvent::Reasoning { text });
            }
            ChatEvent::Completed { chat_id, message } => {
                chat = Some(chat_id.clone());
                if !message.content.is_empty() {
                    assistant_text = message.content.clone();
                }
                let usage = message.token_usage.clone().unwrap_or_default();
                done = Some((
                    chat_id,
                    message.id.clone(),
                    usage.prompt_token_count,
                    usage.candidates_token_count,
                ));
                calls = message.tool_calls;
            }
            ChatEvent::Error(e) => {
                stop = true;
                let _ = on_event.send(UiEvent::Failed {
                    code: e.code,
                    message: e.message,
                    retryable: e.retryable,
                });
            }
            ChatEvent::Cancelled { .. } => {
                stop = true;
                let _ = on_event.send(UiEvent::Cancelled);
            }
            ChatEvent::Mcp(text) | ChatEvent::Notice(text) => {
                let _ = on_event.send(UiEvent::Notice { text });
            }
            ChatEvent::Other(ty) => {
                if (ty.contains("mcp") || ty.contains("tool")) && seen_ev.insert(ty.clone()) {
                    let _ = on_event.send(UiEvent::Notice {
                        text: format!("France Student a émis un événement « {ty} »."),
                    });
                }
            }
        };

        let r = match &target {
            Some(id) => {
                c.send_message_with_options(
                    id,
                    &content,
                    Some(agent_id),
                    &tools,
                    &results,
                    &opts,
                    handler,
                )
                .await
            }
            None => {
                c.stream_chat_with_options(agent_id, &content, &tools, &opts, handler)
                    .await
            }
        };
        r.map_err(|e| explain(e, root.is_some()))?;

        if stop {
            return Ok(());
        }
        let Some((chat_id, message_id, prompt_tokens, output_tokens)) = done else {
            return Ok(());
        };
        api_calls += 1;
        total_prompt += prompt_tokens.unwrap_or(0);
        total_output += output_tokens.unwrap_or(0);

        // Outils exécutés par France Student lui-même (pont MCP) : ce ne sont pas les nôtres, on ne les rejoue pas.
        let (calls, theirs): (Vec<ToolCall>, Vec<ToolCall>) = calls.into_iter().partition(|c| {
            c.name.starts_with("mcp__")
                || (root.is_some() && fs_tools::NAMES.iter().any(|n| *n == c.name))
        });
        if !theirs.is_empty() {
            let names: Vec<String> = theirs.iter().map(|c| c.name.clone()).collect();
            let _ = on_event.send(UiEvent::Notice {
                text: format!("Outils exécutés côté France Student : {}.", names.join(", ")),
            });
        }

        if calls.is_empty() {
            let _ = on_event.send(UiEvent::Done {
                chat_id: chat_id.clone(),
                message_id,
                prompt_tokens,
                output_tokens,
                total_prompt_tokens: total_prompt,
                total_output_tokens: total_output,
                api_calls,
                eco,
            });
            if eco {
                if let Some(p) = prompt_tokens {
                    eco_compact(&c, &chat_id, agent_id, p, on_event).await;
                }
            }
            return Ok(());
        }

        // Le modèle demande des outils : exécution locale ou MCP, puis nouveau tour.
        results.clear();
        for call in calls {
            if flag.load(Ordering::SeqCst) {
                let _ = on_event.send(UiEvent::Cancelled);
                return Ok(());
            }
            let args = fs_tools::parse_args(&call.arguments);

            let is_fs_mcp = call.name == "run_command"
                && args["command"]
                    .as_str()
                    .map(|c| c.trim().starts_with("fs-mcp"))
                    .unwrap_or(false);
            let is_mcp_direct = call.name.starts_with("mcp__");

            let (origin, needs_confirm) = if is_fs_mcp {
                let cmd_text = args["command"].as_str().unwrap_or("").trim();
                let sub = cmd_text.strip_prefix("fs-mcp").unwrap_or("").trim();
                let is_safe = sub.is_empty() || sub.starts_with("list") || sub.starts_with("tools");
                ("mcp".to_string(), !is_safe && !auto_accept)
            } else if is_mcp_direct {
                let trusted: HashSet<String> = state.trusted.lock().map(|g| g.clone()).unwrap_or_default();
                let is_trusted = trusted.contains(&call.name);
                ("mcp".to_string(), !is_trusted && !auto_accept)
            } else {
                let nc = match fs_tools::risk(&call.name) {
                    Some(Risk::Safe) | None => false,
                    Some(Risk::Edit) => !auto_accept,
                    Some(Risk::Command) => true,
                };
                ("local".to_string(), nc)
            };

            let _ = on_event.send(UiEvent::ToolCall {
                call_id: call.call_id.clone(),
                name: call.name.clone(),
                origin,
                args: display_args(&args),
                needs_confirm,
            });

            let allowed = if needs_confirm {
                ask(state, run, &call.call_id).await
            } else {
                true
            };
            if flag.load(Ordering::SeqCst) {
                let _ = on_event.send(UiEvent::Cancelled);
                return Ok(());
            }

            let (output, is_error) = if !allowed {
                ("Refusé par l'utilisateur.".to_string(), true)
            } else if is_fs_mcp {
                let cmd_text = args["command"].as_str().unwrap_or("").trim();
                state
                    .mcp
                    .execute_fs_mcp_command(&dir, root.as_deref(), cmd_text)
                    .await
            } else if is_mcp_direct {
                let stripped = call.name.strip_prefix("mcp__").unwrap_or("");
                let parts: Vec<&str> = stripped.splitn(2, "__").collect();
                let server = parts.first().copied().unwrap_or("");
                let tool = parts.get(1).copied().unwrap_or("");
                match state
                    .mcp
                    .call_tool_auto(&dir, root.as_deref(), server, tool, args)
                    .await
                {
                    Ok((o, err)) => (o, err),
                    Err(e) => (format!("Erreur MCP : {e}"), true),
                }
            } else {
                match root.as_ref() {
                    Some(r) => match fs_tools::execute(r, &call.name, &call.arguments).await {
                        Ok(o) => (o, false),
                        Err(e) => (e, true),
                    },
                    None => (
                        "Outil de fichiers disponible uniquement en mode Code.".to_string(),
                        true,
                    ),
                }
            };
            let _ = on_event.send(UiEvent::ToolResult {
                call_id: call.call_id.clone(),
                output: preview(&output),
                is_error,
                denied: !allowed,
            });
            let mut output = output;
            if output.chars().count() > out_cap {
                output = clip(&output, out_cap);
                output.push_str(&format!("\n[sortie tronquée à {out_cap} caractères]"));
            }
            results.push(ToolResult {
                call_id: call.call_id,
                output,
                is_error,
            });
        }
        content = String::new();
    }

    let _ = on_event.send(UiEvent::Failed {
        code: "MAX_STEPS".to_string(),
        message: format!("Arrêt après {max_steps} appels d'outils sur un seul message."),
        retryable: false,
    });
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .setup(|app| {
            bridge::autostart(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            pick_folder,
            list_agents,
            get_usage,
            list_chats,
            get_chat,
            delete_chat,
            rename_chat,
            pin_chat,
            archive_chat,
            search_chats,
            fork_edit_message,
            list_saved_messages,
            save_message,
            unsave_message,
            list_archived_chats,
            restore_chat,
            get_notification_prefs,
            update_notification_prefs,
            upload_attachment,
            open_external,
            get_context,
            compact_chat,
            cancel_chat,
            resolve_tool,
            list_skills,
            account_skills,
            mcp_status,
            mcp_get_config,
            mcp_save_config,
            mcp_connect,
            mcp_disconnect,
            ext::mcp_call,
            trust_tool,
            send_message,
            ext::skill_read,
            ext::skill_write,
            ext::skill_create,
            ext::skill_delete,
            ext::account_skill_delete,
            ext::account_skill_update,
            ext::mcp_get_server,
            ext::mcp_upsert,
            ext::mcp_remove,
            ext::mcp_import,
            bridge::bridge_start,
            bridge::bridge_stop,
            bridge::bridge_status,
            bridge::bridge_set,
        ])
        .build(tauri::generate_context!())
        .expect("erreur au lancement de FS Code")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                bridge::shutdown(app);
            }
        });
}
