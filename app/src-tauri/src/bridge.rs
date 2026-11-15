//! Pont MCP : met les serveurs MCP locaux (ceux de `mcp.json`) à disposition du serveur
//! France Student, via un petit serveur HTTP local + un tunnel `cloudflared`.
//!
//! Chaîne : modèle → serveur FS → `https://xxxx.trycloudflare.com/mcp?t=…` → cloudflared
//! → `127.0.0.1:PORT/mcp` (ce fichier) → `Hub::call_tool` → serveur MCP local.
//!
//! C'est le serveur FS qui appelle le MCP (fonction officielle `/mcp/servers`) : FS Code
//! ne pilote aucune boucle d'outils ici, il répond seulement à des requêtes JSON-RPC.
//!
//! Sécurité :
//! - jeton aléatoire obligatoire (nouveau à chaque lancement), comparé en temps constant ;
//! - écoute sur 127.0.0.1 uniquement (seul cloudflared peut s'y connecter) ;
//! - confirmation native avant chaque outil non « lecture seule », sauf `auto_allow`
//!   ou outil marqué « toujours autoriser » dans l'onglet MCP.
//!
//! Réglages dans `bridge.json` (dossier de config de l'app) :
//! ```json
//! { "auto_start": false, "auto_allow": false, "label": "fscode",
//!   "register_extra": { "require_approval": "never", "defer_loading": true } }
//! ```
//! `register_extra` est fusionné tel quel dans le corps de `POST /mcp/servers`
//! (pour ajuster `auth_type`, `require_approval`, `defer_loading`, `allowed_tools`…
//! sans recompiler).

use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{RawQuery, State as AxState};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use fs_api::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::oneshot;
use tokio::time::{sleep, timeout, Instant};

use crate::mcp::{self, Exposed};
use crate::{clip, config_dir, AppState};

const CONFIG_FILE: &str = "bridge.json";
const LABEL: &str = "fscode";
const TUNNEL_WAIT: Duration = Duration::from_secs(45);
const REACH_WAIT: Duration = Duration::from_secs(25);
const CONFIRM_WAIT: Duration = Duration::from_secs(120);
const SHUTDOWN_WAIT: Duration = Duration::from_secs(8);
/// Sortie d'outil renvoyée au serveur FS : plafond en caractères.
const MAX_OUT: usize = 50_000;
/// Lignes gardées dans le journal affiché par l'interface.
const LOG_MAX: usize = 30;
/// Versions du protocole MCP gérées (la plus récente d'abord).
const PROTOCOLS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

// ---------- Configuration (bridge.json) ----------

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
struct BridgeConfig {
    /// Démarre le pont au lancement de l'app.
    auto_start: bool,
    /// Pas de confirmation avant les outils (dangereux).
    auto_allow: bool,
    /// Nom (`server_label`) de l'entrée créée chez France Student.
    label: String,
    /// Champs ajoutés au corps de `POST /mcp/servers`.
    register_extra: Map<String, Value>,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            auto_start: false,
            auto_allow: false,
            label: LABEL.to_string(),
            register_extra: Map::new(),
        }
    }
}

/// Fichier absent => réglages par défaut ; fichier invalide => erreur (jamais écrasé en silence).
fn load_cfg(dir: &Path) -> Result<BridgeConfig, String> {
    let Ok(text) = std::fs::read_to_string(dir.join(CONFIG_FILE)) else {
        return Ok(BridgeConfig::default());
    };
    let mut c: BridgeConfig =
        serde_json::from_str(&text).map_err(|e| format!("bridge.json invalide : {e}"))?;
    if c.label.trim().is_empty() {
        c.label = LABEL.to_string();
    }
    Ok(c)
}

fn save_cfg(dir: &Path, cfg: &BridgeConfig) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(CONFIG_FILE), text).map_err(|e| e.to_string())
}

// ---------- État ----------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    #[default]
    Stopped,
    Starting,
    Running,
    Error,
}

/// Photo de l'état, renvoyée à l'interface.
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    phase: Phase,
    /// Étape en cours pendant le démarrage.
    step: String,
    /// URL publique du tunnel (sans le jeton).
    url: Option<String>,
    /// Identifiant de l'entrée chez France Student.
    server_id: Option<String>,
    port: Option<u16>,
    tools: usize,
    servers: Vec<String>,
    error: Option<String>,
    warnings: Vec<String>,
    /// Réponses de France Student à l'enregistrement (jeton masqué) : pour diagnostiquer.
    register_note: Option<String>,
    /// Requêtes reçues du serveur FS depuis le lancement.
    hits: u64,
    log: Vec<String>,
    auto_allow: bool,
    auto_start: bool,
}

