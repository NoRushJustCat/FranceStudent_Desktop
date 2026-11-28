import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import {
  parseComputerActionJson,
  mapImageCoordToDesktop,
  runAutonomousComputerLoop,
} from '../fs_core.mjs';
import { COMPUTER_ACTIONS, executeComputerAction } from '../computer_use.mjs';
import { compareSemver, checkForGithubUpdate } from '../updater.mjs';
import { executeMediaAction } from '../media_tools.mjs';
import { GIT_ACTIONS, executeGitAction } from '../git_tools.mjs';

test('COMPUTER_ACTIONS inclut wait_for et ocr_screen et bloque les écritures en mode plan', async () => {
  assert.ok(COMPUTER_ACTIONS.includes('wait_for'));
  assert.ok(COMPUTER_ACTIONS.includes('ocr_screen'));
  await assert.rejects(
    () => executeComputerAction({ action: 'click', x: 10, y: 10 }, { permissionMode: 'plan' }),
    /Lecture seule \(plan\)/i,
  );
});

test('parseComputerActionJson extrait les actions JSON fenced, inline et TERMINÉ', () => {
  const a1 = parseComputerActionJson('Je vois Deezer en bas à gauche.\n```json\n{"action":"click","x":120,"y":680,"coordSpace":"image"}\n```');
  assert.deepEqual(a1, { action: 'click', x: 120, y: 680, coordSpace: 'image' });

  const a2 = parseComputerActionJson('Je lance le raccourci {"action":"hotkey","keys":"ctrl+esc"} maintenant.');
  assert.deepEqual(a2, { action: 'hotkey', keys: 'ctrl+esc' });

  const a3 = parseComputerActionJson('TERMINÉ — Deezer est ouvert.');
  assert.equal(a3.action, 'done');
});

test('mapImageCoordToDesktop convertit correctement les coordonnées image redimensionnée vers pixels bureau', () => {
  const shotMeta = {
    width: 1280,
    height: 720,
    captureWidth: 2560,
    captureHeight: 1440,
    originX: 1920,
    originY: 0,
    scaleX: 2,
    scaleY: 2,
  };
  const mapped = mapImageCoordToDesktop(100, 200, shotMeta, 'image');
  assert.deepEqual(mapped, { x: 2120, y: 400 });

  const abs = mapImageCoordToDesktop(2120, 400, shotMeta, 'desktop');
  assert.deepEqual(abs, { x: 2120, y: 400 });
});

test('runAutonomousComputerLoop réutilise 1 seule discussion et dessine le viseur rouge après chaque clic', async () => {
  const shotCalls = [];
  const clickCalls = [];
  const streamChatIds = [];
  let turn = 0;

  const res = await runAutonomousComputerLoop({
    state: { computerUseEnabled: true },
    goal: 'Ouvre Deezer',
    model: 'gpt-6.1-terra',
    maxSteps: 4,
    executeComputerAction: async (params) => {
      if (params.action === 'windows') {
        return { ok: true, windows: [{ processName: 'explorer', title: 'Bureau' }] };
      }
      if (params.action === 'screenshot') {
        shotCalls.push(params);
        return {
          ok: true,
          path: 'C:\\fake\\shot.jpg',
          width: 1280,
          height: 720,
          captureWidth: 1920,
          captureHeight: 1080,
          originX: 0,
          originY: 0,
          scaleX: 1.5,
          scaleY: 1.5,
          bytes: 64_000,
          markerDrawn: Boolean(params.marker),
        };
      }
      if (params.action === 'click') {
        clickCalls.push(params);
        return { ok: true, action: 'click', cursor: { x: params.x, y: params.y } };
      }
      return { ok: true };
    },
    uploadFile: async () => `file-${shotCalls.length}`,
    streamTurn: async (req) => {
      turn++;
      streamChatIds.push(req.chatId ?? null);
      if (turn === 1) {
        return {
          chatId: 'chat-deezer-123',
          model: 'gpt-6.1-terra',
          text: 'Icône Deezer visible en bas à gauche.\n{"action":"click","x":100,"y":600,"coordSpace":"image"}',
        };
      }
      assert.match(req.prompt, /viseur ROUGE/i);
      return {
        chatId: 'chat-deezer-123',
        model: 'gpt-6.1-terra',
        text: 'Le clic a bien ouvert Deezer.\n{"action":"done","summary":"Deezer est ouvert."}',
      };
    },
  });

  assert.equal(res.ok, true);
  assert.equal(res.finished, true);
  assert.equal(res.chatId, 'chat-deezer-123');
  assert.equal(res.stepsCount, 2);
  // 1ère étape : nouveau chat, 2ème étape : même chatId réutilisé
  assert.deepEqual(streamChatIds, [null, 'chat-deezer-123']);
  // 1ère capture sans marqueur JPEG 1280px, 2ème capture avec marqueur rouge aux coordonnées bureau mappées (150, 900)
  assert.equal(shotCalls.length, 2);
  assert.equal(shotCalls[0].format, 'jpeg');
  assert.equal(shotCalls[0].maxWidth, 1280);
  assert.equal(shotCalls[0].marker, undefined);
  assert.deepEqual(clickCalls[0], { action: 'click', x: 150, y: 900 });
  assert.equal(shotCalls[1].marker?.x, 150);
  assert.equal(shotCalls[1].marker?.y, 900);
});

