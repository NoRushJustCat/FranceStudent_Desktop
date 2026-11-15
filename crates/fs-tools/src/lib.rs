//! Outils locaux du mode Code : list_directory, read_file, search_files,
//! edit_file, write_file, run_command.
//!
//! Catalogue et arguments = ceux du serveur (`client-tools.md`) :
//! - `list_directory` : `path`, `depth` (1 à 3)
//! - `read_file` : `path`, `start_line`, `end_line`
//! - `search_files` : `query` (texte ou regex), `path`, `glob`, `max_results`
//! - `edit_file` : `path`, `old_text`, `new_text`, `replace_all`
//! - `write_file` : `path`, `content`
//! - `run_command` : `command`, `cwd`, `timeout_seconds`
//!
//! Tout est confiné au dossier de travail (`root`) : un chemin qui en sort
//! (`..`, chemin absolu ailleurs, lien symbolique) est refusé, sauf via
//! `execute_full` (mode « système »).

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use regex::{Regex, RegexBuilder};
use serde_json::Value;

/// Taille maximale d'une sortie renvoyée au modèle (en caractères).
pub const MAX_OUTPUT: usize = 40_000;
const MAX_FILE_BYTES: u64 = 2_000_000;
const MAX_HITS: usize = 200;
const MAX_HITS_CAP: usize = 500;
const MAX_ENTRIES: usize = 500;
const DEFAULT_TIMEOUT_SECS: u64 = 60;
const MAX_TIMEOUT_SECS: u64 = 300;
const SKIP_DIRS: [&str; 4] = [".git", "node_modules", "target", "dist"];

tokio::task_local! {
    /// Vrai pendant `execute_full` : lève le confinement au dossier de travail.
    static FULL: bool;
}

fn unrestricted() -> bool {
    FULL.try_with(|v| *v).unwrap_or(false)
}

/// Noms d'outils déclarés au serveur (`clientTools`). Le serveur n'accepte
/// que ce catalogue fixe : un autre nom donne une erreur 400.
pub const NAMES: [&str; 6] = [
    "list_directory",
    "read_file",
    "search_files",
    "edit_file",
    "write_file",
    "run_command",
];

/// Niveau de risque d'un outil, pour décider de la confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    /// Lecture seule : jamais de confirmation.
    Safe,
    /// Modifie des fichiers : confirmation, sauf « accepter auto ».
    Edit,
    /// Exécute une commande : toujours une confirmation.
    Command,
}

pub fn risk(name: &str) -> Option<Risk> {
    match name {
        "list_directory" | "read_file" | "search_files" => Some(Risk::Safe),
        "edit_file"
        | "write_file"
        | "create_file"
        | "createFile"
        | "new_file"
        | "write_to_file" => Some(Risk::Edit),
        "run_command" => Some(Risk::Command),
        _ => None,
    }
}

/// Les arguments arrivent en objet JSON ou en chaîne JSON selon le fournisseur.
pub fn parse_args(raw: &Value) -> Value {
    match raw {
        Value::String(s) => serde_json::from_str(s).unwrap_or(Value::Null),
        other => other.clone(),
    }
}

/// Exécute un outil dans `root`. `Err` = message d'erreur à renvoyer au modèle.
pub async fn execute(root: &Path, name: &str, raw_args: &Value) -> Result<String, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("dossier de travail invalide : {e}"))?;
    let args = parse_args(raw_args);
    let out = match name {
        "list_directory" => list_directory(&root, &args)?,
        "read_file" => read_file(&root, &args)?,
        "search_files" => search_files(&root, &args)?,
        "edit_file" => edit_file(&root, &args)?,
        "write_file"
        | "create_file"
        | "createFile"
        | "new_file"
        | "write_to_file" => write_file(&root, &args)?,
        "run_command" => run_command(&root, &args).await?,
        other => return Err(format!("outil inconnu : {other}")),
    };
    Ok(clip(out))
}

/// Comme `execute`, mais sans confinement au dossier de travail : chemins
/// absolus et `..` acceptés (mode « système »). `root` reste le dossier
/// de base des chemins relatifs et des commandes.
pub async fn execute_full(root: &Path, name: &str, raw_args: &Value) -> Result<String, String> {
    FULL.scope(true, execute(root, name, raw_args)).await
}

// ---------- utilitaires ----------

