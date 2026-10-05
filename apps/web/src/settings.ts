// BYO API key + model, kept in localStorage only. The key is never logged and only goes to api.anthropic.com.
export const MODELS = [
  { id: 'claude-opus-5-5', label: 'CLAUDE OPUS 5.5 (DEFAULT)' },
  { id: 'claude-sonnet-5-5', label: 'CLAUDE SONNET 5.5' },
];
const K_KEY = 'kerf.apiKey', K_MODEL = 'kerf.model';

function get(k: string): string | null { try { return localStorage.getItem(k); } catch { return null; } }
function set(k: string, v: string | null) { try { if (v === null) localStorage.removeItem(k); else localStorage.setItem(k, v); } catch { /* private mode */ } }

export const settings = {
  get apiKey(): string { return get(K_KEY) ?? ''; },
  set apiKey(v: string) { set(K_KEY, v.trim() || null); },
  get model(): string { const m = get(K_MODEL); return MODELS.some((x) => x.id === m) ? m! : MODELS[0].id; },
  set model(v: string) { set(K_MODEL, v); },
};