test('runAutonomousComputerLoop s’arrête proprement sur action sensible (confirm)', async () => {
  const res = await runAutonomousComputerLoop({
    state: { computerUseEnabled: true },
    goal: 'Valide le paiement',
    maxSteps: 3,
    executeComputerAction: async (params) => {
      if (params.action === 'windows') return { ok: true, windows: [] };
      if (params.action === 'screenshot') return { ok: true, path: 'C:\\fake\\shot.jpg', width: 1280, height: 720, bytes: 50_000 };
      throw new Error('Ne doit pas exécuter une action sensible');
    },
    uploadFile: async () => 'file-1',
    streamTurn: async () => ({
      chatId: 'chat-safe',
      model: 'gpt-6.1-terra',
      text: '{"action":"confirm","reason":"Paiement bancaire détecté"}',
    }),
  });
  assert.equal(res.ok, false);
  assert.equal(res.needsConfirmation, true);
  assert.match(res.confirmationReason, /Paiement/i);
});

test('compareSemver & checkForGithubUpdate détectent une mise à jour GitHub', async () => {
  assert.equal(compareSemver('2.7.0', '2.6.1') > 0, true);
  assert.equal(compareSemver('2.6.1', '2.6.1'), 0);
  assert.equal(compareSemver('2.5.0', '2.6.1') < 0, true);

  const mockFetch = async (url) => {
    if (String(url).includes('/commits/')) {
      return {
        ok: true,
        status: 200,
        json: async () => ({
          sha: 'abcdef1234567890abcdef1234567890abcdef12',
          commit: { message: 'feat: nouvelle version', committer: { date: '2026-11-28T12:00:00Z' } },
        }),
      };
    }
    return {
      ok: true,
      status: 200,
      json: async () => ({ version: '9.9.9' }),
    };
  };
  const check = await checkForGithubUpdate({ fetchImpl: mockFetch, force: true });
  assert.equal(check.ok, true);
  assert.equal(check.updateAvailable, true);
  assert.equal(check.remoteVersion, '9.9.9');
  assert.equal(check.remoteSha, 'abcdef1');
});

test('fs_media canvas_to_hyperframes génère un projet HyperFrames prêt au rendu', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'fs-atg-hf-'));
  try {
    const res = await executeMediaAction({
      action: 'canvas_to_hyperframes',
      html: '<h1>Hello Video</h1>',
      title: 'Intro Demo',
      projectDir: dir,
      width: 1280,
      height: 720,
      fps: 30,
      duration: 4,
    });
    assert.equal(res.ok, true);
    assert.ok(existsSync(res.indexPath));
    assert.ok(existsSync(res.manifestPath));
    const manifest = JSON.parse(readFileSync(res.manifestPath, 'utf-8'));
    assert.equal(manifest.name, 'Intro Demo');
    assert.equal(manifest.fps, 30);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('fs_git supporte stash_list en lecture seule', async () => {
  assert.ok(GIT_ACTIONS.includes('stash_list'));
  const res = await executeGitAction({ action: 'stash_list', workspaceRoot: process.cwd() });
  assert.equal(res.ok, true);
  assert.equal(res.action, 'stash_list');
  assert.ok(Array.isArray(res.entries));
});
