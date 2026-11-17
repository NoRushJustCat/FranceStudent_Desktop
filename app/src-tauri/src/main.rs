// Pas de console supplémentaire sous Windows en release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agents;
mod agents_live;
mod changelog;
mod ext;
mod login;
mod mcp;
mod mcp_ctx;
mod perm;
mod skills;
mod term;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use fs_api::{Agent, ChatEvent, Client, StreamOptions, ToolCall, ToolResult};
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
const ECO_OUTPUT_CAP: usize = 5_000;
/// En dessous de ce nombre de tokens de prompt, le mode économie ne regarde même pas la compaction.
const ECO_COMPACT_MIN: u64 = 20_000;
/// Seuil de compaction automatique si la taille de contexte de l'agent est inconnue.
const ECO_COMPACT_DEFAULT: u64 = 40_000;

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
    /// Chats qui ont déjà reçu la liste des serveurs MCP locaux (bloc « [Contexte client] »)
    /// pendant cette session. Vidé quand mcp.json change.
    ctx_sent: Mutex<HashSet<String>>,
    /// Terminal intégré (shell persistant).
    terms: term::Terms,
    /// Mode d'autorisation courant (non persisté : jamais « système » au redémarrage).
    perm: Mutex<perm::Mode>,
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
    std::env::var_os("USERPROFILE")
        .map(|p| PathBuf::from(p).join(".gemini/config/plugins/eco-quota"))
}

fn has_eco_credentials() -> bool {
    eco_dir()
        .map(|d| d.join("fs_credentials.json").exists())
        .unwrap_or(false)
}

async fn refresh_eco_token() -> Option<(String, u64)> {
    let dir = eco_dir()?;
    let raw = tokio::fs::read_to_string(dir.join("fs_credentials.json"))
        .await
        .ok()?;
    let creds: Value = serde_json::from_str(&raw).ok()?;
    let session_token = creds.get("sessionToken")?.as_str()?.trim().to_string();
    let device_id = creds.get("deviceId")?.as_str()?.trim().to_string();
    if session_token.is_empty() || device_id.is_empty() {
        return None;
    }

    let cookie =
        format!("next-auth.session-token={session_token}; francestudent-device-id={device_id}");
    let http = reqwest::Client::new();
    let resp = http
        .get("https://ia.francestudent.org/api/auth/api-token")
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:157.0) Gecko/20100101 Firefox/157.0",
        )
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
    let expires_in = data
        .get("expiresIn")
        .and_then(|v| v.as_u64())
        .unwrap_or(300);
    let _ = tokio::fs::write(dir.join("fs_token.txt"), &tok).await;
    Some((tok, now_secs() + expires_in))
}

