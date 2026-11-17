//! Pont MCP : expose les serveurs MCP locaux (ceux de `mcp.json`) au serveur France
//! Student via un gateway HTTP local et **un seul** tunnel `cloudflared`.
//!
//! Topologie :
//!
//! ```text
//! cloudflared (1 processus)
//!   └─ https://xxx.trycloudflare.com
//!        ├─ /mcp/github      → serveur MCP « github »      (jeton A)
//!        ├─ /mcp/filesystem  → serveur MCP « filesystem »  (jeton B)
//!        └─ /mcp             → agrégat de tous             (jeton global)
//! ```
//!
//! C'est le serveur France Student qui exécute les outils (`POST /mcp/servers`). FS Code
//! ne pilote aucune boucle d'outils ici : il répond à des requêtes JSON-RPC.
//!
//! Sécurité :
//! - écoute sur `127.0.0.1` uniquement, port aléatoire ;
//! - un jeton aléatoire par entrée, comparé en temps constant, **jamais dans l'URL** ;
//! - corps limité à 1 Mio, délais et concurrence bornés ;
//! - confirmation native avant chaque outil (par défaut), refus si pas de réponse en 120 s ;
//! - jetons masqués dans le journal.
//!
//! Réglages dans `bridge.json` (dossier de config de l'app), voir `BridgeConfig`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path as UrlPath, State as AxState};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use fs_api::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{oneshot, Semaphore};
use tokio::time::{sleep, timeout, Instant};

use crate::mcp::{self, Exposed};
use crate::{clip, config_dir, AppState};

const CONFIG_FILE: &str = "bridge.json";
const PID_FILE: &str = "bridge.pid";
const DEFAULT_PREFIX: &str = "fscode";
const DEFAULT_TOKEN_REF: &str = "cloudflared-tunnel";
const ACCESS_ID_REF: &str = "cloudflared-access-id";
const ACCESS_SECRET_REF: &str = "cloudflared-access-secret";
const TUNNEL_WAIT: Duration = Duration::from_secs(45);
const REACH_WAIT: Duration = Duration::from_secs(25);
const CONFIRM_WAIT: Duration = Duration::from_secs(120);
/// Délai maximal du nettoyage à la fermeture.
pub const SHUTDOWN_WAIT: Duration = Duration::from_secs(8);
/// Corps HTTP maximum (1 Mio).
const BODY_LIMIT: usize = 1024 * 1024;
/// Sortie d'outil renvoyée au serveur France Student : plafond en caractères.
const MAX_OUT: usize = 50_000;
/// Lignes gardées dans le journal affiché par l'interface.
const LOG_MAX: usize = 30;
/// Requêtes simultanées par serveur et au total.
const PER_SERVER_CONC: usize = 4;
const GLOBAL_CONC: usize = 8;
/// Versions du protocole MCP gérées (la plus récente d'abord).
const PROTOCOLS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

// ---------- Configuration (bridge.json) ----------

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
struct BridgeConfig {
    /// `quick` (trycloudflare) ou `named` (domaine Cloudflare + jeton dans le coffre).
    mode: String,
    /// Préfixe des `server_label` créés chez France Student (`fscode-<serveur>`).
    #[serde(alias = "label")]
    label_prefix: String,
    /// Démarre le pont au lancement de l'app.
    auto_start: bool,
    /// Chemin explicite de `cloudflared` (prioritaire).
    cloudflared_path: Option<String>,
    /// URL publique en mode `named` (obligatoire).
    public_url: Option<String>,
    /// Compte du coffre contenant le jeton du tunnel nommé.
    named_tunnel_token_ref: String,
    /// Comptes du coffre pour Cloudflare Access (optionnel).
    access_client_id_ref: String,
    access_client_secret_ref: String,
    /// Champs ajoutés au corps de `POST /mcp/servers` (hors champs de sécurité).
    register_extra: Map<String, Value>,
    /// Nombre maximal d'outils exposés par serveur.
    max_tools_per_entry: usize,
    /// Confirmer aussi les outils marqués « lecture seule ».
    confirm_read_only: bool,
    /// Délai JSON-RPC de base (les `tools/call` ont au moins 120 s).
    timeout_seconds: u64,
    /// Ancien réglage : jamais restauré (session uniquement).
    #[serde(skip_serializing)]
    auto_allow: bool,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            mode: "quick".to_string(),
            label_prefix: DEFAULT_PREFIX.to_string(),
            auto_start: false,
            cloudflared_path: None,
            public_url: None,
            named_tunnel_token_ref: DEFAULT_TOKEN_REF.to_string(),
            access_client_id_ref: ACCESS_ID_REF.to_string(),
            access_client_secret_ref: ACCESS_SECRET_REF.to_string(),
            register_extra: Map::new(),
            max_tools_per_entry: 64,
            confirm_read_only: true,
            timeout_seconds: 60,
            auto_allow: false,
        }
    }
}

fn normalize_cfg(mut c: BridgeConfig) -> Result<BridgeConfig, String> {
    c.mode = c.mode.trim().to_ascii_lowercase();
    if c.mode.is_empty() {
        c.mode = "quick".to_string();
    }
    if c.mode != "quick" && c.mode != "named" {
        return Err(format!("mode de tunnel inconnu : « {} » (quick ou named)", c.mode));
    }
    c.label_prefix = safe(c.label_prefix.trim().trim_matches('-'));
    if c.label_prefix.trim_matches('_').is_empty() {
        c.label_prefix = DEFAULT_PREFIX.to_string();
    }
    c.max_tools_per_entry = c.max_tools_per_entry.clamp(1, 256);
    c.timeout_seconds = c.timeout_seconds.clamp(10, 600);
    if c.named_tunnel_token_ref.trim().is_empty() {
        c.named_tunnel_token_ref = DEFAULT_TOKEN_REF.to_string();
    }
    if c.access_client_id_ref.trim().is_empty() {
        c.access_client_id_ref = ACCESS_ID_REF.to_string();
    }
    if c.access_client_secret_ref.trim().is_empty() {
        c.access_client_secret_ref = ACCESS_SECRET_REF.to_string();
    }
    // Le mode « tout autoriser » est une décision de session, jamais relue du disque.
    c.auto_allow = false;
    Ok(c)
}

/// Fichier absent => réglages par défaut ; fichier invalide => erreur (jamais écrasé en silence).
fn load_cfg(dir: &std::path::Path) -> Result<BridgeConfig, String> {
    let Ok(text) = std::fs::read_to_string(dir.join(CONFIG_FILE)) else {
        return Ok(BridgeConfig::default());
    };
    let c: BridgeConfig =
        serde_json::from_str(&text).map_err(|e| format!("bridge.json invalide : {e}"))?;
    normalize_cfg(c)
}

fn save_cfg(dir: &std::path::Path, cfg: &BridgeConfig) -> Result<(), String> {
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

/// Une entrée `/mcp/<serveur>` enregistrée chez France Student.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryView {
    server: String,
    label: String,
    url: String,
    server_id: Option<String>,
    tools: usize,
    error: Option<String>,
}

/// Photo de l'état, renvoyée à l'interface.
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    phase: Phase,
    /// Étape en cours pendant le démarrage.
    step: String,
    mode: String,
    /// URL publique du tunnel (sans jeton).
    url: Option<String>,
    port: Option<u16>,
    tools: usize,
    servers: Vec<String>,
    entries: Vec<EntryView>,
    error: Option<String>,
    warnings: Vec<String>,
    /// Réponses de France Student à l'enregistrement (jetons masqués).
    register_note: Option<String>,
    /// Requêtes reçues du serveur FS depuis le lancement.
    hits: u64,
    log: Vec<String>,
    auto_allow: bool,
    auto_start: bool,
    confirm_read_only: bool,
    max_tools: usize,
    /// Le jeton du tunnel nommé est présent dans le coffre.
    named_ready: bool,
}

/// Entrée vivante à retirer de France Student à l'arrêt.
struct LiveEntry {
    server_id: String,
    #[allow(dead_code)]
    label: String,
}

