//! Client HTTP non officiel pour l'API France Student.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use reqwest::{Body, Method};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub mod routes;

static BOUNDARY_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Options supplémentaires pour une requête de génération (`stream_chat` / `send_message`).
#[derive(Debug, Clone, Default)]
pub struct StreamOptions {
    pub images: Vec<String>,
    pub files: Vec<String>,
    pub enable_web_search: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("requête HTTP : {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON invalide : {0}")]
    Json(#[from] serde_json::Error),
    #[error("jeton de connexion invalide (caractères interdits dans un header)")]
    BadKey,
    #[error("HTTP {status} : {body}")]
    Status { status: u16, body: String },
    #[error("flux SSE : {0}")]
    Stream(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Un agent (= un modèle) renvoyé par `GET /chats/agents`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub id: i64,
    pub name: String,
    pub namespace: Option<String>,
    pub description: Option<String>,
    pub model: String,
    pub model_provider: String,
    pub is_active: bool,
    pub is_default: bool,
    pub context_window: Option<u64>,
    pub compact_threshold: Option<u64>,
}

/// Compteurs de tokens d'une réponse.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TokenUsage {
    pub prompt_token_count: Option<u64>,
    pub candidates_token_count: Option<u64>,
    pub total_token_count: Option<u64>,
    pub reasoning_token_count: Option<u64>,
}

/// Appel d'outil client demandé par le modèle (`completed.message.toolCalls`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolCall {
    pub call_id: String,
    pub name: String,
    /// Chaîne JSON ou objet selon le fournisseur : à décoder côté appelant.
    pub arguments: Value,
}

/// Résultat d'outil renvoyé au serveur (`toolResults` de `POST /chats/{id}`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResult {
    pub call_id: String,
    pub output: String,
    pub is_error: bool,
}

/// Message final d'un flux (événement `completed`).
#[derive(Debug, Clone)]
pub struct CompletedMessage {
    pub id: String,
    pub content: String,
    pub token_usage: Option<TokenUsage>,
    pub tool_calls: Vec<ToolCall>,
}

/// Erreur terminale envoyée dans le flux (événement `error`).
#[derive(Debug, Clone)]
pub struct StreamError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

impl std::fmt::Display for StreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} : {}", self.code, self.message)
    }
}

/// Événement utile d'un flux de chat.
/// Tout le reste (dont la liste d'outils MCP, énorme) devient `Other`.
#[derive(Debug, Clone)]
pub enum ChatEvent {
    Opened {
        chat_id: String,
    },
    Title {
        chat_id: String,
        title: String,
    },
    Delta(String),
    ReasoningDelta(String),
    Completed {
        chat_id: String,
        message: CompletedMessage,
    },
    Error(StreamError),
    Cancelled {
        chat_id: String,
    },
    /// Résumé lisible d'un événement MCP (liste d'outils, appel d'outil).
    Mcp(String),
    /// Notice d'information (recherche web, statut intermédiaire).
    Notice(String),
    Other(String),
}

fn id_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

fn tool_calls(v: &Value) -> Vec<ToolCall> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|t| serde_json::from_value::<ToolCall>(t.clone()).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Texte tronqué à `max` caractères (pour les notices).
fn short(v: &Value, max: usize) -> String {
    let s = id_str(v);
    if s.chars().count() > max {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    } else {
        s
    }
}

/// Résumé d'un événement MCP (liste d'outils ou appel). `None` si autre chose.
fn mcp_summary(ty: &str, v: &Value) -> Option<String> {
    let item = &v["item"];
    match (ty, item["type"].as_str()) {
        ("response.output_item.done", Some("mcp_list_tools")) => {
            let label = text(&item["server_label"]);
            let err = short(&item["error"], 300);
            if err.is_empty() {
                let n = item["tools"].as_array().map_or(0, |a| a.len());
                Some(format!("MCP « {label} » : {n} outils visibles par le modèle."))
            } else {
                Some(format!("MCP « {label} » : liste d'outils en échec — {err}"))
            }
        }
        ("response.output_item.done", Some("mcp_call")) => {
            let name = text(&item["name"]);
            let status = text(&item["status"]);
            let err = short(&item["error"], 300);
            let out = short(&item["output"], 200);
            let mut s = format!("MCP appel {name} : {status}");
            if !err.is_empty() {
                s.push_str(&format!(" — erreur : {err}"));
            } else if !out.is_empty() {
                s.push_str(&format!(" — sortie : {out}"));
            }
            Some(s)
        }
        ("response.mcp_call.failed", _) => Some("MCP : un appel d'outil a échoué.".to_string()),
        _ => None,
    }
}

