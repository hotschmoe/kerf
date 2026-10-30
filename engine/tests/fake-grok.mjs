#!/usr/bin/env node
// Stand-in for the `grok` binary (Grok Build), started through a shim named like a real npm install (`grok.cmd` on
// Windows, an executable `grok` script elsewhere). `grok --version` is answered by the shim; this file handles the run:
//   grok -p <message> --output-format streaming-json --always-approve --cwd <dir> [--resume <id>]
// Message keyword WORK<n>: n rounds of "stream a line, then `kerf apply -w` one component into the document" (one round
// every 2 s), so a run of WORK15 is a 30 s agent session that edits the folder while a browser is looking at it.
import { execFileSync } from 'node:child_process';

const args = process.argv.slice(2);
const message = args[args.indexOf('-p') + 1] ?? '';
const out = (o) => process.stdout.write(JSON.stringify(o) + '\n');
out({ type: 'system', subtype: 'init', session_id: 'grok-sess-1', cwd: process.cwd() });
const tag = Math.random().toString(36).slice(2, 6); // ids differ between runs of one document
const rounds = +(/WORK(\d+)/.exec(message)?.[1] ?? 1);
const file = /Current document: ([^\s]+?\.kerf\.json)\./.exec(message)?.[1] ?? 'a.kerf.json';
for (let i = 0; i < rounds; i++) {
  out({ type: 'assistant', message: { role: 'assistant', content: [{ type: 'text', text: `round ${i}` }] } });
  const ops = JSON.stringify([{ op: 'add', path: 'components', value: { id: 'g' + tag + i, type: 'lumber', size: '2x4', at: { x: i * 10, y: 0 } } }]);
  const text = execFileSync('kerf', ['apply', file, '--ops', ops, '-w', '--why', 'grok round ' + i], { encoding: 'utf8' });
  out({ type: 'tool_result', text: text.split('\n')[0] });
  await new Promise((r) => setTimeout(r, 2000));
}
out({ type: 'result', subtype: 'success', is_error: false, session_id: 'grok-sess-1', result: 'done' });
