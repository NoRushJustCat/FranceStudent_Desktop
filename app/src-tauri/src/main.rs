// Pas de console supplémentaire sous Windows en release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agent;
mod bridge;
mod changelog;
mod ext;
mod instructions;
mod jobguard;
mod login;
mod mcp;
mod mcp_ctx;
mod perm;
mod plugins;
mod skills;
mod term;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use fs_api::{Agent, Client};
use serde::Serialize;
use serde_json::Value;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::oneshot;

/// Compteur pour les identifiants de génération quand le front n'en fournit pas.
static RUN_SEQ: AtomicU64 = AtomicU64::new(1);
/// Fermeture en cours : évite de relancer le nettoyage en boucle.
static QUITTING: AtomicBool = AtomicBool::new(false);

/// Confirmation d'outil en attente : (identifiant de la génération, canal de réponse).
type Pending = (String, oneshot::Sender<bool>);

/// Client HTTP mis en cache (vidé quand l'URL ou la clé changent),
/// confirmations d'outils en attente, générations en cours (une par chat, en parallèle),
/// MCP et autorisations de session.
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
    /// Terminal intégré (shell persistant).
    terms: term::Terms,
    /// Mode d'autorisation courant (non persisté : jamais « système » au redémarrage).
    perm: Mutex<perm::Mode>,
    /// Flux simultanés (2 par défaut).
    gate: tokio::sync::Semaphore,
    /// Chat → run actif (un seul run par chat).
    chat_runs: Mutex<HashMap<String, String>>,
    /// Pont MCP + tunnel Cloudflare.
    bridge: bridge::Bridge,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            client: Mutex::new(None),
            pending: Mutex::new(HashMap::new()),
            runs: Mutex::new(HashMap::new()),
            mcp: mcp::Hub::default(),
            mcp_up: Mutex::new(HashSet::new()),
            trusted: Mutex::new(HashSet::new()),
            terms: term::Terms::default(),
            perm: Mutex::new(perm::Mode::default()),
            gate: tokio::sync::Semaphore::new(2),
            chat_runs: Mutex::new(HashMap::new()),
            bridge: bridge::Bridge::default(),
        }
    }
}

fn config_dir(app: &AppHandle) -> Result<PathBuf, String> {    app.path().app_config_dir().map_err(|e| e.to_string())
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
    instructions::schedule(&app);
    Ok(())
}

/// Déconnexion : retire d'abord le bloc d'instructions (tant que le jeton existe), puis efface
/// le jeton du coffre-fort système et la session locale. Si le retrait échoue ou dépasse 6 s,
/// la déconnexion continue ; le nettoyage est réessayé à la prochaine connexion.
#[tauri::command]
async fn logout(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(6),
        instructions::remove(&app),
    )
    .await;
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
    Ok(v)
}

fn check_id(s: &str) -> Result<(), String> {
    if !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        Ok(())
    } else {
        Err("identifiant invalide".to_string())
    }
}

/// Mes projets + ceux partagés avec moi (le partage est facultatif : erreur ignorée).
#[tauri::command]
async fn list_projects(app: AppHandle, state: State<'_, AppState>) -> Result<Value, String> {
    let c = client(&app, &state).await?;
    let mut all = match c.projects().await.map_err(|e| e.to_string())? {
 Value::Array(a) => a,
        _ => Vec::new(),
    };
    if let Ok(Value::Array(shared)) = c.shared_projects().await {
        for p in shared {
     if !all.iter().any(|x| x["id"] == p["id"]) {
         all.push(p);
     }
 }
    }
    Ok(Value::Array(all))
}

#[tauri::command]
async fn list_project_chats(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    limit: Option<u32>,
) -> Result<Value, String> {
    check_id(&project_id)?;
    let c = client(&app, &state).await?;
    let mut v = c
 .project_chats(&project_id, limit.unwrap_or(40))
        .await
        .map_err(|e| e.to_string())?;
    mcp_ctx::strip_context(&mut v);
    Ok(v)
}

