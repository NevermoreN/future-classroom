import { BrowserCapturePort, recordingSupport } from './browser-capture'
import { BrowserWavNormalizer } from './normalize'
import { checkedSampleRate, MAX_RAW_BYTES, outputFrames } from './pcm'
import { RecorderError, type CapturePort, type CaptureSession, type NormalizedWav,
  type RecorderClock, type WavNormalizer } from './ports'

export { recordingSupport, RecorderError }
export type { CapturePort, CaptureSession, CaptureSink, WavNormalizer, RecorderClock } from './ports'
export interface RecorderState {
  phase: 'idle' | 'requesting' | 'recording' | 'finalizing'
  recordingId: string | null
  elapsedMillis: number
  cancelGesture: boolean
}
export interface RecordedWav extends NormalizedWav { recordingId: string }
export interface RecorderOptions {
  onState?: (state: RecorderState) => void
  onWav: (recording: RecordedWav) => void
  onError?: (error: RecorderError) => void
  capturePort?: CapturePort
  normalizer?: WavNormalizer
  clock?: RecorderClock
}
interface Recording {
  id: string
  signal: AbortController
  capture: CaptureSession | undefined
  chunks: Float32Array[]
  frames: number
  rate: number
  timer: unknown
  finalized: boolean
}
const idle = (): RecorderState => ({ phase: 'idle', recordingId: null, elapsedMillis: 0, cancelGesture: false })

