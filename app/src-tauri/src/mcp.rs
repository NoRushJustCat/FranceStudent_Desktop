//! Client MCP local : transport stdio (JSON-RPC, une ligne par message) ou
//! HTTP « Streamable » (un POST JSON-RPC par requête, jeton Bearer optionnel).
//!
//! Rien ne démarre tout seul : un serveur n'est lancé / contacté que sur demande
//! explicite de l'utilisateur (`Hub::connect`). La liste des serveurs vient de
//! `mcp.json` (même format que Claude Desktop : `{"mcpServers": {...}}`), avec en plus
//! pour HTTP : `url`, `token` (optionnel) et `headers` (optionnel).

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::timeout;

const CONFIG_FILE: &str = "mcp.json";
const INIT_TIMEOUT: Duration = Duration::from_secs(30);
const CALL_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_TOOLS: usize = 200;

pub const EXAMPLE: &str = r#"{
  "mcpServers": {
    "exemple": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "C:\\Users\\Tom\\Documents"]
    },
    "distant-http": { "type": "http", "url": "https://exemple.com/mcp", "headers": { "Authorization": "Bearer ${MON_JETON}" } },
    "distant-sse": { "type": "sse", "url": "https://exemple.com/sse" },
    "desactive": { "command": "node", "args": ["srv.js"], "disabled": true }
  }
}
"#;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct ServerConfig {
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub cwd: Option<String>,
    /// Serveur HTTP (ex. `http://localhost:3000/mcp`) : remplace `command`.
    /// Alias : `serverUrl` (Cursor/Windsurf), `httpUrl` (Gemini CLI).
    #[serde(alias = "serverUrl", alias = "httpUrl")]
    pub url: Option<String>,
    /// Jeton optionnel, envoyé en `Authorization: Bearer …`.
    pub token: Option<String>,
    /// En-têtes HTTP supplémentaires.
    pub headers: HashMap<String, String>,
    /// `stdio`, `http`/`streamable-http`, `sse`. Déduit si absent.
    #[serde(
        rename = "type",
        alias = "transport",
        skip_serializing_if = "Option::is_none"
    )]
    pub kind: Option<String>,
    /// `true` : serveur ignoré (jamais lancé ni contacté).
    pub disabled: bool,
}