#[tauri::command]
async fn attach_chat_to_project(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    chat_id: String,
) -> Result<(), String> {
    check_id(&project_id)?;
    check_id(&chat_id)?;
    let c = client(&app, &state).await?;
    c.attach_chat(&project_id, &chat_id)
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
fn mcp_save_config(app: AppHandle, text: String) -> Result<(), String> {
    mcp::save_config_text(&config_dir(&app)?, &text)?;
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
    /// Reprise d'un run interrompu.
    Resumed {
        run_id: String,
    },
    /// Appels d'outils en attente de décision.
    AwaitingTools {
        run_id: String,
        calls: usize,
    },
    /// Run interrompu qui ne peut pas reprendre automatiquement.
    RunInterrupted {
        run_id: String,
        reason: String,
    },
    /// Consommation de contexte du chat (`GET /chats/{id}/usage`).
    Usage {
        chat_id: String,
        percent: Option<f64>,
        used_tokens: Option<u64>,
    },
    /// Seuil de requêtes atteint pour ce run.
    BudgetWarning {
        run_id: String,
        requests: u32,
    },
    /// Run en file d'attente (limite de flux simultanés).
    Queued {
        run_id: String,
    },
}

/// Attend la réponse de l'utilisateur pour un outil. `run` = génération concernée
/// (permet d'annuler les confirmations d'un seul chat).
pub(crate) async fn ask(state: &State<'_, AppState>, run: &str, call_id: &str) -> bool {
    let (tx, rx) = oneshot::channel();
    if let Ok(mut map) = state.pending.lock() {
        map.insert(call_id.to_string(), (run.to_string(), tx));
    }
    rx.await.unwrap_or(false)
}

/// Envoie un message. `workdir` renseigné => mode Code (les 6 outils du catalogue).
/// Sans `workdir`, aucun outil client n'est proposé : les MCP passent par le tunnel
/// France Student, jamais par la boucle locale.
/// `local_prompt` est accepté pour compatibilité mais n'est plus jamais injecté.
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
    let _ = local_prompt; // plus jamais ajouté au message
    let dir = config_dir(&app)?;
    let eco = eco.unwrap_or_else(|| fs_store::load(&dir).eco);
    let run = run_id
        .filter(|r| !r.is_empty())
        .unwrap_or_else(|| format!("run-{}", RUN_SEQ.fetch_add(1, Ordering::SeqCst)));

    let flag = Arc::new(AtomicBool::new(false));
    if let Ok(mut runs) = state.runs.lock() {
        runs.insert(run.clone(), flag.clone());
    }

    // Un seul run actif par chat.
    let chat_key = chat_id.clone();
    if let Some(chat) = chat_key.as_deref() {
        if let Ok(mut guard) = state.chat_runs.lock() {
            match guard.get(chat) {
                Some(owner) if owner != &run => {
                    let _ = on_event.send(UiEvent::Failed {
                        code: "CHAT_BUSY".to_string(),
                        message: "Une réponse est déjà en cours dans ce chat.".to_string(),
                        retryable: false,
                    });
                    if let Ok(mut runs) = state.runs.lock() {
                        runs.remove(&run);
                    }
                    return Ok(());
                }
                Some(_) => {}
                None => {
                    guard.insert(chat.to_string(), run.clone());
                }
            }
        }
    }

    // Deux flux simultanés au maximum ; les autres attendent leur tour.
    if state.gate.available_permits() == 0 {
        let _ = on_event.send(UiEvent::Queued {
            run_id: run.clone(),
        });
    }
    let _permit = state
        .gate
        .acquire()
        .await
        .map_err(|_| "file d'attente indisponible".to_string())?;

    let root = workdir
        .filter(|w| !w.trim().is_empty())
        .map(PathBuf::from);
    let tools: Vec<String> = if root.is_some() {
        fs_tools::NAMES.iter().map(|name| name.to_string()).collect()
    } else {
        Vec::new()
    };

    let outcome = async {
        if let Err((code, message)) = agent::preflight(&app, agent_id, &tools).await {
            let _ = on_event.send(UiEvent::Failed {
                code,
                message,
                retryable: false,
            });
            return Ok(());
        }

        let request = fs_agent::RunRequest {
            run_id: run.clone(),
            chat_id,
            agent_id,
            content,
            workdir: root,
            auto_accept,
            eco,
            tools,
            options: fs_api::StreamOptions {
                images: images.unwrap_or_default(),
                files: files.unwrap_or_default(),
                enable_web_search: enable_web_search.unwrap_or(false),
            },
        };
        let control = fs_agent::RunControl::from_flag(flag.clone());
        agent::run(&app, request, control, &on_event)
            .await
            .map(|_| ())
    }
    .await;

    if let Ok(mut runs) = state.runs.lock() {
        runs.remove(&run);
    }
    if let Ok(mut chat_runs) = state.chat_runs.lock() {
        if chat_key.as_deref().and_then(|chat| chat_runs.get(chat)) == Some(&run) {
            if let Some(chat) = chat_key.as_deref() {
                chat_runs.remove(chat);
            }
        }
    }
    if let Ok(mut pending) = state.pending.lock() {
        pending.retain(|_, (owner, _)| owner != &run);
    }
    outcome
}

/// Runs non terminaux (reprise après redémarrage).
#[tauri::command]
async fn list_runs(app: AppHandle) -> Result<Vec<fs_agent::RunState>, String> {
    agent::list_open(&app).await
}

