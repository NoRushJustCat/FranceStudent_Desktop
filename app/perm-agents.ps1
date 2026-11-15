# Installe la pastille « mode d'autorisation » dans le front.
# Usage (depuis fscode\app) :
#   Copy-Item perm-agents.ps1.txt perm-agents.ps1 -Force
#   powershell -ExecutionPolicy Bypass -File .\perm-agents.ps1
$ErrorActionPreference = 'Stop'
try {
    Set-Location $PSScriptRoot

    $src   = 'src\lib\PermMode.svelte.txt'
    $dst   = 'src\lib\PermMode.svelte'
    $store = 'src\lib\permstore.svelte.js'
    $app   = 'src\App.svelte'

    foreach ($f in @($src, $store, $app)) {
        if (-not (Test-Path $f)) { throw "Fichier introuvable : $f" }
    }

    # Lecture de App.svelte en conservant BOM et fins de ligne.
    $full   = (Resolve-Path $app).Path
    $bytes  = [System.IO.File]::ReadAllBytes($full)
    $hasBom = ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF)
    $enc    = New-Object System.Text.UTF8Encoding($hasBom)
    $text   = [System.IO.File]::ReadAllText($full, $enc)
    if ($text.Length -gt 0 -and $text[0] -eq [char]0xFEFF) { $text = $text.Substring(1) }
    $nl = if ($text.Contains("`r`n")) { "`r`n" } else { "`n" }

    if ($text.Contains('PermMode')) { throw 'App.svelte contient deja PermMode : rien a faire.' }

    # 1. Import : apres la ligne qui importe termstore.
    $a1 = "termstore.svelte.js';"
    $i1 = $text.IndexOf($a1)
    if ($i1 -lt 0) { throw "Ancre introuvable dans App.svelte : $a1" }
    $e1 = $text.IndexOf($nl, $i1)
    if ($e1 -lt 0) { throw "Fin de ligne introuvable apres l'import termstore." }

    # 2. Composant : juste apres le bouton Terminal.
    $a2 = 'aria-label="Terminal">'
    $i2 = $text.IndexOf($a2)
    if ($i2 -lt 0) { throw "Ancre introuvable dans App.svelte : $a2" }
    $b2 = $text.IndexOf('</button>', $i2)
    if ($b2 -lt 0) { throw 'Fermeture </button> introuvable apres le bouton Terminal.' }
    $e2 = $b2 + '</button>'.Length
    if ($e2 -le $e1) { throw 'Ordre inattendu : le bouton Terminal precede l import termstore.' }

    # Application bas -> haut pour garder les index valides.
    $new = $text.Substring(0, $e2) + $nl + '    <PermMode />' + $text.Substring($e2)
    $new = $new.Substring(0, $e1) + $nl + "  import PermMode from './lib/PermMode.svelte';" + $new.Substring($e1)

    Copy-Item $app 'src\App.svelte.bak9' -Force
    Copy-Item $src $dst -Force
    [System.IO.File]::WriteAllText($full, $new, $enc)

    Write-Host 'OK. Sauvegarde : src\App.svelte.bak9' -ForegroundColor Green
    Select-String -Path $app -Pattern 'PermMode' | ForEach-Object { Write-Host ("  App.svelte:{0}: {1}" -f $_.LineNumber, $_.Line.Trim()) }
    Write-Host 'Ensuite : cd src-tauri ; cargo fmt ; cargo check --workspace ; cd .. ; npm run tauri dev'
}
catch {
    Write-Host "ERREUR : $($_.Exception.Message)" -ForegroundColor Red
    exit 1
}
