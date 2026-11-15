//! Journal des modifications faites par l'IA : écriture de fichiers, commandes, appels MCP.
//! Une ligne JSON par action dans `changes.jsonl` (dossier de config de l'app).

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use fs_tools::Risk;
use serde_json::{json, Value};

use crate::mcp_ctx;

const FILE: &str = "changes.jsonl";
const OLD: &str = "changes.old.jsonl";
/// Au-delà, le journal est archivé (un seul fichier d'archive, écrasé).
const MAX_BYTES: u64 = 1_000_000;

fn short(s: &str, n: usize) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= n {
        one
    } else {
        let cut: String = one.chars().take(n).collect();
        format!("{cut}…")
    }
}

/// Libellé court d'une action qui modifie quelque chose, `None` pour une simple lecture.
pub fn label(name: &str, args: &Value, mcp_cmd: Option<&str>) -> Option<String> {
    if let Some(cmd) = mcp_cmd {
        let rest = mcp_ctx::mcp_rest(cmd).unwrap_or("");
        let mut it = rest.split_whitespace();
        if !it.next()?.eq_ignore_ascii_case("call") {
            return None;
        }
        let srv = it.next().unwrap_or("?");
        let tool = it.next().unwrap_or("?");
        return Some(format!("MCP {srv} · {tool}"));
    }
    if let Some(rest) = name.strip_prefix("mcp__") {
        let (srv, tool) = rest.split_once("__").unwrap_or((rest, "?"));
        return Some(format!("MCP {srv} · {tool}"));
    }
    match fs_tools::risk(name)? {
        Risk::Safe => None,
        Risk::Edit => {
            let path = args["path"].as_str().unwrap_or("?");
            Some(format!("{name} {path}"))
        }
        Risk::Command => {
            let cmd = args["command"]
                .as_str()
                .or_else(|| args["cmd"].as_str())
                .unwrap_or("?");
            Some(format!("commande : {}", short(cmd, 80)))
        }
    }
}

/// Ajoute une entrée au journal. Les erreurs d'écriture sont ignorées : le journal ne doit
/// jamais faire échouer un outil.
pub fn record(dir: &Path, tool: &str, label: &str, status: &str, root: Option<&Path>) {
    let path = dir.join(FILE);
    if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > MAX_BYTES {
        let _ = std::fs::rename(&path, dir.join(OLD));
    }
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let entry = json!({
        "ts": ts,
        "tool": tool,
        "label": label,
        "status": status,
        "root": root.map(|r| r.display().to_string()),
    });
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{entry}");
    }
}

/// Les `n` dernières entrées, de la plus ancienne à la plus récente.
pub fn read_tail(dir: &Path, n: usize) -> Vec<Value> {
    let text = std::fs::read_to_string(dir.join(FILE)).unwrap_or_default();
    let mut out: Vec<Value> = text
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect();
    if out.len() > n {
        out.drain(..out.len() - n);
    }
    out
}
