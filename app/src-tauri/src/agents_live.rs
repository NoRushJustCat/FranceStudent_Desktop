//! Suivi en direct des sous-agents, transcriptions locales et chats serveur invisibles.
//!
//! - **Rien dans le chat principal** : l'interface reçoit l'événement `agents:live` (et relit
//!   `agents_live_snapshot` au montage) et l'affiche dans son panneau « agents ».
//! - **Transcription sur le PC** : un fichier JSONL par agent,
//!   `<dossier de config>/agent-chats/<date UTC>_<appel>/NN-<nom>.jsonl`.
//! - **Chats serveur invisibles** : dès son ouverture, le chat serveur d'un agent est masqué des
//!   listes (`hide_chats`), puis supprimé (`DELETE /chats/{id}`) à la fin de l'agent. Une
//!   suppression qui échoue est mémorisée dans `pending-delete.json` et retentée au prochain
//!   lancement d'agents. Côté serveur, DELETE peut n'être qu'un marquage « supprimé » :
//!   l'application ne peut pas garantir un effacement définitif.
//!
//! Règle d'or : un seul verrou à la fois (jamais de verrou pris pendant qu'un autre est tenu),
//! et aucun verrou tenu pendant un `.await`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fs_api::Client;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::{clip, config_dir};

/// Entrées gardées en mémoire pour le rattrapage (`agents_live_snapshot`).
const RING_MAX: usize = 3000;
/// Agents gardés dans le panneau (les plus anciens terminés sont retirés au-delà).
const KEEP_AGENTS: usize = 60;
/// Les deltas de texte sont regroupés : au plus ~150 ms ou ~400 octets par morceau.
const CHUNK_BYTES: usize = 400;
const CHUNK_MS: u128 = 150;
/// Échecs de suppression après lesquels on abandonne un chat en attente.
const MAX_TRIES: u32 = 5;
/// Chats en attente retentés à chaque lancement (une seule tentative chacun).
const SWEEP_MAX: usize = 10;
/// Plafonds d'affichage dans le panneau (la transcription locale, elle, est complète).
const UI_ARG_CAP: usize = 1500;
const UI_OUT_CAP: usize = 3000;

/// Libellé d'annulation (comparé tel quel par `agents.rs`).
pub(crate) const CANCELLED: &str = "annulé";
/// Présent dans le message quand la passerelle HTTP (pare-feu/CDN) a bloqué la requête.
pub(crate) const GATEWAY_TAG: &str = "bloquée par la passerelle";

// ---------- État partagé ----------

/// Un agent tel que le panneau l'affiche.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LiveAgent {
    id: String,
    run: String,
    name: String,
    role: String,
    wave: u32,
    model: String,
    task: String,
    /// `running`, `done`, `failed` ou `cancelled`.
    status: String,
    started: u64,
    ended: Option<u64>,
    steps: u32,
    last: String,
    /// Chemin de la transcription locale (vide si le dossier de config est introuvable).
    file: String,
    /// `None` = pas encore tenté ; `Some(false)` = suppression serveur à retenter.
    chat_deleted: Option<bool>,
}

struct Live {
    seq: u64,
    agents: Vec<LiveAgent>,
    ring: VecDeque<Value>,
    /// Drapeau d'annulation par agent.
    cancel: HashMap<String, Arc<AtomicBool>>,
}

fn live() -> MutexGuard<'static, Live> {
    static L: OnceLock<Mutex<Live>> = OnceLock::new();
    L.get_or_init(|| {
        Mutex::new(Live {
            seq: 0,
            agents: Vec::new(),
            ring: VecDeque::new(),
            cancel: HashMap::new(),
        })
    })
    .lock()
    .unwrap_or_else(|e| e.into_inner())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Numérote l'entrée, la garde pour le rattrapage, puis l'envoie à l'interface.