#[allow(dead_code)]
fn str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args[key]
        .as_str()
        .ok_or_else(|| format!("argument manquant ou invalide : {key}"))
}

/// Première clé présente parmi `keys` (le premier nom est le nom officiel).
pub fn any_str<'a>(args: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|k| args[*k].as_str())
}

/// Extrait le chemin depuis divers noms de propriétés possibles (`path`, `file_path`, etc.).
fn get_path_arg<'a>(args: &'a Value) -> Result<&'a str, String> {
    any_str(
        args,
        &[
            "path",
            "file_path",
            "filePath",
            "filepath",
            "filename",
            "file",
            "name",
        ],
    )
    .ok_or_else(|| "argument manquant ou invalide : path".to_string())
}

/// Extrait le contenu depuis divers noms possibles (`content`, `contents`, `text`, etc.).
fn get_content_arg(args: &Value) -> Result<String, String> {
    if let Some(s) = any_str(args, &["content", "contents", "text", "body", "code", "data"]) {
        return Ok(s.to_string());
    }
    for key in &["content", "contents", "text", "body", "code", "data"] {
        if let Some(v) = args.get(key) {
            if !v.is_null() {
                return Ok(if let Value::String(s) = v {
                    s.clone()
                } else {
                    v.to_string()
                });
            }
        }
    }
    Err("argument manquant ou invalide : content".to_string())
}

fn clip(s: String) -> String {
    if s.chars().count() <= MAX_OUTPUT {
        return s;
    }
    let mut cut: String = s.chars().take(MAX_OUTPUT).collect();
    cut.push_str(&format!("\n[... tronqué à {MAX_OUTPUT} caractères]"));
    cut
}

/// Nettoie les préfixes de chemin (`./`, `.\`, `/`, `\`) qui risquent de faire sortir
/// la concaténation de la racine sur Windows.
fn clean_rel_path(rel: &str) -> &str {
    let mut s = rel.trim();
    loop {
        if let Some(stripped) = s.strip_prefix("./") {
            s = stripped.trim_start();
        } else if let Some(stripped) = s.strip_prefix(".\\") {
            s = stripped.trim_start();
        } else if let Some(stripped) = s.strip_prefix('/') {
            s = stripped.trim_start();
        } else if let Some(stripped) = s.strip_prefix('\\') {
            s = stripped.trim_start();
        } else {
            break;
        }
    }
    if s.is_empty() {
        "."
    } else {
        s
    }
}

/// Vérifie que `full` commence par `root` (insensible à la casse sur Windows,
/// et gère le préfixe verbatim `\\?\`).
fn path_starts_with(full: &Path, root: &Path) -> bool {
    #[cfg(windows)]
    {
        let f_str = full.to_string_lossy().to_lowercase().replace('/', "\\");
        let r_str = root.to_string_lossy().to_lowercase().replace('/', "\\");
        let f_clean = f_str.strip_prefix(r"\\?\").unwrap_or(&f_str);
        let r_clean = r_str.strip_prefix(r"\\?\").unwrap_or(&r_str);
        f_clean.starts_with(r_clean)
    }
    #[cfg(not(windows))]
    {
        full.starts_with(root)
    }
}

/// `root` doit déjà être canonique. Renvoie un chemin absolu garanti dans `root`.
fn resolve(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let p = Path::new(rel.trim());
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        let clean = clean_rel_path(rel);
        root.join(clean)
    };

    // Normalisation lexicale (`.` et `..`).
    let mut out = PathBuf::new();
    for c in joined.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            Component::Prefix(pr) => {
                out.push(pr.as_os_str());
            }
            Component::RootDir => {
                out.push(Component::RootDir.as_os_str());
            }
            Component::Normal(n) => {
                out.push(n);
            }
        }
    }

    // Canonicalise le plus long préfixe existant (résout liens symboliques
    // et préfixe `\\?\` Windows), puis rajoute la suite.
    let mut existing = out.clone();
    let mut rest = Vec::new();
    while !existing.exists() {
        match existing.file_name() {
            Some(n) => {
                rest.push(n.to_os_string());
                existing.pop();
            }
            None => break,
        }
    }
    let mut full = match existing.canonicalize() {
        Ok(c) => c,
        Err(_) => existing,
    };
    for n in rest.into_iter().rev() {
        full.push(n);
    }

    if !unrestricted() && !path_starts_with(&full, root) {
        return Err(format!("chemin hors du dossier de travail : {rel}"));
    }
    Ok(full)
}

