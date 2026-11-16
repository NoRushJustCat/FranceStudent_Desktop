//! Swarm de sous-agents : l'IA lance des agents parallèles avec `run_command` et une commande
//! `tasks run {...}` ou `tasks parallel [{...}, {...}]` ou `tasks parallel {"contract":"…","tasks":[…]}`.
//!
//! Chaque sous-agent = un chat serveur temporaire, avec les mêmes outils locaux que l'agent
//! principal (ou la lecture seule pour le rôle `explore`), soumis aux mêmes autorisations.
//!
//! V5 :
//! - jusqu'à 8 agents en parallèle par vague, 12 par lancement ;
//! - **contrat partagé** (`contract`) injecté dans le prompt de chaque agent (noms de fichiers,
//!   interfaces, conventions) pour que les morceaux produits séparément s'assemblent ;
//! - **vagues** (`wave` = 1..4) : les vagues s'exécutent l'une après l'autre, et chaque vague
//!   reçoit les comptes rendus des précédentes (ex. architecte -> workers -> relecteur) ;
//! - **propriété des fichiers** : un fichier modifié par un worker est réservé à ce worker,
//!   les autres workers ne peuvent pas l'écraser ;
//! - **failover** : si un agent échoue (hors annulation / limite d'étapes / requête bloquée par
//!   la passerelle), une seconde tentative est lancée sur un autre modèle actif ;
//! - bilan final (agents terminés, bascules, fichiers par agent).
//!
//! V6 (voir `agents_live.rs`) :
//! - **rien dans le chat principal** : ce que font les agents est suivi en direct dans un panneau
//!   dédié (événement `agents:live`), et seules les demandes d'autorisation y apparaissent
//!   (sinon l'utilisateur ne pourrait pas répondre) ;
//! - chaque agent a sa **transcription sur le PC** (JSONL) ;
//! - le chat serveur de chaque agent est **masqué** des listes dès son ouverture puis **supprimé**
//!   à la fin de l'agent ;
//! - chaque agent peut être **annulé** individuellement depuis le panneau.
//!
//! Un seul niveau de profondeur : les sous-agents ne lancent ni `tasks` ni `mcp`.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use fs_api::{Agent, ChatEvent, Client, StreamOptions, ToolCall, ToolResult};
use fs_tools::Risk;
use serde_json::{json, Value};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

use crate::agents_live::{self as live, friendly_error, NewAgent, Rec, CANCELLED, GATEWAY_TAG};
use crate::perm::{self, Mode, Verdict};
use crate::{ask, changelog, clip, config_dir, display_args, term, AppState, UiEvent};

/// Agents en parallèle dans une même vague.
const MAX_PARALLEL: usize = 8;
/// Agents au total par lancement.
const MAX_AGENTS: usize = 12;
/// Nombre de vagues successives.
const MAX_WAVES: u32 = 4;
const SUB_STEPS: usize = 20;
const SUB_OUT_CAP: usize = 20_000;
const REPORT_CAP: usize = 12_000;
const MAX_TASK_CHARS: usize = 8_000;
/// Taille max d'un compte rendu transmis aux vagues suivantes.
const WAVE_CONTEXT_CAP: usize = 3_000;

/// Mode d'emploi joint au premier message de chaque chat (bloc « Contexte client »).
/// Pas de saut de ligne double + `---` ici : le bloc serait mal retiré à l'affichage.
pub(crate) const DOC: &str = concat!(
    "Sous-agents (mode Code) : tu peux déléguer des sous-tâches à une équipe d'agents qui travaillent en parallèle, avec l'outil run_command :\n",
    "tasks run {\"task\":\"…\",\"role\":\"explore\",\"name\":\"api\"}\n",
    "tasks parallel [{\"task\":\"…\",\"role\":\"explore\",\"name\":\"a\"},{\"task\":\"…\",\"role\":\"worker\",\"name\":\"b\"}]\n",
    "tasks parallel {\"contract\":\"…\",\"tasks\":[{\"task\":\"…\",\"name\":\"a\",\"wave\":1},{\"task\":\"…\",\"name\":\"b\",\"wave\":2}]}\n",
    "- 8 agents au maximum en parallèle par vague, 12 par lancement ; chacun te renvoie son compte rendu final.\n",
    "- role explore = lecture seule (list_directory, read_file, search_files) ; role worker (défaut) = tous les outils, soumis aux mêmes autorisations que toi.\n",
    "- contract (optionnel) = spécification commune donnée à chaque agent : noms des fichiers, interfaces, IDs, conventions. Indispensable quand plusieurs agents écrivent des fichiers qui doivent fonctionner ensemble.\n",
    "- wave (1 à 4, défaut 1) : les vagues s'exécutent dans l'ordre et chaque vague reçoit les comptes rendus des précédentes (ex. 1 = explore, 2 = workers, 3 = relecture).\n",
    "- Un sous-agent ne voit pas la conversation : donne-lui une tâche complète et autonome (fichiers concernés, objectif, format du compte rendu attendu).\n",
    "- Un fichier modifié par un worker lui est réservé : les autres workers ne peuvent pas le modifier. Attribue des fichiers distincts.\n",
    "- Si un agent échoue, il est relancé automatiquement sur un autre modèle. Pas d'imbrication.\n",
    "- L'utilisateur suit les agents en direct dans un panneau dédié de l'application : inutile de recopier leurs échanges, résume seulement le résultat.\n",
    "- Petite tâche : fais-la toi-même."
);

