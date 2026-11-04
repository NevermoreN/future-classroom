import workerUrl from './backup-download-worker.js?url'
import { ApiError, abortError, NetworkFault } from './api/client'
import { ProtocolFault } from './api/protocol'
interface SaveHandle { createWritable(): Promise<WritableStream<Uint8Array>> }
type SavePicker = (options: { suggestedName: string; types: { description: string; accept: Record<string, string[]> }[] }) => Promise<SaveHandle>
export async function downloadMaintenance(request: { url: string; headers: Record<string, string> }, fileName: string, bytes: number, signal: AbortSignal, progress: (bytes: number) => void): Promise<void> {
  if (signal.aborted) throw abortError()
  const picker = (window as Window & { showSaveFilePicker?: SavePicker }).showSaveFilePicker
  if (picker) {
    const handle = await picker.call(window, { suggestedName: fileName, types: [{ description: '未来教室完整备份', accept: { 'application/octet-stream': ['.fcbak'] } }] })
    if (signal.aborted) throw abortError()
    const output = await handle.createWritable(), writer = output.getWriter()
    try {
      const response = await fetch(request.url, { headers: request.headers, credentials: 'same-origin', cache: 'no-store', redirect: 'error', referrerPolicy: 'same-origin', signal })
      if (response.status === 401 || response.status === 403) { await response.body?.cancel(); throw new ApiError(response.status, response.status === 401 ? 'UNAUTHORIZED' : 'FORBIDDEN') }
      if (response.status !== 200 || !response.body || response.headers.get('Content-Length') !== String(bytes)) { await response.body?.cancel(); throw new ProtocolFault() }
      const reader = response.body.getReader(); let written = 0
      try { while (true) { const result = await reader.read(); if (result.done) break; written += result.value.byteLength; if (written > bytes) { await reader.cancel(); throw new ProtocolFault() } await writer.write(new Uint8Array(result.value)); progress(written) } } finally { reader.releaseLock() }
      if (written !== bytes) throw new ProtocolFault()
      await writer.close(); return
    } catch (error) { await writer.abort().catch(() => undefined); throw error }
    finally { writer.releaseLock() }
  }
  if (!('serviceWorker' in navigator)) throw new Error('此浏览器无法执行流式备份下载，请在本机 Microsoft Edge 或支持 Service Worker 的浏览器打开管理页面。')
  const registration = await navigator.serviceWorker.register(workerUrl, { scope: new URL('.', new URL(workerUrl, window.location.href)).pathname })
  const worker = registration.active ?? registration.installing ?? registration.waiting
  if (!worker) throw new NetworkFault()
  if (worker.state !== 'activated') await new Promise<void>((resolve, reject) => { const timer = setTimeout(() => { worker.removeEventListener('statechange', changed); reject(new NetworkFault()) }, 30000); const changed = () => { if (worker.state === 'activated') { clearTimeout(timer); worker.removeEventListener('statechange', changed); resolve() } else if (worker.state === 'redundant') { clearTimeout(timer); worker.removeEventListener('statechange', changed); reject(new NetworkFault()) } }; worker.addEventListener('statechange', changed) })
  const nonce = Array.from(crypto.getRandomValues(new Uint8Array(32)), byte => byte.toString(16).padStart(2, '0')).join(''), channel = new MessageChannel(), frame = document.createElement('iframe')
  frame.hidden = true; frame.title = '完整备份下载'; let timeout: ReturnType<typeof setTimeout> | undefined
  try {
    await new Promise<void>((resolve, reject) => {
      const abort = () => { channel.port1.postMessage({ type: 'CANCEL' }); finish(abortError()) }; signal.addEventListener('abort', abort, { once: true })
      const finish = (error?: Error) => { signal.removeEventListener('abort', abort); if (timeout) clearTimeout(timeout); if (error) { channel.port1.postMessage({ type: 'CANCEL' }); reject(error) } else resolve() }
      timeout = setTimeout(() => finish(new NetworkFault()), 30000)
      channel.port1.onmessage = event => {
        const data = event.data as { type?: string; bridge?: string; download?: string; bytes?: number }
        if (data.type === 'READY' && typeof data.bridge === 'string' && typeof data.download === 'string') { const bridge = new URL(data.bridge), download = new URL(data.download); if (bridge.origin !== window.location.origin || download.origin !== window.location.origin || !bridge.pathname.endsWith('/fc-download-bridge/' + nonce) || !download.pathname.endsWith('/fc-download/' + nonce)) { finish(new ProtocolFault()); return } if (timeout) clearTimeout(timeout); timeout = setTimeout(() => finish(new NetworkFault()), 30 * 60 * 1000); frame.onload = () => { frame.onload = null; frame.contentWindow?.location.assign(download.href) }; frame.src = bridge.href; document.body.append(frame) }
        else if (data.type === 'PROGRESS' && Number.isSafeInteger(data.bytes) && data.bytes! >= 0 && data.bytes! <= bytes) progress(data.bytes!)
        else if (data.type === 'DONE' && data.bytes === bytes) { progress(bytes); finish() }
        else if (data.type === 'ERROR') finish(new NetworkFault())
      }
      worker.postMessage({ type: 'PREPARE', nonce, ...request, fileName, bytes }, [channel.port2])
    })
  } finally { if (timeout) clearTimeout(timeout); frame.remove(); channel.port1.close(); await registration.unregister().catch(() => false) }
}
