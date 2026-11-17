//! Modes d'autorisation : qui décide si un outil s'exécute sans confirmation.
//!
//! - `Ask`     : confirmation pour les éditions et les commandes (comportement historique).
//! - `Plan`    : lecture seule ; éditions et commandes refusées, l'IA présente un plan.
//! - `Auto`    : règles heuristiques (pas un jugement du modèle) : éditions du projet et
//!               commandes « connues sûres » sans confirmation, le reste est demandé.
//! - `Project` : tout dans le projet sans confirmation, sauf commandes dangereuses
//!               ou qui sortent du projet.
//! - `Full`    : tout sans confirmation, chemins absolus hors projet compris.

use std::path::Path;

use fs_tools::Risk;
use serde_json::Value;
use tauri::State;

use crate::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Mode {
    #[default]
    Ask,
    Plan,
    Auto,
    Project,
    Full,
}

impl Mode {
    pub(crate) fn parse(s: &str) -> Option<Mode> {
        match s.trim().to_lowercase().as_str() {
            "ask" | "demander" => Some(Mode::Ask),
            "plan" | "planification" => Some(Mode::Plan),
            "auto" => Some(Mode::Auto),
            "project" | "projet" => Some(Mode::Project),
            "full" | "system" | "systeme" | "système" => Some(Mode::Full),
            _ => None,
        }
    }

    pub(crate) fn id(self) -> &'static str {
        match self {
            Mode::Ask => "ask",
            Mode::Plan => "plan",
            Mode::Auto => "auto",
            Mode::Project => "project",
            Mode::Full => "full",
        }
    }
}

/// Décision pour un appel d'outil local.
pub(crate) enum Verdict {
    /// Exécuter sans confirmation.
    Allow,
    /// Demander à l'utilisateur.
    Ask,
    /// Refuser sans demander ; le message part au modèle.
    Deny(String),
}

const PLAN_DENY: &str = "Refusé (mode planification) : aucune modification ni commande pour l'instant. Explore en lecture seule (list_directory, read_file, search_files), puis présente un plan numéroté ; l'utilisateur validera en changeant de mode.";

pub(crate) fn current(state: &AppState) -> Mode {
    state.perm.lock().map(|m| *m).unwrap_or_default()
}

/// Faut-il demander pour un outil local (`name`, `args`) ?
pub(crate) fn decide(
    mode: Mode,
    auto_accept: bool,
    name: &str,
    args: &Value,
    root: Option<&Path>,
) -> Verdict {
    let risk = match fs_tools::risk(name) {
        Some(r) => r,
        None => return Verdict::Allow,
    };
    if risk == Risk::Safe {
        return Verdict::Allow;
    }
    match mode {
        Mode::Full => Verdict::Allow,
        Mode::Plan => Verdict::Deny(PLAN_DENY.to_string()),
        Mode::Ask => {
            if risk == Risk::Edit && auto_accept {
                Verdict::Allow
            } else {
                Verdict::Ask
            }
        }
        Mode::Auto | Mode::Project => {
            if risk == Risk::Edit {
                Verdict::Allow
            } else {
                command_verdict(mode, args, root)
            }
        }
    }
}

fn command_verdict(mode: Mode, args: &Value, root: Option<&Path>) -> Verdict {
    let cmd = fs_tools::any_str(args, &["command", "cmd"]).unwrap_or("");
    let low = cmd.to_lowercase();
    if dangerous(&low) || escapes(&low, root) {
        return Verdict::Ask;
    }
    if let Some(cwd) = fs_tools::any_str(args, &["cwd", "workdir", "working_directory"]) {
        if escapes(&cwd.to_lowercase(), root) {
            return Verdict::Ask;
        }
    }
    if mode == Mode::Project || known_safe(&low) {
        Verdict::Allow
    } else {
        Verdict::Ask
    }
}

// ---------- analyse des commandes (sur texte en minuscules) ----------