/// `${VAR}` / `${env:VAR}` → variable d'environnement (vide si absente).
fn expand_vars(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("${") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        match after.find('}') {
            Some(j) => {
                let key = after[..j].trim();
                let key = key.strip_prefix("env:").unwrap_or(key);
                out.push_str(&std::env::var(key).unwrap_or_default());
                rest = &after[j + 1..];
            }
            None => {
                out.push_str(&rest[i..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

impl ServerConfig {
    /// Copie avec `${VAR}` développé. Fait à la connexion seulement : `mcp.json` reste intact.
    pub fn expanded(&self) -> Self {
        let mut c = self.clone();
        c.command = expand_vars(&c.command);
        c.args = c.args.iter().map(|a| expand_vars(a)).collect();
        c.env = c
            .env
            .iter()
            .map(|(k, v)| (k.clone(), expand_vars(v)))
            .collect();
        c.cwd = c.cwd.as_deref().map(expand_vars);
        c.url = c.url.as_deref().map(expand_vars);
        c.token = c.token.as_deref().map(expand_vars);
        c.headers = c
            .headers
            .iter()
            .map(|(k, v)| (k.clone(), expand_vars(v)))
            .collect();
        c
    }

    /// Transport SSE : explicite via `type`, sinon deviné par `/sse` dans l'URL.
    pub fn is_sse(&self, url: &str) -> bool {
        match self.kind.as_deref().map(|k| k.trim().to_ascii_lowercase()) {
            Some(k) if k == "sse" => true,
            Some(k) if k.contains("http") => false,
            _ => url.to_ascii_lowercase().contains("/sse"),
        }
    }

    /// `(url, jeton)` si le serveur se joint par HTTP.
    /// Accepte aussi une `command` qui est en fait une URL (le jeton peut alors être `args[0]`).
    pub fn endpoint(&self) -> Option<(String, Option<String>)> {
        let token = self
            .token
            .as_deref()
            .map(clean_token)
            .filter(|t| !t.is_empty());
        if let Some(u) = self.url.as_deref().map(str::trim).filter(|u| !u.is_empty()) {
            return Some((u.to_string(), token));
        }
        let c = self.command.trim();
        if c.starts_with("http://") || c.starts_with("https://") {
            let tok = token.or_else(|| {
                self.args
                    .first()
                    .map(|a| a.trim())
                    .filter(|a| !a.is_empty() && !a.starts_with('-'))
                    .map(clean_token)
            });
            return Some((c.to_string(), tok));
        }
        None
    }

    /// Une commande à lancer ou une URL à joindre.
    pub fn is_valid(&self) -> bool {
        !self.command.trim().is_empty() || self.endpoint().is_some()
    }
}

fn clean_token(t: &str) -> String {
    let t = t.trim();
    let t = t
        .strip_prefix("Bearer ")
        .or_else(|| t.strip_prefix("bearer "))
        .unwrap_or(t);
    t.trim().to_string()
}

#[derive(Default, Deserialize)]
struct ConfigFile {
    /// `mcpServers` (Claude, Cursor, Gemini…) ou `servers` (VS Code).
    #[serde(default, rename = "mcpServers", alias = "servers")]
    mcp_servers: BTreeMap<String, ServerConfig>,
}

/// Serveurs déclarés dans `mcp.json` (vide si le fichier n'existe pas).
pub fn load_config(dir: &Path) -> Result<BTreeMap<String, ServerConfig>, String> {
    let Ok(text) = std::fs::read_to_string(dir.join(CONFIG_FILE)) else {
        return Ok(BTreeMap::new());
    };
    parse_config(&text).map(|c| c.mcp_servers)
}

fn parse_config(text: &str) -> Result<ConfigFile, String> {
    let cfg: ConfigFile =
        serde_json::from_str(text).map_err(|e| format!("mcp.json invalide : {e}"))?;
    for (name, s) in &cfg.mcp_servers {
        if !s.disabled && !s.is_valid() {
            return Err(format!("serveur « {name} » : `command` ou `url` manquant"));
        }
    }
    Ok(cfg)
}

/// Texte brut de `mcp.json` pour l'éditeur (exemple si absent).
pub fn config_text(dir: &Path) -> String {
    std::fs::read_to_string(dir.join(CONFIG_FILE)).unwrap_or_else(|_| EXAMPLE.to_string())
}

/// Valide puis écrit `mcp.json`.
pub fn save_config_text(dir: &Path, text: &str) -> Result<(), String> {
    parse_config(text)?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(CONFIG_FILE), text).map_err(|e| e.to_string())
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub read_only: bool,
}

/// Un outil MCP tel qu'exposé au modèle.
#[derive(Clone, Debug)]
pub struct Exposed {
    /// Nom envoyé au modèle : `mcp__serveur__outil`.
    pub name: String,
    pub server: String,
    pub tool: McpTool,
}

fn clean(s: &str) -> String {
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

pub fn exposed_name(server: &str, tool: &str) -> String {
    let mut n = format!("mcp__{}__{}", clean(server), clean(tool));
    n.truncate(64);
    n
}

/// Résultat d'une réponse JSON-RPC : `result`, ou le message de `error`.
fn rpc_result(v: &Value) -> Result<Value, String> {
    if let Some(e) = v.get("error") {
        let msg = e
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("erreur MCP");
        return Err(msg.to_string());
    }
    Ok(v.get("result").cloned().unwrap_or(Value::Null))
}

// ---------- Transport stdio ----------

struct StdioLink {
    // Gardé pour tuer le processus à la fermeture (kill_on_drop).
    _child: Child,
    stdin: ChildStdin,
    out: BufReader<ChildStdout>,
}

impl StdioLink {
    async fn send(&mut self, v: &Value) -> Result<(), String> {
        let mut s = v.to_string();
        s.push('\n');
        self.stdin
            .write_all(s.as_bytes())
            .await
            .map_err(|e| format!("écriture vers le serveur MCP : {e}"))?;
        self.stdin.flush().await.map_err(|e| e.to_string())
    }

    /// Lit jusqu'à la réponse `id` ; répond aux requêtes du serveur
    /// (`ping` accepté, le reste refusé) et ignore les notifications.
    async fn read_response(&mut self, id: u64) -> Result<Value, String> {
        loop {
            // Octets bruts : un serveur peut écrire du texte non UTF-8 (page de code Windows).
            let mut buf = Vec::new();
            let n = self
                .out
                .read_until(b'\n', &mut buf)
                .await
                .map_err(|e| format!("lecture du serveur MCP : {e}"))?;
            if n == 0 {
                return Err("le serveur MCP s'est arrêté".to_string());
            }
            let line = String::from_utf8_lossy(&buf);
            let Ok(v) = serde_json::from_str::<Value>(line.trim()) else {
                continue;
            };
            if let Some(m) = v.get("method").and_then(Value::as_str) {
                if let Some(rid) = v.get("id") {
                    let resp = if m == "ping" {
                        json!({"jsonrpc": "2.0", "id": rid, "result": {}})
                    } else {
                        json!({"jsonrpc": "2.0", "id": rid,
                               "error": {"code": -32601, "message": "non supporté par FS Code"}})
                    };
                    self.send(&resp).await?;
                }
                continue;
            }
            if v.get("id").and_then(Value::as_u64) == Some(id) {
                return rpc_result(&v);
            }
        }
    }
}

fn spawn(cfg: &ServerConfig) -> Result<Child, String> {
    // Sous Windows, `npx`, `uvx`… sont des .cmd : on passe par `cmd /C`.
    #[cfg(windows)]
    let mut cmd = {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(&cfg.command).args(&cfg.args);
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = Command::new(&cfg.command);
        c.args(&cfg.args);
        c
    };
    cmd.envs(&cfg.env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    if let Some(d) = cfg.cwd.as_deref().filter(|d| !d.trim().is_empty()) {
        cmd.current_dir(d);
    }
    cmd.spawn()
        .map_err(|e| format!("impossible de lancer « {} » : {e}", cfg.command))
}

// ---------- Transport HTTP (Streamable HTTP) ----------

struct HttpLink {
    http: reqwest::Client,
    url: String,
    headers: HeaderMap,
    session: Option<String>,
}

/// `localhost` / `127.0.0.1` / `[::1]` : pas de proxy système pour ceux-là.
fn is_local(url: &str) -> bool {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    host.starts_with("localhost") || host.starts_with("127.0.0.1") || host.starts_with("[::1]")
}

/// `http://localhost:3000/mcp` → `http://127.0.0.1:3000/mcp` (repli si `localhost` résout en IPv6).
fn ipv4_url(url: &str) -> Option<String> {
    for scheme in ["http://", "https://"] {
        if let Some(rest) = url.strip_prefix(scheme) {
            if let Some(after) = rest.strip_prefix("localhost") {
                if after.is_empty() || after.starts_with([':', '/', '?']) {
                    return Some(format!("{scheme}127.0.0.1{after}"));
                }
            }
        }
    }
    None
}

fn net_err(e: &reqwest::Error) -> String {
    if e.is_connect() {
        format!("serveur injoignable (est-il lancé ?) : {e}")
    } else {
        format!("erreur HTTP : {e}")
    }
}

impl HttpLink {
    async fn connect(
        url: String,
        token: Option<String>,
        extra: &HashMap<String, String>,
        is_sse: bool,
    ) -> Result<Self, String> {
        let mut headers = HeaderMap::new();
        for (k, v) in extra {
            let name = HeaderName::from_bytes(k.trim().as_bytes())
                .map_err(|e| format!("en-tête « {k} » invalide : {e}"))?;
            let val = HeaderValue::from_str(v.trim())
                .map_err(|e| format!("valeur de l'en-tête « {k} » invalide : {e}"))?;
            headers.insert(name, val);
        }
        if let Some(t) = token {
            if !headers.contains_key(AUTHORIZATION) {
                let mut val = HeaderValue::from_str(&format!("Bearer {t}"))
                    .map_err(|_| "jeton invalide (caractères interdits)".to_string())?;
                val.set_sensitive(true);
                headers.insert(AUTHORIZATION, val);
            }
        }
        let mut b = reqwest::Client::builder().connect_timeout(Duration::from_secs(10));
        if is_local(&url) {
            b = b.no_proxy();
        }
        let http = b.build().map_err(|e| e.to_string())?;

        let mut target_url = url.clone();
        let mut session = None;

        if is_sse {
            // Tentative de découverte d'endpoint SSE (spec MCP SSE)
            let get_res = http
                .get(&target_url)
                .headers(headers.clone())
                .header(ACCEPT, "text/event-stream, application/json")
                .send()
                .await;

            let res_opt = match get_res {
                Ok(r) => Some(r),
                Err(_e) => {
                    if let Some(alt) = ipv4_url(&target_url) {
                        http.get(&alt)
                            .headers(headers.clone())
                            .header(ACCEPT, "text/event-stream, application/json")
                            .send()
                            .await
                            .ok()
                    } else {
                        None
                    }
                }
            };

            if let Some(res) = res_opt {
                if let Some(s) = res
                    .headers()
                    .get("mcp-session-id")
                    .and_then(|v| v.to_str().ok())
                {
                    session = Some(s.to_string());
                }
                let ct = res
                    .headers()
                    .get(CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if res.status().is_success() && ct.contains("text/event-stream") {
                    if let Ok(text) = res.text().await {
                        let mut lines = text.lines();
                        while let Some(line) = lines.next() {
                            let l = line.trim();
                            if l.starts_with("event: endpoint") || l == "event:endpoint" {
                                for data_line in lines.by_ref() {
                                    if let Some(d) = data_line.strip_prefix("data:") {
                                        let ep = d.trim();
                                        if !ep.is_empty() {
                                            target_url = if ep.starts_with("http://")
                                                || ep.starts_with("https://")
                                            {
                                                ep.to_string()
                                            } else if let Ok(base) = reqwest::Url::parse(&url) {
                                                base.join(ep)
                                                    .map(|u| u.to_string())
                                                    .unwrap_or_else(|_| ep.to_string())
                                            } else {
                                                ep.to_string()
                                            };
                                            break;
                                        }
                                    }
                                }
                                break;
                            }
                        }
                    }
                }
            }
        }

        Ok(Self {
            http,
            url: target_url,
            headers,
            session,
        })
    }

    async fn send_to(
        &self,
        url: &str,
        payload: &Value,
    ) -> Result<reqwest::Response, reqwest::Error> {
        let mut req = self
            .http
            .post(url)
            .headers(self.headers.clone())
            .header(ACCEPT, "application/json, text/event-stream")
            .json(payload);
        if let Some(s) = &self.session {
            req = req.header("Mcp-Session-Id", s.as_str());
        }
        req.send().await
    }

    /// Envoie un message. `want_id` : identifiant de la réponse attendue (`None` pour une notification).
    async fn post(&mut self, payload: &Value, want_id: Option<u64>) -> Result<Value, String> {
        let res = match self.send_to(&self.url, payload).await {
            Ok(r) => r,
            Err(e) => {
                let Some(alt) = ipv4_url(&self.url) else {
                    return Err(net_err(&e));
                };
                match self.send_to(&alt, payload).await {
                    Ok(r) => {
                        self.url = alt;
                        r
                    }
                    Err(_) => return Err(net_err(&e)),
                }
            }
        };
        if let Some(s) = res
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
        {
            self.session = Some(s.to_string());
        }
        let status = res.status();
        if !status.is_success() {
            let body = res.text().await.unwrap_or_default();
            let body: String = body.chars().take(200).collect();
            return Err(if body.trim().is_empty() {
                format!("HTTP {status}")
            } else {
                format!("HTTP {status} : {}", body.trim())
            });
        }
        let Some(id) = want_id else {
            return Ok(Value::Null);
        };
        let is_sse = res
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|c| c.to_ascii_lowercase().contains("text/event-stream"));
        let text = res
            .text()
            .await
            .map_err(|e| format!("lecture de la réponse HTTP : {e}"))?;
        if is_sse {
            for line in text.lines() {
                let Some(d) = line.strip_prefix("data:") else {
                    continue;
                };
                let Ok(v) = serde_json::from_str::<Value>(d.trim()) else {
                    continue;
                };
                if v.get("id").and_then(Value::as_u64) == Some(id) {
                    return rpc_result(&v);
                }
            }
            return Err("réponse SSE sans résultat".to_string());
        }
        let v: Value =
            serde_json::from_str(&text).map_err(|e| format!("réponse HTTP non JSON : {e}"))?;
        rpc_result(&v)
    }
}

// ---------- Connexion ----------

enum Link {
    Stdio(StdioLink),
    Http(HttpLink),
}

struct Conn {
    link: Link,
    next_id: u64,
}

impl Conn {
    async fn request(
        &mut self,
        method: &str,
        params: Value,
        limit: Duration,
    ) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        let payload = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let run = async {
            match &mut self.link {
                Link::Stdio(s) => {
                    s.send(&payload).await?;
                    s.read_response(id).await
                }
                Link::Http(h) => h.post(&payload, Some(id)).await,
            }
        };
        match timeout(limit, run).await {
            Ok(r) => r,
            Err(_) => Err(format!("délai dépassé ({} s) : {method}", limit.as_secs())),
        }
    }

    async fn notify(&mut self, method: &str) -> Result<(), String> {
        let payload = json!({"jsonrpc": "2.0", "method": method});
        let run = async {
            match &mut self.link {
                Link::Stdio(s) => s.send(&payload).await,
                Link::Http(h) => h.post(&payload, None).await.map(|_| ()),
            }
        };
        match timeout(INIT_TIMEOUT, run).await {
            Ok(r) => r,
            Err(_) => Err(format!("délai dépassé : {method}")),
        }
    }
}

const MAX_TOOL_DESC_CHARS: usize = 220;
const MAX_PARAM_DESC_CHARS: usize = 110;

/// Compacte une description MCP (espaces/retours à la ligne multiples + plafond de caractères)
/// afin d'économiser des tokens de contexte à chaque tour LLM.
fn compact_description(raw: &str, max_chars: usize) -> String {
    let mut out = String::with_capacity(raw.len().min(max_chars + 4));
    let mut prev_ws = false;
    for ch in raw.trim().chars() {
        if ch.is_whitespace() {
            if !prev_ws {
                out.push(' ');
                prev_ws = true;
            }
        } else {
            out.push(ch);
            prev_ws = false;
        }
    }
    if out.chars().count() <= max_chars {
        return out;
    }
    let clipped: String = out.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{}…", clipped.trim_end())
}

/// Allège un JSON Schema d'outil MCP : retire les métadonnées sans valeur pour le modèle
/// (`$schema`, `$id`, `$comment`, `title`, `examples`) et compacte les descriptions de paramètres.
fn compact_schema(val: &Value) -> Value {
    match val {
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                if matches!(
                    k.as_str(),
                    "$schema" | "$id" | "$comment" | "title" | "examples"
                ) {
                    continue;
                }
                if k == "description" {
                    if let Some(s) = v.as_str() {
                        let c = compact_description(s, MAX_PARAM_DESC_CHARS);
                        if !c.is_empty() {
                            out.insert(k.clone(), Value::String(c));
                        }
                    }
                    continue;
                }
                out.insert(k.clone(), compact_schema(v));
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(compact_schema).collect()),
        other => other.clone(),
    }
}

fn parse_tools(list: &Value, out: &mut Vec<McpTool>) {
    let Some(arr) = list.get("tools").and_then(Value::as_array) else {
        return;
    };
    for t in arr {
        let Some(name) = t.get("name").and_then(Value::as_str) else {
            continue;
        };
        let raw_desc = t
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let raw_schema = t
            .get("inputSchema")
            .cloned()
            .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
        out.push(McpTool {
            name: name.to_string(),
            description: compact_description(raw_desc, MAX_TOOL_DESC_CHARS),
            input_schema: compact_schema(&raw_schema),
            read_only: t
                .pointer("/annotations/readOnlyHint")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        });
    }
}

async fn open(cfg: &ServerConfig) -> Result<(Conn, Vec<McpTool>), String> {
    if cfg.disabled {
        return Err("serveur désactivé (`disabled: true`)".to_string());
    }
    let cfg = &cfg.expanded();
    let (link, version) = match cfg.endpoint() {
        Some((url, token)) => (
            Link::Http(
                HttpLink::connect(url.clone(), token, &cfg.headers, cfg.is_sse(&url)).await?,
            ),
            "2024-11-05",
        ),
        None => {
            let mut child = spawn(cfg)?;
            let stdin = child.stdin.take().ok_or("stdin indisponible")?;
            let stdout = child.stdout.take().ok_or("stdout indisponible")?;
            (
                Link::Stdio(StdioLink {
                    _child: child,
                    stdin,
                    out: BufReader::new(stdout),
                }),
                "2024-11-05",
            )
        }
    };
    let mut c = Conn { link, next_id: 0 };
    c.request(
        "initialize",
        json!({
            "protocolVersion": version,
            "capabilities": {
                "tools": {}
            },
            "clientInfo": {"name": "francestudent-cli", "version": "3.8.0"}
        }),
        INIT_TIMEOUT,
    )
    .await?;
    let _ = c.notify("notifications/initialized").await;

    let mut tools = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let params = match &cursor {
            Some(cur) => json!({ "cursor": cur }),
            None => json!({}),
        };
        let r = c.request("tools/list", params, INIT_TIMEOUT).await?;
        parse_tools(&r, &mut tools);
        cursor = r
            .get("nextCursor")
            .and_then(Value::as_str)
            .map(String::from);
        if cursor.is_none() || tools.len() >= MAX_TOOLS {
            break;
        }
    }
    Ok((c, tools))
}

/// Texte lisible d'un résultat `tools/call`.
fn result_text(r: &Value) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(items) = r.get("content").and_then(Value::as_array) {
        for it in items {
            match it.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(t) = it.get("text").and_then(Value::as_str) {
                        parts.push(t.to_string());
                    }
                }
                Some("resource") => match it.pointer("/resource/text").and_then(Value::as_str) {
                    Some(t) => parts.push(t.to_string()),
                    None => parts.push("[ressource binaire]".to_string()),
                },
                Some(other) => parts.push(format!("[contenu {other} non affiché]")),
                None => {}
            }
        }
    }
    if parts.is_empty() {
        if let Some(s) = r.get("structuredContent") {
            return s.to_string();
        }
    }
    parts.join("\n")
}

