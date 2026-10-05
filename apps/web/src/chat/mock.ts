// MockTransport: a scripted fake Claude for demo mode (?demo=1) and end-to-end tests.
// It also validates every request the harness sends (tool_use/tool_result pairing, role order) so harness
// bugs fail loudly here instead of at api.anthropic.com.
import type { KerfDoc } from '../types';
import { ChatError, type Block, type ChatRequest, type ChatResponse, type SendOpts, type ToolUseBlock, type Transport } from './transport';

export interface MockOpts {
  loadDoc: () => Promise<KerfDoc>;
  /** ms between streamed text chunks (0 = instant) */
  chunkMs?: number;
  /** inject failures for harness tests: statuses returned for the first N requests */
  failFirst?: number[];
}

export function validateHistory(req: ChatRequest) {
  const m = req.messages;
  if (!m.length || m[0].role !== 'user') throw new ChatError(400, 'messages must start with a user turn');
  for (let i = 0; i < m.length; i++) {
    const cur = m[i];
    const uses = cur.role === 'assistant' ? (cur.content as Block[]).filter((b) => b.type === 'tool_use') as ToolUseBlock[] : [];
    if (uses.length) {
      const next = m[i + 1];
      if (next) {
        const res = (next.content as Block[]).filter((b) => b.type === 'tool_result') as { tool_use_id: string }[];
        for (const u of uses) {
          if (!res.some((r) => r.tool_use_id === u.id)) throw new ChatError(400, `tool_use ${u.id} has no tool_result in the next user message`);
        }
        if (next.role !== 'user') throw new ChatError(400, 'tool_result must be in a user message');
      }
    }
    if (cur.role === 'user') {
      const res = (cur.content as Block[]).filter((b) => b.type === 'tool_result');
      if (res.length && i === 0) throw new ChatError(400, 'tool_result without preceding tool_use');
      if (res.length) {
        const prev = m[i - 1];
        const ids = new Set(((prev?.content ?? []) as Block[]).filter((b) => b.type === 'tool_use').map((b) => (b as ToolUseBlock).id));
        for (const r of res as { tool_use_id: string }[]) if (!ids.has(r.tool_use_id)) throw new ChatError(400, `tool_result ${r.tool_use_id} does not match a tool_use`);
      }
    }
  }
}

export class MockTransport implements Transport {
  calls = 0;
  requests: ChatRequest[] = [];
  private seq = 0;
  constructor(private o: MockOpts) {}

  private id() { return 'toolu_mock_' + String(++this.seq).padStart(3, '0'); }

  async send(req: ChatRequest, opts: SendOpts): Promise<ChatResponse> {
    this.calls++;
    this.requests.push(JSON.parse(JSON.stringify(req)));
    const fail = this.o.failFirst?.[this.calls - 1];
    if (fail) throw new ChatError(fail, fail === 529 ? 'Overloaded' : fail === 429 ? 'rate_limit_error' : 'mock failure', fail === 429 || fail === 529);
    validateHistory(req);
    // which designer turn are we in, and where in its tool loop?
    const userTurns = req.messages.filter((m) => m.role === 'user' && (m.content as Block[]).some((b) => b.type === 'text'));
    const turn = userTurns.length;
    const last = req.messages[req.messages.length - 1];
    const lastResults = last.role === 'user' ? (last.content as Block[]).filter((b) => b.type === 'tool_result') : [];
    const stage = lastResults.length ? this.stageAfter(req) : 0;

    let content: Block[];
    let stop = 'tool_use';
    if (turn === 1) {
      const doc = await this.o.loadDoc();
      const view = doc.views[0]?.id ?? 'A';
      if (stage === 0) {
        content = [
          { type: 'text', text: `Building the ${doc.title ?? doc.id} detail. I am using conventional assumptions: IRC 2021, 8" CMU wall with a grouted bond beam, prefab truss bearing on a PT sill plate.` },
          { type: 'tool_use', id: this.id(), name: 'kerf_apply', input: { ops: [{ op: 'set', path: 'doc', value: doc }], why: `Build ${doc.title ?? doc.id}` } },
        ];
      } else if (stage === 1) {
        content = [
          { type: 'text', text: 'Document accepted. Checking the section view.' },
          { type: 'tool_use', id: this.id(), name: 'kerf_render', input: { view } },
        ];
      } else {
        stop = 'end_turn';
        content = [{ type: 'text', text: `Built ${doc.components.length} components and ${doc.views.length} views. Assumptions: 2021 IRC; PT sill plate; H2.5A hurricane tie at each truss. Please verify the suggested code citations (R802.10, R403.1.6, R606) before issuing.` }];
      }
    } else {
      if (stage === 0) {
        content = [{ type: 'text', text: 'Reading the current summary first.' }, { type: 'tool_use', id: this.id(), name: 'kerf_inspect', input: { q: 'summary' } }];
      } else {
        stop = 'end_turn';
        content = [{ type: 'text', text: 'Checked. The document is consistent. Tell me what to change.' }];
      }
    }
    // simulate streaming of text blocks
    const chunkMs = this.o.chunkMs ?? 0;
    for (const b of content) {
      if (b.type !== 'text') continue;
      const t = (b as { text: string }).text;
      for (let i = 0; i < t.length; i += 14) {
        if (opts.signal?.aborted) throw new DOMException('aborted', 'AbortError');
        opts.onText?.(t.slice(i, i + 14));
        if (chunkMs) await new Promise((r) => setTimeout(r, chunkMs));
      }
    }
    return { content, stop_reason: stop, stop_details: null, usage: { input_tokens: 1000, output_tokens: 200 } };
  }

  /** how many assistant tool rounds have already happened in the current designer turn */
  private stageAfter(req: ChatRequest): number {
    let n = 0;
    for (let i = req.messages.length - 1; i >= 0; i--) {
      const m = req.messages[i];
      if (m.role === 'user' && (m.content as Block[]).some((b) => b.type === 'text')) break;
      if (m.role === 'assistant' && (m.content as Block[]).some((b) => b.type === 'tool_use')) n++;
    }
    return n;
  }
}
