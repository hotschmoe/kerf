#!/usr/bin/env node
// CLI tests for the v0.1.3 agent ergonomics (SPEC 19): apply errors on stderr, --dry-run, schema,
// new --template, call help, guide embedding, W_UNKNOWN_KEY, acknowledge, lints. Runs the real binary.
//
//   node tests/cli_ergonomics.mjs [path/to/kerf]      (default: engine/target/release/kerf; run `cargo build --release` first)
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const KERF = path.resolve(process.argv[2] ?? path.join(here, '..', 'target', 'release', process.platform === 'win32' ? 'kerf.exe' : 'kerf'));
if (!fs.existsSync(KERF)) { console.error('kerf binary not found: ' + KERF + ' (cargo build --release first)'); process.exit(2); }
const DETAILS = path.join(here, '..', '..', 'spec', 'details');
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
  check('guide (short) is at most 12 KB', r.code === 0 && Buffer.byteLength(r.out) <= 12288, Buffer.byteLength(r.out));
  check('guide has the doc/view/note/dim/label/cite schema lines, the example document, the topic list',
    ['- view: id*', '- note: id*', '- dim: id*', '- label: id*', '- cite: code*', 'Complete example', '"notes_side":"both"', '--template section', 'kerf schema <topic> [<topic> ...]', 'Component types'].every((k) => r.out.includes(k)), r.out.slice(0, 300));
  check('guide: one-command-per-line recipes, no chaining, ops file + --why, roof pitch recipe, ambiguity policy',
    /do NOT chain with `&&`/i.test(r.out) && /heredoc/.test(r.out) && r.out.includes('kerf new truss-cmu.kerf.json --ops ops.json') && r.out.includes('kerf apply truss-cmu.kerf.json ops.json -w --why')
      && r.out.includes('"slope":"@truss"') && r.out.includes('"until":"truss@top_chord_end"') && /ONE short question/.test(r.out) && r.out.includes('W_SHORT_SLOPE'), r.out.slice(0, 200));
  check('guide command examples never chain commands or use heredocs/pipes', r.out.split('\n').filter((l) => /^kerf /.test(l)).every((l) => !/(&&|;|\||<<)/.test(l)) && r.out.split('\n').filter((l) => /^kerf /.test(l)).length >= 8);
  check('guide is pure ASCII', !/[^\x00-\x7f]/.test(r.out));
  const full = kerf(['guide', '--full']);
  check('guide --full: long form with the full schema, drafting rules and the catalog', full.code === 0 && full.out.length > 30000 && full.out.includes('Complete minimal document') && full.out.includes('# Drafting instructions') && full.out.includes('# Component catalog') && full.out.includes('Long-form notes') && !/[^\x00-\x7f]/.test(full.out), full.out.length);
  check('guide --full starts with the short guide', full.out.startsWith(r.out.split('\n').slice(0, 20).join('\n')));

  r = kerf(['schema', 'note', 'dim', 'cite']);
  check('schema takes several topics', r.code === 0 && /^note: /m.test(r.out) && /^dim: /m.test(r.out) && /^cite: /m.test(r.out), r.err);
  r = kerf(['schema', 'note', 'veiw', 'lumber']);
  check('schema with one bad topic: others print, bad one reported on stderr, exit 1', r.code === 1 && /^note: /m.test(r.out) && /^lumber: /m.test(r.out) && /Did you mean "view"/.test(r.err), r);
  r = kerf(['schema', 'lumber']);
  check('catalog lumber states the standard beam-in-wall view (elevation along the wall)', /ELEVATION along the wall/.test(r.out) && /end-on section/.test(r.out), r.out.slice(0, 300));
}