/// Comme `resolve`, mais refuse aussi toute écriture dans `.git`.
fn resolve_writable(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let p = resolve(root, rel)?;
    let inside = p.strip_prefix(root).unwrap_or(&p);
    if inside.components().any(|c| c.as_os_str() == ".git") {
        return Err("écriture interdite dans .git".to_string());
    }
    Ok(p)
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

/// `\\?\C:\x` -> `C:\x` (PowerShell supporte mal le préfixe verbatim comme cwd).
fn plain(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(r) if !r.starts_with("UNC\\") => PathBuf::from(r),
        _ => p.to_path_buf(),
    }
}

// ---------- outils ----------

/// Liste récursive jusqu'à `depth` niveaux ; deux espaces d'indentation par niveau.
/// Les liens symboliques ne sont jamais suivis.
fn walk(dir: &Path, level: usize, depth: usize, out: &mut Vec<String>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut kids: Vec<(String, bool)> = rd
        .flatten()
        .map(|e| {
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            (e.file_name().to_string_lossy().into_owned(), is_dir)
        })
        .collect();
    kids.sort();
    for (name, is_dir) in kids {
        if out.len() > MAX_ENTRIES {
            return;
        }
        let pad = "  ".repeat(level);
        if is_dir {
            out.push(format!("{pad}{name}/"));
            if level + 1 < depth && !SKIP_DIRS.contains(&name.as_str()) {
                walk(&dir.join(&name), level + 1, depth, out);
            }
        } else {
            out.push(format!("{pad}{name}"));
        }
    }
}

fn list_directory(root: &Path, args: &Value) -> Result<String, String> {
    let dir = resolve(root, args["path"].as_str().unwrap_or("."))?;
    fs::read_dir(&dir).map_err(|e| format!("{} : {e}", rel(root, &dir)))?;
    let depth = args["depth"].as_u64().unwrap_or(1).clamp(1, 3) as usize;

    let mut lines: Vec<String> = Vec::new();
    walk(&dir, 0, depth, &mut lines);
    if lines.is_empty() {
        return Ok("(dossier vide)".to_string());
    }
    let total = lines.len();
    lines.truncate(MAX_ENTRIES);
    let mut out = lines.join("\n");
    if total > MAX_ENTRIES {
        out.push_str(&format!(
            "\n[... plus de {MAX_ENTRIES} entrées, liste coupée]"
        ));
    }
    Ok(out)
}

fn read_file(root: &Path, args: &Value) -> Result<String, String> {
    let path_str = get_path_arg(args)?;
    let path = resolve(root, path_str)?;
    let meta = fs::metadata(&path).map_err(|e| format!("{} : {e}", rel(root, &path)))?;
    if !meta.is_file() {
        return Err(format!("{} n'est pas un fichier", rel(root, &path)));
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(format!(
            "fichier trop gros ({} octets) : utilise start_line/end_line ou search_files",
            meta.len()
        ));
    }
    let bytes = fs::read(&path).map_err(|e| e.to_string())?;
    let text = if let Ok(s) = String::from_utf8(bytes.clone()) {
        s.trim_start_matches('\u{feff}').to_string()
    } else if bytes.len() >= 2
        && ((bytes[0] == 0xff && bytes[1] == 0xfe) || (bytes[0] == 0xfe && bytes[1] == 0xff))
    {
        let is_le = bytes[0] == 0xff && bytes[1] == 0xfe;
        let u16s: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| {
                if is_le {
                    u16::from_le_bytes([c[0], c[1]])
                } else {
                    u16::from_be_bytes([c[0], c[1]])
                }
            })
            .collect();
        String::from_utf16_lossy(&u16s)
    } else {
        String::from_utf8_lossy(&bytes).to_string()
    };

    let start = args["start_line"].as_u64().unwrap_or(1).max(1) as usize;
    let end = args["end_line"]
        .as_u64()
        .map(|n| n as usize)
        .unwrap_or(usize::MAX);
    let count = end.saturating_sub(start - 1);

    let mut out = String::new();
    for (i, line) in text.lines().enumerate().skip(start - 1).take(count) {
        out.push_str(&format!("{:>5}\t{line}\n", i + 1));
    }
    if out.is_empty() {
        out = "(rien dans cette plage / fichier vide)".to_string();
    }
    Ok(out)
}

