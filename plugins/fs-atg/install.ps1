<#
.SYNOPSIS
  Installateur universel 1-clic pour le plugin France Student [ATG] sur Windows (multi-PC).
  Détecte automatiquement le profil utilisateur, installe les dépendances Node.js et génère mcp_config.json.
#>
$ErrorActionPreference = 'Stop'

Write-Host "`n=== Installation France Student [ATG] pour Antigravity ===" -ForegroundColor Cyan

# 1. Vérification de Node.js (>= 18)
$nodeCmd = Get-Command node -ErrorAction SilentlyContinue
if (-not $nodeCmd) {
  Write-Host "ERREUR : Node.js est requis (version 18 ou supérieure). Installe-le depuis https://nodejs.org/" -ForegroundColor Red
  exit 1
}
$nodeVer = (& node -v).Trim()
Write-Host "Node.js détecté : $nodeVer" -ForegroundColor Green

# 2. Chemins source & destination (100 % portables, aucun nom d'utilisateur en dur)
$sourceDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$targetDir = Join-Path $env:USERPROFILE ".gemini\config\plugins\fs-atg"
$mcpScriptPosix = ($targetDir -replace '\\', '/') + "/mcp_server.mjs"

Write-Host "Dossier cible : $targetDir" -ForegroundColor Gray
New-Item -ItemType Directory -Force -Path $targetDir | Out-Null

# 3. Copie des fichiers du plugin (sans écraser les données utilisateur .fsatg_* ni node_modules si déjà dans la cible)
$sourceNorm = [System.IO.Path]::GetFullPath($sourceDir).TrimEnd('\')
$targetNorm = [System.IO.Path]::GetFullPath($targetDir).TrimEnd('\')

if ($sourceNorm -ine $targetNorm) {
  Write-Host "Copie des fichiers du plugin..." -ForegroundColor Cyan
  $excludeDirs = @('node_modules', '.git', 'tests')
  Get-ChildItem -Path $sourceDir -Force | ForEach-Object {
    $name = $_.Name
    if ($name -like '.fsatg_*' -or $name -eq 'mcp_config.json' -or $excludeDirs -contains $name) {
      return
    }
    $dest = Join-Path $targetDir $name
    if ($_.PSIsContainer) {
      Copy-Item -Path $_.FullName -Destination $dest -Recurse -Force
    } else {
      Copy-Item -Path $_.FullName -Destination $dest -Force
    }
  }
}

# 4. Génération automatique de mcp_config.json avec le chemin exact du PC courant
$mcpConfigPath = Join-Path $targetDir "mcp_config.json"
$mcpConfig = @{
  mcpServers = @{
    "fs-atg" = @{
      command = "node"
      args = @($mcpScriptPosix)
      env = @{
        FS_ATG_IDE = "antigravity"
      }
    }
  }
} | ConvertTo-Json -Depth 5
Set-Content -Path $mcpConfigPath -Value $mcpConfig -Encoding UTF8
Write-Host "mcp_config.json généré automatiquement : $mcpScriptPosix" -ForegroundColor Green

# 5. Installation des dépendances npm (production uniquement)
Write-Host "Installation des dépendances npm..." -ForegroundColor Cyan
Push-Location $targetDir
try {
  & npm install --omit=dev --no-audit --no-fund
  if ($LASTEXITCODE -ne 0) {
    throw "npm install a échoué (code $LASTEXITCODE)."
  }
} finally {
  Pop-Location
}

Write-Host "`nInstallation terminée avec succès !" -ForegroundColor Green
Write-Host "1. Redémarre Antigravity pour charger le plugin France Student [ATG]." -ForegroundColor Yellow
Write-Host "2. Ouvre le panneau latéral France Student [ATG] puis connecte ton compte France Student." -ForegroundColor Yellow
Write-Host "3. Les futures mises à jour se feront en 1 clic via le bouton 'Mettre à jour' du panneau." -ForegroundColor Yellow
