// The chat session: which local agent is selected and the one send/stop surface the console uses.
// Chat exists only in workspace mode (`kerf serve`): the agent CLI runs on this machine, never a remote API.
import type { App } from '../app';
import type { Workspace } from '../workspace/workspace';
import { AgentRunner, type ChatEvent } from './agent';
import { resolveChoice, store, type AgentInfo } from './store';

export const CHAT_NEEDS_SERVER = 'CHAT NEEDS `kerf serve` AND A LOCAL AGENT CLI (claude, codex, grok OR pi). OPEN THIS UI FROM `kerf serve` TO USE IT.';

export class ChatSession {
  readonly agent: AgentRunner | null;
  choice: string | null;
  private listeners = new Set<() => void>();

  constructor(private o: { app: App; ws: Workspace | null }) {
    this.agent = o.ws ? new AgentRunner(o.app, o.ws, () => this.agents()) : null;
    this.choice = resolveChoice(this.agents());
    this.refresh();
  }

  agents(): AgentInfo[] { return this.o.ws?.info.agents ?? []; }
  get busy() { return !!this.agent?.busy; }
  get file() { return this.o.ws?.file ?? null; }
  get available() { return !!this.o.ws; }
  shortLabel(id = this.choice): string { return (this.agents().find((a) => a.id === id)?.name ?? id ?? '').toUpperCase(); }

  on(fn: (e: ChatEvent) => void) { return this.agent?.on(fn) ?? (() => {}); }
  onChange(fn: () => void) { this.listeners.add(fn); return () => this.listeners.delete(fn); }

  setChoice(id: string) {
    if (id === this.choice) { this.refresh(); return; }
    this.choice = id;
    store.setChoice(id);
    this.refresh();
    this.listeners.forEach((f) => f());
  }

  /** can the selected agent take a message right now? `why` is console text when not. */
  ready(): { ok: true } | { ok: false; why: string } {
    if (!this.o.ws) return { ok: false, why: CHAT_NEEDS_SERVER };
    const a = this.agents().find((x) => x.id === this.choice);
    if (!this.agents().length) return { ok: false, why: 'THE SERVER LISTS NO LOCAL AGENT. INSTALL claude, codex, grok OR pi AND RESTART `kerf serve`.' };
    return a?.available ? { ok: true } : { ok: false, why: `${(a?.name ?? 'THE LOCAL AGENT').toUpperCase()} IS NOT AVAILABLE ON THIS MACHINE${a?.reason ? ': ' + a.reason : ''}. PICK ANOTHER AGENT.` };
  }

  refresh() {
    const r = this.ready();
    if (!this.busy) this.o.app.setClaude({ state: r.ok ? 'OK' : 'OFF' });
  }

  async send(text: string): Promise<void> {
    if (this.choice) await this.agent?.send(this.choice, text);
  }

  stop() { void this.agent?.stop(); }

  /** NEW: forget the agent's resume session for this document */
  newConversation() { store.clearSessions(this.o.ws?.file ?? null); }
}