/// `*.rs`, `src/**/*.ts`, `?` : glob minimal converti en regex ancrée.
fn glob_regex(g: &str) -> Result<Regex, String> {
    let chars: Vec<char> = g.chars().collect();
    let mut re = String::from("^");
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' if chars.get(i + 1) == Some(&'*') => {
                if chars.get(i + 2) == Some(&'/') {
                    re.push_str("(?:.*/)?");
                    i += 3;
                } else {
                    re.push_str(".*");
                    i += 2;
                }
            }
            '*' => {
                re.push_str("[^/]*");
                i += 1;
            }
            '?' => {
                re.push_str("[^/]");
                i += 1;
            }
            c => {
                re.push_str(&regex::escape(&c.to_string()));
                i += 1;
            }
        }
    }
    re.push('$');
    Regex::new(&re).map_err(|e| format!("glob invalide : {e}"))
}

/// Recherche texte ou regex (insensible à la casse). Si `query` n'est pas une
/// regex valide, elle est prise comme texte littéral.
fn search_files(root: &Path, args: &Value) -> Result<String, String> {
    let query = any_str(args, &["query", "pattern"])
        .ok_or_else(|| "argument manquant ou invalide : query".to_string())?;
    if query.is_empty() {
        return Err("query vide".to_string());
    }
    let re = match RegexBuilder::new(query).case_insensitive(true).build() {
        Ok(r) => r,
        Err(_) => RegexBuilder::new(&regex::escape(query))
            .case_insensitive(true)
            .build()
            .map_err(|e| e.to_string())?,
    };
    let glob_src = args["glob"].as_str().filter(|g| !g.trim().is_empty());
    let glob = glob_src.map(glob_regex).transpose()?;
    let glob_has_slash = glob_src.map(|g| g.contains('/')).unwrap_or(false);
    let max = args["max_results"]
        .as_u64()
        .map(|n| (n as usize).clamp(1, MAX_HITS_CAP))
        .unwrap_or(MAX_HITS);

    let path_target = args["path"].as_str().unwrap_or(".");
    let base = resolve(root, path_target)?;
    let mut hits: Vec<String> = Vec::new();
    let mut stack = vec![base];
    'outer: while let Some(p) = stack.pop() {
        let Ok(md) = fs::symlink_metadata(&p) else {
            continue;
        };
        if md.file_type().is_symlink() {
            continue;
        }
        if md.is_dir() {
            let Ok(rd) = fs::read_dir(&p) else { continue };
            let mut kids: Vec<PathBuf> = rd
                .flatten()
                .filter(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                    !(is_dir && SKIP_DIRS.contains(&name.as_str()))
                })
                .map(|e| e.path())
                .collect();
            kids.sort();
            kids.reverse(); // le pop() donne alors l'ordre alphabétique
            stack.extend(kids);
            continue;
        }
        if !md.is_file() || md.len() > MAX_FILE_BYTES {
            continue;
        }
        if let Some(g) = &glob {
            let relative = rel(root, &p);
            let target = if glob_has_slash {
                relative.as_str()
            } else {
                p.file_name().and_then(|n| n.to_str()).unwrap_or("")
            };
            if !g.is_match(target) {
                continue;
            }
        }
        let Ok(text) = fs::read_to_string(&p) else {
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            if re.is_match(line) {
                let shown: String = line.trim().chars().take(200).collect();
                hits.push(format!("{}:{}: {shown}", rel(root, &p), i + 1));
                if hits.len() >= max {
                    break 'outer;
                }
            }
        }
    }
    if hits.is_empty() {
        return Ok("aucun résultat".to_string());
    }
    let mut out = hits.join("\n");
    if hits.len() >= max {
        out.push_str(&format!("\n[... limité à {max} résultats]"));
    }
    Ok(out)
}

