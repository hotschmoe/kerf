// Executes the three Kerf tools (spec/llm/tools.json) against the App and shapes tool_result content.
import type { App } from '../app';
import type { Diagnostic, Op } from '../types';
import type { ImageBlock, TextBlock } from './transport';
import { rasterizeDrawing, rasterizeSvg } from '../raster';
import { scaleLabel } from '../units';

export interface ToolOutcome {
  content: (TextBlock | ImageBlock)[];
  is_error: boolean;
  /** one-line card header e.g. "APPLY  6 OPS" */
  title: string;
  /** right-hand status e.g. "✓ 0 ERR 1 WARN" */
  status: string;
  detail: string;     // text shown when the card line is expanded
  thumb?: Blob;       // render thumbnail
}

export function fmtDiagnostics(ds: Diagnostic[]): string {
  return ds.map((d) => {
    const tag = d.level === 'error' ? 'ERROR' : d.level === 'warning' ? 'WARN' : 'INFO';
    return `${tag} ${d.code}${d.id ? ' ' + d.id : ''}${d.path ? ' @' + d.path : ''}: ${d.message}${d.fix ? ' FIX: ' + d.fix : ''}`;
  }).join('\n');
}

const text = (t: string): TextBlock => ({ type: 'text', text: t });
const plural = (n: number, w: string) => `${n} ${w}${n === 1 ? '' : 'S'}`;

export async function runTool(app: App, name: string, input: Record<string, unknown>): Promise<ToolOutcome> {
  try {
    if (name === 'kerf_apply') return await toolApply(app, input);
    if (name === 'kerf_inspect') return await toolInspect(app, input);
    if (name === 'kerf_render') return await toolRender(app, input);
    return fail(name.toUpperCase(), `unknown tool "${name}". Available: kerf_apply, kerf_inspect, kerf_render.`);
  } catch (e) {
    return fail(name.replace('kerf_', '').toUpperCase(), `${app.describeError(e)}`);
  }
}

function fail(title: string, msg: string): ToolOutcome {
  return { content: [text('ERROR: ' + msg)], is_error: true, title, status: '✗ ERROR', detail: msg };
}

async function toolApply(app: App, input: Record<string, unknown>): Promise<ToolOutcome> {
  const ops = input.ops as Op[] | undefined;
  const why = typeof input.why === 'string' ? input.why : 'Edit';
  const title = `APPLY  ${plural(Array.isArray(ops) ? ops.length : 0, 'OP')}`;
  if (!Array.isArray(ops) || ops.length === 0) return fail(title, 'kerf_apply needs a non-empty "ops" array and a "why" line.');
  const res = await app.applyOps(ops, 'llm', why);
  const c = (lvl: Diagnostic['level']) => res.diagnostics.filter((d) => d.level === lvl).length;
  const diag = fmtDiagnostics(res.diagnostics);
  if (!res.ok) {
    const body = ['ERROR: apply rejected, nothing changed.', res.error ?? '', diag, 'Fix the ops and resend.'].filter(Boolean).join('\n');
    return { content: [text(body)], is_error: true, title, status: `✗ ${c('error')} ERR`, detail: body };
  }
  const body = ['ok', res.summary, diag].filter(Boolean).join('\n');
  return { content: [text(body)], is_error: false, title, status: `✓ ${c('error')} ERR ${c('warning')} WARN`, detail: JSON.stringify(ops, null, 2) };
}

async function toolInspect(app: App, input: Record<string, unknown>): Promise<ToolOutcome> {
  const q = String(input.q ?? '');
  const title = `INSPECT  ${q.toUpperCase()}${input.id ? ' ' + String(input.id) : input.type ? ' ' + String(input.type) : ''}`;
  if (!app.doc && q !== 'catalog') return fail(title, 'there is no document yet. Build one with kerf_apply (op "set", path "doc").');
  let out: string;
  if (q === 'doc') out = JSON.stringify(app.doc, null, 2);
  else if (q === 'summary') {
    const r = (await app.inspect({ q: 'summary' })) as unknown;
    out = typeof r === 'string' ? r : (r as { summary?: string })?.summary ?? JSON.stringify(r, null, 2);
  } else {
    const r = await app.inspect(input as never);
    out = typeof r === 'string' ? r : JSON.stringify(r, null, 2);
  }
  return { content: [text(out)], is_error: false, title, status: '✓', detail: out.length > 4000 ? out.slice(0, 4000) + '\n…' : out };
}

async function toolRender(app: App, input: Record<string, unknown>): Promise<ToolOutcome> {
  const viewId = String(input.view ?? '');
  const mode = input.mode === 'sheet' ? 'sheet' : 'view';
  const title = `RENDER  VIEW ${viewId}${mode === 'sheet' ? ' SHEET' : ''}`;
  const doc = app.doc;
  if (!doc) return fail(title, 'there is no document yet. Build one with kerf_apply first.');
  const view = doc.views.find((v) => v.id === viewId);
  if (!view) return fail(title, `no view "${viewId}". Views: ${doc.views.map((v) => v.id).join(', ') || '(none)'}.`);
  let r;
  let notes = 0, drawingDiags: Diagnostic[] = app.diagnostics;
  notes = (view.annotations ?? []).filter((a) => a.type === 'note').length;
  let factor = 0;
  if (mode === 'sheet') {
    r = await rasterizeSvg(await app.getSheetSvg(viewId), 1400);
  } else {
    const model = await app.getDrawingModel(viewId);
    factor = model.drawing.scale;
    if (model.drawing.diagnostics?.length) drawingDiags = model.drawing.diagnostics;
    r = await rasterizeDrawing(model, 1400);
  }
  const errs = drawingDiags.filter((d) => d.level === 'error').length;
  const warns = drawingDiags.filter((d) => d.level === 'warning').length;
  const scale = typeof view.scale === 'string' ? view.scale : factor ? scaleLabel(factor) : '';
  const caption = `view ${viewId} rendered${scale ? ' at ' + scale : ''}; ${notes} notes; ${errs} errors, ${warns} warnings` + (mode === 'sheet' ? ' (full sheet with title block)' : '');
  return {
    content: [{ type: 'image', source: { type: 'base64', media_type: 'image/png', data: r.base64 } }, text(caption)],
    is_error: false, title, status: '✓', detail: `${caption}\n${r.width}x${r.height} PNG`, thumb: r.blob,
  };
}