/// Reprend un run interrompu.
#[tauri::command]
async fn resume_run(
    app: AppHandle,
    state: State<'_, AppState>,
    run_id: String,
    on_event: Channel<UiEvent>,
) -> Result<(), String> {
    let flag = {
        let mut runs = state
            .runs
            .lock()
            .map_err(|_| "état interne verrouillé".to_string())?;
        runs.entry(run_id.clone())
            .or_insert_with(|| Arc::new(AtomicBool::new(false)))
            .clone()
    };
    let control = fs_agent::RunControl::from_flag(flag);

    // Même limites que `send_message` : un run par chat, deux flux simultanés.
    let chat_key = match agent::load_run(&app, &run_id).await {
        Ok(run) => run.chat_id,
        Err(error) => {
            if let Ok(mut runs) = state.runs.lock() {
                runs.remove(&run_id);
            }
            return Err(error);
        }
    };
    if let Some(chat) = chat_key.as_deref() {
        let busy = state
            .chat_runs
            .lock()
            .map(|guard| guard.contains_key(chat))
            .unwrap_or(false);
        if busy {
            if let Ok(mut runs) = state.runs.lock() {
                runs.remove(&run_id);
            }
            let _ = on_event.send(UiEvent::Failed {
                code: "CHAT_BUSY".to_string(),
                message: "Une réponse est déjà en cours dans ce chat.".to_string(),
                retryable: false,
            });
            return Ok(());
        }
        if let Ok(mut guard) = state.chat_runs.lock() {
            guard.insert(chat.to_string(), run_id.clone());
        }
    }
    if state.gate.available_permits() == 0 {
        let _ = on_event.send(UiEvent::Queued {
            run_id: run_id.clone(),
        });
    }
    let _permit = state
        .gate
        .acquire()
        .await
        .map_err(|_| "file d'attente indisponible".to_string())?;

    let result = agent::resume(&app, &run_id, control, &on_event)
        .await
        .map(|_| ());
    if let Ok(mut runs) = state.runs.lock() {
        runs.remove(&run_id);
    }
    if let Some(chat) = chat_key.as_deref() {
        if let Ok(mut guard) = state.chat_runs.lock() {
            if guard.get(chat).map(String::as_str) == Some(run_id.as_str()) {
                guard.remove(chat);
            }
        }
    }
    if let Ok(mut pending) = state.pending.lock() {
        pending.retain(|_, (owner, _)| owner != &run_id);
    }
    result
}

/// Abandonne un run : drapeau d'annulation, confirmations en attente, état persisté.
#[tauri::command]
async fn abandon_run(app: AppHandle, state: State<'_, AppState>, run_id: String) -> Result<(), String> {
    if let Ok(runs) = state.runs.lock() {
        if let Some(flag) = runs.get(&run_id) {
            flag.store(true, Ordering::SeqCst);
        }
    }
    if let Ok(mut pending) = state.pending.lock() {
        pending.retain(|_, (owner, _)| owner != &run_id);
    }
    agent::abandon(&app, &run_id).await.map(|_| ())
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
    // Si l'app est tuée ou plante, Windows tue aussi cloudflared et les serveurs MCP.
    if let Err(e) = jobguard::init() {
 eprintln!("job object : {e}");
    }
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
     list_projects,
     list_project_chats,
     attach_chat_to_project,
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
            list_runs,
            resume_run,
            abandon_run,
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
     plugins::plugin_list,
     plugins::plugin_add,
     plugins::plugin_remove,
     plugins::plugin_set_enabled,
     plugins::plugin_update,
            bridge::bridge_start,
            bridge::bridge_stop,
            bridge::bridge_status,
     bridge::bridge_confirm_answer,
            bridge::bridge_set,
            bridge::bridge_set_token,
            bridge::bridge_set_access,
     instructions::instructions_status,
     instructions::instructions_sync,
     instructions::instructions_remove,
     instructions::instructions_set_enabled,
        ])
        .setup(|app| {
            bridge::on_launch(app.handle());
     instructions::on_launch(app.handle());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("erreur au lancement de FS Code")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                if QUITTING.swap(true, Ordering::SeqCst) {
                    api.prevent_exit();
                    return;
                }
                api.prevent_exit();
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = tokio::time::timeout(
                        bridge::SHUTDOWN_WAIT,
                        async {
       let b = app.clone();
       let h = tauri::async_runtime::spawn(async move {
    let _ = instructions::remove(&b).await;
       });
       let _ = bridge::stop(&app, false).await;
       let _ = h.await;
          },
                    )
                    .await;
                    app.exit(0);
                });
            }
        });
}