const HELP: &str = "Usage : tasks run {\"task\":\"…\",\"role\":\"explore|worker\",\"name\":\"…\",\"wave\":1} ou tasks parallel [{…},{…}] ou tasks parallel {\"contract\":\"…\",\"tasks\":[{…},{…}]} (8 agents par vague, 12 au total, vagues 1 à 4). Un sous-agent ne voit pas la conversation : la tâche doit être complète.";

/// `tasks …` : renvoie ce qui suit le mot-clé.
pub(crate) fn agent_rest(cmd: &str) -> Option<&str> {
    let t = cmd.trim();
    for p in ["tasks"] {
        // Insensible à la casse : « Tasks run … ».
        if t.len() >= p.len() && t.is_char_boundary(p.len()) && t[..p.len()].eq_ignore_ascii_case(p) {
            let rest = &t[p.len()..];
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
    wave: u32,
}

struct Plan {
    contract: String,
    specs: Vec<Spec>,
}

struct Report {
    name: String,
    role: Role,
    ok: bool,
    text: String,
    /// Nom du modèle de secours si une bascule a eu lieu.
    failover: Option<String>,
}

/// État partagé par tous les agents d'un lancement.
struct Swarm {
    contract: String,
    /// Modèles de secours (actifs, hors Google/Gemini), le plus « différent » en premier.
    fallbacks: Vec<Agent>,
    /// Nom de chaque modèle (id -> nom), pour le panneau des agents.
    models: HashMap<i64, String>,
    /// Dossier des transcriptions de ce lancement (vide = pas de transcription).
    dir: PathBuf,
    /// Clé de ce lancement (date UTC + fin de l'identifiant d'appel).
    run_key: String,
    /// Chemin normalisé -> (agent propriétaire, chemin tel qu'écrit).
    owners: Mutex<HashMap<String, (String, String)>>,
}

fn norm_path(p: &str) -> String {
    let mut s = p.trim().replace('\\', "/").to_lowercase();
    while let Some(rest) = s.strip_prefix("./") {
        s = rest.to_string();
    }
    s
}

impl Swarm {
    fn new(
        contract: String,
        fallbacks: Vec<Agent>,
        models: HashMap<i64, String>,
        dir: PathBuf,
        run_key: String,
    ) -> Self {
        Swarm {
            contract,
            fallbacks,
            models,
            dir,
            run_key,
            owners: Mutex::new(HashMap::new()),
        }
    }

    /// Nom lisible d'un modèle (repli : son identifiant).
    fn model_name(&self, id: i64) -> String {
        self.models
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("modèle {id}"))
    }

    /// `Ok(true)` = réservé à l'instant, `Ok(false)` = déjà à cet agent, `Err(propriétaire)` sinon.
    fn claim(&self, path: &str, owner: &str) -> Result<bool, String> {
        let key = norm_path(path);
        let mut map = self.owners.lock().unwrap_or_else(|e| e.into_inner());
        match map.get(&key) {
            Some((o, _)) if o == owner => Ok(false),
            Some((o, _)) => Err(o.clone()),
            None => {
                map.insert(key, (owner.to_string(), path.trim().to_string()));
                Ok(true)
            }
        }
    }

    fn release(&self, path: &str, owner: &str) {
        let key = norm_path(path);
        let mut map = self.owners.lock().unwrap_or_else(|e| e.into_inner());
        if map.get(&key).map(|(o, _)| o == owner).unwrap_or(false) {
            map.remove(&key);
        }
    }

    fn files_by_owner(&self) -> Vec<(String, Vec<String>)> {
        let map = self.owners.lock().unwrap_or_else(|e| e.into_inner());
        let mut by: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (owner, shown) in map.values() {
            by.entry(owner.clone()).or_default().push(shown.clone());
        }
        by.into_iter()
            .map(|(o, mut f)| {
                f.sort();
                (o, f)
            })
            .collect()
    }
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
    let wave = v
        .get("wave")
        .or_else(|| v.get("phase"))
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .clamp(1, MAX_WAVES as u64) as u32;
    Ok(Spec {
        task,
        role,
        name,
        wave,
    })
}

/// `Ok(None)` = afficher l'aide.
fn parse(rest: &str) -> Result<Option<Plan>, String> {
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
            "run" | "spawn" | "parallel" | "par" | "start" | "swarm" => after,
            _ => t,
        }
    };
    // Tolère un JSON entouré de guillemets simples (habitude PowerShell).
    let body = body
        .strip_prefix('\'')
        .and_then(|s| s.strip_suffix('\''))
        .unwrap_or(body)
        .trim();
    if body.is_empty() {
        return Ok(None);
    }
    let mut contract = String::new();
    let items: Vec<Value> = match serde_json::from_str::<Value>(body) {
        Ok(Value::Array(a)) => a,
        Ok(v @ Value::Object(_)) => {
            let inner = v
                .get("tasks")
                .or_else(|| v.get("agents"))
                .and_then(Value::as_array)
                .cloned();
            match inner {
                Some(a) => {
                    contract = fs_tools::any_str(&v, &["contract", "spec", "shared"])
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    a
                }
                None => vec![v],
            }
        }
        Ok(Value::String(s)) => vec![Value::String(s)],
        _ => vec![Value::String(body.to_string())],
    };
    if items.is_empty() {
        return Ok(None);
    }
    if items.len() > MAX_AGENTS {
        return Err(format!(
            "{} agents demandés ; maximum {MAX_AGENTS} par lancement. Découpe en plusieurs lancements.",
            items.len()
        ));
    }
    let mut specs = Vec::new();
    for (i, v) in items.iter().enumerate() {
        specs.push(spec_from(v, i + 1)?);
    }
    for w in 1..=MAX_WAVES {
        let n = specs.iter().filter(|s| s.wave == w).count();
        if n > MAX_PARALLEL {
            return Err(format!(
                "vague {w} : {n} agents ; maximum {MAX_PARALLEL} en parallèle par vague. Répartis-les sur plusieurs vagues (champ \"wave\")."
            ));
        }
    }
    Ok(Some(Plan {
        contract: clip(&contract, MAX_TASK_CHARS),
        specs,
    }))
}

