import { defineConfig } from 'vite';

export default defineConfig({
  base: './',
  server: { fs: { allow: ['../..'] }, port: 5173 },
  build: { target: 'es2022', chunkSizeWarningLimit: 900, sourcemap: false, assetsInlineLimit: 0 },
  worker: { format: 'es' },
});
