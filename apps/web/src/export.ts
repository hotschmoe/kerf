import type { App } from './app';
import { download } from './ui/dom';

const MIME = { dxf: 'application/dxf', pdf: 'application/pdf', svg: 'image/svg+xml' } as const;
export type ExportFormat = keyof typeof MIME;

export interface ExportResult { name: string; bytes: Uint8Array; ms: number }

/** Export the active view as `<docid>-<view>.<ext>` and trigger a browser download. */
export async function exportActive(app: App, format: ExportFormat, save = true): Promise<ExportResult | null> {
  if (!app.doc) { app.flash('NOTHING TO EXPORT: NO DETAIL LOADED', 'warn'); return null; }
  const view = app.activeView || app.doc.views[0]?.id;
  if (!view) { app.flash('NOTHING TO EXPORT: THE DOCUMENT HAS NO VIEWS', 'warn'); return null; }
  const t0 = performance.now();
  try {
    // PDF is always a sheet; SVG is a sheet when the SHEET tab is showing; DXF is model-space only.
    const sheet = format === 'pdf' || (format === 'svg' && app.mode === 'sheet');
    const bytes = await app.engine.exportBytes(app.doc, app.style, view, format, sheet);
    const name = `${app.doc.id}-${view}.${format}`;
    const ms = performance.now() - t0;
    if (save) download(name, bytes as BlobPart, MIME[format]);
    app.flash(`EXPORTED ${name.toUpperCase()} (${Math.max(1, Math.round(bytes.length / 1024))} KB)`);
    app.perf[`export_${format}_ms`] = ms;
    (window as unknown as { __lastExport?: unknown }).__lastExport = { name, size: bytes.length };
    return { name, bytes, ms };
  } catch (e) {
    app.flash(`EXPORT FAILED: ${app.describeError(e)}`.slice(0, 160), 'err');
    return null;
  }
}
