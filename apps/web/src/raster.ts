// kerf_render: rasterize a Drawing (or the engine's sheet SVG) to PNG on an OffscreenCanvas.
import { DrawingModel, fitView, paintBatches } from './draw2d';

export interface Raster { base64: string; width: number; height: number; blob: Blob }

async function toBase64(blob: Blob): Promise<string> {
  const buf = new Uint8Array(await blob.arrayBuffer());
  let s = '';
  const CH = 0x8000;
  for (let i = 0; i < buf.length; i += CH) s += String.fromCharCode(...buf.subarray(i, i + CH));
  return btoa(s);
}

export async function rasterizeDrawing(model: DrawingModel, longSide = 1400): Promise<Raster> {
  const b = model.bounds;
  const bw = Math.max(b[2] - b[0], 1), bh = Math.max(b[3] - b[1], 1);
  const pad = 28;
  const aspect = bh / bw;
  let w = longSide, h = Math.round(longSide * aspect);
  if (h > longSide) { h = longSide; w = Math.round(longSide / aspect); }
  w = Math.max(w, 400); h = Math.max(h, 300);
  const canvas = new OffscreenCanvas(w, h);
  const ctx = canvas.getContext('2d')!;
  ctx.fillStyle = '#ffffff';
  ctx.fillRect(0, 0, w, h);
  const v = fitView(b, w, h, pad);
  ctx.setTransform(v.zoom, 0, 0, -v.zoom, v.tx, v.ty);
  paintBatches(ctx as unknown as CanvasRenderingContext2D, model, v.zoom, { ink: '#000000', dpr: 1, minPx: 1 });
  const blob = await canvas.convertToBlob({ type: 'image/png' });
  return { base64: await toBase64(blob), width: w, height: h, blob };
}

export async function rasterizeSvg(svg: string, longSide = 1400): Promise<Raster> {
  const url = URL.createObjectURL(new Blob([svg], { type: 'image/svg+xml' }));
  try {
    const img = new Image();
    img.src = url;
    await img.decode();
    const iw = img.naturalWidth || 1056, ih = img.naturalHeight || 816;
    const k = longSide / Math.max(iw, ih);
    const w = Math.round(iw * k), h = Math.round(ih * k);
    const canvas = new OffscreenCanvas(w, h);
    const ctx = canvas.getContext('2d')!;
    ctx.fillStyle = '#ffffff';
    ctx.fillRect(0, 0, w, h);
    ctx.drawImage(img, 0, 0, w, h);
    const blob = await canvas.convertToBlob({ type: 'image/png' });
    return { base64: await toBase64(blob), width: w, height: h, blob };
  } finally {
    URL.revokeObjectURL(url);
  }
}
