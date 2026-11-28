/**
 * Auto-Updater GitHub pour France Student [ATG]
 * Vérifie automatiquement les mises à jour sur NoRushJustCat/FranceStudent_Desktop (branche main)
 * et applique la mise à jour sans jamais écraser :
 * - mcp_config.json (chemins spécifiques au PC de l'utilisateur)
 * - node_modules/ (sauf réinstallation npm si package.json change)
 * - .fsatg_* (état local, clés/sessions, politiques de modèles, mémoire, undo, logs)
 */
import {
  existsSync,
  mkdirSync,
  readFileSync,
  writeFileSync,
  readdirSync,
  statSync,
  copyFileSync,
  rmSync,
} from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { syncAgentsRule } from './fs_core.mjs';
import { logEvent } from './logger.mjs';

const execFileAsync = promisify(execFile);
const __dirname = dirname(fileURLToPath(import.meta.url));

export const GITHUB_REPO_OWNER = 'NoRushJustCat';
export const GITHUB_REPO_NAME = 'FranceStudent_Desktop';
export const GITHUB_BRANCH = 'main';
const UPDATE_META_FILE = join(__dirname, '.fsatg_update_meta.json');

/** Fichiers/dossiers qui ne doivent JAMAIS être écrasés lors d'une mise à jour. */
const PROTECTED_TOP_ENTRIES = new Set([
  'mcp_config.json',
  'node_modules',
  '.git',
]);

function isProtectedEntry(name) {
  if (!name) return true;
  if (PROTECTED_TOP_ENTRIES.has(name)) return true;
  if (name.startsWith('.fsatg_')) return true;
  return false;
}

export function readLocalPluginVersion(pluginDir = __dirname) {
  try {
    const pkg = JSON.parse(readFileSync(join(pluginDir, 'package.json'), 'utf-8'));
    return String(pkg?.version || '0.0.0');
  } catch {
    return '0.0.0';
  }
}

export function readUpdateMeta() {
  try {
    if (existsSync(UPDATE_META_FILE)) {
      return JSON.parse(readFileSync(UPDATE_META_FILE, 'utf-8'));
    }
  } catch { /* ignore */ }
  return { installedSha: null, lastCheckedAt: 0, lastResult: null };
}

function writeUpdateMeta(meta) {
  try {
    writeFileSync(UPDATE_META_FILE, JSON.stringify(meta, null, 2), 'utf-8');
  } catch { /* ignore */ }
}

/** Compare deux versions semver "x.y.z" → >0 si a > b, <0 si a < b, 0 si égal. */
export function compareSemver(a, b) {
  const pa = String(a || '0.0.0').replace(/^v/i, '').split('.').map((n) => parseInt(n, 10) || 0);
  const pb = String(b || '0.0.0').replace(/^v/i, '').split('.').map((n) => parseInt(n, 10) || 0);
  for (let i = 0; i < 3; i++) {
    const diff = (pa[i] || 0) - (pb[i] || 0);
    if (diff !== 0) return diff;
  }
  return 0;
}

/**
 * Vérifie sur GitHub si une version ou un commit plus récent est disponible sur main.
 * Non bloquant, avec timeout court (8s).
 */
