import { afterEach, describe, expect, it, vi } from 'vitest'
import { BrowserCapturePort, recordingSupport } from '../src/voice/browser-capture'
import type { CaptureSink } from '../src/voice/ports'

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>(yes => { resolve = yes })
  return { promise, resolve }
}
class Track extends EventTarget {
  readyState = 'live'
  stop = vi.fn(() => { this.readyState = 'ended' })
}
function environment() {
  const contexts: Context[] = [], nodes: Node[] = []
  const tracks = [new Track(), new Track()]
  const stream = { getTracks: () => tracks, getAudioTracks: () => tracks } as unknown as MediaStream
  const getUserMedia = vi.fn().mockResolvedValue(stream)
  class Context extends EventTarget {
    state = 'running'
    sampleRate = 44100
    destination = {}
    module = vi.fn(async () => {})
    get audioWorklet() { return { addModule: this.module } }
    resume = vi.fn(async () => {})
    close = vi.fn(async () => { this.state = 'closed' })
    source = { connect: vi.fn(), disconnect: vi.fn() }
    mute = { gain: { value: 1 }, connect: vi.fn(), disconnect: vi.fn() }
    createMediaStreamSource = vi.fn(() => this.source)
    createGain = vi.fn(() => this.mute)
    constructor() { super(); contexts.push(this) }
  }
  class Node {
    port = {
      onmessage: null as ((event: { data: unknown }) => void) | null,
      postMessage: vi.fn((data: { type: string }) => {
        if (data.type === 'finish') queueMicrotask(() => this.port.onmessage?.({ data: { type: 'stopped' } }))
      }),
      close: vi.fn(),
    }
    onprocessorerror: (() => void) | null = null
    connect = vi.fn()
    disconnect = vi.fn()
    constructor(readonly context: Context, readonly name: string, readonly options: unknown) { nodes.push(this) }
  }
  vi.stubGlobal('location', { protocol: 'https:' }); vi.stubGlobal('isSecureContext', true)
  vi.stubGlobal('navigator', { mediaDevices: { getUserMedia } })
  vi.stubGlobal('AudioContext', Context); vi.stubGlobal('AudioWorkletNode', Node)
  vi.stubGlobal('OfflineAudioContext', class {}); vi.stubGlobal('Worker', class {})
  const sink: CaptureSink = { pcm: vi.fn(), interrupted: vi.fn(), limit: vi.fn() }
  const abort = new AbortController()
  return { contexts, nodes, tracks, stream, getUserMedia, sink, abort, port: new BrowserCapturePort() }
}
afterEach(() => { vi.unstubAllGlobals(); vi.useRealTimers() })
describe('browser microphone ownership without a real microphone', () => {
  it('requires HTTPS even on an otherwise secure localhost and never requests a microphone there', async () => {
    const f = environment(); vi.stubGlobal('location', { protocol: 'http:' })
    expect(recordingSupport().supported).toBe(false)
    await expect(f.port.open(f.sink, f.abort.signal)).rejects.toMatchObject({ code: 'INSECURE_CONTEXT' })
    expect(f.getUserMedia).not.toHaveBeenCalled(); expect(f.contexts).toHaveLength(0)
  })
  it('detects missing worklet/worker support before asking for permission', async () => {
    const f = environment(); vi.stubGlobal('Worker', undefined)
    expect(recordingSupport().reason).toContain('不支持')
    await expect(f.port.open(f.sink, f.abort.signal)).rejects.toMatchObject({ code: 'RECORDING_UNSUPPORTED' })
    expect(f.getUserMedia).not.toHaveBeenCalled()
  })
  it('stops every late permission track before any source or worklet is created', async () => {
    const f = environment(), permission = deferred<MediaStream>(); f.getUserMedia.mockReturnValueOnce(permission.promise)
    const open = f.port.open(f.sink, f.abort.signal); f.abort.abort()
    expect(f.contexts[0]!.close).toHaveBeenCalledOnce(); permission.resolve(f.stream)
    await expect(open).rejects.toMatchObject({ code: 'MIC_INTERRUPTED' })
    f.tracks.forEach(track => expect(track.stop).toHaveBeenCalled())
    expect(f.contexts[0]!.createMediaStreamSource).not.toHaveBeenCalled(); expect(f.nodes).toHaveLength(0)
  })
  it('maps permission denial and closes the already gesture-resumed audio context', async () => {
    const f = environment(); f.getUserMedia.mockRejectedValueOnce(new DOMException('private details', 'NotAllowedError'))
    await expect(f.port.open(f.sink, f.abort.signal)).rejects.toMatchObject({ code: 'MIC_DENIED' })
    expect(f.contexts[0]!.resume).toHaveBeenCalledOnce(); expect(f.contexts[0]!.close).toHaveBeenCalledOnce()
  })
  it('keeps the graph disconnected until start and explicitly mutes its output', async () => {
    const f = environment(), session = await f.port.open(f.sink, f.abort.signal), context = f.contexts[0]!, node = f.nodes[0]!
    expect(session.sampleRate).toBe(44100); expect(context.source.connect).not.toHaveBeenCalled()
    expect(f.getUserMedia.mock.calls[0]![0]).toEqual({ audio: { channelCount: { ideal: 1 } }, video: false })
    expect(node.options).toMatchObject({ channelCountMode: 'max', channelInterpretation: 'discrete', processorOptions: { maxDurationMillis: 60000 } })
    await session.start(); expect(context.source.connect).toHaveBeenCalledOnce(); expect(context.mute.gain.value).toBe(0)
    const chunk = new Float32Array(128); node.port.onmessage?.({ data: { type: 'pcm', chunk, sampleRate: 44100 } })
    expect(f.sink.pcm).toHaveBeenCalledWith(chunk, 44100); session.abort(); session.abort()
    expect(node.port.close).toHaveBeenCalledOnce(); expect(context.close).toHaveBeenCalledOnce()
  })
  it('stops microphone tracks synchronously before asynchronous worklet flush completes', async () => {
    const f = environment(), session = await f.port.open(f.sink, f.abort.signal); await session.start()
    const stop = session.stop(); f.tracks.forEach(track => expect(track.stop).toHaveBeenCalledOnce())
    await stop; expect(f.sink.interrupted).not.toHaveBeenCalled(); expect(f.contexts[0]!.close).toHaveBeenCalledOnce(); expect(f.nodes[0]!.port.close).toHaveBeenCalledOnce()
  })
  it('flush timeout still frees every microphone track, node, port and context', async () => {
    vi.useFakeTimers(); const f = environment(), session = await f.port.open(f.sink, f.abort.signal); await session.start()
    f.nodes[0]!.port.postMessage.mockImplementation(() => {})
    const rejected = expect(session.stop()).rejects.toMatchObject({ code: 'RECORDING_FAILED' })
    f.tracks.forEach(track => expect(track.stop).toHaveBeenCalled()); await vi.advanceTimersByTimeAsync(1000); await rejected
    expect(f.contexts[0]!.source.disconnect).toHaveBeenCalledOnce(); expect(f.nodes[0]!.port.close).toHaveBeenCalledOnce(); expect(f.contexts[0]!.close).toHaveBeenCalledOnce()
  })
  it('module setup failure closes the context and releases permission tracks', async () => {
    const f = environment(), permission = deferred<MediaStream>(); f.getUserMedia.mockReturnValueOnce(permission.promise)
    const open = f.port.open(f.sink, f.abort.signal); f.contexts[0]!.module.mockRejectedValueOnce(new Error('module unavailable')); permission.resolve(f.stream)
    await expect(open).rejects.toThrow('module unavailable'); f.tracks.forEach(track => expect(track.stop).toHaveBeenCalled()); expect(f.contexts[0]!.close).toHaveBeenCalledOnce()
  })
  it('a stalled worklet load has a bounded setup deadline and releases all granted tracks', async () => {
    vi.useFakeTimers(); const f = environment(), permission = deferred<MediaStream>(); f.getUserMedia.mockReturnValueOnce(permission.promise)
    const open = f.port.open(f.sink, f.abort.signal); f.contexts[0]!.module.mockReturnValueOnce(new Promise(() => {})); permission.resolve(f.stream)
    const rejected = expect(open).rejects.toMatchObject({ code: 'RECORDING_FAILED' }); await vi.advanceTimersByTimeAsync(10000); await rejected
    f.tracks.forEach(track => expect(track.stop).toHaveBeenCalled()); expect(f.contexts[0]!.close).toHaveBeenCalledOnce()
  })
  it('abort during worklet load rejects immediately even when the load never settles', async () => {
    const f = environment(), permission = deferred<MediaStream>(); f.getUserMedia.mockReturnValueOnce(permission.promise)
    const open = f.port.open(f.sink, f.abort.signal); f.contexts[0]!.module.mockReturnValueOnce(new Promise(() => {})); permission.resolve(f.stream)
    await Promise.resolve(); await Promise.resolve(); f.abort.abort()
    await expect(open).rejects.toMatchObject({ code: 'MIC_INTERRUPTED' }); f.tracks.forEach(track => expect(track.stop).toHaveBeenCalled())
  })
  it('track ended, interrupted context and processor errors notify cancellation, not normal release', async () => {
    const f = environment(), session = await f.port.open(f.sink, f.abort.signal); await session.start()
    f.tracks[0]!.dispatchEvent(new Event('ended')); f.contexts[0]!.state = 'suspended'; f.contexts[0]!.dispatchEvent(new Event('statechange')); f.nodes[0]!.onprocessorerror?.()
    expect(f.sink.interrupted).toHaveBeenCalledTimes(3); expect(f.nodes[0]!.port.postMessage).not.toHaveBeenCalledWith({ type: 'finish' }); session.abort()
  })
  it('abort while flush is pending resolves the bounded wait but never reports successful stop', async () => {
    const f = environment(), session = await f.port.open(f.sink, f.abort.signal); await session.start()
    f.nodes[0]!.port.postMessage.mockImplementation(() => {})
    const stop = session.stop(); f.abort.abort(); await expect(stop).rejects.toMatchObject({ code: 'MIC_INTERRUPTED' })
    f.tracks.forEach(track => expect(track.stop).toHaveBeenCalled()); expect(f.contexts[0]!.close).toHaveBeenCalledOnce()
  })
  it.each([500, 1250, 60000])('passes the captured %i ms policy to the actual worklet node constructor', async maxDurationMillis => {
    const f = environment(), session = await f.port.open(f.sink, f.abort.signal, maxDurationMillis)
    expect(f.nodes).toHaveLength(1)
    expect(f.nodes[0]!.options).toMatchObject({ processorOptions: { maxDurationMillis } })
    expect(f.getUserMedia).toHaveBeenCalledOnce()
    await session.start(); await session.stop()
    f.tracks.forEach(track => expect(track.stop).toHaveBeenCalled())
    expect(f.contexts[0]!.close).toHaveBeenCalledOnce()
  })
  it.each([0, 499, 60001, 500.5, NaN, Infinity, -Infinity, Number.MAX_SAFE_INTEGER + 1, '500', null, true, {}])(
    'rejects invalid duration %s before constructing a context or requesting microphone permission', async invalid => {
      const f = environment()
      await expect(f.port.open(f.sink, f.abort.signal, invalid as number)).rejects.toMatchObject({ code: 'DURATION_OUT_OF_RANGE' })
      expect(f.contexts).toHaveLength(0); expect(f.nodes).toHaveLength(0)
      expect(f.getUserMedia).not.toHaveBeenCalled(); expect(f.sink.pcm).not.toHaveBeenCalled()
    },
  )
  it('keeps the original short policy across delayed permission and releases every late aborted track', async () => {
    const f = environment(), permission = deferred<MediaStream>(); f.getUserMedia.mockReturnValueOnce(permission.promise)
    let policy = 1250
    const open = f.port.open(f.sink, f.abort.signal, policy)
    policy = 60000; permission.resolve(f.stream)
    const session = await open
    expect(policy).toBe(60000)
    expect(f.nodes[0]!.options).toMatchObject({ processorOptions: { maxDurationMillis: 1250 } })
    session.abort()
    const late = environment(), latePermission = deferred<MediaStream>(); late.getUserMedia.mockReturnValueOnce(latePermission.promise)
    const cancelled = late.port.open(late.sink, late.abort.signal, 500); late.abort.abort(); latePermission.resolve(late.stream)
    await expect(cancelled).rejects.toMatchObject({ code: 'MIC_INTERRUPTED' })
    expect(late.nodes).toHaveLength(0); late.tracks.forEach(track => expect(track.stop).toHaveBeenCalled())
  })
})
