#!/usr/bin/env node
// Fake headless agent for `kerf serve` bridge tests. Mimics the shape of Claude Code's stream-json:
// a system/init event with session_id, assistant events, plain text lines, a result event.
//   node fake-agent.mjs "<message>" [--resume SESSION]
// Message keywords: EDIT (runs `kerf apply … -w` through PATH, proving the bridge put kerf on it),
// SLOW (prints a line every 200 ms for a minute; dies on SIGTERM), FAIL (exit 3).
// Process-tree modes: TREE (a `sleep 60` grandchild sharing our stdio, then SLOW), STUBBORN (ignores SIGTERM, then SLOW),
// ORPHAN (leaves a `sleep 60` grandchild that holds the stdout pipe, then exits at once), FOREVER (SLOW, ignores SIGTERM,
// and never ends on its own). The `pids` event reports our pid and the grandchild's so a test can check nothing survives.
import { execFileSync, spawn } from 'node:child_process';

const args = process.argv.slice(2);
const message = args[0] ?? '';
const ri = args.indexOf('--resume');
const resume = ri >= 0 ? args[ri + 1] : null;
const sid = resume ?? 'fake-session-1';
const out = (o) => process.stdout.write(JSON.stringify(o) + '\n');

if (message.includes('STUBBORN') || message.includes('FOREVER')) process.on('SIGTERM', () => { process.stderr.write('fake-agent: ignoring SIGTERM\n'); });
else process.on('SIGTERM', () => { process.stderr.write('fake-agent: SIGTERM\n'); process.exit(143); });

out({ type: 'system', subtype: 'init', session_id: sid, cwd: process.cwd(), actor: process.env.KERF_ACTOR ?? null });
if (resume) process.stdout.write(`resumed ${resume}\n`);
process.stdout.write('hello from the fake agent (plain text line)\n');
process.stderr.write('fake-agent: a line on stderr\n');
out({ type: 'assistant', message: { role: 'assistant', content: [{ type: 'text', text: 'working on: ' + message.slice(-60) }] } });

let grandchild = null;
if (/TREE|ORPHAN/.test(message)) {
  grandchild = spawn('sleep', ['60'], { stdio: 'inherit' }); // same process group, same pipes
  out({ type: 'pids', self: process.pid, child: grandchild.pid });
}
if (message.includes('ORPHAN')) process.exit(0);
if (message.includes('EDIT')) {
  const m = /Current document: ([^\s]+?\.kerf\.json)\./.exec(message);
  const file = m ? m[1] : 'a.kerf.json';
  const ops = JSON.stringify([{ op: 'add', path: 'components', value: { id: 'agent_plate', type: 'lumber', size: '2x4', at: { x: 0, y: 0 } } }]);
  // Resolved through PATH (the server prepends its own directory).
  const text = execFileSync('kerf', ['apply', file, '--ops', ops, '-w', '--why', 'fake agent edit'], { encoding: 'utf8' });
  out({ type: 'tool_result', text: text.split('\n')[0] });
}
if (/SLOW|TREE|STUBBORN|FOREVER/.test(message)) {
  for (let i = 0; i < 300; i++) {
    out({ type: 'tick', i });
    await new Promise((r) => setTimeout(r, 200));
  }
}
if (message.includes('FAIL')) { process.stderr.write('fake-agent: failing on request\n'); process.exit(3); }
out({ type: 'result', subtype: 'success', is_error: false, session_id: sid, result: 'done' });
