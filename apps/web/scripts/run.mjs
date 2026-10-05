// usage: node scripts/run.mjs <dev|build> <rust|zig|fixture> [extra vite args]
import { spawnSync, spawn } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const web = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const [mode, engine = 'rust', ...rest] = process.argv.slice(2);
const a = spawnSync('node', ['scripts/copy-assets.mjs', engine], { cwd: web, stdio: 'inherit' });
if (a.status !== 0) process.exit(a.status ?? 1);
const vite = path.join(web, 'node_modules/.bin/vite');
const env = { ...process.env, VITE_ENGINE: engine };
if (mode === 'build') {
  const r = spawnSync(vite, ['build', '--outDir', `dist-${engine}`, '--emptyOutDir', ...rest], { cwd: web, stdio: 'inherit', env });
  process.exit(r.status ?? 1);
} else {
  spawn(vite, ['--host', '127.0.0.1', ...rest], { cwd: web, stdio: 'inherit', env });
}
