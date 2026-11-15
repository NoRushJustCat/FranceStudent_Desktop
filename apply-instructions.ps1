# Ajoute les commandes Rust get_user_instructions / update_user_instructions
# (GET / PATCH /user/instructions) dans fs-api et main.rs. Gere CRLF et LF.
# Usage, depuis le dossier fscode :
#   Copy-Item .\apply-instructions.txt .\apply-instructions.ps1 -Force
#   powershell -ExecutionPolicy Bypass -File .\apply-instructions.ps1
# Idempotent. S'arrete sans rien ecrire si une ancre est introuvable.

$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$enc = New-Object System.Text.UTF8Encoding($false)

function Fix-Nl([string]$text, [string]$nl) {
  return ($text -replace "`r?`n", $nl)
}

function Insert-Before([string]$raw, [string]$anchor, [string]$block) {
  $i = $raw.IndexOf($anchor)
  if ($i -lt 0) { throw "Ancre introuvable : $anchor" }
  if ($raw.IndexOf($anchor, $i + 1) -ge 0) { throw "Ancre ambigue : $anchor" }
  return $raw.Insert($i, $block)
}

function Insert-After([string]$raw, [string]$anchor, [string]$block) {
  $i = $raw.IndexOf($anchor)
  if ($i -lt 0) { throw "Ancre introuvable : $anchor" }
  if ($raw.IndexOf($anchor, $i + 1) -ge 0) { throw "Ancre ambigue : $anchor" }
  return $raw.Insert($i + $anchor.Length, $block)
}

$libBlock = @'
    /// Recupere les instructions personnalisees (`GET /user/instructions`).
    pub async fn get_user_instructions(&self) -> Result<Value> {
        self.request(Method::GET, "/user/instructions", None).await
    }

    /// Met a jour les instructions personnalisees (`PATCH /user/instructions`, partiel).
    pub async fn update_user_instructions(&self, body: Value) -> Result<Value> {
        self.request(Method::PATCH, "/user/instructions", Some(body))
            .await
    }


'@

$mainBlock = @'
#[tauri::command]
async fn get_user_instructions(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let c = client(&app, &state).await?;
    c.get_user_instructions().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn update_user_instructions(
    app: AppHandle,
    state: State<'_, AppState>,
    body: Value,
) -> Result<Value, String> {
    let c = client(&app, &state).await?;
    c.update_user_instructions(body)
        .await
        .map_err(|e| e.to_string())
}


'@

# ---------- crates/fs-api/src/lib.rs ----------
$lib = Join-Path $root 'crates\fs-api\src\lib.rs'
$libRaw = [System.IO.File]::ReadAllText($lib)
$libNew = $null
if ($libRaw.Contains('get_user_instructions')) {
  Write-Host 'lib.rs : deja fait'
} else {
  $nl = if ($libRaw.Contains("`r`n")) { "`r`n" } else { "`n" }
  $libNew = Insert-Before $libRaw '    /// Envoie un fichier en `multipart/form-data` sur `path`.' (Fix-Nl $libBlock $nl)
}

# ---------- app/src-tauri/src/main.rs ----------
$main = Join-Path $root 'app\src-tauri\src\main.rs'
$mainRaw = [System.IO.File]::ReadAllText($main)
$mainNew = $null
if ($mainRaw.Contains('get_user_instructions')) {
  Write-Host 'main.rs : deja fait'
} else {
  $nl = if ($mainRaw.Contains("`r`n")) { "`r`n" } else { "`n" }
  $m = Insert-Before $mainRaw ('#[tauri::command]' + $nl + 'async fn upload_attachment(') (Fix-Nl $mainBlock $nl)
  $names = $nl + '            get_user_instructions,' + $nl + '            update_user_instructions,'
  $mainNew = Insert-After $m 'update_notification_prefs,' $names
}

# Ecriture seulement si tout s'est bien passe
if ($libNew) { [System.IO.File]::WriteAllText($lib, $libNew, $enc); Write-Host 'lib.rs : OK' }
if ($mainNew) { [System.IO.File]::WriteAllText($main, $mainNew, $enc); Write-Host 'main.rs : OK' }

Write-Host 'Termine. Lance : cargo check --workspace'