/// L'émission se fait hors verrou ; l'ordre reste correct par agent (un agent = une tâche).
fn publish(app: &AppHandle, mut entry: Value) {
    let e = {
        let mut l = live();
        l.seq += 1;
        entry["seq"] = json!(l.seq);
        l.ring.push_back(entry.clone());
        while l.ring.len() > RING_MAX {
            l.ring.pop_front();
        }
        entry
    };
    let _ = app.emit("agents:live", e);
}

/// Modifie la fiche d'un agent puis publie son nouvel état.
fn modify(app: &AppHandle, id: &str, f: impl FnOnce(&mut LiveAgent)) {
    let snap = {
        let mut l = live();
        match l.agents.iter_mut().find(|a| a.id == id) {
            Some(a) => {
                f(&mut *a);
                Some(a.clone())
            }
            None => None,
        }
    };
    if let Some(a) = snap {
        publish(app, json!({"kind": "agent", "agent": a.id, "data": a}));
    }
}

fn trim_agents(l: &mut Live) {
    while l.agents.len() > KEEP_AGENTS {
        match l.agents.iter().position(|a| a.status != "running") {
            Some(i) => {
                let a = l.agents.remove(i);
                l.cancel.remove(&a.id);
            }
            None => break,
        }
    }
}

/// Message du journal général (vagues, bilans), rattaché à aucun agent.
pub(crate) fn note(app: &AppHandle, text: &str) {
    publish(
        app,
        json!({"kind": "note", "agent": "", "text": text, "t": now_ms()}),
    );
}

// ---------- Dossiers, noms, dates ----------

fn root_dir(app: &AppHandle) -> Option<PathBuf> {
    config_dir(app).ok().map(|d| d.join("agent-chats"))
}

/// `AAAA-MM-JJTHH-MM-SSZ` en UTC (algorithme « civil from days » de H. Hinnant).
fn utc_stamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{h:02}-{mi:02}-{s:02}Z")
}

/// Nom de fichier sûr (lettres, chiffres, `-`, `_`).
fn slug(s: &str, max: usize) -> String {
    let t: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .take(max)
        .collect();
    let t = t.trim_matches('-').to_string();
    if t.is_empty() {
        "agent".to_string()
    } else {
        t
    }
}

/// Les `n` derniers caractères alphanumériques ASCII.
fn tail(s: &str, n: usize) -> String {
    let t: String = s.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let skip = t.chars().count().saturating_sub(n);
    t.chars().skip(skip).collect()
}

/// Nouveau lancement : (dossier des transcriptions, clé du lancement).
/// Dossier vide = pas de transcription (dossier de config introuvable).
pub(crate) fn new_run(app: &AppHandle, parent_call: &str) -> (PathBuf, String) {
    let mut t = tail(parent_call, 8);
    if t.is_empty() {
        t = "run".to_string();
    }
    let key = format!("{}_{t}", utc_stamp(now_ms() / 1000));
    let dir = match root_dir(app) {
        Some(d) => d.join(&key),
        None => PathBuf::new(),
    };
    (dir, key)
}

fn append(path: &Path, v: &Value) {
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{v}");
    }
}

/// Copie d'un JSON où les longues chaînes sont tronquées (affichage seulement).
fn shrink(v: &Value, max: usize) -> Value {
    match v {
        Value::String(s) if s.chars().count() > max => Value::String(clip(s, max)),
        Value::Array(a) => Value::Array(a.iter().map(|x| shrink(x, max)).collect()),
        Value::Object(m) => Value::Object(m.iter().map(|(k, x)| (k.clone(), shrink(x, max))).collect()),
        other => other.clone(),
    }
}

fn tool_summary(name: &str, args: &Value) -> String {
    let arg = fs_tools::any_str(
        args,
        &["command", "path", "file_path", "filename", "query", "pattern"],
    )
    .unwrap_or("");
    clip(format!("{name} {arg}").trim(), 120)
}

fn last_line(s: &str) -> String {
    s.lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(|l| clip(l, 140))
        .unwrap_or_default()
}

// ---------- Erreurs HTTP lisibles ----------