/// Modèles de secours pour le failover : actifs, différents du modèle courant,
/// sans Google/Gemini (outils clients non garantis), autre fournisseur en premier.
/// Renvoie aussi le nom de tous les modèles connus (id -> nom) pour le panneau.
async fn fallback_agents(env: &Env<'_>) -> (Vec<Agent>, HashMap<i64, String>) {
    let all = env.client.agents().await.unwrap_or_default();
    let names: HashMap<i64, String> = all.iter().map(|a| (a.id, a.name.clone())).collect();
    let provider = all
        .iter()
        .find(|a| a.id == env.agent_id)
        .map(|a| a.model_provider.clone())
        .unwrap_or_default();
    let mut v: Vec<Agent> = all
        .into_iter()
        .filter(|a| {
            a.is_active
                && a.id != env.agent_id
                && !a.model_provider.to_lowercase().contains("google")
                && !a.name.to_lowercase().contains("gemini")
        })
        .collect();
    // `false` (autre fournisseur) passe avant `true`.
    v.sort_by_key(|a| a.model_provider == provider);
    (v, names)
}

/// Une seconde tentative sur un autre modèle a-t-elle un sens ?
/// Non si annulation, limite d'étapes atteinte, ou requête bloquée en amont par la passerelle
/// (pare-feu/CDN) : changer de modèle ne lève pas un blocage HTTP.
fn can_failover(err: &str, cancelled: bool) -> bool {
    !cancelled
        && err != CANCELLED
        && !err.starts_with("limite de")
        && !err.contains(GATEWAY_TAG)
}

/// Comptes rendus des vagues précédentes, tronqués, à transmettre à la vague suivante.
fn prior_context(reports: &[Report]) -> String {
    let mut s = String::new();
    for r in reports {
        let status = if r.ok { "terminé" } else { "échec" };
        let body = if r.text.trim().is_empty() {
            "(aucun compte rendu)".to_string()
        } else {
            clip(&r.text, WAVE_CONTEXT_CAP)
        };
        s.push_str(&format!(
            "### {} ({}) — {status}\n{body}\n\n",
            r.name,
            r.role.label()
        ));
    }
    s.trim_end().to_string()
}

