//! Commandes `mcp …` (exécutées via l'outil client `run_command`) et bloc de contexte
//! joint au premier message d'un chat, pour que le modèle sache qu'il dispose des
//! serveurs MCP locaux. Même principe que le dépôt `fs-mcp-cli` : aucun tunnel,
//! tout s'exécute ici, en local (via `Hub`, voir `mcp.rs`).

use std::path::Path;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::time::timeout;

use crate::mcp::{parse_mcp_args, Hub};

/// Délai max pour joindre un serveur pendant la construction du contexte.
const CONTEXT_CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// Taille max (en caractères) de la liste serveurs/outils jointe au message.
const CONTEXT_MAX_CHARS: usize = 8000;
/// Idem en mode économie (liste sans descriptions, donc nettement plus courte).
const CONTEXT_MAX_CHARS_ECO: usize = 2500;

/// Début du bloc de contexte joint au premier message.
const CONTEXT_PREFIX: &str = "[Contexte client]";
/// Séparateur entre le bloc de contexte et le vrai message de l'utilisateur.
const CONTEXT_SEP: &str = "\n\n---\n";

const USAGE: &str =
    "Usage : mcp list | mcp tools <serveur> [outil] | mcp call <serveur> <outil> {json}";

/// Si `cmd` est une commande MCP (`mcp …` ou `fs-mcp …`), renvoie ce qui suit le préfixe.
pub fn mcp_rest(cmd: &str) -> Option<&str> {
    let cmd = cmd.trim();
    for prefix in ["fs-mcp", "mcp"] {
        if let Some(rest) = cmd.strip_prefix(prefix) {
            if rest.is_empty() || rest.starts_with(char::is_whitespace) {
                return Some(rest.trim());
            }
        }
    }
    None
}

/// Si `s` commence par le bloc de contexte joint par l'application, renvoie le message
/// de l'utilisateur qui le suit.
fn strip_context_text(s: &str) -> Option<&str> {
    let t = s.trim_start();
    if !t.starts_with(CONTEXT_PREFIX) {
        return None;
    }
    t.find(CONTEXT_SEP).map(|i| &t[i + CONTEXT_SEP.len()..])
}

/// Retire, dans tout un JSON (chat, liste de chats…), le bloc « [Contexte client] » que
/// l'application a joint au premier message : il sert au modèle, pas à l'affichage.
pub fn strip_context(v: &mut Value) {
    match v {
        Value::String(s) => {
            let cleaned = strip_context_text(s).map(str::to_string);
            if let Some(c) = cleaned {
                *s = c;
            }
        }
        Value::Array(a) => {
            for x in a {
                strip_context(x);
            }
        }
        Value::Object(o) => {
            for x in o.values_mut() {
                strip_context(x);
            }
        }
        _ => {}
    }
}

/// Premier mot de `s` et le reste (sans espaces de tête).
fn next_word(s: &str) -> (&str, &str) {
    let s = s.trim_start();
    match s.find(char::is_whitespace) {
        Some(i) => (&s[..i], s[i..].trim_start()),
        None => (s, ""),
    }
}

/// Noms des paramètres requis d'un schéma d'outil.
fn required_params(schema: &Value) -> Vec<&str> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// Première ligne de `s`, tronquée à `max` caractères.
fn first_line(s: &str, max: usize) -> String {
    s.lines()
        .next()
        .unwrap_or("")
        .trim()
        .chars()
        .take(max)
        .collect()
}

/// Retire les valeurs `null` d'un JSON (inutiles pour le modèle).
fn prune_nulls(v: &mut Value) {
    match v {
        Value::Object(o) => {
            o.retain(|_, x| !x.is_null());
            for x in o.values_mut() {
                prune_nulls(x);
            }
        }
        Value::Array(a) => {
            for x in a {
                prune_nulls(x);
            }
        }
        _ => {}
    }
}

