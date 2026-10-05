// ONE adapter for every OpenAI-compatible chat-completions endpoint (OpenAI, Gemini's OpenAI layer, xAI, OpenRouter, custom).
// The Harness keeps its history in Anthropic block form (the transport contract). This module translates that history to
// chat-completions messages for each request, and the (streamed or plain) response back into blocks.
//
// Wire format (verified against provider docs, see NOTES.md):
//   tools          [{type:"function", function:{name, description, parameters}}]
//   assistant      {role:"assistant", content: string|null, tool_calls:[{id, type:"function", function:{name, arguments:<JSON string>}}]}
//   tool result    {role:"tool", tool_call_id, content: <string>}     <- text ONLY (OpenAI/xAI/OpenRouter/Gemini document string content)
//   images         {role:"user", content:[{type:"text"},{type:"image_url", image_url:{url:"data:image/png;base64,..."}}]}
// Tool-result images are therefore sent as a follow-up user message directly after the tool messages.
import { ChatError, type Block, type ChatMessage, type ChatRequest, type ChatResponse, type SendOpts, type ToolUseBlock, type Transport } from './transport';
import type { LlmFetch } from './net';

export interface OpenAIConfig {
  /** provider id for the proxy (`openai`, `gemini`, `xai`, `openrouter`, `custom`) */
  provider: string;
  baseUrl: string;
  path?: string;
  apiKey: string;
  headers?: Record<string, string>;
  /** model accepts image input; false drops images with a visible stand-in */
  vision: boolean;
  /** tool messages carry `name` (Gemini) */
  toolMessageName?: boolean;
  /** echo `reasoning_content` back (custom endpoints, e.g. DeepSeek-style reasoners) */
  echoReasoningContent?: boolean;
  fetch: LlmFetch;
}

type Json = Record<string, unknown>;
export interface OaiToolCall { id: string; type: 'function'; function: { name: string; arguments: string }; extra_content?: unknown }
export type OaiMessage = Json;

/** Extra fields the provider attached to an assistant message that must travel back verbatim next request. */
const PASSTHROUGH = 'oai_passthrough';
type PassBlock = { type: typeof PASSTHROUGH; fields: Json };
type KerfToolUse = ToolUseBlock & { oai_extra?: unknown };

const textOf = (blocks: Block[]) => blocks.filter((b) => b.type === 'text').map((b) => (b as { text: string }).text).join('\n');

function dataUrl(b: Block): string | null {
  const s = (b as { source?: { media_type?: string; data?: string } }).source;
  return s?.data ? `data:${s.media_type ?? 'image/png'};base64,${s.data}` : null;
}

const NO_VISION = '(image omitted: the selected model does not accept images)';

/** Anthropic-form tool definitions -> function tools. */
export function toolsToFunctions(tools: unknown[]): unknown[] {
  return (tools as { name: string; description?: string; input_schema?: unknown }[]).map((t) => ({
    type: 'function', function: { name: t.name, description: t.description ?? '', parameters: t.input_schema ?? { type: 'object', properties: {} } },
  }));
}

