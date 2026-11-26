import { execFile, spawn } from 'node:child_process';
import { existsSync, readFileSync, statSync } from 'node:fs';
import { extname, isAbsolute, resolve } from 'node:path';
import { promisify } from 'node:util';
import { logEvent } from './logger.mjs';

const execFileAsync = promisify(execFile);
const MAX_FILE_BYTES = 25 * 1024 * 1024;

function runPsOfficeExtract(absPath, ext) {
  return new Promise((resolvePromise, reject) => {
    const b64Payload = Buffer.from(JSON.stringify({ path: absPath, ext }), 'utf8').toString('base64');
    const script = `
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$ErrorActionPreference = 'Stop'
$p = ([System.Text.Encoding]::UTF8.GetString([System.Convert]::FromBase64String('${b64Payload}'))) | ConvertFrom-Json
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [System.IO.Compression.ZipFile]::OpenRead($p.path)
try {
  $readXml = {
    param($entryName)
    $e = $zip.GetEntry($entryName)
    if (-not $e) { return $null }
    $sr = New-Object System.IO.StreamReader($e.Open(), [System.Text.Encoding]::UTF8)
    try { return [xml]$sr.ReadToEnd() } finally { $sr.Dispose() }
  }

  $out = New-Object System.Text.StringBuilder
  if ($p.ext -eq '.docx') {
    $xml = & $readXml 'word/document.xml'
    if (-not $xml) { throw 'word/document.xml introuvable.' }
    $ns = New-Object System.Xml.XmlNamespaceManager($xml.NameTable)
    $ns.AddNamespace('w', 'http://schemas.openxmlformats.org/wordprocessingml/2006/main')
    $paras = $xml.SelectNodes('//w:body/w:p', $ns)
    if (-not $paras -or $paras.Count -eq 0) { $paras = $xml.SelectNodes('//w:p', $ns) }
    foreach ($para in $paras) {
      $style = $para.SelectSingleNode('w:pPr/w:pStyle/@w:val', $ns)
      $texts = @($para.SelectNodes('.//w:t', $ns) | ForEach-Object { $_.InnerText })
      $line = ($texts -join '').Trim()
      if (-not $line) { continue }
      $sval = if ($style) { [string]$style.Value } else { '' }
      if ($sval -match '^Heading([1-6])$|^Titre([1-6])$') {
        $lvl = [int]$Matches[1]
        if (-not $lvl) { $lvl = [int]$Matches[2] }
        [void]$out.AppendLine(("#" * [math]::Min(6, [math]::Max(1, $lvl))) + " " + $line)
        [void]$out.AppendLine()
      } elseif ($sval -match 'Title') {
        [void]$out.AppendLine("# " + $line)
        [void]$out.AppendLine()
      } else {
        [void]$out.AppendLine($line)
        [void]$out.AppendLine()
      }
    }
  } elseif ($p.ext -eq '.pptx') {
    $slides = @($zip.Entries | Where-Object { $_.FullName -match '^ppt/slides/slide(\\d+)\\.xml$' } | Sort-Object { [int]([regex]::Match($_.FullName, 'slide(\\d+)\\.xml$').Groups[1].Value) })
    foreach ($s in $slides) {
      $num = [regex]::Match($s.FullName, 'slide(\\d+)\\.xml$').Groups[1].Value
      $xml = & $readXml $s.FullName
      $ns = New-Object System.Xml.XmlNamespaceManager($xml.NameTable)
      $ns.AddNamespace('a', 'http://schemas.openxmlformats.org/drawingml/2006/main')
      $paras = @($xml.SelectNodes('//a:p', $ns) | ForEach-Object {
        (@($_.SelectNodes('.//a:t', $ns) | ForEach-Object { $_.InnerText }) -join '').Trim()
      } | Where-Object { $_ })
      [void]$out.AppendLine("## Diapo $num")
      foreach ($pl in $paras) { [void]$out.AppendLine("- $pl") }
      [void]$out.AppendLine()
    }
  } elseif ($p.ext -eq '.xlsx') {
    $shared = @()
    $sstXml = & $readXml 'xl/sharedStrings.xml'
    if ($sstXml) {
      foreach ($si in $sstXml.sst.si) {
        if ($si.t) { $shared += [string]$si.t }
        elseif ($si.r) { $shared += (@($si.r | ForEach-Object { [string]$_.t }) -join '') }
        else { $shared += [string]$si.InnerText }
      }
    }
    $sheets = @($zip.Entries | Where-Object { $_.FullName -match '^xl/worksheets/sheet(\\d+)\\.xml$' } | Sort-Object { [int]([regex]::Match($_.FullName, 'sheet(\\d+)\\.xml$').Groups[1].Value) })
    foreach ($sh in $sheets) {
      $snum = [regex]::Match($sh.FullName, 'sheet(\\d+)\\.xml$').Groups[1].Value
      $sxml = & $readXml $sh.FullName
      [void]$out.AppendLine("## Feuille $snum")
      $rowIdx = 0
      foreach ($row in $sxml.worksheet.sheetData.row) {
        $vals = @()
        foreach ($cell in $row.c) {
          $v = [string]$cell.v
          if ($cell.t -eq 's' -and $v -match '^\\d+$') {
            $idx = [int]$v
            $v = if ($idx -ge 0 -and $idx -lt $shared.Count) { $shared[$idx] } else { '' }
          } elseif ($cell.t -eq 'inlineStr' -and $cell.is.t) {
            $v = [string]$cell.is.t
          }
          $vals += ($v -replace '\\|', '\\\\|' -replace '\\r?\\n', ' ').Trim()
        }
        if (($vals -join '') -eq '') { continue }
        [void]$out.AppendLine('| ' + ($vals -join ' | ') + ' |')
        if ($rowIdx -eq 0) {
          [void]$out.AppendLine('| ' + ((@($vals | ForEach-Object { '---' })) -join ' | ') + ' |')
        }
        $rowIdx++
        if ($rowIdx -ge 200) { break }
      }
      [void]$out.AppendLine()
    }
  }
  @{ ok = $true; content = $out.ToString().Trim() } | ConvertTo-Json -Compress
} finally {
  $zip.Dispose()
}
`;
    const encoded = Buffer.from(script, 'utf16le').toString('base64');
    const child = spawn('powershell.exe', [
      '-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
      '-EncodedCommand', encoded,
    ], { stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true });

    let stdout = '';
    let stderr = '';
    const timer = setTimeout(() => {
      try { child.kill(); } catch {}
      reject(new Error('Timeout extraction document Office.'));
    }, 15_000);

    child.stdout.setEncoding('utf8');
    child.stderr.setEncoding('utf8');
    child.stdout.on('data', (d) => { stdout += d; });
    child.stderr.on('data', (d) => { stderr += d; });
    child.on('error', (err) => { clearTimeout(timer); reject(err); });
    child.on('close', (code) => {
      clearTimeout(timer);
      const line = stdout.trim().split(/\r?\n/).filter(Boolean).at(-1) || '';
      try {
        const parsed = JSON.parse(line);
        if (parsed?.ok) resolvePromise(parsed.content || '');
        else reject(new Error(parsed?.error || 'Erreur extraction Office.'));
      } catch {
        reject(new Error((stderr || stdout || `Exit ${code}`).trim().slice(0, 300)));
      }
    });
  });
}

