import { defineConfig } from 'vite';
import { fileURLToPath } from 'node:url';
export default defineConfig({
  root: 'ui', base: './',
  build: { rollupOptions: { input: {
    main: fileURLToPath(new URL('./ui/index.html', import.meta.url)),
    replayMenu: fileURLToPath(new URL('./ui/replay-menu.html', import.meta.url)),
  } } },
});
