import { execFile } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { basename, dirname, extname, isAbsolute, join, resolve } from 'node:path';
import { promisify } from 'node:util';

const execFileAsync = promisify(execFile);

export const MEDIA_ACTIONS = [
  'capabilities',
  'image_info',
  'image_edit',
  'svg_export',
  'video_info',
  'video_edit',
  'hyperframes',
];

const READ_ONLY_MEDIA_ACTIONS = new Set(['capabilities', 'image_info', 'video_info']);

function resolveMediaPath(rawPath, workspaceRoot) {
  const cleaned = String(rawPath || '').trim();
  if (!cleaned) throw new Error('Chemin de fichier requis.');
  if (isAbsolute(cleaned)) return resolve(cleaned);
  const root = String(workspaceRoot || '').trim();
  if (!root) return resolve(cleaned);
  return resolve(root, cleaned);
}

async function whichBin(name) {
  const cmd = process.platform === 'win32' ? 'where.exe' : 'which';
  try {
    const { stdout } = await execFileAsync(cmd, [name], { windowsHide: true, timeout: 4000 });
    const first = String(stdout || '').split(/\r?\n/).map((s) => s.trim()).find(Boolean);
    return first || null;
  } catch {
    return null;
  }
}

function findHeadlessBrowser() {
  if (process.platform !== 'win32') return null;
  const candidates = [
    'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
    'C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe',
    'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
    'C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe',
  ];
  for (const p of candidates) {
    if (existsSync(p)) return p;
  }
  return null;
}

async function detectCapabilities() {
  const [ffmpeg, ffprobe, npxPath] = await Promise.all([
    whichBin('ffmpeg'),
    whichBin('ffprobe'),
    whichBin(process.platform === 'win32' ? 'npx.cmd' : 'npx'),
  ]);
  const browser = findHeadlessBrowser();
  const nodeMajor = Number(String(process.versions.node || '0').split('.')[0]) || 0;
  return {
    ok: true,
    platform: process.platform,
    nativeImageEdit: process.platform === 'win32',
    nativeImageEngine: process.platform === 'win32' ? 'windows-system-drawing' : (ffmpeg ? 'ffmpeg' : null),
    ffmpeg: Boolean(ffmpeg),
    ffmpegPath: ffmpeg,
    ffprobe: Boolean(ffprobe),
    svgToPngBrowser: Boolean(browser),
    hyperframesReady: Boolean(npxPath && ffmpeg && nodeMajor >= 22),
    hyperframesPrereqs: {
      nodeVersion: process.versions.node,
      nodeOk: nodeMajor >= 22,
      npx: Boolean(npxPath),
      ffmpeg: Boolean(ffmpeg),
      installHint: !ffmpeg ? 'winget install Gyan.FFmpeg (puis redémarrer le terminal)' : null,
    },
  };
}

