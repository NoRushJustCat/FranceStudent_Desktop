import { spawn } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { logEvent } from './logger.mjs';

const __dirname = dirname(fileURLToPath(import.meta.url));
export const COMPUTER_CAPTURE_DIR = join(__dirname, 'sidecars', 'fs_atg_panel', '.fsatg_captures');

export const COMPUTER_ACTIONS = Object.freeze([
  'screen_info',
  'screenshot',
  'windows',
  'focus_window',
  'ui_tree',
  'ui_click',
  'mouse_move',
  'click',
  'double_click',
  'right_click',
  'drag',
  'scroll',
  'type',
  'key',
  'hotkey',
  'clipboard_get',
  'clipboard_set',
]);

const FORBIDDEN_HOTKEYS = new Set([
  'ctrl+alt+del',
  'ctrl+alt+delete',
  'win+r',
  'win+x',
  'alt+f4',
]);

function normalizeHotkeyString(raw) {
  return String(raw || '')
    .trim()
    .toLowerCase()
    .replace(/\s+/g, '')
    .replace(/control/g, 'ctrl')
    .replace(/command|cmd|meta|super/g, 'win');
}

export function validateComputerAction(params = {}, { permissionMode = 'acceptEdits', computerUseEnabled = true } = {}) {
  const action = String(params?.action || '').trim();
  if (!COMPUTER_ACTIONS.includes(action)) {
    throw new Error(`Action Computer Use inconnue : "${action}". Actions valides : ${COMPUTER_ACTIONS.join(', ')}`);
  }
  if (computerUseEnabled === false) {
    throw new Error('Computer Use est désactivé dans les réglages du panneau.');
  }
  const readOnlyActions = new Set(['screen_info', 'screenshot', 'windows', 'ui_tree', 'clipboard_get']);
  if (!readOnlyActions.has(action)) {
    if (permissionMode === 'plan') {
      throw new Error(`Action "${action}" bloquée en mode Lecture seule (plan). Passe en mode Édition ou Complet.`);
    }
  }
  if (action === 'ui_click' && !String(params.name || '').trim() && !String(params.automationId || '').trim()) {
    throw new Error('Paramètre "name" ou "automationId" requis pour ui_click.');
  }
  if (action === 'hotkey' || action === 'key') {
    const norm = normalizeHotkeyString(params.keys || params.key || '');
    if (!norm) throw new Error('Paramètre "key" ou "keys" requis.');
    if (FORBIDDEN_HOTKEYS.has(norm)) {
      throw new Error(`Raccourci système protégé refusé : ${norm}`);
    }
  }
  if (action === 'screenshot' && params.screenIndex !== undefined && params.screenIndex !== null && params.screenIndex !== '') {
    const si = Number(params.screenIndex);
    if (!Number.isInteger(si) || si < 0) throw new Error('Paramètre "screenIndex" invalide : entier >= 0 attendu.');
  }
  if (['mouse_move', 'drag'].includes(action)) {
    if (!Number.isFinite(Number(params.x)) || !Number.isFinite(Number(params.y))) {
      throw new Error(`Coordonnées x et y numériques requises pour "${action}".`);
    }
  }
  if (action === 'drag') {
    if (!Number.isFinite(Number(params.toX)) || !Number.isFinite(Number(params.toY))) {
      throw new Error('Coordonnées toX et toY numériques requises pour "drag".');
    }
  }
  return action;
}

function runPowerShellJson(script, payloadObj = {}, timeoutMs = 15_000) {
  return new Promise((resolve, reject) => {
    const b64Payload = Buffer.from(JSON.stringify(payloadObj), 'utf8').toString('base64');
    const fullScript = `[Console]::OutputEncoding = [System.Text.Encoding]::UTF8\n$ErrorActionPreference = 'Stop'\n$payloadJson = [System.Text.Encoding]::UTF8.GetString([System.Convert]::FromBase64String('${b64Payload}'))\n$p = $payloadJson | ConvertFrom-Json\n` + script;
    const encoded = Buffer.from(fullScript, 'utf16le').toString('base64');

    const child = spawn('powershell.exe', [
      '-NoLogo', '-NoProfile', '-NonInteractive', '-OutputFormat', 'Text', '-ExecutionPolicy', 'Bypass',
      '-EncodedCommand', encoded,
    ], { stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true });

    let stdout = '';
    let stderr = '';
    const timer = setTimeout(() => {
      try { child.kill(); } catch {}
      reject(new Error(`Timeout Computer Use (${Math.round(timeoutMs / 1000)}s).`));
    }, timeoutMs);

    child.stdout.setEncoding('utf8');
    child.stderr.setEncoding('utf8');
    child.stdout.on('data', (d) => { stdout += d; });
    child.stderr.on('data', (d) => { stderr += d; });
    child.on('error', (err) => {
      clearTimeout(timer);
      reject(err);
    });
    child.on('close', (code) => {
      clearTimeout(timer);
      const lastLine = stdout.trim().split(/\r?\n/).filter(Boolean).at(-1) || '';
      try {
        const parsed = JSON.parse(lastLine);
        if (parsed && parsed.ok === false) {
          reject(new Error(parsed.error || 'Erreur Computer Use.'));
          return;
        }
        resolve(parsed);
      } catch {
        if (code !== 0) {
          reject(new Error((stderr || stdout || `PowerShell exit ${code}`).trim().slice(0, 320)));
        } else {
          resolve({ ok: true, raw: stdout.trim() });
        }
      }
    });
  });
}

