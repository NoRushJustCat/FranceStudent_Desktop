//! Sous-agents : l'IA lance des agents parallèles avec `run_command` et une commande
//! `agent run {...}` ou `agent parallel [{...}, {...}]`.
//!
//! Chaque sous-agent = un chat serveur temporaire, avec les mêmes outils locaux que l'agent
//! principal (ou la lecture seule pour le rôle `explore`), soumis aux mêmes autorisations.
//! Les sous-agents tournent en parallèle dans la même tâche (`tokio::join!`), un seul niveau
//! de profondeur, et renvoient un compte rendu final à l'agent principal.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use fs_api::{ChatEvent, Client, StreamOptions, ToolCall, ToolResult};
use fs_tools::Risk;
use serde_json::{json, Value};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

use crate::perm::{self, Mode, Verdict};
use crate::{ask, changelog, clip, config_dir, display_args, preview, term, AppState, UiEvent};

const MAX_PARALLEL: usize = 4;
const SUB_STEPS: usize = 20;
const SUB_OUT_CAP: usize = 20_000;
const REPORT_CAP: usize = 12_000;
const MAX_TASK_CHARS: usize = 8_000;

/// Mode d'emploi joint au premier message de chaque chat (bloc « Contexte client »).
/// Pas de saut de ligne double + `---` ici : le bloc serait mal retiré à l'affichage.
pub(crate) const DOC: &str = concat!(
    "Sous-agents (mode Code) : tu peux déléguer des sous-tâches à des agents qui travaillent en parallèle, avec l'outil run_command :\n",
    "agent run {\"task\":\"…\",\"role\":\"explore\",\"name\":\"api\"}\n",
    "agent parallel [{\"task\":\"…\",\"role\":\"explore\",\"name\":\"a\"},{\"task\":\"…\",\"role\":\"worker\",\"name\":\"b\"}]\n",
    "- 4 agents au maximum par lancement ; chacun te renvoie son compte rendu final.\n",
    "- role explore = lecture seule (list_directory, read_file, search_files) ; role worker (défaut) = tous les outils, soumis aux mêmes autorisations que toi.\n",
    "- Un sous-agent ne voit pas la conversation : donne-lui une tâche complète et autonome (fichiers concernés, objectif, format du compte rendu attendu).\n",
    "- Pas d'imbrication. Jamais deux workers sur les mêmes fichiers.\n",
    "- Petite tâche : fais-la toi-même."
);

const HELP: &str = "Usage : agent run {\"task\":\"…\",\"role\":\"explore|worker\",\"name\":\"…\"} ou agent parallel [{…},{…}] (4 agents maximum). Un sous-agent ne voit pas la conversation : la tâche doit être complète.";

/// `agent …` ou `agents …` : renvoie ce qui suit le mot-clé.
pub(crate) fn agent_rest(cmd: &str) -> Option<&str> {
    let t = cmd.trim();
    for p in ["agents", "agent"] {
        if let Some(rest) = t.strip_prefix(p) {
            if rest.is_empty() || rest.starts_with(char::is_whitespace) {
                return Some(rest.trim());
            }
        }
    }
    None
}

/// Tout ce dont un sous-agent a besoin pour tourner et dialoguer avec l'interface.
pub(crate) struct Env<'a> {
    pub(crate) app: &'a AppHandle,
    pub(crate) client: &'a Client,
    pub(crate) agent_id: i64,
    pub(crate) root: &'a Path,
    pub(crate) auto_accept: bool,
    pub(crate) run: &'a str,
    pub(crate) flag: &'a AtomicBool,
    pub(crate) on_event: &'a Channel<UiEvent>,
    /// `call_id` de l'appel `run_command` qui a lancé les sous-agents.
    pub(crate) parent_call: &'a str,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Explore,
    Worker,
}

impl Role {
    fn label(self) -> &'static str {
        match self {
            Role::Explore => "explore",
            Role::Worker => "worker",
        }
    }
}

struct Spec {
    task: String,
    role: Role,
    name: String,
}

struct Report {
    name: String,
    role: Role,
    ok: bool,
    text: String,
}