// ---------------------------------------------------------------------------------------------------
section('2b. kerf new --ops (create + apply in one command), apply --ops FILE');
{
  const ops = [{ op: 'set', path: 'doc', value: { kerf: '0.1', id: 'zz', components: [{ id: 'sill', type: 'lumber', size: '2x6', orient: 'flat', treated: true }], views: [{ id: 'A', annotations: [] }] } }];
  write('newops.json', ops);
  let r = kerf(['new', 'n1.kerf.json', '--ops', 'newops.json', '--why', 'Build it', '--title', 'T']);
  check('new --ops file: exit 0, summary, created', r.code === 0 && /^DOC zz /m.test(r.out) && /^created n1.kerf.json$/m.test(r.out), r);
  check('the ops were applied to the new file', readJson('n1.kerf.json').components.length === 1 && readJson('n1.kerf.json').id === 'zz');
  const lines = fs.readFileSync(path.join(tmp, 'n1.kerf.json.log.jsonl'), 'utf8').trim().split('\n').map((l) => JSON.parse(l));
  check('both steps are logged: create, then the apply with the --why', lines.length === 2 && lines[0].ops[0].op === 'create' && lines[1].why === 'Build it' && lines[1].ops[0].op === 'set' && lines[1].changed.includes('sill'), lines);
  r = kerf(['new', 'n2.kerf.json', '--ops', JSON.stringify([{ op: 'update', path: 'components/x', value: {} }])]);
  check('new --ops with a failing op: error + nothing written, no file, exit 1', r.code === 1 && /^ERROR E_REF_UNKNOWN/m.test(r.err) && /nothing written/.test(r.err) && !fs.existsSync(path.join(tmp, 'n2.kerf.json')), r);
  r = kerf(['new', 'n3.kerf.json', '--ops', 'missing.json']);
  check('new --ops with a missing file: usage error naming it', r.code === 2 && /cannot read ops 'missing.json'/.test(r.err) && !fs.existsSync(path.join(tmp, 'n3.kerf.json')), r);
  r = kerf(['new', 'n4.kerf.json', '--template', 'section', '--ops', JSON.stringify([{ op: 'update', path: 'components/stud', value: { length: 30 } }]), '--why', 'taller stud']);
  check('new --template --ops: ops applied on top of the template', r.code === 0 && readJson('n4.kerf.json').components.find((c) => c.id === 'stud').length === 30, r);
  write('upd.json', [{ op: 'update', path: 'components/sill', value: { size: '2x8' } }]);
  r = kerf(['apply', 'n1.kerf.json', '--ops', 'upd.json', '-w', '--why', 'ops file via --ops']);
  check('apply --ops accepts a file path as well as inline JSON', r.code === 0 && readJson('n1.kerf.json').components[0].size === '2x8', r);
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
  check('no suggestion lists the valid keys', /unknown key "bogus".*Valid keys: id, type, text, target, at, place, (column, )?(key, )?cite/.test(r.out));
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
  check('acknowledged: warning gone, counted on the DOC line, listed by code with component and reason', r.code === 0 && /0 errors  0 warnings  1 acknowledged$/m.test(r.out) && /^ACKNOWLEDGED  1 warning suppressed/m.test(r.out) && /^ W_UNTREATED_CONTACT +x1 +sill: "truss seat moisture barrier by mfr\."$/m.test(r.out), r.out);
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
  const style = JSON.parse(fs.readFileSync(path.join(here, '..', '..', 'spec', 'styles', 'kerf-standard.kerfstyle.json'), 'utf8'));
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

// ---------------------------------------------------------------------------------------------------
section('6. W_SHORT_SLOPE (e07 repro), the roof-pitch recipe, acknowledge of I_* is a no-op');
{
  const ref = JSON.parse(fs.readFileSync(path.join(DETAILS, 'truss-bearing-cmu.kerf.json'), 'utf8'));
  let r = kerf(['check', path.join(DETAILS, 'truss-bearing-cmu.kerf.json')]);
  check('negative: the reference truss detail has 0 warnings', /0 errors  0 warnings/.test(r.out), r.out.split('\n')[0]);
  // e07 repro: pitch changed to 6:12, sheathing follows with slope "@truss" but keeps a literal length that stops short
  const e07 = JSON.parse(JSON.stringify(ref));
  e07.components.find((c) => c.id === 'truss').pitch = '6:12';
  Object.assign(e07.components.find((c) => c.id === 'roof_sheathing'), { slope: '@truss', length: 40 });
  write('e07.kerf.json', e07);
  r = kerf(['check', 'e07.kerf.json']);
  check('e07 repro: W_SHORT_SLOPE on the sheathing, concrete fix with until + anchor', /WARN W_SHORT_SLOPE roof_sheathing: .*short of/.test(r.out) && /"until": "truss@top_chord_end"/.test(r.out) && r.code === 0, r.out);
  // the recipe from the guide fixes it in one ops file
  write('pitch.json', [{ op: 'update', path: 'components/roof_sheathing', value: { slope: '@truss', until: 'truss@top_chord_end', length: null } }]);
  r = kerf(['apply', 'e07.kerf.json', 'pitch.json', '-w', '--why', 'sheathing follows the truss']);
  check('the guide recipe (update with until, length null) clears W_SHORT_SLOPE; the explicit crop now clips the longer roof: W_CROP_STALE (the e07 gap)', r.code === 0 && !/W_SHORT_SLOPE/.test(r.out) && /^WARN W_CROP_STALE roof_sheathing: after this edit/m.test(r.out) && /until truss@top_chord_end/.test(r.out), r.out);
  r = kerf(['apply', 'e07.kerf.json', '--ops', JSON.stringify({ op: 'update', path: 'views/A', value: { crop: null, scale: null } }), '-w', '--why', 'auto-fit the roof']);
  check('removing the crop and scale (the warning\'s fix) leaves 0 warnings', r.code === 0 && /0 errors  0 warnings/.test(r.out), r.out);
  // steeper pitch on the reference with the recipe: still 0 warnings, sheathing follows
  write('pitch8.json', [{ op: 'update', path: 'components/truss', value: { pitch: '8:12' } }]);
  r = kerf(['apply', 'e07.kerf.json', 'pitch8.json', '-w', '--why', 'steeper']);
  check('pitch 8:12 with slope @truss + until: 0 warnings', r.code === 0 && /0 errors  0 warnings/.test(r.out), r.out);
  // acknowledge of an info code: accepted, no error
  const ackI = [{ op: 'update', path: 'components/truss', value: { acknowledge: [{ code: 'I_SOLID_USED', reason: 'not applicable' }] } }];
  r = kerf(['apply', 'e07.kerf.json', '--ops', JSON.stringify(ackI), '--dry-run']);
  check('acknowledge I_SOLID_USED is a no-op, not an error', r.code === 0 && !/ERROR/.test(r.err) && /0 errors/.test(r.out), r);
  // an E_ code is still refused
  r = kerf(['apply', 'e07.kerf.json', '--ops', JSON.stringify([{ op: 'update', path: 'components/truss', value: { acknowledge: [{ code: 'E_PARAM', reason: 'x' }] } }]), '--dry-run']);
  check('acknowledge of an E_ code is still an error', r.code === 1 && /cannot be acknowledged/.test(r.err), r.err);
}

// ---------------------------------------------------------------------------------------------------
section('6b. geometry checks per view: omit-based scenarios, findings named with their views');
{
  const scn = {
    kerf: '0.1', id: 'scn', title: 'SCENARIOS',
    components: [
      { id: 'beam', type: 'lumber', size: '4x10', run: 'z', at: { anchor: 'bottom_left', to: [0, 0] } },
      { id: 'cap', type: 'lumber', size: '2x4', run: 'x', length: 6, at: { anchor: 'bottom_left', to: [1, 9] } },
      { id: 'a_joist', type: 'lumber', size: '2x10', run: 'x', length: 24, at: { anchor: 'bottom_right', to: 'beam@bottom_left' } },
      { id: 'b_joist', type: 'lumber', size: '2x8', run: 'x', length: 24, at: { anchor: 'bottom_right', to: 'beam@bottom_left' } },
      { id: 'b_blk', type: 'lumber', size: '2x4', run: 'x', length: 6, at: { anchor: 'bottom_right', to: [1, 8] } },
      { id: 'b_nailer', type: 'lumber', size: '2x4', run: 'x', length: 6, at: { anchor: 'bottom_left', to: 'a_joist@top_left', offset: [4, 0] } },
    ],
    views: [
      { id: 'A', kind: 'section', title: 'SCENARIO A', omit: ['b_joist', 'b_blk', 'b_nailer'] },
      { id: 'B', kind: 'section', title: 'SCENARIO B', omit: ['a_joist'] },
      { id: 'C', kind: 'iso', title: 'SCENARIO B', omit: ['a_joist', 'b_blk'] },
    ],
  };
  write('scn.kerf.json', scn);
  let r = kerf(['check', 'scn.kerf.json']);
  const warns = r.out.split('\n').filter((l) => l.startsWith('WARN '));
  check('members of different scenarios never drawn together: no W_OVERLAP between them', !warns.some((l) => /'a_joist' and 'b_/.test(l)), warns);
  check('a finding in every view reads as before (no view list)', warns.some((l) => /^WARN W_OVERLAP beam: 'beam' and 'cap' overlap by 0\.63 sq in .* within z -3\/4"\.\.3\/4" Fix:/.test(l)), warns);
  check('the iso view uses its own omit: the overlap with b_blk is reported once, in view B only', warns.filter((l) => /'b_blk' overlap/.test(l)).length === 1 && warns.some((l) => /^WARN W_OVERLAP beam: 'beam' and 'b_blk' overlap by 1\.25 sq in .* \(in view B\) Fix:/.test(l)), warns);
  check('a member resting on another scenario\'s member floats where that member is omitted: W_FLOATING once, views B and C listed', warns.filter((l) => /W_FLOATING/.test(l)).length === 1 && warns.some((l) => /^WARN W_FLOATING b_nailer: component 'b_nailer' touches nothing .* \(in views B, C\) Fix:/.test(l)), warns);
  check('a gap another scenario\'s member hid is a W_NEAR_MISS in the views that show it', warns.some((l) => /^WARN W_NEAR_MISS b_blk: 'b_blk' and 'b_joist' run parallel with a 3\/4" gap along y .* \(in view B\) Fix: move 'b_blk' 3\/4" down/.test(l)), warns);
  check('5 warnings in all, output stable across runs', warns.length === 5 && /0 errors  5 warnings/.test(r.out) && kerf(['check', 'scn.kerf.json']).out === r.out, r.out);
  const ack = { ...scn, components: scn.components.map((c) => (c.id === 'b_blk' ? { ...c, acknowledge: [{ code: 'W_OVERLAP', reason: 'blocking notched around the beam' }] } : c)) };
  write('scn-ack.kerf.json', ack);
  r = kerf(['check', 'scn-ack.kerf.json']);
  const ackJson = JSON.parse(kerf(['call', 'check'], { input: JSON.stringify({ doc: ack }) }).out);
  check('acknowledging the per-view overlap: the I_ACK diagnostic and the acknowledged entry keep the view list', /0 errors  4 warnings  1 acknowledged/.test(r.out) && ackJson.diagnostics.some((d) => d.code === 'I_ACK' && /^W_OVERLAP on 'b_blk' acknowledged: blocking notched around the beam \(was: .*\(in view B\)\)/.test(d.message)) && /\(in view B\)$/.test(ackJson.acknowledged[0].message), r.out);
  const { views, ...noViews } = scn;
  write('scn-noviews.kerf.json', noViews);
  r = kerf(['check', 'scn-noviews.kerf.json']);
  check('no views: every component is checked together, as before', /0 errors  4 warnings/.test(r.out) && /^WARN W_OVERLAP a_joist: 'a_joist' and 'b_joist' overlap by 174\.00 sq in/m.test(r.out) && !/\(in view/.test(r.out), r.out);
  const e07 = JSON.parse(fs.readFileSync(path.join(DETAILS, 'truss-bearing-cmu.kerf.json'), 'utf8'));
  Object.assign(e07.components.find((c) => c.id === 'roof_sheathing'), { slope: '@truss', length: 40 });
  e07.components.find((c) => c.id === 'truss').pitch = '6:12';
  write('e07-iso.kerf.json', e07);
  r = kerf(['check', 'e07-iso.kerf.json']);
  check('W_SHORT_SLOPE on the sheathing the iso omits names the section view only', /^WARN W_SHORT_SLOPE roof_sheathing: .*short of .* \(in view A\) Fix:/m.test(r.out), r.out);
}

// ---------------------------------------------------------------------------------------------------
section('7. v0.1.5 (SPEC 21): lenient ops input');
{
  kerf(['new', 'l.kerf.json', '--template', 'section']);
  const one = { op: 'update', path: 'components/stud', value: { length: 100 } };
  let r = kerf(['apply', 'l.kerf.json', '--ops', JSON.stringify(one), '-w', '--why', 'flag why']);
  check('a single op object is accepted', r.code === 0 && readJson('l.kerf.json').components.find((c) => c.id === 'stud').length === 100, r);
  r = kerf(['apply', 'l.kerf.json', '-', '-w'], { input: JSON.stringify({ ops: [{ op: 'update', path: 'components/stud', value: { length: 96 } }], why: 'envelope why' }) });
  check('{"ops":[...],"why":"..."} on stdin is accepted', r.code === 0 && readJson('l.kerf.json').components.find((c) => c.id === 'stud').length === 96, r);
  let lines = fs.readFileSync(path.join(tmp, 'l.kerf.json.log.jsonl'), 'utf8').trim().split('\n').map((l) => JSON.parse(l));
  check('log: single op is logged as an array; --why from the flag, else from the envelope', lines[1].ops.length === 1 && lines[1].why === 'flag why' && lines[2].why === 'envelope why', lines.slice(1));
  write('flagwins.json', { ops: [{ op: 'update', path: 'components/stud', value: { length: 97 } }], why: 'envelope' });
  r = kerf(['apply', 'l.kerf.json', 'flagwins.json', '-w', '--why', 'flag']);
  lines = fs.readFileSync(path.join(tmp, 'l.kerf.json.log.jsonl'), 'utf8').trim().split('\n').map((l) => JSON.parse(l));
  check('--why wins over the envelope why', r.code === 0 && lines[3].why === 'flag', lines[3]);
  r = kerf(['apply', 'l.kerf.json', '--ops', JSON.stringify({ path: 'components/stud', value: 1 })]);
  check('anything else: E_PARAM naming the three shapes and what was received', r.code === 1 && /^ERROR E_PARAM ops: .*array of op objects.*single op object.*"ops":\[\.\.\.\].*got an object with keys path, value/m.test(r.err) && /nothing written/.test(r.err), r.err);
  r = kerf(['new', 'l2.kerf.json', '--ops', JSON.stringify({ op: 'update', path: 'components/x', value: {} })]);
  check('new --ops takes a single op too (fails here on the unknown component, not on the shape)', r.code === 1 && /^ERROR E_REF_UNKNOWN/m.test(r.err), r.err);
}

// ---------------------------------------------------------------------------------------------------
section('8. v0.1.5 (SPEC 21): meta.requested coverage');
{
  let r = kerf(['guide']);
  check('guide tells agents to fill meta.requested, check COVERAGE and finish only at full coverage; 12 KB, ASCII', /meta\.requested/.test(r.out) && /COVERAGE/.test(r.out) && /full coverage/.test(r.out) && Buffer.byteLength(r.out) <= 12288 && !/[^\x00-\x7f]/.test(r.out), Buffer.byteLength(r.out));
  check('guide mentions W_REQUESTED_MISSING and W_CROP_STALE', /W_REQUESTED_MISSING/.test(r.out) && /W_CROP_STALE/.test(r.out));
  r = kerf(['schema', 'doc']);
  check('schema doc documents meta.requested and the COVERAGE block', /requested\[\]/.test(r.out) && /COVERAGE/.test(r.out) && /W_REQUESTED_MISSING/.test(r.out), r.out.slice(0, 300));
  kerf(['new', 'c.kerf.json', '--template', 'section']);
  write('req.json', { ops: [{ op: 'update', path: 'meta', value: { requested: ['stud wall', 'bond beam', 'H2.5A ties', 'sill'] } }], why: 'record the request' });
  r = kerf(['apply', 'c.kerf.json', 'req.json', '-w']);
  check('apply prints the COVERAGE block: ok with component ids, MISSING for the rest', r.code === 0 && /^COVERAGE  \d\/4 requested elements covered/m.test(r.out) && /^ MISSING  bond beam$/m.test(r.out) && /^ ok       sill\s+sill/m.test(r.out), r.out);
  check('one W_REQUESTED_MISSING per missing item, with a fix', (r.out.match(/^WARN W_REQUESTED_MISSING/gm) || []).length >= 2 && /requested element "bond beam" \(meta\.requested\) matches no component \(id, type, label, model, size\) or annotation \(note, label, dim text, weld, view title\).* Fix: build it/.test(r.out), r.out);
  const chk = kerf(['check', 'c.kerf.json']);
  check('check prints the same COVERAGE block', /^COVERAGE  /m.test(chk.out) && /^ MISSING  H2\.5A ties$/m.test(chk.out) && chk.code === 0, chk.out);
  write('req2.json', [{ op: 'add', path: 'components', value: { id: 'bond_beam', type: 'lumber', size: '2x6', orient: 'flat', at: { anchor: 'bottom_left', to: 'sill@top_left' } } },
    { op: 'add', path: 'views/A/annotations', value: { id: 'n_ht', type: 'note', text: 'SIMPSON H2.5A HURRICANE TIE @ EA. STUD', target: 'stud' } },
    { op: 'update', path: 'meta', value: { requested: ['bond beam', 'H2.5A ties', 'sill'] } }]);
  r = kerf(['apply', 'c.kerf.json', 'req2.json', '-w', '--why', 'cover the request']);
  check('after building them: full coverage, no W_REQUESTED_MISSING', r.code === 0 && /^COVERAGE  3\/3 requested elements covered$/m.test(r.out) && !/W_REQUESTED_MISSING/.test(r.out) && /^ ok       H2\.5A ties\s+stud/m.test(r.out), r.out);
  r = kerf(['apply', 'c.kerf.json', '--ops', JSON.stringify({ op: 'update', path: 'meta', value: { requested: 'bond beam' } }), '--dry-run']);
  check('meta.requested as a string is an E_PARAM with an example', r.code === 1 && /^ERROR E_PARAM meta\/requested: .*array of strings.*Fix: e\.g\./m.test(r.err), r.err);
  for (const d of ['truss-bearing-cmu', 'monopour-slab-door-recess', 'flush-beam-strap']) {
    const o = kerf(['check', path.join(DETAILS, d + '.kerf.json')]);
    check(`reference ${d}: no meta.requested, so no COVERAGE block and 0 warnings`, !/COVERAGE/.test(o.out) && /0 errors  0 warnings/.test(o.out), o.out.split('\n')[0]);
  }
}

// ---------------------------------------------------------------------------------------------------
section('9. v0.1.5 (SPEC 21): W_CROP_STALE');
{
  write('crop.kerf.json', { kerf: '0.1', id: 'crop', components: [{ id: 'stud', type: 'lumber', size: '2x4', run: 'y', length: 30 }, { id: 'sill', type: 'lumber', size: '2x6', orient: 'flat', at: { anchor: 'top_left', to: 'stud@bottom_left' } }],
    views: [{ id: 'A', crop: { x: [-10, 40], y: [-12, 40] }, annotations: [] }] });
  let r = kerf(['check', 'crop.kerf.json']);
  check('a member inside its crop: no W_CROP_STALE', !/W_CROP_STALE/.test(r.out), r.out);
  r = kerf(['apply', 'crop.kerf.json', '--ops', JSON.stringify({ op: 'update', path: 'components/stud', value: { length: 90 } }), '-w', '--why', 'taller stud']);
  check('apply: the edit that pushes the stud out of the crop warns, naming the crop that holds it and "remove crop"', r.code === 0 && /^WARN W_CROP_STALE stud: after this edit, 'stud' has 56% of its extent outside the crop of view A.*Fix: remove "crop" \(and "scale", if you set one\) from view A.*"y":\[-12,90\]/m.test(r.out), r.out);
  r = kerf(['check', 'crop.kerf.json']);
  check('check without history: the stud is still partly in the crop (a deliberate cut), so quiet', !/W_CROP_STALE/.test(r.out), r.out);
  r = kerf(['apply', 'crop.kerf.json', '--ops', JSON.stringify({ op: 'add', path: 'components', value: { id: 'far', type: 'lumber', size: '2x4', run: 'y', length: 30, at: { anchor: 'bottom_left', to: [500, 0] } } }), '-w', '--why', 'far member']);
  check('apply: a new member outside the crop warns', /^WARN W_CROP_STALE far: after this edit/m.test(r.out), r.out);
  r = kerf(['check', 'crop.kerf.json']);
  check('check without history: a member entirely outside the crop (no view shows it) warns, "outside the crop"', /^WARN W_CROP_STALE far: 'far' has 100% of its extent outside the crop of view A/m.test(r.out), r.out);
  r = kerf(['apply', 'crop.kerf.json', '--ops', JSON.stringify({ op: 'update', path: 'views/A', value: { crop: null } }), '-w', '--why', 'auto-fit']);
  check('removing the crop (auto-fit) clears it', r.code === 0 && !/W_CROP_STALE/.test(r.out), r.out);
}

// ---------------------------------------------------------------------------------------------------
section('10. v0.1.5 (SPEC 21): note lint extensions');
{
  kerf(['new', 'nl.kerf.json', '--template', 'section']);
  const ops = [
    { op: 'add', path: 'views/A/annotations', value: { id: 'n_chat', type: 'note', text: 'SILL SHOULD BE TREATED, RIGHT?', target: 'sill' } },
    { op: 'add', path: 'views/A/annotations', value: { id: 'n_dangle', type: 'note', text: '2X6 STUD W/', target: 'stud' } },
    { op: 'add', path: 'views/A/annotations', value: { id: 'n_long', type: 'note', text: '2X4 STUD @ 16" O.C. W/ (2) 16D NAILS EA. END INTO SILL AND TOP PLATE, PLUS TWO MORE NAILS TOE-NAILED THROUGH THE SIDE, INSTALL PER MFR. SPECS AND TABLE', target: 'stud' } },
    { op: 'add', path: 'views/A/annotations', value: { id: 'n_dup', type: 'note', text: '2x6 pt sill plate.', target: 'sill' } },
  ];
  let r = kerf(['apply', 'nl.kerf.json', '--ops', JSON.stringify(ops), '-w', '--why', 'sloppy notes']);
  check('apply succeeds (warnings only)', r.code === 0, r);
  check('commentary: question mark and SHOULD BE', /WARN W_NOTE_STYLE n_chat: .*commentary \(a question mark\)/.test(r.out) && /Fix: rewrite it as/.test(r.out), r.out);
  check('dangling connector', /WARN W_NOTE_STYLE n_dangle: .*ends on "W\/"/.test(r.out), r.out);
  check('length over 130', /WARN W_NOTE_STYLE n_long: .*characters \(notes stay under 130/.test(r.out), r.out);
  check('a note that repeats another one names it', /WARN W_NOTE_STYLE n_dup: .*repeats note 'n_sill'/.test(r.out), r.out);
  for (const d of ['truss-bearing-cmu', 'monopour-slab-door-recess', 'flush-beam-strap']) {
    const o = kerf(['check', path.join(DETAILS, d + '.kerf.json')]);
    check(`reference ${d} still has 0 warnings`, /0 errors  0 warnings/.test(o.out), o.out.split('\n')[0]);
  }
}

// ---------------------------------------------------------------------------------------------------
section('11. writers: parallel apply -w loses no edit, --if-match, atomic fmt -w, hostile --view');
{
  const { spawn } = await import('node:child_process');
  const run = (args) => new Promise((resolve) => {
    const c = spawn(KERF, args, { cwd: tmp, env: { ...process.env, KERF_ACTOR: 'test' } });
    let out = '', err = '';
    c.stdout.on('data', (d) => out += d); c.stderr.on('data', (d) => err += d);
    c.on('close', (code) => resolve({ code, out, err }));
  });
  kerf(['new', 'par.kerf.json', '--template', 'section']);
  const N = 12;
  const rs = await Promise.all(Array.from({ length: N }, (_, i) => run(['apply', 'par.kerf.json', '--ops',
    JSON.stringify([{ op: 'add', path: 'components', value: { id: 'blk' + i, type: 'lumber', size: '2x4', orient: 'flat', at: { to: [i * 6, -20] } } }]), '-w', '--why', 'parallel ' + i])));
  check(`${N} parallel apply -w all succeed`, rs.every((r) => r.code === 0), rs.map((r) => r.err).join('|'));
  const doc = JSON.parse(fs.readFileSync(path.join(tmp, 'par.kerf.json'), 'utf8'));
  const ids = new Set(doc.components.map((c) => c.id));
  check(`no edit lost: all ${N} blocks are in the document`, Array.from({ length: N }, (_, i) => ids.has('blk' + i)).every(Boolean), [...ids]);
  const log = fs.readFileSync(path.join(tmp, 'par.kerf.json.log.jsonl'), 'utf8').trim().split('\n');
  check('one valid log line per write', log.length >= N && log.every((l) => { try { JSON.parse(l); return true; } catch { return false; } }), log.length);
  // --if-match
  let r = kerf(['apply', 'par.kerf.json', '--ops', JSON.stringify([{ op: 'update', path: 'meta', value: { author: 'x' } }]), '-w', '--if-match', '"1-2-0000000000000000"']);
  check('stale --if-match is refused (E_STALE, nothing written)', r.code === 1 && /E_STALE/.test(r.err) && /nothing written/.test(r.err), r);
  check('--if-match without -w is a usage error', kerf(['apply', 'par.kerf.json', '--ops', '[]', '--if-match', 'x']).code === 2);
  r = kerf(['fmt', 'par.kerf.json', '-w']);
  check('fmt -w succeeds', r.code === 0, r);
  check('fmt -w leaves no temp files behind', fs.readdirSync(tmp).every((n) => !/\.tmp|^\./.test(n) || n === '.'), fs.readdirSync(tmp));
  // hostile --view must stay one JSON string
  r = kerf(['drawing', 'par.kerf.json', '--view', 'A","style":{"notes":{"wrap_chars":-5}},"x":"']);
  check('--view with quotes is an unknown view, not JSON injection', r.code === 1 && /unknown view|not found|no view/i.test(r.err + r.out) && !/E_STYLE/.test(r.err + r.out), r);
  r = kerf(['export', 'par.kerf.json', '--view', 'A', '--format', 'svg', '--px', '1600,"x":1', '-o', 'x.svg']);
  check('--px must be a number', r.code === 2 && /--px must be a number/.test(r.err), r);
}

// ---------------------------------------------------------------------------------------------------
section('12. meta.references: reference images next to the document (W_REFERENCE_FILE)');
{
  let r = kerf(['schema', 'doc']);
  check('schema doc documents meta.references and W_REFERENCE_FILE', /references\[\]/.test(r.out) && /meta\.references: \[\{"path": "refs\//.test(r.out) && /W_REFERENCE_FILE/.test(r.out), r.out.slice(0, 300));
  r = kerf(['guide']);
  check('guide lists W_REFERENCE_FILE and stays within 12 KB', /W_REFERENCE_FILE/.test(r.out) && Buffer.byteLength(r.out) <= 12288, Buffer.byteLength(r.out));
  const lib = path.join(tmp, 'lib');
  fs.mkdirSync(path.join(lib, 'refs'), { recursive: true });
  fs.writeFileSync(path.join(lib, 'refs', 'stem.png'), Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==', 'base64'));
  fs.writeFileSync(path.join(tmp, 'outside.png'), 'x');
  kerf(['new', 'lib/r.kerf.json', '--template', 'section']);
  const refs = [
    { caption: 'EXTERIOR WALL AT STEM', path: 'refs/stem.png' },
    { path: 'refs/missing.png' },
    { path: '../outside.png' },
    { path: '/etc/passwd.png' },
    { path: 'refs\\stem.png' },
    { path: 'refs/notes.txt' },
  ];
  r = kerf(['apply', 'lib/r.kerf.json', '--ops', JSON.stringify([{ op: 'update', path: 'meta', value: { references: refs } }]), '-w', '--why', 'add references']);
  check('apply writes meta.references (path problems are warnings, not errors)', r.code === 0 && /^wrote lib\/r\.kerf\.json$/m.test(r.out), r);
  const text = fs.readFileSync(path.join(lib, 'r.kerf.json'), 'utf8');
  check('canonical order inside a reference: path, then caption', text.indexOf('"path": "refs/stem.png"') < text.indexOf('"caption": "EXTERIOR WALL AT STEM"') && text.indexOf('"path": "refs/stem.png"') > 0, text.slice(text.indexOf('"references"'), text.indexOf('"references"') + 160));
  check('apply already reports the path text problems', /^WARN W_REFERENCE_FILE: meta\.references\[2\]\.path "\.\.\/outside\.png" climbs out of the document's folder/m.test(r.out) && !/refs\/missing\.png/.test(r.out), r.out);
  r = kerf(['check', 'lib/r.kerf.json']);
  check('check from another folder: exit 0 (warnings only)', r.code === 0 && /0 errors  5 warnings/.test(r.out), r.out.split('\n')[0]);
  check('check: a missing file is W_REFERENCE_FILE with a Fix, looked up relative to the document (not the cwd)', /^WARN W_REFERENCE_FILE: meta\.references\[1\]\.path "refs\/missing\.png" does not exist relative to the document's folder \(or is not a file\) Fix: copy the snip to refs\/missing\.png/m.test(r.out) && !/references\[0\]/.test(r.out), r.out);
  check('check: ".." escapes the folder', /^WARN W_REFERENCE_FILE: meta\.references\[2\]\.path .* Fix: copy the image into the workspace folder and write its path relative to the document, e\.g\. "refs\/outside\.png"$/m.test(r.out), r.out);
  check('check: an absolute path is refused', /^WARN W_REFERENCE_FILE: meta\.references\[3\]\.path "\/etc\/passwd\.png" is not relative to the document's folder/m.test(r.out), r.out);
  check('check: backslashes get the forward-slash spelling as the fix', /^WARN W_REFERENCE_FILE: meta\.references\[4\]\.path "refs\\stem\.png" uses backslashes Fix: write it with forward slashes: "refs\/stem\.png"$/m.test(r.out), r.out);
  check('check: a file type the panel cannot show', /^WARN W_REFERENCE_FILE: meta\.references\[5\]\.path "refs\/notes\.txt" is not a file type the reference panel shows/m.test(r.out), r.out);
  const inside = spawnSync(KERF, ['check', 'r.kerf.json'], { cwd: lib, encoding: 'utf8' });
  check('check from the document folder reports the same', inside.stdout === r.out, inside.stdout);
  fs.copyFileSync(path.join(lib, 'refs', 'stem.png'), path.join(lib, 'refs', 'missing.png'));
  r = kerf(['check', 'lib/r.kerf.json']);
  check('once the file is there the missing warning is gone', r.code === 0 && /0 errors  4 warnings/.test(r.out) && !/refs\/missing\.png/.test(r.out), r.out);
  if (process.platform !== 'win32') {
    fs.rmSync(path.join(lib, 'refs', 'missing.png'));
    fs.symlinkSync(path.join(tmp, 'outside.png'), path.join(lib, 'refs', 'missing.png'));
    r = kerf(['check', 'lib/r.kerf.json']);
    check('a symlink that leaves the folder is W_REFERENCE_FILE (outside)', /^WARN W_REFERENCE_FILE: meta\.references\[1\]\.path "refs\/missing\.png" resolves outside the document's folder through a symbolic link Fix: copy the image into the folder/m.test(r.out), r.out);
  }
  r = kerf(['apply', 'lib/r.kerf.json', '--ops', JSON.stringify({ op: 'update', path: 'meta', value: { references: 'refs/stem.png' } }), '--dry-run']);
  check('meta.references as a string is an E_PARAM with an example', r.code === 1 && /^ERROR E_PARAM meta\/references: .*array of \{path, caption\} objects.*Fix: e\.g\./m.test(r.err), r.err);
  r = kerf(['apply', 'lib/r.kerf.json', '--ops', JSON.stringify({ op: 'update', path: 'meta', value: { references: [{ caption: 'NO PATH' }, { path: 'refs/stem.png', caption: 7, page: 2 }] } }), '--dry-run']);
  check('entry without a path / non-string caption are E_PARAM', r.code === 1 && /^ERROR E_PARAM meta\/references\/0\/path: .*needs "path"/m.test(r.err) && /^ERROR E_PARAM meta\/references\/1\/caption: /m.test(r.err), r.err);
  r = kerf(['apply', 'lib/r.kerf.json', '--ops', JSON.stringify({ op: 'update', path: 'meta', value: { references: [{ path: 'refs/stem.png', page: 2 }] } }), '--dry-run']);
  check('an unknown key in a reference warns W_UNKNOWN_KEY with the valid keys', r.code === 0 && /^WARN W_UNKNOWN_KEY: unknown key "page" in meta\/references\/0 \(reference\): .*Valid keys: path, caption/m.test(r.out), r.out);
  r = kerf(['call', 'check'], { input: JSON.stringify({ doc: JSON.parse(text), reference_files: { 'refs/stem.png': 'gone' } }) });
  check('call check: reference_files values other than missing/outside are E_INPUT', r.code === 1 && /"code":"E_INPUT"/.test(r.out) && /reference_files/.test(r.out), r.out);
  r = kerf(['call', 'check'], { input: JSON.stringify({ doc: JSON.parse(text), reference_files: { 'refs/stem.png': 'missing' } }) });
  check('call check: the host reports a missing file through reference_files', r.code === 0 && /meta\.references\[0\]\.path \\"refs\/stem\.png\\" does not exist/.test(r.out), r.out.slice(0, 200));
}

// ---------------------------------------------------------------------------------------------------
section('13. keyed notes (SPEC 24): tags, DETAIL NOTES list, shared keys, list mode');
{
  const base = JSON.parse(fs.readFileSync(path.join(DETAILS, 'stud-wall-stem-footing.kerf.json'), 'utf8'));
  const texts = (d) => d.items.filter((it) => it.t === 'text');
  const drawing = (name, doc, view) => {
    write(name, doc);
    const r = kerf(['drawing', name, '--view', view]);
    return { r, d: r.code === 0 ? JSON.parse(r.out) : null };
  };
  let o = kerf(['check', path.join(DETAILS, 'stud-wall-stem-footing.kerf.json')]);
  check('reference stud-wall-stem-footing has 0 warnings', /0 errors  0 warnings/.test(o.out), o.out.split('\n')[0]);

  let { d } = drawing('k1.kerf.json', base, 'A');
  const tagOf = (id) => texts(d).filter((t) => t.src === id && t.valign === 'middle').map((t) => t.s);
  check('unkeyed notes number in note order around the explicit key 5', tagOf('n_sheath')[0] === '1' && tagOf('n_stem')[0] === '6' && tagOf('n_sill')[0] === '12', [tagOf('n_sheath'), tagOf('n_stem'), tagOf('n_sill')]);
  check('two notes with key 5 draw two "5" tags', tagOf('n_ftg_bars')[0] === '5' && tagOf('n_stem_bars')[0] === '5');
  const nums = texts(d).filter((t) => /^\d+\.$/.test(t.s)).map((t) => t.s);
  check('the list has one entry per key (12 entries, 5. once)', nums.length === 12 && nums.filter((s) => s === '5.').length === 1, nums);
  const crop = d.crop ?? null;
  const head = texts(d).find((t) => t.s === 'DETAIL NOTES');
  const cropTop = Math.max(...d.items.filter((it) => it.t === 'region').flatMap((it) => it.loops.flat().map((p) => p[1])));
  check('notes_side above: the DETAIL NOTES list sits above the drawing', head && head.y > cropTop && texts(d).filter((t) => /^\d+\.$/.test(t.s)).every((t) => t.y > cropTop), [head, cropTop, crop]);
  check('the leader text is not at the leader ends', !texts(d).some((t) => t.src === 'n_slab' && t.s === 'CONC. SLAB' && t.valign === 'middle'));
  const xs = new Set(texts(d).filter((t) => /^\d+\.$/.test(t.s)).map((t) => t.x.toFixed(3)));
  check('a 15-line list above wraps into two columns', xs.size === 2, [...xs]);

  // beside: notes_side left with a short crop wraps into two columns left of everything
  const left = structuredClone(base);
  Object.assign(left.views[0], { notes_side: 'left', crop: { x: [-4, 30], y: [20, 40] } });
  left.views[0].annotations = left.views[0].annotations.filter((a) => a.type === 'note');
  ({ d } = drawing('k2.kerf.json', left, 'A'));
  const lnums = texts(d).filter((t) => /^\d+\.$/.test(t.s));
  const tagXs = texts(d).filter((t) => t.valign === 'middle').map((t) => t.x);
  check('notes_side left: list left of the tags, two columns', lnums.length > 0 && Math.max(...lnums.map((t) => t.x)) < Math.min(...tagXs) && new Set(lnums.map((t) => t.x.toFixed(3))).size === 2, lnums.map((t) => [t.s, t.x]));

  // shared key without text, conflicting text, bad key, leader + above
  const edge = structuredClone(base);
  const ann = edge.views[1].annotations;
  ann.push({ id: 'n_sill2', type: 'note', key: 1, target: 'stud' });
  ann.push({ id: 'n_conf', type: 'note', key: 2, text: 'ANOTHER BOLT', target: 'stem' });
  ann.push({ id: 'n_badkey', type: 'note', key: 'A', text: 'CONC. SLAB', target: 'slab' });
  ann.push({ id: 'n_notext', type: 'note', key: 9, target: 'slab' });
  edge.views[0].notes_style = 'leader';
  o = kerf(['check', write('k3.kerf.json', edge)]);
  check('a note with a shared key may omit its text', !/n_sill2/.test(o.out), o.out);
  check('W_PARAM: a shared key with a different text names the note whose text the list shows', /WARN W_PARAM n_conf: .*shares key 2 with note 'n_anchor'/.test(o.out), o.out);
  check('W_PARAM: key must be a whole number 1-999 (ignored)', /WARN W_PARAM n_badkey: .*"key" must be a whole number from 1 to 999 \(got "A"\)/.test(o.out), o.out);
  check('E_PARAM: no text and no note with that key', /ERROR E_PARAM n_notext: note 'n_notext' needs a string "text" \(or a "key" shared/.test(o.out), o.out);
  check('W_PARAM: notes_side above on a leader view says what it is for', /WARN W_PARAM A: view A: notes_side "above" places the DETAIL NOTES list of keyed notes/.test(o.out), o.out);
  ({ d } = drawing('k3.kerf.json', edge, 'B'));
  const sill = texts(d).filter((t) => (t.src === 'n_sill' || t.src === 'n_sill2') && t.valign === 'middle').map((t) => t.s);
  check('the shared key draws a tag at each note', sill.length === 2 && sill.every((s) => s === '1'), sill);

  // lint: repeats with the same key are intended; without one a keyed view suggests the key
  const lint = structuredClone(base);
  lint.views[0].annotations.push({ id: 'n_rep', type: 'note', key: 5, text: '(2) #4 CONT. U.N.O.', target: 'dowel' });
  lint.views[0].annotations.push({ id: 'n_rep2', type: 'note', text: 'CONC. SLAB', target: 'slab' });
  lint.views[0].annotations.push({ id: 'n_lower', type: 'note', text: 'gravel', target: 'base' });
  o = kerf(['check', write('k4.kerf.json', lint)]);
  check('W_NOTE_STYLE: same text under the same key is not a repeat', !/W_NOTE_STYLE n_rep:/.test(o.out), o.out);
  check('W_NOTE_STYLE: a repeat in a keyed view suggests sharing the key', /WARN W_NOTE_STYLE n_rep2: .*repeats note 'n_slab'.*Fix: give 'n_rep2' the "key" of 'n_slab'/.test(o.out), o.out);
  check('W_NOTE_STYLE lints keyed (list) text like leader text', /WARN W_NOTE_STYLE n_lower: .*lowercase/.test(o.out), o.out);

  // list mode: no tags, no leaders, no target needed
  const list = structuredClone(base);
  list.views[1].notes_style = 'list';
  list.views[1].annotations = [
    { id: 't1', type: 'note', text: 'ALL DIMENSIONS ARE TO FACE OF STUD WALL' },
    { id: 't2', type: 'note', text: 'SEE ARCH. DRAWINGS FOR FINISHES' },
  ];
  o = kerf(['check', write('k5.kerf.json', list)]);
  check('list mode: notes need no target', /0 errors/.test(o.out), o.out);
  ({ d } = drawing('k5.kerf.json', list, 'B'));
  const typical = (it) => it.src === 't1' || it.src === 't2';
  check('list mode: two entries, no tags, no leaders', texts(d).filter((t) => /^\d+\.$/.test(t.s)).length === 2 && !texts(d).some((t) => typical(t) && t.valign === 'middle') && !d.items.some((it) => typical(it) && it.t !== 'text'));

  // iso keyed
  const iso = structuredClone(base);
  iso.views.push({ id: 'C', kind: 'iso', number: '113', title: 'ISO', from: 'front_right', cutaway: true, cut_z: 0, notes_style: 'keyed',
    annotations: [{ id: 'i1', type: 'note', text: 'CONC. STEM WALL', target: 'stem' }, { id: 'i2', type: 'note', text: 'CONC. FOOTING', target: 'ftg' }] });
  ({ d } = drawing('k6.kerf.json', iso, 'C'));
  check('iso views draw keyed tags and the list', d && texts(d).some((t) => t.s === 'DETAIL NOTES') && texts(d).filter((t) => /^\d+\.$/.test(t.s)).length === 2);

  const sv = kerf(['schema', 'view', 'note']);
  check('kerf schema view/note document notes_style and key', /notes_style/.test(sv.out) && /leader\|keyed\|list/.test(sv.out) && /\bkey\b.*integer/.test(sv.out) && /right\|left\|both\|above/.test(sv.out), sv.out);
}

// ---------------------------------------------------------------------------------------------------
section('14. steel shapes, bolts and weld symbols: table errors, W_BOLT_EDGE, W_BOLT_SPACING, W_WELD_SIZE');
{
  const steelDoc = (extra, annotations = []) => ({
    kerf: '0.1', id: 'stl', title: 'STEEL',
    components: [
      { id: 'plate', type: 'steel', shape: 'PL3/4x10', length: 10, at: { to: [0, 0] } },
      { id: 'col', type: 'steel', shape: 'HSS4x4x1/4', orient: 'elevation', run: 'y', length: 24, at: { anchor: 'bottom_center', to: 'plate@face_top' } },
      ...extra,
    ],
    views: [{ id: 'A', kind: 'section', number: '1', title: 'STEEL', cut_z: 0, annotations }],
  });
  let r = kerf(['schema', 'steel']);
  check('schema steel lists the shape table and the anchors', r.code === 0 && /Shape table: W: W4x13/.test(r.out) && /HSS4x4x\(/.test(r.out) && /top_flange/.test(r.out) && /inner_top/.test(r.out) && !/[^\x00-\x7f]/.test(r.out), r.out.slice(0, 200));
  r = kerf(['schema', 'bolt', 'weld']);
  check('schema bolt and weld', r.code === 0 && /^bolt: /m.test(r.out) && /head_bearing/.test(r.out) && /^weld: /m.test(r.out) && /all_around/.test(r.out), r.err);
  write('stl.kerf.json', steelDoc([{ id: 'bad', type: 'steel', shape: 'W8x19' }, { id: 'bad2', type: 'steel', shape: 'HSS5x3x5/8' }]));
  r = kerf(['check', 'stl.kerf.json']);
  check('unknown W size: E_PARAM names the sizes of that depth', /ERROR E_PARAM bad: W8x19 is not in the table\. W8 sizes: W8x10, .*W8x18/.test(r.out), r.out);
  check('unknown HSS wall: E_PARAM lists the walls', /ERROR E_PARAM bad2: HSS5x3 has no 5\/8" wall in the table\. Walls: 1\/8, 3\/16/.test(r.out), r.out);
  const bolts = { id: 'rods', type: 'bolt', diameter: 0.75, grip: 0.75, count: 2, spacing: 1.5, at: { to: { ref: 'plate@top_left', offset: [0.75, 0] } } };
  write('stl.kerf.json', steelDoc([bolts]));
  r = kerf(['check', 'stl.kerf.json']);
  check('W_BOLT_EDGE: 3/4" from the plate edge < 1" (AISC J3.4)', /WARN W_BOLT_EDGE rods: .*edge distance to steel 'plate' is 3\/4" in the drawing plane, below the AISC 360 Table J3\.4 minimum of 1" for a 3\/4" bolt/.test(r.out), r.out);
  check('W_BOLT_SPACING: 1 1/2" < 2 2/3 d', /WARN W_BOLT_SPACING rods: .*spacing 1 1\/2" is below the AISC 360 J3\.3 minimum/.test(r.out), r.out);
  write('stl.kerf.json', steelDoc([{ ...bolts, spacing: 3, at: { to: { ref: 'plate@top_left', offset: [1.5, 0] } } }]));
  r = kerf(['check', 'stl.kerf.json']);
  check('bolts 1 1/2" from the edge at 3" spacing: no bolt warning', r.code === 0 && !/W_BOLT/.test(r.out), r.out);
  const weld = { id: 'w1', type: 'weld', at: 'col@bottom_right', offset: [6, 6], size: '5/16', all_around: true, members: ['col', 'plate'] };
  write('stl.kerf.json', steelDoc([], [weld]));
  r = kerf(['check', 'stl.kerf.json']);
  check('W_WELD_SIZE: 5/16 fillet on a 0.233" HSS wall', /WARN W_WELD_SIZE w1: weld 'w1': fillet size 5\/16" is larger than the 0\.2325" maximum along the edge of 'col'/.test(r.out), r.out);
  write('stl.kerf.json', steelDoc([], [{ ...weld, size: '3/16' }]));
  r = kerf(['check', 'stl.kerf.json']);
  check('3/16 fillet: no weld warning', !/W_WELD_SIZE/.test(r.out), r.out);
  r = kerf(['drawing', 'stl.kerf.json', '--view', 'A']);
  const items = r.code === 0 ? JSON.parse(r.out).items : [];
  check('weld symbol drawn: leader, reference line, symbol, size text, all-around circle', items.filter((i) => i.src === 'w1' && i.t === 'path').length >= 4 && items.some((i) => i.src === 'w1' && i.t === 'text' && i.s === '3/16') && items.some((i) => i.src === 'w1' && i.t === 'path' && i.closed), r.err);
  check('HSS in elevation: solid silhouette, no hidden lines by default, never filled', items.some((i) => i.src === 'col' && i.pen === 'steel') && !items.some((i) => i.src === 'col' && (i.pen === 'hidden' || i.t === 'fill')), r.err);
  check('plate cut in section: solid steel fill', items.some((i) => i.src === 'plate' && i.t === 'fill'), r.err);
  const hid = steelDoc([]);
  hid.components[1].hidden_lines = true;
  hid.views[0].crop = { x: [-8, 8], y: [-2, 12] };
  write('stl.kerf.json', hid);
  r = kerf(['drawing', 'stl.kerf.json', '--view', 'A']);
  const its2 = r.code === 0 ? JSON.parse(r.out).items : [];
  check('hidden_lines: true draws the inside wall faces dashed', its2.filter((i) => i.src === 'col' && i.pen === 'hidden').length === 2, r.err);
  check('a steel member seen lengthwise gets a break line where the crop cuts it', its2.some((i) => i.src === 'crop' && i.pen === 'break' && i.pts.every((p) => Math.abs(p[1] - 12) < 1) && i.pts[0][0] < 3 && i.pts[i.pts.length - 1][0] > 7), r.err);
  write('stl.kerf.json', steelDoc([], [{ id: 'w2', type: 'weld', at: 'col@bottom_right' }]));
  r = kerf(['check', 'stl.kerf.json']);
  check('fillet weld without size: E_PARAM with a fix', /ERROR E_PARAM w2: weld 'w2': a fillet weld needs "size".*Fix: e\.g\. "size": "3\/16"/.test(r.out), r.out);
}

// issues #15 (cut fill vs embedded parts) and #16 (rebar placement in polygons, bar anchors)
section('15. #15 cut fill under rotated bolts; #16 rebar place in polygons, E_PLACE_OUTSIDE, W_BAR_OUTSIDE, path/polygon anchors');
{
  const doc = (components, annotations = []) => ({ kerf: '0.1', id: 'g', title: 'G', components, views: [{ id: 'A', kind: 'section', title: 'G', annotations }] });
  const fillArea = (items, src) => items.filter((i) => i.t === 'fill' && i.src === src).reduce((a, i) => a + i.loops.reduce((b, l) => b + l.reduce((c, p, k) => { const q = l[(k + 1) % l.length]; return c + (p[0] * q[1] - q[0] * p[1]) / 2; }, 0), 0), 0);
  for (const rot of [0, 90, 180, 270]) {
    write('b.kerf.json', doc([
      { id: 'beam', type: 'steel', shape: 'W12x26', at: { anchor: 'top_flange', to: [0, 0] } },
      { id: 'nailer', type: 'lumber', size: '3x8', orient: 'flat', at: { anchor: 'bottom_center', to: 'beam@top_flange' } },
      { id: 'stud', type: 'bolt', diameter: 0.5, grip: 2.5, rotate: rot, at: { to: 'beam@top_flange' } },
    ]));
    const r = kerf(['drawing', 'b.kerf.json', '--view', 'A']);
    const d = r.code === 0 ? JSON.parse(r.out) : { items: [], diagnostics: [] };
    const a = fillArea(d.items, 'beam');
    check(`bolt rotate ${rot} at a W flange: the W keeps its solid fill (6.4..7.65 sq in) and no W_CUT_FILL`, a > 6.4 && a < 7.66 && !d.diagnostics.some((g) => g.code === 'W_CUT_FILL'), [a, r.err]);
  }
  write('c.kerf.json', doc([
    { id: 'pl', type: 'steel', shape: 'PL1/2x2', at: { anchor: 'center', to: [0, 0] } },
    { id: 'cover', type: 'solid', material: 'steel', embedded: true, profile: { rect: [4, 4] }, at: { anchor: 'center', to: [0, 0] } },
  ]));
  let r = kerf(['drawing', 'c.kerf.json', '--view', 'A']);
  check('a cut part fully inside an embedded part: W_CUT_FILL names both', r.code === 0 && JSON.parse(r.out).diagnostics.some((g) => g.code === 'W_CUT_FILL' && /'pl' draws no cut fill in view A: embedded 'cover' covers all of its cut/.test(g.message)), r.out.slice(0, 400));

  const slab = { id: 'slab', type: 'concrete', shape: 'polygon', points: [[0, 0], [60, 0], [60, -12], [76, -12], [76, 4], [0, 4]], at: { to: [0, 0] } };
  write('p.kerf.json', doc([slab, { id: 'bars', type: 'rebar', size: '#4', place: { in: 'slab', face: 'bottom', cover: 3, count: 2 } }]));
  r = kerf(['check', 'p.kerf.json']);
  check('place bottom in an L slab: both bars in the thickened footing, 3" clear of its bottom and sides', r.code === 0 && /bars +rebar \(2\) #4 along z @ bottom face +x 5'-3"\.\.6'-1" +y -9"\.\.-8 1\/2"/.test(r.out) && !/W_COVER|W_BAR_OUTSIDE/.test(r.out), r.out);
  write('p.kerf.json', doc([slab, { id: 'bars', type: 'rebar', size: '#4', place: { in: 'slab', face: 'bottom', cover: 1.5, station: 20 } }]));
  r = kerf(['check', 'p.kerf.json']);
  check('place station: the bar sits above the bottom face at that x (the slab soffit, not the footing)', /bars +rebar \(1\) #4 along z @ bottom face +x 1'-7 3\/4"\.\.1'-8 1\/4" +y 1 1\/2"\.\.2"/.test(r.out), r.out);
  write('p.kerf.json', doc([slab, { id: 'bars', type: 'rebar', size: '#4', place: { in: 'slab', face: 'top', cover: 6 } }]));
  r = kerf(['check', 'p.kerf.json']);
  check('cover deeper than the host: E_PLACE_OUTSIDE with the bar position and a fix', r.code === 1 && /ERROR E_PLACE_OUTSIDE bars: the bar would sit at x 3'-2", y -2 1\/4", outside its host 'slab'.*Fix: lower place\.cover/.test(r.out), r.out);
  write('p.kerf.json', doc([slab, { id: 'loose', type: 'rebar', size: '#4', at: { to: [30, -6] } }, { id: 'bent', type: 'rebar', size: '#4', mode: 'path', points: [[3, 2], [40, 2], [40, -9], [50, -9]] }, { id: 'dowel', type: 'rebar', size: '#4', mode: 'path', points: [[70, -9], [70, 30]] }]));
  r = kerf(['check', 'p.kerf.json']);
  check('W_BAR_OUTSIDE: a dot in the air names the nearest host and the fix', /WARN W_BAR_OUTSIDE loose: rebar 'loose' is not inside concrete or masonry: its centre \(x 2'-6 1\/4", y -5 3\/4"\) lies in no host; the nearest host is 'slab', 5 3\/4" away Fix: move the bar into 'slab'/.test(r.out), r.out);
  check('W_BAR_OUTSIDE: a path bar with a bend in the air', /WARN W_BAR_OUTSIDE bent: .*a bend \(x 3'-4/.test(r.out), r.out);
  check('a dowel that leaves the concrete in a straight leg is not flagged', !/W_BAR_OUTSIDE dowel/.test(r.out), r.out);

  write('p.kerf.json', doc([slab, { id: 'bar', type: 'rebar', size: '#4', mode: 'path', points: [[3, 2], [73, 2], [73, -9], [63, -9]] }], [
    { id: 'n1', type: 'note', text: '#4 CORNER BAR', at: 'bar@bend_1' },
    { id: 'n2', type: 'note', text: 'HOOK', at: 'bar@seg_3_mid' },
    { id: 'd1', type: 'dim', from: 'slab@vertex_3', to: 'slab@vertex_4', offset: -6 },
    { id: 'n3', type: 'note', text: 'EDGE', at: 'slab@edge_3_mid' },
  ]));
  r = kerf(['drawing', 'p.kerf.json', '--view', 'A']);
  const its = r.code === 0 ? JSON.parse(r.out).items : [];
  const tip = (id) => {
    const p = its.find((i) => i.src === id && i.t === 'path' && !i.closed);
    const f = its.find((i) => i.src === id && i.t === 'fill');
    if (!p || !f) return null;
    const e = p.pts[p.pts.length - 1];
    return f.loops[0].reduce((b, q) => (Math.hypot(q[0] - e[0], q[1] - e[1]) > Math.hypot(b[0] - e[0], b[1] - e[1]) ? q : b), e);
  };
  const near = (p, x, y, tol) => p && Math.hypot(p[0] - x, p[1] - y) < tol;
  check('note at bar@bend_1 lands on the bend (72.49, 1.49), not a box corner', near(tip('n1'), 72.49, 1.49, 0.05), tip('n1'));
  check('note at bar@seg_3_mid lands on the hook leg (y -9)', tip('n2') && Math.abs(tip('n2')[1] + 9) < 0.3 && tip('n2')[0] > 63 && tip('n2')[0] < 73, tip('n2'));
  check('note at slab@edge_3_mid lands on the footing bottom (68, -12)', near(tip('n3'), 68, -12, 0.05), tip('n3'));
  check('dim between polygon vertices measures 1\'-4"', its.some((i) => i.src === 'd1' && i.t === 'text' && i.s === "1'-4\""), its.filter((i) => i.src === 'd1' && i.t === 'text'));
  write('p.kerf.json', doc([slab], [{ id: 'n1', type: 'note', text: 'X', at: 'slab@vertex_9' }]));
  r = kerf(['check', 'p.kerf.json']);
  check('unknown vertex anchor: the error lists vertex_1..vertex_6 and edge_k_mid', /vertex_6/.test(r.out) && /edge_6_mid/.test(r.out), r.out);
  r = kerf(['schema', 'cmu_wall', 'rebar']);
  check('schema documents cmu cell anchors and path bar anchors', /cell_left/.test(r.out) && /bend_k/.test(r.out) && /seg_k_mid/.test(r.out) && /E_PLACE_OUTSIDE/.test(r.out), r.out.slice(0, 300));
}

// ---------------------------------------------------------------------------------------------------
// issue #18: house-style lint false positives, case on the drawing, coverage through house abbreviations (SPEC 25.3)
section('#18. house style lint, kept case, requested coverage normalization');
{
  kerf(['new', 'hs.kerf.json', '--template', 'section']);
  const note = (id, text, extra = {}) => ({ op: 'add', path: 'views/A/annotations', value: { id, type: 'note', text, target: 'stud', ...extra } });
  const ok = [
    note('n_penny', '(2) 16d NAILS EA. SIDE, 8d @ 6" O.C.'),
    note('n_aci', 'LAP 48 db MIN., ldh PER ACI, fy = 60 KSI, f\'c = 3,000 psi'),
    note('n_model', 'SIMPSON HU212-2/HUC212-2 HANGER'),
    note('n_arch', 'FINISH PER ARCH.'),
    note('n_type', 'TYPE I CEMENT GROUT'),
    note('n_cfr', 'GUARDRAIL PER OSHA', { cite: [{ code: 'OSHA', section: '1926.755(a)(1)', status: 'suggested' }] }),
  ];
  let r = kerf(['apply', 'hs.kerf.json', '--ops', JSON.stringify(ok), '-w', '--why', 'house exceptions']);
  check('16d/8d, db/ldh/fy/f\'c/psi, HU212-2/HUC212-2, PER ARCH., TYPE I: no W_NOTE_STYLE', r.code === 0 && !/W_NOTE_STYLE/.test(r.out), r.out);
  r = kerf(['drawing', 'hs.kerf.json', '--view', 'A']);
  const texts = r.code === 0 ? JSON.parse(r.out).items.filter((i) => i.t === 'text').map((i) => i.s).join(' | ') : '';
  check('the drawing keeps 16d, 8d, db, ldh, f\'c, psi as written and upper-cases the rest', /\(2\) 16d NAILS EA\. SIDE/.test(texts) && /8d( \| )?@/.test(texts) && /LAP 48 db MIN\., ldh PER ACI,/.test(texts) && /fy = 60 KSI/.test(texts) && /f'c = 3,000 psi/.test(texts), texts);
  check('a citation section prints as written: 1926.755(a)(1)', /\(OSHA[ |]+1926\.755\(a\)\(1\)\)\*/.test(texts), texts);
  check('the model number prints unchanged', /SIMPSON HU212-2\/HUC212-2/.test(texts), texts);

  const bad = [
    note('b_lower', '(2) 16d nails each side'),
    note('b_frac', 'SIMPSON HU212-2 W/ 1-1/2" MIN. BEARING'),
    note('b_period', 'STUD EACH.'),
    note('b_trail', 'SHEATHING EDGE.'),
  ];
  r = kerf(['apply', 'hs.kerf.json', '--ops', JSON.stringify(bad), '-w', '--why', 'still wrong']);
  check('lower-case words are still flagged, and the fix keeps 16d', /WARN W_NOTE_STYLE b_lower: .*lowercase letters.*Fix: set text to "\(2\) 16d NAILS EA\. SIDE"$/m.test(r.out), r.out);
  check('a real dashed fraction is still flagged; the fix leaves HU212-2 alone', /WARN W_NOTE_STYLE b_frac: .*dashed fraction.*Fix: set text to "SIMPSON HU212-2 W\/ 1 1\/2" MIN\. BEARING"$/m.test(r.out), r.out);
  check('EACH. becomes EA. (no doubled period)', /WARN W_NOTE_STYLE b_period: .*Fix: set text to "STUD EA\."$/m.test(r.out), r.out);
  check('a period after a plain word is still a trailing period, and the message names the style escape', /WARN W_NOTE_STYLE b_trail: .*trailing period \(notes are not sentences; if "EDGE\." is an abbreviation, add it to the style's notes\.abbreviations\).*Fix: set text to "SHEATHING EDGE"$/m.test(r.out), r.out);

  kerf(['new', 'cov.kerf.json', '--template', 'section']);
  const cov = [
    { op: 'update', path: 'meta', value: { requested: ['bars each face', 'clear span', 'clr. span', 'single mat', 't-intersection', '3/16 fillet weld all around', 'stud 2 ft long', '(8) 16d each side', 'masonry wall opening', "6'-0in chord lap"] } },
    { op: 'update', path: 'views/A', value: { title: 'SINGLE MAT T-INTERSECTION' } },
    note('c1', '(2) #5 BARS EA. FACE CONT. UNDER OPENING'),
    note('c2', '(2) 16d NAILS EA. SIDE'),
    note('c3', '6\'-0" MIN. CHORD LAP'),
    { op: 'add', path: 'views/A/annotations', value: { id: 'd_span', type: 'dim', from: 'stud@bottom_left', to: 'stud@top_left', offset: -4, text: 'CLR. SPAN' } },
    { op: 'add', path: 'views/A/annotations', value: { id: 'd_len', type: 'dim', from: 'stud@bottom_right', to: 'stud@top_right', offset: 4 } },
    { op: 'add', path: 'views/A/annotations', value: { id: 'w1', type: 'weld', at: 'stud@bottom_right', offset: [6, 6], size: '3/16', all_around: true } },
  ];
  r = kerf(['apply', 'cov.kerf.json', '--ops', JSON.stringify(cov), '-w', '--why', 'coverage repro']);
  check('EA. = EACH: "bars each face" is covered by the note', /^ ok       bars each face\s+stud/m.test(r.out), r.out);
  check('dim text counts, both ways: "clear span" and "clr. span"', /^ ok       clear span\s+dim d_span/m.test(r.out) && /^ ok       clr\. span\s+dim d_span/m.test(r.out), r.out);
  check('view titles count, case and hyphen ignored: "single mat", "t-intersection"', /^ ok       single mat\s+view A/m.test(r.out) && /^ ok       t-intersection\s+view A/m.test(r.out), r.out);
  check('a weld counts as "<size> fillet weld all around"', /^ ok       3\/16 fillet weld all around\s+weld w1/m.test(r.out), r.out);
  check('a dim\'s measured 2\'-0" alone does not cover "stud 2 ft long" (one candidate must hold every word)', /^ MISSING  stud 2 ft long\s+closest: /m.test(r.out), r.out);
  check('(8) 16d each side is not covered by a (2) 16d note: MISSING names the closest note and the lacking word', /^ MISSING  \(8\) 16d each side\s+closest: note 'c2' in view A \(lacks "8"\)$/m.test(r.out), r.out);
  check('W_REQUESTED_MISSING quotes the closest candidate text', /WARN W_REQUESTED_MISSING: requested element "masonry wall opening" .*closest: note 'c1' in view A "\(2\) #5 BARS EA\. FACE CONT\. UNDER OPENING" \(1 of 3 words; lacks "masonry", "wall"\)/.test(r.out), r.out);
  check('feet-inches with a unit word: "6\'-0in" = 6\'-0"', /^ ok       6'-0in chord lap\s+stud/m.test(r.out), r.out);
  r = kerf(['apply', 'cov.kerf.json', '--ops', JSON.stringify({ op: 'update', path: 'components/stud', value: { label: 'stud 24in long' } }), '--dry-run']);
  check('lengths compare across units: "2 ft" = "24in"', /^ ok       stud 2 ft long\s+stud/m.test(r.out), r.out);

  write('house.kerfstyle.json', { text: { keep_case: ['kN'] }, notes: { abbreviations: { 'SHR.': 'SHEAR' } } });
  kerf(['new', 'st.kerf.json', '--template', 'section']);
  const st = [
    { op: 'update', path: 'meta', value: { requested: ['stud shear'] } },
    note('s1', 'STUD 25 kN SHR.'),
  ];
  kerf(['apply', 'st.kerf.json', '--ops', JSON.stringify(st), '-w', '--why', 'style house list']);
  r = kerf(['check', 'st.kerf.json', '--style', 'house.kerfstyle.json']);
  check('style text.keep_case and notes.abbreviations: no W_NOTE_STYLE, SHR. covers "shear"', !/W_NOTE_STYLE s1/.test(r.out) && /^ ok       stud shear\s+stud/m.test(r.out), r.out);
  r = kerf(['check', 'st.kerf.json']);
  check('without the style the same note is flagged (kN lower case, SHR. a trailing period) and shear is MISSING', /WARN W_NOTE_STYLE s1: .*lowercase letters.*trailing period/.test(r.out) && /^ MISSING  stud shear/m.test(r.out), r.out);
  write('badhouse.kerfstyle.json', { notes: { abbreviations: ['BRG.'] } });
  r = kerf(['check', 'st.kerf.json', '--style', 'badhouse.kerfstyle.json']);
  check('a malformed notes.abbreviations is E_STYLE with an example', r.code !== 0 && /notes\.abbreviations' must be an object/.test(r.out + r.err), r.out + r.err);
}

// ---------------------------------------------------------------------------------------------------
// issue #17: annotation layout (SPEC 25.2)
section('17. annotation layout: iso at points, view fit clearance, reported moves, pin, overlaps, untangled leaders');
{
  const items = (name, view) => {
    const r = kerf(['drawing', name, '--view', view]);
    return r.code === 0 ? JSON.parse(r.out) : { items: [], bounds: [0, 0, 0, 0], scale: 1, err: r.err };
  };
  const tip = (d, id) => d.items.find((i) => i.src === id && i.t === 'fill')?.loops[0][0];
  const near = (p, x, y) => p && Math.abs(p[0] - x) < 0.01 && Math.abs(p[1] - y) < 0.01;
  const iso = {
    kerf: '0.1', id: 'isoat', title: 'BEAM ON POST', run: [-24, 24],
    components: [
      { id: 'post', type: 'lumber', size: '4x4', run: 'y', length: 36, z: [-1.75, 1.75], at: { to: [0, 0] } },
      { id: 'beam', type: 'lumber', size: '4x8', run: 'z', orient: 'upright', at: { to: 'post@top_left' } },
    ],
    views: [{ id: 'A', kind: 'iso', number: '1', title: 'BEAM ON POST', scale: 'NTS', from: 'front_right', annotations: [
      { id: 'n1', type: 'note', text: 'BEAM CORNER', at: 'beam@top_right' },
      { id: 'n2', type: 'note', text: 'OFFSET POINT', at: { ref: 'post@bottom_left', offset: [1.75, 6] } },
      { id: 'n3', type: 'note', text: '4X4 POST', target: 'post' },
    ] }],
  };
  write('isoat.kerf.json', iso);
  let d = items('isoat.kerf.json', 'A');
  // front_right: u = (x - z) cos30, v = y - (x + z) / 2, z = the near face of the referenced member
  check('iso at Ref lands on the projected corner of the beam (near face z = 24)', near(tip(d, 'n1'), (3.5 - 24) * Math.cos(Math.PI / 6), 43.25 - 27.5 / 2), tip(d, 'n1'));
  check('iso {ref, offset}: the offset applies in model space, then the point projects (post face z = 1.75)', near(tip(d, 'n2'), 0, 4.25), tip(d, 'n2'));
  let r = kerf(['check', 'isoat.kerf.json']);
  check('iso at points on the members: no W_NOTE_LANDING', r.code === 0 && !/W_NOTE_LANDING/.test(r.out), r.out);
  const bubble = d.items.find((i) => i.src === 'title:A' && i.t === 'path' && i.closed);
  const rad = bubble ? (bubble.pts[1][0] - bubble.pts[0][0]) / 2 : 0;
  check('the view extents include the title bubble (its arc, not just its end points)', bubble && Math.abs(bubble.pts[0][1] - rad - d.bounds[1]) < 1e-3, [bubble?.pts, d.bounds]);
  check('NTS fit keeps 1/8" clear of the frame and the title block', (d.bounds[3] - d.bounds[1]) / d.scale <= 6.75 + 1e-6, (d.bounds[3] - d.bounds[1]) / d.scale);
  iso.views[0].annotations.push({ id: 'n4', type: 'note', text: 'NOTHING HERE', at: [60, 60] });
  write('isoat.kerf.json', iso);
  r = kerf(['check', 'isoat.kerf.json']);
  check('W_NOTE_LANDING: an iso arrow off every face, with the at value and a fix', /WARN W_NOTE_LANDING n4: view A: the arrow of note 'n4' \("at": \[60,60\]\) lands on empty paper, off every visible face of the iso, .* Fix: point "at" at the member/.test(r.out), r.out);

  const base = JSON.parse(fs.readFileSync(path.join(DETAILS, 'stud-wall-stem-footing.kerf.json'), 'utf8'));
  const sec = structuredClone(base);
  sec.views[0].annotations.push({ id: 'n_low', type: 'note', text: 'COMPACTED SUBGRADE', target: 'ftg', at: { ref: 'ftg@bottom_left', offset: [6, -30] } });
  r = kerf(['check', write('l1.kerf.json', sec)]);
  check('W_NOTE_LANDING: a section arrow below the crop, fix names the target', /WARN W_NOTE_LANDING n_low: .*lands on empty paper, outside the view crop.*Fix: remove "at" so the arrow lands inside target 'ftg'/.test(r.out), r.out);

  const tall = structuredClone(base);
  tall.views[0].crop.y = [-1, 40];
  r = kerf(['check', write('l2.kerf.json', tall)]);
  check('W_VIEW_FIT: a list + bubble that would touch the frame warns, names the list, suggests notes_side', /WARN W_VIEW_FIT A: .*1\/8" clear of both.*\(the DETAIL NOTES list\).*set notes_side to "right" or "left"/.test(r.out), r.out);

  const dims = structuredClone(base);
  dims.views[1].annotations = dims.views[1].annotations.filter((a) => a.type !== 'dim');
  dims.views[1].annotations.push(
    { id: 'd_a', type: 'dim', from: 'ftg@bottom_left', to: 'ftg@bottom_right', dir: 'h', offset: -6 },
    { id: 'd_b', type: 'dim', from: 'ftg@bottom_left', to: 'ftg@bottom_right', dir: 'h', offset: -6, text: 'VERIFY' },
  );
  r = kerf(['check', write('l3.kerf.json', dims)]);
  check('I_ANNOT_MOVED: a dimension stacked off its offset says how far, what it cleared and the offset that keeps it', /INFO I_ANNOT_MOVED d_b: view B: dimension 'd_b' is drawn at offset -\S+ instead of its offset -6: moved \S+ \(\S+" paper\) outward to clear dimension 'd_a'\s+Fix: keep it there: set "offset": -\S+;.*"pin": true/.test(r.out), r.out);
  dims.views[1].annotations[dims.views[1].annotations.length - 1].pin = true;
  r = kerf(['check', write('l3.kerf.json', dims)]);
  check('pin: no I_ANNOT_MOVED, and the overprint warns W_LABEL_OVERLAP', !/I_ANNOT_MOVED d_b/.test(r.out) && /WARN W_LABEL_OVERLAP d_\w: view B: the text '\S+' of dimension 'd_\w' overprints the text '\S+' of dimension 'd_\w'/.test(r.out), r.out);
  d = items('l3.kerf.json', 'B');
  check('pin: the dimension line is drawn at its own offset (footing bottom y 0, offset -6)', d.items.some((i) => i.src === 'd_b' && i.t === 'path' && i.pen === 'dim' && i.pts.length === 2 && i.pts[0][1] === -6 && i.pts[1][1] === -6), d.items.filter((i) => i.src === 'd_b').map((i) => i.pts));
  dims.views[1].annotations.push({ id: 'l_a', type: 'label', text: 'EXTERIOR', at: 'ftg@top_left', offset: [-12, 6] }, { id: 'l_b', type: 'label', text: 'GRADE', at: 'ftg@top_left', offset: [-12, 6] });
  dims.views[1].annotations.push({ id: 'l_c', type: 'label', text: 'X', at: 'ftg@top_left', pin: 'yes' });
  r = kerf(['check', write('l3.kerf.json', dims)]);
  check('W_LABEL_OVERLAP: two labels on one spot, with an offset that clears it', /WARN W_LABEL_OVERLAP l_a: view B: label 'l_a' \('EXTERIOR'\) overprints label 'l_b' \('GRADE'\)\s+Fix: set label 'l_a' "offset": \[\S+, \S+\] \(now \[-12, 6\]\)/.test(r.out), r.out);
  check('W_PARAM: pin must be a boolean', /WARN W_PARAM l_c: 'l_c' in view B: "pin" must be true or false \(got "yes"\)/.test(r.out), r.out);
  r = kerf(['schema', 'dim', 'label']);
  check('kerf schema dim/label document pin', (r.out.match(/pin.*bool/g) || []).length === 2 && /I_ANNOT_MOVED/.test(r.out), r.out);

  const spots = [[60, 14], [24, 19], [2, 20], [11, 20], [50, 54], [37, 57], [58, 19], [15, 21], [6, 34]];
  const words = ['STRAP', 'TIE', 'ANCHOR', 'HANGER', 'CLIP', 'PLATE', 'ROD', 'BOLT', 'SCREW'];
  write('l4.kerf.json', {
    kerf: '0.1', id: 'dense', title: 'DENSE', components: spots.map(([x, y], i) => ({ id: `b${i}`, type: 'lumber', size: '4x4', run: 'z', at: { to: [x, y] } })),
    views: [{ id: 'A', kind: 'section', number: '1', title: 'DENSE', cut_z: 0, notes_side: 'right',
      annotations: spots.map((_, i) => ({ id: `n${i}`, type: 'note', target: `b${i}`, text: `SIMPSON ${words[i]} EA. SIDE W/ (10) 16D NAILS INTO EA. MEMBER OR OTHER APPROVED HARDWARE PER PLAN ${i}` })) }],
  });
  r = kerf(['check', 'l4.kerf.json']);
  check('a tall column whose landings are spread in x: leaders untangled, no W_LEADER_HIT', r.code === 0 && !/W_LEADER_HIT/.test(r.out), r.out);
}

// ---------------------------------------------------------------------------------------------------
// Issue #8: scenario membership (component views, view only, globs in omit/only, W_REF_UNKNOWN) and acknowledged
// warnings that stay visible (summary count + block, --strict, W_ACK_UNUSED, IR list, export --review).
section('15. issue #8: scenarios at one origin (views, only, omit globs) and visible acknowledgements');
{
  const zb = (extra = {}) => ({
    kerf: '0.1', id: 'zb', title: 'Z BANDS', run: [-24, 112],
    components: [
      { id: 'a_slab', type: 'lumber', size: '2x8', run: 'z', z: [-24, 24], at: { anchor: 'bottom_left', to: [0, 0] }, views: ['A'], ...extra.a },
      { id: 'b_slab', type: 'lumber', size: '4x4', run: 'z', z: [88, 112], at: { anchor: 'bottom_left', to: [0, 0] }, views: ['B', 'C'] },
      { id: 'post', type: 'lumber', size: '2x4', run: 'z', at: { anchor: 'bottom_right', to: [0, 0] } },
    ],
    views: [
      { id: 'A', kind: 'section', cut_z: 0, annotations: [{ id: 'n1', type: 'note', text: '4X4', target: 'b_slab' }] },
      { id: 'B', kind: 'section', cut_z: 100, annotations: [] },
      { id: 'C', kind: 'section', cut_z: 100, only: ['b_*', 'post'], annotations: [] },
      ...(extra.views ?? []),
    ],
  });
  write('zb.kerf.json', zb());
  const srcs = (v) => {
    const d = JSON.parse(kerf(['drawing', 'zb.kerf.json', '--view', v]).out);
    return new Set(d.items.filter((i) => i.t === 'path').map((i) => i.src));
  };
  const sb = srcs('B');
  check('views: a shared-origin alternate in another z band draws no beyond outline in view B', !sb.has('a_slab') && sb.has('b_slab') && sb.has('post'), [...sb]);
  const sa = srcs('A');
  check('views: view A draws a_slab and the shared post, not b_slab', sa.has('a_slab') && sa.has('post') && !sa.has('b_slab'), [...sa]);
  const sc = srcs('C');
  check('only with a glob: view C draws b_slab and post only', sc.has('b_slab') && sc.has('post') && !sc.has('a_slab'), [...sc]);
  let r = kerf(['check', 'zb.kerf.json']);
  check('a note on a member the view excludes: W_NOTE_TARGET names the exclusion and the fix', /^WARN W_NOTE_TARGET n1: note 'n1' in view A: target 'b_slab' is not drawn in this view \(its "views" or the view's "omit" \/ "only" exclude 'b_slab'\)\..* Fix: move the note to a view that draws 'b_slab'/m.test(r.out), r.out);
  write('zb2.kerf.json', zb({ a: { views: ['A', 'D'] } }));
  r = kerf(['check', 'zb2.kerf.json']);
  check('views naming an unknown view: E_REF_UNKNOWN with the view list', r.code === 1 && /^ERROR E_REF_UNKNOWN a_slab: 'a_slab' lists view 'D' in "views", but there is no view 'D'\. Views: A, B, C/m.test(r.out), r.out);
  write('zb3.kerf.json', zb({ a: { views: [] } }));
  r = kerf(['check', 'zb3.kerf.json']);
  check('views: [] is E_PARAM (use visible:false to hide everywhere)', r.code === 1 && /^ERROR E_PARAM a_slab: 'views' of 'a_slab' must be a non-empty array of view ids.*visible": false/m.test(r.out), r.out);

  const base = zb();
  base.views[1].omit = ['c_*'];
  write('ord.kerf.json', base);
  r = kerf(['check', 'ord.kerf.json']);
  check('an omit glob that matches nothing warns W_REF_UNKNOWN (not an error)', r.code === 0 && /^WARN W_REF_UNKNOWN B: view B omits 'c_\*', a pattern that matches no component id, so it has no effect Fix: /m.test(r.out), r.out);
  const ops = [
    { op: 'update', path: 'views/A', value: { omit: ['c_*', 'c_extra'] } },
    { op: 'add', path: 'components', value: { id: 'c_extra', type: 'lumber', size: '2x4', run: 'z', at: { anchor: 'bottom_left', to: [0, 0] }, views: ['B'] } },
  ];
  write('ord-ops.json', ops);
  r = kerf(['apply', 'ord.kerf.json', 'ord-ops.json', '-w', '--why', 'scenario c']);
  check('omit naming an id the same batch adds later: applies, and the glob now matches', r.code === 0 && !/W_REF_UNKNOWN A/.test(r.out) && /wrote ord\.kerf\.json/.test(r.out), r.out + r.err);
  write('ord-ops2.json', [{ op: 'update', path: 'views/B', value: { omit: ['nope'] } }]);
  r = kerf(['apply', 'ord.kerf.json', 'ord-ops2.json']);
  check('an unknown literal id in omit: W_REF_UNKNOWN with a suggestion, the apply still succeeds', r.code === 0 && /^WARN W_REF_UNKNOWN B: view B omits 'nope', which is not a component id \(yet\)/m.test(r.out), r.out + r.err);

  const ack = {
    kerf: '0.1', id: 'ack', title: 'ACK',
    components: [
      { id: 'cmu', type: 'cmu_wall', width: 8, courses: 3, bond_beam_courses: 1, at: { to: [0, 0] } },
      { id: 'sill', type: 'lumber', size: '2x6', orient: 'flat', at: { anchor: 'bottom_left', to: 'cmu@top_left', offset: [1, 0] },
        acknowledge: [{ code: 'W_UNTREATED_CONTACT', reason: 'sill sealer by others' }, { code: 'W_OVERLAP', reason: 'scenario alternates' }, { code: 'W_UNTREATED_CONTACT', reason: 'again' }] },
      { id: 'blk', type: 'lumber', size: '2x4', run: 'x', length: 4, at: { anchor: 'bottom_left', to: 'sill@top_left' }, acknowledge: [{ code: 'I_SOLID_USED', reason: 'n/a' }] },
    ],
    views: [{ id: 'A', scale: '3"=1\'-0"', crop: { x: [-6, 16], y: [10, 30] }, cut_z: 0, annotations: [] }],
  };
  write('ack.kerf.json', ack);
  r = kerf(['check', 'ack.kerf.json']);
  check('summary DOC line counts acknowledged warnings next to errors and warnings', r.code === 0 && /^DOC ack  3 components  1 view  0 errors  2 warnings  1 acknowledged$/m.test(r.out), r.out.split('\n')[0]);
  check('default output lists acknowledged warnings by code with component and reason', /^ACKNOWLEDGED  1 warning suppressed by `acknowledge`/m.test(r.out) && /^ W_UNTREATED_CONTACT +x1 +sill: "sill sealer by others"$/m.test(r.out) && !/^INFO I_ACK/m.test(r.out), r.out);
  check('W_ACK_UNUSED: one line per component, says why each entry is stale, fix is the exact update op', /^WARN W_ACK_UNUSED sill: 2 of the 3 acknowledge entries of 'sill' suppress nothing \(no W_OVERLAP \(#1\) names 'sill' in any view; W_UNTREATED_CONTACT \(#2\) repeats an earlier entry\).* Fix: remove them: \{"op":"update","path":"components\/sill","value":\{"acknowledge":\[\{"code":"W_UNTREATED_CONTACT","reason":"sill sealer by others"\}\]\}\}$/m.test(r.out), r.out);
  check('W_ACK_UNUSED: an acknowledged I_* code suppresses nothing; acknowledge: null removes the only entry', /^WARN W_ACK_UNUSED blk: 1 of the 1 acknowledge entry of 'blk' suppresses nothing \(I_SOLID_USED \(#0\) is info, never suppressed\).*"value":\{"acknowledge":null\}\}$/m.test(r.out), r.out);
  const fixOp = /Fix: remove them: (\{.*\})$/m.exec(r.out);
  write('fix.json', fixOp ? [JSON.parse(fixOp[1])] : []);
  r = kerf(['apply', 'ack.kerf.json', 'fix.json', '-w', '--why', 'drop stale acks']);
  check('applying the W_ACK_UNUSED fix removes the stale entries and keeps the acknowledgement', r.code === 0 && !/W_ACK_UNUSED sill/.test(r.out) && /1 acknowledged$/m.test(r.out), r.out + r.err);
  const logs = fs.readFileSync(path.join(tmp, 'ack.kerf.json.log.jsonl'), 'utf8').trim().split('\n').map((l) => JSON.parse(l));
  check('the op log keeps the acknowledged list (code, component, reason)', logs.at(-1).ack && /W_UNTREATED_CONTACT +x1 +sill: "sill sealer by others"/.test(logs.at(-1).ack[0]), logs.at(-1));
  r = kerf(['check', 'ack.kerf.json', '--strict']);
  check('--strict: the acknowledged warning is a warning again (with who and why), exit 1', r.code === 1 && /^DOC ack .* 0 errors  2 warnings  1 acknowledged \(--strict: listed as warnings\)$/m.test(r.out) && /^WARN W_UNTREATED_CONTACT sill: .*\[acknowledged on 'sill': sill sealer by others\]/m.test(r.out) && !/^ACKNOWLEDGED/m.test(r.out), r.out);
  r = kerf(['check', 'zb.kerf.json', '--strict']);
  check('--strict exits 1 on a plain warning too', r.code === 1, r.out.split('\n')[0]);
  r = kerf(['export', 'ack.kerf.json', '--view', 'A', '--strict', '--format', 'svg', '-o', 'x.svg']);
  check('--strict outside check is a usage error', r.code === 2 && /--strict only applies to `kerf check`/.test(r.err), r.err);
  const cj = JSON.parse(kerf(['call', 'check'], { input: JSON.stringify({ doc: readJson('ack.kerf.json') }) }).out);
  check('check JSON: acknowledged list with code, id, reason and the original message', cj.acknowledged.length === 1 && cj.acknowledged[0].code === 'W_UNTREATED_CONTACT' && cj.acknowledged[0].id === 'sill' && cj.acknowledged[0].reason === 'sill sealer by others' && /untreated wood 'sill'/.test(cj.acknowledged[0].message), cj.acknowledged);
  const ir = JSON.parse(kerf(['drawing', 'ack.kerf.json', '--view', 'A']).out);
  check('drawing IR carries the acknowledged list of its view', Array.isArray(ir.acknowledged) && ir.acknowledged.length === 1 && ir.acknowledged[0].id === 'sill', ir.acknowledged);
  check('drawing IR of a document without acknowledge has no acknowledged key', !('acknowledged' in JSON.parse(kerf(['drawing', 'zb.kerf.json', '--view', 'A']).out)));
  r = kerf(['export', 'ack.kerf.json', '--view', 'A', '--format', 'dxf', '--sheet', '-o', 'plain.dxf']);
  const plain = fs.readFileSync(path.join(tmp, 'plain.dxf'), 'utf8');
  r = kerf(['export', 'ack.kerf.json', '--view', 'A', '--format', 'dxf', '--sheet', '--review', '-o', 'review.dxf']);
  const rev = fs.readFileSync(path.join(tmp, 'review.dxf'), 'utf8');
  check('export --review stamps the sheet, tags the component and lists the reason; the default sheet has none of it', r.code === 0 && /\n1 ACKNOWLEDGED WARNING\r?\n/.test(rev) && /\nACK 1\r?\n/.test(rev) && /\nACK 1  SILL: W_UNTREATED_CONTACT X1 SILL SEALER BY OTHERS\r?\n/.test(rev) && !/ACKNOWLEDGED|ACK 1/.test(plain), r.err);
  r = kerf(['export', 'ack.kerf.json', '--view', 'A', '--format', 'png', '--review', '-o', 'review.png']);
  check('export --review works for png without a sheet', r.code === 0 && fs.statSync(path.join(tmp, 'review.png')).size > 1000, r.err);
  r = kerf(['check', 'ack.kerf.json', '--review']);
  check('--review outside export is a usage error', r.code === 2 && /--review only applies to `kerf export`/.test(r.err), r.err);
}

// ---------------------------------------------------------------------------------------------------
// #19: papercuts from the MASS details dogfood run (checks, drawing, docs and CLI)
section('19. #19 papercuts: check false positives, diagnostics that name their knob, drawing and docs');
{
  const doc = (components, annotations = [], view = {}) => ({ kerf: '0.1', id: 'pc', title: 'PC', run: [-24, 24], components, views: [{ id: 'A', kind: 'section', annotations, ...view }] });
  const warnLines = (out) => out.split('\n').filter((l) => l.startsWith('WARN '));
  const rim = { id: 'rim', type: 'lumber', size: '2x10', at: { to: [0, 0] } };
  const sole = { id: 'sole', type: 'lumber', size: '2x6', orient: 'flat', at: { anchor: 'bottom_left', to: { ref: 'rim@top_left', offset: [0, 0.75] } } };
  const ply = { id: 'subfloor', type: 'panel', material: 'plywood', thickness: 0.75, run: 'x', length: 24, at: { anchor: 'bottom_left', to: 'rim@top_left' } };
  write('pc1.kerf.json', doc([rim, sole]));
  let r = kerf(['check', 'pc1.kerf.json']);
  check('W_NEAR_MISS: rim and sole with an empty 3/4" gap warn', /^WARN W_NEAR_MISS rim: 'rim' and 'sole' leave a 3\/4" gap/m.test(r.out), r.out);
  write('pc1.kerf.json', doc([rim, ply, sole]));
  r = kerf(['check', 'pc1.kerf.json']);
  check('W_NEAR_MISS: a 3/4" subfloor filling the gap counts as contact', !/W_NEAR_MISS/.test(r.out), r.out);

  const plate = { id: 'plate', type: 'lumber', size: '2x4', run: 'x', face: 'narrow', length: 48, at: { to: [0, 0] } };
  const studs = { id: 'studs', type: 'lumber', size: '2x4', run: 'y', face: 'narrow', length: 30, array: { axis: 'x', count: 2, spacing: 16 }, at: { anchor: 'bottom_left', to: [0, 1.5] } };
  const jack = { id: 'jack', type: 'lumber', size: '2x4', run: 'y', face: 'narrow', length: 30, at: { anchor: 'bottom_left', to: [20.5, 1.5] } };
  write('pc2.kerf.json', doc([plate, studs, jack]));
  r = kerf(['check', 'pc2.kerf.json']);
  check('W_NEAR_MISS: studs @ 16" o.c. 3" from a jack stud are framing, not a near miss', !/W_NEAR_MISS/.test(r.out), r.out);
  write('pc2.kerf.json', doc([plate, studs, { ...jack, at: { anchor: 'bottom_left', to: [18, 1.5] } }]));
  r = kerf(['check', 'pc2.kerf.json']);
  check('W_NEAR_MISS: a 1/2" gap between parallel studs still warns, with a move fix', /^WARN W_NEAR_MISS .*run parallel with a 1\/2" gap along x .*Fix: move '/m.test(r.out), r.out);

  const slab = (id, x) => ({ id, type: 'concrete', shape: 'rect', width: 30, height: 6, at: { to: [x, 0] }, cover: { bottom: 1.5, top: 1.5, sides: 1.5 } });
  write('pc3.kerf.json', doc([slab('slab_l', 0), slab('slab_r', 30.5)]));
  r = kerf(['check', 'pc3.kerf.json']);
  check('W_FLOATING: two slab halves across a 1/2" joint touch', !/W_FLOATING/.test(r.out), r.out);
  write('pc3.kerf.json', doc([slab('slab_l', 0), slab('slab_r', 32)]));
  r = kerf(['check', 'pc3.kerf.json']);
  check('W_FLOATING: a 2" gap between slabs still floats', /^WARN W_FLOATING slab_l/m.test(r.out), r.out);

  write('pc4.kerf.json', doc([{ id: 'stud', type: 'lumber', size: '2x4', run: 'y', length: 30 }], [], { crop: { x: [-10, 40], y: [-12, 40] } }));
  r = kerf(['apply', 'pc4.kerf.json', '--ops', JSON.stringify({ op: 'add', path: 'components', value: { id: 'wall', type: 'lumber', size: '2x4', run: 'x', length: 100, at: { to: [-6, 32] } } }), '-w', '--why', 'wall cut by the crop']);
  check('W_CROP_STALE: a new member the crop cuts on purpose (more than 10% inside) is quiet', r.code === 0 && !/W_CROP_STALE/.test(r.out), r.out);

  const keyed = doc([plate], [
    { id: 'n1', type: 'note', key: 1, text: 'PLATE', target: 'plate', cite: [{ code: 'IRC', edition: 2021, section: 'R602.3' }] },
    { id: 'n2', type: 'note', key: 1, target: 'plate' },
    { id: 'n3', type: 'note', key: 1, text: 'PLATE', target: 'plate' },
  ], { notes_style: 'keyed' });
  write('pc5.kerf.json', keyed);
  r = kerf(['check', 'pc5.kerf.json']);
  check('shared key without text takes the cite too: no W_PARAM for n2', !/W_PARAM n2/.test(r.out), r.out);
  check('shared key with the same text but no cite: W_PARAM names "cite"', /^WARN W_PARAM n3: .*shares key 1 and its text with note 'n1' but its "cite" differs/m.test(r.out), r.out);

  write('pc6.kerf.json', doc([{ id: 'slab', type: 'concrete', shape: 'polygon', points: [[0, 0], [48, 0], [48, 4], [0, 4]], at: { to: [0, 0] } }, { id: 'bar', type: 'rebar', size: '#3', at: { anchor: 'center', to: [24, 2] } }]));
  r = kerf(['check', 'pc6.kerf.json']);
  check('W_COVER names its knob and that the default cannot fit a 4" slab', /^WARN W_COVER bar: .*< required 3" \(cover\.bottom, default\).*Fix: .*set cover\.bottom of 'slab' to 1\.8125: \{"op": "update", "path": "components\/slab", "value": \{"cover": \{"bottom": 1\.8125, "top": 1\.5, "sides": 3\}\}\}; 'slab' uses the default cover .* 4" depth/m.test(r.out), r.out);

  const ftg = { id: 'stem', type: 'concrete', shape: 'rect', width: 8, height: 24, at: { to: [0, -24] }, cover: { bottom: 3, top: 1.5, sides: 1.5 } };
  const ab = (id, x, z) => ({ id, type: 'anchor_bolt', diameter: 0.75, hook: 'headed', z, at: { to: [x, 0] } });
  write('pc7.kerf.json', doc([ftg, ab('rod', 1.5, 0), ab('ab', 4, 1)]));
  r = kerf(['check', 'pc7.kerf.json']);
  check('anchor_bolt W_BOLT_EDGE: 1 1/8" clear of the concrete edge (ACI 318-19 17.9.2)', /^WARN W_BOLT_EDGE rod: anchor bolt 'rod': the shaft is 1 1\/8" clear of the edge of 'stem'/m.test(r.out), r.out);
  check('anchor_bolt W_BOLT_SPACING: 2.7" apart < 4d = 3"', /^WARN W_BOLT_SPACING rod: anchor bolts 'rod' and 'ab' are .* below the ACI 318-19 17\.9\.2 minimum of 4d = 3"/m.test(r.out), r.out);
  write('pc7.kerf.json', doc([ftg, ab('ab', 4, 0)]));
  r = kerf(['check', 'pc7.kerf.json']);
  check('an anchor centered in an 8" stem: no bolt warning', !/W_BOLT/.test(r.out), r.out);

  const far = { id: 'far', type: 'lumber', size: '2x4', run: 'y', length: 30, at: { to: [300, 0] } };
  write('pc8.kerf.json', doc([plate, studs, far], [], { scale: '1"=1\'-0"' }));
  r = kerf(['check', 'pc8.kerf.json']);
  check('W_VIEW_FIT names the component that stretches the auto crop and gives the omit op', /^WARN W_VIEW_FIT A: .*because 'far' lie apart from the main group.*Fix: 1\) if view A should not draw them .*"omit":\["far"\]/m.test(r.out), r.out);

  const stl = { kerf: '0.1', id: 'pcw', title: 'W', components: [
    { id: 'plate', type: 'steel', shape: 'PL3/4x10', length: 10, at: { to: [0, 0] } },
    { id: 'col', type: 'steel', shape: 'HSS4x4x1/4', orient: 'elevation', run: 'y', length: 24, at: { anchor: 'bottom_center', to: 'plate@face_top' } },
    { id: 'nailer', type: 'lumber', size: '3x8', orient: 'flat', at: { anchor: 'bottom_left', to: 'col@top_left' } }],
    views: [{ id: 'A', kind: 'section', cut_z: 0, annotations: [
      { id: 'w1', type: 'weld', at: 'col@bottom_right', offset: [6, 6], size: '3/16', length: 2, pitch: 12, tail: 'EA. END', members: ['col', 'plate'] },
      { id: 'w2', type: 'weld', at: 'col@bottom_left', offset: [-6, 6], size: '3/16', members: ['col', 'nailer'] }] }] };
  write('pc9.kerf.json', stl);
  r = kerf(['check', 'pc9.kerf.json']);
  check('weld members: a lumber member is W_PARAM (not steel)', /^WARN W_PARAM w2: weld 'w2' in view A: member 'nailer' is lumber, not steel/m.test(r.out), r.out);
  check('3x8 is a sawn size (2 1/2" thick)', /nailer +lumber 3x8 flat run z +x -?[0-9 /"']+\.\.[0-9 /"'-]+ +y 2'-0 3\/4"\.\.2'-3 1\/4"/m.test(r.out), r.out);
  r = kerf(['drawing', 'pc9.kerf.json', '--view', 'A']);
  let items = r.code === 0 ? JSON.parse(r.out).items : [];
  check('weld length and pitch print as 2-12 right of the symbol', items.some((i) => i.src === 'w1' && i.t === 'text' && i.s === '2-12' && i.align === 'left'), r.err);
  stl.views[0].annotations = [{ id: 'w3', type: 'weld', at: 'col@bottom_right', size: '3/16', pitch: 12 }];
  write('pc9.kerf.json', stl);
  r = kerf(['check', 'pc9.kerf.json']);
  check('weld pitch without length: E_PARAM with the fix', /^ERROR E_PARAM w3: weld 'w3': "pitch" needs "length".*Fix: add "length"/m.test(r.out), r.out);

  write('pc10.kerf.json', doc([plate], [
    { id: 'l1', type: 'label', text: 'EXTERIOR\nFACE', at: [0, 12], align: 'left' },
    { id: 'd1', type: 'dim', from: 'plate@top_left', to: { ref: 'plate@top_left', offset: [0.5, 0] }, dir: 'h', offset: 3 },
    { id: 'n1', type: 'note', text: 'SILL PLATE ANCHORED TO THE FOOTING, LAP SPLICE 24" MIN.', target: 'plate' }], { scale: '1"=1\'-0"' }));
  r = kerf(['drawing', 'pc10.kerf.json', '--view', 'A']);
  items = r.code === 0 ? JSON.parse(r.out).items : [];
  const lab = items.filter((i) => i.src === 'l1' && i.t === 'text');
  check('label: a newline breaks the line, align left puts each line start on `at`', lab.length === 2 && lab[0].s === 'EXTERIOR' && lab[1].s === 'FACE' && lab.every((t) => t.align === 'left' && t.x === 0) && lab[0].y > lab[1].y, lab);
  check('a dimension too short for its ticks draws dot terminators', items.filter((i) => i.src === 'd1' && i.t === 'fill').length === 2 && !items.some((i) => i.src === 'd1' && i.pen === 'profile'), items.filter((i) => i.src === 'd1'));
  check('note wrap keeps 24" MIN. on one line', items.some((i) => i.src === 'n1' && i.t === 'text' && /24" MIN\.$/.test(i.s)) && !items.some((i) => i.src === 'n1' && i.t === 'text' && /^MIN\./.test(i.s)), items.filter((i) => i.src === 'n1' && i.t === 'text').map((i) => i.s));
  write('pc10.kerf.json', doc([plate], [{ id: 'l2', type: 'label', text: 'X', at: [0, 0], align: 'middle' }]));
  r = kerf(['check', 'pc10.kerf.json']);
  check('label align must be left|center|right', /^ERROR E_PARAM l2: label 'l2': "align" must be "left", "center" or "right" \(got "middle"\)/m.test(r.out), r.out);

  write('pc11.kerf.json', doc([plate, { id: 'post', type: 'lumber', size: '8x8', length: 24, at: { anchor: 'bottom_left', to: 'plate@top_left' } }]));
  r = kerf(['check', 'pc11.kerf.json']);
  check('run z lumber takes length (summary L=) and 8x8 is 7 1/2" square', /^ post +lumber 8x8 upright run z L=2'-0" +x 0"\.\.7 1\/2" +y 1 1\/2"\.\.9"/m.test(r.out), r.out);
  r = kerf(['mesh', 'pc11.kerf.json']);
  const mz = r.code === 0 ? JSON.parse(r.out) : null;
  const pz = mz ? mz.parts.filter((p) => p.src === 'post').flatMap((p) => p.positions.filter((_, k) => k % 3 === 2)) : [];
  check('run z length: the mesh is 24" long along z, centered on z 0 (not the doc run)', pz.length > 0 && Math.min(...pz) === -12 && Math.max(...pz) === 12, [Math.min(...pz), Math.max(...pz)]);

  const arr = doc([{ id: 'pl', type: 'lumber', size: '2x6', orient: 'flat', z: [-8, 24], at: { to: [0, 0] } },
    { id: 'st', type: 'lumber', size: '2x6', run: 'y', length: 24, z: 0, array: { axis: 'z', count: 4, spacing: 16 }, at: { anchor: 'bottom_left', to: 'pl@top_left' } }], [], { cut_z: 0 });
  write('pc12.kerf.json', arr);
  r = kerf(['check', 'pc12.kerf.json']);
  check('W_FLOATING names array instances past the member they sit on', /^WARN W_FLOATING st: array instance\(s\) st#2, st#3 of 'st' touch nothing.*Fix: an array starts at instance 0/m.test(r.out), r.out);

  r = kerf(['schema', 'lumber', 'view', 'label', 'weld']);
  check('schema: 3x and 8x sizes, 3/16" scale, cut_z in front of members, label align, weld length/pitch', /"3x8"/.test(r.out) && /3\/16\\?"=1'-0\\?"/.test(r.out) && /wholly in front of it \(z0 >= cut_z\) are not drawn/.test(r.out) && /^- align: left\|center\|right, default center/m.test(r.out) && /^- pitch: /m.test(r.out), r.out.slice(0, 300));
  r = kerf(['version']);
  check('kerf version stamps the build (git describe)', r.code === 0 && /^\{"engine":"kerf","version":"[^"]+","spec":"[^"]+","build":"[^"]+"\}$/m.test(r.out), r.out);
}

// ---------------------------------------------------------------------------------------------------
// issue #14: subtracted solids: fills clipped under solids, concrete voids, sleeves (SPEC 26.4)
section('#14 fills under solids, voids around embedded members, sleeves');
{
  const doc = (components, annotations = [], view = {}) => ({ kerf: '0.1', id: 'v', title: 'V', run: [-12, 12], components, views: [{ id: 'A', kind: 'section', title: 'V', annotations, ...view }] });
  const crop = { scale: '1"=1\'-0"', crop: { x: [-30, 30], y: [-40, 16] } };
  const area = (loops) => loops.reduce((b, l) => b + l.reduce((c, p, k) => { const q = l[(k + 1) % l.length]; return c + (p[0] * q[1] - q[0] * p[1]) / 2; }, 0), 0);
  const items = (r) => (r.code === 0 ? JSON.parse(r.out).items : []);
  const srcArea = (its, t, src) => its.filter((i) => i.t === t && i.src === src).reduce((a, i) => a + area(i.loops), 0);
  const ftg = { id: 'ftg', type: 'concrete', shape: 'footing', width: 24, height: 10, at: { anchor: 'top_center', to: [0, -24] } };
  const stem = { id: 'stem', type: 'concrete', shape: 'rect', width: 8, height: 36, at: { anchor: 'bottom_center', to: 'ftg@top_center' } };
  const earth = { id: 'earth', type: 'fill', material: 'earth', points: [[-30, 0], [30, 0], [30, -40], [-30, -40]] };
  write('v1.kerf.json', doc([ftg, stem, earth], [], crop));
  let r = kerf(['check', 'v1.kerf.json']);
  check('an earth rectangle around a footing and stem: 0 warnings (a fill over a solid is not W_OVERLAP)', r.code === 0 && /0 errors +0 warnings/.test(r.out), r.out);
  r = kerf(['drawing', 'v1.kerf.json', '--view', 'A']);
  let a = srcArea(items(r), 'hatch', 'earth');
  check('the earth hatch is the rectangle minus the footing and the buried stem (2400 - 240 - 192 sq in)', Math.abs(a - 1968) < 0.01, a);
  check('the grade line stops at the stem', items(r).filter((i) => i.t === 'path' && i.src === 'earth').map((i) => JSON.stringify(i.pts)).join(' ') === '[[30,0],[4,0]] [[-4,0],[-30,0]]', items(r).filter((i) => i.t === 'path' && i.src === 'earth'));
  write('v1.kerf.json', doc([ftg, stem, { ...earth, except: ['stem'] }], [], crop));
  a = srcArea(items(kerf(['drawing', 'v1.kerf.json', '--view', 'A'])), 'hatch', 'earth');
  check('fill.except: the stem no longer clips the earth', Math.abs(a - 2160) < 0.01, a);
  write('v1.kerf.json', doc([ftg, stem, { ...earth, except: ['stme'] }]));
  r = kerf(['check', 'v1.kerf.json']);
  check('fill.except naming no component: E_REF_UNKNOWN with the nearest id', r.code === 1 && /ERROR E_REF_UNKNOWN earth: fill 'earth' except: no component with id 'stme'; did you mean 'stem'\?/.test(r.out), r.out);

  const grout = { id: 'grout', type: 'concrete', material: 'grout', at: { to: 'ftg@top_center' }, z: [-7.5, 7.5], shape: 'polygon', points: [[-7.5, 0], [7.5, 0], [6, 1.5], [-6, 1.5]] };
  const plate = { id: 'plate', type: 'steel', at: { anchor: 'face_bottom', to: 'grout@top_center' }, shape: 'PL3/4x12', length: 12 };
  const col = { id: 'col', type: 'steel', at: { anchor: 'bottom_center', to: 'plate@face_top' }, shape: 'HSS6x6x1/4', orient: 'elevation', run: 'y', length: 36 };
  const pour = { id: 'pour', type: 'concrete', at: { to: [0, 0] }, shape: 'polygon', points: [[-12, 0], [12, 0], [12, -24], [-12, -24]] };
  write('v2.kerf.json', doc([ftg, grout, plate, col, pour]));
  r = kerf(['check', 'v2.kerf.json']);
  check('a pour around a column without voids: W_OVERLAP whose fix is the voids op', /WARN W_OVERLAP col: 'col' and 'pour' overlap .*Fix: if 'col' is cast into 'pour', subtract it \(the hole follows the member\): \{"op": "update", "path": "components\/pour", "value": \{"voids": \["col"\]\}\}/.test(r.out), r.out);
  write('v2.kerf.json', doc([ftg, grout, plate, col, { ...pour, voids: ['col', 'plate', 'grout'] }]));
  r = kerf(['check', 'v2.kerf.json']);
  check('pour.voids [col, plate, grout]: 0 warnings, the summary names what the pour lost', r.code === 0 && /0 errors +0 warnings/.test(r.out) && /^ pour +concrete polygon minus col, plate, grout /m.test(r.out), r.out);
  a = srcArea(items(kerf(['drawing', 'v2.kerf.json', '--view', 'A'])), 'region', 'pour');
  check('the cut pour is 576 sq in less the column (6 x 21.75), plate (12 x 0.75) and grout (20.25)', Math.abs(a - (576 - 130.5 - 9 - 20.25)) < 0.01, a);
  r = kerf(['apply', 'v2.kerf.json', '--ops', JSON.stringify([{ op: 'update', path: 'components/col', value: { shape: 'HSS8x8x1/4' } }]), '-w', '--why', 'bigger column']);
  a = srcArea(items(kerf(['drawing', 'v2.kerf.json', '--view', 'A'])), 'region', 'pour');
  check('resizing the column needs no change to the pour: still 0 warnings, the hole is 8" wide', r.code === 0 && /0 errors +0 warnings/.test(r.out) && Math.abs(a - (576 - 174 - 9 - 20.25)) < 0.01, [a, r.out]);
  r = kerf(['apply', 'v2.kerf.json', '--ops', JSON.stringify([{ op: 'remove', path: 'components/col' }])]);
  check('removing a voided member names the host that subtracts it', r.code === 1 && /cannot remove 'col': still referenced by pour/.test(r.err), r.err);
  r = kerf(['mesh', 'v2.kerf.json']);
  const mz = r.code === 0 ? JSON.parse(r.out).parts.filter((p) => p.src === 'pour').map((p) => [Math.min(...p.positions.filter((_, k) => k % 3 === 2)), Math.max(...p.positions.filter((_, k) => k % 3 === 2))]) : [];
  const slabs = [...new Set(mz.map((z) => z.join()))];
  check('3D: the pour is split into 7 slabs where the column, plate and grout end along z (the column slab in two pieces)', slabs.join(' ') === '-12,-7.5 -7.5,-6 -6,-4 -4,4 4,6 6,7.5 7.5,12' && mz.length === 8 && Math.min(...mz.map((z) => z[0])) === -12 && Math.max(...mz.map((z) => z[1])) === 12, mz);

  write('v3.kerf.json', doc([{ ...pour, voids: ['colum', 'pour', 'ftg.web', { rect: [2, 2] }, { circle: 2, at: [100, 100] }, { circle: 2, at: [0, -6], bogus: 1 }] }, ftg]));
  r = kerf(['check', 'v3.kerf.json']);
  check('voids naming a missing component: E_REF_UNKNOWN with the nearest id', /ERROR E_REF_UNKNOWN pour: 'pour' voids: no component with id 'colum'; did you mean 'pour'\?/.test(r.out) || /ERROR E_REF_UNKNOWN pour: 'pour' voids: no component with id 'colum'/.test(r.out), r.out);
  check('a host voiding itself is E_PARAM', /ERROR E_PARAM pour: 'pour' cannot void itself/.test(r.out), r.out);
  check('a missing part lists the parts', /ERROR E_REF_UNKNOWN pour: 'pour' voids 'ftg\.web': component 'ftg' has no part 'web'\. Parts: footing/.test(r.out), r.out);
  check('a shape without at, and an unknown key, are E_PARAM with an example', /ERROR E_PARAM pour: 'pour' voids\[3\]: a rect needs "at"/.test(r.out) && /ERROR E_PARAM pour: 'pour' voids\[5\]: unknown key 'bogus'; a void shape takes rect, circle, points, at, anchor, z Fix: "voids": \[/.test(r.out), r.out);
  check('a void that misses its host: W_VOID_MISS with both boxes and the op that drops it', /WARN W_VOID_MISS pour: void voids\[4\] \(circle\) of 'pour' removes nothing: the void \(x 8'-3"\.\.8'-5", y 8'-3"\.\.8'-5", through\) and 'pour' .*their outlines do not overlap Fix: move the void or 'pour' so they overlap, or remove the entry: \{"op": "update", "path": "components\/pour", "value": \{"voids": \[/.test(r.out), r.out);
  write('v3.kerf.json', doc([ftg, { id: 'pc', type: 'concrete', shape: 'rect', width: 4, height: 4, z: [-6, 6], at: { to: [40, 0] } }, { ...pour, voids: ['pc'] }]));
  r = kerf(['check', 'v3.kerf.json']);
  check('W_VOID_MISS names a component void and the z ranges', /WARN W_VOID_MISS pour: void 'pc' of 'pour' removes nothing/.test(r.out), r.out);

  const wall = { id: 'wall', type: 'concrete', shape: 'rect', width: 72, height: 24, at: { to: [0, 0] } };
  const sl = { id: 'sl', type: 'sleeve', pipe: 4, fill: 'joint_filler', at: { to: [36, 12] } };
  write('v4.kerf.json', doc([wall, sl]));
  r = kerf(['check', 'v4.kerf.json']);
  check('a sleeve inside concrete without through: W_OVERLAP whose fix sets through', /WARN W_OVERLAP wall: 'wall' and 'sl' overlap .*Fix: if 'sl' passes through 'wall', cut its hole: \{"op": "update", "path": "components\/sl", "value": \{"through": "wall"\}\}/.test(r.out), r.out);
  write('v4.kerf.json', doc([wall, { ...sl, through: 'wall' }, { id: 'bar', type: 'rebar', size: '#4', at: { anchor: 'center', to: [36, 16] } }], [{ id: 'n1', type: 'note', text: 'PIPE', target: 'sl.pipe' }]));
  r = kerf(['check', 'v4.kerf.json']);
  check('sleeve through the wall: 5 1/2" OD (4" pipe, 1/2" clearance, 1/4" wall), the wall loses it', /^ sl +sleeve 5 1\/2" OD x 1\/4" wall, pipe 4", joint_filler fill run z through wall /m.test(r.out) && /^ wall +concrete rect 6'-0" x 2'-0" minus sleeve sl /m.test(r.out) && !/W_OVERLAP/.test(r.out), r.out);
  check('W_COVER measures to the sleeve: a bar 1" clear above it', /WARN W_COVER bar: clear cover from .* to the bottom of 'wall' is 1" < required 3" \(cover\.bottom, default\)/.test(r.out), r.out);
  const its = items(kerf(['drawing', 'v4.kerf.json', '--view', 'A']));
  const reg = its.filter((i) => i.t === 'region' && i.src === 'wall');
  check('section: the wall is cut with a hole (outer + one hole loop), the sleeve and pipe walls fill, the packing hatches, the note lands', reg.length === 1 && reg[0].loops.length === 2 && its.some((i) => i.t === 'fill' && i.src === 'sl') && its.some((i) => i.t === 'hatch' && i.src === 'sl' && i.pattern === 'KERF-FILLER') && its.some((i) => i.t === 'path' && i.src === 'n1'), reg.map((x) => x.loops.length));
  write('v4.kerf.json', doc([wall, { id: 'stud', type: 'lumber', size: '2x4', at: { to: [-20, 0] } }, { ...sl, through: ['stud'] }]));
  r = kerf(['check', 'v4.kerf.json']);
  check('a sleeve through a non-host: E_PARAM naming the types that take a sleeve', /ERROR E_PARAM sl: sleeve 'sl' through 'stud': 'stud' is a lumber; only concrete and cmu_wall take a sleeve or void/.test(r.out), r.out);
  write('v4.kerf.json', doc([{ ...wall, width: 8, height: 36 }, { ...sl, pipe: 3, run: 'x', length: 8, through: 'wall', at: { anchor: 'middle_left', to: [0, 18] } }]));
  r = kerf(['check', 'v4.kerf.json']);
  check('a sleeve seen lengthwise (run x) through an 8" wall: 0 warnings', r.code === 0 && /0 errors +0 warnings/.test(r.out) && /run x L=8" through wall/.test(r.out), r.out);
  write('v4.kerf.json', doc([{ ...sl, run: 'x' }]));
  r = kerf(['check', 'v4.kerf.json']);
  check('a sleeve along x without length: E_PARAM', r.code === 1 && /ERROR E_PARAM sl: .*length/.test(r.out), r.out);
  r = kerf(['schema', 'sleeve', 'concrete', 'fill']);
  check('schema documents sleeve.through, concrete.voids and fill.except', /through/.test(r.out) && /clearance/.test(r.out) && /^- voids/m.test(r.out) && /^- except/m.test(r.out), r.out.slice(0, 400));
}

console.log(`\n${passed} passed, ${failed} failed`);
process.exit(failed ? 1 : 0);
