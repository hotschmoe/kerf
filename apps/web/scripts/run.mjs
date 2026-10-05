// usage: node scripts/run.mjs <dev|build> <zig|fixture|serve> [extra vite args]
// `serve` = the zig engine bundle written to dist-serve/ (what `kerf serve` embeds: -Dui=../../apps/web/dist-serve)
import { spawnSync, spawn } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const web = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const [mode, target = 'zig', ...rest] = process.argv.slice(2);
const engine = target === 'serve' ? 'zig' : target;
const a = spawnSync('node', ['scripts/copy-assets.mjs', engine], { cwd: web, stdio: 'inherit' });
if (a.status !== 0) process.exit(a.status ?? 1);
const vite = path.join(web, 'node_modules/.bin/vite');
const env = { ...process.env, VITE_ENGINE: engine, VITE_SERVE: target === 'serve' ? '1' : '' };
if (mode === 'build') {
  const r = spawnSync(vite, ['build', '--outDir', `dist-${target}`, '--emptyOutDir', ...rest], { cwd: web, stdio: 'inherit', env });
  process.exit(r.status ?? 1);
} else {
  spawn(vite, ['--host', '127.0.0.1', ...rest], { cwd: web, stdio: 'inherit', env });
}