/// Ressources vivantes du pont.
struct Live {
    server: Option<JoinHandle<()>>,
    tunnel: Option<Child>,
    entries: Vec<LiveEntry>,
    public_url: String,
    pid_file: PathBuf,
}

/// État partagé du pont (champ `bridge` de `AppState`).
#[derive(Default)]
pub struct Bridge {
    /// Sérialise démarrage et arrêt.
    op: tokio::sync::Mutex<()>,
    live: tokio::sync::Mutex<Option<Live>>,
    /// Une seule confirmation à l'écran à la fois.
    ask: tokio::sync::Mutex<()>,
    /// Confirmation en attente de réponse de l'interface (identifiant, canal).
    pending: Mutex<Option<(u64, oneshot::Sender<bool>)>>,
    next_id: AtomicU64,
    /// Jamais tenu pendant un `await`.
    snap: Mutex<Snapshot>,
    /// Session uniquement : jamais enregistré sur disque.
    auto_allow: AtomicBool,
    /// Confirmation des outils « lecture seule ».
    confirm_read_only: AtomicBool,
    /// Jetons à masquer dans le journal.
    tokens: Mutex<Vec<String>>,
}

fn push_log(s: &mut Snapshot, text: String) {
    s.log.push(text);
    if s.log.len() > LOG_MAX {
        s.log.remove(0);
    }
}

fn mask_secrets(text: &str, tokens: &[String]) -> String {
    let mut out = text.to_string();
    for t in tokens {
        if t.len() >= 8 {
            out = out.replace(t, "***");
        }
    }
    out
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

    /// Journalise en masquant tous les jetons connus.
    fn note(&self, text: String) {
        let tokens = self.tokens.lock().map(|t| t.clone()).unwrap_or_default();
        let text = mask_secrets(&text, &tokens);
        self.set(|s| push_log(s, text));
    }

    /// Une requête du serveur FS (comptée et journalisée).
    fn hit(&self, text: String) {
        self.set(|s| s.hits += 1);
        self.note(text);
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
        if let Ok(mut t) = self.tokens.lock() {
            t.clear();
        }
    }
}

// ---------- Gateway MCP local ----------

#[derive(Clone, Debug)]
struct RawTool {
    name: String,
    description: String,
    input_schema: Value,
    read_only: bool,
}

#[derive(Clone, Debug)]
struct PublicTool {
    public: String,
    raw: String,
    description: String,
    input_schema: Value,
    read_only: bool,
}

struct ServerEntry {
    server: String,
    route: String,
    token: String,
    tools: Vec<PublicTool>,
}

struct AggTool {
    server: String,
    tool: PublicTool,
}

#[derive(Debug)]
struct ToolOut {
    text: String,
    is_error: bool,
}

/// Ce que le gateway demande à l'application : exécution (avec confirmation) d'un outil.
#[async_trait]
trait Host: Send + Sync {
    async fn call(&self, server: &str, tool: &str, read_only: bool, args: Value) -> ToolOut;
    fn note(&self, line: String);
    fn hit(&self, line: String);
}

struct Gateway {
    servers: Vec<ServerEntry>,
    aggregate: Vec<AggTool>,
    global: String,
    host: Arc<dyn Host>,
    timeout: Duration,
    call_timeout: Duration,
    global_sem: Arc<Semaphore>,
    per_server: HashMap<String, Arc<Semaphore>>,
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

fn truncate_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Hash FNV-1a court et stable (suffixe anti-collision).
fn hash4(seed: &str) -> String {
    let mut h: u32 = 0x811c_9dc5;
    for b in seed.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    format!("{:04x}", h & 0xffff)
}

/// Nom unique dans un espace donné ; suffixe de hash stable en cas de collision.
fn unique_name(base: &str, seed: &str, used: &mut HashSet<String>) -> String {
    if used.insert(base.to_string()) {
        return base.to_string();
    }
    let stem = truncate_chars(base, 52);
    for i in 0..64 {
        let cand = if i == 0 {
            format!("{stem}__{}", hash4(seed))
        } else {
            format!("{stem}__{}_{i}", hash4(seed))
        };
        if used.insert(cand.clone()) {
            return cand;
        }
    }
    let mut n = base.to_string();
    while !used.insert(n.clone()) {
        n.push('_');
    }
    n
}

fn unique_route(server: &str, used: &mut HashSet<String>) -> String {
    let base = {
        let s = truncate_chars(&safe(server), 40);
        if s.trim_matches('_').is_empty() {
            "server".to_string()
        } else {
            s
        }
    };
    if used.insert(base.clone()) {
        return base;
    }
    for i in 0..64 {
        let cand = if i == 0 {
            format!("{base}-{}", hash4(server))
        } else {
            format!("{base}-{}_{i}", hash4(server))
        };
        if used.insert(cand.clone()) {
            return cand;
        }
    }
    format!("{base}-{}", used.len())
}

/// Un jeton aléatoire (48 caractères hexadécimaux).
fn new_token() -> Result<String, String> {
    let mut b = [0u8; 24];
    getrandom::getrandom(&mut b).map_err(|e| format!("aléa système indisponible : {e}"))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// `Authorization: <jeton>` ou `Authorization: Bearer <jeton>` ; comparé en temps constant.
fn authorized(headers: &HeaderMap, want: &str) -> bool {
    let Some(raw) = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let raw = raw.trim();
    let low = raw.to_ascii_lowercase();
    // France Student envoie le jeton tel quel (auth_type manual) ; on accepte aussi `Bearer <jeton>`.
    let got = if low.starts_with("bearer ") { raw[7..].trim() } else { raw };
    ct_eq(got.as_bytes(), want.as_bytes())
}

fn rpc_http(status: StatusCode, value: Value) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        value.to_string(),
    )
        .into_response()
}

fn error_value(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

fn result_value(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
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

fn tool_json(t: &PublicTool) -> Value {
    let mut v = json!({
        "name": t.public,
        "description": t.description,
        "inputSchema": t.input_schema,
    });
    if t.read_only {
        v["annotations"] = json!({ "readOnlyHint": true });
    }
    v
}

impl Gateway {
    fn router(self: &Arc<Self>) -> Router {
        Router::new()
            .route("/mcp", post(mcp_root))
            .route("/mcp/{server}", post(mcp_route))
            .route("/health", get(health))
            .layer(DefaultBodyLimit::max(BODY_LIMIT))
            .with_state(self.clone())
    }

    fn tools_value(&self, entry: Option<usize>) -> Value {
        let tools: Vec<Value> = match entry {
            Some(i) => self.servers[i].tools.iter().map(tool_json).collect(),
            None => self.aggregate.iter().map(|a| tool_json(&a.tool)).collect(),
        };
        json!({ "tools": tools })
    }

    fn resolve<'a>(
        &'a self,
        entry: Option<usize>,
        name: &str,
    ) -> Result<(&'a str, &'a PublicTool), (i64, String)> {
        match entry {
            Some(i) => {
                let e = &self.servers[i];
                let t = e
                    .tools
                    .iter()
                    .find(|t| t.public == name)
                    .ok_or_else(|| (-32602, format!("outil inconnu : {name}")))?;
                Ok((e.server.as_str(), t))
            }
            None => {
                let a = self
                    .aggregate
                    .iter()
                    .find(|a| a.tool.public == name)
                    .ok_or_else(|| (-32602, format!("outil inconnu : {name}")))?;
                Ok((a.server.as_str(), &a.tool))
            }
        }
    }
}

async fn health() -> &'static str {
    "ok"
}

async fn mcp_root(
    AxState(gw): AxState<Arc<Gateway>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !authorized(&headers, &gw.global) {
        gw.host.note("401 : jeton absent ou faux".to_string());
        return rpc_http(
            StatusCode::UNAUTHORIZED,
            error_value(Value::Null, -32001, "unauthorized"),
        );
    }
    process(gw, None, body).await
}