const DANGEROUS_HEADS: [&str; 46] = [
    "shutdown",
    "format",
    "diskpart",
    "reg",
    "regedit",
    "sc",
    "net",
    "netsh",
    "bcdedit",
    "takeown",
    "icacls",
    "cacls",
    "schtasks",
    "taskkill",
    "stop-process",
    "kill",
    "sudo",
    "su",
    "chown",
    "dd",
    "mkfs",
    "iex",
    "invoke-expression",
    "setx",
    "set-executionpolicy",
    "winget",
    "choco",
    "scoop",
    "msiexec",
    "restart-computer",
    "stop-computer",
    "clear-disk",
    "remove-partition",
    "format-volume",
    "powershell",
    "pwsh",
    "cmd",
    "wsl",
    "bash",
    "sh",
    "start-process",
    "mshta",
    "rundll32",
    "regsvr32",
    "certutil",
    "bitsadmin",
];

const SAFE_HEADS: [&str; 51] = [
    "ls",
    "dir",
    "cat",
    "type",
    "get-childitem",
    "gci",
    "get-content",
    "gc",
    "select-string",
    "sls",
    "findstr",
    "rg",
    "grep",
    "pwd",
    "get-location",
    "echo",
    "write-output",
    "write-host",
    "where",
    "which",
    "get-command",
    "head",
    "tail",
    "wc",
    "sort",
    "measure-object",
    "test-path",
    "tree",
    "get-item",
    "resolve-path",
    "select-object",
    "where-object",
    "sort-object",
    "format-table",
    "out-string",
    "get-date",
    "hostname",
    "whoami",
    "cd",
    "set-location",
    "push-location",
    "pop-location",
    "mkdir",
    "md",
    "clear-host",
    "cls",
    "tsc",
    "eslint",
    "vitest",
    "jest",
    "pytest",
];

const SAFE_TOOLS_2: [&str; 4] = ["ruff", "mypy", "prettier", "rustfmt"];

const ENV_VARS: [&str; 20] = [
    "$home",
    "$env:userprofile",
    "$env:appdata",
    "$env:localappdata",
    "$env:windir",
    "$env:systemroot",
    "$env:programfiles",
    "$env:programdata",
    "$env:homepath",
    "$env:homedrive",
    "$env:temp",
    "$env:tmp",
    "%userprofile%",
    "%appdata%",
    "%localappdata%",
    "%windir%",
    "%systemroot%",
    "%programfiles%",
    "%programdata%",
    "%temp%",
];

fn unquote(t: &str) -> &str {
    t.trim_matches(|c: char| c == '"' || c == '\'' || c == '`')
}

/// Nom de programme sans dossier ni extension (`C:\x\Git.exe` → `git`).
fn base(tok: &str) -> String {
    let t = unquote(tok);
    let t = t
        .rsplit(|c: char| c == '\\' || c == '/')
        .next()
        .unwrap_or(t);
    for ext in [".exe", ".cmd", ".bat"] {
        if let Some(b) = t.strip_suffix(ext) {
            return b.to_string();
        }
    }
    t.to_string()
}

/// Retire les redirections inoffensives (`2>&1`, `>$null`, `>nul`, `>/dev/null`).
fn strip_harmless(low: &str) -> String {
    let mut s = low.to_string();
    for pat in [
        "2>&1",
        "1>&2",
        "2>$null",
        "1>$null",
        ">$null",
        "2>nul",
        ">nul",
        "2>/dev/null",
        ">/dev/null",
    ] {
        s = s.replace(&format!("{pat} "), " ");
        if s.ends_with(pat) {
            let n = s.len() - pat.len();
            s.truncate(n);
        }
    }
    s
}

fn segments(s: &str) -> impl Iterator<Item = &str> {
    s.split(|c: char| matches!(c, ';' | '|' | '&' | '\n' | '\r'))
        .map(str::trim)
}

fn dangerous(low: &str) -> bool {
    let s = strip_harmless(low);
    for seg in segments(&s) {
        if dangerous_segment(seg) {
            return true;
        }
    }
    false
}

/// Sous-commande git, en sautant les options globales (`-c clé=valeur`, `--no-pager`…).
fn git_sub(rest: &[String]) -> &str {
    let mut skip = false;
    for r in rest {
        if skip {
            skip = false;
            continue;
        }
        if r == "-c" {
            skip = true;
            continue;
        }
        if r.starts_with('-') {
            continue;
        }
        return r.as_str();
    }
    ""
}

