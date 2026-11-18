export interface AgentInfo { id: string; name: string; available: boolean; version?: string; reason?: string }

const get = (k: string): string | null => { try { return localStorage.getItem(k); } catch { return null; } };
const set = (k: string, v: string | null) => { try { if (v === null || v === '') localStorage.removeItem(k); else localStorage.setItem(k, v); } catch {} };

export const store = {
  choice(): string | null { return get('kerf.agent'); },
  setChoice(id: string) { set('kerf.agent', id); },
  session(agent: string, file: string | null): string { return get(`kerf.session.${agent}.${file ?? '_'}`) ?? ''; },
  setSession(agent: string, file: string | null, id: string) { set(`kerf.session.${agent}.${file ?? '_'}`, id); },
  clearSessions(file: string | null) {
    try {
      const keys: string[] = [];
      for (let i = 0; i < localStorage.length; i++) { const k = localStorage.key(i); if (k) keys.push(k); }
      for (const k of keys) if (k.startsWith('kerf.session.') && k.endsWith(`.${file ?? '_'}`)) localStorage.removeItem(k);
    } catch {}
  },
};

export function resolveChoice(agents: AgentInfo[]): string | null {
  const saved = store.choice();
  if (saved && agents.some((a) => a.id === saved)) return saved;
  return (agents.find((a) => a.available) ?? agents[0])?.id ?? null;
}