async fn mcp_route(
    AxState(gw): AxState<Arc<Gateway>>,
    UrlPath(route): UrlPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(idx) = gw.servers.iter().position(|s| s.route == route) else {
        gw.host
            .note(format!("404 : entrée inconnue « {route} »"));
        return rpc_http(
            StatusCode::NOT_FOUND,
            error_value(Value::Null, -32601, "route inconnue"),
        );
    };
    if !authorized(&headers, &gw.servers[idx].token) {
        gw.host.note("401 : jeton absent ou faux".to_string());
        return rpc_http(
            StatusCode::UNAUTHORIZED,
            error_value(Value::Null, -32001, "unauthorized"),
        );
    }
    process(gw, Some(idx), body).await
}

async fn process(gw: Arc<Gateway>, entry: Option<usize>, body: Bytes) -> Response {
    let Ok(msg) = serde_json::from_slice::<Value>(&body) else {
        return rpc_http(
            StatusCode::BAD_REQUEST,
            error_value(Value::Null, -32700, "JSON invalide"),
        );
    };
    if let Some(batch) = msg.as_array() {
        if batch.is_empty() {
            return rpc_http(
                StatusCode::BAD_REQUEST,
                error_value(Value::Null, -32600, "lot JSON-RPC vide"),
            );
        }
        let mut out = Vec::new();
        for m in batch {
            if let Some(r) = handle_msg(&gw, entry, m).await {
                out.push(r);
            }
        }
        if out.is_empty() {
            return StatusCode::ACCEPTED.into_response();
        }
        return rpc_http(StatusCode::OK, Value::Array(out));
    }
    match handle_msg(&gw, entry, &msg).await {
        Some(r) => rpc_http(StatusCode::OK, r),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

async fn handle_msg(gw: &Gateway, entry: Option<usize>, msg: &Value) -> Option<Value> {
    let Some(obj) = msg.as_object() else {
        return Some(error_value(Value::Null, -32600, "requête invalide"));
    };
    let id = obj.get("id").cloned();
    if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Some(error_value(
            id.unwrap_or(Value::Null),
            -32600,
            "jsonrpc doit valoir « 2.0 »",
        ));
    }
    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        return Some(error_value(
            id.unwrap_or(Value::Null),
            -32600,
            "méthode absente",
        ));
    };
    // Notification : aucune réponse, aucune exécution (le pont ne fait que répondre).
    let id = id?;
    let params = obj.get("params").cloned().unwrap_or(Value::Null);
    if method != "ping" {
        let label = if method == "tools/call" {
            let n = params.get("name").and_then(Value::as_str).unwrap_or("?");
            format!("tools/call {n}")
        } else {
            method.to_string()
        };
        gw.host.hit(label);
    }
    let dur = if method == "tools/call" {
        gw.call_timeout
    } else {
        gw.timeout
    };
    match timeout(dur, dispatch(gw, entry, method, &params)).await {
        Ok(Ok(r)) => Some(result_value(id, r)),
        Ok(Err((code, message))) => Some(error_value(id, code, &message)),
        Err(_) => Some(error_value(id, -32603, "délai dépassé")),
    }
}

async fn dispatch(
    gw: &Gateway,
    entry: Option<usize>,
    method: &str,
    params: &Value,
) -> Result<Value, (i64, String)> {
    match method {
        "initialize" => Ok(initialize_result(params)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(gw.tools_value(entry)),
        "tools/call" => call_tool(gw, entry, params).await,
        _ => Err((-32601, format!("méthode non gérée : {method}"))),
    }
}

async fn call_tool(
    gw: &Gateway,
    entry: Option<usize>,
    params: &Value,
) -> Result<Value, (i64, String)> {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return Err((-32602, "paramètre « name » manquant".to_string()));
    };
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if !args.is_object() {
        return Err((-32602, "« arguments » doit être un objet".to_string()));
    }
    let (server, tool) = gw.resolve(entry, name)?;

    let sem = gw
        .per_server
        .get(server)
        .cloned()
        .unwrap_or_else(|| gw.global_sem.clone());
    let Ok(Ok(_per_server)) = timeout(gw.call_timeout, sem.acquire()).await else {
        return Err((-32603, "trop de requêtes simultanées".to_string()));
    };
    let Ok(Ok(_global)) = timeout(gw.call_timeout, gw.global_sem.acquire()).await else {
        return Err((-32603, "trop de requêtes simultanées".to_string()));
    };

    let out = gw
        .host
        .call(server, &tool.raw, tool.read_only, args)
        .await;
    Ok(tool_result(&out.text, out.is_error))
}

// ---------- Construction des noms publics ----------

struct Plan {
    server: String,
    route: String,
    token: String,
    label: String,
    tools: Vec<PublicTool>,
}

fn build_plans(exposed: Vec<Exposed>, cfg: &BridgeConfig) -> (Vec<Plan>, Vec<AggTool>) {
    let mut by_server: BTreeMap<String, Vec<RawTool>> = BTreeMap::new();
    for e in exposed {
        by_server.entry(e.server).or_default().push(RawTool {
            name: e.tool.name,
            description: e.tool.description,
            input_schema: e.tool.input_schema,
            read_only: e.tool.read_only,
        });
    }

    let mut plans = Vec::new();
    let mut used_routes = HashSet::new();
    let mut used_labels = HashSet::new();
    for (server, mut tools) in by_server {
        tools.sort_by(|a, b| a.name.cmp(&b.name));
        tools.truncate(cfg.max_tools_per_entry);
        let mut used = HashSet::new();
        let public_tools = tools
            .into_iter()
            .map(|t| {
                let base = {
                    let s = truncate_chars(&safe(&t.name), 60);
                    if s.trim_matches('_').is_empty() {
                        "tool".to_string()
                    } else {
                        s
                    }
                };
                let public = unique_name(&base, &format!("{}:{}", server, t.name), &mut used);
                PublicTool {
                    public,
                    raw: t.name,
                    description: clip(&format!("[{}] {}", server, t.description), 1500),
                    input_schema: t.input_schema,
                    read_only: t.read_only,
                }
            })
            .collect();
        let route = unique_route(&server, &mut used_routes);
        let label = unique_name(
            &format!("{}-{}", cfg.label_prefix, safe(&server)),
            &format!("label:{}", server),
            &mut used_labels,
        );
        plans.push(Plan {
            server,
            route,
            token: String::new(),
            label,
            tools: public_tools,
        });
    }

    let mut aggregate = Vec::new();
    let mut used_agg = HashSet::new();
    for p in &plans {
        for t in &p.tools {
            let base = {
                let s = truncate_chars(&format!("{}__{}", safe(&p.server), safe(&t.raw)), 56);
                if s.trim_matches('_').is_empty() {
                    "tool".to_string()
                } else {
                    s
                }
            };
            let public = unique_name(
                &base,
                &format!("agg:{}:{}", p.server, t.raw),
                &mut used_agg,
            );
            aggregate.push(AggTool {
                server: p.server.clone(),
                tool: PublicTool {
                    public,
                    ..t.clone()
                },
            });
        }
    }
    (plans, aggregate)
}

fn should_defer(tools: &[PublicTool]) -> bool {
    tools.len() > 40 || tools.iter().map(|t| t.description.chars().count()).sum::<usize>() > 12_000
}

// ---------- Application : exécution et confirmation ----------

struct AppHost {
    app: AppHandle,
}

/// Demande l'autorisation dans l'interface de l'app (modale au-dessus du chat). Sans réponse à temps => refus.
async fn confirm(app: &AppHandle, e: &Exposed, args: &Value) -> bool {
    let state = app.state::<AppState>();
    let b = &state.bridge;
    let (tx, rx) = oneshot::channel::<bool>();
    let id = b.next_id.fetch_add(1, Ordering::SeqCst) + 1;
    if let Ok(mut p) = b.pending.lock() {
        *p = Some((id, tx));
    }
    let tokens = b.tokens.lock().map(|t| t.clone()).unwrap_or_default();
    let shown = mask_secrets(&serde_json::to_string_pretty(args).unwrap_or_default(), &tokens);
    let payload = json!({
 "id": id,
 "server": e.server,
        "tool": e.tool.name,
 "readOnly": e.tool.read_only,
 "args": clip(&shown, 2000),
        "waitSecs": CONFIRM_WAIT.as_secs(),
    });
    // Ramène la fenêtre au premier plan : la demande vient de France Student, pas de l'utilisateur.
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
 let _ = w.unminimize();
        let _ = w.set_focus();
    }
    let ok = if app.emit("bridge:confirm", payload).is_ok() {
        matches!(timeout(CONFIRM_WAIT, rx).await, Ok(Ok(true)))
    } else {
 false
    };
    if let Ok(mut p) = b.pending.lock() {
        if matches!(&*p, Some((i, _)) if *i == id) {
     *p = None;
        }
    }
    let _ = app.emit("bridge:confirm-close", json!({ "id": id }));
    ok
}

