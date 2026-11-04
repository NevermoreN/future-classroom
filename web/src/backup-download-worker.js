/* A narrow, uncached download bridge. Credentials exist only in this worker's
 * memory, never in URLs, CacheStorage, IndexedDB or installation state. */
const prepared = new Map()
self.addEventListener('install', event => event.waitUntil(self.skipWaiting()))
self.addEventListener('activate', event => event.waitUntil(self.clients.claim()))
self.addEventListener('message', event => {
  const input = event.data, port = event.ports[0]
  if (!port || !input || input.type !== 'PREPARE' || !/^[0-9a-f]{64}$/.test(input.nonce)) return
  let url
  try { url = new URL(input.url) } catch { port.postMessage({ type: 'ERROR' }); return }
  const origin = new URL(self.registration.scope).origin
  if (url.origin !== origin || !/^\/fc\/api\/v1\/admin\/maintenance\/jobs\/[1-9][0-9]*\/file$/.test(url.pathname) || url.search || url.hash || !Number.isSafeInteger(input.bytes) || input.bytes <= 0 || input.bytes > 18 * 1024 ** 3 + 8 * 1024 ** 2 + 20 || typeof input.fileName !== 'string' || /[\\/\x00-\x1f]/.test(input.fileName)) { port.postMessage({ type: 'ERROR' }); return }
  const controller = new AbortController(), entry = { ...input, port, controller, used: false }
  if (prepared.size >= 2 || prepared.has(input.nonce)) { port.postMessage({ type: 'ERROR' }); return }
  prepared.set(input.nonce, entry)
  const timer = setTimeout(() => { if (prepared.delete(input.nonce)) port.close() }, 60000)
  port.onmessage = message => { if (message.data?.type === 'CANCEL') { prepared.delete(input.nonce); controller.abort(); clearTimeout(timer); port.close() } }
  entry.timer = timer
  port.postMessage({ type: 'READY', bridge: new URL('fc-download-bridge/' + input.nonce, self.registration.scope).href, download: new URL('fc-download/' + input.nonce, self.registration.scope).href })
})
self.addEventListener('fetch', event => {
  const url = new URL(event.request.url), scope = new URL(self.registration.scope), path = url.pathname.slice(scope.pathname.length)
  if (url.origin !== scope.origin || !url.pathname.startsWith(scope.pathname)) return
  const bridge = /^fc-download-bridge\/([0-9a-f]{64})$/.exec(path)
  if (bridge) { event.respondWith(Promise.resolve(new Response('<!doctype html><html lang="zh-CN"><title>未来教室备份下载</title><body></body></html>', { headers: { 'Content-Type': 'text/html;charset=utf-8', 'Cache-Control': 'no-store', 'Content-Security-Policy': "default-src 'none'; frame-ancestors 'self'" } }))); return }
  const match = /^fc-download\/([0-9a-f]{64})$/.exec(path)
  if (!match) return
  const entry = prepared.get(match[1])
  if (!entry || entry.used || event.request.method !== 'GET') { event.respondWith(Promise.resolve(new Response('Download unavailable', { status: 404, headers: { 'Cache-Control': 'no-store' } }))); return }
  entry.used = true; prepared.delete(match[1]); clearTimeout(entry.timer)
  let finish
  event.waitUntil(new Promise(resolve => { finish = resolve }))
  event.respondWith((async () => {
    try {
      const response = await fetch(entry.url, { headers: entry.headers, credentials: 'same-origin', cache: 'no-store', redirect: 'error', referrerPolicy: 'same-origin', signal: entry.controller.signal })
      if (response.status !== 200 || !response.body || response.headers.get('Content-Length') !== String(entry.bytes)) { await response.body?.cancel(); throw new Error('Invalid download response') }
      entry.headers = undefined
      let bytes = 0, reported = 0
      const reader = response.body.getReader()
      const stream = new ReadableStream({
        async pull(controller) {
          try {
            const result = await reader.read()
            if (result.done) {
              if (bytes !== entry.bytes) throw new Error('Incomplete download')
              controller.close(); entry.port.postMessage({ type: 'DONE', bytes }); entry.port.close(); finish(); return
            }
            bytes += result.value.byteLength
            if (bytes > entry.bytes) throw new Error('Download exceeded expected length')
            controller.enqueue(result.value)
            if (bytes - reported >= 1024 * 1024) { reported = bytes; entry.port.postMessage({ type: 'PROGRESS', bytes }) }
          } catch { entry.controller.abort(); await reader.cancel().catch(() => undefined); controller.error(new Error('Backup transfer failed')); entry.port.postMessage({ type: 'ERROR' }); entry.port.close(); finish() }
        },
        cancel() { entry.controller.abort(); entry.port.postMessage({ type: 'ERROR' }); entry.port.close(); finish(); return reader.cancel() },
      })
      return new Response(stream, { headers: { 'Content-Type': 'application/octet-stream', 'Content-Length': String(entry.bytes), 'Content-Disposition': "attachment; filename*=UTF-8''" + encodeURIComponent(entry.fileName), 'Cache-Control': 'no-store' } })
    } catch { entry.headers = undefined; entry.port.postMessage({ type: 'ERROR' }); entry.port.close(); finish(); return new Response('Download failed', { status: 502, headers: { 'Cache-Control': 'no-store' } }) }
  })())
})
