// Chat providers: Anthropic (Messages API via the official SDK), five OpenAI-compatible chat-completions
// providers (one adapter, chat/openai.ts), and local agents (workspace mode only, /api/agent/run).
// Keys and models live in localStorage per provider and are never logged.
// Model ids verified 2026-10-06 against each provider's docs / openrouter.ai/api/v1/models (see NOTES.md).

export type CloudProviderId = 'anthropic' | 'openai' | 'gemini' | 'xai' | 'openrouter' | 'custom';
export const AGENT_PREFIX = 'agent:';

export interface ModelOpt { id: string; label: string }
export interface ProviderDef {
  id: CloudProviderId;
  /** picker label */
  label: string;
  /** console card header: KERF/<card> */
  card: string;
  baseUrl: string;
  /** path appended to baseUrl (chat completions); Anthropic uses the SDK instead */
  path: string;
  keyHint: string;
  keyUrl: string;
  models: ModelOpt[];
  defaultModel: string;
  /** the model family accepts image input */
  vision: boolean;
  /** tool messages may carry `name` (Gemini's OpenAI layer expects it) */
  toolMessageName?: boolean;
  /** extra request headers (OpenRouter attribution) */
  headers?: Record<string, string>;
  /** the API answers browser (CORS) requests; false/unknown providers get a clear message when blocked */
  cors: 'yes' | 'unknown';
  keyOptional?: boolean;
}

export const PROVIDERS: ProviderDef[] = [
  {
    id: 'anthropic', label: 'Anthropic (Claude)', card: 'CLAUDE', baseUrl: 'https://api.anthropic.com', path: '/v1/messages',
    keyHint: 'sk-ant-…', keyUrl: 'console.anthropic.com',
    models: [{ id: 'claude-opus-5-5', label: 'Claude Opus 5.5' }, { id: 'claude-sonnet-5-5', label: 'Claude Sonnet 5.5' }],
    defaultModel: 'claude-opus-5-5', vision: true, cors: 'yes',
  },
  {
    id: 'openai', label: 'OpenAI', card: 'OPENAI', baseUrl: 'https://api.openai.com/v1', path: '/chat/completions',
    keyHint: 'sk-…', keyUrl: 'platform.openai.com',
    models: [{ id: 'gpt-6.1-sol', label: 'GPT-6.1 Sol' }, { id: 'gpt-6-astra', label: 'GPT-6 Astra' }, { id: 'gpt-6-luna', label: 'GPT-6 Luna' }],
    defaultModel: 'gpt-6.1-sol', vision: true, cors: 'yes',
  },
  {
    id: 'gemini', label: 'Google Gemini', card: 'GEMINI', baseUrl: 'https://generativelanguage.googleapis.com/v1beta/openai', path: '/chat/completions',
    keyHint: 'AIza…', keyUrl: 'aistudio.google.com',
    models: [{ id: 'gemini-3.8-flash', label: 'Gemini 3.8 Flash' }, { id: 'gemini-3.1-pro-preview', label: 'Gemini 3.1 Pro (preview)' }, { id: 'gemini-2.5-pro', label: 'Gemini 2.5 Pro' }, { id: 'gemini-2.5-flash', label: 'Gemini 2.5 Flash' }],
    defaultModel: 'gemini-3.8-flash', vision: true, toolMessageName: true, cors: 'unknown',
  },
  {
    id: 'xai', label: 'xAI (Grok API)', card: 'GROK', baseUrl: 'https://api.x.ai/v1', path: '/chat/completions',
    keyHint: 'xai-…', keyUrl: 'console.x.ai',
    models: [{ id: 'grok-4.7', label: 'Grok 4.7' }, { id: 'grok-4.6', label: 'Grok 4.6' }, { id: 'grok-4.3', label: 'Grok 4.3' }, { id: 'grok-4.20-0309-reasoning', label: 'Grok 4.20 reasoning' }],
    defaultModel: 'grok-4.7', vision: true, cors: 'unknown',
  },
  {
    id: 'openrouter', label: 'OpenRouter', card: 'OPENROUTER', baseUrl: 'https://openrouter.ai/api/v1', path: '/chat/completions',
    keyHint: 'sk-or-…', keyUrl: 'openrouter.ai/keys',
    models: [
      { id: 'anthropic/claude-sonnet-5.5', label: 'Claude Sonnet 5.5' }, { id: 'anthropic/claude-opus-5.5', label: 'Claude Opus 5.5' },
      { id: 'openai/gpt-6.1-sol', label: 'GPT-6.1 Sol' }, { id: 'google/gemini-3.8-flash', label: 'Gemini 3.8 Flash' }, { id: 'x-ai/grok-4.7', label: 'Grok 4.7' },
    ],
    defaultModel: 'anthropic/claude-sonnet-5.5', vision: true, headers: { 'X-Title': 'Kerf' }, cors: 'yes',
  },
  {
    id: 'custom', label: 'Custom (OpenAI-compatible)', card: 'CUSTOM', baseUrl: 'http://localhost:11434/v1', path: '/chat/completions',
    keyHint: '(optional)', keyUrl: '', models: [], defaultModel: '', vision: true, cors: 'unknown', keyOptional: true,
  },
];