struct Entry {
    tools: Vec<McpTool>,
    /// Partagé pour ne pas bloquer le reste du `Hub` pendant un appel long.
    conn: Arc<tokio::sync::Mutex<Conn>>,
}

/// Serveurs MCP connectés (état partagé de l'application).
#[derive(Default)]
pub struct Hub {
    conns: tokio::sync::Mutex<HashMap<String, Entry>>,
}

impl Hub {
    /// Lance / contacte le serveur et liste ses outils. Remplace une connexion existante.
    pub async fn connect(&self, name: &str, cfg: &ServerConfig) -> Result<Vec<McpTool>, String> {
        self.conns.lock().await.remove(name);
        let (conn, tools) = open(cfg).await?;
        self.conns.lock().await.insert(
            name.to_string(),
            Entry {
                tools: tools.clone(),
                conn: Arc::new(tokio::sync::Mutex::new(conn)),
            },
        );
        Ok(tools)
    }

    pub async fn disconnect(&self, name: &str) {
        self.conns.lock().await.remove(name);
    }

    /// Outils des serveurs connectés (noms tels qu'exposés au modèle).
    pub async fn exposed(&self) -> Vec<Exposed> {
        let g = self.conns.lock().await;
        let mut v: Vec<Exposed> = g
            .iter()
            .flat_map(|(server, e)| {
                e.tools.iter().map(move |t| Exposed {
                    name: exposed_name(server, &t.name),
                    server: server.clone(),
                    tool: t.clone(),
                })
            })
            .collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    /// Appelle l'outil `tool` du serveur `server`. `Ok((texte, is_error))`.
    pub async fn call_tool(
        &self,
        server: &str,
        tool: &str,
        args: Value,
    ) -> Result<(String, bool), String> {
        let conn = {
            let g = self.conns.lock().await;
            let e = g
                .get(server)
                .ok_or_else(|| format!("serveur « {server} » non connecté"))?;
            if !e.tools.iter().any(|t| t.name == tool) {
                return Err(format!("outil inconnu sur « {server} » : {tool}"));
            }
            e.conn.clone()
        };
        let args = if args.is_object() { args } else { json!({}) };
        let r = conn
            .lock()
            .await
            .request(
                "tools/call",
                json!({"name": tool, "arguments": args}),
                CALL_TIMEOUT,
            )
            .await?;
        let is_error = r.get("isError").and_then(Value::as_bool).unwrap_or(false);
        Ok((result_text(&r), is_error))
    }

    /// Charge tous les serveurs déclarés (app + dossier de travail du projet).
    pub fn load_all_servers(
        config_dir: &Path,
        workdir: Option<&Path>,
    ) -> BTreeMap<String, ServerConfig> {
        let mut map = BTreeMap::new();
 map.extend(crate::plugins::mcp_servers(config_dir));
        if let Ok(c) = load_config(config_dir) {
            map.extend(c);
        }
        if let Some(w) = workdir {
            for sub in [
                w.join("mcp.json"),
                w.join(".fscode").join("mcp.json"),
                w.join(".claude").join("mcp.json"),
            ] {
                if let Ok(text) = std::fs::read_to_string(&sub) {
                    if let Ok(parsed) = parse_config(&text) {
                        map.extend(parsed.mcp_servers);
                    }
                }
            }
        }
        map.retain(|_, s| !s.disabled);
        map
    }

    /// Récupère ou connecte à la volée un serveur MCP (par son nom, insensible à la casse).
    pub async fn get_or_connect(
        &self,
        config_dir: &Path,
        workdir: Option<&Path>,
        name: &str,
    ) -> Result<Vec<McpTool>, String> {
        {
            let g = self.conns.lock().await;
            if let Some(e) = g.get(name) {
                return Ok(e.tools.clone());
            }
            for (k, e) in g.iter() {
                if k.eq_ignore_ascii_case(name) {
                    return Ok(e.tools.clone());
                }
            }
        }
        let servers = Self::load_all_servers(config_dir, workdir);
        let (matched_name, cfg) = servers
            .into_iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("serveur MCP \"{name}\" introuvable dans mcp.json"))?;
        self.connect(&matched_name, &cfg).await
    }