/** Owns only transient recording data. The caller freezes authorization/target/repeat context by id. */
export class RecorderController {
  private current: Recording | undefined
  private value = idle()
  private disposed = false
  private readonly usedIds = new Set<string>()
  private readonly capturePort: CapturePort
  private readonly normalizer: WavNormalizer
  private readonly clock: RecorderClock
  constructor(private readonly options: RecorderOptions) {
    this.capturePort = options.capturePort ?? new BrowserCapturePort()
    this.normalizer = options.normalizer ?? new BrowserWavNormalizer()
    this.clock = options.clock ?? {
      setTimeout: (callback, milliseconds) => globalThis.setTimeout(callback, milliseconds),
      clearTimeout: timer => globalThis.clearTimeout(timer as ReturnType<typeof setTimeout>),
    }
    globalThis.addEventListener?.('pagehide', this.pageLost)
    globalThis.addEventListener?.('blur', this.pageLost)
    globalThis.document?.addEventListener('visibilitychange', this.visibilityChanged)
  }
  get state(): RecorderState { return { ...this.value } }
  private readonly pageLost = () => { this.cancel('page-left') }
  private readonly visibilityChanged = () => { if (document.visibilityState !== 'visible') this.cancel('hidden') }
  private publish(patch: Partial<RecorderState>) {
    this.value = { ...this.value, ...patch }
    this.options.onState?.(this.state)
  }
  private live(recording: Recording) { return this.current === recording && !recording.signal.signal.aborted && !this.disposed }
  async start(recordingId: string): Promise<void> {
    if (this.disposed || this.current) return
    // Bounded, never evicted: even a very late duplicate gesture cannot reuse a finalized id.
    if (!/^[A-Za-z0-9_-]{1,128}$/.test(recordingId) || this.usedIds.has(recordingId) || this.usedIds.size >= 1024) {
      this.options.onError?.(new RecorderError('RECORDING_UNSUPPORTED')); return
    }
    this.usedIds.add(recordingId)
    const recording: Recording = { id: recordingId, signal: new AbortController(), capture: undefined,
      chunks: [], frames: 0, rate: 0, timer: undefined, finalized: false }
    this.current = recording
    this.publish({ phase: 'requesting', recordingId, elapsedMillis: 0, cancelGesture: false })
    try {
      const capture = await this.capturePort.open({
        pcm: (chunk, rate) => this.receive(recording, chunk, rate),
        interrupted: () => this.fail(recording, new RecorderError('MIC_INTERRUPTED')),
        limit: () => { if (this.live(recording)) void this.finish(recording.id) },
      }, recording.signal.signal)
      if (!this.live(recording)) { capture.abort(); return }
      recording.capture = capture
      recording.rate = checkedSampleRate(capture.sampleRate)
      this.publish({ phase: 'recording' })
      if (!this.live(recording)) return
      recording.timer = this.clock.setTimeout(() => { if (this.live(recording)) void this.finish(recording.id) }, 60000)
      await capture.start()
    } catch (error) { this.fail(recording, error) }
  }
  private receive(recording: Recording, chunk: Float32Array, rate: number) {
    if (!this.live(recording) || !['recording', 'finalizing'].includes(this.value.phase)) return
    try {
      if (!(chunk instanceof Float32Array) || rate !== recording.rate || !chunk.length) throw new RecorderError('INVALID_PCM')
      const next = recording.frames + chunk.length
      if (next * 4 > MAX_RAW_BYTES || next > rate * 60 || recording.chunks.length >= 8192) throw new RecorderError('RAW_LIMIT')
      for (const sample of chunk) if (!Number.isFinite(sample)) throw new RecorderError('INVALID_PCM')
      recording.chunks.push(chunk); recording.frames = next
      this.publish({ elapsedMillis: Math.floor(next * 1000 / rate) })
    } catch (error) { this.fail(recording, error) }
  }
  setCancelGesture(cancelGesture: boolean): void {
    if (this.current && !this.current.finalized) this.publish({ cancelGesture })
  }
  async finish(recordingId: string): Promise<void> {
    const recording = this.current
    if (!recording || recording.id !== recordingId || recording.finalized) return
    if (this.value.phase === 'requesting' || this.value.cancelGesture) { this.cancel('gesture-ended'); return }
    recording.finalized = true
    this.clock.clearTimeout(recording.timer)
    this.publish({ phase: 'finalizing' })
    if (!this.live(recording)) return
    try {
      await recording.capture!.stop()
      if (!this.live(recording)) return
      recording.capture = undefined
      const expected = outputFrames(recording.frames, recording.rate)
      const normalized = await this.normalizer.normalize(recording.chunks, recording.rate, recording.signal.signal)
      if (!this.live(recording)) return
      if (normalized.frames !== expected || normalized.sampleRate !== 16000 || normalized.wav.size !== 44 + expected * 2 ||
          normalized.wav.type !== 'audio/wav') throw new RecorderError('INVALID_PCM')
      recording.chunks.length = 0
      this.current = undefined
      this.publish(idle())
      if (!recording.signal.signal.aborted && !this.disposed) this.options.onWav({ ...normalized, recordingId: recording.id })
    } catch (error) { this.fail(recording, error) }
  }
  /** Reason is caller context only; never copied to state, logs, or errors. */
  cancel(_reason = 'cancelled'): void {
    const recording = this.current
    if (!recording) return
    this.current = undefined
    this.clock.clearTimeout(recording.timer)
    recording.signal.abort()
    recording.capture?.abort()
    recording.chunks.length = 0
    this.publish(idle())
  }
  private fail(recording: Recording, error: unknown) {
    if (!this.live(recording)) return
    this.cancel('failure')
    const known = error instanceof RecorderError ? error : error instanceof Error &&
      ['RECORDING_UNSUPPORTED', 'DURATION_OUT_OF_RANGE', 'INVALID_PCM'].includes(error.message)
      ? new RecorderError(error.message as 'RECORDING_UNSUPPORTED' | 'DURATION_OUT_OF_RANGE' | 'INVALID_PCM')
      : new RecorderError('RECORDING_FAILED')
    this.options.onError?.(known)
  }
  dispose(): void {
    if (this.disposed) return
    this.cancel('disposed'); this.disposed = true
    globalThis.removeEventListener?.('pagehide', this.pageLost)
    globalThis.removeEventListener?.('blur', this.pageLost)
    globalThis.document?.removeEventListener('visibilitychange', this.visibilityChanged)
  }
}
