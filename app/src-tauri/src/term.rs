//! Terminal intégré : un shell persistant (PowerShell sous Windows, `sh` ailleurs).
//!
//! - La sortie est envoyée en continu à l'interface (événement `term:event`).
//! - Les commandes lancées par l'IA (`run_command`) sont recopiées avec leur sortie
//!   (événement `term:ai`), sans passer par ce shell.
//! - Chaque commande est suivie d'un marqueur (`__FSDONE__<dossier>`) qui indique qu'elle est
//!   terminée et donne le dossier courant.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};

const MARK: &str = "__FSDONE__";

#[cfg(windows)]
const INIT: &str = "try{[Console]::OutputEncoding=[System.Text.Encoding]::UTF8}catch{};$OutputEncoding=[System.Text.Encoding]::UTF8;$ProgressPreference='SilentlyContinue'\n";
#[cfg(not(windows))]
const INIT: &str = "\n";

#[cfg(windows)]
const MARKER: &str = "Write-Output ('__FSDONE__' + (Get-Location).Path)\n";
#[cfg(not(windows))]
const MARKER: &str = "printf '__FSDONE__%s\\n' \"$PWD\"\n";

struct Session {
    sid: u64,
    stdin: Arc<tokio::sync::Mutex<ChildStdin>>,
    child: Child,
}

/// Sessions de terminal, par identifiant (l'interface n'en utilise qu'une : `main`).
#[derive(Default)]
pub(crate) struct Terms {
    map: Mutex<HashMap<String, Session>>,
}

fn emit(app: &AppHandle, sid: u64, kind: &str, data: &str) {
    let _ = app.emit(
        "term:event",
        json!({ "sid": sid, "kind": kind, "data": data }),
    );
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
}

fn shell_cmd() -> Command {
    #[cfg(windows)]
    {
        let mut c = Command::new("powershell.exe");
        c.args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "-",
        ]);
        // CREATE_NO_WINDOW : pas de fenêtre console.
        c.creation_flags(0x0800_0000);
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = Command::new("sh");
        c.arg("-s");
        c
    }
}

/// Extrait de `buf` le plus long préfixe UTF-8 valide (le reste attend l'octet suivant).
fn drain_utf8(buf: &mut Vec<u8>) -> String {
    let (valid, bad) = match std::str::from_utf8(buf.as_slice()) {
        Ok(_) => (buf.len(), false),
        Err(e) => (e.valid_up_to(), e.error_len().is_some()),
    };
    if bad {
        let s = String::from_utf8_lossy(buf.as_slice()).into_owned();
        buf.clear();
        s
    } else {
        let s = String::from_utf8_lossy(&buf[..valid]).into_owned();
        buf.drain(..valid);
        s
    }
}

/// Le reste d'une ligne est peut-être le début du marqueur : on l'attend avant de l'afficher.
fn could_be_mark(s: &str) -> bool {
    MARK.starts_with(s) || s.contains(MARK)
}

async fn pump_out(app: AppHandle, sid: u64, id: String, mut out: ChildStdout) {
    let mut raw: Vec<u8> = Vec::new();
    let mut text = String::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = match out.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        raw.extend_from_slice(&chunk[..n]);
        text.push_str(&drain_utf8(&mut raw));

        while let Some(i) = text.find('\n') {
            let line: String = text.drain(..=i).collect();
            match line.find(MARK) {
                Some(p) => {
                    if p > 0 {
                        emit(&app, sid, "out", &format!("{}\n", &line[..p]));
                    }
                    emit(&app, sid, "done", line[p + MARK.len()..].trim_end());
                }
                None => emit(&app, sid, "out", &line),
            }
        }
        if !text.is_empty() && !could_be_mark(&text) {
            emit(&app, sid, "out", &text);
            text.clear();
        }
    }
    if !text.is_empty() {
        emit(&app, sid, "out", &text);
    }
    emit(&app, sid, "exit", "");
    if let Some(st) = app.try_state::<crate::AppState>() {
        if let Ok(mut m) = st.terms.map.lock() {
            if m.get(&id).map(|s| s.sid) == Some(sid) {
                m.remove(&id);
            }
        }
    }
}

