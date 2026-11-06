import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { fileURLToPath, URL } from 'node:url'
import { readFileSync } from 'node:fs'
import { pwaBuild } from './scripts/pwa-build.ts'
export default defineConfig(({ command }) => ({
  base: (() => { const base = process.env.FC_WEB_BASE ?? '/fc/'; if (!/^\/(?:[A-Za-z0-9_-]+\/)*$/.test(base)) throw new Error('Invalid static base'); return base })(), plugins: [react(), tailwindcss(), pwaBuild()],
  resolve: { alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) } },
  build: { assetsInlineLimit: filePath => filePath.endsWith('.png') && !filePath.replaceAll('\\', '/').includes('/assets/tutorial/'), target: 'es2022', sourcemap: false, rolldownOptions: { output: { postBanner: command === 'build' ? '/*!\n' + readFileSync('THIRD-PARTY-NOTICES.txt', 'utf8').replaceAll('*/', '* /') + '\n*/' : '' } } },
  test: { include: ['tests/**/*.test.ts'], environment: 'node', restoreMocks: true },
}))
