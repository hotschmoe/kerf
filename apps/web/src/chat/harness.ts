// The Kerf chat harness (spec/llm/HARNESS.md): append-only history, the kerf_* tool loop, retries, refusals.
import type { App } from '../app';
import { runTool, type ToolOutcome } from './tools';
import {
  ChatError, type Block, type ChatMessage, type ChatResponse, type ImageBlock, type ToolResultBlock, type ToolUseBlock, type Transport,
} from './transport';
import systemMd from '../../../../spec/llm/system.md?raw';
import toolsJson from '../../../../spec/llm/tools.json';

export const MAX_ROUNDS = 25;
export const BACKOFF_MS = [2000, 4000, 8000];

export type ChatEvent =
  | { type: 'user'; text: string; images: string[]; edits: string[] }
  | { type: 'assistant-start' }
  | { type: 'text'; delta: string }
  | { type: 'tool'; id: string; phase: 'start' | 'end'; title?: string; status?: string; ok?: boolean; detail?: string; thumb?: Blob; input?: unknown }
  | { type: 'notice'; text: string; level: 'info' | 'warn' | 'err' }
  | { type: 'round'; n: number }
  | { type: 'done' };

export interface HarnessOpts {
  transport: () => Transport | null;
  model: () => string;
  catalogMd: () => string;
  sleep?: (ms: number) => Promise<void>;
}

export function buildSystem(catalogMd: string): string {
  return `${systemMd.trimEnd()}\n\n# Component catalog\n${catalogMd}`;
}

export class Harness {
  /** append-only: never edited, never truncated (thinking blocks are bound to their turns) */
  readonly messages: ChatMessage[] = [];
  private useFallbacks = true;
  private abort: AbortController | null = null;
  busy = false;
  private listeners = new Set<(e: ChatEvent) => void>();
  private sleep: (ms: number) => Promise<void>;

  constructor(private app: App, private opts: HarnessOpts) {
    this.sleep = opts.sleep ?? ((ms) => new Promise((r) => setTimeout(r, ms)));
  }

  on(fn: (e: ChatEvent) => void) { this.listeners.add(fn); return () => this.listeners.delete(fn); }
  private emit(e: ChatEvent) { this.listeners.forEach((f) => f(e)); }

  stop() { this.abort?.abort(); }

