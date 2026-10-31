import { afterEach, describe, expect, it, vi } from 'vitest'
import { RecorderController, RecorderError, type CapturePort, type CaptureSession, type CaptureSink,
  type RecorderClock, type WavNormalizer } from '../src/voice/recorder'
import { encodeWav, MAX_RAW_BYTES, outputFrames } from '../src/voice/pcm'

function deferred<T>() {
  let resolve!: (value: T) => void, reject!: (reason: unknown) => void
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}
const owned: RecorderController[] = []
afterEach(() => { owned.splice(0).forEach(controller => controller.dispose()); vi.unstubAllGlobals() })
function fixture(rate = 48000) {
  let sink!: CaptureSink, signal!: AbortSignal
  const session = { sampleRate: rate, start: vi.fn(async () => {}), stop: vi.fn(async () => {}), abort: vi.fn() } satisfies CaptureSession
  const capturePort = { open: vi.fn<CapturePort['open']>().mockImplementation(async (next, abort) => { sink = next; signal = abort; return session }) }
  const normalize = vi.fn<WavNormalizer['normalize']>().mockImplementation(async (chunks, sampleRate) => {
    const frames = outputFrames(chunks.reduce((n, chunk) => n + chunk.length, 0), sampleRate)
    return { wav: new Blob([encodeWav(new Float32Array(frames))], { type: 'audio/wav' }), frames, sampleRate: 16000 }
  })
  const timers: Array<() => void> = []
  const clock = { setTimeout: vi.fn<RecorderClock['setTimeout']>().mockImplementation(callback => { timers.push(callback); return callback }), clearTimeout: vi.fn() }
  const onWav = vi.fn(), onError = vi.fn(), onState = vi.fn()
  const controller = new RecorderController({ capturePort, normalizer: { normalize }, clock, onWav, onError, onState })
  owned.push(controller)
  return { controller, session, capturePort, normalize, clock, timers, onWav, onError, onState,
    pcm: (frames = rate / 2) => sink.pcm(new Float32Array(frames), rate), sink: () => sink, signal: () => signal }
}