    /// Appelle un outil en connectant automatiquement le serveur si besoin.
    pub async fn call_tool_auto(
        &self,
        config_dir: &Path,
        workdir: Option<&Path>,
        server: &str,
        tool: &str,
        args: Value,
    ) -> Result<(String, bool), String> {
        let _ = self.get_or_connect(config_dir, workdir, server).await?;
        let actual_server = {
            let g = self.conns.lock().await;
            g.keys()
                .find(|k| k.eq_ignore_ascii_case(server))
                .cloned()
                .unwrap_or_else(|| server.to_string())
        };
        self.call_tool(&actual_server, tool, args).await
    }

    /// Exécute une commande `fs-mcp ...` émise par l'IA via `run_command`.
    /// Syntaxes :
    ///   fs-mcp list
    ///   fs-mcp tools <serveur>
    ///   fs-mcp call <serveur> <outil> [json_args]
    #[allow(dead_code)]
    pub async fn execute_fs_mcp_command(
        &self,
        config_dir: &Path,
        workdir: Option<&Path>,
        raw_command: &str,
    ) -> (String, bool) {
        let raw = raw_command.trim();
        let rest = if let Some(stripped) = raw.strip_prefix("fs-mcp") {
            stripped.trim()
        } else {
            raw
        };

        if rest.is_empty() || rest.eq_ignore_ascii_case("list") {
            let servers = Self::load_all_servers(config_dir, workdir);
            if servers.is_empty() {
                return (
                    "Aucun serveur MCP local configuré (vérifie mcp.json).".to_string(),
                    false,
                );
            }
            let mut lines = Vec::new();
            for (name, s) in &servers {
                let target = if let Some(u) = &s.url {
                    u.clone()
                } else if !s.command.is_empty() {
                    format!("{} {}", s.command, s.args.join(" "))
                } else {
                    "inconnu".to_string()
                };
                lines.push(format!("- {name} ({target})"));
            }
            return (
                format!("Serveurs MCP locaux disponibles :\n{}", lines.join("\n")),
                false,
            );
        }

        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.len() >= 2 && parts[0].eq_ignore_ascii_case("tools") {
            let server_name = parts[1];
            return match self.get_or_connect(config_dir, workdir, server_name).await {
                Ok(tools) => {
                    if tools.is_empty() {
                        (
                            format!("Serveur \"{server_name}\" connecté mais aucun outil exposé."),
                            false,
                        )
                    } else {
                        let mut list = Vec::new();
                        for t in &tools {
                            let props: Vec<&str> = t
                                .input_schema
                                .get("properties")
                                .and_then(Value::as_object)
                                .map(|o| o.keys().map(String::as_str).collect())
                                .unwrap_or_default();
                            list.push(format!(
                                "- {}({}) : {}",
                                t.name,
                                props.join(", "),
                                t.description
                            ));
                        }
                        (
                            format!(
                                "Outils disponibles sur \"{server_name}\" :\n{}",
                                list.join("\n")
                            ),
                            false,
                        )
                    }
                }
                Err(e) => (format!("Erreur MCP ({server_name}) : {e}"), true),
            };
        }

        if parts.len() >= 3 && parts[0].eq_ignore_ascii_case("call") {
            let server_name = parts[1];
            let tool_name = parts[2];
            let raw_args = if let Some(idx) = rest.find(tool_name) {
                rest[idx + tool_name.len()..].trim()
            } else {
                ""
            };

            let parsed = parse_mcp_args(raw_args);
            return match parsed {
                Ok(args) => {
                    match self
                        .call_tool_auto(config_dir, workdir, server_name, tool_name, args)
                        .await
                    {
                        Ok((out, is_err)) => (out, is_err),
                        Err(e) => (
                            format!("Erreur appel MCP {server_name}.{tool_name} : {e}"),
                            true,
                        ),
                    }
                }
                Err(e) => (
                    format!(
                        "Arguments JSON invalides pour {server_name}.{tool_name} : {e}\nReçu : {raw_args}"
                    ),
                    true,
                ),
            };
        }

        (
            "Usage fs-mcp : \"fs-mcp list\" | \"fs-mcp tools <serveur>\" | \"fs-mcp call <serveur> <outil> '{...}'\"".to_string(),
            true,
        )
    }
}

