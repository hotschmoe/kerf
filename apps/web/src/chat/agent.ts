// Local agent runner (workspace mode): POST /api/agent/run, then render the SSE `agent` events as console cards.
// Event shapes (one per stdout JSON line of the agent CLI; verified live for `claude -p --output-format stream-json --verbose`):
//   {type:"system", subtype:"init", session_id}
//   {type:"assistant", message:{content:[{type:"text",text} | {type:"tool_use",id,name,input} | {type:"thinking"}]}}
//   {type:"user", message:{content:[{type:"tool_result", tool_use_id, content: string | [{type:"text",text}], is_error}]}}
//   {type:"result", subtype, is_error, result, session_id, ...}          (final; its text was already streamed as an assistant block)
//   {type:"text", text}                                                  (plain-text agents)
//   {type:"exit", code, session_id?}                                     (added by the server)
// Also understood (recorded from the real CLIs, see test/fixtures/agent-*.jsonl):
//   Grok Build `-p --output-format streaming-json`:  {type:"text", data:"<delta>"} · {type:"tool_call", toolCallId, toolName, rawInput}
//        · {type:"tool_call_update", toolCallId, status:"completed", rawOutput:{output_for_prompt, exit_code}} · {type:"end", sessionId}
//   Codex `exec --json`:  {type:"item.completed", item:{type:"agent_message", text} | {type:"command_execution", command, aggregated_output, exit_code}}
import type { App } from '../app';
import type { Workspace } from '../workspace/workspace';
import { store, agentId, type AgentInfo } from './providers';
import type { ChatEvent } from './harness';

type Json = Record<string, unknown>;
const str = (v: unknown) => (typeof v === 'string' ? v : '');

