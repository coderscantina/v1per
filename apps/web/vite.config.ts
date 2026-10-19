import { fileURLToPath } from 'node:url'

import tailwindcss from '@tailwindcss/vite'
import vue from '@vitejs/plugin-vue'
import AutoImport from 'unplugin-auto-import/vite'
import { defineConfig } from 'vitest/config'

/** The Rust server during development: `v1per serve` on its default address. */
const apiTarget = process.env.V1PER_DEV_API ?? 'http://127.0.0.1:8080'

export default defineConfig({
  plugins: [
    // Only the framework APIs, as in Kit. Composables are imported explicitly.
    AutoImport({
      imports: ['vue', 'vue-router'],
      dts: 'src/auto-imports.d.ts',
      vueTemplate: true,
    }),
    vue(),
    tailwindcss(),
  ],
  resolve: {
    alias: { '~': fileURLToPath(new URL('./src', import.meta.url)) },
  },
  server: {
    port: 5173,
    strictPort: true,
    // changeOrigin stays off: commands must carry the browser's own origin.
    proxy: {
      '/api': apiTarget,
      '/config.json': apiTarget,
    },
  },
  build: {
    target: 'baseline-widely-available',
    rolldownOptions: {
      // Two pages in one `dist`: the admin app (`index.html`, every client route) and the public
      // status page (`status.html`, which the server will send for `/s/<slug>`). They share
      // hashed chunks under `assets/`, but the status page never loads admin code.
      input: {
        admin: fileURLToPath(new URL('./index.html', import.meta.url)),
        status: fileURLToPath(new URL('./status.html', import.meta.url)),
      },
      output: {
        // Stable vendors in their own chunks stay cached across deploys. Vue itself is apart
        // from the router and i18n, so the status page loads only what it uses.
        codeSplitting: {
          groups: [
            { name: 'vue', test: /node_modules[\\/](vue|@vue)[\\/]/ },
            { name: 'vue-app', test: /node_modules[\\/](vue-router|vue-i18n|@intlify)[\\/]/ },
            { name: 'query', test: /node_modules[\\/]@tanstack[\\/]/ },
            { name: 'reka', test: /node_modules[\\/](reka-ui|@floating-ui)[\\/]/ },
            // The renderer and the Lucide subset it draws (`src/lib/icons.ts`), which changes only
            // when an icon is added.
            { name: 'icons', test: /node_modules[\\/]@iconify[\\/]|src[\\/]lib[\\/]icons\.ts$/ },
          ],
        },
      },
    },
  },
  test: {
    include: ['src/**/*.test.ts'],
    environment: 'node',
  },
})