/// Extrait les arguments JSON passés à `fs-mcp call`, en nettoyant les quotes du shell
/// et les antislashs éventuels.
pub fn parse_mcp_args(raw: &str) -> Result<Value, String> {
    let mut clean = raw.trim();
    if clean.is_empty() {
        return Ok(json!({}));
    }
    if (clean.starts_with('\'') && clean.ends_with('\''))
        || (clean.starts_with('"') && clean.ends_with('"'))
    {
        clean = &clean[1..clean.len() - 1];
    }
    clean = clean.trim();
    if clean.is_empty() {
        return Ok(json!({}));
    }
    match serde_json::from_str::<Value>(clean) {
        Ok(v) => Ok(v),
        Err(e) => {
            let unescaped = clean.replace(r#"\""#, "\"");
            serde_json::from_str::<Value>(&unescaped).map_err(|_| e.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_formats_parse() {
        let c = parse_config(
            r#"{"servers":{
     "a":{"command":"node","args":["x.js"]},
     "b":{"type":"http","serverUrl":"http://h/mcp"},
            "c":{"transport":"sse","httpUrl":"http://h/events"},
     "d":{"disabled":true}}}"#,
        )
        .unwrap();
        assert!(c.mcp_servers["c"].is_sse("http://h/events"));
        assert!(!c.mcp_servers["b"].is_sse("http://h/mcp"));
        assert_eq!(c.mcp_servers["b"].endpoint().unwrap().0, "http://h/mcp");
    }

    #[test]
    fn compacts_verbose_tool_metadata() {
        let long_desc = format!("Render video.   \n\n  {}", "x".repeat(400));
        let payload = json!({
            "tools": [{
                "name": "render_video",
                "description": long_desc,
                "inputSchema": {
                    "$schema": "http://json-schema.org/draft-07/schema#",
                    "title": "RenderVideoArgs",
                    "type": "object",
                    "examples": [{"fps": 30}],
                    "properties": {
                        "fps": {
                            "type": "number",
                            "title": "FPS",
                            "description": format!("Frames per second {}", "y".repeat(200)),
                            "enum": [24, 30, 60]
                        }
                    },
                    "required": ["fps"]
                }
            }]
        });
        let mut out = Vec::new();
        parse_tools(&payload, &mut out);
        assert_eq!(out.len(), 1);
        assert!(out[0].description.chars().count() <= MAX_TOOL_DESC_CHARS);
        assert!(out[0].description.ends_with('…'));
        assert!(out[0].input_schema.get("$schema").is_none());
        assert!(out[0].input_schema.get("title").is_none());
        assert!(out[0].input_schema.get("examples").is_none());
        let fps = &out[0].input_schema["properties"]["fps"];
        assert!(fps.get("title").is_none());
        assert_eq!(fps["enum"], json!([24, 30, 60]));
        assert!(fps["description"].as_str().unwrap().chars().count() <= MAX_PARAM_DESC_CHARS);
        assert_eq!(out[0].input_schema["required"], json!(["fps"]));
    }
}