export async function checkForGithubUpdate({ fetchImpl = fetch, force = false } = {}) {
  const localVersion = readLocalPluginVersion();
  const meta = readUpdateMeta();
  const now = Date.now();
  if (!force && meta.lastResult && now - Number(meta.lastCheckedAt || 0) < 5 * 60_000) {
    return { ...meta.lastResult, cached: true };
  }

  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 8000);
  const headers = {
    'User-Agent': `FranceStudent-ATG/${localVersion}`,
    Accept: 'application/vnd.github+json',
  };

  try {
    const commitUrl = `https://api.github.com/repos/${GITHUB_REPO_OWNER}/${GITHUB_REPO_NAME}/commits/${GITHUB_BRANCH}`;
    const pkgUrl = `https://raw.githubusercontent.com/${GITHUB_REPO_OWNER}/${GITHUB_REPO_NAME}/${GITHUB_BRANCH}/plugins/fs-atg/package.json`;

    const [commitRes, pkgRes] = await Promise.all([
      fetchImpl(commitUrl, { headers, signal: controller.signal }),
      fetchImpl(pkgUrl, { headers: { 'User-Agent': headers['User-Agent'] }, signal: controller.signal }).catch(() => null),
    ]);

    if (!commitRes.ok) {
      throw new Error(`GitHub API HTTP ${commitRes.status}`);
    }
    const commitData = await commitRes.json();
    const remoteSha = String(commitData?.sha || '').trim();
    const shortSha = remoteSha.slice(0, 7);
    const commitMessage = String(commitData?.commit?.message || '').split('\n')[0].trim();
    const commitDate = commitData?.commit?.committer?.date || commitData?.commit?.author?.date || null;

    let remoteVersion = localVersion;
    if (pkgRes && pkgRes.ok) {
      try {
        const remotePkg = await pkgRes.json();
        if (remotePkg?.version) remoteVersion = String(remotePkg.version);
      } catch { /* ignore */ }
    }

    const versionNewer = compareSemver(remoteVersion, localVersion) > 0;
    const shaChanged = Boolean(remoteSha && meta.installedSha && remoteSha !== meta.installedSha);
    // Si jamais installé via updater (installedSha inconnu), on compare la version semver ;
    // sinon on détecte aussi tout nouveau commit sur main.
    const updateAvailable = versionNewer || shaChanged;

    const result = {
      ok: true,
      repo: `${GITHUB_REPO_OWNER}/${GITHUB_REPO_NAME}`,
      branch: GITHUB_BRANCH,
      localVersion,
      remoteVersion,
      installedSha: meta.installedSha ? String(meta.installedSha).slice(0, 7) : null,
      remoteSha: shortSha,
      fullRemoteSha: remoteSha,
      commitMessage,
      commitDate,
      updateAvailable,
      releaseUrl: `https://github.com/${GITHUB_REPO_OWNER}/${GITHUB_REPO_NAME}/commits/${GITHUB_BRANCH}`,
      checkedAt: now,
    };

    writeUpdateMeta({
      ...meta,
      installedSha: meta.installedSha || remoteSha,
      lastCheckedAt: now,
      lastResult: result,
    });
    return result;
  } catch (e) {
    const errRes = {
      ok: false,
      localVersion,
      updateAvailable: false,
      error: String(e?.message || e),
      checkedAt: now,
    };
    return errRes;
  } finally {
    clearTimeout(timer);
  }
}

function copyTreeFiltered(srcDir, dstDir, isRoot = true) {
  let copied = 0;
  mkdirSync(dstDir, { recursive: true });
  for (const entry of readdirSync(srcDir)) {
    if (isRoot && isProtectedEntry(entry)) continue;
    const srcPath = join(srcDir, entry);
    const dstPath = join(dstDir, entry);
    const st = statSync(srcPath);
    if (st.isDirectory()) {
      copied += copyTreeFiltered(srcPath, dstPath, false);
    } else if (st.isFile()) {
      copyFileSync(srcPath, dstPath);
      copied++;
    }
  }
  return copied;
}

/**
 * Télécharge l'archive ZIP de la branche main sur GitHub, met à jour les fichiers
 * du plugin France Student [ATG] en préservant mcp_config.json, node_modules et .fsatg_*.
 */
