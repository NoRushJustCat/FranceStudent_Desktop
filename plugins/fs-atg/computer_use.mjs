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
  'wait_for',
  'ocr_screen',
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
  const readOnlyActions = new Set(['screen_info', 'screenshot', 'windows', 'ui_tree', 'clipboard_get', 'wait_for', 'ocr_screen']);
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
          const cleanErr = String(stderr || stdout || `PowerShell exit ${code}`)
            .replace(/#< CLIXML[\s\S]*?<\/Objs>/g, '')
            .replace(/#< CLIXML[\s\S]*/g, '')
            .trim();
          reject(new Error((cleanErr || stdout || `PowerShell exit ${code}`).trim().slice(0, 320)));
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
  $csCode = @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class FsAtgWin32 {
  [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT { public int dx; public int dy; public uint mouseData; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
  [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
  [StructLayout(LayoutKind.Sequential)] public struct HARDWAREINPUT { public uint uMsg; public ushort wParamL; public ushort wParamH; }
  [StructLayout(LayoutKind.Explicit)] public struct INPUTUNION { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; [FieldOffset(0)] public HARDWAREINPUT hi; }
  [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public INPUTUNION U; }
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int X, int Y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint dwFlags, int dx, int dy, int dwData, IntPtr dwExtraInfo);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr hWnd, StringBuilder text, int count);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll", SetLastError=true)] public static extern uint SendInput(uint nInputs, INPUT[] pInputs, int cbSize);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern short VkKeyScanW(char ch);
  public static uint TypeUnicode(string text) {
    if (string.IsNullOrEmpty(text)) return 0;
    var list = new List<INPUT>(text.Length * 2);
    for (int i = 0; i < text.Length; i++) {
      char ch = text[i];
      if (ch == '\\r') {
        if (i + 1 < text.Length && text[i + 1] == '\\n') i++;
        list.Add(new INPUT { type = 1, U = new INPUTUNION { ki = new KEYBDINPUT { wVk = 0x0D } } });
        list.Add(new INPUT { type = 1, U = new INPUTUNION { ki = new KEYBDINPUT { wVk = 0x0D, dwFlags = 0x0002 } } });
        continue;
      }
      if (ch == '\\n') {
        list.Add(new INPUT { type = 1, U = new INPUTUNION { ki = new KEYBDINPUT { wVk = 0x0D } } });
        list.Add(new INPUT { type = 1, U = new INPUTUNION { ki = new KEYBDINPUT { wVk = 0x0D, dwFlags = 0x0002 } } });
        continue;
      }
      list.Add(new INPUT { type = 1, U = new INPUTUNION { ki = new KEYBDINPUT { wVk = 0, wScan = ch, dwFlags = 0x0004 } } });
      list.Add(new INPUT { type = 1, U = new INPUTUNION { ki = new KEYBDINPUT { wVk = 0, wScan = ch, dwFlags = 0x0004 | 0x0002 } } });
    }
    return SendInput((uint)list.Count, list.ToArray(), Marshal.SizeOf(typeof(INPUT)));
  }
  public static uint SendVkCombo(int[] vks) {
    if (vks == null || vks.Length == 0) return 0;
    var list = new List<INPUT>(vks.Length * 2);
    for (int i = 0; i < vks.Length; i++) {
      list.Add(new INPUT { type = 1, U = new INPUTUNION { ki = new KEYBDINPUT { wVk = (ushort)vks[i] } } });
    }
    for (int i = vks.Length - 1; i >= 0; i--) {
      list.Add(new INPUT { type = 1, U = new INPUTUNION { ki = new KEYBDINPUT { wVk = (ushort)vks[i], dwFlags = 0x0002 } } });
    }
    return SendInput((uint)list.Count, list.ToArray(), Marshal.SizeOf(typeof(INPUT)));
  }
  public static int LookupVk(char ch) {
    short r = VkKeyScanW(ch);
    return r == -1 ? 0 : (r & 0xFF);
  }
}
'@
  Add-Type -TypeDefinition $csCode
}
[void][FsAtgWin32]::SetProcessDPIAware()
`;

const VK_MAP = {
  backspace: 0x08, bs: 0x08,
  tab: 0x09,
  enter: 0x0D, return: 0x0D,
  shift: 0x10,
  ctrl: 0x11, control: 0x11,
  alt: 0x12, menu: 0x12,
  pause: 0x13, capslock: 0x14,
  escape: 0x1B, esc: 0x1B,
  space: 0x20, ' ': 0x20,
  pageup: 0x21, pgup: 0x21,
  pagedown: 0x22, pgdn: 0x22,
  end: 0x23, home: 0x24,
  left: 0x25, arrowleft: 0x25,
  up: 0x26, arrowup: 0x26,
  right: 0x27, arrowright: 0x27,
  down: 0x28, arrowdown: 0x28,
  insert: 0x2D, ins: 0x2D,
  delete: 0x2E, del: 0x2E,
  win: 0x5B, windows: 0x5B, super: 0x5B, meta: 0x5B, cmd: 0x5B,
  f1: 0x70, f2: 0x71, f3: 0x72, f4: 0x73, f5: 0x74, f6: 0x75,
  f7: 0x76, f8: 0x77, f9: 0x78, f10: 0x79, f11: 0x7A, f12: 0x7B,
};

function toVirtualKeySpec(rawKey, isHotkey = false) {
  const s = String(rawKey || '').trim();
  const tokens = (isHotkey || s.includes('+'))
    ? s.split('+').map((p) => p.trim()).filter(Boolean)
    : [s];
  const vks = [];
  const fallbackChars = [];
  for (const tok of tokens) {
    const low = tok.toLowerCase();
    if (VK_MAP[low] !== undefined) {
      vks.push(VK_MAP[low]);
    } else if (/^[a-z0-9]$/i.test(tok)) {
      vks.push(tok.toUpperCase().charCodeAt(0));
    } else if (tok.length === 1) {
      fallbackChars.push(tok);
    } else {
      throw new Error(`Touche inconnue : "${tok}".`);
    }
  }
  return { vks, fallbackChars };
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
      const fmt = String(params.format || (params.maxWidth ? 'jpeg' : 'png')).toLowerCase() === 'jpeg' || String(params.format || '').toLowerCase() === 'jpg'
        ? 'jpeg'
        : 'png';
      const ext = fmt === 'jpeg' ? 'jpg' : 'png';
      const fileName = `capture_${Date.now()}_${randomUUID().slice(0, 6)}.${ext}`;
      const outPath = params.outputPath ? String(params.outputPath) : join(COMPUTER_CAPTURE_DIR, fileName);
      const region = params.region && typeof params.region === 'object' ? params.region : null;
      const marker = params.marker && typeof params.marker === 'object' && Number.isFinite(Number(params.marker.x)) && Number.isFinite(Number(params.marker.y))
        ? { x: Math.round(Number(params.marker.x)), y: Math.round(Number(params.marker.y)), label: String(params.marker.label || '').slice(0, 24) }
        : null;
      const maxWidth = Number(params.maxWidth) > 0 ? Math.max(320, Math.min(3840, Math.round(Number(params.maxWidth)))) : 0;
      const quality = Math.max(35, Math.min(95, Math.round(Number(params.quality || 76))));
      const winQuery = String(params.windowTitle || params.processName || '').trim();

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

if ($p.winQuery) {
  $q = [string]$p.winQuery
  $wp = Get-Process | Where-Object { $_.MainWindowHandle -ne 0 -and ($_.MainWindowTitle -like "*$q*" -or $_.ProcessName -like "*$q*") } | Select-Object -First 1
  if ($wp) {
    $rect = New-Object FsAtgWin32+RECT
    if ([FsAtgWin32]::GetWindowRect($wp.MainWindowHandle, [ref]$rect)) {
      $ww = $rect.Right - $rect.Left
      $wh = $rect.Bottom - $rect.Top
      if ($ww -gt 40 -and $wh -gt 40) {
        $rx = $rect.Left; $ry = $rect.Top; $rw = $ww; $rh = $wh
      }
    }
  }
}

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

$markerDrawn = $false
if ($null -ne $p.marker) {
  $mx = [int]$p.marker.x - $rx
  $my = [int]$p.marker.y - $ry
  if ($mx -ge 0 -and $mx -lt $rw -and $my -ge 0 -and $my -lt $rh) {
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $whitePen = New-Object System.Drawing.Pen([System.Drawing.Color]::White, 6)
    $redPen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(245, 239, 68, 68), 3.5)
    $redBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(230, 239, 68, 68))
    $g.DrawEllipse($whitePen, $mx - 20, $my - 20, 40, 40)
    $g.DrawEllipse($redPen, $mx - 20, $my - 20, 40, 40)
    $g.DrawLine($whitePen, $mx - 28, $my, $mx + 28, $my)
    $g.DrawLine($whitePen, $mx, $my - 28, $mx, $my + 28)
    $g.DrawLine($redPen, $mx - 26, $my, $mx + 26, $my)
    $g.DrawLine($redPen, $mx, $my - 26, $mx, $my + 26)
    $g.FillEllipse($redBrush, $mx - 5, $my - 5, 10, 10)
    $lbl = if ($p.marker.label) { "$($p.marker.label) ($($p.marker.x),$($p.marker.y))" } else { "CLIC ($($p.marker.x),$($p.marker.y))" }
    $fLbl = New-Object System.Drawing.Font('Segoe UI', 12, [System.Drawing.FontStyle]::Bold)
    $sz = $g.MeasureString($lbl, $fLbl)
    $bx = [Math]::Min([Math]::Max(6, $mx + 24), [Math]::Max(6, $rw - [int]$sz.Width - 18))
    $by = [Math]::Min([Math]::Max(6, $my - 14), [Math]::Max(6, $rh - [int]$sz.Height - 12))
    $bgBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(220, 15, 23, 42))
    $g.FillRectangle($bgBrush, $bx, $by, $sz.Width + 10, $sz.Height + 4)
    $g.DrawRectangle($redPen, $bx, $by, $sz.Width + 10, $sz.Height + 4)
    $g.DrawString($lbl, $fLbl, [System.Drawing.Brushes]::White, $bx + 5, $by + 2)
    $whitePen.Dispose(); $redPen.Dispose(); $redBrush.Dispose(); $bgBrush.Dispose(); $fLbl.Dispose()
    $markerDrawn = $true
  }
}
$g.Dispose()

$outW = $rw
$outH = $rh
if ([int]$p.maxWidth -gt 0 -and $rw -gt [int]$p.maxWidth) {
  $outW = [int]$p.maxWidth
  $outH = [Math]::Max(1, [int][Math]::Round($rh * ($outW / [double]$rw)))
  $scaled = New-Object System.Drawing.Bitmap($outW, $outH)
  $sg = [System.Drawing.Graphics]::FromImage($scaled)
  $sg.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
  $sg.DrawImage($bmp, 0, 0, $outW, $outH)
  $sg.Dispose()
  $bmp.Dispose()
  $bmp = $scaled
}

$dir = Split-Path -Parent $p.outPath
if ($dir -and -not (Test-Path $dir)) { New-Item -ItemType Directory -Force $dir | Out-Null }
if ([string]$p.fmt -eq 'jpeg') {
  $codec = [System.Drawing.Imaging.ImageCodecInfo]::GetImageEncoders() | Where-Object { $_.MimeType -eq 'image/jpeg' } | Select-Object -First 1
  $ep = New-Object System.Drawing.Imaging.EncoderParameters(1)
  $ep.Param[0] = New-Object System.Drawing.Imaging.EncoderParameter([System.Drawing.Imaging.Encoder]::Quality, [long]$p.quality)
  $bmp.Save($p.outPath, $codec, $ep)
} else {
  $bmp.Save($p.outPath, [System.Drawing.Imaging.ImageFormat]::Png)
}
$bmp.Dispose()
$pos = [System.Windows.Forms.Cursor]::Position
$fg = [FsAtgWin32]::GetForegroundWindow()
$sb = New-Object System.Text.StringBuilder 512
[void][FsAtgWin32]::GetWindowText($fg, $sb, 512)
@{
  ok = $true
  action = 'screenshot'
  path = $p.outPath
  format = [string]$p.fmt
  width = $outW
  height = $outH
  captureWidth = $rw
  captureHeight = $rh
  originX = $rx
  originY = $ry
  scaleX = [Math]::Round($rw / [double]$outW, 4)
  scaleY = [Math]::Round($rh / [double]$outH, 4)
  markerDrawn = $markerDrawn
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
        fmt,
        maxWidth,
        quality,
        marker,
        winQuery,
        rx: region?.x ?? null,
        ry: region?.y ?? null,
        rw: region?.width ?? null,
        rh: region?.height ?? null,
        si: Number.isInteger(Number(params.screenIndex)) && params.screenIndex !== null && params.screenIndex !== '' ? Number(params.screenIndex) : null,
        dn: params.deviceName ? String(params.deviceName) : null,
      });
      const bytes = existsSync(outPath) ? statSync(outPath).size : 0;
      const mime = fmt === 'jpeg' ? 'image/jpeg' : 'image/png';
      let dataUrl = null;
      if (bytes > 0 && bytes <= 1_800_000) {
        try {
          dataUrl = `data:${mime};base64,${readFileSync(outPath).toString('base64')}`;
        } catch {}
      }
      logEvent('info', 'computer', 'computer.screenshot', `Capture écran enregistrée (${res.width}x${res.height} ${fmt}, ${bytes} octets${res.markerDrawn ? ', marqueur clic' : ''}).`, { path: outPath });
      return { ...res, bytes, mimeType: mime, dataUrl };
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

$outControls = $controls.ToArray()
# Windows 11 UWP / Store fallback (ApplicationFrameHost / CalculatorApp / etc.)
if ($controls.Count -eq 0 -and ($p.query -or $winTitle)) {
  try {
    $searchKey = if ($p.query) { [string]$p.query } else { [string]$winTitle }
    $rootChildren = [System.Windows.Automation.AutomationElement]::RootElement.FindAll(
      [System.Windows.Automation.TreeScope]::Children,
      [System.Windows.Automation.Condition]::TrueCondition
    )
    $uwpRoot = $null
    foreach ($rc in $rootChildren) {
      try {
        $rn = [string]$rc.Current.Name
        if ($rn -and ($rn -like "*$searchKey*" -or $searchKey -like "*$rn*")) {
          $uwpRoot = $rc
          break
        }
      } catch {}
    }
    if ($uwpRoot) {
      $winTitle = [string]$uwpRoot.Current.Name
      try { $winPid = [int]$uwpRoot.Current.ProcessId } catch {}
      $rawWalker = [System.Windows.Automation.TreeWalker]::RawViewWalker
      $q2 = New-Object System.Collections.ArrayList
      [void]$q2.Add($uwpRoot)
      $h2 = 0
      $v2 = 0
      while ($h2 -lt $q2.Count -and $controls.Count -lt [int]$p.maxElements -and $v2 -lt 700) {
        $n2 = $q2[$h2]; $h2++; $v2++
        $ch2 = $null
        try { $ch2 = $rawWalker.GetFirstChild($n2) } catch {}
        while ($ch2 -and $v2 -lt 700 -and $controls.Count -lt [int]$p.maxElements) {
          $v2++
          [void]$q2.Add($ch2)
          try {
            $c = $ch2.Current
            $ct = ($c.ControlType.ProgrammaticName -replace '^ControlType\.', '')
            $nm = ([string]$c.Name).Trim()
            $aid = ([string]$c.AutomationId).Trim()
            $rect = $c.BoundingRectangle
            $validRect = (-not [double]::IsInfinity($rect.X)) -and (-not [double]::IsInfinity($rect.Y)) -and (-not [double]::IsInfinity($rect.Width)) -and (-not [double]::IsInfinity($rect.Height)) -and ($rect.Width -gt 1) -and ($rect.Height -gt 1)
            $isShortText = ($ct -ne 'Text') -or ($nm.Length -gt 0 -and $nm.Length -le 90)
            if ((-not $c.IsOffscreen) -and $validRect -and ($allowedTypes -contains $ct) -and $isShortText -and ($nm -or $aid)) {
              [void]$controls.Add(@{
                name = if ($nm.Length -gt 120) { $nm.Substring(0, 120) } else { $nm }
                controlType = $ct
                automationId = $aid
                x = [int][math]::Round($rect.X + $rect.Width / 2.0)
                y = [int][math]::Round($rect.Y + $rect.Height / 2.0)
                width = [int][math]::Round($rect.Width)
                height = [int][math]::Round($rect.Height)
                enabled = [bool]$c.IsEnabled
              })
            }
          } catch {}
          try { $ch2 = $rawWalker.GetNextSibling($ch2) } catch { $ch2 = $null }
        }
      }
      $visited += $v2
      $outControls = $controls.ToArray()
    }
  } catch {}
}

$outTitle = [string]$winTitle
$outPid = [int]$winPid
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
  # Windows 11 UWP / Store fallback via RootElement + RawViewWalker
  try {
    $searchKey = [string]$p.query
    $rootChildren = [System.Windows.Automation.AutomationElement]::RootElement.FindAll(
      [System.Windows.Automation.TreeScope]::Children,
      [System.Windows.Automation.Condition]::TrueCondition
    )
    $uwpRoot = $null
    if ($searchKey) {
      foreach ($rc in $rootChildren) {
        try {
          $rn = [string]$rc.Current.Name
          if ($rn -and ($rn -like "*$searchKey*" -or $searchKey -like "*$rn*")) { $uwpRoot = $rc; break }
        } catch {}
      }
    } elseif ($rootEl) {
      $uwpRoot = $rootEl
    }
    if ($uwpRoot) {
      $rawWalker = [System.Windows.Automation.TreeWalker]::RawViewWalker
      $q2 = New-Object System.Collections.ArrayList
      [void]$q2.Add($uwpRoot)
      $h2 = 0; $v2 = 0
      while ($h2 -lt $q2.Count -and $v2 -lt 700 -and -not $hit) {
        $n2 = $q2[$h2]; $h2++; $v2++
        $ch2 = $null
        try { $ch2 = $rawWalker.GetFirstChild($n2) } catch {}
        while ($ch2 -and $v2 -lt 700 -and -not $hit) {
          $v2++
          [void]$q2.Add($ch2)
          try {
            $c = $ch2.Current
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
              $hit = $ch2
              $hitMeta = @{ name = $nm; controlType = $ct; automationId = $aid; x = $cx; y = $cy }
              break
            }
          } catch {}
          try { $ch2 = $rawWalker.GetNextSibling($ch2) } catch { $ch2 = $null }
        }
      }
    }
  } catch {}
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
      const res = await runPowerShellJson(`
${WIN32_CSHARP_HEADER}
$sent = [FsAtgWin32]::TypeUnicode([string]$p.text)
@{ ok = $true; action = 'type'; length = ([string]$p.text).Length; events = $sent } | ConvertTo-Json -Compress
`, { text });
      logEvent('info', 'computer', 'computer.type', `Saisie clavier (${text.length} caractères).`);
      return res;
    }

    case 'key':
    case 'hotkey': {
      const raw = String(params.keys || params.key || '').trim();
      const spec = toVirtualKeySpec(raw, action === 'hotkey' || raw.includes('+'));
      const res = await runPowerShellJson(`
${WIN32_CSHARP_HEADER}
$vks = New-Object System.Collections.Generic.List[int]
foreach ($v in @($p.vks)) { if ($v) { [void]$vks.Add([int]$v) } }
foreach ($ch in @($p.fallbackChars)) {
  if ($ch) {
    $vk = [FsAtgWin32]::LookupVk(([string]$ch)[0])
    if ($vk -gt 0) { [void]$vks.Add([int]$vk) }
  }
}
if ($vks.Count -eq 0) {
  @{ ok = $false; error = "Aucune touche valide pour '$($p.raw)'." } | ConvertTo-Json -Compress
  exit 0
}
$sent = [FsAtgWin32]::SendVkCombo($vks.ToArray())
@{ ok = $true; action = [string]$p.act; key = [string]$p.raw; events = $sent } | ConvertTo-Json -Compress
`, { act: action, raw, vks: spec.vks, fallbackChars: spec.fallbackChars });
      logEvent('info', 'computer', `computer.${action}`, `Touche envoyée : ${raw}`);
      return res;
    }

    case 'clipboard_get': {
      return await runPowerShellJson(`
$ProgressPreference = 'SilentlyContinue'
$txt = Get-Clipboard -Raw -ErrorAction SilentlyContinue
@{ ok = $true; action = 'clipboard_get'; text = [string]$txt } | ConvertTo-Json -Compress
`);
    }

    case 'clipboard_set': {
      const text = String(params.text ?? '');
      await runPowerShellJson(`
$ProgressPreference = 'SilentlyContinue'
Set-Clipboard -Value ([string]$p.text)
@{ ok = $true; action = 'clipboard_set'; length = ([string]$p.text).Length } | ConvertTo-Json -Compress
`, { text });
      return { ok: true, action: 'clipboard_set', length: text.length };
    }

    case 'wait_for': {
      const titleQuery = String(params.windowTitle || params.title || params.processName || '').trim().toLowerCase();
      const controlName = String(params.name || '').trim().toLowerCase();
      const timeoutMs = Math.max(500, Math.min(30_000, Math.round(Number(params.timeoutMs ?? 8000) || 8000)));
      const pollMs = Math.max(200, Math.min(2000, Math.round(Number(params.pollMs ?? 400) || 400)));
      const startedAt = Date.now();
      while (Date.now() - startedAt < timeoutMs) {
        if (titleQuery && !controlName) {
          const wins = await executeComputerAction({ action: 'windows' }, opts);
          const match = (wins?.windows || []).find((w) =>
            String(w.title || '').toLowerCase().includes(titleQuery)
            || String(w.processName || '').toLowerCase().includes(titleQuery)
          );
          if (match) {
            return { ok: true, action: 'wait_for', matched: 'window', window: match, waitedMs: Date.now() - startedAt };
          }
        } else if (controlName) {
          const tree = await executeComputerAction({
            action: 'ui_tree',
            title: params.windowTitle || params.title || undefined,
            processName: params.processName || undefined,
            maxElements: 60,
          }, opts);
          const ctrl = (tree?.controls || []).find((c) =>
            String(c.name || '').toLowerCase().includes(controlName)
            || String(c.automationId || '').toLowerCase().includes(controlName)
          );
          if (ctrl) {
            return { ok: true, action: 'wait_for', matched: 'control', control: ctrl, windowTitle: tree.windowTitle, waitedMs: Date.now() - startedAt };
          }
        } else {
          // Simple passive wait
          await new Promise((r) => setTimeout(r, Math.min(timeoutMs, 1000)));
          return { ok: true, action: 'wait_for', matched: 'delay', waitedMs: Date.now() - startedAt };
        }
        await new Promise((r) => setTimeout(r, pollMs));
      }
      return {
        ok: false,
        action: 'wait_for',
        timedOut: true,
        waitedMs: Date.now() - startedAt,
        error: `Délai dépassé (${timeoutMs} ms) en attendant "${params.name || params.windowTitle || params.title || params.processName || ''}".`,
      };
    }

    case 'ocr_screen': {
      const shot = await executeComputerAction({
        action: 'screenshot',
        screenIndex: params.screenIndex,
        deviceName: params.deviceName,
        windowTitle: params.windowTitle || params.title,
        processName: params.processName,
        format: 'png',
        maxWidth: 0,
      }, opts);
      if (!shot?.ok || !shot?.path) {
        return { ok: false, action: 'ocr_screen', error: shot?.error || 'Capture préalable échouée.' };
      }
      const res = await runPowerShellJson(`
$ProgressPreference = 'SilentlyContinue'
Add-Type -AssemblyName System.Runtime.WindowsRuntime
$null = [Windows.Storage.StorageFile, Windows.Storage, ContentType=WindowsRuntime]
$null = [Windows.Graphics.Imaging.BitmapDecoder, Windows.Graphics.Imaging, ContentType=WindowsRuntime]
$null = [Windows.Media.Ocr.OcrEngine, Windows.Media.Ocr, ContentType=WindowsRuntime]
$asTaskGeneric = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
  $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation\`1'
})[0]
function Await-WinRt($op, $type) {
  $task = $asTaskGeneric.MakeGenericMethod($type).Invoke($null, @($op))
  $task.Wait()
  return $task.Result
}
try {
  $engine = [Windows.Media.Ocr.OcrEngine]::TryCreateFromUserProfileLanguages()
  if (-not $engine) {
    @{ ok = $false; error = 'Moteur OCR Windows indisponible (aucun pack de langue OCR installé).' } | ConvertTo-Json -Compress
    exit 0
  }
  $file = Await-WinRt ([Windows.Storage.StorageFile]::GetFileFromPathAsync([string]$p.path)) ([Windows.Storage.StorageFile])
  $stream = Await-WinRt ($file.OpenAsync([Windows.Storage.FileAccessMode]::Read)) ([Windows.Storage.Streams.IRandomAccessStream])
  $decoder = Await-WinRt ([Windows.Graphics.Imaging.BitmapDecoder]::CreateAsync($stream)) ([Windows.Graphics.Imaging.BitmapDecoder])
  $bitmap = Await-WinRt ($decoder.GetSoftwareBitmapAsync()) ([Windows.Graphics.Imaging.SoftwareBitmap])
  $ocrRes = Await-WinRt ($engine.RecognizeAsync($bitmap)) ([Windows.Media.Ocr.OcrResult])
  $lines = New-Object System.Collections.ArrayList
  foreach ($ln in $ocrRes.Lines) {
    $words = @($ln.Words)
    $minX = 999999; $minY = 999999; $maxX = 0; $maxY = 0
    foreach ($w in $words) {
      $r = $w.BoundingRect
      if ($r.X -lt $minX) { $minX = $r.X }
      if ($r.Y -lt $minY) { $minY = $r.Y }
      if (($r.X + $r.Width) -gt $maxX) { $maxX = $r.X + $r.Width }
      if (($r.Y + $r.Height) -gt $maxY) { $maxY = $r.Y + $r.Height }
    }
    $cx = if ($maxX -gt $minX) { [int][math]::Round(($minX + $maxX) / 2.0) + [int]$p.originX } else { $null }
    $cy = if ($maxY -gt $minY) { [int][math]::Round(($minY + $maxY) / 2.0) + [int]$p.originY } else { $null }
    [void]$lines.Add(@{ text = [string]$ln.Text; x = $cx; y = $cy })
  }
  $stream.Dispose()
  @{
    ok = $true
    action = 'ocr_screen'
    text = [string]$ocrRes.Text
    lineCount = $lines.Count
    lines = $lines.ToArray()
    screenshotPath = [string]$p.path
  } | ConvertTo-Json -Compress -Depth 4
} catch {
  @{ ok = $false; action = 'ocr_screen'; error = $_.Exception.Message } | ConvertTo-Json -Compress
}
`, { path: shot.path, originX: shot.originX ?? 0, originY: shot.originY ?? 0 }, 22_000);
      logEvent('info', 'computer', 'computer.ocr_screen', `OCR Windows (${res.lineCount ?? 0} lignes).`);
      return res;
    }

    default:
      throw new Error(`Action non supportée : ${action}`);
  }
}