/// Réponse de la modale (Autoriser / Refuser). `false` si la demande a expiré.
#[tauri::command]
pub fn bridge_confirm_answer(app: AppHandle, id: u64, allow: bool) -> bool {
    let state = app.state::<AppState>();
    let tx = match state.bridge.pending.lock() {
        Ok(mut p) => match p.take() {
     Some((i, tx)) if i == id => Some(tx),
     other => {
  *p = other;
         None
     }
        },
        Err(_) => None,
    };
    tx.map(|tx| tx.send(allow).is_ok()).unwrap_or(false)
}
#[async_trait]
impl Host for AppHost {
    async fn call(&self, server: &str, tool: &str, read_only: bool, args: Value) -> ToolOut {
        let state = self.app.state::<AppState>();
        let b = &state.bridge;
        let found: Option<Exposed> = state
            .mcp
            .exposed()
            .await
            .into_iter()
            .find(|e| e.server == server && e.tool.name == tool);
        let Some(e) = found else {
            b.note(format!("✗ outil inconnu : {server} › {tool}"));
            return ToolOut {
                text: format!("outil inconnu : {tool}"),
                is_error: true,
            };
        };

        let trusted = state
            .trusted
            .lock()
            .map(|t| t.contains(&e.name))
            .unwrap_or(false);
        let skip = b.auto_allow.load(Ordering::SeqCst)
            || trusted
            || (!b.confirm_read_only.load(Ordering::SeqCst) && read_only);

        let mut notes = Vec::new();
        if let Some(rogue) = rogue_arg(&args) {
            notes.push(format!("argument suspect : {rogue}"));
        }
        let shown_args = clip(&serde_json::to_string(&args).unwrap_or_default(), 300);
        b.note(format!(
            "→ {} › {} {}",
            e.server,
            e.tool.name,
            if shown_args.is_empty() { "{}" } else { &shown_args }
        ));
        for n in notes {
            b.note(n);
        }

        if !skip {
            let _one_at_a_time = b.ask.lock().await;
            b.note(format!("? confirmation demandée : {}", e.name));
            if !confirm(&self.app, &e, &args).await {
                b.note(format!("✗ refusé : {}", e.name));
                return ToolOut {
                    text:
                        "Refusé par l'utilisateur sur l'ordinateur (ou pas de réponse à temps)."
                            .to_string(),
                    is_error: true,
                };
            }
        }

        match state.mcp.call_tool(&e.server, &e.tool.name, args).await {
            Ok((text, is_error)) => {
                let mark = if is_error { "✗ erreur de l'outil" } else { "✓ ok" };
                b.note(format!("  {mark} : {}", e.name));
                ToolOut {
                    text: clip(&text, MAX_OUT),
                    is_error,
                }
            }
            Err(msg) => {
                b.note(format!("  ✗ échec : {} ({msg})", e.name));
                ToolOut {
                    text: msg,
                    is_error: true,
                }
            }
        }
    }

    fn note(&self, line: String) {
        self.app.state::<AppState>().bridge.note(line);
    }

    fn hit(&self, line: String) {
        self.app.state::<AppState>().bridge.hit(line);
    }
}

/// Détecte un argument qui ressemble à un secret (pour le signaler, jamais l'afficher).
fn rogue_arg(args: &Value) -> Option<String> {
    let o = args.as_object()?;
    for (k, _) in o {
        let low = k.to_ascii_lowercase();
        if ["token", "secret", "password", "authorization", "api_key", "apikey"]
            .iter()
            .any(|p| low.contains(p))
        {
            return Some(format!("« {k} » (valeur masquée)"));
        }
    }
    None
}

// ---------- Tunnel cloudflared ----------

fn candidates(app: &AppHandle, cfg: &BridgeConfig) -> Vec<String> {
    let mut v = Vec::new();
    if let Some(p) = cfg.cloudflared_path.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        v.push(p.to_string());
    }
    let suffix = if cfg!(windows) { ".exe" } else { "" };
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
    // En mode `cargo tauri dev`, le binaire source est souvent dans src-tauri/binaries.
    dirs.push(PathBuf::from("binaries"));
    dirs.push(PathBuf::from("src-tauri/binaries"));

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
    let mut seen = HashSet::new();
    v.retain(|p| seen.insert(p.clone()));
    v
}

/// `https://xxxx.trycloudflare.com` trouvée dans une ligne de log (hors `api.`).
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

fn configure_cmd(cmd: &mut Command) {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
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
                tail.push(clip(&line, 200));
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

/// Vide stderr en arrière-plan (mode nommé : l'URL est connue d'avance).
fn drain_stderr(mut child: Child) -> Child {
    let Some(stderr) = child.stderr.take() else {
        return child;
    };
    tauri::async_runtime::spawn(async move {
        let mut rd = BufReader::new(stderr);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match rd.read_until(b'\n', &mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
    });
    child
}

async fn start_tunnel(app: &AppHandle, cfg: &BridgeConfig, port: u16) -> Result<(Child, String), String> {
    let named = cfg.mode == "named";
    let public = if named {
        let u = cfg
            .public_url
            .as_deref()
            .map(str::trim)
            .filter(|u| !u.is_empty())
            .ok_or("mode named : `public_url` est obligatoire dans bridge.json (ex. https://fscode.example.com)")?;
        if !(u.starts_with("https://") || u.starts_with("http://")) {
            return Err("mode named : `public_url` doit commencer par https:// (ou http://)".to_string());
        }
        Some(u.trim_end_matches('/').to_string())
    } else {
        None
    };
    let token = if named {
        Some(
            fs_store::get_secret(&cfg.named_tunnel_token_ref)
                .ok_or("mode named : jeton du tunnel absent du coffre (bouton « Jeton » dans le panneau Pont MCP).")?,
        )
    } else {
        None
    };

    let target = format!("http://127.0.0.1:{port}");
    let mut last = String::new();
    for exe in candidates(app, cfg) {
        let mut cmd = Command::new(&exe);
        if let Some(url) = &public {
            cmd.args(["tunnel", "--no-autoupdate", "run"]);
            if let Some(t) = &token {
                cmd.env("TUNNEL_TOKEN", t); // jamais dans la ligne de commande
            }
            let _ = url;
        } else {
            cmd.args([
                "tunnel",
                "--no-autoupdate",
                "--protocol",
                "http2",
                "--url",
                &target,
            ]);
        }
        configure_cmd(&mut cmd);
        let child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("impossible de lancer cloudflared : {e}")),
        };
        if let Some(url) = public {
            let mut child = drain_stderr(child);
            sleep(Duration::from_millis(1500)).await;
            if let Ok(Some(status)) = child.try_wait() {
                return Err(format!(
                    "cloudflared s'est arrêté ({status}). Vérifie le jeton du tunnel nommé et `public_url`."
                ));
            }
            return Ok((child, url));
        }
        match read_tunnel_url(child).await {
            Ok(ok) => return Ok(ok),
            Err(e) => last = e,
        }
    }
    if last.is_empty() {
        Err("cloudflared introuvable. Il devrait être embarqué avec l'app : lance `npm run fetch-cloudflared` dans fscode/app puis relance (ou installe-le : winget install --id Cloudflare.cloudflared).".to_string())
    } else {
        Err(last)
    }
}