fn edit_file(root: &Path, args: &Value) -> Result<String, String> {
    let path_str = get_path_arg(args)?;
    let path = resolve_writable(root, path_str)?;
    let old = any_str(args, &["old_text", "old_string"]).unwrap_or("");
    let new = any_str(args, &["new_text", "new_string", "content", "text"])
        .ok_or_else(|| "argument manquant ou invalide : new_text".to_string())?;

    // Si le fichier n'existe pas ou si old_text est vide, on crée ou remplace directement le fichier
    if !path.exists() || old.is_empty() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("création du dossier parent : {e}"))?;
        }
        fs::write(&path, new.as_bytes()).map_err(|e| e.to_string())?;
        return Ok(format!(
            "{} écrit ({} octets)",
            rel(root, &path),
            new.len()
        ));
    }

    let replace_all = args["replace_all"].as_bool().unwrap_or(false);

    let text = fs::read_to_string(&path).map_err(|e| format!("{} : {e}", rel(root, &path)))?;
    // Fichiers CRLF : le modèle envoie des \n, on édite en \n puis on restaure.
    let crlf = text.contains("\r\n");
    let text = if crlf {
        text.replace("\r\n", "\n")
    } else {
        text
    };
    let old = old.replace("\r\n", "\n");
    let new = new.replace("\r\n", "\n");

    let n = text.matches(old.as_str()).count();
    if n == 0 {
        return Err("old_text introuvable (copie le texte exact, espaces compris)".to_string());
    }
    if n > 1 && !replace_all {
        return Err(format!(
            "old_text apparaît {n} fois : ajoute du contexte pour le rendre unique, ou replace_all=true"
        ));
    }
    let updated = if replace_all {
        text.replace(old.as_str(), &new)
    } else {
        text.replacen(old.as_str(), &new, 1)
    };
    let updated = if crlf {
        updated.replace('\n', "\r\n")
    } else {
        updated
    };
    fs::write(&path, updated).map_err(|e| e.to_string())?;
    Ok(format!(
        "{} modifié ({} remplacement(s))",
        rel(root, &path),
        if replace_all { n } else { 1 }
    ))
}

fn write_file(root: &Path, args: &Value) -> Result<String, String> {
    let path_str = get_path_arg(args)?;
    let path = resolve_writable(root, path_str)?;
    let content = get_content_arg(args)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("création du dossier parent : {e}"))?;
    }
    fs::write(&path, content.as_bytes()).map_err(|e| e.to_string())?;
    Ok(format!(
        "{} écrit ({} octets)",
        rel(root, &path),
        content.len()
    ))
}

