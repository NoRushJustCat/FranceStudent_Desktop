import { execFile } from 'node:child_process';
import { existsSync } from 'node:fs';
import { resolve } from 'node:path';
import { promisify } from 'node:util';
import { logEvent } from './logger.mjs';

const execFileAsync = promisify(execFile);

export const GIT_ACTIONS = Object.freeze(['status', 'diff', 'log', 'branch', 'show', 'stash_list']);

function validateRef(ref) {
  const clean = String(ref || 'HEAD').trim() || 'HEAD';
  if (clean.startsWith('-')) {
    throw new Error(`Référence Git refusée : "${clean}"`);
  }
  if (!/^[a-zA-Z0-9._~^/@{}-]+$/.test(clean)) {
    throw new Error(`Caractères non autorisés dans la référence Git : "${clean}"`);
  }
  return clean;
}

function validateFilePath(p) {
  const clean = String(p || '').trim();
  if (!clean) return '';
  if (clean.startsWith('-') || clean.includes('\0')) {
    throw new Error(`Chemin Git invalide : "${clean}"`);
  }
  return clean;
}

async function runGit(args, cwd) {
  const { stdout, stderr } = await execFileAsync('git', args, {
    cwd,
    windowsHide: true,
    timeout: 15_000,
    maxBuffer: 4 * 1024 * 1024,
  });
  return { stdout: String(stdout || ''), stderr: String(stderr || '') };
}

/** Read-only Git inspection helper (status, diff, log, branch, show). */
export async function executeGitAction({
  action = 'status',
  workspaceRoot = '',
  staged = false,
  limit = 15,
  ref = 'HEAD',
  path = '',
  maxChars = 14_000,
} = {}) {
  const act = String(action || 'status').trim().toLowerCase();
  if (!GIT_ACTIONS.includes(act)) {
    throw new Error(`Action fs_git invalide : "${act}". Actions autorisées : ${GIT_ACTIONS.join(', ')}`);
  }
  const ws = workspaceRoot ? resolve(workspaceRoot) : process.cwd();
  if (!existsSync(ws)) {
    throw new Error(`Dossier workspace introuvable : ${ws}`);
  }
  const boundedMax = Math.max(500, Math.min(80_000, Math.round(Number(maxChars) || 14_000)));
  const truncate = (txt) => {
    const s = String(txt || '');
    const cut = s.length > boundedMax;
    return {
      text: cut ? `${s.slice(0, boundedMax)}\n\n… [tronqué à ${boundedMax} / ${s.length} caractères]` : s,
      totalChars: s.length,
      truncated: cut,
    };
  };

  switch (act) {
    case 'status': {
      const { stdout } = await runGit(['status', '--porcelain=v1', '-b'], ws);
      const lines = stdout.replace(/\r\n/g, '\n').split('\n').filter(Boolean);
      const branchLine = lines[0]?.startsWith('## ') ? lines[0].slice(3).trim() : '';
      const files = (branchLine ? lines.slice(1) : lines).map((line) => ({
        status: line.slice(0, 2).trim(),
        path: line.slice(3).trim(),
      }));
      logEvent('info', 'mcp', 'git.status', `git status (${files.length} fichier(s) modifié(s)).`, { workspaceRoot: ws });
      return {
        ok: true,
        action: 'status',
        workspaceRoot: ws,
        branch: branchLine,
        clean: files.length === 0,
        files,
        raw: truncate(stdout.trim()).text,
      };
    }

    case 'diff': {
      const args = ['diff', '--no-color'];
      if (staged) args.push('--staged');
      const targetPath = validateFilePath(path);
      if (targetPath) args.push('--', targetPath);
      const { stdout } = await runGit(args, ws);
      const t = truncate(stdout.trim());
      return {
        ok: true,
        action: 'diff',
        workspaceRoot: ws,
        staged: Boolean(staged),
        path: targetPath || null,
        diff: t.text,
        totalChars: t.totalChars,
        truncated: t.truncated,
      };
    }

    case 'log': {
      const n = Math.max(1, Math.min(50, Math.round(Number(limit) || 15)));
      const { stdout } = await runGit(['log', '-n', String(n), '--pretty=format:%h|%ad|%an|%s', '--date=short'], ws);
      const commits = stdout
        .replace(/\r\n/g, '\n')
        .split('\n')
        .filter(Boolean)
        .map((line) => {
          const [hash, date, author, ...rest] = line.split('|');
          return { hash, date, author, subject: rest.join('|') };
        });
      return {
        ok: true,
        action: 'log',
        workspaceRoot: ws,
        count: commits.length,
        commits,
      };
    }

    case 'branch': {
      const { stdout } = await runGit(['branch', '-a', '--no-color'], ws);
      const lines = stdout.replace(/\r\n/g, '\n').split('\n').map((l) => l.trimEnd()).filter(Boolean);
      let current = '';
      const branches = lines.map((l) => {
        if (l.startsWith('* ')) {
          const name = l.slice(2).trim();
          current = name;
          return name;
        }
        return l.trim();
      });
      return {
        ok: true,
        action: 'branch',
        workspaceRoot: ws,
        current,
        branches,
      };
    }

    case 'show': {
      const safeRef = validateRef(ref);
      const args = ['show', '--stat', '--no-color', safeRef];
      const targetPath = validateFilePath(path);
      if (targetPath) args.push('--', targetPath);
      const { stdout } = await runGit(args, ws);
      const t = truncate(stdout.trim());
      return {
        ok: true,
        action: 'show',
        workspaceRoot: ws,
        ref: safeRef,
        content: t.text,
        totalChars: t.totalChars,
        truncated: t.truncated,
      };
    }

    case 'stash_list': {
      const { stdout } = await runGit(['stash', 'list', '--date=short'], ws);
      const entries = stdout
        .replace(/\r\n/g, '\n')
        .split('\n')
        .filter(Boolean)
        .slice(0, 30);
      return {
        ok: true,
        action: 'stash_list',
        workspaceRoot: ws,
        count: entries.length,
        entries,
      };
    }

    default:
      throw new Error(`Action fs_git inconnue : ${act}`);
  }
}