/// Ressources vivantes du pont.
struct Live {
    server: Option<JoinHandle<()>>,
    tunnel: Option<Child>,
    /// Entrée créée chez France Student (à supprimer à l'arrêt).
    server_id: Option<String>,
}

/// État partagé du pont (champ `bridge` de `AppState`).
#[derive(Default)]
pub struct Bridge {
    /// Sérialise démarrage et arrêt.
    op: tokio::sync::Mutex<()>,
    live: tokio::sync::Mutex<Option<Live>>,
    /// Une seule confirmation à l'écran à la fois.
    ask: tokio::sync::Mutex<()>,
    /// Jamais tenu pendant un `await`.
    snap: Mutex<Snapshot>,
    auto_allow: AtomicBool,
}

fn push_log(s: &mut Snapshot, text: String) {
    s.log.push(text);
    if s.log.len() > LOG_MAX {
        s.log.remove(0);
    }
}

impl Bridge {
    fn set(&self, f: impl FnOnce(&mut Snapshot)) {
        if let Ok(mut s) = self.snap.lock() {
            f(&mut s);
        }
    }

    fn snapshot(&self) -> Snapshot {
        self.snap.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn step(&self, text: &str) {
        self.set(|s| s.step = text.to_string());
    }

    fn note(&self, text: String) {
        self.set(|s| push_log(s, text));
    }

    /// Une requête du serveur FS (comptée et journalisée).
    fn hit(&self, text: String) {
        self.set(|s| {
            s.hits += 1;
            push_log(s, text);
        });
    }

    /// Repart d'un état propre en gardant le journal et le compteur.
    fn reset(&self, phase: Phase, step: &str) {
        self.set(|s| {
            *s = Snapshot {
                phase,
                step: step.to_string(),
                hits: s.hits,
                log: std::mem::take(&mut s.log),
                ..Snapshot::default()
            };
        });
    }
}

// ---------- Serveur MCP local (ce que FS appelle à travers le tunnel) ----------

#[derive(Clone)]
struct Ctx {
    app: AppHandle,
    token: Arc<str>,
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut d = 0u8;
    for (x, y) in a.iter().zip(b) {
        d |= x ^ y;
    }
    d == 0
}

/// Jeton accepté en `Authorization: Bearer …` (ou brut) ou en `?t=…`.
fn authorized(ctx: &Ctx, headers: &HeaderMap, query: &Option<String>) -> bool {
    let want = ctx.token.as_bytes();
    if let Some(v) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        let v = v.trim();
        let t = v
            .strip_prefix("Bearer ")
            .or_else(|| v.strip_prefix("bearer "))
            .unwrap_or(v)
            .trim();
        if ct_eq(t.as_bytes(), want) {
            return true;
        }
    }
    if let Some(q) = query {
        for pair in q.split('&') {
            if let Some(t) = pair.strip_prefix("t=") {
                if ct_eq(t.as_bytes(), want) {
                    return true;
                }
            }
        }
    }
    false
}

fn safe(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Nom de l'outil tel que vu par France Student : `serveur__outil` (64 caractères max chez eux).
fn public_name(server: &str, tool: &str) -> String {
    let mut n = format!("{}__{}", safe(server), safe(tool));
    n.truncate(56); // ASCII uniquement : pas de coupure au milieu d'un caractère
    n
}

fn tool_result(text: &str, is_error: bool) -> Value {
    let text = if text.is_empty() { "(aucune sortie)" } else { text };
    json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
}

fn initialize_result(params: &Value) -> Value {
    let want = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or("");
    let version = PROTOCOLS
        .iter()
        .find(|v| **v == want)
        .copied()
        .unwrap_or(PROTOCOLS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "fs-code-bridge", "version": env!("CARGO_PKG_VERSION") }
    })
}

async fn list_tools(ctx: &Ctx) -> Vec<Value> {
    let state = ctx.app.state::<AppState>();
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for e in state.mcp.exposed().await {
        let name = public_name(&e.server, &e.tool.name);
        if !seen.insert(name.clone()) {
            continue;
        }
        let mut t = json!({
            "name": name,
            "description": clip(&format!("[{}] {}", e.server, e.tool.description), 1500),
            "inputSchema": e.tool.input_schema,
        });
        if e.tool.read_only {
            t["annotations"] = json!({ "readOnlyHint": true });
        }
        out.push(t);
    }
    out
}

