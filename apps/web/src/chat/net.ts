// How a provider request leaves the browser: straight to the provider (static mode, CORS permitting) or through the
// local server's POST /api/llm proxy (workspace mode; no CORS limits, keys travel in `headers` and are never stored server side).
import { ChatError } from './transport';

export interface LlmRequest {
  provider: string;
  baseUrl: string;
  path: string;
  headers: Record<string, string>;
  body: unknown;
  signal?: AbortSignal;
}
export type LlmFetch = (r: LlmRequest) => Promise<Response>;

const hostOf = (u: string) => { try { return new URL(u).host; } catch { return u; } };

export class CorsBlockedError extends ChatError {
  constructor(host: string) {
    super(null, `THE BROWSER BLOCKED THE REQUEST TO ${host.toUpperCase()} (NO NETWORK, OR THE API REFUSES BROWSER ORIGINS: CORS). `
      + 'RUN `kerf serve` AND OPEN THE UI FROM IT (IT FORWARDS PROVIDER CALLS), OR PICK ANOTHER PROVIDER.', false);
  }
}

export const directFetch: LlmFetch = async (r) => {
  const url = r.baseUrl.replace(/\/+$/, '') + r.path;
  try {
    return await fetch(url, { method: 'POST', headers: { 'content-type': 'application/json', ...r.headers }, body: JSON.stringify(r.body), signal: r.signal });
  } catch (e) {
    if (r.signal?.aborted) throw e;
    // fetch() rejects with a bare TypeError for both CORS rejection and a dead network; the browser hides which.
    throw new CorsBlockedError(hostOf(url));
  }
};

export interface ProxyEnv { apiBase: string; authHeaders(): Record<string, string> }

export function proxyFetch(env: ProxyEnv): LlmFetch {
  return async (r) => {
    try {
      return await fetch(`${env.apiBase}/llm`, {
        method: 'POST',
        headers: { 'content-type': 'application/json', ...env.authHeaders() },
        body: JSON.stringify({ provider: r.provider, base_url: r.baseUrl, path: r.path, headers: r.headers, body: r.body }),
        signal: r.signal,
      });
    } catch (e) {
      if (r.signal?.aborted) throw e;
      throw new ChatError(null, 'THE LOCAL KERF SERVER DID NOT ANSWER (/api/llm). IS `kerf serve` STILL RUNNING?', true);
    }
  };
}
