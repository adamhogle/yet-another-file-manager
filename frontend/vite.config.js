import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';

// https://vite.dev/config/
export default defineConfig({
  plugins: [vue()],
  build: {
    // Vite's copyPublicDir copy fails with EPERM on 9p checkouts (see
    // CONTRIBUTING.md); the build skips the copy and scripts/copy-public.mjs
    // copies every file in public/ into dist/ afterwards instead.
    copyPublicDir: false
  }
});