export const providerDef = (id: string): ProviderDef | undefined => PROVIDERS.find((p) => p.id === id);
export const isAgentChoice = (choice: string) => choice.startsWith(AGENT_PREFIX);
export const agentId = (choice: string) => choice.slice(AGENT_PREFIX.length);

// ---------- persistence (localStorage; every access guarded: private mode / blocked storage) ----------
const get = (k: string): string | null => { try { return localStorage.getItem(k); } catch { return null; } };
const set = (k: string, v: string | null) => { try { if (v === null || v === '') localStorage.removeItem(k); else localStorage.setItem(k, v); } catch { /* ignore */ } };

// Anthropic keeps its original keys (kerf.apiKey / kerf.model) so existing browsers and tests keep working.
const keyKey = (id: string) => (id === 'anthropic' ? 'kerf.apiKey' : `kerf.key.${id}`);
const modelKey = (id: string) => (id === 'anthropic' ? 'kerf.model' : `kerf.model.${id}`);

export const store = {
  choice(): string | null { return get('kerf.provider'); },
  setChoice(v: string) { set('kerf.provider', v); },
  key(id: string): string { return get(keyKey(id)) ?? ''; },
  setKey(id: string, v: string) { set(keyKey(id), v.trim()); },
  model(id: string): string { const d = providerDef(id); return (get(modelKey(id)) ?? '').trim() || d?.defaultModel || ''; },
  setModel(id: string, v: string) { set(modelKey(id), v.trim()); },
  baseUrl(id: string): string { return id === 'custom' ? (get('kerf.baseurl.custom') ?? '').trim() || providerDef('custom')!.baseUrl : providerDef(id)!.baseUrl; },
  setBaseUrl(v: string) { set('kerf.baseurl.custom', v.trim()); },
  vision(id: string): boolean { return id === 'custom' ? get('kerf.vision.custom') !== '0' : providerDef(id)?.vision ?? true; },
  setVision(v: boolean) { set('kerf.vision.custom', v ? '1' : '0'); },
  /** agent session id (claude --resume) per agent and document */
  session(agent: string, file: string | null): string { return get(`kerf.session.${agent}.${file ?? '_'}`) ?? ''; },
  setSession(agent: string, file: string | null, id: string) { set(`kerf.session.${agent}.${file ?? '_'}`, id); },
  clearSessions(file: string | null) {
    try {
      const keys: string[] = [];
      for (let i = 0; i < localStorage.length; i++) { const k = localStorage.key(i); if (k) keys.push(k); }
      for (const k of keys) if (k.startsWith('kerf.session.') && k.endsWith(`.${file ?? '_'}`)) localStorage.removeItem(k);
    } catch { /* ignore */ }
  },
};

/** Is this cloud provider ready to send? (custom may be keyless, e.g. a local Ollama) */
export function configured(id: CloudProviderId): boolean {
  const d = providerDef(id)!;
  if (id === 'custom') return !!store.baseUrl('custom') && !!store.model('custom');
  return d.keyOptional ? true : !!store.key(id);
}

export interface AgentInfo { id: string; name: string; available: boolean; version?: string; reason?: string }

/** The provider selected at startup / validated when agents appear or vanish. */
export function resolveChoice(agents: AgentInfo[]): string {
  const saved = store.choice();
  if (saved) {
    if (isAgentChoice(saved)) { if (agents.some((a) => a.id === agentId(saved))) return saved; }
    else if (providerDef(saved)) return saved;
  }
  const first = agents.find((a) => a.available);
  return first ? AGENT_PREFIX + first.id : 'anthropic';
}

export function choiceLabel(choice: string, agents: AgentInfo[]): string {
  if (isAgentChoice(choice)) return `Local agent: ${agents.find((a) => a.id === agentId(choice))?.name ?? agentId(choice)}`;
  return providerDef(choice)?.label ?? choice;
}