function parseImageHeader(filePath) {
  const st = statSync(filePath);
  const ext = extname(filePath).toLowerCase().replace(/^\./, '');
  const buf = readFileSync(filePath);
  let format = ext || 'unknown';
  let width = null;
  let height = null;

  if (buf.length >= 24 && buf[0] === 0x89 && buf[1] === 0x50 && buf[2] === 0x4E && buf[3] === 0x47) {
    format = 'png';
    width = buf.readUInt32BE(16);
    height = buf.readUInt32BE(20);
  } else if (buf.length >= 10 && buf.toString('ascii', 0, 6).startsWith('GIF8')) {
    format = 'gif';
    width = buf.readUInt16LE(6);
    height = buf.readUInt16LE(8);
  } else if (buf.length >= 26 && buf[0] === 0x42 && buf[1] === 0x4D) {
    format = 'bmp';
    width = Math.abs(buf.readInt32LE(18));
    height = Math.abs(buf.readInt32LE(22));
  } else if (buf.length >= 30 && buf.toString('ascii', 0, 4) === 'RIFF' && buf.toString('ascii', 8, 12) === 'WEBP') {
    format = 'webp';
    const chunk = buf.toString('ascii', 12, 16);
    if (chunk === 'VP8 ' && buf.length >= 30) {
      width = buf.readUInt16LE(26) & 0x3FFF;
      height = buf.readUInt16LE(28) & 0x3FFF;
    } else if (chunk === 'VP8L' && buf.length >= 25) {
      const b0 = buf[21], b1 = buf[22], b2 = buf[23], b3 = buf[24];
      width = 1 + (((b1 & 0x3F) << 8) | b0);
      height = 1 + (((b3 & 0x0F) << 10) | (b2 << 2) | ((b1 & 0xC0) >> 6));
    } else if (chunk === 'VP8X' && buf.length >= 30) {
      width = 1 + (buf[24] | (buf[25] << 8) | (buf[26] << 16));
      height = 1 + (buf[27] | (buf[28] << 8) | (buf[29] << 16));
    }
  } else if (buf.length >= 4 && buf[0] === 0xFF && buf[1] === 0xD8) {
    format = 'jpeg';
    let off = 2;
    while (off + 8 < buf.length) {
      if (buf[off] !== 0xFF) { off += 1; continue; }
      const marker = buf[off + 1];
      if (marker === 0xD9 || marker === 0xDA) break;
      const len = buf.readUInt16BE(off + 2);
      if ([0xC0, 0xC1, 0xC2, 0xC3, 0xC5, 0xC6, 0xC7, 0xC9, 0xCA, 0xCB, 0xCD, 0xCE, 0xCF].includes(marker)) {
        height = buf.readUInt16BE(off + 5);
        width = buf.readUInt16BE(off + 7);
        break;
      }
      off += 2 + Math.max(2, len);
    }
  } else if (ext === 'svg' || buf.toString('utf8', 0, Math.min(512, buf.length)).includes('<svg')) {
    format = 'svg';
    const head = buf.toString('utf8', 0, Math.min(4096, buf.length));
    const vb = head.match(/viewBox\s*=\s*["']\s*[\d.-]+[\s,]+[\d.-]+[\s,]+([\d.]+)[\s,]+([\d.]+)\s*["']/i);
    const wM = head.match(/\bwidth\s*=\s*["']\s*([\d.]+)/i);
    const hM = head.match(/\bheight\s*=\s*["']\s*([\d.]+)/i);
    width = wM ? Math.round(Number(wM[1])) : (vb ? Math.round(Number(vb[1])) : null);
    height = hM ? Math.round(Number(hM[1])) : (vb ? Math.round(Number(vb[2])) : null);
  }

  return {
    ok: true,
    path: filePath,
    fileName: basename(filePath),
    format,
    width,
    height,
    aspectRatio: width && height ? Number((width / height).toFixed(3)) : null,
    bytes: st.size,
    sizeKb: Number((st.size / 1024).toFixed(1)),
  };
}

async function editImageWindows(params, workspaceRoot) {
  const inputPath = resolveMediaPath(params.inputPath || params.path, workspaceRoot);
  if (!existsSync(inputPath)) throw new Error(`Image introuvable : ${inputPath}`);

  const fmtRaw = String(params.format || extname(params.outputPath || inputPath).replace(/^\./, '') || 'png').toLowerCase();
  const fmt = fmtRaw === 'jpeg' ? 'jpg' : fmtRaw;
  if (!['png', 'jpg', 'bmp', 'gif', 'webp'].includes(fmt)) {
    throw new Error(`Format image non supporté : ${fmt} (acceptés : png, jpg, bmp, gif, webp).`);
  }

  const outPath = params.outputPath
    ? resolveMediaPath(params.outputPath, workspaceRoot)
    : join(dirname(inputPath), `${basename(inputPath, extname(inputPath))}_edited.${fmt}`);
  mkdirSync(dirname(outPath), { recursive: true });

  // WebP output uses ffmpeg if available
  if (fmt === 'webp') {
    const ffmpeg = await whichBin('ffmpeg');
    if (!ffmpeg) throw new Error('Conversion WebP requiert FFmpeg (winget install Gyan.FFmpeg). Utilise png ou jpg sans dépendance.');
    const vf = [];
    if (params.crop) {
      const c = params.crop;
      vf.push(`crop=${Math.max(1, Math.round(c.width))}:${Math.max(1, Math.round(c.height))}:${Math.max(0, Math.round(c.x || 0))}:${Math.max(0, Math.round(c.y || 0))}`);
    }
    if (params.width || params.height) {
      const w = params.width ? Math.round(params.width) : -1;
      const h = params.height ? Math.round(params.height) : -1;
      vf.push(`scale=${w}:${h}`);
    }
    const args = ['-y', '-i', inputPath];
    if (vf.length) args.push('-vf', vf.join(','));
    if (params.quality) args.push('-q:v', String(Math.max(1, Math.min(100, Math.round(params.quality)))));
    args.push(outPath);
    await execFileAsync(ffmpeg, args, { windowsHide: true, timeout: 60_000 });
    return { ...parseImageHeader(outPath), engine: 'ffmpeg', inputPath, outputPath: outPath };
  }

  if (process.platform !== 'win32') {
    throw new Error('Édition image native disponible sur Windows ; installe FFmpeg sur cet OS.');
  }

  const psCfg = JSON.stringify({
    inPath: inputPath,
    outPath,
    width: params.width ? Math.max(1, Math.round(Number(params.width))) : 0,
    height: params.height ? Math.max(1, Math.round(Number(params.height))) : 0,
    keepAspect: params.keepAspect !== false,
    rotate: [90, 180, 270].includes(Number(params.rotate)) ? Number(params.rotate) : 0,
    crop: params.crop ? {
      x: Math.max(0, Math.round(Number(params.crop.x || 0))),
      y: Math.max(0, Math.round(Number(params.crop.y || 0))),
      w: Math.max(1, Math.round(Number(params.crop.width || 1))),
      h: Math.max(1, Math.round(Number(params.crop.height || 1))),
    } : null,
    format: fmt,
    quality: Math.max(10, Math.min(100, Math.round(Number(params.quality || 90)))),
  });

  const psScript = `
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$cfg = $env:FS_MEDIA_CFG | ConvertFrom-Json
$src = [System.Drawing.Image]::FromFile($cfg.inPath)
$bmp = New-Object System.Drawing.Bitmap($src)
$src.Dispose()
try {
  if ($cfg.rotate -eq 90) { $bmp.RotateFlip([System.Drawing.RotateFlipType]::Rotate90FlipNone) }
  elseif ($cfg.rotate -eq 180) { $bmp.RotateFlip([System.Drawing.RotateFlipType]::Rotate180FlipNone) }
  elseif ($cfg.rotate -eq 270) { $bmp.RotateFlip([System.Drawing.RotateFlipType]::Rotate270FlipNone) }

  if ($null -ne $cfg.crop) {
    $cx = [Math]::Min([int]$cfg.crop.x, [Math]::Max(0, $bmp.Width - 1))
    $cy = [Math]::Min([int]$cfg.crop.y, [Math]::Max(0, $bmp.Height - 1))
    $cw = [Math]::Min([int]$cfg.crop.w, $bmp.Width - $cx)
    $ch = [Math]::Min([int]$cfg.crop.h, $bmp.Height - $cy)
    $rect = New-Object System.Drawing.Rectangle($cx, $cy, [Math]::Max(1, $cw), [Math]::Max(1, $ch))
    $cropped = $bmp.Clone($rect, $bmp.PixelFormat)
    $bmp.Dispose()
    $bmp = $cropped
  }

  $tw = [int]$cfg.width
  $th = [int]$cfg.height
  if ($tw -gt 0 -or $th -gt 0) {
    if ($tw -gt 0 -and $th -eq 0) { $th = [Math]::Max(1, [int][Math]::Round($bmp.Height * ($tw / [double]$bmp.Width))) }
    elseif ($th -gt 0 -and $tw -eq 0) { $tw = [Math]::Max(1, [int][Math]::Round($bmp.Width * ($th / [double]$bmp.Height))) }
    elseif ($cfg.keepAspect) {
      $r = [Math]::Min($tw / [double]$bmp.Width, $th / [double]$bmp.Height)
      $tw = [Math]::Max(1, [int][Math]::Round($bmp.Width * $r))
      $th = [Math]::Max(1, [int][Math]::Round($bmp.Height * $r))
    }
    $resized = New-Object System.Drawing.Bitmap($tw, $th)
    $g = [System.Drawing.Graphics]::FromImage($resized)
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.DrawImage($bmp, 0, 0, $tw, $th)
    $g.Dispose()
    $bmp.Dispose()
    $bmp = $resized
  }

  if ($cfg.format -eq 'jpg') {
    $codec = [System.Drawing.Imaging.ImageCodecInfo]::GetImageEncoders() | Where-Object { $_.MimeType -eq 'image/jpeg' } | Select-Object -First 1
    $ep = New-Object System.Drawing.Imaging.EncoderParameters(1)
    $ep.Param[0] = New-Object System.Drawing.Imaging.EncoderParameter([System.Drawing.Imaging.Encoder]::Quality, [long]$cfg.quality)
    $bmp.Save($cfg.outPath, $codec, $ep)
  } elseif ($cfg.format -eq 'bmp') {
    $bmp.Save($cfg.outPath, [System.Drawing.Imaging.ImageFormat]::Bmp)
  } elseif ($cfg.format -eq 'gif') {
    $bmp.Save($cfg.outPath, [System.Drawing.Imaging.ImageFormat]::Gif)
  } else {
    $bmp.Save($cfg.outPath, [System.Drawing.Imaging.ImageFormat]::Png)
  }
  Write-Output (@{ ok = $true; width = $bmp.Width; height = $bmp.Height } | ConvertTo-Json -Compress)
} finally {
  if ($bmp) { $bmp.Dispose() }
}
`;

  await execFileAsync('powershell.exe', ['-NoProfile', '-OutputFormat', 'Text', '-Command', psScript], {
    windowsHide: true,
    timeout: 30_000,
    env: { ...process.env, FS_MEDIA_CFG: psCfg },
  });

  return { ...parseImageHeader(outPath), engine: 'windows-system-drawing', inputPath, outputPath: outPath };
}

async function exportSvg(params, workspaceRoot) {
  const svgContent = String(params.svg || params.content || '').trim();
  if (!svgContent || !svgContent.includes('<svg')) {
    throw new Error('Contenu SVG valide requis (balise <svg> manquante).');
  }
  const svgPath = resolveMediaPath(params.outputPath || params.path || 'graphic.svg', workspaceRoot);
  const finalSvgPath = svgPath.toLowerCase().endsWith('.svg') ? svgPath : `${svgPath}.svg`;
  mkdirSync(dirname(finalSvgPath), { recursive: true });
  writeFileSync(finalSvgPath, svgContent, 'utf8');

  const result = {
    ok: true,
    svgPath: finalSvgPath,
    fileName: basename(finalSvgPath),
    bytes: Buffer.byteLength(svgContent, 'utf8'),
    pngPath: null,
    pngEngine: null,
  };

  if (params.renderPng) {
    const pngPath = finalSvgPath.replace(/\.svg$/i, '.png');
    const w = Math.max(64, Math.min(4096, Math.round(Number(params.width || 1280))));
    const h = Math.max(64, Math.min(4096, Math.round(Number(params.height || 720))));
    const browser = findHeadlessBrowser();
    if (browser) {
      const fileUrl = `file:///${finalSvgPath.replace(/\\/g, '/')}`;
      await execFileAsync(browser, [
        '--headless=new',
        '--disable-gpu',
        '--hide-scrollbars',
        `--window-size=${w},${h}`,
        `--screenshot=${pngPath}`,
        fileUrl,
      ], { windowsHide: true, timeout: 25_000 });
      if (existsSync(pngPath)) {
        result.pngPath = pngPath;
        result.pngEngine = basename(browser);
        result.pngInfo = parseImageHeader(pngPath);
      }
    } else {
      result.pngNote = 'Navigateur Edge/Chrome introuvable pour le rendu PNG ; fichier SVG enregistré.';
    }
  }
  return result;
}

async function inspectVideo(params, workspaceRoot) {
  const inputPath = resolveMediaPath(params.inputPath || params.path, workspaceRoot);
  if (!existsSync(inputPath)) throw new Error(`Média introuvable : ${inputPath}`);
  const ffprobe = await whichBin('ffprobe');
  if (!ffprobe) {
    return {
      ok: false,
      error: 'ffprobe introuvable sur ce PC. Installe FFmpeg via `winget install Gyan.FFmpeg` puis redémarre.',
      path: inputPath,
      bytes: statSync(inputPath).size,
    };
  }
  const { stdout } = await execFileAsync(ffprobe, [
    '-v', 'quiet',
    '-print_format', 'json',
    '-show_format',
    '-show_streams',
    inputPath,
  ], { windowsHide: true, timeout: 20_000 });
  const data = JSON.parse(stdout || '{}');
  const vStream = (data.streams || []).find((s) => s.codec_type === 'video');
  const aStream = (data.streams || []).find((s) => s.codec_type === 'audio');
  return {
    ok: true,
    path: inputPath,
    fileName: basename(inputPath),
    durationSec: data.format?.duration ? Number(Number(data.format.duration).toFixed(2)) : null,
    sizeMb: data.format?.size ? Number((Number(data.format.size) / (1024 * 1024)).toFixed(2)) : null,
    format: data.format?.format_name || extname(inputPath).slice(1),
    video: vStream ? {
      codec: vStream.codec_name,
      width: vStream.width,
      height: vStream.height,
      fps: vStream.r_frame_rate,
    } : null,
    audio: aStream ? {
      codec: aStream.codec_name,
      sampleRate: aStream.sample_rate,
      channels: aStream.channels,
    } : null,
  };
}

async function editVideo(params, workspaceRoot) {
  const ffmpeg = await whichBin('ffmpeg');
  if (!ffmpeg) {
    throw new Error('FFmpeg requis pour éditer des vidéos localement. Installe-le avec : winget install Gyan.FFmpeg');
  }
  const inputPath = resolveMediaPath(params.inputPath || params.path, workspaceRoot);
  if (!existsSync(inputPath)) throw new Error(`Vidéo introuvable : ${inputPath}`);

  const sub = String(params.subAction || 'clip').toLowerCase();
  const baseName = basename(inputPath, extname(inputPath));
  const outDir = dirname(inputPath);

  if (sub === 'frames') {
    const framesDir = params.outputPath
      ? resolveMediaPath(params.outputPath, workspaceRoot)
      : join(outDir, `${baseName}_frames`);
    mkdirSync(framesDir, { recursive: true });
    const fps = Math.max(0.1, Math.min(10, Number(params.fps || 1)));
    const maxFrames = Math.max(1, Math.min(30, Math.round(Number(params.maxFrames || 6))));
    const pattern = join(framesDir, 'frame_%03d.png');
    await execFileAsync(ffmpeg, [
      '-y', '-i', inputPath,
      '-vf', `fps=${fps}`,
      '-vframes', String(maxFrames),
      pattern,
    ], { windowsHide: true, timeout: 90_000 });
    return { ok: true, subAction: sub, outputDir: framesDir, maxFrames, fps };
  }

  if (sub === 'audio_extract') {
    const fmt = ['mp3', 'wav', 'm4a'].includes(String(params.format || '').toLowerCase())
      ? String(params.format).toLowerCase()
      : 'mp3';
    const outPath = params.outputPath ? resolveMediaPath(params.outputPath, workspaceRoot) : join(outDir, `${baseName}.${fmt}`);
    mkdirSync(dirname(outPath), { recursive: true });
    await execFileAsync(ffmpeg, ['-y', '-i', inputPath, '-vn', outPath], { windowsHide: true, timeout: 120_000 });
    return { ok: true, subAction: sub, outputPath: outPath, bytes: statSync(outPath).size };
  }

  if (sub === 'normalize_audio') {
    const outPath = params.outputPath ? resolveMediaPath(params.outputPath, workspaceRoot) : join(outDir, `${baseName}_norm${extname(inputPath) || '.mp4'}`);
    mkdirSync(dirname(outPath), { recursive: true });
    await execFileAsync(ffmpeg, ['-y', '-i', inputPath, '-c:v', 'copy', '-af', 'loudnorm=I=-16:TP=-1.5:LRA=11', outPath], { windowsHide: true, timeout: 180_000 });
    return { ok: true, subAction: sub, outputPath: outPath, bytes: statSync(outPath).size };
  }

  if (sub === 'reframe_9_16') {
    const outPath = params.outputPath ? resolveMediaPath(params.outputPath, workspaceRoot) : join(outDir, `${baseName}_9x16.mp4`);
    mkdirSync(dirname(outPath), { recursive: true });
    await execFileAsync(ffmpeg, [
      '-y', '-i', inputPath,
      '-vf', 'crop=ih*9/16:ih,scale=1080:1920',
      '-c:a', 'copy',
      outPath,
    ], { windowsHide: true, timeout: 180_000 });
    return { ok: true, subAction: sub, outputPath: outPath, bytes: statSync(outPath).size };
  }

  // clip or convert
  const ext = ['mp4', 'webm', 'gif'].includes(String(params.format || '').toLowerCase())
    ? String(params.format).toLowerCase()
    : 'mp4';
  const outPath = params.outputPath ? resolveMediaPath(params.outputPath, workspaceRoot) : join(outDir, `${baseName}_${sub}.${ext}`);
  mkdirSync(dirname(outPath), { recursive: true });
  const args = ['-y'];
  if (params.start !== undefined && params.start !== '') args.push('-ss', String(params.start));
  args.push('-i', inputPath);
  if (params.duration !== undefined && params.duration !== '') args.push('-t', String(params.duration));
  if (ext === 'gif') {
    args.push('-vf', `fps=${Math.max(5, Math.min(20, Number(params.fps || 12)))},scale=${Math.max(160, Math.min(960, Number(params.width || 480)))}:-1:flags=lanczos`);
  }
  args.push(outPath);
  await execFileAsync(ffmpeg, args, { windowsHide: true, timeout: 180_000 });
  return { ok: true, subAction: sub, outputPath: outPath, bytes: statSync(outPath).size };
}

async function runHyperframes(params, workspaceRoot) {
  const sub = String(params.subAction || 'init').toLowerCase();
  if (!['init', 'render', 'normalize-audio'].includes(sub)) {
    throw new Error('subAction HyperFrames invalide (attendu : init, render ou normalize-audio).');
  }
  const caps = await detectCapabilities();
  if (!caps.hyperframesPrereqs.npx) {
    throw new Error('npx introuvable (Node.js requis pour HyperFrames).');
  }
  if (sub !== 'init' && !caps.ffmpeg) {
    throw new Error('FFmpeg requis pour le rendu HyperFrames. Installe-le avec : winget install Gyan.FFmpeg');
  }

  const projectDir = resolveMediaPath(params.projectDir || params.path || 'hyperframes-video', workspaceRoot);
  mkdirSync(projectDir, { recursive: true });
  const npxBin = process.platform === 'win32' ? 'npx.cmd' : 'npx';
  const args = ['-y', 'hyperframes', sub];

  if (sub === 'render') {
    const outFile = params.outputPath ? resolveMediaPath(params.outputPath, workspaceRoot) : join(projectDir, 'output.mp4');
    args.push('-o', outFile);
  } else if (sub === 'normalize-audio' && params.inputPath) {
    args.push(resolveMediaPath(params.inputPath, workspaceRoot));
  }

  const { stdout, stderr } = await execFileAsync(npxBin, args, {
    cwd: projectDir,
    windowsHide: true,
    timeout: 180_000,
    maxBuffer: 4 * 1024 * 1024,
  });
  return {
    ok: true,
    engine: 'hyperframes',
    subAction: sub,
    projectDir,
    stdout: String(stdout || '').slice(0, 4000),
    stderr: String(stderr || '').slice(0, 2000),
  };
}

export async function executeMediaAction(params = {}, {
  permissionMode = 'acceptEdits',
  mediaToolsEnabled = true,
  workspaceRoot = '',
} = {}) {
  if (!mediaToolsEnabled) {
    throw new Error('Média & Vidéo (fs_media) désactivé dans les réglages France Student.');
  }
  const action = String(params?.action || 'capabilities').toLowerCase();
  if (!MEDIA_ACTIONS.includes(action)) {
    throw new Error(`Action média inconnue : ${action}. Valeurs : ${MEDIA_ACTIONS.join(', ')}.`);
  }
  if (permissionMode === 'plan' && !READ_ONLY_MEDIA_ACTIONS.has(action)) {
    throw new Error(`Action "${action}" bloquée en mode "plan" (lecture seule).`);
  }

  switch (action) {
    case 'capabilities':
      return await detectCapabilities();
    case 'image_info': {
      const p = resolveMediaPath(params.inputPath || params.path, workspaceRoot);
      if (!existsSync(p)) throw new Error(`Image introuvable : ${p}`);
      return parseImageHeader(p);
    }
    case 'image_edit':
      return await editImageWindows(params, workspaceRoot);
    case 'svg_export':
      return await exportSvg(params, workspaceRoot);
    case 'video_info':
      return await inspectVideo(params, workspaceRoot);
    case 'video_edit':
      return await editVideo(params, workspaceRoot);
    case 'hyperframes':
      return await runHyperframes(params, workspaceRoot);
    default:
      throw new Error(`Action non gérée : ${action}`);
  }
}