/// Extrait un « Ray ID » (page de blocage Cloudflare) d'un corps HTML.
fn ray_id(s: &str) -> Option<String> {
    let low = s.to_lowercase();
    let at = low.find("ray id")?;
    let tail = s.get(at..)?;
    let mut visible = String::new();
    let mut in_tag = false;
    for c in tail.chars().take(300) {
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                visible.push(' ');
            }
            _ if !in_tag => visible.push(c),
            _ => {}
        }
    }
    visible
        .split(|c: char| !c.is_ascii_hexdigit())
        .find(|t| t.len() >= 12 && t.len() <= 32)
        .map(str::to_string)
}

/// Résume une erreur HTTP : jamais de page HTML brute dans l'interface.
/// Le booléen vaut `true` pour un 403 avec page HTML (requête bloquée en amont de l'API) :
/// réessayer ou changer de modèle ne sert alors à rien.
pub(crate) fn friendly_error(e: &str) -> (String, bool) {
    let low = e.to_lowercase();
    let html = low.contains("<html") || low.contains("<!doctype") || low.contains("</body");
    if !html {
        return (e.to_string(), false);
    }
    let status: String = e
        .strip_prefix("HTTP ")
        .unwrap_or("")
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let ray = ray_id(e)
        .map(|r| format!(" [Ray ID {r}]"))
        .unwrap_or_default();
    if status == "403" {
        (
            format!("HTTP 403 (page HTML) : requête probablement {GATEWAY_TAG} (pare-feu/CDN){ray}. Pas de nouvelle tentative automatique ; à examiner côté Cloudflare ou avec le support."),
            true,
        )
    } else {
        let s = if status.is_empty() {
            String::new()
        } else {
            format!("HTTP {status} : ")
        };
        (
            format!("{s}page HTML reçue à la place de la réponse de l'API{ray}"),
            false,
        )
    }
}

// ---------- Chats serveur invisibles puis supprimés ----------

#[derive(Default)]
struct Hide {
    /// Chats d'agents connus de cette session : jamais listés.
    hidden: HashSet<String>,
    /// Chats d'agents encore en cours d'utilisation (pas touchés par `sweep`).
    active: HashSet<String>,
}