/// Le tunnel répond-il depuis ce PC ? `any` accepte une réponse 403 (Cloudflare Access).
async fn wait_reachable(url: &str, any: bool) -> bool {
    let Ok(http) = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
    else {
        return false;
    };
    let end = Instant::now() + REACH_WAIT;
    while Instant::now() < end {
        if let Ok(r) = http.get(format!("{url}/health")).send().await {
            if r.status().is_success() || (any && r.status().is_client_error()) {
                return true;
            }
        }
        sleep(Duration::from_secs(1)).await;
    }
    false
}

// ---------- Fichier PID et orphelins ----------

#[derive(Serialize, Deserialize)]
struct PidInfo {
    pid: u32,
    started_at: u64,
    command: String,
    public_url: Option<String>,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn write_pid(dir: &std::path::Path, child: &Child, mode: &str, url: &str) -> Option<PathBuf> {
    let pid = child.id()?;
    let info = PidInfo {
        pid,
        started_at: now_secs(),
        command: if mode == "named" {
            "cloudflared tunnel run".to_string()
        } else {
            "cloudflared tunnel --url 127.0.0.1".to_string()
        },
        public_url: Some(url.to_string()),
    };
    let path = dir.join(PID_FILE);
    let text = serde_json::to_string_pretty(&info).ok()?;
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(&path, text);
    Some(path)
}

#[cfg(windows)]
fn process_cmdline(pid: u32) -> Option<String> {
    use std::os::windows::process::CommandExt;
    let script = format!(
        "$p=Get-CimInstance Win32_Process -Filter \"ProcessId={pid}\"; if($p){{$p.CommandLine}}"
    );
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(0x0800_0000)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

#[cfg(not(windows))]
fn process_cmdline(pid: u32) -> Option<String> {
    let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let text = String::from_utf8_lossy(&raw).replace('\0', " ");
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
}

#[cfg(windows)]
fn kill_pid(pid: u32) {
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .creation_flags(0x0800_0000)
        .status();
}

#[cfg(not(windows))]
fn kill_pid(pid: u32) {
    let _ = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status();
}

/// Tue le `cloudflared` laissé par une exécution précédente (PID + ligne de commande vérifiés).
async fn cleanup_orphan(app: &AppHandle) -> Option<String> {
    let dir = config_dir(app).ok()?;
    let path = dir.join(PID_FILE);
    let text = std::fs::read_to_string(&path).ok()?;
    let info: PidInfo = serde_json::from_str(&text).ok()?;
    let _ = std::fs::remove_file(&path);
    let cmd = process_cmdline(info.pid)?;
    let low = cmd.to_ascii_lowercase();
    if low.contains("cloudflared") && low.contains("tunnel") {
        kill_pid(info.pid);
        return Some(format!(
            "ancien cloudflared (PID {}) arrêté au démarrage",
            info.pid
        ));
    }
    // PID réutilisé par un autre programme : on ne touche à rien.
    None
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
    Ok(server_list(&v)
        .into_iter()
        .find(|s| s.get("server_label").and_then(Value::as_str) == Some(label)))
}

async fn unregister(c: &Client, id: &str) -> Result<(), String> {
    if !safe_id(id) {
        return Err(format!("identifiant refusé : {id}"));
    }
    match c
        .send_json("DELETE", &format!("/mcp/servers/{id}"), None)
        .await
    {
        Ok(_) => Ok(()),
        Err(fs_api::Error::Status { status: 404, .. }) => Ok(()),
        Err(e) => Err(format!("DELETE /mcp/servers/{id} : {e}")),
    }
}

/// Supprime toutes les entrées `fscode-*` (jamais celles de l'utilisateur).
async fn purge_orphans(c: &Client, prefix: &str) -> (usize, Vec<String>) {
    let mut removed = 0;
    let mut warnings = Vec::new();
    let list = match c.get_json("/mcp/servers").await {
        Ok(v) => server_list(&v),
        Err(e) => return (0, vec![format!("purge /mcp/servers impossible : {e}")]),
    };
    let start = format!("{prefix}-");
    for s in list {
        let Some(label) = s.get("server_label").and_then(Value::as_str) else {
            continue;
        };
        if !label.starts_with(&start) {
            continue;
        }
        let Some(id) = pick_id(&s) else { continue };
        match unregister(c, &id).await {
            Ok(()) => removed += 1,
            Err(e) => warnings.push(format!("entrée « {label} » non retirée : {e}")),
        }
    }
    (removed, warnings)
}

/// Réponse de FS sans la liste (énorme) des outils : on n'en garde que le nombre.
fn summarize(v: &Value, tokens: &[String]) -> String {
    let mut o = v.clone();
    if let Some(m) = o.as_object_mut() {
        if let Some(t) = m.remove("available_tools") {
            let n = t.as_array().map(|a| a.len()).unwrap_or(0);
            m.insert("available_tools_count".to_string(), json!(n));
        }
    }
    clip(&mask_secrets(&o.to_string(), tokens), 1500)
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

/// Crée (ou met à jour) l'entrée d'un serveur. Renvoie `(identifiant, réponse résumée)`.
async fn register(
    c: &Client,
    cfg: &BridgeConfig,
    label: &str,
    server: &str,
    url: &str,
    token: &str,
    tools: &[PublicTool],
    tokens: &[String],
) -> Result<(String, String), String> {
    let names: Vec<&str> = tools.iter().map(|t| t.public.as_str()).collect();
    let defer = should_defer(tools);

    // Le jeton ne passe que par `authorization` (l'API refuse les deux ensemble) ; `headers` = Cloudflare Access seulement.
    let mut headers = Map::new();
    if cfg.mode == "named" {
        if let (Some(id), Some(secret)) = (
            fs_store::get_secret(&cfg.access_client_id_ref),
            fs_store::get_secret(&cfg.access_client_secret_ref),
        ) {
            headers.insert("CF-Access-Client-Id".to_string(), Value::String(id));
            headers.insert("CF-Access-Client-Secret".to_string(), Value::String(secret));
        }
    }

    let mut body = json!({
        "server_label": label,
        "server_description": format!("FS Code — serveur MCP {server} exposé via tunnel Cloudflare"),
        "server_url": url,
        "auth_type": "manual",
        "authorization": token,
        "require_approval": "never",
        "defer_loading": defer,
        "enabled": true,
        "allowed_tools": names,
    });

    if !headers.is_empty() {
        body["headers"] = Value::Object(headers);
    }

    // Champs libres de `register_extra`, sans jamais laisser écraser les champs de sécurité.
    if let Some(o) = body.as_object_mut() {
        for (k, v) in &cfg.register_extra {
            if matches!(
                k.as_str(),
                "server_label" | "server_url" | "authorization" | "headers" | "auth_type"
            ) {
                continue;
            }
            o.insert(k.clone(), v.clone());
        }
    }

    let mut existing = find_entry(c, label)
        .await
        .ok()
        .flatten()
        .and_then(|v| pick_id(&v));

    let mut last = String::new();
    for attempt in 0..3 {
        if attempt > 0 {
            // FS teste peut-être l'URL à l'enregistrement : le tunnel peut mettre du temps.
            sleep(Duration::from_secs(4)).await;
        }
        let res = match &existing {
            Some(id) => {
                c.send_json("POST", &format!("/mcp/servers/{id}"), Some(body.clone()))
                    .await
            }
            None => c.send_json("POST", "/mcp/servers", Some(body.clone())).await,
        };
        match res {
            Ok(v) => {
                let mut id = pick_id(&v).or_else(|| existing.clone());
                if id.is_none() {
                    id = find_entry(c, label)
                        .await
                        .ok()
                        .flatten()
                        .and_then(|e| pick_id(&e));
                }
                let id = id.ok_or("entrée créée, mais identifiant introuvable dans la réponse")?;
                let mut note = format!("POST → {}", summarize(&v, tokens));
                // `allowed_tools: []` est ambigu : on liste alors explicitement les outils vus par FS.
                let empty = v
                    .get("allowed_tools")
                    .and_then(Value::as_array)
                    .map(|a| a.is_empty())
                    .unwrap_or(false);
                let seen = tool_names(&v);
                if empty && !seen.is_empty() {
                    let mut upd = body.clone();
                    upd["allowed_tools"] = json!(seen);
                    match c
                        .send_json("POST", &format!("/mcp/servers/{id}"), Some(upd))
                        .await
                    {
                        Ok(v2) => note.push_str(&format!(
                            "\nallowed_tools explicite ({} outils) → {}",
                            seen.len(),
                            summarize(&v2, tokens)
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

// ---------- Cycle de vie ----------

async fn serve(gw: Arc<Gateway>) -> Result<(u16, JoinHandle<()>), String> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| format!("port local indisponible : {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let router = gw.router();
    let handle = tauri::async_runtime::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    Ok((port, handle))
}

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

    b.step("nettoyage des anciennes entrées");
    let (removed, mut warnings) = purge_orphans(&c, &cfg.label_prefix).await;
    if removed > 0 {
        b.note(format!("{removed} ancienne(s) entrée(s) MCP retirée(s)"));
    }

    b.step("connexion des serveurs MCP");
    warnings.extend(connect_all(app, state).await?);
    let exposed = state.mcp.exposed().await;
    if exposed.is_empty() {
        return Err("aucun outil MCP disponible : ajoute un serveur dans l'onglet MCP (mcp.json) et vérifie qu'il démarre.".to_string());
    }

    let (mut plans, aggregate) = build_plans(exposed, cfg);
    let tools_n: usize = plans.iter().map(|p| p.tools.len()).sum();
    let servers: Vec<String> = plans.iter().map(|p| p.server.clone()).collect();

    let mut tokens = Vec::new();
    for p in &mut plans {
        p.token = new_token()?;
        tokens.push(p.token.clone());
    }
    let global = new_token()?;
    tokens.push(global.clone());
    if let Ok(mut t) = b.tokens.lock() {
        *t = tokens.clone();
    }
    b.confirm_read_only
        .store(cfg.confirm_read_only, Ordering::SeqCst);

    let mut per_server = HashMap::new();
    let mut servers_idx = Vec::new();
    for p in &plans {
        per_server.insert(p.server.clone(), Arc::new(Semaphore::new(PER_SERVER_CONC)));
        servers_idx.push(ServerEntry {
            server: p.server.clone(),
            route: p.route.clone(),
            token: p.token.clone(),
            tools: p.tools.clone(),
        });
    }
    let gateway = Arc::new(Gateway {
        servers: servers_idx,
        aggregate,
        global,
        host: Arc::new(AppHost { app: app.clone() }),
        timeout: Duration::from_secs(cfg.timeout_seconds),
        // Une confirmation peut durer 120 s, puis l'outil jusqu'à 120 s.
        call_timeout: Duration::from_secs(cfg.timeout_seconds.max(120) + CONFIRM_WAIT.as_secs()),
        global_sem: Arc::new(Semaphore::new(GLOBAL_CONC)),
        per_server,
    });

    b.step("serveur local");
    let (port, handle) = serve(gateway).await?;
    live.server = Some(handle);
    b.set(|s| {
        s.port = Some(port);
        s.servers = servers.clone();
        s.tools = tools_n;
        s.warnings = warnings.clone();
    });

    b.step("tunnel cloudflared");
    let (child, url) = start_tunnel(app, cfg, port).await?;
    if let Some(path) = write_pid(&config_dir(app)?, &child, &cfg.mode, &url) {
        live.pid_file = path;
    }
    live.tunnel = Some(child);
    live.public_url = url.clone();
    b.set(|s| s.url = Some(url.clone()));

    b.step("attente du tunnel");
    let any = cfg.mode == "named";
    if !wait_reachable(&url, any).await {
        warnings.push(format!(
            "tunnel pas joignable depuis ce PC après {} s (DNS/réseau ?) : enregistré quand même",
            REACH_WAIT.as_secs()
        ));
    }

    let mut views: Vec<EntryView> = plans
        .iter()
        .map(|p| EntryView {
            server: p.server.clone(),
            label: p.label.clone(),
            url: format!("{}/mcp/{}", url.trim_end_matches('/'), p.route),
            server_id: None,
            tools: p.tools.len(),
            error: None,
        })
        .collect();

    let mut notes = Vec::new();
    let mut registered = 0;
    for (i, p) in plans.iter().enumerate() {
        b.step(&format!("enregistrement de « {} »", p.server));
        match register(
            &c,
            cfg,
            &p.label,
            &p.server,
            &views[i].url,
            &p.token,
            &p.tools,
            &tokens,
        )
        .await
        {
            Ok((id, note)) => {
                registered += 1;
                live.entries.push(LiveEntry {
                    server_id: id.clone(),
                    label: p.label.clone(),
                });
                views[i].server_id = Some(id);
                notes.push(note);
            }
            Err(e) => {
                views[i].error = Some(e.clone());
                warnings.push(format!("{} : {e}", p.server));
            }
        }
        b.set(|s| {
            s.entries = views.clone();
            s.warnings = warnings.clone();
        });
    }
    if registered == 0 {
        return Err(format!(
            "aucune entrée MCP enregistrée : {}",
            warnings.join(" ; ")
        ));
    }

    b.set(|s| {
        s.phase = Phase::Running;
        s.step = String::new();
        s.entries = views;
        s.warnings = warnings;
        s.register_note = Some(clip(&notes.join("\n\n"), 6000));
    });
    b.note(format!(
        "pont actif : {} outil(s), {} serveur(s) via {url}",
        tools_n,
        plans.len()
    ));
    Ok(())
}

/// Retire les entrées FS, coupe le serveur local et le tunnel.
async fn teardown(app: &AppHandle, state: &State<'_, AppState>, live: &mut Live) -> Option<String> {
    let mut warnings = Vec::new();
    if !live.entries.is_empty() {
        match crate::client(app, state).await {
            Ok(c) => {
                for e in live.entries.drain(..) {
                    if let Err(err) = unregister(&c, &e.server_id).await {
                        warnings.push(format!("entrée « {} » non retirée : {err}", e.label));
                    }
                }
            }
            Err(e) => warnings.push(format!("entrées FS non retirées : {e}")),
        }
    }
    if let Some(h) = live.server.take() {
        h.abort();
    }
    if let Some(mut t) = live.tunnel.take() {
        let _ = t.kill().await;
        let _ = t.wait().await;
    }
    if !live.pid_file.as_os_str().is_empty() {
        let _ = std::fs::remove_file(&live.pid_file);
    }
    (!warnings.is_empty()).then(|| warnings.join(" ; "))
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

    if let Some(w) = cleanup_orphan(app).await {
        b.note(w);
    }

    let mut live = Live {
        server: None,
        tunnel: None,
        entries: Vec::new(),
        public_url: String::new(),
        pid_file: PathBuf::new(),
    };
    match run_start(app, &state, &cfg, &mut live).await {
        Ok(()) => {
     let pid = live.tunnel.as_ref().and_then(|t| t.id());
            *b.live.lock().await = Some(live);
     watch_tunnel(app.clone(), pid);
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
            });
            b.note(format!("échec du démarrage : {e}"));
            Err(e)
        }
    }
}

/// Surveille `cloudflared` : s'il s'arrête seul, retire les entrées FS (sinon elles pointent vers
/// un tunnel mort) et affiche l'erreur, pour que « Démarrer » refonctionne.
fn watch_tunnel(app: AppHandle, pid: Option<u32>) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        loop {
     sleep(Duration::from_secs(5)).await;
            let dead: Option<String> = {
         let mut g = state.bridge.live.lock().await;
  let Some(t) = g.as_mut().and_then(|l| l.tunnel.as_mut()) else {
      return; // pont arrêté
  };
         if t.id() != pid {
      return; // un autre tunnel a pris la suite
  }
         match t.try_wait() {
      Ok(Some(st)) => Some(format!("{st}")),
      Ok(None) => None,
      Err(_) => return,
  }
     };
     if let Some(st) = dead {
         let msg = format!("cloudflared s'est arrêté ({st}) : entrées retirées, relancez « Démarrer »");
  let _ = stop(&app, false).await;
  state.bridge.note(msg.clone());
  state.bridge.set(|s| {
      s.phase = Phase::Error;
      s.error = Some(msg);
         });
         return;
     }
 }
    });
}

/// `purge` : même sans pont lancé, supprime les entrées périmées et l'orphelin.
pub async fn stop(app: &AppHandle, purge: bool) -> Snapshot {
    let state = app.state::<AppState>();
    let b = &state.bridge;
    let _op = b.op.lock().await;
    let taken = b.live.lock().await.take();
    let was_running = taken.is_some();
    // Une confirmation en cours est refusée (le canal est fermé).
    if let Ok(mut p) = b.pending.lock() {
        *p = None;
    }
    let mut warns = Vec::new();
    if let Some(mut live) = taken {
        if let Some(w) = teardown(app, &state, &mut live).await {
            warns.push(w);
        }
    }
    if purge {
        let dir = config_dir(app).ok();
        let prefix = dir
            .as_deref()
            .and_then(|d| load_cfg(d).ok())
            .map(|c| c.label_prefix)
            .unwrap_or_else(|| DEFAULT_PREFIX.to_string());
        match crate::client(app, &state).await {
            Ok(c) => {
                let (n, w) = purge_orphans(&c, &prefix).await;
                if n > 0 {
                    warns.push(format!("{n} entrée(s) MCP périmée(s) retirée(s)"));
                }
                warns.extend(w);
            }
            Err(e) => warns.push(format!("nettoyage des entrées FS ignoré : {e}")),
        }
        if let Some(w) = cleanup_orphan(app).await {
            warns.push(w);
        }
    }
    b.reset(Phase::Stopped, "");
    b.set(|s| {
        if was_running {
            push_log(s, "pont arrêté".to_string());
        }
        if !warns.is_empty() {
            s.warnings = warns;
        }
    });
    b.snapshot()
}

/// Au lancement sans démarrage auto : retire chez FS les entrées `fscode-*` laissées par un crash.
async fn purge_stale(app: &AppHandle, prefix: &str) {
    let state = app.state::<AppState>();
    let _op = state.bridge.op.lock().await;
    if state.bridge.live.lock().await.is_some() {
        return;
    }
    let Ok(c) = crate::client(app, &state).await else {
 return; // pas connecté : rien à nettoyer
    };
    let (n, w) = purge_orphans(&c, prefix).await;
    if n > 0 {
        state.bridge.note(format!("{n} entrée(s) MCP périmée(s) retirée(s) au démarrage"));
    }
    for x in w {
 state.bridge.note(x);
    }
}

/// Appelé au lancement de l'app : tue un éventuel orphelin, puis démarre si demandé.
pub fn on_launch(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(w) = cleanup_orphan(&app).await {
            app.state::<AppState>().bridge.note(w);
        }
        let Ok(dir) = config_dir(&app) else {
            return;
        };
        match load_cfg(&dir) {
            Ok(c) if c.auto_start => {
                let _ = start(&app).await;
            }
            Ok(c) => purge_stale(&app, &c.label_prefix).await,
     Err(_) => purge_stale(&app, DEFAULT_PREFIX).await,
        }
    });
}

/// Serveurs MCP exposés (nom, nombre d'outils), pour le contexte des instructions.
/// Vide si le pont n'est pas lancé.
pub fn mcp_summary(app: &AppHandle) -> Vec<(String, usize)> {
    let s = app.state::<AppState>().bridge.snapshot();
    if !matches!(s.phase, Phase::Running) {
        return Vec::new();
    }
    s.entries
        .into_iter()
        .filter(|e| e.error.is_none())
        .map(|e| (e.server, e.tools))
        .collect()
}

fn status(app: &AppHandle) -> Snapshot {
    let state = app.state::<AppState>();
    let mut s = state.bridge.snapshot();
    if let Ok(dir) = config_dir(app) {
        if let Ok(c) = load_cfg(&dir) {
            s.auto_start = c.auto_start;
            s.mode = c.mode;
            s.confirm_read_only = c.confirm_read_only;
            s.max_tools = c.max_tools_per_entry;
            s.named_ready = fs_store::get_secret(&c.named_tunnel_token_ref).is_some();
        }
    }
    s.auto_allow = state.bridge.auto_allow.load(Ordering::SeqCst);
    s
}

// ---------- Commandes Tauri ----------

/// Démarre le pont (peut durer jusqu'à ~2 min : serveurs MCP, tunnel, enregistrement).
#[tauri::command]
pub async fn bridge_start(app: AppHandle) -> Result<Snapshot, String> {
    let r = start(&app).await;
    crate::instructions::schedule(&app);
    r
}

/// Arrête le pont et supprime les entrées `fscode-*` restées chez France Student.
#[tauri::command]
pub async fn bridge_stop(app: AppHandle) -> Result<Snapshot, String> {
    let s = stop(&app, true).await;
    crate::instructions::schedule(&app);
    Ok(s)
}

#[tauri::command]
pub fn bridge_status(app: AppHandle) -> Snapshot {
    status(&app)
}

/// Seuls les champs fournis changent. `auto_allow` ne vaut que pour la session.
#[tauri::command]
pub async fn bridge_set(
    app: AppHandle,
    auto_start: Option<bool>,
    auto_allow: Option<bool>,
    confirm_read_only: Option<bool>,
    mode: Option<String>,
    public_url: Option<String>,
) -> Result<Snapshot, String> {
    let state = app.state::<AppState>();
    let changing = mode.is_some() || public_url.is_some();
    if changing && state.bridge.live.lock().await.is_some() {
        return Err("arrête le pont avant de changer de mode ou d'URL".to_string());
    }
    let dir = config_dir(&app)?;
    let mut c = load_cfg(&dir)?;
    if let Some(v) = auto_start {
        c.auto_start = v;
    }
    if let Some(v) = auto_allow {
        state.bridge.auto_allow.store(v, Ordering::SeqCst);
    }
    if let Some(v) = confirm_read_only {
        c.confirm_read_only = v;
        state.bridge.confirm_read_only.store(v, Ordering::SeqCst);
    }
    if let Some(v) = mode {
        c.mode = v;
    }
    if let Some(v) = public_url {
        let v = v.trim();
        c.public_url = (!v.is_empty()).then(|| v.to_string());
    }
    let c = normalize_cfg(c)?;
    save_cfg(&dir, &c)?;
    drop(state);
    Ok(status(&app))
}

/// Jeton du tunnel nommé. Chaîne vide = effacement. Jamais écrit dans `bridge.json`.
#[tauri::command]
pub fn bridge_set_token(app: AppHandle, token: String) -> Result<Snapshot, String> {
    let dir = config_dir(&app)?;
    let c = load_cfg(&dir)?;
    if token.trim().is_empty() {
        fs_store::clear_secret(&c.named_tunnel_token_ref);
    } else {
        fs_store::set_secret(&c.named_tunnel_token_ref, token.trim())?;
    }
    Ok(status(&app))
}

/// Identifiants Cloudflare Access (mode nommé). Chaînes vides = effacement.
#[tauri::command]
pub fn bridge_set_access(app: AppHandle, client_id: String, client_secret: String) -> Result<Snapshot, String> {
    let dir = config_dir(&app)?;
    let c = load_cfg(&dir)?;
    if client_id.trim().is_empty() && client_secret.trim().is_empty() {
        fs_store::clear_secret(&c.access_client_id_ref);
        fs_store::clear_secret(&c.access_client_secret_ref);
    } else if client_id.trim().is_empty() || client_secret.trim().is_empty() {
        return Err("Cloudflare Access : renseigne l'identifiant et le secret (ou laisse les deux vides).".to_string());
    } else {
        fs_store::set_secret(&c.access_client_id_ref, client_id.trim())?;
        fs_store::set_secret(&c.access_client_secret_ref, client_secret.trim())?;
    }
    Ok(status(&app))
}

// ---------- Tests ----------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    struct MockHost;

    #[async_trait]
    impl Host for MockHost {
        async fn call(&self, server: &str, tool: &str, _read_only: bool, args: Value) -> ToolOut {
            ToolOut {
                text: format!("{server}/{tool}:{args}"),
                is_error: false,
            }
        }
        fn note(&self, _line: String) {}
        fn hit(&self, _line: String) {}
    }

    fn gateway() -> Arc<Gateway> {
        let tool = PublicTool {
            public: "echo".to_string(),
            raw: "echo".to_string(),
            description: "[alpha] echo".to_string(),
            input_schema: json!({ "type": "object" }),
            read_only: false,
        };
        Arc::new(Gateway {
            servers: vec![ServerEntry {
                server: "alpha".to_string(),
                route: "alpha".to_string(),
                token: "tok-alpha".to_string(),
                tools: vec![tool.clone()],
            }],
            aggregate: vec![AggTool {
                server: "alpha".to_string(),
                tool: PublicTool {
                    public: "alpha__echo".to_string(),
                    ..tool
                },
            }],
            global: "tok-global".to_string(),
            host: Arc::new(MockHost),
            timeout: Duration::from_secs(5),
            call_timeout: Duration::from_secs(5),
            global_sem: Arc::new(Semaphore::new(8)),
            per_server: HashMap::new(),
        })
    }

    async fn send(
        gw: &Arc<Gateway>,
        path: &str,
        token: Option<&str>,
        body: &str,
    ) -> (StatusCode, Value) {
        let mut req = Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json");
        if let Some(t) = token {
            req = req.header("authorization", format!("Bearer {t}"));
        }
        let res = gw
            .router()
            .oneshot(req.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let v = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, v)
    }

    #[tokio::test]
    async fn auth_required_and_query_token_rejected() {
        let gw = gateway();
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
        assert_eq!(send(&gw, "/mcp/alpha", None, body).await.0, 401);
        assert_eq!(send(&gw, "/mcp/alpha", Some("faux"), body).await.0, 401);
        // Jeton dans l'URL : ignoré (fuite dans les logs Cloudflare).
        assert_eq!(
            send(&gw, "/mcp/alpha?t=tok-alpha", None, body).await.0,
            401
        );
        assert_eq!(
            send(&gw, "/mcp/alpha", Some("tok-alpha"), body).await.0,
            200
        );
    }

    #[tokio::test]
    async fn unknown_route_is_jsonrpc_404() {
        let gw = gateway();
        let (status, v) = send(
            &gw,
            "/mcp/inconnu",
            Some("tok-alpha"),
            r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#,
        )
        .await;
        assert_eq!(status, 404);
        assert_eq!(v["error"]["code"], -32601);
    }

    #[tokio::test]
    async fn tools_list_and_call() {
        let gw = gateway();
        let (status, v) = send(
            &gw,
            "/mcp/alpha",
            Some("tok-alpha"),
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(v["result"]["tools"][0]["name"], "echo");
        assert!(v["result"]["tools"][0].get("annotations").is_none());

        let (status, v) = send(
            &gw,
            "/mcp/alpha",
            Some("tok-alpha"),
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"echo","arguments":{"x":1}}}"#,
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(v["result"]["isError"], false);
        assert!(v["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("alpha/echo"));
    }

    #[tokio::test]
    async fn aggregate_and_batch() {
        let gw = gateway();
        let (status, v) = send(
            &gw,
            "/mcp",
            Some("tok-global"),
            r#"[{"jsonrpc":"2.0","id":1,"method":"ping"},{"jsonrpc":"2.0","method":"notifications/initialized"}]"#,
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(v.as_array().unwrap().len(), 1);
        assert_eq!(v[0]["id"], 1);

        let (status, v) = send(
            &gw,
            "/mcp",
            Some("tok-global"),
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"alpha__echo","arguments":{}}}"#,
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(v["result"]["isError"], false);
    }

    #[tokio::test]
    async fn protocol_errors() {
        let gw = gateway();
        let (status, v) = send(&gw, "/mcp/alpha", Some("tok-alpha"), "{").await;
        assert_eq!(status, 400);
        assert_eq!(v["error"]["code"], -32700);

        let (status, v) = send(&gw, "/mcp/alpha", Some("tok-alpha"), "[]").await;
        assert_eq!(status, 400);
        assert_eq!(v["error"]["code"], -32600);

        let (status, v) = send(
            &gw,
            "/mcp/alpha",
            Some("tok-alpha"),
            r#"{"jsonrpc":"2.0","id":1,"method":"inconnue"}"#,
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(v["error"]["code"], -32601);

        let (status, v) = send(
            &gw,
            "/mcp/alpha",
            Some("tok-alpha"),
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"absent"}}"#,
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(v["error"]["code"], -32602);
    }

    #[tokio::test]
    async fn body_limit_is_413() {
        let gw = gateway();
        let body = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"ping","params":{{"x":"{}"}}}}"#,
            "a".repeat(BODY_LIMIT)
        );
        let res = gw
            .router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/mcp/alpha")
                    .header("authorization", "Bearer tok-alpha")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[test]
    fn tool_name_collisions_are_stable() {
        let exposed = vec![
            Exposed {
                name: "mcp__a__foo bar".to_string(),
                server: "a".to_string(),
                tool: mcp::McpTool {
                    name: "foo bar".to_string(),
                    description: "un".to_string(),
                    input_schema: json!({}),
                    read_only: false,
                },
            },
            Exposed {
                name: "mcp__a__foo.bar".to_string(),
                server: "a".to_string(),
                tool: mcp::McpTool {
                    name: "foo.bar".to_string(),
                    description: "deux".to_string(),
                    input_schema: json!({}),
                    read_only: true,
                },
            },
        ];
        let (plans, agg) = build_plans(exposed, &BridgeConfig::default());
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].route, "a");
        assert_eq!(plans[0].label, "fscode-a");
        let names: Vec<&str> = plans[0].tools.iter().map(|t| t.public.as_str()).collect();
        assert_eq!(names.len(), 2);
        assert_ne!(names[0], names[1]);
        assert!(names.iter().all(|n| n.starts_with("foo_bar")));
        assert_eq!(agg.len(), 2);
        assert_ne!(agg[0].tool.public, agg[1].tool.public);
    }