describe('press recording privacy, exact finalization and cancellation', () => {
  it('waits one bounded grace when the 500ms wall timer beats the final quantum and sends the exact cap once', async () => {
    const f = fixture(16000)
    await f.controller.start('wall-before-quantum', 500)
    f.pcm(4096); f.timers[0]!()
    expect(f.session.stop).not.toHaveBeenCalled()
    expect(f.onError).not.toHaveBeenCalled(); expect(f.onWav).not.toHaveBeenCalled()
    expect(f.clock.setTimeout.mock.calls.map(call => call[1])).toEqual([500, 250])
    f.pcm(3904); f.sink().limit()
    await Promise.resolve(); await Promise.resolve(); await Promise.resolve()
    expect(f.session.stop).toHaveBeenCalledOnce()
    expect(f.onError).not.toHaveBeenCalled(); expect(f.onWav).toHaveBeenCalledOnce()
    expect(f.onWav.mock.calls[0]![0]).toMatchObject({ frames: 8000, recordingId: 'wall-before-quantum' })
    f.timers[1]!(); f.timers[0]!(); f.sink().limit()
    await Promise.resolve()
    expect(f.onWav).toHaveBeenCalledOnce(); expect(f.onError).not.toHaveBeenCalled()
    expect(f.clock.setTimeout).toHaveBeenCalledTimes(2)
  })
  it('cancels a permanently missing worklet cap after one 250ms grace without sending partial PCM', async () => {
    const f = fixture(16000)
    await f.controller.start('wall-stalled-worklet', 500)
    f.pcm(4096); f.timers[0]!()
    expect(f.session.stop).not.toHaveBeenCalled(); expect(f.session.abort).not.toHaveBeenCalled()
    expect(f.clock.setTimeout.mock.calls.map(call => call[1])).toEqual([500, 250])
    f.timers[1]!()
    expect(f.session.abort).toHaveBeenCalledOnce(); expect(f.signal().aborted).toBe(true)
    expect(f.onError).toHaveBeenCalledWith(expect.objectContaining({ code: 'RECORDING_FAILED' }))
    expect(f.controller.state.phase).toBe('idle')
    expect(f.normalize).not.toHaveBeenCalled(); expect(f.onWav).not.toHaveBeenCalled()
    f.sink().limit(); f.pcm(3904); f.timers[0]!(); f.timers[1]!()
    expect(f.onWav).not.toHaveBeenCalled(); expect(f.onError).toHaveBeenCalledOnce()
    expect(f.clock.setTimeout).toHaveBeenCalledTimes(2)
  })
  it('does not arm wall grace after cancellation, explicit finalization or a cancel gesture', async () => {
    for (const action of ['cancel', 'finish', 'gesture']) {
      const f = fixture(16000)
      await f.controller.start(`wall-after-${action}`, 500)
      f.pcm(4096)
      if (action === 'cancel') f.controller.cancel()
      else if (action === 'finish') await f.controller.finish(`wall-after-${action}`)
      else f.controller.setCancelGesture(true)
      f.timers[0]!()
      await Promise.resolve()
      expect(f.clock.setTimeout).toHaveBeenCalledOnce()
      expect(f.controller.state.phase).toBe('idle'); expect(f.onWav).not.toHaveBeenCalled()
      if (action === 'gesture') {
        expect(f.session.abort).toHaveBeenCalledOnce(); expect(f.onError).not.toHaveBeenCalled()
      }
    }
  })
  it('captures configured duration before permission, applies its timer and sends exactly its allowed frames', async () => {
    const f = fixture(16000)
    await f.controller.start('configured', 1250)
    expect(f.capturePort.open.mock.calls[0]![2]).toBe(1250)
    expect(f.clock.setTimeout.mock.calls[0]![1]).toBe(1250)
    f.pcm(20000); f.timers[0]!()
    await Promise.resolve(); await Promise.resolve()
    expect(f.onWav).toHaveBeenCalledOnce()
    expect(f.onWav.mock.calls[0]![0]).toMatchObject({ frames: 20000, recordingId: 'configured' })
    await f.controller.finish('configured'); expect(f.onWav).toHaveBeenCalledOnce()
    await f.controller.start('next-default'); expect(f.capturePort.open.mock.calls[1]![2]).toBe(60000)
  })
  it('rejects invalid configured durations before permission and rejects a single frame above the captured limit', async () => {
    for (const limit of [0, 499, 60001, 1250.5, NaN, Infinity]) {
      const f = fixture(); await f.controller.start('invalid-limit', limit)
      expect(f.capturePort.open).not.toHaveBeenCalled(); expect(f.onError.mock.calls[0]![0].code).toBe('DURATION_OUT_OF_RANGE')
    }
    const f = fixture(16000); await f.controller.start('overflow', 1250); f.pcm(20001)
    expect(f.session.abort).toHaveBeenCalled(); expect(f.normalize).not.toHaveBeenCalled(); expect(f.onWav).not.toHaveBeenCalled()
  })
  it('constructs without permission, reports copied state and sends only after explicit release', async () => {
    const f = fixture(); expect(f.capturePort.open).not.toHaveBeenCalled()
    await f.controller.start('first'); expect(f.controller.state.phase).toBe('recording')
    const copy = f.controller.state; copy.phase = 'idle'; expect(f.controller.state.phase).toBe('recording')
    f.pcm(); expect(f.controller.state.elapsedMillis).toBe(500); expect(f.onWav).not.toHaveBeenCalled()
    await f.controller.finish('first'); expect(f.session.stop).toHaveBeenCalledOnce(); expect(f.onWav).toHaveBeenCalledOnce()
    expect(f.onWav.mock.calls[0]![0]).toMatchObject({ recordingId: 'first', frames: 8000, sampleRate: 16000 })
    expect(f.controller.state.phase).toBe('idle')
  })
  it('release during permission rejects late capture without ever starting or encoding it', async () => {
    const f = fixture(), late = deferred<CaptureSession>(); f.capturePort.open.mockReturnValueOnce(late.promise)
    const opening = f.controller.start('late'); await f.controller.finish('late')
    expect(f.controller.state.phase).toBe('idle'); late.resolve(f.session); await opening
    expect(f.session.abort).toHaveBeenCalledOnce(); expect(f.session.start).not.toHaveBeenCalled(); expect(f.normalize).not.toHaveBeenCalled()
  })
  it('old permission completion cannot stop or feed the next recording', async () => {
    const f = fixture(), late = deferred<CaptureSession>(); f.capturePort.open.mockReturnValueOnce(late.promise)
    const opening = f.controller.start('old'); f.controller.cancel('pointercancel'); await f.controller.start('new')
    const old = { ...f.session, abort: vi.fn(), start: vi.fn() }; late.resolve(old); await opening
    expect(old.abort).toHaveBeenCalledOnce(); expect(old.start).not.toHaveBeenCalled(); expect(f.controller.state.recordingId).toBe('new')
    f.pcm(); await f.controller.finish('new'); expect(f.onWav.mock.calls[0]![0].recordingId).toBe('new')
  })
  it('slide cancellation never sends even if a valid half second has been collected', async () => {
    const f = fixture(); await f.controller.start('slide'); f.pcm(); f.controller.setCancelGesture(true)
    await f.controller.finish('slide'); expect(f.session.abort).toHaveBeenCalled(); expect(f.onWav).not.toHaveBeenCalled(); expect(f.normalize).not.toHaveBeenCalled()
  })
  it('returning from slide cancellation permits a normal release', async () => {
    const f = fixture(); await f.controller.start('back'); f.pcm(); f.controller.setCancelGesture(true); f.controller.setCancelGesture(false)
    await f.controller.finish('back'); expect(f.onWav).toHaveBeenCalledOnce()
  })
  it.each(['pointercancel', 'target-changed', 'class-changed', 'leaving'])('explicit %s immediately aborts and discards PCM', async reason => {
    const f = fixture(); await f.controller.start('cancel'); f.pcm(); f.controller.cancel(reason); f.pcm(); await f.controller.finish('cancel')
    expect(f.signal().aborted).toBe(true); expect(f.session.abort).toHaveBeenCalled(); expect(f.onWav).not.toHaveBeenCalled(); expect(f.controller.state.elapsedMillis).toBe(0)
  })
  it('automatically cancels on page hidden, blur and pagehide without sending', async () => {
    const windowEvents = new EventTarget(), documentEvents = new EventTarget()
    vi.stubGlobal('addEventListener', windowEvents.addEventListener.bind(windowEvents)); vi.stubGlobal('removeEventListener', windowEvents.removeEventListener.bind(windowEvents))
    vi.stubGlobal('document', Object.assign(documentEvents, { visibilityState: 'hidden' }))
    for (const event of ['visibilitychange', 'blur', 'pagehide']) {
      const f = fixture(); await f.controller.start(event); f.pcm()
      ;(event === 'visibilitychange' ? documentEvents : windowEvents).dispatchEvent(new Event(event))
      expect(f.session.abort).toHaveBeenCalled(); expect(f.onWav).not.toHaveBeenCalled(); expect(f.controller.state.phase).toBe('idle'); f.controller.dispose()
    }
  })
  it('unexpected track/context interruption aborts and has a bounded user error', async () => {
    const f = fixture(); await f.controller.start('ended'); f.pcm(); f.sink().interrupted()
    expect(f.session.abort).toHaveBeenCalled(); expect(f.onError.mock.calls[0]![0].code).toBe('MIC_INTERRUPTED'); expect(f.onWav).not.toHaveBeenCalled()
  })
  it('deduplicates simultaneous finish, automatic limit and later pointerup', async () => {
    const f = fixture(), stop = deferred<void>(); f.session.stop.mockReturnValueOnce(stop.promise)
    await f.controller.start('once'); f.pcm(); const first = f.controller.finish('once'); await f.controller.finish('once'); f.timers[0]!(); f.sink().limit()
    expect(f.session.stop).toHaveBeenCalledOnce(); stop.resolve(); await first; await f.controller.finish('once'); await f.controller.start('once')
    expect(f.onWav).toHaveBeenCalledOnce(); expect(f.capturePort.open).toHaveBeenCalledOnce()
  })
  it('60 second deadline sends exactly once and honors cancellation gesture', async () => {
    const f = fixture(16000); await f.controller.start('max'); f.pcm(960000); f.timers[0]!(); await Promise.resolve(); await Promise.resolve()
    expect(f.clock.setTimeout.mock.calls[0]![1]).toBe(60000); expect(f.onWav).toHaveBeenCalledOnce(); expect(f.onWav.mock.calls[0]![0].frames).toBe(960000)
    const cancel = fixture(); await cancel.controller.start('max-cancel'); cancel.pcm(); cancel.controller.setCancelGesture(true); cancel.timers[0]!()
    expect(cancel.onWav).not.toHaveBeenCalled(); expect(cancel.session.abort).toHaveBeenCalled()
  })
  it('includes final worklet flush frames after mic stop, before duration validation', async () => {
    const f = fixture(); await f.controller.start('flush'); f.pcm(23900); f.session.stop.mockImplementationOnce(async () => { f.pcm(100) })
    await f.controller.finish('flush'); expect(f.onWav.mock.calls[0]![0].frames).toBe(8000)
    expect(f.session.stop.mock.invocationCallOrder[0]!).toBeLessThan(f.normalize.mock.invocationCallOrder[0]!)
  })
  it('cancels while worklet flush is pending and discards its late completion', async () => {
    const f = fixture(), stop = deferred<void>(); f.session.stop.mockReturnValueOnce(stop.promise)
    await f.controller.start('flush-cancel'); f.pcm(); const finish = f.controller.finish('flush-cancel'); f.controller.cancel(); stop.resolve(); await finish
    expect(f.normalize).not.toHaveBeenCalled(); expect(f.onWav).not.toHaveBeenCalled()
  })
  it('cancels while offline normalization is pending; late output cannot send or mutate next state', async () => {
    const f = fixture(), rendering = deferred<Awaited<ReturnType<WavNormalizer['normalize']>>>(); f.normalize.mockReturnValueOnce(rendering.promise)
    await f.controller.start('render'); f.pcm(); const finish = f.controller.finish('render'); await Promise.resolve(); f.controller.cancel(); await f.controller.start('next')
    rendering.resolve({ wav: new Blob([new ArrayBuffer(16044)], { type: 'audio/wav' }), frames: 8000, sampleRate: 16000 }); await finish
    expect(f.onWav).not.toHaveBeenCalled(); expect(f.controller.state.recordingId).toBe('next')
  })
  it('rejects a half-second minus one frame rather than rounding it into a valid recording', async () => {
    const f = fixture(); await f.controller.start('short'); f.pcm(23999); await f.controller.finish('short')
    expect(f.onError.mock.calls[0]![0].code).toBe('DURATION_OUT_OF_RANGE'); expect(f.normalize).not.toHaveBeenCalled(); expect(f.onWav).not.toHaveBeenCalled()
  })
  it.each([7999, 96001, Infinity, NaN])('unsupported actual sample rate %s aborts the prepared capture', async rate => {
    const f = fixture(rate); await f.controller.start('unsupported')
    expect(f.session.abort).toHaveBeenCalled(); expect(f.session.start).not.toHaveBeenCalled(); expect(f.onError.mock.calls[0]![0].code).toBe('RECORDING_UNSUPPORTED')
  })
  it('rejects raw overflow before caching or encoding and rejects over-60-second PCM', async () => {
    const f = fixture(); await f.controller.start('large'); f.pcm(MAX_RAW_BYTES / 4 + 1)
    expect(f.onError.mock.calls[0]![0].code).toBe('RAW_LIMIT'); expect(f.normalize).not.toHaveBeenCalled(); expect(f.session.abort).toHaveBeenCalled()
    const g = fixture(16000); await g.controller.start('long'); g.pcm(960001); expect(g.onError.mock.calls[0]![0].code).toBe('RAW_LIMIT')
  })
  it('rejects nonfinite PCM and inconsistent rate without forwarding raw platform messages', async () => {
    const f = fixture(); await f.controller.start('nan'); f.sink().pcm(new Float32Array([NaN]), 48000)
    expect(f.onError.mock.calls[0]![0].code).toBe('INVALID_PCM'); expect(f.onWav).not.toHaveBeenCalled()
    const g = fixture(); await g.controller.start('rate'); g.sink().pcm(new Float32Array([1]), 44100); expect(g.onError.mock.calls[0]![0].code).toBe('INVALID_PCM')
  })
  it('every setup/flush/normalization failure cancels and never sends', async () => {
    for (const stage of ['open', 'start', 'stop', 'normalize']) {
      const f = fixture(), error = new Error('sensitive platform diagnostics')
      if (stage === 'open') f.capturePort.open.mockRejectedValueOnce(error)
      if (stage === 'start') f.session.start.mockRejectedValueOnce(error)
      if (stage === 'stop') f.session.stop.mockRejectedValueOnce(error)
      if (stage === 'normalize') f.normalize.mockRejectedValueOnce(error)
      await f.controller.start(stage)
      if (stage === 'stop' || stage === 'normalize') { f.pcm(); await f.controller.finish(stage) }
      expect(f.onWav).not.toHaveBeenCalled(); expect(f.onError.mock.calls[0]![0].message).not.toContain('sensitive'); expect(f.controller.state.phase).toBe('idle')
      if (stage !== 'open' && stage !== 'normalize') expect(f.session.abort).toHaveBeenCalled()
    }
  })
  it('rejects corrupted normalizer output and ignores finish for a foreign id', async () => {
    const f = fixture(); await f.controller.start('id'); f.pcm(); await f.controller.finish('other'); expect(f.session.stop).not.toHaveBeenCalled()
    f.normalize.mockResolvedValueOnce({ wav: new Blob(['bad']), frames: 8000, sampleRate: 16000 }); await f.controller.finish('id')
    expect(f.onError.mock.calls[0]![0].code).toBe('INVALID_PCM'); expect(f.onWav).not.toHaveBeenCalled()
  })
  it('dispose is idempotent, cancels pending permission, and cannot be restarted', async () => {
    const f = fixture(), late = deferred<CaptureSession>(); f.capturePort.open.mockReturnValueOnce(late.promise)
    const start = f.controller.start('dispose'); f.controller.dispose(); f.controller.dispose(); late.resolve(f.session); await start; await f.controller.start('again')
    expect(f.capturePort.open).toHaveBeenCalledOnce(); expect(f.session.abort).toHaveBeenCalledOnce(); expect(f.onWav).not.toHaveBeenCalled()
  })
  it('state observer cancellation before capture.start prevents microphone graph activation', async () => {
    const f = fixture(); f.onState.mockImplementation(state => { if (state.phase === 'recording') f.controller.cancel('target-changed') })
    await f.controller.start('observer'); expect(f.session.start).not.toHaveBeenCalled(); expect(f.session.abort).toHaveBeenCalled()
  })
  it('preserves known permission failure codes and does not attempt normalization', async () => {
    const f = fixture(); f.capturePort.open.mockRejectedValueOnce(new RecorderError('MIC_DENIED')); await f.controller.start('denied')
    expect(f.onError.mock.calls[0]![0].code).toBe('MIC_DENIED'); expect(f.normalize).not.toHaveBeenCalled()
  })
})
