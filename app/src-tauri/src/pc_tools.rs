use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[tauri::command]
pub fn pc_sys_info() -> Result<Value, String> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let cpu_cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);

    let hostname = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| {
            #[cfg(target_os = "windows")]
            {
                run_ps("hostname").unwrap_or_else(|| "unknown".to_string())
            }
            #[cfg(not(target_os = "windows"))]
            {
                Command::new("hostname")
                    .output()
                    .ok()
                    .and_then(|o| {
                        if o.status.success() {
                            Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| "unknown".to_string())
            }
        });

    let (memory_hint, disk_hint) = get_mem_and_disk_hints();

    Ok(json!({
        "os": os,
        "arch": arch,
        "cpu_cores": cpu_cores,
        "hostname": hostname,
        "memory_hint": memory_hint,
        "disk_hint": disk_hint,
    }))
}

#[cfg(target_os = "windows")]
fn run_ps(script: &str) -> Option<String> {
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .ok()?;
    if out.status.success() {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    } else {
        None
    }
}

#[cfg(target_os = "windows")]
fn get_mem_and_disk_hints() -> (String, String) {
    let mem = run_ps(
        "$os = Get-CimInstance Win32_OperatingSystem; $free = [math]::Round($os.FreePhysicalMemory / 1MB, 1); $tot = [math]::Round($os.TotalVisibleMemorySize / 1MB, 1); \"$free GB free / $tot GB\"",
    )
    .unwrap_or_else(|| "unavailable".to_string());

    let disk = run_ps(
        "$d = Get-PSDrive -Name C -ErrorAction SilentlyContinue; if ($d) { $free = [math]::Round($d.Free / 1GB, 1); $used = [math]::Round($d.Used / 1GB, 1); \"$free GB free (C:)\" } else { 'unavailable' }",
    )
    .unwrap_or_else(|| "unavailable".to_string());

    (mem, disk)
}