/** Translate the Harness history (+ system prompt) into chat-completions messages. */
export function toOpenAIMessages(system: string, history: ChatMessage[], cfg: Pick<OpenAIConfig, 'vision' | 'toolMessageName' | 'echoReasoningContent'>): OaiMessage[] {
  const out: OaiMessage[] = [{ role: 'system', content: system }];
  const names = new Map<string, string>(); // tool_call_id -> function name (for Gemini's `name`)
  for (const m of history) {
    if (m.role === 'assistant') {
      const calls: OaiToolCall[] = [];
      let text = '';
      const extra: Json = {};
      for (const b of m.content) {
        if (b.type === 'text') text += (b as { text: string }).text;
        else if (b.type === 'tool_use') {
          const u = b as KerfToolUse;
          names.set(u.id, u.name);
          calls.push({ id: u.id, type: 'function', function: { name: u.name, arguments: JSON.stringify(u.input ?? {}) }, ...(u.oai_extra !== undefined ? { extra_content: u.oai_extra } : {}) });
        } else if (b.type === PASSTHROUGH) {
          const f = (b as unknown as PassBlock).fields;
          if ('reasoning_details' in f) extra.reasoning_details = f.reasoning_details;
          if (cfg.echoReasoningContent && 'reasoning_content' in f) extra.reasoning_content = f.reasoning_content;
        }
      }
      if (!text && !calls.length) continue;
      out.push({ role: 'assistant', content: text || null, ...(calls.length ? { tool_calls: calls } : {}), ...extra });
      continue;
    }
    const results = m.content.filter((b) => b.type === 'tool_result') as { tool_use_id: string; content: Block[] | string; is_error?: boolean }[];
    if (results.length) {
      const followImages: { id: string; url: string }[] = [];
      for (const r of results) {
        const parts = typeof r.content === 'string' ? [{ type: 'text', text: r.content } as Block] : r.content;
        let text = textOf(parts);
        for (const p of parts) {
          if (p.type !== 'image') continue;
          const url = dataUrl(p);
          if (url && cfg.vision) followImages.push({ id: r.tool_use_id, url });
          else text += (text ? '\n' : '') + NO_VISION;
        }
        if (followImages.some((i) => i.id === r.tool_use_id)) text += `${text ? '\n' : ''}(the image rendered by this call follows in the next message)`;
        const nm = names.get(r.tool_use_id);
        out.push({ role: 'tool', tool_call_id: r.tool_use_id, content: text || (r.is_error ? 'ERROR' : 'ok'), ...(cfg.toolMessageName && nm ? { name: nm } : {}) });
      }
      if (followImages.length) {
        out.push({ role: 'user', content: followImages.flatMap((i) => [
          { type: 'text', text: `[image returned by tool call ${i.id}]` }, { type: 'image_url', image_url: { url: i.url } },
        ]) });
      }
      const rest = m.content.filter((b) => b.type !== 'tool_result');
      if (rest.length) out.push(userMessage(rest, cfg));
      continue;
    }
    out.push(userMessage(m.content, cfg));
  }
  return out;
}

function userMessage(blocks: Block[], cfg: Pick<OpenAIConfig, 'vision'>): OaiMessage {
  const parts: Json[] = [];
  let dropped = 0;
  for (const b of blocks) {
    if (b.type === 'image') {
      const url = dataUrl(b);
      if (url && cfg.vision) parts.push({ type: 'image_url', image_url: { url } }); else dropped++;
    } else if (b.type === 'text') parts.push({ type: 'text', text: (b as { text: string }).text });
  }
  if (dropped) parts.push({ type: 'text', text: `[${dropped} attached image${dropped > 1 ? 's' : ''} omitted: the selected model does not accept images]` });
  if (parts.every((p) => p.type === 'text')) return { role: 'user', content: parts.map((p) => p.text).join('\n') };
  return { role: 'user', content: parts };
}

export function buildBody(req: ChatRequest, cfg: OpenAIConfig, opt: { stream?: boolean; reasoningNone?: boolean } = {}): Json {
  const body: Json = {
    model: req.model,
    messages: toOpenAIMessages(req.system, req.messages, cfg),
    tools: toolsToFunctions(req.tools),
    stream: opt.stream ?? true,
  };
  if (opt.reasoningNone) body.reasoning_effort = 'none'; // OpenAI: function tools in Chat Completions need reasoning_effort "none" (gpt-5.4+/gpt-6)
  return body;
}

// ---------------------------------------------------------------- response parsing

const FINISH: Record<string, string> = { stop: 'end_turn', tool_calls: 'tool_use', function_call: 'tool_use', length: 'max_tokens', content_filter: 'refusal' };

class Acc {
  text = '';
  calls: { id: string; name: string; args: string; extra?: unknown }[] = [];
  private byIndex = new Map<number, number>();
  reasoningContent = '';
  reasoningDetails: unknown[] = [];
  finish: string | null = null;
  refusal = '';