const WIN32_CSHARP_HEADER = `
$ProgressPreference = 'SilentlyContinue'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
if (-not ("FsAtgWin32" -as [type])) {
  $csCode = 'using System; using System.Runtime.InteropServices; using System.Text; public static class FsAtgWin32 { [DllImport("user32.dll")] public static extern bool SetCursorPos(int X, int Y); [DllImport("user32.dll")] public static extern void mouse_event(uint dwFlags, int dx, int dy, int dwData, IntPtr dwExtraInfo); [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow(); [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr hWnd, StringBuilder text, int count); [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd); [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow); [DllImport("user32.dll")] public static extern bool SetProcessDPIAware(); }'
  Add-Type -TypeDefinition $csCode
}
[void][FsAtgWin32]::SetProcessDPIAware()
`;

function toSendKeysPattern(rawKey, isHotkey = false) {
  const s = String(rawKey || '').trim();
  if (!isHotkey) {
    const map = {
      enter: '{ENTER}', return: '{ENTER}',
      escape: '{ESC}', esc: '{ESC}',
      tab: '{TAB}',
      backspace: '{BACKSPACE}', bs: '{BACKSPACE}',
      delete: '{DELETE}', del: '{DELETE}',
      up: '{UP}', arrowup: '{UP}',
      down: '{DOWN}', arrowdown: '{DOWN}',
      left: '{LEFT}', arrowleft: '{LEFT}',
      right: '{RIGHT}', arrowright: '{RIGHT}',
      home: '{HOME}', end: '{END}',
      pageup: '{PGUP}', pgup: '{PGUP}',
      pagedown: '{PGDN}', pgdn: '{PGDN}',
      space: ' ',
      f1: '{F1}', f2: '{F2}', f3: '{F3}', f4: '{F4}', f5: '{F5}', f6: '{F6}',
      f7: '{F7}', f8: '{F8}', f9: '{F9}', f10: '{F10}', f11: '{F11}', f12: '{F12}',
    };
    const low = s.toLowerCase();
    return map[low] || s.replace(/([+^%~(){}])/g, '{$1}');
  }
  const parts = s.toLowerCase().split('+').map((p) => p.trim()).filter(Boolean);
  let prefix = '';
  let main = '';
  for (const part of parts) {
    if (part === 'ctrl' || part === 'control') prefix += '^';
    else if (part === 'alt') prefix += '%';
    else if (part === 'shift') prefix += '+';
    else main = toSendKeysPattern(part, false);
  }
  return `${prefix}${main}`;
}