fn spec_from(v: &Value, n: usize) -> Result<Spec, String> {
    let (task, role, name) = match v {
        Value::String(s) => (s.trim().to_string(), None, None),
        Value::Object(_) => (
            fs_tools::any_str(v, &["task", "prompt", "instructions", "goal"])
                .unwrap_or("")
                .trim()
                .to_string(),
            fs_tools::any_str(v, &["role", "type"]),
            fs_tools::any_str(v, &["name", "label", "title"]),
        ),
        _ => {
            return Err(format!(
                "agent {n} : entrée invalide (objet ou texte attendu)."
            ))
        }
    };
    if task.is_empty() {
        return Err(format!("agent {n} : champ \"task\" manquant."));
    }
    let role = match role.map(|r| r.trim().to_lowercase()).as_deref() {
        Some("explore" | "explorer" | "read" | "readonly" | "read-only" | "lecture") => {
            Role::Explore
        }
        _ => Role::Worker,
    };
    let name = name
        .map(|s| clip(s.trim(), 40))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("agent-{n}"));
    Ok(Spec { task, role, name })
}

/// `Ok(None)` = afficher l'aide.
fn parse(rest: &str) -> Result<Option<Vec<Spec>>, String> {
    let t = rest.trim();
    let body = if t.starts_with('{') || t.starts_with('[') {
        t
    } else {
        let (verb, after) = match t.split_once(char::is_whitespace) {
            Some((v, a)) => (v, a.trim()),
            None => (t, ""),
        };
        match verb.to_lowercase().as_str() {
            "" | "help" | "aide" | "list" => return Ok(None),
            "run" | "spawn" | "parallel" | "par" | "start" => after,
            _ => t,
        }
    };
    if body.is_empty() {
        return Ok(None);
    }
    let items: Vec<Value> = match serde_json::from_str::<Value>(body) {
        Ok(Value::Array(a)) => a,
        Ok(Value::Object(o)) => {
            let v = Value::Object(o);
            let inner = v
                .get("agents")
                .or_else(|| v.get("tasks"))
                .and_then(Value::as_array)
                .cloned();
            match inner {
                Some(a) => a,
                None => vec![v],
            }
        }
        Ok(Value::String(s)) => vec![Value::String(s)],
        _ => vec![Value::String(body.to_string())],
    };
    if items.is_empty() {
        return Ok(None);
    }
    if items.len() > MAX_PARALLEL {
        return Err(format!(
            "{} agents demandés ; maximum {MAX_PARALLEL} par lancement. Découpe en plusieurs lancements.",
            items.len()
        ));
    }
    let mut specs = Vec::new();
    for (i, v) in items.iter().enumerate() {
        specs.push(spec_from(v, i + 1)?);
    }
    Ok(Some(specs))
}

fn notice(env: &Env<'_>, text: String) {
    let _ = env.on_event.send(UiEvent::Notice { text });
}

/// Exécute la commande `agent …` : renvoie (comptes rendus, erreur si tous ont échoué).
pub(crate) async fn exec(env: &Env<'_>, rest: &str) -> (String, bool) {
    let specs = match parse(rest) {
        Ok(Some(s)) => s,
        Ok(None) => return (HELP.to_string(), false),
        Err(e) => return (e, true),
    };
    let (r0, r1, r2, r3) = tokio::join!(
        slot(env, &specs, 0),
        slot(env, &specs, 1),
        slot(env, &specs, 2),
        slot(env, &specs, 3),
    );
    let reports: Vec<Report> = [r0, r1, r2, r3].into_iter().flatten().collect();
    let mut out = String::new();
    let mut failed = 0usize;
    for r in &reports {
        if !r.ok {
            failed += 1;
        }
        let status = if r.ok { "terminé" } else { "échec" };
        let body = if r.text.trim().is_empty() {
            "(aucun compte rendu)".to_string()
        } else {
            clip(&r.text, REPORT_CAP)
        };
        out.push_str(&format!(
            "### {} ({}) — {status}\n{body}\n\n",
            r.name,
            r.role.label()
        ));
    }
    let all_failed = !reports.is_empty() && failed == reports.len();
    (out.trim_end().to_string(), all_failed)
}

async fn slot(env: &Env<'_>, specs: &[Spec], i: usize) -> Option<Report> {
    let spec = specs.get(i)?;
    Some(run_one(env, spec, i).await)
}

async fn run_one(env: &Env<'_>, spec: &Spec, idx: usize) -> Report {
    let mode = {
        let state = env.app.state::<AppState>();
        perm::current(&state)
    };
    // En planification, tout le monde reste en lecture seule.
    let role = if mode == Mode::Plan {
        Role::Explore
    } else {
        spec.role
    };
    notice(env, format!("⎇ {} ({}) : démarre", spec.name, role.label()));
    let res = drive(env, spec, idx, role, mode).await;
    let (ok, text) = match res {
        Ok(t) => (true, t),
        Err(e) => (false, e),
    };
    notice(
        env,
        format!(
            "⎇ {} ({}) : {}",
            spec.name,
            role.label(),
            if ok { "terminé" } else { "échec" }
        ),
    );
    Report {
        name: spec.name.clone(),
        role,
        ok,
        text,
    }
}

