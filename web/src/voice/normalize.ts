import { outputFrames, OUTPUT_RATE } from './pcm'
import { RecorderError, type NormalizedWav, type WavNormalizer } from './ports'

// Physical ownership belongs to this JS realm, not a mounted component/normalizer.
// OfflineAudioContext has no close/abort API. Public cancellation never returns this
// token; only the original render promise's actual resolution/rejection can do so.
let renderOwner: symbol | undefined

async function render(chunks: Float32Array[], sourceFrames: number, frames: number, rate: number, signal: AbortSignal): Promise<AudioBuffer> {
  if (renderOwner) throw new RecorderError('RECORDING_FAILED')
  const owner = Symbol()
  renderOwner = owner
  const release = () => { if (renderOwner === owner) renderOwner = undefined }
  let rendering: Promise<AudioBuffer> | undefined
  let source: AudioBufferSourceNode | undefined
  const disconnect = () => { source?.disconnect(); if (source) source.buffer = null }
  try {
    // Reserve before allocation: a remounted workspace cannot allocate a second
    // source AudioBuffer while an abandoned old render still owns its first one.
    const context = new OfflineAudioContext(1, frames, OUTPUT_RATE)
    const input = context.createBuffer(1, sourceFrames, rate)
    let offset = 0
    for (const chunk of chunks) { input.getChannelData(0).set(chunk, offset); offset += chunk.length }
    chunks.length = 0
    source = context.createBufferSource(); source.buffer = input; source.connect(context.destination); source.start()
    signal.addEventListener('abort', disconnect, { once: true })
    rendering = context.startRendering()
    void rendering.then(release, release)
    return await new Promise<AudioBuffer>((resolve, reject) => {
      const finish = (result?: AudioBuffer, error?: unknown) => {
        clearTimeout(timer); signal.removeEventListener('abort', abort)
        if (error) reject(error); else resolve(result!)
      }
      const abort = () => finish(undefined, new RecorderError('MIC_INTERRUPTED'))
      const timer = globalThis.setTimeout(() => finish(undefined, new RecorderError('RECORDING_FAILED')), 15000)
      signal.addEventListener('abort', abort, { once: true })
      void rendering!.then(buffer => finish(buffer), error => finish(undefined, error))
      if (signal.aborted) abort()
    })
  } finally {
    // Setup failure before startRendering returned cannot have a physical render.
    if (!rendering) release()
    signal.removeEventListener('abort', disconnect); disconnect()
  }
}

export class BrowserWavNormalizer implements WavNormalizer {
  async normalize(chunks: Float32Array[], rate: number, signal: AbortSignal): Promise<NormalizedWav> {
    if (signal.aborted) throw new RecorderError('MIC_INTERRUPTED')
    const sourceFrames = chunks.reduce((sum, chunk) => sum + chunk.length, 0)
    const frames = outputFrames(sourceFrames, rate)
    const rendered = await render(chunks, sourceFrames, frames, rate, signal)
    if (signal.aborted) throw new RecorderError('MIC_INTERRUPTED')
    const pcm = rendered.getChannelData(0).slice()
    // Always a separate, same-origin packaged worker; no blob/data URL or main-thread encoder fallback.
    const worker = new Worker(new URL('./wav-worker.ts', import.meta.url), { type: 'module' })
    const wav = await new Promise<ArrayBuffer>((resolve, reject) => {
      let active = true
      const cleanup = () => { active = false; clearTimeout(timer); signal.removeEventListener('abort', abort); worker.terminate() }
      const fail = (code: 'MIC_INTERRUPTED' | 'RECORDING_FAILED') => { if (active) { cleanup(); reject(new RecorderError(code)) } }
      const abort = () => fail('MIC_INTERRUPTED')
      const timer = globalThis.setTimeout(() => fail('RECORDING_FAILED'), 15000)
      signal.addEventListener('abort', abort, { once: true })
      worker.onerror = () => fail('RECORDING_FAILED')
      worker.onmessageerror = () => fail('RECORDING_FAILED')
      worker.onmessage = event => {
        if (!active) return
        const result = event.data?.wav
        if (!(result instanceof ArrayBuffer) || result.byteLength !== 44 + frames * 2) { fail('RECORDING_FAILED'); return }
        cleanup(); resolve(result)
      }
      if (signal.aborted) { abort(); return }
      try { worker.postMessage(pcm, [pcm.buffer]) } catch { fail('RECORDING_FAILED') }
    })
    if (signal.aborted) throw new RecorderError('MIC_INTERRUPTED')
    return { wav: new Blob([wav], { type: 'audio/wav' }), frames, sampleRate: OUTPUT_RATE }
  }
}