async fn pump_err(app: AppHandle, sid: u64, mut err: ChildStderr) {
    let mut raw: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = match err.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        raw.extend_from_slice(&chunk[..n]);
        let s = drain_utf8(&mut raw);
        if !s.is_empty() {
            emit(&app, sid, "err", &s);
        }
    }
}

async fn kill_tree(child: &mut Child) {
    #[cfg(windows)]
    {
        if let Some(pid) = child.id() {
            let _ = Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .creation_flags(0x0800_0000)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
        }
    }
    let _ = child.kill().await;
}

/// Démarre le shell (sans effet s'il tourne déjà). `sid` est choisi par l'interface et
/// accompagne chaque événement, pour ignorer ceux d'un ancien shell. Renvoie le `sid` actif.
#[tauri::command]
pub(crate) async fn term_start(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    id: String,
    sid: u64,
    cwd: Option<String>,
) -> Result<u64, String> {
    let existing = {
        let m = state.terms.map.lock().map_err(|e| e.to_string())?;
        let r = m.get(&id).map(|s| s.sid);
        r
    };
    if let Some(s) = existing {
        return Ok(s);
    }

    let dir = cwd
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .or_else(home_dir);

    let mut cmd = shell_cmd();
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .env("NO_COLOR", "1")
        .env("TERM", "dumb");
    if let Some(d) = &dir {
        cmd.current_dir(d);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Shell impossible à lancer : {e}"))?;
    let mut stdin = child.stdin.take().ok_or("stdin indisponible")?;
    let stdout = child.stdout.take().ok_or("stdout indisponible")?;
    let stderr = child.stderr.take().ok_or("stderr indisponible")?;

    stdin
        .write_all(format!("{INIT}{MARKER}").as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    stdin.flush().await.map_err(|e| e.to_string())?;

    {
        let mut m = state.terms.map.lock().map_err(|e| e.to_string())?;
        m.insert(
            id.clone(),
            Session {
                sid,
                stdin: Arc::new(tokio::sync::Mutex::new(stdin)),
                child,
            },
        );
    }
    tokio::spawn(pump_out(app.clone(), sid, id, stdout));
    tokio::spawn(pump_err(app, sid, stderr));
    Ok(sid)
}

/// Envoie une commande au shell, suivie du marqueur de fin.
#[tauri::command]
pub(crate) async fn term_write(
    state: State<'_, crate::AppState>,
    id: String,
    text: String,
) -> Result<(), String> {
    let stdin = {
        let m = state.terms.map.lock().map_err(|e| e.to_string())?;
        let r = m.get(&id).map(|s| s.stdin.clone());
        r
    }
    .ok_or("Terminal non démarré")?;

    let line = text.trim_end_matches(|c| c == '\r' || c == '\n');
    let mut w = stdin.lock().await;
    w.write_all(format!("{line}\n{MARKER}").as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    w.flush().await.map_err(|e| e.to_string())?;
    Ok(())
}

/// Interrompt : tue le shell et ses processus fils. L'interface le relance à la commande suivante.
#[tauri::command]
pub(crate) async fn term_kill(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    id: String,
) -> Result<(), String> {
    let sess = {
        let mut m = state.terms.map.lock().map_err(|e| e.to_string())?;
        let r = m.remove(&id);
        r
    };
    if let Some(mut s) = sess {
        kill_tree(&mut s.child).await;
        emit(&app, s.sid, "exit", "");
    }
    Ok(())
}

/// Recopie dans le terminal une commande lancée par l'IA et sa sortie.
pub(crate) fn emit_ai(
    app: &AppHandle,
    command: &str,
    output: &str,
    is_error: bool,
    cwd: Option<&Path>,
) {
    let _ = app.emit(
        "term:ai",
        json!({
            "command": command,
            "output": output,
            "isError": is_error,
            "cwd": cwd.map(|p| p.display().to_string()),
        }),
    );
}