function csvToMarkdown(text, sep = ',') {
  const lines = String(text || '').replace(/\r\n/g, '\n').split('\n').filter((l) => l.trim().length > 0).slice(0, 200);
  if (!lines.length) return '';
  const parseLine = (line) => {
    const out = [];
    let cur = '';
    let inQuotes = false;
    for (let i = 0; i < line.length; i++) {
      const ch = line[i];
      if (ch === '"') {
        if (inQuotes && line[i + 1] === '"') { cur += '"'; i++; }
        else inQuotes = !inQuotes;
      } else if (ch === sep && !inQuotes) {
        out.push(cur.replace(/\|/g, '\\|').trim());
        cur = '';
      } else {
        cur += ch;
      }
    }
    out.push(cur.replace(/\|/g, '\\|').trim());
    return out;
  };
  const rows = lines.map(parseLine);
  const head = `| ${rows[0].join(' | ')} |`;
  const rule = `| ${rows[0].map(() => '---').join(' | ')} |`;
  const body = rows.slice(1).map((r) => `| ${r.join(' | ')} |`);
  return [head, rule, ...body].join('\n');
}

function stripHtmlBasic(html) {
  return String(html || '')
    .replace(/<(script|style|noscript|svg)\b[^>]*>[\s\S]*?<\/\1>/gi, '')
    .replace(/<h([1-6])\b[^>]*>([\s\S]*?)<\/h\1>/gi, (_, l, t) => `\n\n${'#'.repeat(Number(l))} ${t.replace(/<[^>]+>/g, '').trim()}\n\n`)
    .replace(/<li\b[^>]*>([\s\S]*?)<\/li>/gi, (_, t) => `\n- ${t.replace(/<[^>]+>/g, '').trim()}`)
    .replace(/<br\s*\/?>/gi, '\n')
    .replace(/<\/(?:p|div|section|tr)>/gi, '\n\n')
    .replace(/<[^>]+>/g, '')
    .replace(/&nbsp;/gi, ' ')
    .replace(/&amp;/gi, '&')
    .replace(/&lt;/gi, '<')
    .replace(/&gt;/gi, '>')
    .replace(/\n{3,}/g, '\n\n')
    .trim();
}

