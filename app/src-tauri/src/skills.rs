//! Skills locaux : un dossier par skill, avec un fichier `SKILL.md` dedans.
//!
//! Dossiers lus, par ordre de priorité (le premier nom trouvé gagne) :
//! 1. `<projet>/.fscode/skills`
//! 2. `<projet>/.claude/skills`
//! 3. `~/.claude/skills`
//! 4. `<config de l'app>/skills`

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    /// `projet`, `utilisateur` ou `app`.
    pub source: String,
    /// Dossier du skill.
    pub path: String,
}

fn roots(config: &Path, workdir: Option<&Path>) -> Vec<(&'static str, PathBuf)> {
    let mut v = Vec::new();
    if let Some(w) = workdir {
        v.push(("projet", w.join(".fscode").join("skills")));
        v.push(("projet", w.join(".claude").join("skills")));
    }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"));
    if let Some(h) = home {
        v.push((
            "utilisateur",
            PathBuf::from(h).join(".claude").join("skills"),
        ));
    }
    v.push(("app", config.join("skills")));
    v
}

fn unquote(s: &str) -> String {
    s.trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .to_string()
}

/// Lit `name` et `description` dans l'en-tête `---` d'un SKILL.md.
/// Renvoie aussi le corps (sans l'en-tête).
fn parse(text: &str) -> (Option<String>, String, String) {
    let text = text.trim_start_matches('\u{feff}');
    let lines: Vec<&str> = text.lines().collect();
    if lines.first().map(|l| l.trim()) != Some("---") {
        return (None, String::new(), text.to_string());
    }
    let Some(end) = lines.iter().skip(1).position(|l| l.trim() == "---") else {
        return (None, String::new(), text.to_string());
    };
    let end = end + 1; // index réel de la ligne `---` fermante
    let mut name = None;
    let mut desc = String::new();
    let mut i = 1;
    while i < end {
        let line = lines[i];
        if let Some(v) = line.strip_prefix("name:") {
            name = Some(unquote(v)).filter(|n| !n.is_empty());
        } else if let Some(v) = line.strip_prefix("description:") {
            let v = v.trim();
            if v.is_empty() || v.starts_with('>') || v.starts_with('|') {
                // Valeur sur les lignes suivantes (indentées).
                let mut parts = Vec::new();
                while i + 1 < end && lines[i + 1].starts_with([' ', '\t']) {
                    parts.push(lines[i + 1].trim());
                    i += 1;
                }
                desc = parts.join(" ");
            } else {
                desc = unquote(v);
            }
        }
        i += 1;
    }
    let body = lines[end + 1..].join("\n");
    (name, desc, body.trim_start().to_string())
}

pub fn list(config: &Path, workdir: Option<&Path>) -> Vec<SkillInfo> {
    let mut out: Vec<SkillInfo> = Vec::new();
    for (source, root) in roots(config, workdir) {
        let Ok(rd) = fs::read_dir(&root) else {
            continue;
        };
        let mut dirs: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        dirs.sort();
        for dir in dirs {
            let Ok(text) = fs::read_to_string(dir.join("SKILL.md")) else {
                continue;
            };
            let (name, description, _) = parse(&text);
            let name = name.unwrap_or_else(|| {
                dir.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });
            if name.is_empty() || out.iter().any(|s| s.name == name) {
                continue;
            }
            out.push(SkillInfo {
                name,
                description,
                source: source.to_string(),
                path: dir.to_string_lossy().into_owned(),
            });
        }
    }
    out
}