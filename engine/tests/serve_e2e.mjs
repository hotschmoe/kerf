#!/usr/bin/env node
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const binary = path.resolve(process.argv[2]);
const workspace = fs.mkdtempSync(path.join(os.tmpdir(), 'kerf-serve-e2e-'));
fs.copyFileSync(path.join(here, '../../spec/details/flush-beam-strap.kerf.json'), path.join(workspace, 'beam.kerf.json'));
const server = spawn(binary, ['serve', '--dir', workspace, '--port', '0', '--no-token'], { stdio: ['ignore', 'pipe', 'pipe'] });
let output = '';
server.stderr.on('data', (data) => { output += data; });

try {
  const port = await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`server did not start: ${output}`)), 15000);
    server.once('error', (err) => { clearTimeout(timer); reject(err); });
    server.once('exit', (code) => { clearTimeout(timer); reject(new Error(`server exited ${code}: ${output}`)); });
    server.stdout.on('data', (data) => {
      output += data;
      const match = /http:\/\/[\d.]+:(\d+)\//.exec(output);
      if (match) { clearTimeout(timer); resolve(match[1]); }
    });
  });
  const request = (url, options = {}) => fetch(`http://127.0.0.1:${port}${url}`, { signal: AbortSignal.timeout(15000), ...options });
  const info = await request('/api/info');
  assert.equal(info.status, 200);
  const created = await request('/api/docs', {
    method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ file: 'example', title: 'Example' }),
  });
  assert.equal(created.status, 201);
  const document = await request('/api/docs/example.kerf.json');
  assert.equal(document.status, 200);
  const etag = document.headers.get('etag');
  assert.ok(etag);
  const ops = [{ op: 'add', path: 'components', value: { id: 'stud', type: 'lumber', size: '2x4', at: { x: 0, y: 0 } } }];
  const apply = (if_match) => request('/api/docs/example.kerf.json/apply', {
    method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ ops, why: 'add stud', actor: 'designer', if_match }),
  });
  const edited = await apply(etag);
  assert.equal(edited.status, 200);
  assert.equal((await edited.json()).ok, true);
  assert.ok(JSON.parse(fs.readFileSync(path.join(workspace, 'example.kerf.json'))).components.some((part) => part.id === 'stud'));
  const stale = await apply(etag);
  assert.equal(stale.status, 409);
  assert.equal((await stale.json()).error.code, 'E_CONFLICT');
  const pdf = await request('/api/docs/beam.kerf.json/export?view=A&format=pdf');
  assert.equal(pdf.status, 200);
  assert.ok(Buffer.from(await pdf.arrayBuffer()).subarray(0, 5).equals(Buffer.from('%PDF-')));
  console.log('serve: create, edit, conflict and export passed');
} finally {
  server.kill();
}