export async function executeComputerAction(params = {}, options = {}) {
  const action = validateComputerAction(params, options);

  if (process.platform !== 'win32') {
    throw new Error(`Computer Use natif requiert Windows (plateforme actuelle : ${process.platform}).`);
  }

  try {
    mkdirSync(COMPUTER_CAPTURE_DIR, { recursive: true });
  } catch {}

  switch (action) {
    case 'screen_info': {
      const res = await runPowerShellJson(`
${WIN32_CSHARP_HEADER}
$screens = @([System.Windows.Forms.Screen]::AllScreens | ForEach-Object {
  @{
    deviceName = $_.DeviceName
    primary = [bool]$_.Primary
    x = $_.Bounds.X
    y = $_.Bounds.Y
    width = $_.Bounds.Width
    height = $_.Bounds.Height
    workingArea = @{ x = $_.WorkingArea.X; y = $_.WorkingArea.Y; width = $_.WorkingArea.Width; height = $_.WorkingArea.Height }
  }
})
$pos = [System.Windows.Forms.Cursor]::Position
$fg = [FsAtgWin32]::GetForegroundWindow()
$sb = New-Object System.Text.StringBuilder 512
[void][FsAtgWin32]::GetWindowText($fg, $sb, 512)
$actTitle = $sb.ToString()
if (-not $actTitle) {
  $topP = Get-Process | Where-Object { $_.WorkingSet64 -gt 35MB -and $_.ProcessName -notmatch '^(svchost|conhost|RuntimeBroker|SearchIndexer|dllhost|WmiPrvSE|csrss|smss|services|lsass|fontdrvhost|dwm|Memory Compression|System)$' } | Sort-Object WorkingSet64 -Descending | Select-Object -First 1
  if ($topP) { $actTitle = "$($topP.ProcessName) (PID $($topP.Id))" }
}
@{
  ok = $true
  action = 'screen_info'
  cursor = @{ x = $pos.X; y = $pos.Y }
  activeWindow = $actTitle
  screens = $screens
} | ConvertTo-Json -Compress -Depth 5
`);
      return res;
    }

    case 'screenshot': {
      const fileName = `capture_${Date.now()}_${randomUUID().slice(0, 6)}.png`;
      const outPath = params.outputPath ? String(params.outputPath) : join(COMPUTER_CAPTURE_DIR, fileName);
      const region = params.region && typeof params.region === 'object' ? params.region : null;
      const res = await runPowerShellJson(`
${WIN32_CSHARP_HEADER}
$allScreens = @([System.Windows.Forms.Screen]::AllScreens)
$screen = [System.Windows.Forms.Screen]::PrimaryScreen
$screenIndex = [Array]::IndexOf($allScreens, $screen)
if ($p.dn) {
  for ($i = 0; $i -lt $allScreens.Count; $i++) {
    if ($allScreens[$i].DeviceName -eq $p.dn) { $screen = $allScreens[$i]; $screenIndex = $i; break }
  }
} elseif ($null -ne $p.si) {
  $wanted = [int]$p.si
  if ($wanted -ge 0 -and $wanted -lt $allScreens.Count) { $screen = $allScreens[$wanted]; $screenIndex = $wanted }
}
$bounds = $screen.Bounds
$rx = if ($null -ne $p.rx) { [int]$p.rx } else { $bounds.X }
$ry = if ($null -ne $p.ry) { [int]$p.ry } else { $bounds.Y }
$rw = if ($null -ne $p.rw -and [int]$p.rw -gt 0) { [int]$p.rw } else { $bounds.Width }
$rh = if ($null -ne $p.rh -and [int]$p.rh -gt 0) { [int]$p.rh } else { $bounds.Height }
$bmp = New-Object System.Drawing.Bitmap($rw, $rh)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$protectedSurface = $false
try {
  $g.CopyFromScreen($rx, $ry, 0, 0, (New-Object System.Drawing.Size($rw, $rh)))
} catch {
  $protectedSurface = $true
  $g.Clear([System.Drawing.Color]::FromArgb(15, 23, 42))
  $fTitle = New-Object System.Drawing.Font('Segoe UI', 24, [System.Drawing.FontStyle]::Bold)
  $fSub = New-Object System.Drawing.Font('Segoe UI', 16, [System.Drawing.FontStyle]::Regular)
  $g.DrawString("France Student ATG v2.6.0 - Capture Ecran ($rw x $rh)", $fTitle, [System.Drawing.Brushes]::White, 36, 36)
  $g.DrawString("Surface DirectX / Session active sur $env:COMPUTERNAME", $fSub, [System.Drawing.Brushes]::LightSkyBlue, 36, 88)
  $topList = @(Get-Process | Where-Object { $_.WorkingSet64 -gt 35MB -and $_.ProcessName -notmatch '^(svchost|conhost|RuntimeBroker|SearchIndexer|dllhost|WmiPrvSE|csrss|smss|services|lsass|fontdrvhost|dwm|Memory Compression|System)$' } | Sort-Object WorkingSet64 -Descending | Select-Object -First 6)
  $yOff = 136
  foreach ($tp in $topList) {
    $mb = [math]::Round($tp.WorkingSet64 / 1MB)
    $g.DrawString("$($tp.ProcessName) (PID $($tp.Id)) - $mb Mo RAM", $fSub, [System.Drawing.Brushes]::LightGreen, 36, $yOff)
    $yOff += 34
  }
  $fTitle.Dispose()
  $fSub.Dispose()
}
$g.Dispose()
$dir = Split-Path -Parent $p.outPath
if ($dir -and -not (Test-Path $dir)) { New-Item -ItemType Directory -Force $dir | Out-Null }
$bmp.Save($p.outPath, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
$pos = [System.Windows.Forms.Cursor]::Position
$fg = [FsAtgWin32]::GetForegroundWindow()
$sb = New-Object System.Text.StringBuilder 512
[void][FsAtgWin32]::GetWindowText($fg, $sb, 512)
@{
  ok = $true
  action = 'screenshot'
  path = $p.outPath
  width = $rw
  height = $rh
  protectedSurface = $protectedSurface
  screenIndex = $screenIndex
  deviceName = $screen.DeviceName
  screensCount = $allScreens.Count
  screenBounds = @{ x = $bounds.X; y = $bounds.Y; width = $bounds.Width; height = $bounds.Height }
  cursor = @{ x = $pos.X; y = $pos.Y }
  activeWindow = $sb.ToString()
} | ConvertTo-Json -Compress
`, {
        outPath,
        rx: region?.x ?? null,
        ry: region?.y ?? null,
        rw: region?.width ?? null,
        rh: region?.height ?? null,
        si: Number.isInteger(Number(params.screenIndex)) && params.screenIndex !== null && params.screenIndex !== '' ? Number(params.screenIndex) : null,
        dn: params.deviceName ? String(params.deviceName) : null,
      });
      const bytes = existsSync(outPath) ? statSync(outPath).size : 0;
      let dataUrl = null;
      if (bytes > 0 && bytes <= 1_800_000) {
        try {
          dataUrl = `data:image/png;base64,${readFileSync(outPath).toString('base64')}`;
        } catch {}
      }
      logEvent('info', 'computer', 'computer.screenshot', `Capture écran enregistrée (${res.width}x${res.height}, ${bytes} octets).`, { path: outPath });
      return { ...res, bytes, dataUrl };
    }

    case 'windows': {
      return await runPowerShellJson(`
$list = @(Get-Process | Where-Object { $_.MainWindowHandle -ne 0 -and $_.MainWindowTitle } | Select-Object -First 40 | ForEach-Object {
  @{
    pid = $_.Id
    processName = $_.ProcessName
    title = $_.MainWindowTitle
  }
})
if ($list.Count -eq 0) {
  $list = @(Get-Process | Where-Object { $_.WorkingSet64 -gt 30MB -and $_.ProcessName -notmatch '^(svchost|conhost|RuntimeBroker|SearchIndexer|dllhost|WmiPrvSE|csrss|smss|services|lsass|fontdrvhost|dwm|Memory Compression|System)$' } | Sort-Object WorkingSet64 -Descending | Select-Object -First 15 | ForEach-Object {
    @{
      pid = $_.Id
      processName = $_.ProcessName
      title = "$($_.ProcessName) ($([math]::Round($_.WorkingSet64 / 1MB)) Mo)"
    }
  })
}
@{ ok = $true; action = 'windows'; count = $list.Count; windows = $list } | ConvertTo-Json -Compress -Depth 4
`);
    }

    case 'focus_window': {
      const query = String(params.title || params.processName || '').trim();
      const pid = Number(params.pid) || null;
      if (!query && !pid) throw new Error('Paramètre "title", "processName" ou "pid" requis pour focus_window.');
      const res = await runPowerShellJson(`
${WIN32_CSHARP_HEADER}
$proc = $null
if ($p.pid) {
  $proc = Get-Process -Id ([int]$p.pid) -ErrorAction SilentlyContinue
} else {
  $q = [string]$p.query
  $proc = Get-Process | Where-Object { $_.MainWindowHandle -ne 0 -and ($_.MainWindowTitle -like "*$q*" -or $_.ProcessName -like "*$q*") } | Select-Object -First 1
}
if (-not $proc -or $proc.MainWindowHandle -eq 0) {
  @{ ok = $false; error = "Fenêtre introuvable." } | ConvertTo-Json -Compress
  exit 0
}
[void][FsAtgWin32]::ShowWindow($proc.MainWindowHandle, 9)
[void][FsAtgWin32]::SetForegroundWindow($proc.MainWindowHandle)
@{ ok = $true; action = 'focus_window'; pid = $proc.Id; title = $proc.MainWindowTitle; processName = $proc.ProcessName } | ConvertTo-Json -Compress
`, { query, pid });
      logEvent('info', 'computer', 'computer.focus_window', `Fenêtre activée : ${res.title || query}`);
      return res;
    }

    case 'ui_tree': {
      const query = String(params.title || params.processName || '').trim();
      const pid = Number(params.pid) || null;
      const maxElements = Math.max(5, Math.min(120, Math.round(Number(params.maxElements ?? 50) || 50)));
      const res = await runPowerShellJson(`
${WIN32_CSHARP_HEADER}
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

$targetHwnd = [IntPtr]::Zero
$winTitle = ''
$winPid = 0
if ($p.pid) {
  $proc = Get-Process -Id ([int]$p.pid) -ErrorAction SilentlyContinue
  if ($proc -and $proc.MainWindowHandle -ne 0) {
    $targetHwnd = $proc.MainWindowHandle
    $winTitle = $proc.MainWindowTitle
    $winPid = $proc.Id
  }
} elseif ($p.query) {
  $q = [string]$p.query
  $proc = Get-Process | Where-Object { $_.MainWindowHandle -ne 0 -and ($_.MainWindowTitle -like "*$q*" -or $_.ProcessName -like "*$q*") } | Select-Object -First 1
  if ($proc) {
    $targetHwnd = $proc.MainWindowHandle
    $winTitle = $proc.MainWindowTitle
    $winPid = $proc.Id
  }
} else {
  $targetHwnd = [FsAtgWin32]::GetForegroundWindow()
}

$rootEl = $null
if ($targetHwnd -ne [IntPtr]::Zero) {
  try { $rootEl = [System.Windows.Automation.AutomationElement]::FromHandle($targetHwnd) } catch {}
}
if (-not $rootEl) {
  try {
    $focused = [System.Windows.Automation.AutomationElement]::FocusedElement
    if ($focused) {
      $walkerUp = [System.Windows.Automation.TreeWalker]::ControlViewWalker
      $cur = $focused
      while ($cur -and $cur.Current.ControlType.ProgrammaticName -ne 'ControlType.Window') {
        $parent = $walkerUp.GetParent($cur)
        if (-not $parent -or $parent -eq [System.Windows.Automation.AutomationElement]::RootElement) { break }
        $cur = $parent
      }
      $rootEl = $cur
    }
  } catch {}
}
if (-not $rootEl) {
  try {
    $fallbackProc = Get-Process | Where-Object { $_.MainWindowHandle -ne 0 -and $_.MainWindowTitle } | Select-Object -First 1
    if ($fallbackProc) {
      $targetHwnd = $fallbackProc.MainWindowHandle
      $winTitle = $fallbackProc.MainWindowTitle
      $winPid = $fallbackProc.Id
      $rootEl = [System.Windows.Automation.AutomationElement]::FromHandle($targetHwnd)
    }
  } catch {}
}
if (-not $rootEl) {
  try { $rootEl = [System.Windows.Automation.AutomationElement]::RootElement } catch {}
}
if (-not $rootEl) {
  @{ ok = $true; action = 'ui_tree'; windowTitle = ''; pid = 0; count = 0; visited = 0; controls = @(); note = 'Aucune fenêtre UIA accessible.' } | ConvertTo-Json -Compress
  exit 0
}
if (-not $winTitle) { try { $winTitle = [string]$rootEl.Current.Name } catch { $winTitle = '' } }
if (-not $winPid) { try { $winPid = [int]$rootEl.Current.ProcessId } catch { $winPid = 0 } }

$allowedTypes = @('Button','Edit','Document','ComboBox','CheckBox','RadioButton','TabItem','MenuItem','ListItem','Hyperlink','TreeItem','Slider','Text')
$walker = [System.Windows.Automation.TreeWalker]::ControlViewWalker
$queue = New-Object System.Collections.ArrayList
[void]$queue.Add($rootEl)
$qHead = 0
$controls = New-Object System.Collections.ArrayList
$visited = 0
$maxVisit = 650

while ($qHead -lt $queue.Count -and $controls.Count -lt [int]$p.maxElements -and $visited -lt $maxVisit) {
  $node = $queue[$qHead]
  $qHead++
  $visited++
  $child = $null
  try { $child = $walker.GetFirstChild($node) } catch {}
  while ($child -and $visited -lt $maxVisit -and $controls.Count -lt [int]$p.maxElements) {
    $visited++
    [void]$queue.Add($child)
    try {
      $c = $child.Current
      $ct = ($c.ControlType.ProgrammaticName -replace '^ControlType\.', '')
      $nm = ([string]$c.Name).Trim()
      $aid = ([string]$c.AutomationId).Trim()
      $rect = $c.BoundingRectangle
      $validRect = (-not [double]::IsInfinity($rect.X)) -and (-not [double]::IsInfinity($rect.Y)) -and (-not [double]::IsInfinity($rect.Width)) -and (-not [double]::IsInfinity($rect.Height)) -and ($rect.Width -gt 1) -and ($rect.Height -gt 1)
      $isShortText = ($ct -ne 'Text') -or ($nm.Length -gt 0 -and $nm.Length -le 90)
      if ((-not $c.IsOffscreen) -and $validRect -and ($allowedTypes -contains $ct) -and $isShortText -and ($nm -or $aid)) {
        $w = [int][math]::Round($rect.Width)
        $h = [int][math]::Round($rect.Height)
        $x = [int][math]::Round($rect.X + $rect.Width / 2.0)
        $y = [int][math]::Round($rect.Y + $rect.Height / 2.0)
        [void]$controls.Add(@{
          name = if ($nm.Length -gt 120) { $nm.Substring(0, 120) } else { $nm }
          controlType = $ct
          automationId = $aid
          x = $x
          y = $y
          width = $w
          height = $h
          enabled = [bool]$c.IsEnabled
        })
      }
    } catch {}
    try { $child = $walker.GetNextSibling($child) } catch { $child = $null }
  }
}

$outTitle = [string]$winTitle
$outPid = [int]$winPid
$outControls = $controls.ToArray()
@{
  ok = $true
  action = 'ui_tree'
  windowTitle = $outTitle
  pid = $outPid
  count = $controls.Count
  visited = $visited
  controls = $outControls
} | ConvertTo-Json -Compress -Depth 5
`, { query, pid, maxElements }, 18_000);
      logEvent('info', 'computer', 'computer.ui_tree', `Arbre UIA lu (${res.count ?? 0} contrôles sur "${res.windowTitle || 'fenêtre'}").`);
      return res;
    }

    case 'ui_click': {
      const targetName = String(params.name || '').trim();
      const targetId = String(params.automationId || '').trim();
      const targetType = String(params.controlType || '').trim();
      const query = String(params.title || params.processName || '').trim();
      const pid = Number(params.pid) || null;
      const useInvoke = params.useInvoke !== false;
      const res = await runPowerShellJson(`
${WIN32_CSHARP_HEADER}
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

$targetHwnd = [IntPtr]::Zero
if ($p.pid) {
  $proc = Get-Process -Id ([int]$p.pid) -ErrorAction SilentlyContinue
  if ($proc -and $proc.MainWindowHandle -ne 0) { $targetHwnd = $proc.MainWindowHandle }
} elseif ($p.query) {
  $q = [string]$p.query
  $proc = Get-Process | Where-Object { $_.MainWindowHandle -ne 0 -and ($_.MainWindowTitle -like "*$q*" -or $_.ProcessName -like "*$q*") } | Select-Object -First 1
  if ($proc) { $targetHwnd = $proc.MainWindowHandle }
} else {
  $targetHwnd = [FsAtgWin32]::GetForegroundWindow()
}

$rootEl = $null
if ($targetHwnd -ne [IntPtr]::Zero) {
  try { $rootEl = [System.Windows.Automation.AutomationElement]::FromHandle($targetHwnd) } catch {}
}
if (-not $rootEl) {
  try {
    $fallbackProc = Get-Process | Where-Object { $_.MainWindowHandle -ne 0 -and $_.MainWindowTitle } | Select-Object -First 1
    if ($fallbackProc) { $rootEl = [System.Windows.Automation.AutomationElement]::FromHandle($fallbackProc.MainWindowHandle) }
  } catch {}
}
if (-not $rootEl) {
  @{ ok = $false; error = 'Fenêtre cible UIA introuvable.' } | ConvertTo-Json -Compress
  exit 0
}

$walker = [System.Windows.Automation.TreeWalker]::ControlViewWalker
$queue = New-Object System.Collections.ArrayList
[void]$queue.Add($rootEl)
$qHead = 0
$visited = 0
$maxVisit = 800
$hit = $null
$hitMeta = $null
$wantName = ([string]$p.targetName).ToLowerInvariant()
$wantId = ([string]$p.targetId).ToLowerInvariant()
$wantType = ([string]$p.targetType).ToLowerInvariant()

while ($qHead -lt $queue.Count -and $visited -lt $maxVisit -and -not $hit) {
  $node = $queue[$qHead]
  $qHead++
  $visited++
  $child = $null
  try { $child = $walker.GetFirstChild($node) } catch {}
  while ($child -and $visited -lt $maxVisit -and -not $hit) {
    $visited++
    [void]$queue.Add($child)
    try {
      $c = $child.Current
      $ct = ($c.ControlType.ProgrammaticName -replace '^ControlType\.', '')
      $nm = ([string]$c.Name).Trim()
      $aid = ([string]$c.AutomationId).Trim()
      $typeOk = (-not $wantType) -or ($ct.ToLowerInvariant() -eq $wantType)
      $idOk = (-not $wantId) -or ($aid.ToLowerInvariant() -eq $wantId)
      $nameOk = (-not $wantName) -or ($nm.ToLowerInvariant().Contains($wantName))
      if ((-not $c.IsOffscreen) -and $c.IsEnabled -and $typeOk -and $idOk -and $nameOk) {
        $rect = $c.BoundingRectangle
        $validRect = (-not [double]::IsInfinity($rect.X)) -and (-not [double]::IsInfinity($rect.Y)) -and (-not [double]::IsInfinity($rect.Width)) -and (-not [double]::IsInfinity($rect.Height)) -and ($rect.Width -gt 1) -and ($rect.Height -gt 1)
        $cx = if ($validRect) { [int][math]::Round($rect.X + $rect.Width / 2.0) } else { $null }
        $cy = if ($validRect) { [int][math]::Round($rect.Y + $rect.Height / 2.0) } else { $null }
        $hit = $child
        $hitMeta = @{ name = $nm; controlType = $ct; automationId = $aid; x = $cx; y = $cy }
        break
      }
    } catch {}
    try { $child = $walker.GetNextSibling($child) } catch { $child = $null }
  }
}

if (-not $hit) {
  @{ ok = $false; error = "Contrôle UIA introuvable (name='$($p.targetName)', automationId='$($p.targetId)')." } | ConvertTo-Json -Compress
  exit 0
}

$method = ''
if ($p.useInvoke) {
  $pat = $null
  try {
    if ($hit.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pat) -and $pat) {
      $pat.Invoke()
      $method = 'InvokePattern'
    }
  } catch {}
}
if (-not $method) {
  if ($null -eq $hitMeta.x -or $null -eq $hitMeta.y) {
    @{ ok = $false; error = 'Contrôle trouvé mais sans rectangle cliquable ni InvokePattern.' } | ConvertTo-Json -Compress
    exit 0
  }
  [void][FsAtgWin32]::SetCursorPos([int]$hitMeta.x, [int]$hitMeta.y)
  Start-Sleep -Milliseconds 25
  [FsAtgWin32]::mouse_event(0x0002, 0, 0, 0, [IntPtr]::Zero)
  Start-Sleep -Milliseconds 20
  [FsAtgWin32]::mouse_event(0x0004, 0, 0, 0, [IntPtr]::Zero)
  $method = 'mouse_click'
}

@{
  ok = $true
  action = 'ui_click'
  method = $method
  matched = $hitMeta
} | ConvertTo-Json -Compress -Depth 4
`, { targetName, targetId, targetType, query, pid, useInvoke }, 18_000);
      logEvent('info', 'computer', 'computer.ui_click', `Clic UIA "${res.matched?.name || targetName || targetId}" (${res.method}).`);
      return res;
    }

    case 'mouse_move':
    case 'click':
    case 'double_click':
    case 'right_click': {
      const hasCoords = params.x !== undefined && params.y !== undefined;
      const x = hasCoords ? Math.round(Number(params.x)) : null;
      const y = hasCoords ? Math.round(Number(params.y)) : null;
      const res = await runPowerShellJson(`
${WIN32_CSHARP_HEADER}
if ($null -ne $p.x -and $null -ne $p.y) {
  [void][FsAtgWin32]::SetCursorPos([int]$p.x, [int]$p.y)
  Start-Sleep -Milliseconds 25
}
$pos = [System.Windows.Forms.Cursor]::Position
switch ([string]$p.act) {
  'click' {
    [FsAtgWin32]::mouse_event(0x0002, 0, 0, 0, [IntPtr]::Zero)
    Start-Sleep -Milliseconds 20
    [FsAtgWin32]::mouse_event(0x0004, 0, 0, 0, [IntPtr]::Zero)
  }
  'double_click' {
    [FsAtgWin32]::mouse_event(0x0002, 0, 0, 0, [IntPtr]::Zero)
    [FsAtgWin32]::mouse_event(0x0004, 0, 0, 0, [IntPtr]::Zero)
    Start-Sleep -Milliseconds 60
    [FsAtgWin32]::mouse_event(0x0002, 0, 0, 0, [IntPtr]::Zero)
    [FsAtgWin32]::mouse_event(0x0004, 0, 0, 0, [IntPtr]::Zero)
  }
  'right_click' {
    [FsAtgWin32]::mouse_event(0x0008, 0, 0, 0, [IntPtr]::Zero)
    Start-Sleep -Milliseconds 20
    [FsAtgWin32]::mouse_event(0x0010, 0, 0, 0, [IntPtr]::Zero)
  }
}
$finalPos = [System.Windows.Forms.Cursor]::Position
@{ ok = $true; action = [string]$p.act; cursor = @{ x = $finalPos.X; y = $finalPos.Y } } | ConvertTo-Json -Compress
`, { act: action, x, y });
      logEvent('info', 'computer', `computer.${action}`, `Souris ${action} (${res.cursor?.x}, ${res.cursor?.y}).`);
      return res;
    }

    case 'drag': {
      const x = Math.round(Number(params.x));
      const y = Math.round(Number(params.y));
      const toX = Math.round(Number(params.toX));
      const toY = Math.round(Number(params.toY));
      const res = await runPowerShellJson(`
${WIN32_CSHARP_HEADER}
[void][FsAtgWin32]::SetCursorPos([int]$p.x, [int]$p.y)
Start-Sleep -Milliseconds 40
[FsAtgWin32]::mouse_event(0x0002, 0, 0, 0, [IntPtr]::Zero)
$steps = 12
for ($i = 1; $i -le $steps; $i++) {
  $cx = [int]($p.x + (($p.toX - $p.x) * $i / $steps))
  $cy = [int]($p.y + (($p.toY - $p.y) * $i / $steps))
  [void][FsAtgWin32]::SetCursorPos($cx, $cy)
  Start-Sleep -Milliseconds 12
}
[FsAtgWin32]::mouse_event(0x0004, 0, 0, 0, [IntPtr]::Zero)
@{ ok = $true; action = 'drag'; from = @{ x = $p.x; y = $p.y }; to = @{ x = $p.toX; y = $p.toY } } | ConvertTo-Json -Compress
`, { x, y, toX, toY });
      logEvent('info', 'computer', 'computer.drag', `Glisser-déposer (${x},${y}) -> (${toX},${toY}).`);
      return res;
    }

    case 'scroll': {
      const deltaY = Math.round(Number(params.deltaY ?? -360));
      const x = params.x !== undefined ? Math.round(Number(params.x)) : null;
      const y = params.y !== undefined ? Math.round(Number(params.y)) : null;
      return await runPowerShellJson(`
${WIN32_CSHARP_HEADER}
if ($null -ne $p.x -and $null -ne $p.y) {
  [void][FsAtgWin32]::SetCursorPos([int]$p.x, [int]$p.y)
  Start-Sleep -Milliseconds 20
}
[FsAtgWin32]::mouse_event(0x0800, 0, 0, [int]$p.deltaY, [IntPtr]::Zero)
@{ ok = $true; action = 'scroll'; deltaY = [int]$p.deltaY } | ConvertTo-Json -Compress
`, { deltaY, x, y });
    }

    case 'type': {
      const text = String(params.text ?? '');
      if (!text) throw new Error('Paramètre "text" requis pour type.');
      const pattern = text.replace(/([+^%~(){}])/g, '{$1}').replace(/\r?\n/g, '{ENTER}');
      await runPowerShellJson(`
Add-Type -AssemblyName System.Windows.Forms
[System.Windows.Forms.SendKeys]::SendWait([string]$p.pattern)
@{ ok = $true; action = 'type'; length = ([string]$p.text).Length } | ConvertTo-Json -Compress
`, { pattern, text });
      logEvent('info', 'computer', 'computer.type', `Saisie clavier (${text.length} caractères).`);
      return { ok: true, action: 'type', length: text.length };
    }

    case 'key':
    case 'hotkey': {
      const raw = String(params.keys || params.key || '').trim();
      const pattern = toSendKeysPattern(raw, action === 'hotkey' || raw.includes('+'));
      await runPowerShellJson(`
Add-Type -AssemblyName System.Windows.Forms
[System.Windows.Forms.SendKeys]::SendWait([string]$p.pattern)
@{ ok = $true; action = [string]$p.act; key = [string]$p.raw } | ConvertTo-Json -Compress
`, { act: action, raw, pattern });
      logEvent('info', 'computer', `computer.${action}`, `Touche envoyée : ${raw}`);
      return { ok: true, action, key: raw, pattern };
    }

    case 'clipboard_get': {
      return await runPowerShellJson(`
$txt = Get-Clipboard -Raw -ErrorAction SilentlyContinue
@{ ok = $true; action = 'clipboard_get'; text = [string]$txt } | ConvertTo-Json -Compress
`);
    }

    case 'clipboard_set': {
      const text = String(params.text ?? '');
      await runPowerShellJson(`
Set-Clipboard -Value ([string]$p.text)
@{ ok = $true; action = 'clipboard_set'; length = ([string]$p.text).Length } | ConvertTo-Json -Compress
`, { text });
      return { ok: true, action: 'clipboard_set', length: text.length };
    }

    default:
      throw new Error(`Action non supportée : ${action}`);
  }
}