/// Mode économie : si `s` est un JSON objet/tableau, le renvoie sur une seule ligne,
/// sans espaces ni valeurs `null`. Sinon, renvoie `s` tel quel.
fn compact_output(s: &str) -> String {
    match serde_json::from_str::<Value>(s.trim()) {
        Ok(mut v @ (Value::Object(_) | Value::Array(_))) => {
            prune_nulls(&mut v);
            serde_json::to_string(&v).unwrap_or_else(|_| s.to_string())
        }
        _ => s.to_string(),
    }
}

impl Hub {
    /// Bloc « [Contexte client] » à joindre au premier message d'un chat : liste les
    /// serveurs MCP locaux et leurs outils (données brutes, sans consigne).
    /// `None` si aucun serveur n'a pu être joint.
    /// En mode économie (`eco`) : en-tête minimal, pas de descriptions, les paramètres
    /// requis par tous les outils d'un serveur sont indiqués une seule fois.
    pub async fn context_text(
        &self,
        config_dir: &Path,
        workdir: Option<&Path>,
        eco: bool,
    ) -> Option<String> {
        let max_chars = if eco {
            CONTEXT_MAX_CHARS_ECO
        } else {
            CONTEXT_MAX_CHARS
        };
        let servers = Hub::load_all_servers(config_dir, workdir);
        let mut body = String::new();
        for name in servers.keys() {
            let joined = timeout(
                CONTEXT_CONNECT_TIMEOUT,
                self.get_or_connect(config_dir, workdir, name),
            )
            .await;
            let tools = match joined {
                Ok(Ok(t)) if !t.is_empty() => t,
                _ => continue,
            };
            if eco {
                let reqs: Vec<Vec<&str>> = tools
                    .iter()
                    .map(|t| required_params(&t.input_schema))
                    .collect();
                let common: Vec<&str> = if reqs.len() >= 3 {
                    reqs[0]
                        .iter()
                        .copied()
                        .filter(|p| reqs.iter().all(|r| r.contains(p)))
                        .collect()
                } else {
                    Vec::new()
                };
                if common.is_empty() {
                    body.push_str(&format!("Serveur \"{name}\" :\n"));
                } else {
                    body.push_str(&format!(
                        "Serveur \"{name}\" (requis pour tous : {}) :\n",
                        common.join(",")
                    ));
                }
                for (t, r) in tools.iter().zip(&reqs) {
                    let own: Vec<&str> =
                        r.iter().copied().filter(|p| !common.contains(p)).collect();
                    if own.is_empty() {
                        body.push_str(&format!("{}\n", t.name));
                    } else {
                        body.push_str(&format!("{}({})\n", t.name, own.join(",")));
                    }
                }
                continue;
            }
            body.push_str(&format!("Serveur \"{name}\" :\n"));
            for t in &tools {
                let required = required_params(&t.input_schema);
                body.push_str(&format!(
                    "- {}({}) : {}\n",
                    t.name,
                    required.join(", "),
                    first_line(&t.description, 80)
                ));
            }
        }
        if body.is_empty() {
            return None;
        }
        if body.chars().count() > max_chars {
            body = body.chars().take(max_chars).collect();
            body.push_str("\n… (liste tronquée : `mcp tools <serveur>`)\n");
        }
        let head = "[Contexte client] Serveurs MCP locaux, outil(arguments requis) :";
        Some(format!("{head}\n{body}"))
    }

