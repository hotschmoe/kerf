#!/usr/bin/env node
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const binary = path.resolve(process.argv[2]);
const workspace = fs.mkdtempSync(path.join(os.tmpdir(), 'kerf-cli-e2e-'));
const run = (...args) => {
  const result = spawnSync(binary, args, { cwd: workspace, encoding: 'utf8', timeout: 30000 });
  if (result.error) throw result.error;
  return result;
};
const created = run('new', 'detail.kerf.json', '--template', 'section', '--title', 'Detail');
assert.equal(created.status, 0, created.stderr);
const file = path.join(workspace, 'detail.kerf.json');
const before = fs.readFileSync(file);
const invalid = run('apply', 'detail.kerf.json', '--ops', JSON.stringify([{ op: 'update', path: 'components/missing', value: {} }]), '-w');
assert.equal(invalid.status, 1);
assert.match(invalid.stderr, /\bE_REF_UNKNOWN\b/);
assert.ok(fs.readFileSync(file).equals(before));
const edited = run('apply', 'detail.kerf.json', '--ops', JSON.stringify([{ op: 'update', path: 'components/stud', value: { length: 30 } }]), '-w');
assert.equal(edited.status, 0, edited.stderr);
assert.equal(JSON.parse(fs.readFileSync(file)).components.find((part) => part.id === 'stud').length, 30);
const checked = run('check', 'detail.kerf.json');
assert.equal(checked.status, 0, checked.stderr);
for (const format of ['svg', 'dxf', 'pdf', 'png']) {
  const out = `detail.${format}`;
  const exported = run('export', 'detail.kerf.json', '--view', 'A', '--format', format, '-o', out);
  assert.equal(exported.status, 0, exported.stderr);
  assert.ok(fs.statSync(path.join(workspace, out)).size > 0, `${format} output is empty`);
}
console.log('cli: create, reject invalid edit, apply and export passed');
