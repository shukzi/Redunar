import { defineConfig } from 'vite';
import { fileURLToPath } from 'node:url';
import { readFileSync } from 'node:fs';
const spec = readFileSync(new URL('../../packaging/redunar-app.spec', import.meta.url), 'utf8');
const version = spec.match(/^Version:\s+([^%\s]+)/m)?.[1];
if (!version) throw new Error('Package version is missing');
export default defineConfig({
  plugins: [{
    name: 'redunar-product-version',
    transformIndexHtml(html) { return html.replaceAll('__REDUNAR_VERSION__', version); },
  }],
  root: 'ui', base: './',
  build: { rollupOptions: { input: {
    main: fileURLToPath(new URL('./ui/index.html', import.meta.url)),
  } } },
});
