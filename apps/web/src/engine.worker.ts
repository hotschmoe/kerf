// Runs RawEngine in a Web Worker so long engine calls never block the UI thread.
import { RawEngine, EngineCallError } from './engine-raw';

let eng: RawEngine | null = null;

self.onmessage = async (ev: MessageEvent) => {
  const m = ev.data as { id: number; kind: string; url?: string; bytes?: ArrayBuffer; fn?: string; input?: unknown; binary?: boolean };
  try {
    if (m.kind === 'init') {
      eng = await RawEngine.load(m.bytes ?? m.url!);
      (self as unknown as Worker).postMessage({ id: m.id, ok: true, result: { loadMs: eng.loadMs, wasmBytes: eng.wasmBytes } });
    } else if (m.kind === 'call') {
      if (!eng) throw new Error('engine not loaded');
      if (m.binary) {
        const out = eng.callBytes(m.fn!, m.input);
        (self as unknown as Worker).postMessage({ id: m.id, ok: true, result: out, stats: eng.stats.slice(-1) }, [out.buffer]);
      } else {
        const out = eng.callJson(m.fn!, m.input);
        (self as unknown as Worker).postMessage({ id: m.id, ok: true, result: out, stats: eng.stats.slice(-1) });
      }
    }
  } catch (e) {
    const err = e as Error;
    (self as unknown as Worker).postMessage({
      id: m.id, ok: false, message: err.message, payload: e instanceof EngineCallError ? e.payload : undefined,
      stats: eng ? eng.stats.slice(-1) : [],
    });
  }
};