/// Boîte de dialogue native : autoriser cet appel ? Sans réponse à temps => refus.
async fn confirm(app: &AppHandle, e: &Exposed, args: &Value) -> bool {
    let (tx, rx) = oneshot::channel::<bool>();
    let shown = clip(&serde_json::to_string_pretty(args).unwrap_or_default(), 700);
    let text = format!(
        "France Student (via le pont MCP) demande de lancer :\n\n{} › {}\n\nArguments :\n{}\n\nAutoriser ? (sans réponse en {} s : refus)",
        e.server,
        e.tool.name,
        shown,
        CONFIRM_WAIT.as_secs()
    );
    app.dialog()
        .message(text)
        .title("FS Code · pont MCP")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::YesNo)
        .show(move |ok| {
            let _ = tx.send(ok);
        });
    matches!(timeout(CONFIRM_WAIT, rx).await, Ok(Ok(true)))
}

/// `tools/call`. `Err` = erreur de protocole ; un échec d'outil est un résultat `isError`.
async fn call_tool(ctx: &Ctx, params: &Value) -> Result<Value, (i64, String)> {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let state = ctx.app.state::<AppState>();
    let b = &state.bridge;

    let found: Option<Exposed> = state
        .mcp
        .exposed()
        .await
        .into_iter()
        .find(|e| public_name(&e.server, &e.tool.name) == name);
    let Some(e) = found else {
        b.note(format!("  ✗ outil inconnu : {name}"));
        return Err((-32602, format!("outil inconnu : {name}")));
    };

    let trusted = state
        .trusted
        .lock()
        .map(|t| t.contains(&e.name))
        .unwrap_or(false);
    let mut allowed = b.auto_allow.load(Ordering::SeqCst) || e.tool.read_only || trusted;
    if !allowed {
        b.note(format!("  ? confirmation demandée : {}", e.name));
        let _one_at_a_time = b.ask.lock().await;
        allowed = confirm(&ctx.app, &e, &args).await;
    }
    if !allowed {
        b.note(format!("  ✗ refusé : {}", e.name));
        return Ok(tool_result(
            "Refusé par l'utilisateur sur l'ordinateur (ou pas de réponse à temps).",
            true,
        ));
    }

    match state.mcp.call_tool(&e.server, &e.tool.name, args).await {
        Ok((text, is_error)) => {
            let mark = if is_error { "✗ erreur de l'outil" } else { "✓ ok" };
            b.note(format!("  {mark} : {}", e.name));
            Ok(tool_result(&clip(&text, MAX_OUT), is_error))
        }
        Err(msg) => {
            b.note(format!("  ✗ échec : {} ({msg})", e.name));
            Ok(tool_result(&msg, true))
        }
    }
}

/// Un message JSON-RPC. `None` = rien à répondre (notification).
async fn handle_rpc(ctx: &Ctx, msg: &Value) -> Option<Value> {
    let method = msg.get("method").and_then(Value::as_str)?;
    if method.starts_with("notifications/") {
        return None;
    }
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let state = ctx.app.state::<AppState>();
    if method != "ping" {
        let label = if method == "tools/call" {
            let n = params.get("name").and_then(Value::as_str).unwrap_or("?");
            format!("tools/call {n}")
        } else {
            method.to_string()
        };
        state.bridge.hit(label);
    }
    let id = msg.get("id").cloned()?;

    let out: Result<Value, (i64, String)> = match method {
        "initialize" => Ok(initialize_result(&params)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": list_tools(ctx).await })),
        "tools/call" => call_tool(ctx, &params).await,
        _ => Err((-32601, format!("méthode non gérée : {method}"))),
    };
    Some(match out {
        Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
        Err((code, m)) => {
            json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": m } })
        }
    })
}

fn json_response(status: StatusCode, v: &Value) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        v.to_string(),
    )
        .into_response()
}