async function tryPdfExtract(absPath) {
  try {
    const { stdout } = await execFileAsync('markitdown', [absPath], { windowsHide: true, timeout: 20_000, maxBuffer: 4 * 1024 * 1024 });
    if (stdout?.trim()) return { engine: 'markitdown-cli', content: stdout.trim() };
  } catch { /* markitdown not installed */ }

  const pyScript = `import sys, json
p = sys.argv[1]
try:
    import pypdf
    r = pypdf.PdfReader(p)
    txt = "\\n\\n".join((pg.extract_text() or "") for pg in r.pages[:60])
    print(json.dumps({"ok": True, "engine": "pypdf", "content": txt}))
    sys.exit(0)
except Exception:
    pass
try:
    import fitz
    doc = fitz.open(p)
    txt = "\\n\\n".join(doc[i].get_text() for i in range(min(len(doc), 60)))
    print(json.dumps({"ok": True, "engine": "pymupdf", "content": txt}))
    sys.exit(0)
except Exception:
    pass
print(json.dumps({"ok": False}))`;

  for (const pyCmd of ['python', 'py']) {
    try {
      const { stdout } = await execFileAsync(pyCmd, ['-c', pyScript, absPath], { windowsHide: true, timeout: 15_000, maxBuffer: 4 * 1024 * 1024 });
      const parsed = JSON.parse(stdout.trim().split(/\r?\n/).at(-1) || '{}');
      if (parsed.ok && parsed.content !== undefined) {
        return { engine: parsed.engine, content: String(parsed.content).trim() };
      }
    } catch { /* try next */ }
  }
  return null;
}

/** Reads Markdown/text/CSV/HTML/Office (.docx/.pptx/.xlsx) or PDF into clean Markdown without external deps. */
export async function executeDocRead({
  path,
  workspaceRoot = '',
  maxChars = 16_000,
} = {}) {
  const rawPath = String(path || '').trim();
  if (!rawPath) throw new Error('Paramètre "path" requis.');
  const absPath = isAbsolute(rawPath) ? resolve(rawPath) : resolve(workspaceRoot || process.cwd(), rawPath);
  if (!existsSync(absPath)) {
    throw new Error(`Fichier introuvable : ${absPath}`);
  }
  const st = statSync(absPath);
  if (!st.isFile()) {
    throw new Error(`Le chemin n'est pas un fichier : ${absPath}`);
  }
  if (st.size > MAX_FILE_BYTES) {
    throw new Error(`Fichier trop volumineux (${Math.round(st.size / (1024 * 1024))} Mo > 25 Mo).`);
  }

  const ext = extname(absPath).toLowerCase();
  const boundedMax = Math.max(500, Math.min(80_000, Math.round(Number(maxChars) || 16_000)));
  let engine = 'native-text';
  let content = '';

  if (['.md', '.txt', '.log', '.json', '.yaml', '.yml', '.xml', '.ini', '.toml', '.mjs', '.js', '.ts', '.py', '.rs', '.svelte'].includes(ext)) {
    content = readFileSync(absPath, 'utf-8');
    if (ext === '.json') {
      try { content = JSON.stringify(JSON.parse(content), null, 2); } catch {}
    }
  } else if (ext === '.csv' || ext === '.tsv') {
    engine = 'native-csv';
    content = csvToMarkdown(readFileSync(absPath, 'utf-8'), ext === '.tsv' ? '\t' : ',');
  } else if (ext === '.html' || ext === '.htm') {
    engine = 'native-html';
    content = stripHtmlBasic(readFileSync(absPath, 'utf-8'));
  } else if (['.docx', '.pptx', '.xlsx'].includes(ext)) {
    engine = `native-ooxml-${ext.slice(1)}`;
    content = await runPsOfficeExtract(absPath, ext);
  } else if (ext === '.pdf') {
    const pdfRes = await tryPdfExtract(absPath);
    if (!pdfRes) {
      return {
        ok: false,
        path: absPath,
        format: 'pdf',
        error: 'Aucun extracteur PDF installé localement. Pour lire des PDF, installe markitdown : pip install markitdown-mcp',
      };
    }
    engine = pdfRes.engine;
    content = pdfRes.content;
  } else {
    throw new Error(`Format non supporté par fs_doc_read : "${ext || '(sans extension)'}".`);
  }

  const totalChars = content.length;
  const truncated = totalChars > boundedMax;
  const finalContent = truncated ? `${content.slice(0, boundedMax)}\n\n… [tronqué à ${boundedMax} / ${totalChars} caractères]` : content;

  logEvent('info', 'mcp', 'doc_read.ok', `Document lu (${ext || 'txt'}, ${engine}, ${totalChars} car.).`, { path: absPath, engine });
  return {
    ok: true,
    path: absPath,
    format: ext.replace(/^\./, '') || 'text',
    engine,
    content: finalContent,
    totalChars,
    truncated,
  };
}