/// Extrait un statut lisible pour les événements SSE liés à la recherche web.
fn parse_web_search(ty: &str, v: &Value) -> Option<String> {
    match ty {
        "response.web_search_call.in_progress" => Some("Recherche web en cours…".to_string()),
        "response.web_search_call.searching" => Some("Recherche web active…".to_string()),
        "response.web_search_call.completed" => Some("Recherche web terminée.".to_string()),
        "response.output_item.done" => {
            let item = &v["item"];
            if item["type"].as_str() == Some("web_search_call") {
                let status = item["status"].as_str().unwrap_or("completed");
                Some(format!("Recherche web ({status})."))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn sanitize_header_value(input: &str) -> String {
    input
        .chars()
        .filter(|&c| c != '\r' && c != '\n' && c != '"' && c != '\\')
        .collect()
}

fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            _ => {
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}

fn build_multipart_file(
    field_name: &str,
    filename: &str,
    content_type: &str,
    bytes: &[u8],
) -> (String, Vec<u8>) {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = BOUNDARY_COUNTER.fetch_add(1, Ordering::Relaxed);
    let boundary = format!("----fsdesktop{nanos:x}{seq:x}");
    let safe_field = sanitize_header_value(field_name);
    let safe_name = sanitize_header_value(filename);
    let safe_ct = if content_type.trim().is_empty() {
        "application/octet-stream".to_string()
    } else {
        sanitize_header_value(content_type)
    };

    let head = format!(
        "--{boundary}\r\n\
         Content-Disposition: form-data; name=\"{safe_field}\"; filename=\"{safe_name}\"\r\n\
         Content-Type: {safe_ct}\r\n\r\n"
    );
    let tail = format!("\r\n--{boundary}--\r\n");

    let mut body = Vec::with_capacity(head.len() + bytes.len() + tail.len());
    body.extend_from_slice(head.as_bytes());
    body.extend_from_slice(bytes);
    body.extend_from_slice(tail.as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

fn parse_event(ty: &str, v: &Value) -> ChatEvent {
    match ty {
        "stream.open" => ChatEvent::Opened {
            chat_id: id_str(&v["chat_id"]),
        },
        "chat.title" => ChatEvent::Title {
            chat_id: id_str(&v["chat_id"]),
            title: text(&v["title"]),
        },
        "response.output_text.delta" => ChatEvent::Delta(text(&v["delta"])),
        "response.reasoning_summary_text.delta" => ChatEvent::ReasoningDelta(text(&v["delta"])),
        "completed" => {
            let m = &v["message"];
            ChatEvent::Completed {
                chat_id: id_str(&v["chat_id"]),
                message: CompletedMessage {
                    id: id_str(&m["id"]),
                    content: text(&m["content"]),
                    token_usage: serde_json::from_value(m["tokenUsage"].clone()).ok(),
                    tool_calls: tool_calls(&m["toolCalls"]),
                },
            }
        }
        "error" => ChatEvent::Error(StreamError {
            code: text(&v["code"]),
            message: text(&v["message"]),
            retryable: v["retryable"].as_bool().unwrap_or(false),
        }),
        "cancelled" => ChatEvent::Cancelled {
            chat_id: id_str(&v["chat_id"]),
        },
        other => {
            if let Some(s) = parse_web_search(other, v) {
                ChatEvent::Notice(s)
            } else if let Some(s) = mcp_summary(other, v) {
                ChatEvent::Mcp(s)
            } else {
                ChatEvent::Other(other.to_string())
            }
        }
    }
}

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base: String,
}

impl Client {
    /// `base` = URL de base de l'API (`https://` ajouté si absent).
    /// `key` = clé API (jamais loguée).
    pub fn new(base: &str, key: &str) -> Result<Self> {
        let mut auth =
            HeaderValue::from_str(&format!("Bearer {key}")).map_err(|_| Error::BadKey)?;
        auth.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, auth);
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .connect_timeout(Duration::from_secs(15))
            .build()?;
        let base = base.trim().trim_end_matches('/');
        let base = if base.starts_with("http://") || base.starts_with("https://") {
            base.to_string()
        } else {
            format!("https://{base}")
        };
        Ok(Self { http, base })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    /// Requête JSON générique. Corps vide => `Value::Null`.
    pub async fn request(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value> {
        let mut rb = self.http.request(method, self.url(path));
        if let Some(b) = body {
            rb = rb.json(&b);
        }
        let resp = rb.send().await?;
        let status = resp.status();
        let raw = resp.text().await?;
        if !status.is_success() {
            return Err(Error::Status {
                status: status.as_u16(),
                body: raw,
            });
        }
        if raw.trim().is_empty() {
            return Ok(Value::Null);
        }
        Ok(serde_json::from_str(&raw)?)
    }

    /// GET `path` (ex. `/user/usage`) et décode le JSON en `T`.
    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let v = self.request(Method::GET, path, None).await?;
        Ok(serde_json::from_value(v)?)
    }

    /// GET `path` en JSON brut.
    pub async fn get_json(&self, path: &str) -> Result<Value> {
        self.request(Method::GET, path, None).await
    }

    /// Requête JSON avec une méthode choisie par nom (`POST`, `PATCH`, `PUT`, `DELETE`, sinon `GET`).
    pub async fn send_json(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value> {
        let m = match method.to_ascii_uppercase().as_str() {
            "POST" => Method::POST,
            "PATCH" => Method::PATCH,
            "PUT" => Method::PUT,
            "DELETE" => Method::DELETE,
            _ => Method::GET,
        };
        self.request(m, path, body).await
    }

    /// Appelle une route du catalogue (`routes::ROUTES`). Les routes multipart, SSE et binaires sont refusées.
    /// Le chemin est reconstruit avec des segments encodés ; paramètres de requête et en-têtes
    /// non documentés refusés. Corps vide => `Value::Null` ; texte non JSON => `Value::String`.
    pub async fn call_route(
        &self,
        route: &routes::Route,
        path_params: &std::collections::BTreeMap<String, String>,
 query: &[(String, String)],
        headers: &[(String, String)],
        body: Option<Value>,
    ) -> std::result::Result<Value, String> {
        if !route.callable() {
     return Err(format!("route « {} » non appelable en JSON (flux, fichier ou binaire)", route.id));
        }
        if body.is_some() && !route.has_body {
     return Err(format!("la route « {} » n'accepte pas de corps", route.id));
        }
        if route.has_body && body.is_none() {
     return Err(format!("la route « {} » exige un corps JSON", route.id));
        }
        route.check_extras(query, headers)?;
        let path = route.build_path(path_params)?;
        let method = Method::from_bytes(route.method.as_bytes()).map_err(|e| e.to_string())?;
        let mut rb = self.http.request(method, self.url(&path));
        if !query.is_empty() {
     rb = rb.query(query);
        }
        for (k, v) in headers {
     let mut hv = HeaderValue::from_str(v).map_err(|_| format!("valeur d'en-tête invalide : {k}"))?;
     hv.set_sensitive(true);
            rb = rb.header(k.as_str(), hv);
        }
        if let Some(b) = body {
     rb = rb.json(&b);
        }
 let resp = rb.send().await.map_err(|e| Error::Http(e).to_string())?;
 let status = resp.status();
        let raw = resp.text().await.map_err(|e| Error::Http(e).to_string())?;
        if !status.is_success() {
     return Err(Error::Status { status: status.as_u16(), body: raw }.to_string());
        }
        if raw.trim().is_empty() {
     return Ok(Value::Null);
        }
        Ok(serde_json::from_str(&raw).unwrap_or(Value::String(raw)))
    }

    pub async fn agents(&self) -> Result<Vec<Agent>> {
        self.get("/chats/agents").await
    }

    /// `GET /user/usage` en JSON brut (budget chat, limites).
    pub async fn usage(&self) -> Result<Value> {
        self.request(Method::GET, "/user/usage", None).await
    }

    /// `GET /chats` : historique, du plus récent au plus ancien.
    pub async fn list_chats(&self, limit: u32, cursor: Option<&str>) -> Result<Value> {
        let mut path = format!("/chats?limit={limit}&sortedBy=lastMessageAt");
        if let Some(c) = cursor {
            path.push_str("&cursor_chat_id=");
            path.push_str(c);
        }
        self.request(Method::GET, &path, None).await
    }

    /// `GET /chats/{id}` : détail + messages.
    pub async fn get_chat(&self, chat_id: &str) -> Result<Value> {
        self.request(Method::GET, &format!("/chats/{chat_id}"), None)
            .await
    }

    /// `GET /chats/{id}/context` : occupation du contexte.
    pub async fn context(&self, chat_id: &str) -> Result<Value> {
        self.request(Method::GET, &format!("/chats/{chat_id}/context"), None)
            .await
    }

    /// `POST /chats/{id}/context` : compaction à la demande.
    pub async fn compact(&self, chat_id: &str) -> Result<Value> {
        self.request(Method::POST, &format!("/chats/{chat_id}/context"), None)
            .await
    }

    /// `POST /chats/{id}/stream/cancel` (idempotent).
    pub async fn cancel(&self, chat_id: &str) -> Result<()> {
        self.request(
            Method::POST,
            &format!("/chats/{chat_id}/stream/cancel"),
            None,
        )
        .await?;
        Ok(())
    }

    pub async fn delete_chat(&self, chat_id: &str) -> Result<()> {
        self.request(Method::DELETE, &format!("/chats/{chat_id}"), None)
            .await?;
        Ok(())
    }

    pub async fn rename_chat(&self, chat_id: &str, title: &str) -> Result<()> {
        self.request(
            Method::PATCH,
            &format!("/chats/{chat_id}"),
            Some(json!({ "title": title })),
        )
        .await?;
        Ok(())
    }

    /// Épingle ou désépingle une conversation (`PUT` / `DELETE /chats/{id}/pins`).
    pub async fn pin_chat(&self, chat_id: &str, pinned: bool) -> Result<()> {
        let method = if pinned { Method::PUT } else { Method::DELETE };
        self.request(method, &format!("/chats/{chat_id}/pins"), None)
            .await?;
        Ok(())
    }

    /// Archive une conversation (`PUT /chats/{id}/archive`).
    pub async fn archive_chat(&self, chat_id: &str) -> Result<()> {
        self.request(Method::PUT, &format!("/chats/{chat_id}/archive"), None)
            .await?;
        Ok(())
    }

    /// Recherche des conversations (`GET /chats/search?query=...&limit=...`).
    pub async fn search_chats(&self, query: &str, limit: u32) -> Result<Value> {
        let path = format!("/chats/search?query={}&limit={limit}", percent_encode(query));
        self.request(Method::GET, &path, None).await
    }

    /// Édite un message en créant un fork (`PATCH /chats/{id}/messages/{msgId}`).
    pub async fn fork_edit_message(
        &self,
        chat_id: &str,
        message_id: &str,
        content: &str,
    ) -> Result<Value> {
        self.request(
            Method::PATCH,
            &format!("/chats/{chat_id}/messages/{message_id}"),
            Some(json!({ "content": content })),
        )
        .await
    }

    /// Liste les messages épinglés/sauvegardés (`GET /user/@me/saved-messages`).
    pub async fn list_saved_messages(&self, limit: u32, cursor: Option<&str>) -> Result<Value> {
        let mut path = format!("/user/@me/saved-messages?limit={limit}");
        if let Some(c) = cursor {
            if !c.is_empty() {
                path.push_str("&cursor=");
                path.push_str(&percent_encode(c));
            }
        }
        self.request(Method::GET, &path, None).await
    }

    /// Épingle/sauvegarde un message (`PUT /user/@me/saved-messages/{chatId}/{messageId}`).
    pub async fn save_message(&self, chat_id: &str, message_id: &str) -> Result<Value> {
        self.request(
            Method::PUT,
            &format!("/user/@me/saved-messages/{chat_id}/{message_id}"),
            None,
        )
        .await
    }

    /// Retire un message épinglé/sauvegardé (`DELETE /user/@me/saved-messages/{chatId}/{messageId}`).
    pub async fn unsave_message(&self, chat_id: &str, message_id: &str) -> Result<()> {
        self.request(
            Method::DELETE,
            &format!("/user/@me/saved-messages/{chat_id}/{message_id}"),
            None,
        )
        .await?;
        Ok(())
    }

    /// Liste les conversations archivées (`GET /chats/archived?limit=...`).
    pub async fn list_archived_chats(&self, limit: u32) -> Result<Value> {
        self.request(Method::GET, &format!("/chats/archived?limit={limit}"), None)
            .await
    }

    /// Restaure une conversation archivée (`PUT /chats/{chatId}/restore`).
    pub async fn restore_chat(&self, chat_id: &str) -> Result<()> {
        self.request(Method::PUT, &format!("/chats/{chat_id}/restore"), None)
            .await?;
        Ok(())
    }

    /// Récupère les préférences de notifications (`GET /user/notification-preferences`).
    pub async fn get_notification_preferences(&self) -> Result<Value> {
        self.request(Method::GET, "/user/notification-preferences", None)
            .await
    }

    /// Met à jour les préférences de notifications (`PATCH /user/notification-preferences`).
    pub async fn update_notification_preferences(&self, prefs: Value) -> Result<Value> {
        self.request(Method::PATCH, "/user/notification-preferences", Some(prefs))
            .await
    }

    /// Recupere les instructions personnalisees (`GET /user/instructions`).
    pub async fn get_user_instructions(&self) -> Result<Value> {
        self.request(Method::GET, "/user/instructions", None).await
    }

    /// Met a jour les instructions personnalisees (`PATCH /user/instructions`, partiel).
    pub async fn update_user_instructions(&self, body: Value) -> Result<Value> {
        self.request(Method::PATCH, "/user/instructions", Some(body))
            .await
    }

    /// Envoie un fichier en `multipart/form-data` sur `path`.
    async fn upload_multipart(
        &self,
        path: &str,
        filename: &str,
        content_type: &str,
        bytes: Vec<u8>,
    ) -> Result<Value> {
        let (ct_header, body) = build_multipart_file("file", filename, content_type, &bytes);
        let header_val = HeaderValue::from_str(&ct_header).map_err(|e| Error::Status {
            status: 400,
            body: format!("En-tête Content-Type invalide : {e}"),
        })?;
        let resp = self
            .http
            .post(self.url(path))
            .header(CONTENT_TYPE, header_val)
            .body(Body::from(body))
            .send()
            .await?;
        let status = resp.status();
        let raw = resp.text().await?;
        if !status.is_success() {
            return Err(Error::Status {
                status: status.as_u16(),
                body: raw,
            });
        }
        if raw.trim().is_empty() {
            return Ok(Value::Null);
        }
        Ok(serde_json::from_str(&raw)?)
    }

    const FILES_V2_UPLOAD_PATH: &'static str =
        "/filesv2?expires_after%5Banchor%5D=created_at&expires_after%5Bseconds%5D=3600";

    /// Upload d'image pour un chat (`POST /filesv2`, retourne `id`, `filename`, …).
    pub async fn upload_image(
        &self,
        filename: &str,
        content_type: &str,
        bytes: Vec<u8>,
    ) -> Result<Value> {
        self.upload_file_v2(filename, content_type, bytes).await
    }

    /// Upload de fichier / document pour un chat (`POST /filesv2`, retourne `id`, `filename`, …).
    pub async fn upload_file_v2(
        &self,
        filename: &str,
        content_type: &str,
        bytes: Vec<u8>,
    ) -> Result<Value> {
        self.upload_multipart(Self::FILES_V2_UPLOAD_PATH, filename, content_type, bytes)
            .await
    }

    /// Crée un chat (`POST /chats/stream`) et appelle `on_event` à chaque événement.
    /// Attention : crée un vrai chat (limite 50 nouveaux chats/jour).
    /// `client_tools` = noms d'outils du catalogue fixe du serveur (liste de chaînes,
    /// peut être vide). Un objet ou un nom inconnu donne une erreur 400.
    pub async fn stream_chat(
        &self,
        agent_id: i64,
        content: &str,
        client_tools: &[Value],
        on_event: impl FnMut(ChatEvent),
    ) -> Result<()> {
        self.stream_chat_with_options(
            agent_id,
            content,
            client_tools,
            &StreamOptions::default(),
            on_event,
        )
        .await
    }

    /// Variante de `stream_chat` acceptant images, fichiers v2 et recherche web.
    pub async fn stream_chat_with_options(
        &self,
        agent_id: i64,
        content: &str,
        client_tools: &[Value],
        opts: &StreamOptions,
        on_event: impl FnMut(ChatEvent),
    ) -> Result<()> {
        self.stream_chat_with_options_id(agent_id, content, client_tools, opts, on_event)
            .await
            .map(|_| ())
    }

    /// Comme `stream_chat_with_options`, en renvoyant le dernier `Last-Event-ID`.
    pub async fn stream_chat_with_options_id(
        &self,
        agent_id: i64,
        content: &str,
        client_tools: &[Value],
        opts: &StreamOptions,
        on_event: impl FnMut(ChatEvent),
    ) -> Result<Option<String>> {
        let mut body = json!({
            "agentId": agent_id,
            "initialContent": content,
        });
        if !opts.images.is_empty() {
            body["images"] = json!(opts.images);
        }
        if !opts.files.is_empty() {
            body["files"] = json!(opts.files);
        }
        if opts.enable_web_search {
            body["enableWebSearch"] = json!(true);
        }
        if !client_tools.is_empty() {
            body["clientTools"] = json!(client_tools);
        }
        self.stream_with_id("/chats/stream", body, on_event).await
    }

    /// Répond dans un chat existant (`POST /chats/{id}`).
    /// `agent_id` change le modèle pour cette réponse et les suivantes.
    /// `tool_results` renvoie les résultats d'outils clients du tour précédent :
    /// dans ce cas `content` est omis s'il est vide et `clientTools` n'est pas
    /// renvoyé (le serveur garde les outils du tour qui a fait les appels).
    pub async fn send_message(
        &self,
        chat_id: &str,
        content: &str,
        agent_id: Option<i64>,
        client_tools: &[Value],
        tool_results: &[ToolResult],
        on_event: impl FnMut(ChatEvent),
    ) -> Result<()> {
        self.send_message_with_options(
            chat_id,
            content,
            agent_id,
            client_tools,
            tool_results,
            &StreamOptions::default(),
            on_event,
        )
        .await
    }

    /// Variante de `send_message` acceptant images, fichiers v2 et recherche web.
    #[allow(clippy::too_many_arguments)]
    pub async fn send_message_with_options(
        &self,
        chat_id: &str,
        content: &str,
        agent_id: Option<i64>,
        client_tools: &[Value],
        tool_results: &[ToolResult],
        opts: &StreamOptions,
        on_event: impl FnMut(ChatEvent),
    ) -> Result<()> {
        self.send_message_with_options_id(
            chat_id,
            content,
            agent_id,
            client_tools,
            tool_results,
            opts,
            on_event,
        )
        .await
        .map(|_| ())
    }

    /// Comme `send_message_with_options`, en renvoyant le dernier `Last-Event-ID`.
    #[allow(clippy::too_many_arguments)]
    pub async fn send_message_with_options_id(
        &self,
        chat_id: &str,
        content: &str,
        agent_id: Option<i64>,
        client_tools: &[Value],
        tool_results: &[ToolResult],
        opts: &StreamOptions,
        on_event: impl FnMut(ChatEvent),
    ) -> Result<Option<String>> {
        let mut body = json!({});
        if !content.is_empty() {
            body["content"] = json!(content);
        }
        if let Some(id) = agent_id {
            body["agentId"] = json!(id);
        }
        if opts.enable_web_search {
            body["enableWebSearch"] = json!(true);
        }
        if tool_results.is_empty() {
            if !opts.images.is_empty() {
                body["images"] = json!(opts.images);
            }
            if !opts.files.is_empty() {
                body["files"] = json!(opts.files);
            }
            if !client_tools.is_empty() {
                body["clientTools"] = json!(client_tools);
            }
        } else {
            body["toolResults"] = json!(tool_results);
        }
        self.stream_with_id(&format!("/chats/{chat_id}"), body, on_event)
            .await
    }

    /// Ré-attache le flux d'une génération en cours (`GET /chats/{id}/stream`).
    /// `last_event_id` évite de rejouer les événements déjà reçus.
    pub async fn resume_stream(
        &self,
        chat_id: &str,
        last_event_id: Option<&str>,
        on_event: impl FnMut(ChatEvent),
    ) -> Result<Option<String>> {
        let mut request = self
            .http
            .get(self.url(&format!("/chats/{chat_id}/stream")))
            .header(ACCEPT, "text/event-stream");
        if let Some(id) = last_event_id {
            if !id.is_empty() {
                request = request.header("Last-Event-ID", id);
            }
        }
        let resp = request.send().await?;
        let status = resp.status();
        if !status.is_success() {
            let raw = resp.text().await?;
            return Err(Error::Status {
                status: status.as_u16(),
                body: raw,
            });
        }
        self.read_sse(resp, on_event).await
    }

    /// `GET /chats/{id}/usage` : consommation de contexte du chat.
    pub async fn chat_usage(&self, chat_id: &str) -> Result<Value> {
        self.request(Method::GET, &format!("/chats/{chat_id}/usage"), None)
            .await
    }

    /// POST + lecture SSE. Garantit un événement terminal : si le flux se coupe
    /// sans `completed`/`error`/`cancelled`, émet `Error{STREAM_ENDED}`.
    async fn stream_with_id(
        &self,
        path: &str,
        body: Value,
        on_event: impl FnMut(ChatEvent),
    ) -> Result<Option<String>> {
        let resp = self
            .http
            .post(self.url(path))
            .header(ACCEPT, "text/event-stream")
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let raw = resp.text().await?;
            return Err(Error::Status {
                status: status.as_u16(),
                body: raw,
            });
        }
        self.read_sse(resp, on_event).await
    }

    async fn read_sse(
        &self,
        resp: reqwest::Response,
        mut on_event: impl FnMut(ChatEvent),
    ) -> Result<Option<String>> {
        let mut finished = false;
        let mut last_id: Option<String> = None;
        let events = resp.bytes_stream().eventsource();
        futures_util::pin_mut!(events);
        while let Some(ev) = events.next().await {
            let ev = ev.map_err(|e| Error::Stream(e.to_string()))?;
            if !ev.id.is_empty() {
                last_id = Some(ev.id.clone());
            }
            let Ok(v) = serde_json::from_str::<Value>(&ev.data) else {
                continue; // keep-alive ou ligne non JSON
            };
            let ty = v["type"].as_str().unwrap_or(&ev.event);
            let event = parse_event(ty, &v);
            if matches!(
                event,
                ChatEvent::Completed { .. } | ChatEvent::Error(_) | ChatEvent::Cancelled { .. }
            ) {
                finished = true;
            }
            on_event(event);
        }
        if !finished {
            on_event(ChatEvent::Error(StreamError {
                code: "STREAM_ENDED".to_string(),
                message: "Le flux s'est arrêté avant la fin de la réponse.".to_string(),
                retryable: true,
            }));
        }
        Ok(last_id)
    }
}
