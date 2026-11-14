// Télécharge cloudflared dans src-tauri/binaries/ sous le nom attendu par Tauri
// (« sidecar » : cloudflared-<triple>[.exe]). Idempotent : ne fait rien si le fichier existe.
// Lancé par `npm install` (postinstall) et avant `npm run dev` / `npm run build`.
//
// Variables optionnelles :
//   CLOUDFLARED_VERSION  version à télécharger (ex. 2025.8.1), défaut : la dernière
//   CLOUDFLARED_FORCE=1  re-télécharge même si le fichier existe
import { execSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

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
  "aarch64-pc-windows-msvc": "cloudflared-windows-amd64.exe", // pas de build arm64 : émulation x64
  "i686-pc-windows-msvc": "cloudflared-windows-386.exe",
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

const isWin = triple.includes("windows");
const dest = join(dir, `cloudflared-${triple}${isWin ? ".exe" : ""}`);

if (existsSync(dest) && !process.env.CLOUDFLARED_FORCE) {
  console.log(`[cloudflared] déjà présent : ${dest}`);
  process.exit(0);
}

const v = process.env.CLOUDFLARED_VERSION;
const base = v
  ? `https://github.com/cloudflare/cloudflared/releases/download/${v}`
  : "https://github.com/cloudflare/cloudflared/releases/latest/download";
const url = `${base}/${asset}`;

console.log(`[cloudflared] téléchargement ${url}`);
let data;
try {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  data = Buffer.from(await res.arrayBuffer());
} catch (e) {
  console.error(`[cloudflared] échec du téléchargement : ${e.message}`);
  console.error(`[cloudflared] télécharge-le à la main et place-le ici : ${dest}`);
  process.exit(1);
}

mkdirSync(dir, { recursive: true });

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

console.log(`[cloudflared] installé : ${dest} (${(data.length / 1048576).toFixed(1)} Mo)`);
