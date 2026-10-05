// Designer image attachments: downscale so the long side is <= 1568 px, return an API image block.
import type { ImageBlock } from './transport';

export const MAX_SIDE = 1568;

export async function blobToImageBlock(blob: Blob): Promise<{ block: ImageBlock; thumbUrl: string; w: number; h: number }> {
  const bmp = await createImageBitmap(blob);
  const k = Math.min(1, MAX_SIDE / Math.max(bmp.width, bmp.height));
  const w = Math.max(1, Math.round(bmp.width * k)), h = Math.max(1, Math.round(bmp.height * k));
  const canvas = new OffscreenCanvas(w, h);
  const ctx = canvas.getContext('2d')!;
  ctx.fillStyle = '#ffffff';
  ctx.fillRect(0, 0, w, h);
  ctx.imageSmoothingQuality = 'high';
  ctx.drawImage(bmp, 0, 0, w, h);
  bmp.close();
  const jpeg = blob.type === 'image/jpeg';
  const out = await canvas.convertToBlob(jpeg ? { type: 'image/jpeg', quality: 0.9 } : { type: 'image/png' });
  const buf = new Uint8Array(await out.arrayBuffer());
  let s = '';
  for (let i = 0; i < buf.length; i += 0x8000) s += String.fromCharCode(...buf.subarray(i, i + 0x8000));
  return {
    block: { type: 'image', source: { type: 'base64', media_type: out.type, data: btoa(s) } },
    thumbUrl: URL.createObjectURL(out), w, h,
  };
}
