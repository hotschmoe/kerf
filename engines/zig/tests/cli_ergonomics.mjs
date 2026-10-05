#!/usr/bin/env node
// CLI tests for the v0.1.3 agent ergonomics (SPEC 19): apply errors on stderr, --dry-run, schema,
// new --template, call help, guide embedding, W_UNKNOWN_KEY, acknowledge, lints. Runs the real binary.
//
//   node tests/cli_ergonomics.mjs [path/to/kerf]      (default: zig-out/bin/kerf; run `zig build` first)
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const KERF = path.resolve(process.argv[2] ?? path.join(here, '..', 'zig-out', 'bin', 'kerf'));
if (!fs.existsSync(KERF)) { console.error('kerf binary not found: ' + KERF + ' (zig build first)'); process.exit(2); }
const DETAILS = path.join(here, '..', '..', '..', 'spec', 'details');
const DOCS = path.join(here, 'docs');

let passed = 0, failed = 0;
function check(name, cond, extra) {
  if (cond) { passed++; console.log('  ok   ' + name); } else { failed++; console.log('  FAIL ' + name + (extra !== undefined ? '  -> ' + (typeof extra === 'string' ? extra : JSON.stringify(extra)) : '')); }
}
const section = (t) => console.log('\n# ' + t);

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'kerf-cli-'));
process.on('exit', () => { try { fs.rmSync(tmp, { recursive: true, force: true }); } catch {} });

function kerf(args, opts = {}) {
  const r = spawnSync(KERF, args, { cwd: tmp, encoding: 'utf8', input: opts.input ?? '', env: { ...process.env, KERF_ACTOR: 'test' } });
  return { code: r.status, out: r.stdout, err: r.stderr };
}
const write = (name, text) => { const p = path.join(tmp, name); fs.writeFileSync(p, typeof text === 'string' ? text : JSON.stringify(text, null, 2)); return p; };
const readJson = (name) => JSON.parse(fs.readFileSync(path.join(tmp, name), 'utf8'));

// ---------------------------------------------------------------------------------------------------
section('1. failed apply prints the errors (stderr), says nothing written, exits 1');
{
  kerf(['new', 'a.kerf.json', '--title', 'T']);
  const before = fs.readFileSync(path.join(tmp, 'a.kerf.json'), 'utf8');
  const bad = [{ op: 'update', path: 'components/nope', value: { size: '2x6' } }];
  let r = kerf(['apply', 'a.kerf.json', '--ops', JSON.stringify(bad), '-w', '--why', 'x']);
  check('exit 1', r.code === 1, r);
  check('ERROR line: code, path, message', /^ERROR E_REF_UNKNOWN components\/nope: .*no component 'nope'/m.test(r.err), r.err);
  check('says nothing written', /^nothing written$/m.test(r.err), r.err);
  check('document untouched', fs.readFileSync(path.join(tmp, 'a.kerf.json'), 'utf8') === before);
  check('no log line for a failed apply', !fs.existsSync(path.join(tmp, 'a.kerf.json.log.jsonl')) || !/nope/.test(fs.readFileSync(path.join(tmp, 'a.kerf.json.log.jsonl'), 'utf8')));

  // validation error with a Fix hint (bad component param)
  const bad2 = [{ op: 'add', path: 'components', value: { id: 'Bad-Id', type: 'lumber', size: '2x4' } }];
  r = kerf(['apply', 'a.kerf.json', '-'], { input: JSON.stringify(bad2) });
  check('validation error printed with Fix:', r.code === 1 && /^ERROR E_PARAM .*invalid.*  Fix: rename it/m.test(r.err), r.err);

  // several errors are all listed
  const bad3 = [{ op: 'add', path: 'components', value: { id: 'p', type: 'panel', thickness: 'x', length: 'y' } }];
  r = kerf(['apply', 'a.kerf.json', '--ops', JSON.stringify(bad3)]);
  check('every error listed', (r.err.match(/^ERROR /gm) || []).length >= 2, r.err);

  // not-JSON ops
  r = kerf(['apply', 'a.kerf.json', '--ops', '[{op: add}]']);
  check('malformed ops JSON: ERROR E_JSON + nothing written', r.code === 1 && /^ERROR E_JSON ops:/m.test(r.err) && /nothing written/.test(r.err), r.err);

  // --dry-run: summary, nothing written; contradiction with -w
  const good = [{ op: 'add', path: 'components', value: { id: 'sill', type: 'lumber', size: '2x6', orient: 'flat', treated: true } }];
  r = kerf(['apply', 'a.kerf.json', '--ops', JSON.stringify(good), '--dry-run']);
  check('--dry-run exits 0, prints the summary, writes nothing', r.code === 0 && /DOC a /.test(r.out) && /dry run: nothing written/.test(r.out) && fs.readFileSync(path.join(tmp, 'a.kerf.json'), 'utf8') === before, r);
  r = kerf(['apply', 'a.kerf.json', '--ops', JSON.stringify(good), '--dry-run', '-w']);
  check('--dry-run with -w is a usage error', r.code === 2, r);
  r = kerf(['apply', 'a.kerf.json', '--ops', JSON.stringify(good), '-w', '--why', 'add sill']);
  check('good apply still writes', r.code === 0 && /^wrote a.kerf.json$/m.test(r.out) && readJson('a.kerf.json').components.length === 1, r);
}