/** One-line card title for a tool call: Bash `kerf apply …` reads like a Kerf command, file tools like their verb. */
export function toolTitle(name: string, input: Json): string {
  const clip = (s: string, n = 72) => (s.length > n ? s.slice(0, n - 1) + '…' : s);
  name = TOOL_ALIAS[name] ?? name;
  if (name === 'Bash') {
    // codex wraps commands: /bin/bash -lc '<cmd>'
    const cmd = str(input.command).trim().replace(/^(?:\/\S*\/)?(?:ba|z)?sh\s+-l?c\s+(['"])([\s\S]*)\1$/, '$2');
    const first = cmd.split('\n')[0].replace(/\s*<<-?\s*['"]?\w+['"]?\s*$/, ' <<').replace(/\s+/g, ' ');
    return clip(/^kerf\b/.test(first) ? first : `BASH ${first}`);
  }
  const path = str(input.file_path) || str(input.path) || str(input.pattern);
  const rel = path.replace(/^.*\/(?=[^/]+\/[^/]+$)/, '');
  return clip(`${name.toUpperCase()} ${rel}`.trim());
}

/** the other agents' tool names, mapped to the Claude Code names the cards are written in */
const TOOL_ALIAS: Record<string, string> = { run_terminal_command: 'Bash', shell: 'Bash', command_execution: 'Bash', read_file: 'Read', write: 'Write', search_replace: 'Edit', list_dir: 'LS' };

function resultText(c: unknown): string {
  if (typeof c === 'string') return c;
  if (Array.isArray(c)) return c.map((p) => str((p as Json).text)).filter(Boolean).join('\n');
  return '';
}

/** `0 errors 1 warning` in a kerf summary -> "✓ 0 ERR 1 WARN" */
export function resultStatus(text: string, ok: boolean): string {
  if (!ok) return '✗ ERROR';
  const m = /(\d+)\s+errors?\b[^\d]*(\d+)\s+warnings?/i.exec(text);
  return m ? `✓ ${m[1]} ERR ${m[2]} WARN` : '✓';
}

export class AgentRunner {
  busy = false;
  private runId: string | null = null;
  private listeners = new Set<(e: ChatEvent) => void>();
  private open = new Map<string, { title: string; input: unknown }>();
  private sawExit = false;
  /** raw events that matched no known shape (debugging aid, `window.__kerf.agent.unknown`) */
  unknown: unknown[] = [];

  constructor(private app: App, private ws: Workspace, private info: () => AgentInfo[]) {}

  on(fn: (e: ChatEvent) => void) { this.listeners.add(fn); return () => this.listeners.delete(fn); }
  private emit(e: ChatEvent) { this.listeners.forEach((f) => f(e)); }

  async stop() { if (this.runId) await this.ws.client.agentStop(this.runId); }

  /** `choice` is `agent:<id>`. Never throws. */
  async send(choice: string, text: string, imageCount = 0): Promise<void> {
    if (this.busy) return;
    const id = agentId(choice);
    const agent = this.info().find((a) => a.id === id);
    const file = this.ws.file;
    this.busy = true; this.sawExit = false;
    this.app.pendingDesignerEdits.length = 0; // the agent reads the folder itself; designer edits are already on disk
    this.ws.uiRunActive = true;
    this.emit({ type: 'user', text, images: [], edits: [] });
    if (imageCount) this.emit({ type: 'notice', level: 'warn', text: 'LOCAL AGENTS TAKE TEXT ONLY HERE. THE ATTACHED IMAGE WAS NOT SENT.' });
    this.app.setClaude({ state: 'BUSY', detail: agent?.name });
    const session = store.session(id, file);
    try {
      const runId = await this.ws.client.agentRun(id, text, session || undefined, file);
      this.runId = runId;
      this.emit({ type: 'assistant-start', who: `LOCAL AGENT · ${(agent?.name ?? id).toUpperCase()}` });
      await new Promise<void>((resolve) => {
        // If the event stream stays down the run can never report its exit: give up after 20 s offline.
        let offline = 0;
        const off = this.ws.on('online', () => {
          clearTimeout(offline);
          if (!this.ws.online) offline = window.setTimeout(() => { this.emit({ type: 'notice', level: 'err', text: 'LOST THE CONNECTION TO THE LOCAL SERVER; THE RUN MAY STILL BE GOING.' }); off(); resolve(); }, 20000);
        });
        this.ws.onAgentRun(runId, (ev) => { this.handle(ev, id, file); if (this.sawExit) { clearTimeout(offline); off(); resolve(); } });
      });
    } catch (e) {
      this.emit({ type: 'notice', level: 'err', text: `COULD NOT START THE AGENT: ${(e as Error).message}`.slice(0, 300) });
      this.app.setClaude({ state: 'ERR', detail: (e as Error).message });
    } finally {
      if (this.runId) this.ws.onAgentRun(this.runId, null);
      this.runId = null; this.busy = false; this.ws.endUiRun();
      this.open.clear();
      if (this.app.claude.state === 'BUSY') this.app.setClaude({ state: 'OK' });
      this.emit({ type: 'done' });
    }
  }

  private handle(ev: unknown, agent: string, file: string | null) {
    if (!ev || typeof ev !== 'object') return;
    const e = ev as Json;
    const sid = str(e.session_id) || str(e.sessionId) || str(e.thread_id);
    if (sid) store.setSession(agent, file, sid);
    switch (e.type) {
      case 'system': break; // init: session id captured above
      case 'assistant': {
        const content = ((e.message as Json | undefined)?.content ?? []) as Json[];
        for (const b of content) {
          if (b.type === 'text' && str(b.text)) this.text(str(b.text) + '\n');
          else if (b.type === 'tool_use') this.toolStart(str(b.id), str(b.name), (b.input ?? {}) as Json);
        }
        break;
      }
      case 'user': {
        const content = ((e.message as Json | undefined)?.content ?? []) as Json[];
        for (const b of Array.isArray(content) ? content : []) if (b.type === 'tool_result') this.toolEnd(str(b.tool_use_id), resultText(b.content), b.is_error !== true);
        break;
      }
      case 'result':
        if (e.is_error === true) this.emit({ type: 'notice', level: 'err', text: `AGENT ERROR: ${str(e.result) || str(e.subtype) || 'run failed'}`.slice(0, 400) });
        break;
      case 'text':
        if (typeof e.data === 'string') this.text(e.data); // streamed delta (Grok)
        else if (str(e.text)) this.text(str(e.text) + (str(e.text).endsWith('\n') ? '' : '\n')); // a plain stdout line
        break;
      case 'tool_call': { // Grok
        const raw = (e.rawInput ?? {}) as Json;
        this.toolStart(str(e.toolCallId), str(e.toolName) || str(e.title), raw);
        break;
      }
      case 'tool_call_update': {
        const st = str(e.status);
        if (st !== 'completed' && st !== 'failed') break;
        const raw = (e.rawOutput ?? {}) as Json;
        const content = Array.isArray(e.content) ? (e.content as Json[]).map((c) => str(((c.content ?? {}) as Json).text)).join('') : '';
        const out = str(raw.output_for_prompt) || content;
        this.toolEnd(str(e.toolCallId), out, st === 'completed' && (raw.exit_code === undefined || raw.exit_code === 0 || raw.exit_code === null));
        break;
      }
      case 'item.completed': { // codex exec --json
        const it = (e.item ?? {}) as Json;
        if (it.type === 'agent_message' && str(it.text)) this.text(str(it.text) + '\n');
        else if (it.type === 'command_execution') {
          const id = str(it.id) || `cmd${Math.random().toString(36).slice(2)}`;
          this.toolStart(id, 'Bash', { command: str(it.command) });
          this.toolEnd(id, str(it.aggregated_output), it.exit_code === 0 || it.exit_code === undefined || it.exit_code === null);
        }
        break;
      }
      case 'end': break; // Grok: session id captured above
      case 'exit': {
        this.sawExit = true;
        const code = typeof e.code === 'number' ? e.code : 0;
        if (code !== 0) {
          this.emit({ type: 'notice', level: 'err', text: `THE AGENT EXITED WITH CODE ${code}.${store.session(agent, file) ? ' IF IT COMPLAINED ABOUT THE SESSION, PRESS NEW (CONSOLE HEADER) AND RETRY.' : ''}` });
        }
        break;
      }
      default: this.unknown.push(ev); if (this.unknown.length > 50) this.unknown.shift(); break;
    }
  }

  private text(t: string) { this.emit({ type: 'text', delta: t }); }

  private toolStart(id: string, name: string, input: Json) {
    const title = toolTitle(name, input);
    this.open.set(id, { title, input });
    this.emit({ type: 'tool', id, phase: 'start', title, input: TOOL_ALIAS[name] === 'Bash' || name === 'Bash' ? { command: str(input.command) } : input });
  }

  private toolEnd(id: string, text: string, ok: boolean) {
    const o = this.open.get(id);
    if (!o) return;
    this.open.delete(id);
    const cmd = (o.input as Json).command;
    this.emit({ type: 'tool', id, phase: 'end', title: o.title, ok, status: resultStatus(text, ok), detail: (typeof cmd === 'string' ? `$ ${cmd}\n\n` : '') + (text.length > 6000 ? text.slice(0, 6000) + '\n…' : text), input: o.input });
  }
}