async fn mcp_post(
    AxState(ctx): AxState<Ctx>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !authorized(&ctx, &headers, &query) {
        ctx.app
            .state::<AppState>()
            .bridge
            .note("401 : jeton absent ou faux".to_string());
        return (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Bearer")],
            "unauthorized",
        )
            .into_response();
    }
    let Ok(msg) = serde_json::from_slice::<Value>(&body) else {
        return json_response(
            StatusCode::BAD_REQUEST,
            &json!({ "jsonrpc": "2.0", "id": null,
                     "error": { "code": -32700, "message": "JSON invalide" } }),
        );
    };
    if let Some(batch) = msg.as_array() {
        let mut out = Vec::new();
        for m in batch {
            if let Some(r) = handle_rpc(&ctx, m).await {
                out.push(r);
            }
        }
        if out.is_empty() {
            return StatusCode::ACCEPTED.into_response();
        }
        return json_response(StatusCode::OK, &Value::Array(out));
    }
    match handle_rpc(&ctx, &msg).await {
        Some(r) => json_response(StatusCode::OK, &r),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

/// Lance le serveur sur un port libre de 127.0.0.1.
async fn serve(ctx: Ctx) -> Result<(u16, JoinHandle<()>), String> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| format!("port local indisponible : {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let router = Router::new()
        .route("/mcp", post(mcp_post))
        .route("/health", get(|| async { "ok" }))
        .with_state(ctx);
    let handle = tauri::async_runtime::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    Ok((port, handle))
}

fn new_token() -> Result<String, String> {
    let mut b = [0u8; 24];
    getrandom::getrandom(&mut b).map_err(|e| format!("aléa système indisponible : {e}"))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

// ---------- Tunnel cloudflared ----------

/// Ordre de recherche : binaire embarqué par Tauri, puis PATH/installations système.
fn candidates(app: &AppHandle) -> Vec<String> {
    let mut v = Vec::new();
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    
    // Détection automatique du triplet cible sans avoir besoin de build.rs
    let target = if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        "x86_64-pc-windows-msvc"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "aarch64-apple-darwin"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "x86_64-apple-darwin"
    } else {
        "x86_64-unknown-linux-gnu"
    };

    let bundled = format!("cloudflared-{target}{suffix}");
    let plain = format!("cloudflared{suffix}");

    let mut dirs = Vec::new();
    if let Ok(dir) = app.path().resource_dir() {
        dirs.push(dir.clone());
        dirs.push(dir.join("binaries"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            dirs.push(dir.to_path_buf());
            dirs.push(dir.join("binaries"));
        }
    }
    // En mode `cargo tauri dev`, le binaire source est souvent dans src-tauri/binaries
    dirs.push(std::path::PathBuf::from("binaries"));
    dirs.push(std::path::PathBuf::from("src-tauri/binaries"));

    for dir in dirs {
        for name in [&bundled, &plain] {
            let path = dir.join(name);
            if path.is_file() {
                v.push(path.to_string_lossy().into_owned());
            }
        }
    }
    v.push("cloudflared".to_string());
    #[cfg(windows)]
    {
        v.push(r"C:\Program Files (x86)\cloudflared\cloudflared.exe".to_string());
        v.push(r"C:\Program Files\cloudflared\cloudflared.exe".to_string());
        if let Ok(p) = std::env::var("LOCALAPPDATA") {
            v.push(format!(r"{p}\Microsoft\WinGet\Links\cloudflared.exe"));
        }
    }
    v
}

/// `https://xxxx.trycloudflare.com` trouvée dans une ligne de log (hors `api.trycloudflare.com`).
fn find_tunnel_url(line: &str) -> Option<String> {
    const SUFFIX: &str = ".trycloudflare.com";
    let end = line.find(SUFFIX)? + SUFFIX.len();
    let start = line[..end].rfind("https://")?;
    let url = &line[start..end];
    if url.starts_with("https://api.") {
        return None;
    }
    Some(url.to_string())
}

/// Lit stderr de cloudflared jusqu'à l'URL du tunnel, puis continue de vider le tube.
async fn read_tunnel_url(mut child: Child) -> Result<(Child, String), String> {
    let stderr = child
        .stderr
        .take()
        .ok_or("sortie de cloudflared indisponible")?;
    let mut rd = BufReader::new(stderr);
    let mut tail: Vec<String> = Vec::new();
    let found = timeout(TUNNEL_WAIT, async {
        loop {
            // Octets bruts : pas de panique sur un texte non UTF-8.
            let mut buf = Vec::new();
            match rd.read_until(b'\n', &mut buf).await {
                Ok(0) | Err(_) => return None,
                Ok(_) => {}
            }
            let line = String::from_utf8_lossy(&buf).trim().to_string();
            if let Some(u) = find_tunnel_url(&line) {
                return Some(u);
            }
            if !line.is_empty() {
                tail.push(line);
                if tail.len() > 6 {
                    tail.remove(0);
                }
            }
        }
    })
    .await;
    match found {
        Ok(Some(url)) => {
            // Sans lecteur, le tube se remplit et cloudflared finit par se bloquer.
            tauri::async_runtime::spawn(async move {
                let mut buf = Vec::new();
                loop {
                    buf.clear();
                    match rd.read_until(b'\n', &mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                }
            });
            Ok((child, url))
        }
        Ok(None) => Err(format!(
            "cloudflared s'est arrêté avant d'ouvrir le tunnel : {}",
            tail.join(" | ")
        )),
        Err(_) => Err(format!(
            "cloudflared : pas d'URL de tunnel après {} s : {}",
            TUNNEL_WAIT.as_secs(),
            tail.join(" | ")
        )),
    }
}

async fn start_tunnel(app: &AppHandle, port: u16) -> Result<(Child, String), String> {
    let target = format!("http://127.0.0.1:{port}");
    for exe in candidates(app) {
        let mut cmd = Command::new(&exe);
        // ... le reste de la fonction est inchangé ...
        cmd.args([
            "tunnel",
            "--no-autoupdate",
            "--protocol",
            "http2",
            "--url",
            &target,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
        #[cfg(windows)]
        {
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        match cmd.spawn() {
            Ok(child) => return read_tunnel_url(child).await,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("impossible de lancer cloudflared : {e}")),
        }
    }
    Err("cloudflared introuvable. Il devrait être embarqué avec l'app : lance `npm run fetch-cloudflared` dans fscode/app puis relance (ou installe-le : winget install --id Cloudflare.cloudflared).".to_string())
}

/// Le tunnel répond-il depuis ce PC ? (Le DNS d'un tunnel tout neuf peut mettre quelques secondes.)
async fn wait_reachable(url: &str) -> bool {
    let Ok(http) = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
    else {
        return false;
    };
    let end = Instant::now() + REACH_WAIT;
    while Instant::now() < end {
        if let Ok(r) = http.get(format!("{url}/health")).send().await {
            if r.status().is_success() {
                return true;
            }
        }
        sleep(Duration::from_secs(1)).await;
    }
    false
}

// ---------- Enregistrement chez France Student (/mcp/servers) ----------

/// L'identifiant doit pouvoir être mis dans une URL sans danger.
fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id.chars().any(|c| c != '.')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn pick_id(v: &Value) -> Option<String> {
    for p in ["/id", "/data/id", "/server/id"] {
        match v.pointer(p) {
            Some(Value::String(s)) if safe_id(s) => return Some(s.clone()),
            Some(Value::Number(n)) => return Some(n.to_string()),
            _ => {}
        }
    }
    None
}

/// Liste de serveurs, que la réponse soit un tableau ou un objet qui en contient un.
fn server_list(v: &Value) -> Vec<Value> {
    if let Some(a) = v.as_array() {
        return a.clone();
    }
    for k in ["data", "servers", "items", "results", "mcp_servers"] {
        if let Some(a) = v.get(k).and_then(Value::as_array) {
            return a.clone();
        }
    }
    Vec::new()
}

async fn find_entry(c: &Client, label: &str) -> Result<Option<Value>, String> {
    let v = c
        .get_json("/mcp/servers")
        .await
        .map_err(|e| format!("GET /mcp/servers : {e}"))?;
    let list = server_list(&v);
    Ok(list
        .into_iter()
        .find(|s| s.get("server_label").and_then(Value::as_str) == Some(label)))
}

async fn unregister(c: &Client, id: &str) -> Result<(), String> {
    match c
        .send_json("DELETE", &format!("/mcp/servers/{id}"), None)
        .await
    {
        Ok(_) => Ok(()),
        Err(fs_api::Error::Status { status: 404, .. }) => Ok(()),
        Err(e) => Err(format!("DELETE /mcp/servers/{id} : {e}")),
    }
}

/// Réponse de FS sans la liste (énorme) des outils : on n'en garde que le nombre.
fn summarize(v: &Value, token: &str) -> String {
    let mut o = v.clone();
    if let Some(m) = o.as_object_mut() {
        if let Some(t) = m.remove("available_tools") {
            let n = t.as_array().map(|a| a.len()).unwrap_or(0);
            m.insert("available_tools_count".to_string(), json!(n));
        }
    }
    clip(&o.to_string().replace(token, "***"), 1500)
}

/// Noms des outils que FS a détectés sur notre serveur.
fn tool_names(v: &Value) -> Vec<String> {
    v.get("available_tools")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|t| t.get("name").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Crée (ou met à jour, si l'entrée `label` existe déjà) l'entrée pointant vers le tunnel.
/// Renvoie `(identifiant, réponse de FS tronquée)`.
async fn register(
    c: &Client,
    cfg: &BridgeConfig,
    url: &str,
    token: &str,
) -> Result<(String, String), String> {
    let mut body = json!({
        "server_label": cfg.label,
        "server_url": format!("{url}/mcp?t={token}"),
        "server_description": "FS Code : pont local temporaire",
        "authorization": token,
        "enabled": true,
        // Outils visibles d'emblée par le modèle (pas de chargement à la demande).
        "defer_loading": false,
    });
    if let Some(o) = body.as_object_mut() {
        for (k, v) in &cfg.register_extra {
            o.insert(k.clone(), v.clone());
        }
    }

    let mut existing = find_entry(c, &cfg.label)
        .await
        .ok()
        .flatten()
        .and_then(|v| pick_id(&v));

    let mut last = String::new();
    for attempt in 0..3 {
        if attempt > 0 {
            // FS teste peut-être l'URL à l'enregistrement : le tunnel met parfois du temps à s'établir.
            sleep(Duration::from_secs(4)).await;
        }
        let res = match &existing {
            Some(id) => {
                c.send_json("POST", &format!("/mcp/servers/{id}"), Some(body.clone()))
                    .await
            }
            None => {
                c.send_json("POST", "/mcp/servers", Some(body.clone()))
                    .await
            }
        };
        match res {
            Ok(v) => {
                let mut id = pick_id(&v).or_else(|| existing.clone());
                if id.is_none() {
                    id = find_entry(c, &cfg.label)
                        .await
                        .ok()
                        .flatten()
                        .and_then(|e| pick_id(&e));
                }
                let id = id.ok_or("entrée créée, mais identifiant introuvable dans la réponse")?;
                let names = tool_names(&v);
                let mut note = format!("POST → {}", summarize(&v, token));
                // `allowed_tools: []` est ambigu (tous ? aucun ?) : on liste alors explicitement les outils vus par FS.
                let empty = v
                    .get("allowed_tools")
                    .and_then(Value::as_array)
                    .map(|a| a.is_empty())
                    .unwrap_or(false);
                if empty && !names.is_empty() {
                    let mut upd = body.clone();
                    upd["allowed_tools"] = json!(names);
                    match c
                        .send_json("POST", &format!("/mcp/servers/{id}"), Some(upd))
                        .await
                    {
                        Ok(v2) => note.push_str(&format!(
                            "\nallowed_tools explicite ({} outils) → {}",
                            names.len(),
                            summarize(&v2, token)
                        )),
                        Err(e) => note.push_str(&format!("\nallowed_tools explicite refusé : {e}")),
                    }
                }
                return Ok((id, note));
            }
            Err(fs_api::Error::Status { status: 404, .. }) if existing.is_some() => {
                existing = None; // l'entrée a disparu côté FS : on la recrée
                last = "entrée introuvable (404), recréation".to_string();
            }
            Err(e) => last = e.to_string(),
        }
    }
    Err(format!("enregistrement chez France Student refusé : {last}"))
}

/// Entrée telle que FS la voit (outils détectés, état OAuth…), jeton masqué.
async fn entry_note(c: &Client, label: &str, token: &str) -> String {
    match find_entry(c, label).await {
        Ok(Some(v)) => format!(
            "GET → {}",
            summarize(&v, token)
        ),
        Ok(None) => {
            let shape = match c.get_json("/mcp/servers").await {
                Ok(v) => {
                    let keys = v
                        .as_object()
                        .map(|o| o.keys().cloned().collect::<Vec<_>>().join(","))
                        .unwrap_or_else(|| "tableau".to_string());
                    let s = if token.is_empty() { v.to_string() } else { v.to_string().replace(token, "***") };
                    let cut: String = s.chars().take(300).collect();
                    format!("clés [{keys}], {} entrées lues, début : {cut}", server_list(&v).len())
                }
                Err(e) => e.to_string(),
            };
            format!("GET /mcp/servers : entrée « {label} » introuvable ({shape})")
        }
        Err(e) => e,
    }
}

// ---------- Cycle de vie ----------

/// Connecte les serveurs de `mcp.json` qui ne le sont pas déjà. Renvoie les avertissements.
async fn connect_all(app: &AppHandle, state: &State<'_, AppState>) -> Result<Vec<String>, String> {
    let cfg = mcp::load_config(&config_dir(app)?)?;
    let mut warnings = Vec::new();
    for (name, server) in &cfg {
        let up = state
            .mcp_up
            .lock()
            .map(|u| u.contains(name))
            .unwrap_or(false);
        if up {
            continue;
        }
        state.bridge.step(&format!("connexion de « {name} »"));
        match state.mcp.connect(name, server).await {
            Ok(_) => {
                if let Ok(mut u) = state.mcp_up.lock() {
                    u.insert(name.clone());
                }
            }
            Err(e) => warnings.push(format!("{name} : {e}")),
        }
    }
    Ok(warnings)
}

async fn run_start(
    app: &AppHandle,
    state: &State<'_, AppState>,
    cfg: &BridgeConfig,
    live: &mut Live,
) -> Result<(), String> {
    let b = &state.bridge;
    let c = crate::client(app, state).await?; // clé API requise

    let mut warnings = connect_all(app, state).await?;
    let exposed = state.mcp.exposed().await;
    if exposed.is_empty() {
        return Err("aucun outil MCP disponible : ajoute un serveur dans l'onglet MCP (mcp.json) et vérifie qu'il démarre.".to_string());
    }
    let n = exposed.len();
    let mut servers: Vec<String> = exposed.iter().map(|e| e.server.clone()).collect();
    servers.sort();
    servers.dedup();
    {
        let (s2, w2) = (servers.clone(), warnings.clone());
        b.set(|s| {
            s.tools = n;
            s.servers = s2;
            s.warnings = w2;
        });
    }

    b.step("serveur local");
    let token = new_token()?;
    let ctx = Ctx {
        app: app.clone(),
        token: Arc::from(token.as_str()),
    };
    let (port, handle) = serve(ctx).await?;
    live.server = Some(handle);
    b.set(|s| s.port = Some(port));

    b.step("tunnel cloudflared");
    let (child, url) = start_tunnel(app, port).await?;
    live.tunnel = Some(child);
    {
        let u2 = url.clone();
        b.set(|s| s.url = Some(u2));
    }

    b.step("attente du tunnel");
    if !wait_reachable(&url).await {
        warnings.push(format!(
            "tunnel pas joignable depuis ce PC après {} s (DNS/réseau ?) : enregistré quand même",
            REACH_WAIT.as_secs()
        ));
        let w2 = warnings.clone();
        b.set(|s| s.warnings = w2);
    }

    b.step("enregistrement chez France Student");
    let (id, note) = register(&c, cfg, &url, &token).await?;
    live.server_id = Some(id.clone());
    let extra = entry_note(&c, &cfg.label, &token).await;
    b.set(|s| {
        s.server_id = Some(id);
        s.register_note = Some(format!("{note}\n{extra}"));
        s.phase = Phase::Running;
        s.step = String::new();
    });
    b.note(format!("pont actif : {n} outils via {url}"));
    Ok(())
}

/// Retire l'entrée FS, coupe le serveur local et le tunnel. Renvoie un avertissement éventuel.
async fn teardown(
    app: &AppHandle,
    state: &State<'_, AppState>,
    live: &mut Live,
) -> Option<String> {
    let mut warn = None;
    if let Some(id) = live.server_id.take() {
        match crate::client(app, state).await {
            Ok(c) => {
                if let Err(e) = unregister(&c, &id).await {
                    warn = Some(format!("entrée FS non retirée : {e}"));
                }
            }
            Err(e) => warn = Some(format!("entrée FS non retirée : {e}")),
        }
    }
    if let Some(h) = live.server.take() {
        h.abort();
    }
    if let Some(mut t) = live.tunnel.take() {
        let _ = t.kill().await;
    }
    warn
}

/// Supprime l'entrée `label` restée chez FS après un arrêt brutal (rien n'est lancé).
async fn purge_entry(app: &AppHandle, state: &State<'_, AppState>) -> Option<String> {
    let c = match crate::client(app, state).await {
        Ok(c) => c,
        Err(e) => return Some(e),
    };
    let dir = config_dir(app).ok()?;
    let cfg = load_cfg(&dir).unwrap_or_default();
    match find_entry(&c, &cfg.label).await {
        Ok(Some(v)) => match pick_id(&v) {
            Some(id) if safe_id(&id) => unregister(&c, &id).await.err(),
            _ => None,
        },
        Ok(None) => None,
        Err(e) => Some(e),
    }
}

pub async fn start(app: &AppHandle) -> Result<Snapshot, String> {
    let state = app.state::<AppState>();
    let b = &state.bridge;
    let _op = b.op.lock().await;
    if b.live.lock().await.is_some() {
        return Ok(b.snapshot()); // déjà lancé
    }

    b.reset(Phase::Starting, "lecture de bridge.json");
    let cfg = match config_dir(app).and_then(|d| load_cfg(&d)) {
        Ok(c) => c,
        Err(e) => {
            b.set(|s| {
                s.phase = Phase::Error;
                s.step = String::new();
                s.error = Some(e.clone());
            });
            return Err(e);
        }
    };
    b.auto_allow.store(cfg.auto_allow, Ordering::SeqCst);

    let mut live = Live {
        server: None,
        tunnel: None,
        server_id: None,
    };
    match run_start(app, &state, &cfg, &mut live).await {
        Ok(()) => {
            *b.live.lock().await = Some(live);
            Ok(b.snapshot())
        }
        Err(e) => {
            let warn = teardown(app, &state, &mut live).await;
            b.set(|s| {
                s.phase = Phase::Error;
                s.step = String::new();
                s.error = Some(e.clone());
                s.url = None;
                s.port = None;
                if let Some(w) = warn {
                    s.warnings.push(w);
                }
                push_log(s, format!("échec du démarrage : {e}"));
            });
            Err(e)
        }
    }
}

/// `purge` : même sans pont lancé, supprime l'entrée périmée restée chez FS.
pub async fn stop(app: &AppHandle, purge: bool) -> Snapshot {
    let state = app.state::<AppState>();
    let b = &state.bridge;
    let _op = b.op.lock().await;
    let taken = b.live.lock().await.take();
    let was_running = taken.is_some();
    let warn = match taken {
        Some(mut live) => teardown(app, &state, &mut live).await,
        None if purge => purge_entry(app, &state).await,
        None => None,
    };
    b.reset(Phase::Stopped, "");
    b.set(|s| {
        if was_running {
            push_log(s, "pont arrêté".to_string());
        }
        if let Some(w) = warn {
            s.warnings.push(w);
        }
    });
    b.snapshot()
}

/// Appelé au lancement de l'app : démarre le pont si `auto_start` est activé.
pub fn autostart(app: &AppHandle) {
    let Ok(dir) = config_dir(app) else {
        return;
    };
    match load_cfg(&dir) {
        Ok(c) if c.auto_start => {}
        _ => return,
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = start(&app).await;
    });
}

/// Appelé à la fermeture normale de l'app (`RunEvent::Exit`) : retire l'entrée FS, tue le tunnel.
pub fn shutdown(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::block_on(async move {
        let _ = timeout(SHUTDOWN_WAIT, stop(&app, false)).await;
    });
}

fn status(app: &AppHandle) -> Snapshot {
    let state = app.state::<AppState>();
    let mut s = state.bridge.snapshot();
    if let Ok(dir) = config_dir(app) {
        let c = load_cfg(&dir).unwrap_or_default();
        s.auto_start = c.auto_start;
        s.auto_allow = c.auto_allow;
    }
    s
}

// ---------- Commandes Tauri ----------

/// Démarre le pont (peut durer jusqu'à ~2 min : serveurs MCP, tunnel, enregistrement).
#[tauri::command]
pub async fn bridge_start(app: AppHandle) -> Result<Snapshot, String> {
    start(&app).await
}

/// Arrête le pont ; s'il n'est pas lancé, supprime l'entrée périmée restée chez FS.
#[tauri::command]
pub async fn bridge_stop(app: AppHandle) -> Result<Snapshot, String> {
    Ok(stop(&app, true).await)
}

#[tauri::command]
pub fn bridge_status(app: AppHandle) -> Snapshot {
    status(&app)
}

/// Seuls les champs fournis changent. `auto_allow` agit tout de suite sur le pont en cours.
#[tauri::command]
pub fn bridge_set(
    app: AppHandle,
    auto_start: Option<bool>,
    auto_allow: Option<bool>,
) -> Result<Snapshot, String> {
    let dir = config_dir(&app)?;
    let mut c = load_cfg(&dir)?;
    if let Some(v) = auto_start {
        c.auto_start = v;
    }
    if let Some(v) = auto_allow {
        c.auto_allow = v;
        app.state::<AppState>()
            .bridge
            .auto_allow
            .store(v, Ordering::SeqCst);
    }
    save_cfg(&dir, &c)?;
    Ok(status(&app))
}