  tool(tc: Json) {
    const fn = (tc.function ?? {}) as { name?: string; arguments?: unknown };
    const idx = typeof tc.index === 'number' ? tc.index : undefined;
    const id = typeof tc.id === 'string' ? tc.id : '';
    let pos = idx !== undefined ? this.byIndex.get(idx) : undefined;
    if (pos !== undefined && id && this.calls[pos].id && this.calls[pos].id !== id) pos = undefined; // a provider reusing one index for several calls
    if (pos === undefined) {
      // No index (some Gemini chunks): a delta carrying a name or a fresh id starts a call, anything else continues the last one.
      if (idx === undefined && this.calls.length && !fn.name && !id) pos = this.calls.length - 1;
      else { this.calls.push({ id, name: '', args: '' }); pos = this.calls.length - 1; }
      if (idx !== undefined) this.byIndex.set(idx, pos);
    }
    const c = this.calls[pos];
    if (id && !c.id) c.id = id;
    if (fn.name) c.name += fn.name;
    if (typeof fn.arguments === 'string') c.args += fn.arguments;
    else if (fn.arguments && typeof fn.arguments === 'object') c.args = JSON.stringify(fn.arguments);
    if (tc.extra_content !== undefined) c.extra = tc.extra_content;
  }

  /** a streamed `delta` or a complete `message` */
  take(d: Json, onText?: (t: string) => void) {
    const content = d.content;
    if (typeof content === 'string' && content) { this.text += content; onText?.(content); }
    else if (Array.isArray(content)) for (const p of content as Json[]) if (typeof p.text === 'string') { this.text += p.text; onText?.(p.text); }
    if (typeof d.refusal === 'string') this.refusal += d.refusal;
    if (typeof d.reasoning_content === 'string') this.reasoningContent += d.reasoning_content;
    if (Array.isArray(d.reasoning_details)) this.reasoningDetails.push(...d.reasoning_details);
    if (Array.isArray(d.tool_calls)) for (const tc of d.tool_calls as Json[]) this.tool(tc);
  }

  response(): ChatResponse {
    const content: Block[] = [];
    const fields: Json = {};
    if (this.reasoningDetails.length) fields.reasoning_details = this.reasoningDetails;
    if (this.reasoningContent) fields.reasoning_content = this.reasoningContent;
    if (Object.keys(fields).length) content.push({ type: PASSTHROUGH, fields } as unknown as Block);
    if (this.text) content.push({ type: 'text', text: this.text });
    this.calls.forEach((c, i) => {
      const id = c.id || `call_kerf_${Date.now().toString(36)}_${i}`;
      let input: Record<string, unknown>;
      try { const v = c.args.trim() ? JSON.parse(c.args) : {}; input = v && typeof v === 'object' && !Array.isArray(v) ? v : { _value: v }; }
      catch { input = { __invalid_json: c.args }; } // surfaced to the model as a tool error by runTool
      content.push({ type: 'tool_use', id, name: c.name, input, ...(c.extra !== undefined ? { oai_extra: c.extra } : {}) } as Block);
    });
    let stop = this.finish ? (FINISH[this.finish] ?? 'end_turn') : 'end_turn';
    if (this.calls.length) stop = 'tool_use'; // Gemini reports finish_reason "stop" alongside tool calls
    else if (this.refusal && !this.text) { content.push({ type: 'text', text: this.refusal }); }
    return { content, stop_reason: stop, stop_details: this.refusal ? { explanation: this.refusal } : null };
  }
}

export function errorMessage(status: number, bodyText: string): string {
  try {
    const j = JSON.parse(bodyText);
    const e = Array.isArray(j) ? j[0]?.error ?? j[0] : j.error ?? j;
    const msg = typeof e === 'string' ? e : e?.message ?? e?.error?.message;
    if (msg) {
      const raw = e?.metadata?.raw; // OpenRouter nests the upstream error here
      return String(msg) + (typeof raw === 'string' && raw && !String(msg).includes(raw) ? ` — ${raw.slice(0, 300)}` : '');
    }
  } catch { /* not JSON */ }
  return bodyText.trim().slice(0, 400) || `HTTP ${status}`;
}