fn build_prompt(env: &Env<'_>, spec: &Spec, role: Role, mode: Mode) -> String {
    let mut p =
        String::from("Tu es un sous-agent lancé par un agent principal pour une tâche précise. ");
    p.push_str(&format!("Projet : {}. ", env.root.display()));
    match role {
        Role::Explore => {
            p.push_str("Rôle : explore (lecture seule : list_directory, read_file, search_files). ")
        }
        Role::Worker => p.push_str(
            "Rôle : worker (tu peux lire, modifier des fichiers et lancer des commandes). ",
        ),
    }
    p.push_str("Tu ne vois pas la conversation de l'utilisateur : tout ce dont tu as besoin est dans la tâche. N'appelle pas `agent` ni `mcp`. ");
    if let Some(h) = perm::mode_hint(mode) {
        p.push_str(h);
        p.push(' ');
    }
    p.push_str("Termine par un compte rendu court : ce que tu as trouvé ou fait, fichiers touchés, points ouverts.\n\nTâche : ");
    p.push_str(&clip(&spec.task, MAX_TASK_CHARS));
    p
}

/// Boucle d'un sous-agent : chat serveur temporaire, outils locaux, compte rendu final.
async fn drive(
    env: &Env<'_>,
    spec: &Spec,
    idx: usize,
    role: Role,
    mode: Mode,
) -> Result<String, String> {
    let tools: Vec<Value> = match role {
        Role::Explore => ["list_directory", "read_file", "search_files"]
            .iter()
            .map(|n| json!(n))
            .collect(),
        Role::Worker => fs_tools::NAMES.iter().map(|n| json!(n)).collect(),
    };
    let prompt = build_prompt(env, spec, role, mode);
    let opts = StreamOptions::default();
    let mut chat_id: Option<String> = None;
    let mut results: Vec<ToolResult> = Vec::new();
    let mut last_text = String::new();

    let outcome: Result<String, String> = 'run: {
        for _ in 0..SUB_STEPS {
            if env.flag.load(Ordering::SeqCst) {
                break 'run Err("annulé".to_string());
            }
            let mut opened: Option<String> = None;
            let mut text = String::new();
            let mut calls: Vec<ToolCall> = Vec::new();
            let mut failed: Option<String> = None;
            let mut cancelled = false;
            {
                let handler = |ev: ChatEvent| match ev {
                    ChatEvent::Opened { chat_id: cid } => opened = Some(cid),
                    ChatEvent::Delta(t) => text.push_str(&t),
                    ChatEvent::Completed {
                        chat_id: cid,
                        message,
                    } => {
                        opened = Some(cid);
                        if !message.content.is_empty() {
                            text = message.content;
                        }
                        calls = message.tool_calls;
                    }
                    ChatEvent::Error(e) => failed = Some(e.message),
                    ChatEvent::Cancelled { .. } => cancelled = true,
                    _ => {}
                };
                let r = match chat_id.as_deref() {
                    Some(id) => {
                        env.client
                            .send_message_with_options(
                                id,
                                "",
                                Some(env.agent_id),
                                &tools,
                                &results,
                                &opts,
                                handler,
                            )
                            .await
                    }
                    None => {
                        env.client
                            .stream_chat_with_options(env.agent_id, &prompt, &tools, &opts, handler)
                            .await
                    }
                };
                if let Err(e) = r {
                    break 'run Err(e.to_string());
                }
            }
            if chat_id.is_none() {
                chat_id = opened.take();
            }
            if let Some(m) = failed.take() {
                break 'run Err(m);
            }
            if cancelled {
                break 'run Err("annulé".to_string());
            }
            if !text.is_empty() {
                last_text = text.clone();
            }
            // Seuls nos outils sont rejoués ; les autres sont exécutés côté serveur.
            let ours: Vec<ToolCall> = calls
                .into_iter()
                .filter(|c| fs_tools::risk(&c.name).is_some())
                .collect();
            if ours.is_empty() {
                break 'run Ok(last_text.clone());
            }
            results.clear();
            for call in ours {
                if env.flag.load(Ordering::SeqCst) {
                    break 'run Err("annulé".to_string());
                }
                let (output, is_error) = tool_call(env, idx, &spec.name, role, &call).await;
                results.push(ToolResult {
                    call_id: call.call_id,
                    output: clip(&output, SUB_OUT_CAP),
                    is_error,
                });
            }
        }
        Err(format!(
            "limite de {SUB_STEPS} étapes atteinte sans compte rendu final"
        ))
    };

    // Chat temporaire : suppression au mieux.
    if let Some(id) = chat_id.as_deref() {
        let _ = env.client.delete_chat(id).await;
    }
    outcome
}

