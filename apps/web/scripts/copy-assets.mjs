// Copies runtime assets into apps/web/public (git-ignored): samples, style, stroke font and the engine wasm (or the fixtures).
// usage: node scripts/copy-assets.mjs <engine|fixture>   (KERF_WASM=<file> overrides engine/dist/kerf.wasm)
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const web = path.resolve(here, '..');
const repo = path.resolve(web, '../..');
const pub = path.join(web, 'public');
const target = process.argv[2] || 'engine';

fs.mkdirSync(path.join(pub, 'samples'), { recursive: true });
const samples = [];
for (const f of fs.readdirSync(path.join(repo, 'spec/details')).sort()) {
  if (!f.endsWith('.kerf.json')) continue;
  fs.copyFileSync(path.join(repo, 'spec/details', f), path.join(pub, 'samples', f));
  const d = JSON.parse(fs.readFileSync(path.join(repo, 'spec/details', f), 'utf8'));
  samples.push({ file: f, id: d.id, title: d.title ?? d.id });
}
fs.writeFileSync(path.join(pub, 'samples/index.json'), JSON.stringify(samples));
fs.copyFileSync(path.join(repo, 'spec/styles/kerf-standard.kerfstyle.json'), path.join(pub, 'style.json'));
fs.copyFileSync(path.join(repo, 'spec/fonts/kerf-simplex.json'), path.join(pub, 'kerf-simplex.json'));

if (target === 'fixture') {
  fs.mkdirSync(path.join(pub, 'fixtures'), { recursive: true });
  for (const f of ['drawing-A.json', 'mesh.json', 'sheet-A.svg', 'echo.wasm']) fs.copyFileSync(path.join(web, 'test/fixtures', f), path.join(pub, 'fixtures', f));
  fs.rmSync(path.join(pub, 'kerf.wasm'), { force: true });
} else {
  fs.rmSync(path.join(pub, 'fixtures'), { recursive: true, force: true });
  const src = process.env.KERF_WASM || path.join(repo, 'engine', 'dist', 'kerf.wasm');
  if (!fs.existsSync(src)) {
    console.error(`copy-assets: ${src} not found. Build it with: node tools/build_wasm.mjs`);
    process.exit(1);
  }
  fs.copyFileSync(src, path.join(pub, 'kerf.wasm'));
  console.log(`copy-assets: kerf.wasm ${fs.statSync(src).size} bytes`);
}