/** Parse a chat-completions SSE stream (or a plain JSON body) into one response. Exported for fixtures. */
export async function parseResponse(res: Response, onText?: (t: string) => void): Promise<ChatResponse> {
  const acc = new Acc();
  const ct = res.headers.get('content-type') ?? '';
  if (!ct.includes('event-stream')) {
    const j = (await res.json()) as Json;
    if (j.error) throw new ChatError(typeof (j.error as Json).code === 'number' ? ((j.error as Json).code as number) : 502, errorMessage(502, JSON.stringify(j)), false);
    const ch = (j.choices as Json[] | undefined)?.[0];
    if (!ch) throw new ChatError(502, 'THE PROVIDER RETURNED NO CHOICES.', true);
    acc.take((ch.message ?? {}) as Json, onText);
    acc.finish = (ch.finish_reason as string) ?? null;
    return acc.response();
  }
  const reader = res.body!.getReader();
  const dec = new TextDecoder();
  let buf = '';
  const handle = (line: string) => {
    if (!line.startsWith('data:')) return; // comments (": OPENROUTER PROCESSING"), event:, id:
    const data = line.slice(5).trim();
    if (!data || data === '[DONE]') return;
    let j: Json;
    try { j = JSON.parse(data); } catch { return; }
    if (j.error) throw new ChatError(typeof (j.error as Json).code === 'number' ? ((j.error as Json).code as number) : 502, errorMessage(502, data), false);
    const ch = (j.choices as Json[] | undefined)?.[0];
    if (!ch) return; // usage-only chunk
    acc.take((ch.delta ?? ch.message ?? {}) as Json, onText);
    if (ch.finish_reason) acc.finish = ch.finish_reason as string;
  };
  for (;;) {
    const { done, value } = await reader.read();
    if (value) buf += dec.decode(value, { stream: !done });
    let nl: number;
    while ((nl = buf.search(/\r?\n/)) >= 0) { handle(buf.slice(0, nl)); buf = buf.slice(buf[nl] === '\r' ? nl + 2 : nl + 1); }
    if (done) break;
  }
  if (buf) handle(buf);
  return acc.response();
}

export class OpenAITransport implements Transport {
  /** remembered for the session: send reasoning_effort:"none" because the endpoint rejected tools alongside reasoning */
  private reasoningNone = false;
  constructor(private cfg: OpenAIConfig) {}

  async send(req: ChatRequest, opts: SendOpts): Promise<ChatResponse> {
    const { cfg } = this;
    const headers: Record<string, string> = { ...(cfg.headers ?? {}) };
    if (cfg.apiKey) headers.authorization = `Bearer ${cfg.apiKey}`;
    let attempted = false;
    for (;;) {
      const res = await cfg.fetch({ provider: cfg.provider, baseUrl: cfg.baseUrl, path: cfg.path ?? '/chat/completions', headers, body: buildBody(req, cfg, { reasoningNone: this.reasoningNone }), signal: opts.signal });
      if (!res.ok) {
        const text = await res.text().catch(() => '');
        const msg = errorMessage(res.status, text);
        // OpenAI (gpt-5.4+/gpt-6): "Function tools with reasoning_effort are not supported ... use /v1/responses or set reasoning_effort to 'none'"
        if (res.status === 400 && !this.reasoningNone && !attempted && /reasoning_effort/i.test(msg) && /tool/i.test(msg)) { this.reasoningNone = true; attempted = true; continue; }
        throw new ChatError(res.status, msg, res.status === 429 || res.status === 529 || res.status >= 500);
      }
      try { return await parseResponse(res, opts.onText); }
      catch (e) {
        if (opts.signal?.aborted || e instanceof ChatError) throw e;
        throw new ChatError(null, `PROVIDER STREAM ENDED UNEXPECTEDLY: ${(e as Error).message}`, true);
      }
    }
  }
}
