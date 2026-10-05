// The chat session: which provider is selected, how to reach it (direct / via the local server), and the one
// send/stop surface the console uses, whether the backend is the tool-loop Harness or a local agent run.
import type { App } from '../app';
import type { Workspace } from '../workspace/workspace';
import { Harness, type ChatEvent } from './harness';
import { AgentRunner } from './agent';
import { AnthropicTransport, type Transport, type ImageBlock } from './transport';
import { OpenAITransport } from './openai';
import { MockTransport } from './mock';
import { directFetch, proxyFetch, type LlmFetch } from './net';
import { AGENT_PREFIX, PROVIDERS, agentId, choiceLabel, configured, isAgentChoice, providerDef, resolveChoice, store, type AgentInfo, type CloudProviderId } from './providers';

export interface SessionOpts {
  app: App;
  ws: Workspace | null;
  catalogMd: () => string;
  mock: MockTransport;
  demo: boolean;
  /** test hook (?api=): Anthropic SDK base URL, localhost only */
  testBase?: string;
}

export class ChatSession {
  readonly harness: Harness;
  readonly agent: AgentRunner | null;
  demo: boolean;
  choice: string;
  private listeners = new Set<() => void>();
  private oaCache = new Map<string, OpenAITransport>();

  constructor(private o: SessionOpts) {
    this.demo = o.demo;
    this.choice = resolveChoice(this.agents());
    this.harness = new Harness(o.app, {
      transport: () => this.transport(),
      model: () => (this.cloud ? store.model(this.cloud.id) : ''),
      catalogMd: o.catalogMd,
      label: () => (this.demo ? 'KERF/CLAUDE' : `KERF/${this.cloud?.card ?? 'CLAUDE'}`),
    });
    this.agent = o.ws ? new AgentRunner(o.app, o.ws, () => this.agents()) : null;
    this.refresh();
  }

  agents(): AgentInfo[] { return this.o.ws?.info.agents ?? []; }
  get isAgent() { return isAgentChoice(this.choice); }
  get cloud() { return this.isAgent ? undefined : providerDef(this.choice); }
  get busy() { return this.harness.busy || !!this.agent?.busy; }
  get fetcher(): LlmFetch { return this.o.ws ? proxyFetch(this.o.ws.client) : directFetch; }
  get file() { return this.o.ws?.file ?? null; }
  get viaServer() { return !!this.o.ws; }
  label(): string { return this.demo ? 'DEMO' : choiceLabel(this.choice, this.agents()); }
  /** short picker text */
  shortLabel(choice = this.choice): string {
    if (isAgentChoice(choice)) return `AGENT: ${(this.agents().find((a) => a.id === agentId(choice))?.name ?? agentId(choice)).toUpperCase()}`;
    return ({ anthropic: 'ANTHROPIC', openai: 'OPENAI', gemini: 'GEMINI', xai: 'XAI GROK', openrouter: 'OPENROUTER', custom: 'CUSTOM' } as Record<string, string>)[choice] ?? choice.toUpperCase();
  }

  on(fn: (e: ChatEvent) => void) {
    const a = this.harness.on(fn);
    const b = this.agent?.on(fn);
    return () => { a(); b?.(); };
  }
  onChange(fn: () => void) { this.listeners.add(fn); return () => this.listeners.delete(fn); }

  setChoice(c: string) {
    if (c === this.choice) { this.refresh(); return; }
    const had = this.harness.messages.length > 0;
    this.choice = c;
    store.setChoice(c);
    if (had) this.harness.reset(); // histories are not portable between providers (thinking blocks, call ids, reasoning fields)
    this.refresh();
    this.listeners.forEach((f) => f());
  }

  setDemo(on: boolean) {
    this.demo = on; this.refresh(); this.listeners.forEach((f) => f());
    if (on) this.o.app.flash('DEMO MODE: SCRIPTED CLAUDE (NO API CALLS)');
  }

  /** can the selected backend take a message right now? `why` is console text when not. */
  ready(): { ok: true } | { ok: false; why: string } {
    if (this.demo) return { ok: true };
    if (this.isAgent) {
      const a = this.agents().find((x) => x.id === agentId(this.choice));
      return a?.available ? { ok: true } : { ok: false, why: `${(a?.name ?? 'THE LOCAL AGENT').toUpperCase()} IS NOT AVAILABLE ON THIS MACHINE${a?.reason ? ': ' + a.reason : ''}. PICK ANOTHER PROVIDER.` };
    }
    const def = this.cloud!;
    return configured(def.id as CloudProviderId) ? { ok: true } : { ok: false, why: def.id === 'custom' ? 'CUSTOM PROVIDER NEEDS A BASE URL AND A MODEL — OPEN SETUP.' : `NO API KEY FOR ${def.label.toUpperCase()} — OPEN SETUP (TOP RIGHT) AND ENTER ONE.` };
  }

  refresh() {
    const r = this.ready();
    this.o.app.providerLabel = this.demo ? 'CLAUDE' : this.isAgent ? 'AGENT' : (this.cloud?.card ?? 'CLAUDE');
    if (!this.busy) this.o.app.setClaude({ state: r.ok ? 'OK' : 'NO KEY' });
  }

  transport(): Transport | null {
    if (this.demo) return this.o.mock;
    const def = this.cloud;
    if (!def || !this.ready().ok) return null;
    if (def.id === 'anthropic') return new AnthropicTransport(store.key('anthropic'), this.o.testBase, this.o.ws ? this.fetcher : undefined);
    const cfg = {
      provider: def.id, baseUrl: store.baseUrl(def.id), path: def.path, apiKey: store.key(def.id), headers: def.headers,
      vision: store.vision(def.id), toolMessageName: def.toolMessageName, echoReasoningContent: def.id === 'custom',
    };
    const k = JSON.stringify(cfg) + (this.o.ws ? 'p' : 'd');
    let t = this.oaCache.get(k);
    if (!t) { t = new OpenAITransport({ ...cfg, fetch: this.fetcher }); this.oaCache.set(k, t); }
    return t;
  }

  async send(text: string, images: { block: ImageBlock; thumbUrl: string }[]): Promise<void> {
    if (this.isAgent && !this.demo) await this.agent!.send(this.choice, text, images.length);
    else await this.harness.send(text, images);
  }

  stop() { if (this.agent?.busy) void this.agent.stop(); else this.harness.stop(); }

  /** NEW: forget the model's conversation (and the agent's resume session for this document) */
  newConversation() {
    this.harness.reset();
    store.clearSessions(this.o.ws?.file ?? null);
  }
}

export { AGENT_PREFIX, PROVIDERS };
