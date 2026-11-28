// Télécharge `cloudflared` dans src-tauri/binaries/ sous le nom attendu par Tauri
// (« sidecar » : cloudflared-<triple>[.exe]). Idempotent et vérifié.
// Lancé par `npm install` (postinstall), avant `npm run dev` et `npm run build`.
//
// La version est épinglée et l'empreinte SHA-256 est lue dans
// `scripts/cloudflared.sha256`. Un binaire qui ne correspond pas est refusé :
// aucune mise à jour silencieuse en release.
//
// Variables (développement uniquement) :
//   CLOUDFLARED_VERSION  version à télécharger (désactive la vérification si ≠ épinglée)
//   CLOUDFLARED_FORCE=1  re-télécharge même si le fichier existe
import { execSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const PINNED_VERSION = "2026.11.0";
const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const dir = join(root, "src-tauri", "binaries");

// Cible : fournie par le CLI Tauri (build croisé), sinon l'hôte de rustc.
function targetTriple() {
  if (process.env.TAURI_ENV_TARGET_TRIPLE) return process.env.TAURI_ENV_TARGET_TRIPLE;
  const out = execSync("rustc -vV", { encoding: "utf8" });
  const m = out.match(/host:\s*(\S+)/);
  if (!m) throw new Error("impossible de déterminer la cible (rustc -vV)");
  return m[1];
}

const ASSETS = {
  "x86_64-pc-windows-msvc": "cloudflared-windows-amd64.exe",
  "x86_64-pc-windows-gnu": "cloudflared-windows-amd64.exe",
  "aarch64-pc-windows-msvc": "cloudflared-windows-amd64.exe", // pas de build arm64 : émulation x64
  "i686-pc-windows-msvc": "cloudflared-windows-386.exe",
  "i686-pc-windows-gnu": "cloudflared-windows-386.exe",
  "x86_64-unknown-linux-gnu": "cloudflared-linux-amd64",
  "aarch64-unknown-linux-gnu": "cloudflared-linux-arm64",
  "x86_64-apple-darwin": "cloudflared-darwin-amd64.tgz",
  "aarch64-apple-darwin": "cloudflared-darwin-arm64.tgz",
};

const triple = targetTriple();
const asset = ASSETS[triple];
if (!asset) {
  console.error(`[cloudflared] cible non gérée : ${triple}`);
  process.exit(1);
}

const version = (process.env.CLOUDFLARED_VERSION || PINNED_VERSION).trim();
const pinned = version === PINNED_VERSION;
const isWin = triple.includes("windows");
const dest = join(dir, `cloudflared-${triple}${isWin ? ".exe" : ""}`);
const noticePath = join(dir, "CLOUDFLARED_NOTICE.txt");
if (!existsSync(noticePath)) {
  mkdirSync(dir, { recursive: true });
  writeFileSync(
    noticePath,
    "Cloudflare Tunnel (cloudflared)\nCopyright (c) Cloudflare, Inc.\nLicensed under the Apache License, Version 2.0 (https://www.apache.org/licenses/LICENSE-2.0).\nOfficial releases: https://github.com/cloudflare/cloudflared/releases\n",
    "utf8",
  );
}

function sha256(buf) {
  return createHash("sha256").update(buf).digest("hex").toLowerCase();
}

let hashes = null;
function expectedHash() {
  if (!pinned) return null;
  if (hashes) return hashes.get(asset) ?? null;
  hashes = new Map();
  try {
    const text = readFileSync(join(root, "scripts", "cloudflared.sha256"), "utf8");
    for (const line of text.split(/\r?\n/)) {
      if (!line || line.trimStart().startsWith("#")) continue;
      const m = line.trim().match(/^([0-9a-fA-F]{64})\s+\*?(\S+)$/);
      if (m) hashes.set(m[2], m[1].toLowerCase());
    }
  } catch (e) {
    console.error(`[cloudflared] scripts/cloudflared.sha256 illisible : ${e.message}`);
  }
  return hashes.get(asset) ?? null;
}

function verify(buf, label) {
  const want = expectedHash();
  if (!want) {
    console.warn(
      pinned
        ? `[cloudflared] aucune empreinte pour ${asset} : téléchargement NON vérifié.`
        : `[cloudflared] version ${version} ≠ ${PINNED_VERSION} : vérification désactivée (dev).`,
    );
    return;
  }
  const got = sha256(buf);
  if (got !== want) {
    throw new Error(
      `${label} : SHA-256 inattendu (${got}, attendu ${want}). Refus d'utiliser ce binaire.`,
    );
  }
}

if (existsSync(dest) && !process.env.CLOUDFLARED_FORCE) {
  try {
    verify(readFileSync(dest), "binaire existant");
    console.log(`[cloudflared] déjà présent et vérifié : ${dest}`);
    process.exit(0);
  } catch (e) {
    console.error(`[cloudflared] ${e.message}`);
    console.error(`[cloudflared] supprime le fichier ou lance CLOUDFLARED_FORCE=1 pour le remplacer : ${dest}`);
    process.exit(1);
  }
}

const base = `https://github.com/cloudflare/cloudflared/releases/download/${version}`;
const url = `${base}/${asset}`;

console.log(`[cloudflared] téléchargement ${url}`);
mkdirSync(dir, { recursive: true });
let data = null;
try {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  data = Buffer.from(await res.arrayBuffer());
  verify(data, url);
} catch (e) {
  const localWin = "C:\\Program Files (x86)\\cloudflared\\cloudflared.exe";
  if (isWin && existsSync(localWin)) {
    data = readFileSync(localWin);
    try {
      verify(data, "installation locale");
    } catch (e2) {
      console.error(`[cloudflared] ${e2.message}`);
      process.exit(1);
    }
    copyFileSync(localWin, dest);
    console.log(`[cloudflared] copié depuis installation locale : ${dest}`);
    process.exit(0);
  }
  try {
    const tmpDl = join(dir, asset);
    execSync(`curl.exe -fsSL -L "${url}" -o "${tmpDl}"`, { stdio: "inherit" });
    data = readFileSync(tmpDl);
    verify(data, url);
    rmSync(tmpDl, { force: true });
  } catch (e2) {
    console.error(`[cloudflared] échec du téléchargement : ${e.message}`);
    console.error(`[cloudflared] ${e2.message}`);
    console.error(`[cloudflared] télécharge-le à la main et place-le ici : ${dest}`);
    process.exit(1);
  }
}

if (asset.endsWith(".tgz")) {
  // macOS : archive contenant un unique binaire « cloudflared ».
  const tmp = join(dir, "cloudflared.tgz");
  writeFileSync(tmp, data);
  execSync(`tar -xzf "${tmp}" -C "${dir}"`);
  rmSync(tmp, { force: true });
  renameSync(join(dir, "cloudflared"), dest);
} else {
  writeFileSync(dest, data);
}
if (!isWin) chmodSync(dest, 0o755);

console.log(
  `[cloudflared] installé : ${dest} (${(data.length / 1048576).toFixed(1)} Mo, ${sha256(data).slice(0, 12)}…)`,
);