fn broad_target(t: &str) -> bool {
    let n = unquote(t).replace('\\', "/");
    if matches!(
        n.as_str(),
        "." | ".." | "*" | "./" | "../" | "/" | "~" | "./*" | "../*" | "*.*" | ".git" | "./.git"
    ) {
        return true;
    }
    n.starts_with('~') || n.ends_with("/.git") || n.contains("/.git/")
}

fn dangerous_segment(seg: &str) -> bool {
    let toks: Vec<&str> = seg.split_whitespace().collect();
    let head = match toks.first() {
        Some(t) => base(t),
        None => return false,
    };
    if DANGEROUS_HEADS.iter().any(|d| *d == head) {
        return true;
    }
    let rest: Vec<String> = toks[1..].iter().map(|t| unquote(t).to_string()).collect();
    let has = |w: &str| rest.iter().any(|r| r == w);
    let sub: &str = rest
        .iter()
        .find(|r| !r.starts_with('-'))
        .map(String::as_str)
        .unwrap_or("");
    match head.as_str() {
        "git" => {
            let g = git_sub(&rest);
            match g {
                "push" | "clean" | "filter-branch" => true,
                "reset" => has("--hard"),
                "branch" => has("-d") || has("--delete"),
                "checkout" | "restore" => has("."),
                "stash" => has("drop") || has("clear"),
                _ => false,
            }
        }
        "npm" | "pnpm" | "yarn" | "bun" | "cargo" => {
            if sub == "publish" || sub == "global" {
                return true;
            }
            if head == "cargo" && sub == "install" {
                return true;
            }
            let global = has("-g") || has("--global");
            global
                && matches!(
                    sub,
                    "install" | "i" | "add" | "uninstall" | "remove" | "rm" | "un"
                )
        }
        "pip" | "pip3" => matches!(sub, "install" | "uninstall"),
        "python" | "python3" | "py" => has("pip") && (has("install") || has("uninstall")),
        "rm" | "del" | "erase" | "rmdir" | "rd" | "remove-item" | "ri" => {
            for r in &rest {
                // Options : `-rf`, `-recurse`, `/s`, `/q`…
                if r.starts_with('-') || (r.len() == 2 && r.starts_with('/')) {
                    continue;
                }
                if broad_target(r) {
                    return true;
                }
            }
            false
        }
        _ => false,
    }
}

fn norm(p: &str) -> String {
    let l = p.to_lowercase().replace('/', "\\");
    let l = l.strip_prefix("\\\\?\\").map(str::to_string).unwrap_or(l);
    l.trim_end_matches('\\').to_string()
}

fn token_escapes(tok: &str, root: Option<&str>) -> bool {
    let n = tok.replace('\\', "/");
    if n == ".." || n.starts_with("../") || n.contains("/../") || n.ends_with("/..") {
        return true;
    }
    if tok.starts_with('~') {
        return true;
    }
    if ENV_VARS.iter().any(|v| tok.contains(v)) {
        return true;
    }
    let b = tok.as_bytes();
    let drive = b.len() >= 2
        && b[0].is_ascii_alphabetic()
        && b[1] == b':'
        && (b.len() == 2 || b[2] == b'\\' || b[2] == b'/');
    let unc = tok.starts_with("\\\\") || tok.starts_with("//");
    if drive || unc {
        return match root {
            Some(r) => {
                let t = norm(tok);
                !(t == r || t.starts_with(&format!("{r}\\")))
            }
            None => true,
        };
    }
    n == "/" || (n.starts_with('/') && n[1..].contains('/'))
}

/// La commande vise-t-elle un chemin hors du dossier de travail ?
fn escapes(low: &str, root: Option<&Path>) -> bool {
    let s = strip_harmless(low);
    let root_norm = root.map(|r| norm(&r.to_string_lossy()));
    let seps = |c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '"' | '\'' | '`' | '(' | ')' | ',' | '=' | '{' | '}' | ';' | '|' | '&' | '<' | '>'
            )
    };
    for tok in s.split(seps) {
        if tok.is_empty() {
            continue;
        }
        if token_escapes(tok, root_norm.as_deref()) {
            return true;
        }
    }
    false
}