    #[test]
    fn many_tools_are_capped_and_deferred() {
        let mut exposed = Vec::new();
        for i in 0..100 {
            exposed.push(Exposed {
                name: format!("mcp__srv__t{i}"),
                server: "srv".to_string(),
                tool: mcp::McpTool {
                    name: format!("t{i}"),
                    description: "d".repeat(80),
                    input_schema: json!({}),
                    read_only: false,
                },
            });
        }
        let cfg = BridgeConfig::default();
        let (plans, _) = build_plans(exposed, &cfg);
        assert_eq!(plans[0].tools.len(), cfg.max_tools_per_entry);
        assert!(should_defer(&plans[0].tools));
    }

    #[test]
    fn tunnel_url_and_secret_masking() {
        assert_eq!(
            find_tunnel_url("... https://abc-def.trycloudflare.com ...").as_deref(),
            Some("https://abc-def.trycloudflare.com")
        );
        assert_eq!(find_tunnel_url("https://api.trycloudflare.com"), None);
        let token = "0123456789abcdef";
        assert_eq!(
            mask_secrets(&format!("jeton {token} ici"), &[token.to_string()]),
            "jeton *** ici"
        );
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"abcd"));
    }

    #[test]
    fn authorized_accepts_raw_and_bearer() {
 let mk = |v: &str| {
     let mut h = HeaderMap::new();
     h.insert(header::AUTHORIZATION, v.parse().unwrap());
            h
        };
        assert!(authorized(&mk("tok"), "tok"));
 assert!(authorized(&mk("Bearer tok"), "tok"));
        assert!(authorized(&mk("bearer  tok "), "tok"));
        assert!(!authorized(&mk("tok2"), "tok"));
        assert!(!authorized(&mk("Bearer nope"), "tok"));
        assert!(!authorized(&HeaderMap::new(), "tok"));
    }
}