/// Exécute la commande `agent …` : renvoie (comptes rendus, erreur si tous ont échoué).
pub(crate) async fn exec(env: &Env<'_>, rest: &str) -> (String, bool) {
    let plan = match parse(rest) {
        Ok(Some(p)) => p,
        Ok(None) => return (HELP.to_string(), false),
        Err(e) => return (e, true),
    };
    let (dir, run_key) = live::new_run(env.app, env.parent_call);
    // Suppressions de chats d'agents restées en attente lors d'un lancement précédent.
    live::sweep(env.app, env.client).await;
    let (fallbacks, models) = fallback_agents(env).await;
    let swarm = Swarm::new(
        plan.contract.clone(),
        fallbacks,
        models,
        dir,
        run_key.clone(),
    );
    let multi_wave = plan.specs.iter().any(|s| s.wave > 1);
    live::note(
        env.app,
        &format!("{run_key} · lancement de {} agent(s)", plan.specs.len()),
    );

    let mut reports: Vec<Report> = Vec::new();
    let mut skipped_note = String::new();
    let mut last_wave_len = 0usize;
    for wave in 1..=MAX_WAVES {
        let idxs: Vec<usize> = plan
            .specs
            .iter()
            .enumerate()
            .filter(|(_, s)| s.wave == wave)
            .map(|(i, _)| i)
            .collect();
        if idxs.is_empty() {
            continue;
        }
        if env.flag.load(Ordering::SeqCst) {
            skipped_note = format!("vague {wave} et suivantes non lancées : annulé.");
            break;
        }
        // Si toute la vague précédente a échoué, la suite n'a aucune base : on s'arrête.
        if last_wave_len > 0 {
            let tail = &reports[reports.len() - last_wave_len..];
            if tail.iter().all(|r| !r.ok) {
                skipped_note = format!(
                    "vague {wave} et suivantes non lancées : tous les agents de la vague précédente ont échoué."
                );
                break;
            }
        }
        if multi_wave {
            live::note(
                env.app,
                &format!(
                    "{run_key} · vague {wave} : {} agent(s) en parallèle",
                    idxs.len()
                ),
            );
        }
        let prior = prior_context(&reports);
        let specs = &plan.specs;
        let (r0, r1, r2, r3, r4, r5, r6, r7) = tokio::join!(
            slot(env, specs, &swarm, &prior, &idxs, 0),
            slot(env, specs, &swarm, &prior, &idxs, 1),
            slot(env, specs, &swarm, &prior, &idxs, 2),
            slot(env, specs, &swarm, &prior, &idxs, 3),
            slot(env, specs, &swarm, &prior, &idxs, 4),
            slot(env, specs, &swarm, &prior, &idxs, 5),
            slot(env, specs, &swarm, &prior, &idxs, 6),
            slot(env, specs, &swarm, &prior, &idxs, 7),
        );
        let wave_reports: Vec<Report> = [r0, r1, r2, r3, r4, r5, r6, r7]
            .into_iter()
            .flatten()
            .collect();
        last_wave_len = wave_reports.len();
        reports.extend(wave_reports);
    }

    let mut out = String::new();
    let mut failed = 0usize;
    let mut switched = 0usize;
    for r in &reports {
        if !r.ok {
            failed += 1;
        }
        let status = if r.ok { "terminé" } else { "échec" };
        let via = match &r.failover {
            Some(m) => {
                switched += 1;
                format!(" · via {m}")
            }
            None => String::new(),
        };
        let body = if r.text.trim().is_empty() {
            "(aucun compte rendu)".to_string()
        } else {
            clip(&r.text, REPORT_CAP)
        };
        out.push_str(&format!(
            "### {} ({}) — {status}{via}\n{body}\n\n",
            r.name,
            r.role.label()
        ));
    }

    // Bilan global.
    out.push_str(&format!(
        "Bilan : {}/{} agents terminés",
        reports.len() - failed,
        reports.len()
    ));
    if switched > 0 {
        out.push_str(&format!(" · {switched} bascule(s) de modèle"));
    }
    out.push('\n');
    let files = swarm.files_by_owner();
    if !files.is_empty() {
        out.push_str("Fichiers modifiés par agent :\n");
        for (owner, list) in files {
            out.push_str(&format!("- {owner} : {}\n", list.join(", ")));
        }
    }
    if !skipped_note.is_empty() {
        out.push_str(&format!("Attention : {skipped_note}\n"));
        live::note(env.app, &format!("{run_key} · {skipped_note}"));
    }
    live::note(
        env.app,
        &format!(
            "{run_key} · bilan : {}/{} agents terminés{}",
            reports.len() - failed,
            reports.len(),
            if switched > 0 {
                format!(" · {switched} bascule(s)")
            } else {
                String::new()
            }
        ),
    );

    let all_failed = !reports.is_empty() && failed == reports.len();
    (out.trim_end().to_string(), all_failed)
}

async fn slot(
    env: &Env<'_>,
    specs: &[Spec],
    sw: &Swarm,
    prior: &str,
    idxs: &[usize],
    k: usize,
) -> Option<Report> {
    let i = *idxs.get(k)?;
    Some(run_one(env, &specs[i], i, sw, prior).await)
}

