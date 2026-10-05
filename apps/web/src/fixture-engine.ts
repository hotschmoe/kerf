// Dev-only stand-in for a real engine (VITE_ENGINE=fixture): replays hand-written fixtures that match SPEC §10/§11.
// Lets the UI be developed and screenshot-tested without a wasm build. Not included in rust/zig bundles.
import type { Engine, InspectQuery } from './engine';
import type { ApplyResult, Drawing, KerfDoc, Mesh, Op } from './types';

function mergePatch(t: any, p: any): any {
  if (p === null || typeof p !== 'object' || Array.isArray(p)) return p;
  const o = t && typeof t === 'object' && !Array.isArray(t) ? { ...t } : {};
  for (const [k, v] of Object.entries(p)) { if (v === null) delete o[k]; else o[k] = mergePatch(o[k], v); }
  return o;
}

export async function createFixtureEngine(base = './fixtures/'): Promise<Engine> {
  const j = async (f: string) => (await fetch(base + f)).json();
  const eng: Engine = {
    kind: 'fixture', info: { engine: 'kerf-fixture', version: '0.0.0', spec: '0.1' }, stats: [], loadMs: 0, wasmBytes: 0,
    async version() { return eng.info; },
    async catalog() { return '# Catalog (fixture)\n- lumber\n- cmu_wall\n'; },
    async fmt(doc) { return { doc }; },
    async check() { return { diagnostics: [], summary: '' }; },
    async apply(doc: KerfDoc, _s, ops: Op[]): Promise<ApplyResult> {
      let d: any = JSON.parse(JSON.stringify(doc));
      const changed: string[] = [];
      for (const op of ops) {
        const parts = op.path.split('/');
        if (op.op === 'set' && op.path === 'doc') d = JSON.parse(JSON.stringify(op.value));
        else if (parts[0] === 'views' && parts[1] && parts[2] === 'annotations' && parts[3]) {
          const v = d.views.find((x: any) => x.id === parts[1]);
          const i = v.annotations.findIndex((a: any) => a.id === parts[3]);
          if (op.op === 'update') v.annotations[i] = mergePatch(v.annotations[i], op.value);
          if (op.op === 'remove') v.annotations.splice(i, 1);
          changed.push(parts[3]);
        } else if (parts[0] === 'views' && parts[2] === 'annotations' && op.op === 'add') {
          d.views.find((x: any) => x.id === parts[1]).annotations.push(op.value); changed.push((op.value as any).id);
        }
      }
      const summary = `DOC ${d.id}  ${d.components.length} components  ${d.views.length} views  0 errors 0 warnings\n` +
        d.components.map((c: any) => ` ${c.id.padEnd(14)} ${(c.type + ' ' + (c.size ?? '')).padEnd(44)} x 0..8"  y 0..1 1/2"`).join('\n');
      return { ok: true, doc: d, diagnostics: [], summary, changed };
    },
    async inspect(doc, _s, q: InspectQuery) {
      if (q.q === 'summary') return { summary: 'fixture summary' };
      if (q.q === 'component') return { id: q.id, params: doc.components.find((c) => c.id === q.id), anchors: { top_left: { x: 0, y: 1.5 }, center: { x: 3.6, y: 0.75 } } };
      return {};
    },
    async drawing(doc, _s, view): Promise<Drawing> {
      const d = (await j('drawing-A.json')) as Drawing;
      d.doc = doc.id; d.view = view;
      return d;
    },
    async mesh(): Promise<Mesh> { return j('mesh.json'); },
    async exportBytes(_d, _s, _v, format) {
      if (format === 'svg') return new TextEncoder().encode(await (await fetch(base + 'sheet-A.svg')).text());
      return new TextEncoder().encode('fixture ' + format);
    },
  };
  return eng;
}
