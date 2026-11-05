import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import type { Plugin } from 'vite'

export function serviceWorkerSource(base: string, assets: string[], version: string): string {
  return `const PREFIX = ${JSON.stringify(`fc-static:${base}:`)};
const CACHE = PREFIX + ${JSON.stringify(version)};
const STATIC = ${JSON.stringify(assets.map(name => base + name))};
self.addEventListener('install', event => event.waitUntil((async () => {
  const cache = await caches.open(CACHE);
  // Keep classroom listener admission available: preload one resource at a time.
  for (const path of STATIC) await cache.addAll([new Request(path, { credentials: 'omit', cache: 'reload' })]);
})()));
// Let the browser activate an update after all old pages close: never reload a recording or draft.
self.addEventListener('activate', event => event.waitUntil((async () => {
  for (const key of await caches.keys()) if (key.startsWith(PREFIX) && key !== CACHE) await caches.delete(key);
})()));
self.addEventListener('fetch', event => {
  const request = event.request, url = new URL(request.url);
  if (request.method !== 'GET' || url.origin !== self.location.origin || url.search) return;
  if (STATIC.includes(url.pathname)) {
    event.respondWith(caches.open(CACHE).then(cache => cache.match(url.pathname)).then(hit => hit || fetch(request)));
  } else if (request.mode === 'navigate' && url.pathname === ${JSON.stringify(base)}) {
    event.respondWith(fetch(request).catch(() => caches.open(CACHE).then(cache => cache.match(${JSON.stringify(base + 'offline.html')}))));
  }
});
`
}

export function pwaBuild(): Plugin {
  let base = '/fc/'
  return {
    name: 'future-classroom-pwa',
    apply: 'build',
    configResolved(config) { base = config.base },
    generateBundle(_options, bundle) {
      const manifest = { id: base, name: '未来教室', short_name: '未来教室', lang: 'zh-CN', start_url: base, scope: base, display: 'standalone', background_color: '#f5f7fa', theme_color: '#f5f7fa', description: '安全连接教室，使用现有账号管理教学内容。', icons: [
        { src: 'pwa-icon-192.png', sizes: '192x192', type: 'image/png', purpose: 'any' },
        { src: 'pwa-icon-512.png', sizes: '512x512', type: 'image/png', purpose: 'any' },
      ] }
      const offline = '<!doctype html><html lang="zh-CN"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="theme-color" content="#f5f7fa"><title>未来教室 · 暂时离线</title><body><main><h1>暂时无法连接教室</h1><p>请连接学校网络后重新打开。离线时不能登录、查看教学数据或发送任务。</p><p>本页没有保存账号、作业、资料或录音。</p><a href="./">重新连接</a></main></body></html>'
      const extra: Record<string, string | Buffer> = {
        'manifest.webmanifest': JSON.stringify(manifest),
        'offline.html': offline,
        'pwa-icon-192.png': readFileSync(new URL('../src/assets/icons/pwa-192.png', import.meta.url)),
        'pwa-icon-512.png': readFileSync(new URL('../src/assets/icons/pwa-512.png', import.meta.url)),
      }
      for (const [fileName, source] of Object.entries(extra)) this.emitFile({ type: 'asset', fileName, source })
      const staticFiles = [...Object.keys(bundle).filter(name => /^assets\/[A-Za-z0-9_.-]+\.(js|css|png|svg)$/.test(name)), ...Object.keys(extra)].sort()
      const hash = createHash('sha256')
      for (const name of Object.keys(bundle).sort()) {
        const item = bundle[name]!
        hash.update(item.type === 'chunk' ? item.code : item.source)
      }
      for (const source of Object.values(extra)) hash.update(source)
      this.emitFile({ type: 'asset', fileName: 'sw.js', source: serviceWorkerSource(base, staticFiles, hash.digest('hex').slice(0, 16)) })
    },
  }
}