fn hide() -> MutexGuard<'static, Hide> {
    static H: OnceLock<Mutex<Hide>> = OnceLock::new();
    H.get_or_init(|| Mutex::new(Hide::default()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Protège `pending-delete.json` (lecture-modification-écriture).
static FILE_LOCK: Mutex<()> = Mutex::new(());

fn pending_path(app: &AppHandle) -> Option<PathBuf> {
    root_dir(app).map(|d| d.join("pending-delete.json"))
}

fn pending_read(path: &Path) -> HashMap<String, u32> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn pending_write(path: &Path, m: &HashMap<String, u32>) {
    if m.is_empty() {
        let _ = fs::remove_file(path);
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(s) = serde_json::to_string(m) {
        let tmp = path.with_extension("tmp");
        if fs::write(&tmp, s).is_ok() {
            let _ = fs::rename(&tmp, path);
        }
    }
}

fn pending_edit(app: &AppHandle, f: impl FnOnce(&mut HashMap<String, u32>)) {
    let Some(path) = pending_path(app) else {
        return;
    };
    let _g = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut m = pending_read(&path);
    f(&mut m);
    pending_write(&path, &m);
}

/// Masque ce chat des listes et le note « à supprimer ». `true` = première fois.
fn hide_register(app: &AppHandle, id: &str) -> bool {
    let fresh = {
        let mut h = hide();
        h.active.insert(id.to_string());
        h.hidden.insert(id.to_string())
    };
    if fresh {
        pending_edit(app, |m| {
            m.entry(id.to_string()).or_insert(0);
        });
    }
    fresh
}

fn is_hidden(x: &Value, ids: &HashSet<String>) -> bool {
    match x.as_object() {
        Some(o) => ["id", "chatId"].iter().any(|k| {
            o.get(*k)
                .and_then(Value::as_str)
                .map(|s| ids.contains(s))
                .unwrap_or(false)
        }),
        None => false,
    }
}

/// Retire récursivement les éléments (objets) dont `id` ou `chatId` est masqué.
fn strip_ids(v: &mut Value, ids: &HashSet<String>) {
    match v {
        Value::Array(a) => {
            a.retain(|x| !is_hidden(x, ids));
            for x in a.iter_mut() {
                strip_ids(x, ids);
            }
        }
        Value::Object(m) => {
            for (_, x) in m.iter_mut() {
                strip_ids(x, ids);
            }
        }
        _ => {}
    }
}

/// À appeler sur le JSON renvoyé par `list_chats` / `search_chats` : les chats des agents
/// (ceux de cette session et ceux dont la suppression est encore en attente) disparaissent.
pub(crate) fn hide_chats(app: &AppHandle, v: &mut Value) {
    let mut ids: HashSet<String> = hide().hidden.clone();
    if let Some(p) = pending_path(app) {
        let _g = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        ids.extend(pending_read(&p).into_keys());
    }
    if !ids.is_empty() {
        strip_ids(v, &ids);
    }
}

/// `DELETE /chats/{id}` avec quelques essais (`waits` = attente avant chaque essai, en s).
/// `true` si le serveur a accepté (ou répond « introuvable »). Sinon l'id reste en attente.
pub(crate) async fn delete_remote(
    app: &AppHandle,
    client: &Client,
    chat_id: &str,
    waits: &[u64],
) -> bool {
    let mut ok = false;
    for w in waits {
        if *w > 0 {
            tokio::time::sleep(Duration::from_secs(*w)).await;
        }
        match client.delete_chat(chat_id).await {
            Ok(_) => {
                ok = true;
                break;
            }
            Err(e) => {
                let s = e.to_string();
                if s.starts_with("HTTP 404") || s.starts_with("HTTP 410") {
                    ok = true;
                    break;
                }
            }
        }
    }
    hide().active.remove(chat_id);
    if ok {
        pending_edit(app, |m| {
            m.remove(chat_id);
        });
    }
    ok
}

/// Retente les suppressions restées en attente (lancements précédents, plantage, réseau).
pub(crate) async fn sweep(app: &AppHandle, client: &Client) {
    let Some(path) = pending_path(app) else {
        return;
    };
    let active: HashSet<String> = hide().active.clone();
    let ids: Vec<String> = {
        let _g = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        pending_read(&path)
            .into_keys()
            .filter(|id| !active.contains(id))
            .take(SWEEP_MAX)
            .collect()
    };
    for id in ids {
        if !delete_remote(app, client, &id, &[0]).await {
            pending_edit(app, |m| {
                let n = m.entry(id.clone()).or_insert(0);
                *n += 1;
                if *n >= MAX_TRIES {
                    m.remove(&id);
                }
            });
        }
    }
}

// ---------- Enregistrement d'un agent ----------

pub(crate) struct NewAgent<'a> {
    pub(crate) run_key: &'a str,
    pub(crate) dir: &'a Path,
    pub(crate) idx: usize,
    pub(crate) name: &'a str,
    pub(crate) role: &'a str,
    pub(crate) wave: u32,
    pub(crate) model: &'a str,
    pub(crate) task: &'a str,
}

struct Buf {
    kind: &'static str,
    text: String,
    at: Instant,
}

/// Tout ce qui concerne un agent : fiche du panneau, flux en direct, transcription locale.
pub(crate) struct Rec {
    app: AppHandle,
    id: String,
    /// Vide = pas de transcription.
    file: PathBuf,
    cancel: Arc<AtomicBool>,
    buf: Mutex<Buf>,
}

impl Rec {
    pub(crate) fn start(app: &AppHandle, n: &NewAgent<'_>) -> Rec {
        let id = format!("{}#{}", n.run_key, n.idx);
        let file = if n.dir.as_os_str().is_empty() {
            PathBuf::new()
        } else {
            let _ = fs::create_dir_all(n.dir);
            n.dir
                .join(format!("{:02}-{}.jsonl", n.idx + 1, slug(n.name, 40)))
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let snap = {
            let mut l = live();
            let a = LiveAgent {
                id: id.clone(),
                run: n.run_key.to_string(),
                name: n.name.to_string(),
                role: n.role.to_string(),
                wave: n.wave,
                model: n.model.to_string(),
                task: clip(n.task, 300),
                status: "running".to_string(),
                started: now_ms(),
                ended: None,
                steps: 0,
                last: String::new(),
                file: file.display().to_string(),
                chat_deleted: None,
            };
            l.cancel.insert(id.clone(), cancel.clone());
            l.agents.push(a.clone());
            trim_agents(&mut l);
            a
        };
        publish(app, json!({"kind": "agent", "agent": id, "data": snap}));
        let rec = Rec {
            app: app.clone(),
            id,
            file,
            cancel,
            buf: Mutex::new(Buf {
                kind: "text",
                text: String::new(),
                at: Instant::now(),
            }),
        };
        rec.write(json!({
            "type": "start", "agent": n.name, "role": n.role, "wave": n.wave,
            "model": n.model, "task": n.task
        }));
        rec
    }

    /// Une ligne JSON dans la transcription (horodatée).
    fn write(&self, mut v: Value) {
        if self.file.as_os_str().is_empty() {
            return;
        }
        v["t"] = json!(now_ms());
        append(&self.file, &v);
    }

    pub(crate) fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    pub(crate) fn cancel_flag(&self) -> &AtomicBool {
        &self.cancel
    }

    pub(crate) fn model(&self, name: &str) {
        modify(&self.app, &self.id, |a| a.model = name.to_string());
    }

    /// Prompt complet envoyé au modèle (dans la transcription seulement).
    pub(crate) fn prompt(&self, text: &str, retry: bool) {
        self.write(json!({"type": "prompt", "retry": retry, "text": text}));
    }

    /// Chat serveur de l'agent : masqué des listes dès maintenant, supprimé à la fin.
    pub(crate) fn chat(&self, chat_id: &str) {
        if hide_register(&self.app, chat_id) {
            self.write(json!({"type": "chat", "chat_id": chat_id}));
        }
    }

    /// Résultat de la suppression serveur du chat de l'agent.
    pub(crate) fn deleted(&self, chat_id: &str, ok: bool) {
        self.write(json!({"type": "chat_deleted", "chat_id": chat_id, "ok": ok}));
        modify(&self.app, &self.id, |a| {
            a.chat_deleted = Some(a.chat_deleted.unwrap_or(true) && ok)
        });
    }

    pub(crate) fn delta(&self, text: &str) {
        self.buffer("text", text);
    }

    pub(crate) fn reasoning(&self, text: &str) {
        self.buffer("reasoning", text);
    }

    fn buffer(&self, kind: &'static str, s: &str) {
        let mut ready: Vec<(&'static str, String)> = Vec::new();
        {
            let mut b = self.buf.lock().unwrap_or_else(|e| e.into_inner());
            // Changement de nature (raisonnement -> texte) : on vide d'abord, pour garder l'ordre.
            if b.kind != kind && !b.text.is_empty() {
                let k = b.kind;
                ready.push((k, std::mem::take(&mut b.text)));
            }
            if b.text.is_empty() {
                b.at = Instant::now();
            }
            b.kind = kind;
            b.text.push_str(s);
            if b.text.len() >= CHUNK_BYTES || b.at.elapsed().as_millis() >= CHUNK_MS {
                let k = b.kind;
                ready.push((k, std::mem::take(&mut b.text)));
            }
        }
        for (k, t) in ready {
            self.chunk(k, &t);
        }
    }

    /// Envoie ce qui reste dans le tampon.
    pub(crate) fn flush(&self) {
        let ready = {
            let mut b = self.buf.lock().unwrap_or_else(|e| e.into_inner());
            if b.text.is_empty() {
                None
            } else {
                let k = b.kind;
                Some((k, std::mem::take(&mut b.text)))
            }
        };
        if let Some((k, t)) = ready {
            self.chunk(k, &t);
        }
    }

    fn chunk(&self, kind: &str, text: &str) {
        publish(
            &self.app,
            json!({"kind": kind, "agent": self.id, "text": text, "t": now_ms()}),
        );
    }

    /// Fin d'une étape (un aller-retour avec le modèle) : texte complet dans la transcription.
    pub(crate) fn step(&self, n: usize, text: &str, reasoning: &str) {
        self.flush();
        if !reasoning.is_empty() {
            self.write(json!({"type": "reasoning", "step": n, "text": reasoning}));
        }
        if !text.is_empty() {
            self.write(json!({"type": "assistant", "step": n, "text": text}));
        }
        let last = last_line(text);
        modify(&self.app, &self.id, |a| {
            a.steps = n as u32;
            if !last.is_empty() {
                a.last = last;
            }
        });
    }

    pub(crate) fn tool(&self, uid: &str, name: &str, args: &Value, needs_confirm: bool) {
        self.flush();
        self.write(json!({
            "type": "tool", "id": uid, "name": name, "args": args, "confirm": needs_confirm
        }));
        let summary = tool_summary(name, args);
        modify(&self.app, &self.id, |a| a.last = summary);
        publish(
            &self.app,
            json!({
                "kind": "tool", "agent": self.id, "uid": uid, "name": name,
                "args": shrink(args, UI_ARG_CAP), "confirm": needs_confirm, "t": now_ms()
            }),
        );
    }

    pub(crate) fn tool_result(&self, uid: &str, output: &str, is_error: bool, denied: bool) {
        self.write(json!({
            "type": "tool_result", "id": uid, "output": output, "error": is_error, "denied": denied
        }));
        publish(
            &self.app,
            json!({
                "kind": "tool_result", "agent": self.id, "uid": uid,
                "output": clip(output, UI_OUT_CAP), "error": is_error, "denied": denied,
                "t": now_ms()
            }),
        );
    }

    pub(crate) fn note(&self, text: &str) {
        self.write(json!({"type": "note", "text": text}));
        publish(
            &self.app,
            json!({"kind": "note", "agent": self.id, "text": text, "t": now_ms()}),
        );
    }

    /// Fin de l'agent : `done`, `failed` ou `cancelled`, avec son compte rendu.
    pub(crate) fn finish(&self, status: &str, report: &str) {
        self.flush();
        self.write(json!({"type": "end", "status": status, "report": report}));
        modify(&self.app, &self.id, |a| {
            a.status = status.to_string();
            a.ended = Some(now_ms());
        });
        live().cancel.remove(&self.id);
    }
}

impl Drop for Rec {
    /// Filet de sécurité : si la génération est arrêtée en plein vol (future abandonnée),
    /// la carte ne reste pas « en cours » pour toujours.
    fn drop(&mut self) {
        let running = live()
            .agents
            .iter()
            .any(|a| a.id == self.id && a.status == "running");
        if running {
            self.write(json!({
                "type": "end", "status": "cancelled",
                "report": "interrompu (génération arrêtée ou erreur interne)"
            }));
            modify(&self.app, &self.id, |a| {
                a.status = "cancelled".to_string();
                a.ended = Some(now_ms());
            });
        }
        live().cancel.remove(&self.id);
    }
}

/// Attend qu'un des deux drapeaux passe à `true` (sondage toutes les 250 ms).
pub(crate) async fn wait_cancel(a: &AtomicBool, b: &AtomicBool) {
    loop {
        if a.load(Ordering::SeqCst) || b.load(Ordering::SeqCst) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

// ---------- Commandes Tauri ----------

/// État complet du panneau : agents, entrées plus récentes que `since`, dossier des transcriptions.
#[tauri::command]
pub(crate) fn agents_live_snapshot(app: AppHandle, since: Option<u64>) -> Value {
    let dir = root_dir(&app)
        .map(|d| d.display().to_string())
        .unwrap_or_default();
    let l = live();
    let ids: HashSet<&str> = l.agents.iter().map(|a| a.id.as_str()).collect();
    let since = since.unwrap_or(0);
    let entries: Vec<&Value> = l
        .ring
        .iter()
        .filter(|e| e["seq"].as_u64().unwrap_or(0) > since)
        .filter(|e| {
            let a = e["agent"].as_str().unwrap_or("");
            a.is_empty() || ids.contains(a)
        })
        .collect();
    let out = json!({"seq": l.seq, "agents": l.agents, "entries": entries, "dir": dir});
    out
}

/// Annule un agent (`id`), ou tous ceux qui tournent (`"*"`). `false` = rien à annuler.
#[tauri::command]
pub(crate) fn agents_live_cancel(app: AppHandle, id: String) -> bool {
    let flags: Vec<Arc<AtomicBool>> = {
        let l = live();
        if id == "*" {
            l.cancel.values().cloned().collect()
        } else {
            l.cancel.get(&id).cloned().into_iter().collect()
        }
    };
    for f in &flags {
        f.store(true, Ordering::SeqCst);
    }
    if !flags.is_empty() {
        note(&app, "annulation demandée");
    }
    !flags.is_empty()
}

/// Retire du panneau les agents terminés (les transcriptions sur le PC ne sont pas touchées).
#[tauri::command]
pub(crate) fn agents_live_clear(app: AppHandle) {
    {
        let mut l = live();
        l.agents.retain(|a| a.status == "running");
    }
    publish(&app, json!({"kind": "clear", "agent": ""}));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamp_utc() {
        assert_eq!(utc_stamp(0), "1970-01-01T00-00-00Z");
        assert_eq!(utc_stamp(1_000_000_000), "2001-09-09T01-46-40Z");
        assert_eq!(utc_stamp(1_709_164_800), "2024-02-29T00-00-00Z");
    }

    #[test]
    fn slug_and_tail() {
        assert_eq!(slug("API / back:end", 40), "API---back-end");
        assert_eq!(slug("///", 40), "agent");
        assert_eq!(tail("call_AbC-12345678", 8), "12345678");
    }

    #[test]
    fn gateway_403_is_summarised() {
        let raw = "HTTP 403 : <!DOCTYPE html><html><body>Sorry, you have been blocked. Cloudflare Ray ID: <strong>9a1b2c3d4e5f6a7b</strong></body></html>";
        let (msg, gateway) = friendly_error(raw);
        assert!(gateway);
        assert!(msg.contains(GATEWAY_TAG));
        assert!(msg.contains("9a1b2c3d4e5f6a7b"));
        assert!(!msg.contains("<html"));

        let (plain, g2) = friendly_error("HTTP 429 : trop de requêtes");
        assert!(!g2);
        assert_eq!(plain, "HTTP 429 : trop de requêtes");

        let (m502, g3) = friendly_error("HTTP 502 : <html><body>Bad gateway</body></html>");
        assert!(!g3);
        assert!(m502.contains("502") && !m502.contains("<html"));
    }

    #[test]
    fn hidden_chats_are_stripped() {
        let ids: HashSet<String> = ["a".to_string()].into_iter().collect();
        let mut v = json!({"items": [{"id": "a", "title": "x"}, {"id": "b"}], "next": null});
        strip_ids(&mut v, &ids);
        assert_eq!(v["items"].as_array().unwrap().len(), 1);
        let mut w = json!([{"chatId": "a"}, {"id": "c"}]);
        strip_ids(&mut w, &ids);
        assert_eq!(w.as_array().unwrap().len(), 1);
    }

    #[test]
    fn ui_args_are_shortened() {
        let v = json!({"content": "x".repeat(5000), "n": 1});
        let s = shrink(&v, 100);
        assert!(s["content"].as_str().unwrap().chars().count() < 5000);
        assert_eq!(s["n"], 1);
    }
}
