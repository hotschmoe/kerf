// Transport layer for the Claude Messages API. The harness only sees `Transport`; the real implementation
// uses the official SDK (loaded lazily so demo mode and first paint never pay for it).

export type TextBlock = { type: 'text'; text: string };
export type ImageBlock = { type: 'image'; source: { type: 'base64'; media_type: string; data: string } };
export type ToolUseBlock = { type: 'tool_use'; id: string; name: string; input: Record<string, unknown> };
export type ToolResultBlock = {
  type: 'tool_result'; tool_use_id: string; content: (TextBlock | ImageBlock)[]; is_error?: boolean;
};
export type OtherBlock = { type: string; [k: string]: unknown }; // thinking, redacted_thinking, ...
export type Block = TextBlock | ImageBlock | ToolUseBlock | ToolResultBlock | OtherBlock;

export interface ChatMessage { role: 'user' | 'assistant'; content: Block[] }

export interface ChatRequest {
  model: string;
  system: string;
  tools: unknown[];
  messages: ChatMessage[];
  /** send the server-side-fallback beta + `fallbacks: "default"` (HARNESS.md) */
  useFallbacks: boolean;
}

export interface ChatResponse {
  content: Block[];
  stop_reason: string | null;
  stop_details?: { explanation?: string | null; category?: string | null } | null;
  usage?: Record<string, number | undefined>;
}

export class ChatError extends Error {
  constructor(public status: number | null, message: string, public retryable = false) { super(message); }
}

export interface SendOpts { signal?: AbortSignal; onText?: (delta: string) => void }
export interface Transport { send(req: ChatRequest, opts: SendOpts): Promise<ChatResponse> }

export class AnthropicTransport implements Transport {
  constructor(private apiKey: string) {}

  async send(req: ChatRequest, opts: SendOpts): Promise<ChatResponse> {
    const { default: Anthropic } = await import('@anthropic-ai/sdk');
    // maxRetries 0: the harness owns retry/backoff (2s, 4s, 8s) so the status line can show it.
    const client = new Anthropic({ apiKey: this.apiKey, dangerouslyAllowBrowser: true, maxRetries: 0 });
    const body: Record<string, unknown> = {
      model: req.model,
      max_tokens: 32000,
      thinking: { type: 'adaptive' },
      output_config: { effort: 'high' },
      system: [{ type: 'text', text: req.system, cache_control: { type: 'ephemeral' } }],
      tools: req.tools,
      messages: req.messages,
    };
    const reqOpts: Record<string, unknown> = { signal: opts.signal };
    if (req.useFallbacks) {
      body.fallbacks = 'default';
      reqOpts.headers = { 'anthropic-beta': 'server-side-fallback-2026-07-01' };
    }
    try {
      // Streaming: max_tokens 32000 is far over what the SDK allows for a non-streaming request.
      const stream = client.messages.stream(body as never, reqOpts as never);
      if (opts.onText) stream.on('text', (d: string) => opts.onText!(d));
      const msg = (await stream.finalMessage()) as unknown as ChatResponse;
      return { content: msg.content, stop_reason: msg.stop_reason, stop_details: msg.stop_details ?? null, usage: msg.usage as never };
    } catch (e) {
      if (opts.signal?.aborted) throw e;
      const err = e as { status?: number; message?: string; error?: { error?: { message?: string } } };
      if (typeof err.status === 'number') {
        const msg = err.error?.error?.message ?? err.message ?? 'API error';
        throw new ChatError(err.status, msg, err.status === 429 || err.status === 529 || err.status >= 500);
      }
      throw new ChatError(null, err.message ?? String(e), true); // network failure
    }
  }
}