async fn run_command(root: &Path, args: &Value) -> Result<String, String> {
    let command = any_str(args, &["command", "cmd"])
        .ok_or_else(|| "argument manquant ou invalide : command".to_string())?;

    // `cwd` optionnel : relatif au dossier de travail, jamais en dehors.
    let cwd = match args["cwd"].as_str().filter(|c| !c.trim().is_empty()) {
        Some(c) => {
            let p = resolve(root, c)?;
            if !p.is_dir() {
                return Err(format!("cwd n'est pas un dossier : {c}"));
            }
            p
        }
        None => root.to_path_buf(),
    };
    let secs = args["timeout_seconds"]
        .as_u64()
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
        .clamp(1, MAX_TIMEOUT_SECS);

    #[cfg(windows)]
    let mut cmd = {
        let script = format!("[Console]::OutputEncoding=[System.Text.Encoding]::UTF8; $OutputEncoding=[System.Text.Encoding]::UTF8; $PSDefaultParameterValues['Out-File:Encoding']='utf8'; {command}");
        let mut c = tokio::process::Command::new("powershell");
        c.args(["-NoProfile", "-NonInteractive", "-Command", script.as_str()]);
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = tokio::process::Command::new("sh");
        c.args(["-c", command]);
        c
    };

    cmd.current_dir(plain(&cwd))
        .stdin(Stdio::null())
        .kill_on_drop(true);

    let out = tokio::time::timeout(Duration::from_secs(secs), cmd.output())
        .await
        .map_err(|_| format!("délai dépassé ({secs} s) : commande arrêtée"))?
        .map_err(|e| format!("lancement impossible : {e}"))?;

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let code = out
        .status
        .code()
        .map(|c| c.to_string())
        .unwrap_or_else(|| "?".to_string());
    let mut text = format!("code de sortie : {code}\n");
    if !stdout.trim().is_empty() {
        text.push_str(&stdout);
    }
    if !stderr.trim().is_empty() {
        text.push_str("\n[stderr]\n");
        text.push_str(&stderr);
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fs-tools-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    #[test]
    fn refuse_de_sortir_du_dossier() {
        let d = tmp("sortie");
        assert!(resolve(&d, "../x").is_err());
        assert!(resolve(&d, "a/../../x").is_err());
        assert!(resolve(&d, "a/b.txt").is_ok());
    }

    #[test]
    fn git_est_protege() {
        let d = tmp("git");
        assert!(resolve_writable(&d, ".git/config").is_err());
        assert!(resolve_writable(&d, "src/x.rs").is_ok());
    }

    #[tokio::test]
    async fn edition_crlf_et_ambiguite() {
        let d = tmp("edit");
        execute(
            &d,
            "write_file",
            &json!({"path": "a.txt", "content": "un\r\ndeux\r\n"}),
        )
        .await
        .unwrap();
        execute(
            &d,
            "edit_file",
            &json!({"path": "a.txt", "old_text": "un\ndeux", "new_text": "1\n2"}),
        )
        .await
        .unwrap();
        assert_eq!(fs::read_to_string(d.join("a.txt")).unwrap(), "1\r\n2\r\n");

        execute(
            &d,
            "write_file",
            &json!({"path": "b.txt", "content": "x x"}),
        )
        .await
        .unwrap();
        let e = execute(
            &d,
            "edit_file",
            &json!({"path": "b.txt", "old_text": "x", "new_text": "y"}),
        )
        .await;
        assert!(e.is_err());
    }

    #[tokio::test]
    async fn lecture_et_recherche() {
        let d = tmp("read");
        execute(
            &d,
            "write_file",
            &json!({"path": "src/m.rs", "content": "fn main() {}\nlet Truc = 1;\n"}),
        )
        .await
        .unwrap();
        let r = execute(
            &d,
            "read_file",
            &json!({"path": "src/m.rs", "start_line": 2}),
        )
        .await
        .unwrap();
        assert!(r.contains("Truc"));
        let s = execute(&d, "search_files", &json!({"query": "truc"}))
            .await
            .unwrap();
        assert!(s.contains("src/m.rs:2"));
    }

    #[tokio::test]
    async fn recherche_regex_et_glob() {
        let d = tmp("regex");
        execute(
            &d,
            "write_file",
            &json!({"path": "src/a.rs", "content": "fn alpha() {}\n"}),
        )
        .await
        .unwrap();
        execute(
            &d,
            "write_file",
            &json!({"path": "src/b.txt", "content": "fn beta() {}\n"}),
        )
        .await
        .unwrap();
        let r = execute(
            &d,
            "search_files",
            &json!({"query": "fn \\w+\\(", "glob": "*.rs"}),
        )
        .await
        .unwrap();
        assert!(r.contains("src/a.rs:1"));
        assert!(!r.contains("b.txt"));
    }

    #[tokio::test]
    async fn liste_avec_profondeur() {
        let d = tmp("depth");
        execute(
            &d,
            "write_file",
            &json!({"path": "src/m.rs", "content": "x"}),
        )
        .await
        .unwrap();
        let un = execute(&d, "list_directory", &json!({"path": "."}))
            .await
            .unwrap();
        assert!(un.contains("src/"));
        assert!(!un.contains("m.rs"));
        let deux = execute(&d, "list_directory", &json!({"path": ".", "depth": 2}))
            .await
            .unwrap();
        assert!(deux.contains("  m.rs"));
    }

    #[tokio::test]
    async fn creation_avec_slash_et_alias() {
        let d = tmp("creation");
        // Chemin relatif avec slash initial
        execute(
            &d,
            "write_file",
            &json!({"file_path": "/sub/dir/test.txt", "content": "bonjour"}),
        )
        .await
        .unwrap();
        assert_eq!(
            fs::read_to_string(d.join("sub").join("dir").join("test.txt")).unwrap(),
            "bonjour"
        );

        // Alias create_file
        execute(
            &d,
            "create_file",
            &json!({"path": "./autre.txt", "text": "contenu autre"}),
        )
        .await
        .unwrap();
        assert_eq!(fs::read_to_string(d.join("autre.txt")).unwrap(), "contenu autre");

        // edit_file sur fichier inexistant (création)
        execute(
            &d,
            "edit_file",
            &json!({"path": "inexistant.txt", "old_text": "", "new_text": "nouveau"}),
        )
        .await
        .unwrap();
        assert_eq!(fs::read_to_string(d.join("inexistant.txt")).unwrap(), "nouveau");
    }
}