/// Un appel d'outil d'un sous-agent : mêmes autorisations que l'agent principal.
async fn tool_call(
    env: &Env<'_>,
    idx: usize,
    label: &str,
    role: Role,
    call: &ToolCall,
) -> (String, bool) {
    let args = fs_tools::parse_args(&call.arguments);
    let uid = format!("{}~{}~{}", env.parent_call, idx, call.call_id);
    let cmd: Option<String> = if call.name == "run_command" {
        args["command"].as_str().map(str::to_string)
    } else {
        None
    };
    let risk = fs_tools::risk(&call.name);
    let state = env.app.state::<AppState>();
    let mode = perm::current(&state);

    let mut blocked: Option<String> = None;
    let mut needs_confirm = false;
    if role == Role::Explore && risk != Some(Risk::Safe) {
        blocked = Some("Refusé : sous-agent en lecture seule (rôle explore).".to_string());
    } else if let Some(c) = cmd.as_deref() {
        if agent_rest(c).is_some() || crate::mcp_ctx::mcp_rest(c).is_some() {
            blocked =
                Some("Refusé : un sous-agent ne peut pas lancer `agent` ni `mcp`.".to_string());
        }
    }
    if blocked.is_none() {
        match perm::decide(mode, env.auto_accept, &call.name, &args, Some(env.root)) {
            Verdict::Allow => {}
            Verdict::Ask => needs_confirm = true,
            Verdict::Deny(m) => blocked = Some(m),
        }
    }

    let mut shown = display_args(&args);
    if let Some(o) = shown.as_object_mut() {
        o.insert("_agent".to_string(), json!(label));
    }
    let _ = env.on_event.send(UiEvent::ToolCall {
        call_id: uid.clone(),
        name: call.name.clone(),
        origin: "local".to_string(),
        args: shown,
        needs_confirm,
    });

    let allowed = if needs_confirm && blocked.is_none() {
        ask(&state, env.run, &uid).await
    } else {
        true
    };
    let denied = blocked.is_some() || !allowed;

    let (output, is_error) = if let Some(m) = blocked {
        (m, true)
    } else if !allowed {
        ("Refusé par l'utilisateur.".to_string(), true)
    } else {
        let r = if mode == Mode::Full {
            fs_tools::execute_full(env.root, &call.name, &call.arguments).await
        } else {
            fs_tools::execute(env.root, &call.name, &call.arguments).await
        };
        match r {
            Ok(o) => (o, false),
            Err(e) => (e, true),
        }
    };

    let _ = env.on_event.send(UiEvent::ToolResult {
        call_id: uid,
        output: preview(&output),
        is_error,
        denied,
    });
    if !denied {
        if let Some(c) = cmd.as_deref() {
            term::emit_ai(env.app, c, &clip(&output, 20_000), is_error, Some(env.root));
        }
    }
    if let Some(lbl) = changelog::label(&call.name, &args, None) {
        let status = if denied {
            "refusé"
        } else if is_error {
            "échec"
        } else {
            "ok"
        };
        if let Ok(dir) = config_dir(env.app) {
            changelog::record(&dir, &call.name, &lbl, status, Some(env.root));
        }
        notice(env, format!("✎ {lbl} · {status} · {label}"));
    }
    (output, is_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rest_prefix() {
        assert_eq!(agent_rest("agent run {}"), Some("run {}"));
        assert_eq!(agent_rest("agents"), Some(""));
        assert_eq!(agent_rest("agentx run"), None);
        assert_eq!(agent_rest("git status"), None);
    }

    #[test]
    fn parse_forms() {
        let one = parse("run {\"task\":\"lis a.txt\",\"role\":\"explore\",\"name\":\"x\"}")
            .unwrap()
            .unwrap();
        assert_eq!(one.len(), 1);
        assert!(one[0].role == Role::Explore);
        assert_eq!(one[0].name, "x");

        let many = parse("parallel [{\"task\":\"a\"},{\"task\":\"b\"}]")
            .unwrap()
            .unwrap();
        assert_eq!(many.len(), 2);
        assert!(many[0].role == Role::Worker);
        assert_eq!(many[1].name, "agent-2");

        assert!(parse("").unwrap().is_none());
        assert!(parse("run {\"role\":\"explore\"}").is_err());
        assert!(parse("parallel [{\"task\":\"a\"},{\"task\":\"b\"},{\"task\":\"c\"},{\"task\":\"d\"},{\"task\":\"e\"}]").is_err());
    }
}
