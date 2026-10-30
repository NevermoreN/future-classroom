import { encodeWav } from './pcm'

const scope = globalThis as unknown as {
  onmessage: ((event: MessageEvent<unknown>) => void) | null
  postMessage(message: unknown, transfer: Transferable[]): void
}
scope.onmessage = event => {
  try {
    if (!(event.data instanceof Float32Array)) throw new Error('INVALID_PCM')
    const wav = encodeWav(event.data)
    scope.postMessage({ wav }, [wav])
  } catch {
    scope.postMessage({ error: 'INVALID_PCM' }, [])
  }
}