    /// Exécute une commande MCP émise par le modèle via `run_command`.
    /// Syntaxes (préfixe `mcp` ou `fs-mcp`) :
    ///   mcp list
    ///   mcp tools <serveur> [outil]
    ///   mcp call <serveur> <outil> [json_args]
    /// En mode économie (`eco`), les sorties sont plus courtes : descriptions réduites à
    /// une ligne, JSON sur une ligne et sans `null`.
    pub async fn exec_mcp(
        &self,
        config_dir: &Path,
        workdir: Option<&Path>,
        raw_command: &str,
        eco: bool,
    ) -> (String, bool) {
        let raw = raw_command.trim();
        let rest = mcp_rest(raw).unwrap_or(raw);

        if rest.is_empty() || rest.eq_ignore_ascii_case("list") {
            let servers = Hub::load_all_servers(config_dir, workdir);
            if servers.is_empty() {
                return (
                    "Aucun serveur MCP local configuré (vérifie mcp.json).".to_string(),
                    false,
                );
            }
            // Ni commande ni arguments ici : ils peuvent contenir des secrets.
            let lines: Vec<String> = servers
                .iter()
                .map(|(name, s)| {
                    let kind = if s.endpoint().is_some() {
                        "http"
                    } else {
                        "stdio"
                    };
                    format!("- {name} ({kind})")
                })
                .collect();
            return (
                format!("Serveurs MCP locaux disponibles :\n{}", lines.join("\n")),
                false,
            );
        }

        let (verb, after_verb) = next_word(rest);
        let (server, after_server) = next_word(after_verb);
        let verb = verb.to_ascii_lowercase();
        if server.is_empty() {
            return (USAGE.to_string(), true);
        }

        match verb.as_str() {
            "tools" => {
                let (tool_filter, _) = next_word(after_server);
                let tools = match self.get_or_connect(config_dir, workdir, server).await {
                    Ok(t) => t,
                    Err(e) => return (format!("Erreur MCP ({server}) : {e}"), true),
                };
                if !tool_filter.is_empty() {
                    return match tools
                        .iter()
                        .find(|t| t.name.eq_ignore_ascii_case(tool_filter))
                    {
                        Some(t) => {
                            let schema = json!({
                                "name": t.name,
                                "description": t.description,
                                "inputSchema": t.input_schema,
                            });
                            let text = if eco {
                                serde_json::to_string(&schema)
                            } else {
                                serde_json::to_string_pretty(&schema)
                            };
                            (text.unwrap_or_default(), false)
                        }
                        None => (
                            format!("Outil inconnu sur \"{server}\" : {tool_filter}"),
                            true,
                        ),
                    };
                }
                if tools.is_empty() {
                    return (
                        format!("Serveur \"{server}\" connecté mais aucun outil exposé."),
                        false,
                    );
                }
                let list: Vec<String> = tools
                    .iter()
                    .map(|t| {
                        let required = required_params(&t.input_schema);
                        let props: Vec<String> = t
                            .input_schema
                            .get("properties")
                            .and_then(Value::as_object)
                            .map(|o| {
                                o.keys()
                                    .map(|k| {
                                        if required.contains(&k.as_str()) {
                                            format!("{k}*")
                                        } else {
                                            k.clone()
                                        }
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        let desc = if eco {
                            first_line(&t.description, 100)
                        } else {
                            t.description.clone()
                        };
                        format!("- {}({}) : {}", t.name, props.join(", "), desc)
                    })
                    .collect();
                (
                    format!(
                        "Outils disponibles sur \"{server}\" (* = requis) :\n{}",
                        list.join("\n")
                    ),
                    false,
                )
            }
            "call" => {
                let (tool, raw_args) = next_word(after_server);
                if tool.is_empty() {
                    return (USAGE.to_string(), true);
                }
                match parse_mcp_args(raw_args) {
                    Ok(args) => match self
                        .call_tool_auto(config_dir, workdir, server, tool, args)
                        .await
                    {
                        Ok((out, is_err)) => {
                            if eco && !is_err {
                                (compact_output(&out), is_err)
                            } else {
                                (out, is_err)
                            }
                        }
                        Err(e) => (format!("Erreur appel MCP {server}.{tool} : {e}"), true),
                    },
                    Err(e) => (
                        format!(
                            "Arguments JSON invalides pour {server}.{tool} : {e}\nReçu : {raw_args}"
                        ),
                        true,
                    ),
                }
            }
            _ => (USAGE.to_string(), true),
        }
    }
}