/// Un agent, avec une seconde tentative sur un autre modèle en cas d'échec.
async fn run_one(env: &Env<'_>, spec: &Spec, idx: usize, sw: &Swarm, prior: &str) -> Report {
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

    // Fiche du panneau + transcription locale : tout ce que fait l'agent passe par `rec`.
    let rec = Rec::start(
        env.app,
        &NewAgent {
            run_key: &sw.run_key,
            dir: &sw.dir,
            idx,
            name: &spec.name,
            role: role.label(),
            wave: spec.wave,
            model: &sw.model_name(env.agent_id),
            task: &spec.task,
        },
    );

    let mut res = drive(
        env,
        sw,
        &rec,
        &Job {
            spec,
            idx,
            role,
            mode,
            agent_id: env.agent_id,
            prior,
            retry: false,
        },
    )
    .await;

    let mut failover: Option<String> = None;
    let err_msg: Option<String> = res.as_ref().err().cloned();
    if let Some(e) = err_msg {
        let cancelled = env.flag.load(Ordering::SeqCst) || rec.cancelled();
        if can_failover(&e, cancelled) {
            if let Some(fb) = sw.fallbacks.first() {
                rec.note(&format!(
                    "échec ({}) → bascule sur {}",
                    clip(&e, 80),
                    fb.name
                ));
                rec.model(&fb.name);
                failover = Some(fb.name.clone());
                res = drive(
                    env,
                    sw,
                    &rec,
                    &Job {
                        spec,
                        idx,
                        role,
                        mode,
                        agent_id: fb.id,
                        prior,
                        retry: true,
                    },
                )
                .await;
            }
        } else if e.contains(GATEWAY_TAG) {
            // Pas de bascule : un blocage HTTP en amont ne dépend pas du modèle.
            rec.note(
                "requête bloquée en amont (HTTP 403 avec page HTML) : pas de nouvelle tentative automatique. Voir le message d'erreur (Ray ID) pour l'examiner côté pare-feu/CDN.",
            );
        }
    }

    let (ok, text) = match res {
        Ok(t) => (true, t),
        Err(e) => (false, e),
    };
    let status = if ok {
        "done"
    } else if text == CANCELLED {
        "cancelled"
    } else {
        "failed"
    };
    rec.finish(status, &text);
    Report {
        name: spec.name.clone(),
        role,
        ok,
        text,
        failover,
    }
}

/// Paramètres d'une exécution d'agent (évite une signature à rallonge).
struct Job<'a> {
    spec: &'a Spec,
    idx: usize,
    role: Role,
    mode: Mode,
    agent_id: i64,
    prior: &'a str,
    retry: bool,
}

fn build_prompt(env: &Env<'_>, sw: &Swarm, job: &Job<'_>) -> String {
    let mut p =
        String::from("Tu es un sous-agent lancé par un agent principal pour une tâche précise. ");
    p.push_str(&format!("Projet : {}. ", env.root.display()));
    p.push_str(&format!("Ton nom dans l'équipe : {}. ", job.spec.name));
    match job.role {
        Role::Explore => {
            p.push_str("Rôle : explore (lecture seule : list_directory, read_file, search_files). ")
        }
        Role::Worker => p.push_str(
            "Rôle : worker (tu peux lire, modifier des fichiers et lancer des commandes). Un fichier que tu modifies t'est réservé : si une écriture est refusée parce qu'un autre agent en est propriétaire, ne la contourne pas et signale-le dans ton compte rendu. ",
        ),
    }
    p.push_str("Tu ne vois pas la conversation de l'utilisateur : tout ce dont tu as besoin est dans la tâche. N'appelle pas `agent` ni `mcp`. ");
    if let Some(h) = perm::mode_hint(job.mode) {
        p.push_str(h);
        p.push(' ');
    }
    if job.retry {
        p.push_str("Une première tentative de cette tâche a échoué en cours de route : vérifie l'état actuel des fichiers avant de réécrire quoi que ce soit. ");
    }
    p.push_str("Termine par un compte rendu court : ce que tu as trouvé ou fait, fichiers touchés, points ouverts.");
    if !sw.contract.is_empty() {
        p.push_str(
            "\n\nContrat partagé par toute l'équipe (noms de fichiers, interfaces, IDs, conventions : respecte-le à la lettre pour que les parties produites par chacun s'assemblent) :\n",
        );
        p.push_str(&sw.contract);
    }
    if !job.prior.is_empty() {
        p.push_str("\n\nComptes rendus des agents des vagues précédentes :\n");
        p.push_str(job.prior);
    }
    p.push_str("\n\nTâche : ");
    p.push_str(&clip(&job.spec.task, MAX_TASK_CHARS));
    p
}

