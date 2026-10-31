import { afterEach, describe, expect, it, vi } from 'vitest'
import { BrowserWavNormalizer } from '../src/voice/normalize'
import { encodeWav } from '../src/voice/pcm'
import { RecorderController, type CaptureSink } from '../src/voice/recorder'

function deferred<T>() {
  let resolve!: (value: T) => void, reject!: (reason: unknown) => void
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}
function environment(mode = 'success') {
  const contexts: Context[] = [], workers: Encoder[] = []
  const rendering = deferred<AudioBuffer>()
  class Context {
    destination = {}
    input = new Float32Array(0)
    source = { buffer: null as unknown, connect: vi.fn(), start: vi.fn(), disconnect: vi.fn() }
    rendered = { getChannelData: () => new Float32Array(this.frames).fill(0.5) } as unknown as AudioBuffer
    constructor(readonly channels: number, readonly frames: number, readonly rate: number) { contexts.push(this) }
    createBuffer = vi.fn((_channels: number, frames: number, _rate: number) => {
      this.input = new Float32Array(frames)
      return { getChannelData: () => this.input }
    })
    createBufferSource = vi.fn(() => this.source)
    startRendering = vi.fn(() => {
      if (mode === 'render-throws') throw new Error('render setup unavailable')
      return mode.startsWith('render-') ? rendering.promise : Promise.resolve(this.rendered)
    })
  }
  class Encoder {
    onmessage: ((event: { data: unknown }) => void) | null = null
    onerror: (() => void) | null = null
    onmessageerror: (() => void) | null = null
    terminate = vi.fn()
    postMessage = vi.fn((pcm: Float32Array, transfer: Transferable[]) => {
      expect(transfer).toEqual([pcm.buffer])
      if (mode === 'post-throws') throw new Error('private worker failure')
      if (mode === 'worker-pending') return
      queueMicrotask(() => {
        if (mode === 'worker-error') this.onerror?.()
        else if (mode === 'worker-malformed') this.onmessage?.({ data: { wav: new ArrayBuffer(1) } })
        else this.onmessage?.({ data: { wav: encodeWav(pcm) } })
      })
    })
    constructor(readonly url: URL, readonly options: unknown) { workers.push(this) }
  }
  vi.stubGlobal('OfflineAudioContext', Context); vi.stubGlobal('Worker', Encoder)
  return { contexts, workers, rendering, normalizer: new BrowserWavNormalizer(), abort: new AbortController() }
}
afterEach(() => { vi.unstubAllGlobals(); vi.useRealTimers() })
describe('bounded offline resampling and disposable encoding worker', () => {
  it('normalizes the floor-quantized 500ms cap through recorder and real normalizer at odd source rates', async () => {
    for (const rate of [11025, 44101, 95999]) {
      const f = environment(), frames = Math.floor(rate / 2)
      let sink!: CaptureSink
      const onWav = vi.fn(), onError = vi.fn()
      const controller = new RecorderController({ onWav, onError, normalizer: f.normalizer,
        capturePort: { open: async next => {
          sink = next
          return { sampleRate: rate, start: async () => {}, stop: async () => {}, abort: () => {} }
        } } })
      try {
        await controller.start(`minimum-quantized-${rate}`, 500)
        sink.pcm(new Float32Array(frames).fill(0.25), rate)
        await controller.finish(`minimum-quantized-${rate}`)
        expect(onError).not.toHaveBeenCalled()
        expect(onWav).toHaveBeenCalledOnce()
        const result = onWav.mock.calls[0]![0] as { frames: number; sampleRate: number; wav: Blob }
        expect(result.frames).toBe(8000); expect(result.sampleRate).toBe(16000)
        expect(result.wav.size).toBe(16044); expect(result.wav.type).toBe('audio/wav')
        expect(f.contexts[0]!.frames).toBe(8000)
        expect(f.contexts[0]!.createBuffer).toHaveBeenCalledWith(1, frames, rate)
        expect(f.workers[0]!.terminate).toHaveBeenCalledOnce()
      } finally { controller.dispose() }
    }
  })
  it('rejects one whole source frame below the floor-quantized minimum before resampling or encoding', async () => {
    for (const rate of [8000, 11025, 44100, 44101, 95999, 96000]) {
      const f = environment()
      await expect(f.normalizer.normalize([new Float32Array(Math.floor(rate / 2) - 1)], rate, f.abort.signal)).rejects.toThrow('DURATION_OUT_OF_RANGE')
      expect(f.contexts).toHaveLength(0); expect(f.workers).toHaveLength(0)
    }
  })
  it('uses actual source rate, one offline channel and exact 16k output frames before worker encoding', async () => {
    const f = environment(), chunks = [new Float32Array(11025).fill(1), new Float32Array(11025).fill(-1)]
    const result = await f.normalizer.normalize(chunks, 44100, f.abort.signal), context = f.contexts[0]!
    expect(context.channels).toBe(1); expect(context.frames).toBe(8000); expect(context.rate).toBe(16000)
    expect(context.createBuffer).toHaveBeenCalledWith(1, 22050, 44100)
    expect(context.input[0]).toBe(1); expect(context.input[11025]).toBe(-1); expect(chunks).toHaveLength(0)
    expect(result.frames).toBe(8000); expect(result.wav.size).toBe(16044); expect(result.wav.type).toBe('audio/wav')
    const view = new DataView(await result.wav.arrayBuffer()); expect(view.getInt16(44, true)).toBe(16384)
    expect(f.workers[0]!.options).toEqual({ type: 'module' }); expect(f.workers[0]!.url.pathname).toContain('wav-worker')
    expect(f.workers[0]!.terminate).toHaveBeenCalledOnce(); expect(context.source.disconnect).toHaveBeenCalled()
  })
  it('aborted normalization never allocates context or worker', async () => {
    const f = environment(); f.abort.abort()
    await expect(f.normalizer.normalize([new Float32Array(8000)], 16000, f.abort.signal)).rejects.toMatchObject({ code: 'MIC_INTERRUPTED' })
    expect(f.contexts).toHaveLength(0); expect(f.workers).toHaveLength(0)
  })
  it('rejects invalid source duration before allocating browser buffers', async () => {
    const f = environment(); await expect(f.normalizer.normalize([new Float32Array(7999)], 16000, f.abort.signal)).rejects.toThrow('DURATION_OUT_OF_RANGE')
    expect(f.contexts).toHaveLength(0)
  })
  it('cancelled offline rendering returns promptly, cannot spawn a worker later, and cannot accumulate abandoned contexts', async () => {
    const f = environment('render-pending'), operation = f.normalizer.normalize([new Float32Array(8000)], 16000, f.abort.signal)
    f.abort.abort(); await expect(operation).rejects.toMatchObject({ code: 'MIC_INTERRUPTED' })
    expect(f.contexts[0]!.source.disconnect).toHaveBeenCalled()
    await expect(f.normalizer.normalize([new Float32Array(8000)], 16000, new AbortController().signal)).rejects.toMatchObject({ code: 'RECORDING_FAILED' })
    expect(f.contexts).toHaveLength(1); f.rendering.resolve(f.contexts[0]!.rendered); await Promise.resolve(); await Promise.resolve()
    expect(f.workers).toHaveLength(0)
  })
  it('cancelled physical render retains the shared owner across different normalizer instances until actual settlement', async () => {
    const f = environment('render-pending'), nextAbort = new AbortController()
    const first = f.normalizer.normalize([new Float32Array(8000)], 16000, f.abort.signal)
    f.abort.abort(); await expect(first).rejects.toMatchObject({ code: 'MIC_INTERRUPTED' })
    const next = new BrowserWavNormalizer().normalize([new Float32Array(8000)], 16000, nextAbort.signal)
    void next.catch(() => {})
    try {
      expect(f.contexts).toHaveLength(1)
      await expect(next).rejects.toMatchObject({ code: 'RECORDING_FAILED' })
      expect(f.workers).toHaveLength(0)
    } finally {
      nextAbort.abort(); f.rendering.resolve(f.contexts[0]!.rendered); await next.catch(() => {}); await Promise.resolve()
    }
    const recovered = environment()
    await expect(recovered.normalizer.normalize([new Float32Array(8000)], 16000, recovered.abort.signal)).resolves.toMatchObject({ frames: 8000 })
  })
  it('disposing an old recorder and constructing a new recorder cannot mint a second unsettled physical render', async () => {
    const f = environment('render-pending')
    const recorder = () => {
      let sink!: CaptureSink
      const onWav = vi.fn(), onError = vi.fn()
      const controller = new RecorderController({ onWav, onError, normalizer: new BrowserWavNormalizer(),
        capturePort: { open: async next => {
          sink = next
          return { sampleRate: 16000, start: async () => {}, stop: async () => {}, abort: () => {} }
        } } })
      return { controller, onWav, onError, pcm: () => sink.pcm(new Float32Array(8000), 16000) }
    }
    const old = recorder(), replacement = recorder()
    let oldFinish: Promise<void> | undefined, nextFinish: Promise<void> | undefined
    try {
      await old.controller.start('old-realm-owner'); old.pcm(); oldFinish = old.controller.finish('old-realm-owner')
      await Promise.resolve(); expect(f.contexts).toHaveLength(1)
      old.controller.dispose(); await oldFinish
      await replacement.controller.start('replacement-owner'); replacement.pcm(); nextFinish = replacement.controller.finish('replacement-owner')
      await Promise.resolve(); expect(f.contexts).toHaveLength(1)
      await nextFinish
      expect(replacement.onError).toHaveBeenCalledWith(expect.objectContaining({ code: 'RECORDING_FAILED' }))
      expect(replacement.onWav).not.toHaveBeenCalled(); expect(old.onWav).not.toHaveBeenCalled()
    } finally {
      old.controller.dispose(); replacement.controller.dispose(); f.rendering.resolve(f.contexts[0]!.rendered)
      await oldFinish; await nextFinish; await Promise.resolve()
    }
  })
  it('offline rendering has a finite deadline without inventing a nonexistent close API', async () => {
    vi.useFakeTimers(); const f = environment('render-pending'), operation = f.normalizer.normalize([new Float32Array(8000)], 16000, f.abort.signal)
    const rejected = expect(operation).rejects.toMatchObject({ code: 'RECORDING_FAILED' }); await vi.advanceTimersByTimeAsync(15000); await rejected
    expect(f.contexts[0]!.source.disconnect).toHaveBeenCalled(); expect(f.workers).toHaveLength(0); f.rendering.resolve(f.contexts[0]!.rendered)
  })
  it('render rejection does not start a worker and releases source references', async () => {
    const f = environment('render-reject'), operation = f.normalizer.normalize([new Float32Array(8000)], 16000, f.abort.signal)
    f.rendering.reject(new Error('offline unavailable')); await expect(operation).rejects.toThrow('offline unavailable')
    expect(f.contexts[0]!.source.disconnect).toHaveBeenCalled(); expect(f.contexts[0]!.source.buffer).toBeNull(); expect(f.workers).toHaveLength(0)
  })
  it('synchronous render setup failure still removes the source and does not retain physical render ownership', async () => {
    const f = environment('render-throws')
    await expect(f.normalizer.normalize([new Float32Array(8000)], 16000, f.abort.signal)).rejects.toThrow('render setup unavailable')
    expect(f.contexts[0]!.source.disconnect).toHaveBeenCalledOnce(); expect(f.contexts[0]!.source.buffer).toBeNull()
    await expect(f.normalizer.normalize([new Float32Array(8000)], 16000, f.abort.signal)).rejects.toThrow('render setup unavailable')
    expect(f.contexts).toHaveLength(2); expect(f.workers).toHaveLength(0)
  })
  it.each(['worker-error', 'worker-malformed', 'post-throws'])('%s terminates the encoding worker and never yields a WAV', async mode => {
    const f = environment(mode)
    await expect(f.normalizer.normalize([new Float32Array(8000)], 16000, f.abort.signal)).rejects.toMatchObject({ code: 'RECORDING_FAILED' })
    expect(f.workers[0]!.terminate).toHaveBeenCalledOnce()
  })
  it('aborting an encoding worker terminates it once and ignores a late WAV message', async () => {
    const f = environment('worker-pending'), operation = f.normalizer.normalize([new Float32Array(8000)], 16000, f.abort.signal)
    await Promise.resolve(); await Promise.resolve(); await Promise.resolve(); expect(f.workers).toHaveLength(1)
    f.abort.abort(); await expect(operation).rejects.toMatchObject({ code: 'MIC_INTERRUPTED' })
    f.workers[0]!.onmessage?.({ data: { wav: new ArrayBuffer(16044) } }); expect(f.workers[0]!.terminate).toHaveBeenCalledOnce()
  })
  it('worker deadline terminates an otherwise permanently pending encoder', async () => {
    vi.useFakeTimers(); const f = environment('worker-pending'), operation = f.normalizer.normalize([new Float32Array(8000)], 16000, f.abort.signal)
    const rejected = expect(operation).rejects.toMatchObject({ code: 'RECORDING_FAILED' }); await vi.advanceTimersByTimeAsync(15000); await rejected
    expect(f.workers[0]!.terminate).toHaveBeenCalledOnce()
  })
})
