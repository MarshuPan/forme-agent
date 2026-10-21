import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  envDir: false,
  plugins: [react()],
  build: { sourcemap: false, assetsInlineLimit: 0 },
  server: { host: '127.0.0.1', strictPort: true },
});