/// Boucle d'un sous-agent : chat serveur temporaire (invisible dans l'application, supprimé à la
/// fin), outils locaux, compte rendu final. Tout ce qui se passe est envoyé au panneau des agents
/// et écrit dans la transcription locale via `rec`.
async fn drive(
    env: &Env<'_>,
    sw: &Swarm,
    rec: &Rec,
    job: &Job<'_>,
) -> Result<String, String> {
    let spec = job.spec;
    let tools: Vec<Value> = match job.role {
        Role::Explore => ["list_directory", "read_file", "search_files"]
            .iter()
            .map(|n| json!(n))
            .collect(),
        Role::Worker => fs_tools::NAMES.iter().map(|n| json!(n)).collect(),
    };
    let prompt = build_prompt(env, sw, job);
    rec.prompt(&prompt, job.retry);
    let opts = StreamOptions::default();
    let mut chat_id: Option<String> = None;
    let mut results: Vec<ToolResult> = Vec::new();
    let mut last_text = String::new();

    let outcome: Result<String, String> = 'run: {
        for step in 0..SUB_STEPS {
            if env.flag.load(Ordering::SeqCst) || rec.cancelled() {
                break 'run Err(CANCELLED.to_string());
            }
            let mut opened: Option<String> = None;
            let mut text = String::new();
            let mut reasoning = String::new();
            let mut calls: Vec<ToolCall> = Vec::new();
            let mut failed: Option<String> = None;
            let mut cancelled = false;
            // Erreur d'envoi (réseau, HTTP) et annulation demandée pendant l'appel.
            let mut transport: Option<String> = None;
            let mut aborted = false;
            {
                let handler = |ev: ChatEvent| match ev {
                    ChatEvent::Opened { chat_id: cid } => {
                        // Le chat de l'agent disparaît des listes dès qu'il existe.
                        rec.chat(&cid);
                        opened = Some(cid);
                    }
                    ChatEvent::Delta(t) => {
                        rec.delta(&t);
                        text.push_str(&t);
                    }
                    ChatEvent::ReasoningDelta(t) => {
                        rec.reasoning(&t);
                        reasoning.push_str(&t);
                    }
                    ChatEvent::Completed {
                        chat_id: cid,
                        message,
                    } => {
                        rec.chat(&cid);
                        opened = Some(cid);
                        if !message.content.is_empty() {
                            // Aucun delta reçu : le panneau n'a encore rien vu de ce texte.
                            if text.is_empty() {
                                rec.delta(&message.content);
                            }
                            text = message.content;
                        }
                        calls = message.tool_calls;
                    }
                    ChatEvent::Error(e) => failed = Some(e.message),
                    ChatEvent::Cancelled { .. } => cancelled = true,
                    _ => {}
                };
                let fut = async {
                    match chat_id.as_deref() {
                        Some(id) => {
                            env.client
                                .send_message_with_options(
                                    id,
                                    "",
                                    Some(job.agent_id),
                                    &tools,
                                    &results,
                                    &opts,
                                    handler,
                                )
                                .await
                        }
                        None => {
                            env.client
                                .stream_chat_with_options(
                                    job.agent_id,
                                    &prompt,
                                    &tools,
                                    &opts,
                                    handler,
                                )
                                .await
                        }
                    }
                };
                // L'appel est interrompu dès que l'utilisateur annule (agent ou génération).
                tokio::select! {
                    r = fut => {
                        if let Err(e) = r {
                            transport = Some(e.to_string());
                        }
                    }
                    _ = live::wait_cancel(env.flag, rec.cancel_flag()) => {
                        aborted = true;
                    }
                }
            }
            if chat_id.is_none() {
                chat_id = opened.take();
            }
            rec.step(step + 1, &text, &reasoning);
            if aborted {
                // Arrêt de la génération côté serveur, au mieux et sans attendre trop longtemps.
                if let Some(id) = chat_id.as_deref() {
                    let _ = tokio::time::timeout(Duration::from_secs(5), env.client.cancel(id))
                        .await;
                }
                break 'run Err(CANCELLED.to_string());
            }
            if let Some(e) = transport.take() {
                break 'run Err(friendly_error(&e).0);
            }
            if let Some(m) = failed.take() {
                break 'run Err(friendly_error(&m).0);
            }
            if cancelled {
                break 'run Err(CANCELLED.to_string());
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
                if env.flag.load(Ordering::SeqCst) || rec.cancelled() {
                    break 'run Err(CANCELLED.to_string());
                }
                let (output, is_error) =
                    tool_call(env, sw, rec, job.idx, &spec.name, job.role, &call).await;
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

    rec.flush();
    // Chat temporaire : suppression côté serveur (quelques essais). En cas d'échec, l'identifiant
    // reste en attente et sera retenté au prochain lancement d'agents ; en attendant il reste masqué.
    if let Some(id) = chat_id.as_deref() {
        let ok = live::delete_remote(env.app, env.client, id, &[0, 1, 3]).await;
        rec.deleted(id, ok);
    }
    outcome
}

/// Un appel d'outil d'un sous-agent : mêmes autorisations que l'agent principal.
/// Dans le chat principal, seule une **demande d'autorisation** apparaît (sinon l'utilisateur ne
/// pourrait pas répondre) ; tout le reste va au panneau des agents.
async fn tool_call(
    env: &Env<'_>,
    sw: &Swarm,
    rec: &Rec,
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
    // Chemin réservé à l'instant par cet appel (libéré si l'utilisateur refuse).
    let mut newly_claimed: Option<String> = None;
    if role == Role::Explore && risk != Some(Risk::Safe) {
        blocked = Some("Refusé : sous-agent en lecture seule (rôle explore).".to_string());
    } else if let Some(c) = cmd.as_deref() {
        if agent_rest(c).is_some() || crate::mcp_ctx::mcp_rest(c).is_some() {
            blocked =
                Some("Refusé : un sous-agent ne peut pas lancer `agent` ni `mcp`.".to_string());
        }
    }
    // Propriété des fichiers : un worker ne peut pas modifier un fichier d'un autre agent.
    if blocked.is_none() && role == Role::Worker && risk != Some(Risk::Safe) && cmd.is_none() {
        if let Some(path) = fs_tools::any_str(&args, &["path", "file_path", "filename"]) {
            match sw.claim(path, label) {
                Ok(true) => newly_claimed = Some(path.to_string()),
                Ok(false) => {}
                Err(owner) => {
                    blocked = Some(format!(
                        "Refusé : « {path} » est déjà modifié par l'agent « {owner} ». Ne le modifie pas ; mentionne le besoin dans ton compte rendu."
                    ));
                }
            }
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
    // Panneau des agents : l'appel (et, plus bas, son résultat).
    rec.tool(&uid, &call.name, &shown, needs_confirm);

    // Chat principal : uniquement la demande d'autorisation.
    if needs_confirm {
        let _ = env.on_event.send(UiEvent::ToolCall {
            call_id: uid.clone(),
            name: call.name.clone(),
            origin: "local".to_string(),
            args: shown,
            needs_confirm: true,
        });
    }

    let allowed = if needs_confirm {
        tokio::select! {
            a = ask(&state, env.run, &uid) => a,
            _ = live::wait_cancel(env.flag, rec.cancel_flag()) => {
                // Annulation pendant l'attente : on retire la demande restée en suspens.
                if let Ok(mut m) = state.pending.lock() {
                    m.remove(&uid);
                }
                false
            }
        }
    } else {
        true
    };
    let denied = blocked.is_some() || !allowed;
    let cancelled = env.flag.load(Ordering::SeqCst) || rec.cancelled();
    if denied {
        if let Some(p) = newly_claimed.as_deref() {
            sw.release(p, label);
        }
    }

    let (output, is_error) = if let Some(m) = blocked {
        (m, true)
    } else if !allowed {
        let m = if cancelled {
            "Annulé."
        } else {
            "Refusé par l'utilisateur."
        };
        (m.to_string(), true)
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

    rec.tool_result(&uid, &clip(&output, SUB_OUT_CAP), is_error, denied);
    if needs_confirm {
        // Referme la carte d'autorisation du chat principal, sans y recopier la sortie.
        let short = if !denied {
            format!(
                "Autorisé : exécuté par l'agent « {label} » (détails dans le panneau des agents)."
            )
        } else if cancelled {
            "Annulé.".to_string()
        } else {
            "Refusé.".to_string()
        };
        let _ = env.on_event.send(UiEvent::ToolResult {
            call_id: uid,
            output: short,
            is_error: denied,
            denied,
        });
    }
    // Transparence : les commandes des agents restent visibles dans le terminal intégré.
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
        rec.note(&format!("✎ {lbl} · {status}"));
    }
    (output, is_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rest_prefix() {
        assert_eq!(agent_rest("tasks run {}"), Some("run {}"));
        assert_eq!(agent_rest("tasks"), Some(""));
        assert_eq!(agent_rest("tasksx run"), None);
        assert_eq!(agent_rest("git status"), None);
    }

    #[test]
    fn parse_forms() {
        let one = parse("run {\"task\":\"lis a.txt\",\"role\":\"explore\",\"name\":\"x\"}")
            .unwrap()
            .unwrap();
        assert_eq!(one.specs.len(), 1);
        assert!(one.specs[0].role == Role::Explore);
        assert_eq!(one.specs[0].name, "x");
        assert_eq!(one.specs[0].wave, 1);
        assert!(one.contract.is_empty());

        let many = parse("parallel [{\"task\":\"a\"},{\"task\":\"b\"}]")
            .unwrap()
            .unwrap();
        assert_eq!(many.specs.len(), 2);
        assert!(many.specs[0].role == Role::Worker);
        assert_eq!(many.specs[1].name, "agent-2");

        assert!(parse("").unwrap().is_none());
        assert!(parse("run {\"role\":\"explore\"}").is_err());
    }

    #[test]
    fn parse_contract_and_waves() {
        let p = parse(
            "parallel {\"contract\":\"index.html + app.js\",\"tasks\":[{\"task\":\"a\",\"wave\":1},{\"task\":\"b\",\"wave\":2},{\"task\":\"c\",\"wave\":9}]}",
        )
        .unwrap()
        .unwrap();
        assert_eq!(p.contract, "index.html + app.js");
        assert_eq!(p.specs[0].wave, 1);
        assert_eq!(p.specs[1].wave, 2);
        // Vague hors bornes : ramenée au maximum.
        assert_eq!(p.specs[2].wave, MAX_WAVES);
    }

    #[test]
    fn parse_tasks_key_and_legacy_alias() {
 let rest = agent_rest(
     "tasks parallel {\"contract\":\"c\",\"tasks\":[{\"task\":\"a\"},{\"task\":\"b\",\"wave\":2}]}",
 )
        .unwrap();
        let p = parse(rest).unwrap().unwrap();
        assert_eq!(p.contract, "c");
 assert_eq!(p.specs.len(), 2);
        assert_eq!(p.specs[1].wave, 2);
        // L'ancienne clé reste acceptée.
        let old = parse("parallel {\"contract\":\"c\",\"agents\":[{\"task\":\"a\"}]}")
            .unwrap()
     .unwrap();
 assert_eq!(old.contract, "c");
 assert_eq!(old.specs.len(), 1);
    }

    #[test]
    fn parse_limits() {
        let nine: Vec<String> = (0..9).map(|i| format!("{{\"task\":\"t{i}\"}}")).collect();
        // 9 agents dans la même vague : refusé.
        assert!(parse(&format!("parallel [{}]", nine.join(","))).is_err());
        // Les mêmes 9 agents répartis sur deux vagues : accepté.
        let split: Vec<String> = (0..9)
            .map(|i| format!("{{\"task\":\"t{i}\",\"wave\":{}}}", if i < 5 { 1 } else { 2 }))
            .collect();
        assert!(parse(&format!("parallel [{}]", split.join(","))).is_ok());
        // Plus de 12 agents au total : refusé.
        let many: Vec<String> = (0..13)
            .map(|i| format!("{{\"task\":\"t{i}\",\"wave\":{}}}", (i % 3) + 1))
            .collect();
        assert!(parse(&format!("parallel [{}]", many.join(","))).is_err());
    }

    fn swarm() -> Swarm {
        Swarm::new(
            String::new(),
            Vec::new(),
            HashMap::new(),
            PathBuf::new(),
            "t".to_string(),
        )
    }

    #[test]
    fn file_ownership() {
        let sw = swarm();
        assert_eq!(sw.claim("src/a.rs", "x"), Ok(true));
        assert_eq!(sw.claim(".\\SRC\\a.rs", "x"), Ok(false));
        assert_eq!(sw.claim("src/a.rs", "y"), Err("x".to_string()));
        sw.release("src/a.rs", "y");
        assert_eq!(sw.claim("src/a.rs", "y"), Err("x".to_string()));
        sw.release("src/a.rs", "x");
        assert_eq!(sw.claim("src/a.rs", "y"), Ok(true));
    }

    #[test]
    fn model_names() {
        let mut m = HashMap::new();
        m.insert(7_i64, "Modèle A".to_string());
        let sw = Swarm::new(
            String::new(),
            Vec::new(),
            m,
            PathBuf::new(),
            "t".to_string(),
        );
        assert_eq!(sw.model_name(7), "Modèle A");
        assert_eq!(sw.model_name(8), "modèle 8");
    }

    #[test]
    fn failover_rules() {
        assert!(can_failover("HTTP 500 : erreur serveur", false));
        // Annulation (utilisateur ou génération) : pas de seconde tentative.
        assert!(!can_failover("HTTP 500 : erreur serveur", true));
        assert!(!can_failover(CANCELLED, false));
        assert!(!can_failover("limite de 20 étapes atteinte", false));
        // 403 avec page HTML : bloqué en amont, changer de modèle n'y change rien.
        let (msg, gateway) = friendly_error(
            "HTTP 403 : <!DOCTYPE html><html><body>blocked. Ray ID: 9a1b2c3d4e5f6a7b</body></html>",
        );
        assert!(gateway);
        assert!(!can_failover(&msg, false));
    }
}