#[cfg(not(target_os = "windows"))]
fn get_mem_and_disk_hints() -> (String, String) {
    let mem = Command::new("sh")
        .args(["-c", "free -h 2>/dev/null | awk '/Mem:/ {print $7 \" free / \" $2}'"])
        .output()
        .ok()
        .and_then(|o| {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if o.status.success() && !s.is_empty() {
                Some(s)
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unavailable".to_string());

    let disk = Command::new("sh")
        .args(["-c", "df -h . 2>/dev/null | awk 'NR==2 {print $4 \" free / \" $2}'"])
        .output()
        .ok()
        .and_then(|o| {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if o.status.success() && !s.is_empty() {
                Some(s)
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unavailable".to_string());

    (mem, disk)
}

#[tauri::command]
pub fn pc_git_summary(workdir: String) -> Result<Value, String> {
    let root = Path::new(&workdir);
    if !root.exists() || !root.is_dir() {
        return Err(format!("Workdir does not exist: {}", workdir));
    }

    let branch_out = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if !branch_out.status.success() {
        return Ok(json!({
            "is_git": false,
            "branch": "",
            "modified_count": 0,
            "staged_count": 0,
            "untracked_count": 0,
            "files": [],
            "shortstat": "",
            "recent_commits": []
        }));
    }

    let branch = String::from_utf8_lossy(&branch_out.stdout)
        .trim()
        .to_string();

    let status_out = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("Failed to run git status: {}", e))?;

    let status_text = String::from_utf8_lossy(&status_out.stdout);
    let mut modified_count = 0usize;
    let mut staged_count = 0usize;
    let mut untracked_count = 0usize;
    let mut files: Vec<Value> = Vec::new();

    for line in status_text.lines() {
        if line.len() < 3 {
            continue;
        }
        let bytes = line.as_bytes();
        let x = bytes[0] as char;
        let y = bytes[1] as char;
        let path_part = line[3..].trim().to_string();

        if x == '?' && y == '?' {
            untracked_count += 1;
        } else {
            if x != ' ' && x != '?' {
                staged_count += 1;
            }
            if y != ' ' && y != '?' {
                modified_count += 1;
            }
        }

        if files.len() < 25 {
            let status_code = format!("{}{}", x, y).trim().to_string();
            files.push(json!({
                "status": status_code,
                "path": path_part
            }));
        }
    }

    let shortstat = Command::new("git")
        .args(["diff", "--shortstat"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    let recent_commits: Vec<String> = Command::new("git")
        .args(["log", "-n", "5", "--oneline"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default();

    Ok(json!({
        "is_git": true,
        "branch": branch,
        "modified_count": modified_count,
        "staged_count": staged_count,
        "untracked_count": untracked_count,
        "files": files,
        "shortstat": shortstat,
        "recent_commits": recent_commits
    }))
}

#[tauri::command]
pub fn pc_find_files(
    workdir: String,
    query: Option<String>,
    max_results: Option<usize>,
) -> Result<Vec<String>, String> {
    let root = Path::new(&workdir);
    if !root.exists() || !root.is_dir() {
        return Err(format!("Workdir does not exist: {}", workdir));
    }

    let limit = max_results.unwrap_or(60).min(200);
    let q = query
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());

    let mut results = Vec::new();
    walk_dir_filtered(root, root, q.as_deref(), limit, &mut results);
    Ok(results)
}

fn walk_dir_filtered(
    root: &Path,
    current: &Path,
    query: Option<&str>,
    limit: usize,
    results: &mut Vec<String>,
) {
    if results.len() >= limit {
        return;
    }

    let entries = match fs::read_dir(current) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        if results.len() >= limit {
            break;
        }

        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if path.is_dir() {
            if matches!(
                name_str.as_ref(),
                ".git" | "node_modules" | "target" | "dist" | "build" | ".vite"
            ) {
                continue;
            }
            walk_dir_filtered(root, &path, query, limit, results);
        } else if path.is_file() {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");

            let matched = match query {
                Some(q) => rel.to_lowercase().contains(q),
                None => true,
            };

            if matched {
                results.push(rel);
            }
        }
    }
}

#[tauri::command]
pub fn pc_read_range(
    workdir: String,
    path: String,
    start_line: usize,
    end_line: usize,
) -> Result<Value, String> {
    let root_canon = fs::canonicalize(&workdir)
        .map_err(|e| format!("Invalid workdir '{}': {}", workdir, e))?;

    let target = PathBuf::from(&workdir).join(&path);
    let target_canon = fs::canonicalize(&target)
        .map_err(|e| format!("Cannot resolve path '{}': {}", path, e))?;

    if !target_canon.starts_with(&root_canon) {
        return Err("Access denied: path escapes workdir".to_string());
    }

    if !target_canon.is_file() {
        return Err(format!("Not a file: {}", path));
    }

    let raw = fs::read_to_string(&target_canon)
        .map_err(|e| format!("Failed to read file '{}': {}", path, e))?;

    let lines: Vec<&str> = raw.lines().collect();
    let total = lines.len();

    if total == 0 {
        return Ok(json!({
            "path": path.replace('\\', "/"),
            "start_line": 1,
            "end_line": 0,
            "total_lines": 0,
            "content": "",
            "estimated_tokens": 1
        }));
    }

    let s = start_line.max(1).min(total);
    let max_end = s.saturating_add(399);
    let e = end_line.max(s).min(total).min(max_end);

    let snippet = lines[s - 1..e].join("\n");
    let estimated_tokens = (snippet.len() / 4).max(1);

    let rel = target_canon
        .strip_prefix(&root_canon)
        .unwrap_or(&target_canon)
        .to_string_lossy()
        .replace('\\', "/");

    Ok(json!({
        "path": rel,
        "start_line": s,
        "end_line": e,
        "total_lines": total,
        "content": snippet,
        "estimated_tokens": estimated_tokens
    }))
}

#[tauri::command]
pub fn pc_quick_clean_preview(workdir: String) -> Result<Value, String> {
    let root = Path::new(&workdir);
    if !root.exists() || !root.is_dir() {
        return Err(format!("Workdir does not exist: {}", workdir));
    }

    let targets = ["target", "node_modules/.vite", "dist", "app/dist"];
    let mut items = Vec::with_capacity(targets.len());

    for name in targets {
        let p = root.join(name);
        if p.exists() {
            let bytes = dir_size_bytes(&p);
            let size_mb = ((bytes as f64) / (1024.0 * 1024.0) * 100.0).round() / 100.0;
            items.push(json!({
                "name": name,
                "exists": true,
                "size_mb": size_mb
            }));
        } else {
            items.push(json!({
                "name": name,
                "exists": false,
                "size_mb": 0.0
            }));
        }
    }

    Ok(json!({ "items": items }))
}

fn dir_size_bytes(path: &Path) -> u64 {
    if path.is_file() {
        return fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    }

    let mut total = 0u64;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                total = total.saturating_add(dir_size_bytes(&p));
            } else if let Ok(meta) = entry.metadata() {
                total = total.saturating_add(meta.len());
            }
        }
    }
    total
}
