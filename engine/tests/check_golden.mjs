#!/usr/bin/env node
// Cross-platform twin of check_golden.sh (used on Windows, where there is no bash): regenerates every export into a
// temp folder, compares byte for byte with tests/golden, and runs the DXF / PDF checkers when python is found.
//   KERF=path/to/kerf[.exe] node tests/check_golden.mjs
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, '..', '..');
const K = path.resolve(process.env.KERF ?? path.join(here, '..', 'target', 'release', process.platform === 'win32' ? 'kerf.exe' : 'kerf'));
const goldenRoot = path.join(here, 'golden');
const docs = [
  ...fs.readdirSync(path.join(root, 'spec', 'details')).filter((f) => f.endsWith('.kerf.json')).map((f) => path.join(root, 'spec', 'details', f)),
  ...fs.readdirSync(path.join(here, 'docs')).filter((f) => f.endsWith('.kerf.json')).map((f) => path.join(here, 'docs', f)),
];
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'kerf-golden-'));
const py = process.env.PYTHON ?? (process.platform === 'win32' ? 'python' : 'python3');
const havePy = spawnSync(py, ['--version']).status === 0;
let fail = 0, compared = 0;
const kerf = (...a) => { const r = spawnSync(K, a, { encoding: 'utf8' }); return r; };
for (const doc of docs) {
  const name = path.basename(doc, '.kerf.json');
  const g = path.join(goldenRoot, name);
  const views = JSON.parse(fs.readFileSync(doc, 'utf8')).views.map((v) => v.id);
  for (const v of views) {
    const jobs = [
      [`drawing-${v}.json`, ['drawing', doc, '--view', v]],
      [`${v}.svg`, ['export', doc, '--view', v, '--format', 'svg']],
      [`${v}-sheet.svg`, ['export', doc, '--view', v, '--format', 'svg', '--sheet']],
      [`${v}.dxf`, ['export', doc, '--view', v, '--format', 'dxf']],
      [`${v}.pdf`, ['export', doc, '--view', v, '--format', 'pdf']],
      [`${v}.raster.png`, ['export', doc, '--view', v, '--format', 'png']],
      [`${v}-sheet.raster.png`, ['export', doc, '--view', v, '--format', 'png', '--sheet']],
    ];
    for (const [f, args] of jobs) {
      const out = path.join(tmp, f);
      const r = kerf(...args, '-o', out);
      if (r.status !== 0) { console.log(`RUN FAIL ${name}/${f}: ${r.stderr}`); fail++; continue; }
      compared++;
      if (!fs.readFileSync(out).equals(fs.readFileSync(path.join(g, f)))) { console.log(`DIFF ${name}/${f}`); fail++; }
    }
    if (havePy) {
      const d = spawnSync(py, ['-I', path.join(root, 'tools', 'dxf_check.py'), path.join(tmp, `${v}.dxf`)], { encoding: 'utf8' });
      if (d.status !== 0) { console.log(`DXF AUDIT FAIL ${name}/${v}`); fail++; }
      const p = spawnSync(py, ['-I', path.join(root, 'tools', 'pdf_check.py'), path.join(tmp, `${v}.pdf`)], { encoding: 'utf8' });
      if (!p.stdout.includes('vector=True')) { console.log(`PDF NOT VECTOR ${name}/${v}`); fail++; }
    }
  }
  const mesh = path.join(tmp, 'mesh.json');
  kerf('mesh', doc, '-o', mesh);
  compared++;
  if (!fs.readFileSync(mesh).equals(fs.readFileSync(path.join(g, 'mesh.json')))) { console.log(`DIFF ${name}/mesh.json`); fail++; }
  const c = kerf('check', doc);
  const summary = Buffer.from(c.stdout + c.stderr);
  compared++;
  if (!summary.equals(fs.readFileSync(path.join(g, 'summary.txt')))) { console.log(`DIFF ${name}/summary.txt`); fail++; }
  if (!summary.toString().includes(' 0 errors')) { console.log(`CHECK ERRORS ${name}`); fail++; }
}
fs.rmSync(tmp, { recursive: true, force: true });
console.log(`compared ${compared} outputs${havePy ? '' : ' (no python: DXF/PDF checkers skipped)'}`);
console.log(fail === 0 ? 'golden: OK' : `golden: FAILED (${fail})`);
process.exit(fail === 0 ? 0 : 1);
