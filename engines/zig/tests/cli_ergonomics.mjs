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

// ---------------------------------------------------------------------------------------------------
section('3. W_UNKNOWN_KEY, dim dir default + W_DIM_ZERO, W_NOTE_STYLE');
{
  for (const f of ['flush-beam-strap', 'monopour-slab-door-recess', 'truss-bearing-cmu']) {
    const r = kerf(['check', path.join(DETAILS, f + '.kerf.json')]);
    check(`reference ${f}: 0 errors, 0 warnings`, /0 errors  0 warnings/.test(r.out), r.out.split('\n')[0]);
  }
  for (const f of fs.readdirSync(DOCS).filter((n) => n.endsWith('.kerf.json'))) {
    const r = kerf(['check', path.join(DOCS, f)]);
    check(`tests/docs/${f}: 0 errors, 0 warnings`, /0 errors  0 warnings/.test(r.out), r.out.split('\n')[0]);
  }
  kerf(['new', 'k.kerf.json', '--template', 'section']);
  const ops = [
    { op: 'add', path: 'views/A/annotations', value: { id: 'n3', type: 'note', text: 'GYPSUM BOARD', target: 'stud', citations: [{ code: 'IRC', section: 'R602.3' }], side: 'left', point: [1, 2], kind: 'note', pos: [0, 0], bogus: 1 } },
    { op: 'add', path: 'views/A/annotations', value: { id: 'd_v', type: 'dim', from: 'sill@bottom_left', to: 'sill@top_left', offset: -3 } },
    { op: 'add', path: 'views/A/annotations', value: { id: 'd_zero', type: 'dim', from: 'sill@bottom_left', to: 'sill@bottom_right', dir: 'v', offset: -3 } },
    { op: 'update', path: 'views/A', value: { side: 'left' } },
  ];
  let r = kerf(['apply', 'k.kerf.json', '--ops', JSON.stringify(ops), '-w', '--why', 'junk']);
  check('apply succeeds (warnings only)', r.code === 0, r);
  check('citations -> cite', /W_UNKNOWN_KEY n3: unknown key "citations".*Did you mean "cite"/.test(r.out), r.out);
  check('note side -> view notes_side hint', /unknown key "side" in views\/A\/annotations\/n3.*notes_side/.test(r.out));
  check('point -> at', /unknown key "point".*Did you mean "at"/.test(r.out));
  check('kind -> type', /unknown key "kind".*Did you mean "type"/.test(r.out));
  check('pos -> place', /unknown key "pos".*Did you mean "place"/.test(r.out));
  check('view side -> notes_side', /unknown key "side" in views\/A \(view\).*Did you mean "notes_side"/.test(r.out));
  check('no suggestion lists the valid keys', /unknown key "bogus".*Valid keys: id, type, text, target, at, place, cite/.test(r.out));
  check('unknown keys are preserved in the file', (() => { const d = readJson('k.kerf.json'); const n = d.views[0].annotations.find((a) => a.id === 'n3'); return n.citations && n.side && n.point && n.bogus === 1 && d.views[0].side === 'left'; })());
  check('W_DIM_ZERO for a dim measuring 0"', /WARN W_DIM_ZERO d_zero:.*measures 0"/.test(r.out) && !/W_DIM_ZERO d_v/.test(r.out), r.out);
  check('W_NOTE_STYLE for GYPSUM BOARD', /W_NOTE_STYLE n3:.*GYP\. BD\./.test(r.out), r.out);
  const stored = readJson('k.kerf.json');
  check('dim dir default is not written into the document', stored.views[0].annotations.find((a) => a.id === 'd_v').dir === undefined);
  // the default dir makes d_v vertical: its text reads 1 1/2"
  r = kerf(['drawing', 'k.kerf.json', '--view', 'A']);
  const dr = JSON.parse(r.out);
  const texts = [];
  (function walk(x) { if (Array.isArray(x)) x.forEach(walk); else if (x && typeof x === 'object') { if (x.src === 'd_v' && typeof x.s === 'string') texts.push(x.s); Object.values(x).forEach(walk); } })(dr);
  check('dim without dir measures vertically (1 1/2")', texts.some((t) => t.includes('1 1/2')), texts);
}

// ---------------------------------------------------------------------------------------------------
section('4. acknowledge (I_ACK, logged) and lumber.barrier');
{
  const doc = {
    kerf: '0.1', id: 'b', run: [-24, 24],
    components: [
      { id: 'cmu', type: 'cmu_wall', width: 8, courses: 3, bond_beam_courses: 1, at: { to: [0, 0] } },
      { id: 'sill', type: 'lumber', size: '2x6', orient: 'flat', at: { anchor: 'bottom_left', to: 'cmu@top_left', offset: [1, 0] } },
    ],
    views: [{ id: 'A', scale: '3"=1\'-0"', crop: { x: [-6, 16], y: [10, 30] }, cut_z: 0, annotations: [] }],
  };
  write('b.kerf.json', doc);
  let r = kerf(['check', 'b.kerf.json']);
  check('untreated sill on CMU warns', /WARN W_UNTREATED_CONTACT sill/.test(r.out), r.out);
  const ack = [{ op: 'update', path: 'components/sill', value: { acknowledge: [{ code: 'W_UNTREATED_CONTACT', reason: 'truss seat moisture barrier by mfr.' }] } }];
  write('ack.json', ack);
  r = kerf(['apply', 'b.kerf.json', 'ack.json', '-w', '--why', 'ack the sill']);
  check('acknowledged: warning gone, I_ACK line with the reason', r.code === 0 && /0 errors  0 warnings/.test(r.out) && /^INFO I_ACK sill: W_UNTREATED_CONTACT on 'sill' acknowledged: truss seat moisture barrier by mfr\./m.test(r.out), r.out);
  const log = fs.readFileSync(path.join(tmp, 'b.kerf.json.log.jsonl'), 'utf8').trim().split('\n').map((l) => JSON.parse(l));
  check('apply -w logs the acknowledgement (reason) and the ops read from a file', log[0].ack && /moisture barrier by mfr/.test(log[0].ack[0]) && log[0].ops.length === 1 && log[0].ops[0].path === 'components/sill', log[0]);
  r = kerf(['apply', 'b.kerf.json', '--ops', JSON.stringify([{ op: 'update', path: 'components/sill', value: { acknowledge: [{ code: 'E_PARAM', reason: 'x' }, { code: 'W_OVERLAP' }] } }])]);
  check('errors cannot be acknowledged; reason required', r.code === 1 && /acknowledge\/0: 'E_PARAM' cannot be acknowledged/.test(r.err) && /acknowledge\/1:.*non-empty "reason"/.test(r.err), r.err);
  r = kerf(['apply', 'b.kerf.json', '--ops', JSON.stringify([{ op: 'update', path: 'components/sill', value: { acknowledge: null, barrier: 'sill_seal' } }]), '-w', '--why', 'sealer']);
  check('barrier sill_seal clears W_UNTREATED_CONTACT and adds the strip part', r.code === 0 && /0 errors  0 warnings/.test(r.out) && /\+ sill_seal/.test(r.out) && !/I_ACK/.test(r.out), r.out);
  r = kerf(['call', 'inspect'], { input: JSON.stringify({ doc: readJson('b.kerf.json'), query: { q: 'component', id: 'sill' } }) });
  const comp = JSON.parse(r.out);
  check('sill has a barrier part and is raised 1/8"', comp.parts.some((p) => p.name === 'barrier') && Math.abs(comp.bbox[3] - comp.bbox[1] - 1.625) < 1e-6, comp.bbox);
  r = kerf(['apply', 'b.kerf.json', '--ops', JSON.stringify([{ op: 'update', path: 'components/sill', value: { barrier: 'tape' } }])]);
  check('bad barrier value: E_PARAM names the choices', r.code === 1 && /barrier.*"sill_seal", "membrane"/.test(r.err), r.err);
  const style = JSON.parse(fs.readFileSync(path.join(here, '..', '..', '..', 'spec', 'styles', 'kerf-standard.kerfstyle.json'), 'utf8'));
  check('style has material sill_seal', !!style.materials.sill_seal);
}

// ---------------------------------------------------------------------------------------------------
section('5. defaults: anchor_bolt anchor and z, in-plane member z = first section view cut_z');
{
  const doc = {
    kerf: '0.1', id: 'dz', run: [-24, 24],
    components: [
      { id: 'cmu', type: 'cmu_wall', width: 8, courses: 3, at: { to: [0, 0] } },
      { id: 'bolt', type: 'anchor_bolt', diameter: 0.5, at: { to: 'cmu@top_center' } },
      { id: 'stud', type: 'lumber', size: '2x4', run: 'y', length: 24, at: { anchor: 'bottom_left', to: 'cmu@top_left', offset: [20, 0] } },
      { id: 'pinned', type: 'lumber', size: '2x4', run: 'y', length: 24, z: 0, at: { anchor: 'bottom_left', to: 'cmu@top_left', offset: [30, 0] } },
    ],
    views: [{ id: 'S', kind: 'section', cut_z: 6, crop: { x: [-10, 40], y: [0, 40] }, scale: '1"=1\'-0"', annotations: [] }, { id: 'B', kind: 'iso', cut_z: -10 }],
  };
  const info = (id) => JSON.parse(kerf(['call', 'inspect'], { input: JSON.stringify({ doc, query: { q: 'component', id } }) }).out);
  const bolt = info('bolt');
  const top = 1 * (7.625 + 0.375) * 3 - 0.375;
  check('anchor_bolt defaults to the top_of_concrete anchor: shank top above the CMU, hook below', bolt.bbox[3] > top && bolt.bbox[1] < top, bolt.bbox);
  check('anchor_bolt z defaults to the first section view cut_z (6)', Math.abs((bolt.z[0] + bolt.z[1]) / 2 - 6) < 1e-9, bolt.z);
  check('lumber run y z defaults to cut_z', Math.abs((info('stud').z[0] + info('stud').z[1]) / 2 - 6) < 1e-9);
  check('explicit z wins', Math.abs((info('pinned').z[0] + info('pinned').z[1]) / 2) < 1e-9);
  const noView = { ...doc, views: [] };
  const b2 = JSON.parse(kerf(['call', 'inspect'], { input: JSON.stringify({ doc: noView, query: { q: 'component', id: 'stud' } }) }).out);
  check('no section view: centered on the middle of run (0)', Math.abs((b2.z[0] + b2.z[1]) / 2) < 1e-9, b2.z);
}

console.log(`\n${passed} passed, ${failed} failed`);
process.exit(failed ? 1 : 0);