fn safe_segment(seg: &str) -> bool {
    let toks: Vec<String> = seg
        .split_whitespace()
        .map(|t| unquote(t).to_string())
        .collect();
    let head = match toks.first() {
        Some(t) => base(t),
        None => return true,
    };
    let h = head.as_str();
    if SAFE_HEADS.iter().any(|s| *s == h) || SAFE_TOOLS_2.iter().any(|s| *s == h) {
        return true;
    }
    let nf: Vec<&str> = toks
        .iter()
        .skip(1)
        .filter(|t| !t.starts_with('-'))
        .map(String::as_str)
        .collect();
    let sub = nf.first().copied().unwrap_or("");
    match h {
        "git" => matches!(
            git_sub(&toks[1..]),
            "status"
                | "diff"
                | "log"
                | "show"
                | "rev-parse"
                | "ls-files"
                | "blame"
                | "add"
                | "commit"
                | "describe"
                | "shortlog"
        ),
        "cargo" => matches!(
            sub,
            "check" | "build" | "test" | "clippy" | "fmt" | "doc" | "tree" | "metadata" | "bench"
        ),
        "npm" | "pnpm" | "yarn" | "bun" => {
            if matches!(
                sub,
                "test"
                    | "lint"
                    | "check"
                    | "typecheck"
                    | "format"
                    | "ls"
                    | "list"
                    | "outdated"
                    | "build"
            ) {
                return true;
            }
            if sub == "run" || sub == "run-script" {
                let target = nf.get(1).copied().unwrap_or("");
                return matches!(
                    target,
                    "build" | "lint" | "test" | "check" | "typecheck" | "format"
                );
            }
            false
        }
        "go" => matches!(sub, "build" | "test" | "vet" | "fmt" | "list"),
        "dotnet" => matches!(sub, "build" | "test" | "format"),
        "node" | "python" | "python3" | "py" | "rustc" | "java" => {
            toks.len() == 2
                && matches!(
                    toks[1].as_str(),
                    "--version" | "-v" | "-version" | "version"
                )
        }
        _ => false,
    }
}

/// Commande clairement inoffensive : lecture, build, tests, git local.
fn known_safe(low: &str) -> bool {
    let s = strip_harmless(low);
    if s.contains('>') || s.contains('<') || s.contains('{') || s.contains("$(") || s.contains('`')
    {
        return false;
    }
    for seg in segments(&s) {
        if !safe_segment(seg) {
            return false;
        }
    }
    true
}

// ---------- commandes Tauri ----------

#[tauri::command]
pub(crate) fn set_perm_mode(state: State<'_, AppState>, mode: String) -> Result<String, String> {
    let m = Mode::parse(&mode).ok_or_else(|| format!("Mode inconnu : {mode}"))?;
    let mut g = state
        .perm
        .lock()
        .map_err(|_| "état verrouillé".to_string())?;
    *g = m;
    Ok(m.id().to_string())
}

#[tauri::command]
pub(crate) fn get_perm_mode(state: State<'_, AppState>) -> String {
    current(&state).id().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(mode: Mode, cmd: &str) -> Verdict {
        let root = Path::new("C:\\proj");
        decide(
            mode,
            false,
            "run_command",
            &json!({ "command": cmd }),
            Some(root),
        )
    }

    #[test]
    fn project_mode() {
        assert!(matches!(run(Mode::Project, "cargo build"), Verdict::Allow));
        assert!(matches!(
            run(Mode::Project, "git push origin main"),
            Verdict::Ask
        ));
        assert!(matches!(
            run(Mode::Project, "type ..\\secret.txt"),
            Verdict::Ask
        ));
        assert!(matches!(run(Mode::Project, "rm -rf ."), Verdict::Ask));
    }

    #[test]
    fn full_and_plan() {
        assert!(matches!(run(Mode::Full, "shutdown /s"), Verdict::Allow));
        let root = Path::new("C:\\proj");
        let v = decide(
            Mode::Plan,
            false,
            "edit_file",
            &json!({ "path": "a.txt" }),
            Some(root),
        );
        assert!(matches!(v, Verdict::Deny(_)));
        assert!(matches!(run(Mode::Plan, "cargo build"), Verdict::Deny(_)));
    }

    #[test]
    fn auto_mode() {
        assert!(matches!(
            run(Mode::Auto, "cargo check 2>&1"),
            Verdict::Allow
        ));
        assert!(matches!(run(Mode::Auto, "npm install"), Verdict::Ask));
        assert!(matches!(run(Mode::Auto, "echo hi > a.txt"), Verdict::Ask));
    }

}
