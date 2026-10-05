#!/usr/bin/env node
// A scripted stand-in for `claude -p --output-format stream-json --verbose` used by the REAL-server e2e (via <dir>/.kerf/agents.json).
// It emits Claude-Code-shaped events and really edits the document through the `kerf` CLI (found on PATH, as for a real agent).
//   node fake-agent.mjs "<message>" [--resume SESSION]      keywords: EDIT (edit the first note), SLOW (take ~6 s), FAIL (a failing command)
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';

const args = process.argv.slice(2);
const message = args[0] ?? '';
const ri = args.indexOf('--resume');
const sid = ri >= 0 ? args[ri + 1] : 'fake-sess-1';
const out = (o) => process.stdout.write(JSON.stringify({ ...o, session_id: sid }) + '\n');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const step = /SLOW/.test(message) ? 700 : 40;
process.on('SIGTERM', () => process.exit(143));

out({ type: 'system', subtype: 'init', cwd: process.cwd() });
await sleep(step);
out({ type: 'assistant', message: { role: 'assistant', content: [{ type: 'text', text: 'Reading the guide, then editing the detail.' }] } });
const tool = async (id, name, input, run) => {
  await sleep(step);
  out({ type: 'assistant', message: { role: 'assistant', content: [{ type: 'tool_use', id, name, input }] } });
  await sleep(step);
  let content, is_error = false;
  try { content = run(); } catch (e) { content = String(e.stdout || e.message); is_error = true; }
  out({ type: 'user', message: { role: 'user', content: [{ type: 'tool_result', tool_use_id: id, content, is_error }] } });
};
await tool('toolu_1', 'Bash', { command: 'kerf guide | head -5', description: 'Read the guide' }, () => execFileSync('sh', ['-c', 'kerf guide | head -5'], { encoding: 'utf8' }));
const file = /Current document: ([^\s]+?\.kerf\.json)\./.exec(message)?.[1];
if (/FAIL/.test(message)) {
  await tool('toolu_2', 'Bash', { command: 'kerf apply nope.kerf.json --ops \'[]\' -w' }, () => execFileSync('kerf', ['apply', 'nope.kerf.json', '--ops', '[]', '-w'], { encoding: 'utf8', stdio: 'pipe' }));
} else if (/EDIT/.test(message) && file) {
  const doc = JSON.parse(fs.readFileSync(file, 'utf8'));
  const n = doc.views[0].annotations.find((a) => a.type === 'note');
  const ops = [{ op: 'update', path: `views/${doc.views[0].id}/annotations/${n.id}`, value: { text: `${n.text} (AGENT)` } }];
  await tool('toolu_3', 'Bash', { command: `kerf apply ${file} -w --why "Fake agent edit" --ops '${JSON.stringify(ops)}'` },
    () => execFileSync('kerf', ['apply', file, '--ops', JSON.stringify(ops), '-w', '--why', 'Fake agent edit'], { encoding: 'utf8' }));
  await tool('toolu_4', 'Read', { file_path: `${process.cwd()}/${file}` }, () => '{ ...document... }');
}
await sleep(step);
out({ type: 'assistant', message: { role: 'assistant', content: [{ type: 'text', text: 'Done. The note was updated.' }] } });
out({ type: 'result', subtype: 'success', is_error: false, result: 'Done.' });
if (/SLOW/.test(message)) await sleep(8000);
