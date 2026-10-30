// usage: node scripts/run.mjs <dev|build> <engine|serve|fixture> [extra vite args]
//   engine   the static bundle with the engine wasm        -> dist/
//   serve    the workspace UI that `kerf serve` embeds     -> dist-serve/  (cd engine && KERF_UI=../apps/web/dist-serve cargo build --release)
//   fixture  UI development against canned fixtures        -> dist-fixture/ (no wasm)
// engine and serve build engine/dist/kerf.wasm with cargo first (KERF_WASM=<file> uses a prebuilt module instead).
import { spawnSync, spawn } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const web = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const repo = path.resolve(web, '..', '..');
const [mode, target = 'engine', ...rest] = process.argv.slice(2);
if (!['dev', 'build'].includes(mode) || !['engine', 'serve', 'fixture'].includes(target)) {
  console.error('usage: node scripts/run.mjs <dev|build> <engine|serve|fixture> [extra vite args]');
  process.exit(2);
}
const node = (script, args, opts = {}) => spawnSync(process.execPath, [script, ...args], { cwd: web, stdio: 'inherit', ...opts });
const must = (r) => { if (r.status !== 0) process.exit(r.status ?? 1); };

if (target !== 'fixture' && !process.env.KERF_WASM) must(node(path.join(repo, 'tools/build_wasm.mjs'), []));
must(node('scripts/copy-assets.mjs', [target === 'fixture' ? 'fixture' : 'engine']));

// Run vite's JS entry through node: `.bin/vite` is a shell script on POSIX and a `.cmd` shim on Windows (spawnSync cannot run it).
const viteBin = path.join(web, 'node_modules/vite/bin/vite.js');
const env = { ...process.env, VITE_ENGINE: target === 'fixture' ? 'fixture' : 'kerf', VITE_SERVE: target === 'serve' ? '1' : '' };
if (mode === 'build') {
  const outDir = { engine: 'dist', serve: 'dist-serve', fixture: 'dist-fixture' }[target];
  process.exit(node(viteBin, ['build', '--outDir', outDir, '--emptyOutDir', ...rest], { env }).status ?? 1);
}
spawn(process.execPath, [viteBin, '--host', '127.0.0.1', ...rest], { cwd: web, stdio: 'inherit', env });
