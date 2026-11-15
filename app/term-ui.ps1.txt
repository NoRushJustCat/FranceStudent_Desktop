# term-ui.ps1 : branche l'onglet Terminal dans App.svelte (fichier ASCII uniquement).
# A lancer depuis fscode\app :
#   Copy-Item term-ui.ps1.txt term-ui.ps1 -Force
#   powershell -NoProfile -ExecutionPolicy Bypass -File .\term-ui.ps1
$ErrorActionPreference = 'Stop'
$lib = Join-Path $PSScriptRoot 'src\lib'
$app = Join-Path $PSScriptRoot 'src\App.svelte'

# Remplacement litteral, exactement une occurrence.
function Rep([string]$s, [string]$old, [string]$new, [string]$name) {
  $i = $s.IndexOf($old, [StringComparison]::Ordinal)
  if ($i -lt 0) { throw "ancre introuvable : $name" }
  if ($s.IndexOf($old, $i + 1, [StringComparison]::Ordinal) -ge 0) { throw "ancre multiple : $name" }
  return $s.Substring(0, $i) + $new + $s.Substring($i + $old.Length)
}

# Remplacement par regex, exactement une correspondance.
function RepRx([string]$s, [string]$pat, [string]$new, [string]$name) {
  $n = [regex]::Matches($s, $pat).Count
  if ($n -ne 1) { throw "ancre $name : $n correspondance(s) au lieu de 1" }
  return [regex]::Replace($s, $pat, $new)
}

try {
  foreach ($f in 'DiffPanel.svelte.txt', 'Terminal.svelte.txt', 'termstore.svelte.js') {
    if (-not (Test-Path (Join-Path $lib $f))) { throw "fichier manquant : src\lib\$f" }
  }
  if (-not (Test-Path $app)) { throw 'src\App.svelte introuvable : lancer le script depuis fscode\app' }

  $bytes = [IO.File]::ReadAllBytes($app)
  $bom = ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF)
  $raw = [IO.File]::ReadAllText($app)
  $crlf = $raw.Contains("`r`n")
  $t = $raw.Replace("`r`n", "`n")

  if ($t.Contains('termstore') -or $t.Contains('toggleDp')) { throw 'deja applique (termstore ou toggleDp present dans App.svelte)' }
  if (-not $t.Contains('curWorkdir')) { throw 'curWorkdir absent de App.svelte' }

  # (a) import de l'etat du terminal (ecoute des evenements des le demarrage)
  $r1 = @'
import DiffPanel from './lib/DiffPanel.svelte';
  import { term as termState } from './lib/termstore.svelte.js';
'@
  $t = Rep $t "import DiffPanel from './lib/DiffPanel.svelte';" $r1 'import DiffPanel'

  # (b) onglet courant + bascule
  $r2 = @'
let showDiff = $state(false);
  let panelTab = $state('files');
  function toggleDp(t) {
    if (showDiff && panelTab === t) showDiff = false;
    else {
      panelTab = t;
      showDiff = true;
    }
  }
'@
  $t = Rep $t 'let showDiff = $state(false);' $r2 'let showDiff'

  # (c) le bouton diff ouvre l'onglet Fichiers
  $p3 = 'class:on=[{]showDiff[}](\s+)onclick=[{][(][)]\s*=>\s*[(]showDiff = !showDiff[)][}]'
  $r3 = @'
class:on={showDiff && panelTab === 'files'}${1}onclick={() => toggleDp('files')}
'@
  $r3 = $r3.Trim()
  $t = RepRx $t $p3 $r3 'bouton diff'

  # (d) bouton terminal juste apres le bouton diff (hors du {#if} eventuel qui l'entoure)
  $k = $t.IndexOf("toggleDp('files')", [StringComparison]::Ordinal)
  $bs = $t.LastIndexOf('<button', $k, [StringComparison]::Ordinal)
  $be = $t.IndexOf('</button>', $k, [StringComparison]::Ordinal)
  if ($bs -lt 0 -or $be -lt 0) { throw 'balise button du bouton diff introuvable' }
  $be += 9
  $ins = $be
  $before = $t.Substring(0, $bs)
  $after = $t.Substring($be)
  if ($before -match '[{][#:]if[^}]*[}]\s*$') {
    $m = [regex]::Match($after, '^\s*[{]/if[}]')
    if ($m.Success) { $ins = $be + $m.Length }
  }
  $btn = @'
<button type="button" class="pill anim-fade dfbtn" class:on={showDiff && panelTab === 'term'} onclick={() => toggleDp('term')} title="Terminal" aria-label="Terminal">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="4 17 10 11 4 5" /><line x1="12" y1="19" x2="20" y2="19" /></svg>
        {#if termState.unread}<span class="dfn">{termState.unread}</span>{/if}
      </button>
'@
  $t = $t.Insert($ins, "`n      " + $btn.TrimEnd())

  # (e) le panneau recoit le dossier du chat et l'onglet courant
  $p5 = '<DiffPanel\s+[{]items[}]\s+onclose=[{][(][)]\s*=>\s*[(]showDiff = false[)][}]\s*/>'
  $r5 = '<DiffPanel {items} cwd={curWorkdir} bind:tab={panelTab} onclose={() => (showDiff = false)} />'
  $t = RepRx $t $p5 $r5 'DiffPanel'

  $out = $t
  if ($crlf) { $out = $out.Replace("`n", "`r`n") }

  # Tout est calcule : on ecrit (composants d'abord, App.svelte en dernier).
  Copy-Item $app "$app.bak6" -Force
  $dp = Join-Path $lib 'DiffPanel.svelte'
  if (Test-Path $dp) { Copy-Item $dp "$dp.bak6" -Force }
  Copy-Item (Join-Path $lib 'DiffPanel.svelte.txt') $dp -Force
  Copy-Item (Join-Path $lib 'Terminal.svelte.txt') (Join-Path $lib 'Terminal.svelte') -Force
  [IO.File]::WriteAllText($app, $out, (New-Object Text.UTF8Encoding($bom)))

  Select-String -Path $app -Pattern 'termstore|toggleDp|panelTab|termState' | ForEach-Object { '{0}: {1}' -f $_.LineNumber, $_.Line.Trim() }
  Write-Host 'OK : terminal branche. Sauvegardes : src\App.svelte.bak6, src\lib\DiffPanel.svelte.bak6' -ForegroundColor Green
}
catch {
  Write-Host ('ERREUR : ' + $_.Exception.Message) -ForegroundColor Red
  Write-Host 'App.svelte non modifie. Lignes utiles a me coller :'
  if (Test-Path $app) {
    Select-String -Path $app -Pattern 'showDiff|DiffPanel|dfbtn|curWorkdir' | ForEach-Object { '{0}: {1}' -f $_.LineNumber, $_.Line.Trim() }
  }
  exit 1
}