export async function applyGithubUpdate({ fetchImpl = fetch, pluginDir = __dirname } = {}) {
  const check = await checkForGithubUpdate({ fetchImpl, force: true });
  const zipUrl = `https://github.com/${GITHUB_REPO_OWNER}/${GITHUB_REPO_NAME}/archive/refs/heads/${GITHUB_BRANCH}.zip`;
  const tempBase = join(tmpdir(), `fs-atg-update-${Date.now()}`);
  const zipPath = join(tempBase, 'update.zip');
  const extractDir = join(tempBase, 'extracted');

  mkdirSync(tempBase, { recursive: true });
  try {
    const res = await fetchImpl(zipUrl, {
      headers: { 'User-Agent': `FranceStudent-ATG/${readLocalPluginVersion(pluginDir)}` },
    });
    if (!res.ok) throw new Error(`Téléchargement GitHub échoué : HTTP ${res.status}`);
    const buf = Buffer.from(await res.arrayBuffer());
    if (buf.length < 1024) throw new Error('Archive ZIP GitHub invalide ou vide.');
    writeFileSync(zipPath, buf);

    mkdirSync(extractDir, { recursive: true });
    if (process.platform === 'win32') {
      await execFileAsync('powershell.exe', [
        '-NoProfile',
        '-NonInteractive',
        '-ExecutionPolicy',
        'Bypass',
        '-Command',
        `Expand-Archive -LiteralPath '${zipPath.replace(/'/g, "''")}' -DestinationPath '${extractDir.replace(/'/g, "''")}' -Force`,
      ], { timeout: 60_000, windowsHide: true });
    } else {
      await execFileAsync('unzip', ['-qo', zipPath, '-d', extractDir], { timeout: 60_000 });
    }

    const roots = readdirSync(extractDir).filter((d) => statSync(join(extractDir, d)).isDirectory());
    if (!roots.length) throw new Error('Archive GitHub sans dossier racine.');
    const sourcePluginDir = resolve(extractDir, roots[0], 'plugins', 'fs-atg');
    if (!existsSync(join(sourcePluginDir, 'package.json'))) {
      throw new Error('Dossier plugins/fs-atg introuvable dans l’archive GitHub.');
    }

    const oldPkgRaw = existsSync(join(pluginDir, 'package.json'))
      ? readFileSync(join(pluginDir, 'package.json'), 'utf-8')
      : '';
    const newPkgRaw = readFileSync(join(sourcePluginDir, 'package.json'), 'utf-8');
    const depsChanged = (() => {
      try {
        const a = JSON.stringify(JSON.parse(oldPkgRaw).dependencies || {});
        const b = JSON.stringify(JSON.parse(newPkgRaw).dependencies || {});
        return a !== b;
      } catch {
        return true;
      }
    })();

    const filesUpdated = copyTreeFiltered(sourcePluginDir, pluginDir, true);

    let npmInstalled = false;
    if (depsChanged || !existsSync(join(pluginDir, 'node_modules'))) {
      const npmCmd = process.platform === 'win32' ? 'npm.cmd' : 'npm';
      await execFileAsync(npmCmd, ['install', '--omit=dev', '--no-audit', '--no-fund'], {
        cwd: pluginDir,
        timeout: 120_000,
        windowsHide: true,
      });
      npmInstalled = true;
    }

    try { syncAgentsRule(); } catch { /* best-effort */ }

    const newVersion = readLocalPluginVersion(pluginDir);
    const now = Date.now();
    const meta = readUpdateMeta();
    const finalResult = {
      ok: true,
      updated: true,
      localVersion: newVersion,
      remoteVersion: check.remoteVersion || newVersion,
      installedSha: check.fullRemoteSha ? check.fullRemoteSha.slice(0, 7) : null,
      remoteSha: check.remoteSha || null,
      commitMessage: check.commitMessage || '',
      filesUpdated,
      npmInstalled,
      restartRecommended: true,
      updateAvailable: false,
      checkedAt: now,
    };
    writeUpdateMeta({
      ...meta,
      installedSha: check.fullRemoteSha || meta.installedSha || null,
      lastCheckedAt: now,
      lastResult: finalResult,
    });
    logEvent('info', 'updater', 'update.applied', `Plugin mis à jour vers v${newVersion} (${filesUpdated} fichiers, commit ${check.remoteSha || 'main'}).`);
    return finalResult;
  } finally {
    try { rmSync(tempBase, { recursive: true, force: true }); } catch { /* ignore */ }
  }
}