async fn resolve_api_key(dir: &std::path::Path) -> Option<String> {
    let stored = fs_store::get_key();
    if let Some(ref k) = stored {
        if !k.starts_with("eyJ") {
            return Some(k.clone());
        }
        if jwt_exp(k).is_some_and(|exp| now_secs() + 30 < exp) {
            return Some(k.clone());
        }
    }

    // Session ouverte avec e-mail + mot de passe : le jeton d'accès est renouvelé tout seul.
    if let Some(tok) = login::access_token(dir).await {
 return Some(tok);
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

    // Ne jamais renvoyer un JWT expiré si son renouvellement a échoué.
    stored.filter(|k| !k.starts_with("eyJ"))
}

async fn client(app: &AppHandle, state: &State<'_, AppState>) -> Result<Client, String> {
    let dir = config_dir(app)?;
    let key = resolve_api_key(&dir)
        .await
        .ok_or_else(|| {
     if login::has_session(&dir) {
  "Session France Student expirée ou injoignable : reconnecte-toi avec ton e-mail et ton mot de passe."
     } else {
         "Non connecté : connecte-toi avec ton e-mail et ton mot de passe."
     }
 })?;
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
    let settings = fs_store::load(&dir);
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
    let dir = config_dir(&app)?;
    let s = fs_store::load(&dir);
    Ok(SettingsView {
        base_url: s.base_url,
        has_key: fs_store::get_key().is_some()
     || login::has_session(&dir)
     || has_eco_credentials(),
        last_agent_id: s.last_agent_id,
        last_workdir: s.last_workdir,
        auto_accept: s.auto_accept,
        eco: s.eco,
    })
}

/// Enregistre une clé API personnelle, générée sur le site, dans le coffre-fort système.
#[tauri::command]
fn store_api_token(state: State<'_, AppState>, token: String) -> Result<(), String> {
    let key = token.trim();
    if key.is_empty() {
        return Err("Le jeton de connexion est requis.".to_string());
    }
    fs_store::set_key(key)?;
    *state.client.lock().map_err(|_| "état interne verrouillé".to_string())? = None;
    if let Ok(mut active) = ACTIVE_CLIENT_KEY.get_or_init(|| Mutex::new(None)).lock() {
        *active = None;
    }
    Ok(())
}

/// Connexion avec l'e-mail et le mot de passe France Student. Seule la session est conservée
/// (le mot de passe n'est jamais enregistré) ; le jeton d'accès se renouvelle ensuite seul.
#[tauri::command]
async fn login_password(
    app: AppHandle,
    state: State<'_, AppState>,
    email: String,
    password: String,
) -> Result<(), String> {
    let dir = config_dir(&app)?;
    login::login_with_password(&dir, &email, &password).await?;

    // Vérifie que le jeton obtenu est bien accepté par l'API avant de valider la connexion.
    let token = login::access_token(&dir)
        .await
        .ok_or("Connexion échouée : France Student n'a pas délivré de jeton d'accès.")?;
    let base_url = fs_store::load(&dir).base_url;
    let probe = Client::new(&base_url, &token).map_err(|e| e.to_string())?;
    if let Err(e) = probe.agents().await {
        let _ = login::clear_session(&dir);
        return Err(format!("Connexion refusée par France Student : {e}"));
    }

    // La session prime sur un éventuel ancien jeton collé à la main.
    fs_store::clear_key();
    *state.client.lock().map_err(|_| "état interne verrouillé".to_string())? = None;
    if let Ok(mut active) = ACTIVE_CLIENT_KEY.get_or_init(|| Mutex::new(None)).lock() {
 *active = None;
    }
    Ok(())
}

/// Déconnexion : efface le jeton enregistré dans le coffre-fort système et la session locale.
#[tauri::command]
fn logout(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    fs_store::clear_key();
    login::clear_session(&config_dir(&app)?)?;
    *state.client.lock().map_err(|_| "état interne verrouillé".to_string())? = None;
    if let Ok(mut active) = ACTIVE_CLIENT_KEY.get_or_init(|| Mutex::new(None)).lock() {
        *active = None;
    }
    Ok(())
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
    let mut v = c
        .list_chats(limit.unwrap_or(40), cursor.as_deref())
        .await
        .map_err(|e| e.to_string())?;
    mcp_ctx::strip_context(&mut v);
    agents_live::hide_chats(&app, &mut v);
    Ok(v)
}

#[tauri::command]
async fn get_chat(
    app: AppHandle,
    state: State<'_, AppState>,
    chat_id: String,
) -> Result<Value, String> {
    let c = client(&app, &state).await?;
    let mut v = c.get_chat(&chat_id).await.map_err(|e| e.to_string())?;
    mcp_ctx::strip_context(&mut v);
    Ok(v)
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
    c.pin_chat(&chat_id, pinned)
        .await
        .map_err(|e| e.to_string())
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
    let mut v = c
        .search_chats(query.trim(), limit.unwrap_or(20))
        .await
        .map_err(|e| e.to_string())?;
    agents_live::hide_chats(&app, &mut v);
    Ok(v)
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
async fn get_user_instructions(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let c = client(&app, &state).await?;
    c.get_user_instructions().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn update_user_instructions(
    app: AppHandle,
    state: State<'_, AppState>,
    body: Value,
) -> Result<Value, String> {
    let c = client(&app, &state).await?;
    c.update_user_instructions(body)
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
    /// Toujours false : le modèle appelle les outils MCP via `run_command` (`mcp call …`),
    /// exécutés ici en local, sans tunnel.
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
fn mcp_save_config(app: AppHandle, state: State<'_, AppState>, text: String) -> Result<(), String> {
    mcp::save_config_text(&config_dir(&app)?, &text)?;
    // La liste des serveurs/outils a pu changer : elle sera rejointe au prochain message de chaque chat.
    if let Ok(mut sent) = state.ctx_sent.lock() {
        sent.clear();
    }
    Ok(())
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
    /// répondre via `resolve_tool`. `origin` : `local` (fichiers/commande) ou `mcp`
    /// (serveur MCP local, via `mcp call …`).
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

/// Message d'erreur lisible pour l'utilisateur. `tools_on` : des outils client étaient
/// proposés au modèle (mode Code, ou serveurs MCP locaux configurés).
fn explain(e: fs_api::Error, tools_on: bool) -> String {
    if let fs_api::Error::Status { status, body } = &e {
        if tools_on && *status == 403 {
            return format!(
                "Outils client refusés (HTTP 403) : le flag `2026_11_client_tools` n'est probablement pas activé pour ton compte. Détail : {}",
                clip(body, 300)
            );
        }
        if tools_on && *status == 400 && body.contains("not supported by this model") {
            return "Ce modèle ne gère pas les outils client (Gemini). Choisis un modèle OpenAI ou Claude (mode Code ou outils MCP).".to_string();
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
    let threshold = window.map(|w| w / 3).unwrap_or(ECO_COMPACT_DEFAULT);
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
/// Sans `workdir`, si des serveurs MCP sont configurés, seul `run_command` est proposé, et
/// uniquement pour les commandes `mcp …` (serveurs MCP locaux).
/// `eco` absent => valeur des réglages.
/// `run_id` : identifiant de cette génération (choisi par le front) ; permet d'en annuler
/// une seule alors que plusieurs chats génèrent en parallèle.
///
/// Les générations sont indépendantes : changer de chat dans l'interface ne coupe rien,
/// les événements continuent d'arriver sur le même canal.
///
/// Les skills passent par `$nom` dans le message (instructions jointes au texte). Les
/// serveurs MCP locaux passent par l'outil client `run_command` (`mcp list|tools|call …`) :
/// leur liste est jointe au premier message d'un chat. Pas de pont, pas de tunnel.
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
    local_prompt: Option<String>,
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
        local_prompt,
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

/// Titre local d'un chat : première ligne du message, 50 caractères au plus.
fn local_title(s: &str) -> String {
    let line = s
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("Nouveau chat");
    let mut t: String = line.chars().take(50).collect();
    if line.chars().count() > 50 {
        t.push('…');
    }
    t
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
    mut opts: StreamOptions,
    local_prompt: Option<String>,
    run: &str,
    flag: &AtomicBool,
    on_event: &Channel<UiEvent>,
) -> Result<(), String> {
    let dir = config_dir(app)?;
    let eco = eco.unwrap_or_else(|| fs_store::load(&dir).eco);
    let max_steps = if eco { MAX_STEPS_ECO } else { MAX_STEPS };
    let out_cap = if eco {
        ECO_OUTPUT_CAP
    } else {
        fs_tools::MAX_OUTPUT
    };
    let skill_cap = if eco { 4_000 } else { 20_000 };
    if eco && opts.enable_web_search {
        // Les résultats de recherche web pèsent lourd en tokens.
        opts.enable_web_search = false;
        let _ = on_event.send(UiEvent::Notice {
            text: "Mode éco : recherche web désactivée pour ce message.".to_string(),
        });
    }

    let root = workdir.filter(|w| !w.trim().is_empty()).map(PathBuf::from);
    let skill_list = skills::list(&dir, root.as_deref());
    // Au moins un serveur MCP local configuré (mcp.json de l'app ou du projet) ?
    let mcp_on = !mcp::Hub::load_all_servers(&dir, root.as_deref()).is_empty();

    // Outils client : le catalogue fixe du serveur en mode Code ; sinon, seulement
    // `run_command` (réservé aux commandes `mcp …`) quand des serveurs MCP existent.
    let tools: Vec<Value> = if root.is_some() {
        fs_tools::NAMES.iter().map(|n| json!(n)).collect()
    } else if mcp_on {
        vec![json!("run_command")]
    } else {
        Vec::new()
    };

    // `$nom-de-skill` dans le message : les instructions sont jointes au message.
    let mut content = content;
    let raw_title = local_title(&content);
    let (expanded, used) = skills::expand_mentions(&skill_list, &content, skill_cap);
    if !used.is_empty() {
        content = expanded;
        let _ = on_event.send(UiEvent::Notice {
            text: format!("Skills joints au message : {}.", used.join(", ")),
        });
    }

    // Premier message d'un chat (par session) : joint la liste des serveurs MCP locaux et
    // la façon de les appeler. Marqué « envoyé » seulement une fois l'envoi réussi.
    let mut mcp_ctx_pending = false;
    let mut retitle: Option<String> = None;
    if mcp_on {
        let already_sent = match chat_id.as_deref() {
            Some(id) => state
                .ctx_sent
                .lock()
                .map(|s| s.contains(id))
                .unwrap_or(false),
            None => false,
        };
        if !already_sent {
            if let Some(ctx) = state
                .mcp
                .context_text(&dir, root.as_deref(), eco)
                .await
            {
                content = format!("{ctx}\n\n---\n{content}");
                mcp_ctx_pending = true;
                if chat_id.is_none() {
                    retitle = Some(raw_title.clone());
                }
                let _ = on_event.send(UiEvent::Notice {
                    text: "Outils MCP locaux joints au message.".to_string(),
                });
            }
        }
    }

    // Instructions locales de l'utilisateur, jointes au message ([Contexte client]).
    {
        let mut pre = String::new();
        // Prompt système local (mode développeur) : joint à chaque message, jamais synchronisé.
        if let Some(p) = local_prompt.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
     pre.push_str("Instructions locales de l'utilisateur :\n");
     pre.push_str(p);
     pre.push('\n');
        }
        if !pre.is_empty() {
            let pre = pre.trim_end().to_string();
            content = if content.starts_with("[Contexte client]") {
                content.replacen(
                    "[Contexte client]",
                    &format!("[Contexte client]\n{pre}\n"),
                    1,
                )
            } else {
                format!("[Contexte client]\n{pre}\n\n---\n{content}")
            };
            if chat_id.is_none() && retitle.is_none() {
                retitle = Some(raw_title.clone());
            }
        }
    }

    let mut chat = chat_id;
    let mut results: Vec<ToolResult> = Vec::new();
    let mut total_prompt: u64 = 0;
    let mut total_output: u64 = 0;
    let mut api_calls: u32 = 0;
    let mut seen_ev: HashSet<String> = HashSet::new();

    for _ in 0..max_steps {
        // Un tour avec outils peut durer plus longtemps que le jeton initial.
        let c = client(app, state).await?;
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
                let title = retitle.clone().unwrap_or(title);
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
        r.map_err(|e| explain(e, !tools.is_empty()))?;

        // Titre local : le serveur nommerait le chat d'après le bloc de contexte.
        if let (Some(t), Some(id)) = (retitle.take(), chat.as_ref()) {
            let _ = c.rename_chat(id, &t).await;
            let _ = on_event.send(UiEvent::Title {
                chat_id: id.clone(),
                title: t,
            });
        }

        // Le contexte MCP est parti avec ce message : on ne le rejoint plus dans ce chat.
        if mcp_ctx_pending {
            mcp_ctx_pending = false;
            if let Some(id) = chat.as_ref() {
                if let Ok(mut sent) = state.ctx_sent.lock() {
                    sent.insert(id.clone());
                }
            }
        }

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
        // `run_command` est à nous en mode Code, et hors mode Code dès que des serveurs MCP sont configurés.
        let (calls, theirs): (Vec<ToolCall>, Vec<ToolCall>) = calls.into_iter().partition(|c| {
            c.name.starts_with("mcp__")
                || (root.is_some() && fs_tools::NAMES.iter().any(|n| *n == c.name))
                || (mcp_on && c.name == "run_command")
        });
        if !theirs.is_empty() {
            let names: Vec<String> = theirs.iter().map(|c| c.name.clone()).collect();
            let _ = on_event.send(UiEvent::Notice {
                text: format!(
                    "Outils exécutés côté France Student : {}.",
                    names.join(", ")
                ),
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

            // `run_command` avec une commande `mcp …` (ou `fs-mcp …`) : serveurs MCP locaux,
            // exécutés ici (aucun shell, aucun tunnel), y compris hors mode Code.
            let mcp_cmd: Option<String> = if call.name == "run_command" {
                args["command"]
                    .as_str()
                    .filter(|c| mcp_ctx::mcp_rest(c).is_some())
                    .map(|c| c.trim().to_string())
            } else {
                None
            };
            // Commande lancée par l'IA : recopiée dans le terminal intégré avec sa sortie.
            let ai_cmd: Option<String> = if call.name == "run_command" {
                args["command"].as_str().map(str::to_string)
            } else {
                None
            };
            // Sous-agents : run_command dont la commande commence par `agent` (mode Code seulement).
            let agent_cmd: Option<String> = if call.name == "run_command" && root.is_some() {
                args["command"]
                    .as_str()
                    .and_then(agents::agent_rest)
                    .map(str::to_string)
            } else {
                None
            };
            // Mode d autorisation lu une fois par appel ; blocked = refus automatique (planification).
            let mode = perm::current(state);
            let mut blocked: Option<String> = None;
            let is_mcp_direct = call.name.starts_with("mcp__");
            let change_label = changelog::label(&call.name, &args, mcp_cmd.as_deref());

            let (origin, needs_confirm) = if let Some(cmd_text) = mcp_cmd.as_deref() {
                // `mcp list` et `mcp tools …` sont sans effet de bord ; `mcp call …` demande accord.
                let sub = mcp_ctx::mcp_rest(cmd_text).unwrap_or("");
                let verb = sub
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                let is_safe = matches!(verb.as_str(), "" | "list" | "tools");
                let tool = sub.split_whitespace().nth(2).unwrap_or("");
                (
                    "mcp".to_string(),
                    !is_safe && perm::mcp_ask(mode, auto_accept, tool),
                )
            } else if is_mcp_direct {
                let trusted: HashSet<String> =
                    state.trusted.lock().map(|g| g.clone()).unwrap_or_default();
                let is_trusted = trusted.contains(&call.name);
                let tool = call.name.splitn(3, "__").nth(2).unwrap_or("");
                (
                    "mcp".to_string(),
                    !is_trusted && perm::mcp_ask(mode, auto_accept, tool),
                )
            } else {
                let nc = if root.is_none() {
                    // Pas de mode Code : refus immédiat plus bas, inutile de demander.
                    false
                } else {
                    if agent_cmd.is_some() {
                        false
                    } else {
                        match perm::decide(mode, auto_accept, &call.name, &args, root.as_deref()) {
                            perm::Verdict::Allow => false,
                            perm::Verdict::Ask => true,
                            perm::Verdict::Deny(m) => {
                                blocked = Some(m);
                                false
                            }
                        }
                    }
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
            } else if let Some(m) = blocked.clone() {
                (m, true)
            } else if let Some(rest) = agent_cmd.as_deref() {
                match root.as_deref() {
                    Some(r) => {
                        let env = agents::Env {
                            app,
                            client: &c,
                            agent_id,
                            root: r,
                            auto_accept,
                            run,
                            flag,
                            on_event,
                            parent_call: &call.call_id,
                        };
                        agents::exec(&env, rest).await
                    }
                    None => (
                        "Les sous-agents exigent le mode Code (dossier de travail).".to_string(),
                        true,
                    ),
                }
            } else if let Some(cmd_text) = mcp_cmd.as_deref() {
                state
                    .mcp
                    .exec_mcp(&dir, root.as_deref(), cmd_text, eco)
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
                    Some(r) => {
                        let res = if mode == perm::Mode::Full {
                            fs_tools::execute_full(r, &call.name, &call.arguments).await
                        } else {
                            fs_tools::execute(r, &call.name, &call.arguments).await
                        };
                        match res {
                            Ok(o) => (o, false),
                            Err(e) => (e, true),
                        }
                    }
                    None if call.name == "run_command" => (
                        "Hors mode Code, run_command n'accepte que les commandes `mcp …`."
                            .to_string(),
                        true,
                    ),
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
                denied: !allowed || blocked.is_some(),
            });
            if let Some(cmd) = ai_cmd.as_deref().filter(|_| agent_cmd.is_none()) {
                term::emit_ai(app, cmd, &clip(&output, 20_000), is_error, root.as_deref());
            }
            if let Some(label) = change_label.as_deref() {
                let status = if !allowed {
                    "refusé"
                } else if is_error {
                    "échec"
                } else {
                    "ok"
                };
                changelog::record(&dir, &call.name, label, status, root.as_deref());
                let _ = on_event.send(UiEvent::Notice {
                    text: format!("✎ {label} · {status}"),
                });
            }
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

/// Journal des modifications faites par l'IA (les `limit` dernières, 200 par défaut).
#[tauri::command]
fn get_change_log(app: AppHandle, limit: Option<usize>) -> Result<Vec<Value>, String> {
    Ok(changelog::read_tail(
        &config_dir(&app)?,
        limit.unwrap_or(200),
    ))
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            store_api_token,
 login_password,
            logout,
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
            get_user_instructions,
            update_user_instructions,
            upload_attachment,
            open_external,
            get_change_log,
            agents_live::agents_live_snapshot,
            agents_live::agents_live_cancel,
            agents_live::agents_live_clear,
            term::term_start,
            term::term_write,
            term::term_kill,
            perm::set_perm_mode,
            perm::get_perm_mode,
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
        ])
        .build(tauri::generate_context!())
        .expect("erreur au lancement de FS Code")
        .run(|_app, _event| {});
}