  /** Run one designer turn (text + images) through the tool loop. Never throws. */
  async send(text: string, images: { block: ImageBlock; thumbUrl: string }[] = []): Promise<void> {
    if (this.busy) return;
    const transport = this.opts.transport();
    if (!transport) { this.emit({ type: 'notice', level: 'err', text: 'NO API KEY — ENTER KEY TO ENABLE CLAUDE.' }); return; }
    this.busy = true;
    this.abort = new AbortController();
    const signal = this.abort.signal;
    const edits = this.app.pendingDesignerEdits.splice(0);
    const content: Block[] = [...images.map((i) => i.block)];
    content.push({ type: 'text', text });
    if (edits.length) content.push({ type: 'text', text: `[designer edits since your last turn: ${edits.join('; ')}]` });
    this.messages.push({ role: 'user', content });
    this.emit({ type: 'user', text, images: images.map((i) => i.thumbUrl), edits });
    this.app.setClaude({ state: 'BUSY', round: 1 });
    try {
      for (let round = 1; round <= MAX_ROUNDS; round++) {
        this.app.setClaude({ state: 'BUSY', round });
        this.emit({ type: 'round', n: round });
        this.emit({ type: 'assistant-start' });
        const resp = await this.request(transport, signal);
        this.messages.push({ role: 'assistant', content: resp.content }); // verbatim, thinking blocks included
        if (resp.stop_reason === 'refusal') {
          const why = resp.stop_details?.explanation;
          this.emit({ type: 'notice', level: 'err', text: 'CLAUDE DECLINED THIS REQUEST' + (why ? ': ' + why : '.') });
          break;
        }
        if (resp.stop_reason === 'max_tokens') this.emit({ type: 'notice', level: 'warn', text: 'RESPONSE CUT OFF AT THE TOKEN LIMIT. ASK CLAUDE TO CONTINUE.' });
        if (resp.stop_reason === 'model_context_window_exceeded') this.emit({ type: 'notice', level: 'err', text: 'CONVERSATION EXCEEDS THE MODEL CONTEXT WINDOW. START A NEW SESSION.' });
        if (resp.stop_reason !== 'tool_use') break;
        const uses = resp.content.filter((b): b is ToolUseBlock => b.type === 'tool_use');
        const results: ToolResultBlock[] = [];
        for (const u of uses) {
          this.emit({ type: 'tool', id: u.id, phase: 'start', title: u.name.replace('kerf_', '').toUpperCase(), input: u.input });
          const out: ToolOutcome = await runTool(this.app, u.name, u.input ?? {});
          this.emit({ type: 'tool', id: u.id, phase: 'end', title: out.title, status: out.status, ok: !out.is_error, detail: out.detail, thumb: out.thumb, input: u.input });
          results.push({ type: 'tool_result', tool_use_id: u.id, content: out.content, ...(out.is_error ? { is_error: true } : {}) });
        }
        this.messages.push({ role: 'user', content: results }); // ALL results in ONE message
        if (round === MAX_ROUNDS) this.emit({ type: 'notice', level: 'warn', text: `STOPPED AFTER ${MAX_ROUNDS} TOOL ROUNDS. SEND A MESSAGE TO CONTINUE.` });
      }
      this.app.setClaude({ state: 'OK' });
    } catch (e) {
      if (signal.aborted) {
        this.emit({ type: 'notice', level: 'warn', text: 'STOPPED BY DESIGNER.' });
        this.app.setClaude({ state: 'OK' });
      } else {
        const err = e as ChatError;
        const msg = err instanceof ChatError && err.status === 401 ? 'INVALID API KEY' : (err.message ?? String(e));
        this.emit({ type: 'notice', level: 'err', text: msg });
        this.app.setClaude({ state: err instanceof ChatError && err.status === 401 ? 'NO KEY' : 'ERR', detail: msg });
      }
    } finally {
      this.busy = false;
      this.abort = null;
      this.emit({ type: 'done' });
    }
  }

  /** One API call with 429/529/5xx/network backoff and one beta-param fallback. */
  private async request(transport: Transport, signal: AbortSignal): Promise<ChatResponse> {
    let attempt = 0;
    for (;;) {
      try {
        return await transport.send({
          model: this.opts.model(),
          system: buildSystem(this.opts.catalogMd()),
          tools: toolsJson as unknown[],
          messages: this.messages,
          useFallbacks: this.useFallbacks,
        }, { signal, onText: (d) => this.emit({ type: 'text', delta: d }) });
      } catch (e) {
        if (signal.aborted) throw e;
        if (!(e instanceof ChatError)) throw e;
        if (e.status === 400 && this.useFallbacks && /fallback|anthropic-beta|server-side/i.test(e.message)) {
          this.useFallbacks = false; // remembered for the session
          this.emit({ type: 'notice', level: 'info', text: 'SERVER FALLBACK PARAMS REJECTED; RETRYING WITHOUT THEM.' });
          continue;
        }
        if (e.retryable && attempt < BACKOFF_MS.length) {
          const wait = BACKOFF_MS[attempt++];
          this.emit({ type: 'notice', level: 'warn', text: `${e.status ?? 'NETWORK'} ${e.status === 429 ? 'RATE LIMITED' : e.status === 529 ? 'API OVERLOADED' : 'ERROR'} — RETRY ${attempt}/${BACKOFF_MS.length} IN ${wait / 1000}s` });
          this.app.setClaude({ state: 'BUSY', detail: `RETRY ${attempt}` });
          await this.sleep(wait);
          continue;
        }
        throw e;
      }
    }
  }
}
