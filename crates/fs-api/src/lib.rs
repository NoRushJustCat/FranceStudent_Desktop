//! Client HTTP non officiel pour l'API France Student.

use std::time::Duration;

use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION};
use reqwest::Method;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("requête HTTP : {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON invalide : {0}")]
    Json(#[from] serde_json::Error),
    #[error("clé API invalide (caractères interdits dans un header)")]
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
        other => match mcp_summary(other, v) {
            Some(s) => ChatEvent::Mcp(s),
            None => ChatEvent::Other(other.to_string()),
        },
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
        let mut body = json!({
            "agentId": agent_id,
            "initialContent": content,
        });
        if !client_tools.is_empty() {
            body["clientTools"] = json!(client_tools);
        }
        self.stream("/chats/stream", body, on_event).await
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
        let mut body = json!({});
        if !content.is_empty() {
            body["content"] = json!(content);
        }
        if let Some(id) = agent_id {
            body["agentId"] = json!(id);
        }
        if tool_results.is_empty() {
            if !client_tools.is_empty() {
                body["clientTools"] = json!(client_tools);
            }
        } else {
            body["toolResults"] = json!(tool_results);
        }
        self.stream(&format!("/chats/{chat_id}"), body, on_event)
            .await
    }

    /// POST + lecture SSE. Garantit un événement terminal : si le flux se coupe
    /// sans `completed`/`error`/`cancelled`, émet `Error{STREAM_ENDED}`.
    async fn stream(
        &self,
        path: &str,
        body: Value,
        mut on_event: impl FnMut(ChatEvent),
    ) -> Result<()> {
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

        let mut finished = false;
        let events = resp.bytes_stream().eventsource();
        futures_util::pin_mut!(events);
        while let Some(ev) = events.next().await {
            let ev = ev.map_err(|e| Error::Stream(e.to_string()))?;
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
        Ok(())
    }
}