// ---------------------------------------------------------------------------------------------------
section('2. kerf schema, kerf new --template, kerf call help, kerf guide');
{
  let r = kerf(['schema']);
  check('schema (no topic) lists the topics', r.code === 0 && /view/.test(r.out) && /note/.test(r.out) && /lumber/.test(r.out) && /ops/.test(r.out), r.out.slice(0, 200));
  for (const t of ['doc', 'view', 'note', 'dim', 'label', 'cite', 'ops', 'at', 'array', 'acknowledge', 'common', 'refs']) {
    r = kerf(['schema', t]);
    check(`schema ${t}`, r.code === 0 && r.out.length > 100 && !/[^\x00-\x7f]/.test(r.out), r.err || r.out.slice(0, 80));
  }
  for (const t of ['lumber', 'panel', 'cmu_wall', 'concrete', 'rebar', 'anchor_bolt', 'connector', 'truss', 'membrane', 'fill', 'insulation', 'flashing', 'joint', 'solid']) {
    r = kerf(['schema', t]);
    check(`schema ${t} (component) has params and an example`, r.code === 0 && /Example:/.test(r.out) && /- /.test(r.out), r.err);
  }
  r = kerf(['schema', 'view']);
  check('schema view names the fields agents guessed wrong', ['crop', 'scale', 'notes_side', 'cut_z', 'annotations', 'number', 'title'].every((k) => r.out.includes('- ' + k + ':')), r.out);
  r = kerf(['schema', 'note']);
  check('schema note: text/target/at/place/cite', ['text', 'target', 'at', 'place', 'cite'].every((k) => r.out.includes('- ' + k + ':')));
  r = kerf(['schema', 'veiw']);
  check('unknown topic: nearest suggestion, exit 1', r.code === 1 && /Did you mean "view"/.test(r.err), r.err);

  r = kerf(['new', 't.kerf.json', '--template', 'section', '--title', 'MY TITLE']);
  check('new --template section', r.code === 0 && /created t.kerf.json/.test(r.out), r);
  const t = readJson('t.kerf.json');
  check('template: id from the file name, title applied, 2 components, 1 view with 4 annotations',
    t.id === 't' && t.title === 'MY TITLE' && t.components.length === 2 && t.views.length === 1 && t.views[0].annotations.length === 4, t);
  check('template view has crop, scale, notes_side; one note has a citation',
    t.views[0].crop && t.views[0].scale && t.views[0].notes_side && t.views[0].annotations.filter((a) => a.cite).length === 1);
  r = kerf(['check', 't.kerf.json']);
  check('template checks with 0 errors and 0 warnings', r.code === 0 && /0 errors  0 warnings/.test(r.out), r.out);
  r = kerf(['export', 't.kerf.json', '--view', 'A', '--format', 'png', '-o', 't.png']);
  check('template exports', r.code === 0 && fs.statSync(path.join(tmp, 't.png')).size > 1000, r);
  r = kerf(['new', 't.kerf.json', '--template', 'section']);
  check('new refuses to overwrite', r.code === 1);
  r = kerf(['new', 'u.kerf.json', '--template', 'nope']);
  check('unknown template', r.code === 2 && /available: section/.test(r.err), r.err);

  r = kerf(['call', 'help']);
  let h = null; try { h = JSON.parse(r.out); } catch {}
  check('call help: functions with input shapes', r.code === 0 && h && h.functions.some((f) => f.name === 'apply' && /ops/.test(f.input)) && h.functions.some((f) => f.name === 'export' && /format/.test(f.input)), r.out);
  r = kerf(['call', 'nope'], { input: '{}' });
  check('call unknown fn mentions help', r.code === 1 && /kerf call help/.test(r.out + r.err), r);

  r = kerf(['guide']);
  check('guide embeds schema view/note/dim/label/cite/ops and the example document',
    r.code === 0 && ['view: ', 'note: ', 'dim: ', 'label: ', 'cite: ', 'ops: ', 'Complete minimal document', '"notes_side": "both"', 'nothing written', '--template section'].every((k) => r.out.includes(k)), r.out.slice(0, 300));
  check('guide is pure ASCII', !/[^\x00-\x7f]/.test(r.out));
}

console.log(`\n${passed} passed, ${failed} failed`);
process.exit(failed ? 1 : 0);
