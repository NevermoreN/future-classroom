import workletUrl from './pcm-worklet.ts?worker&url'
import { checkedMaxDurationMillis, checkedSampleRate } from './pcm'
import { RecorderError, type CapturePort, type CaptureSession, type CaptureSink } from './ports'

export function recordingSupport(): { supported: boolean; reason: string } {
  if (typeof location === 'undefined' || location.protocol !== 'https:' || !globalThis.isSecureContext) {
    return { supported: false, reason: new RecorderError('INSECURE_CONTEXT').message }
  }
  if (!globalThis.navigator?.mediaDevices?.getUserMedia || typeof AudioContext !== 'function' ||
      typeof AudioWorkletNode !== 'function' || !('audioWorklet' in AudioContext.prototype) ||
      typeof OfflineAudioContext !== 'function' || typeof Worker !== 'function') {
    return { supported: false, reason: new RecorderError('RECORDING_UNSUPPORTED').message }
  }
  return { supported: true, reason: '' }
}

export class BrowserCapturePort implements CapturePort {
  async open(sink: CaptureSink, signal: AbortSignal, maxDurationMillis = 60000): Promise<CaptureSession> {
    // Capture one validated policy before allocating a context or requesting a
    // microphone. Permission latency cannot replace this recording's limit.
    try { checkedMaxDurationMillis(maxDurationMillis) }
    catch { throw new RecorderError('DURATION_OUT_OF_RANGE') }
    if (typeof location === 'undefined' || location.protocol !== 'https:' || !globalThis.isSecureContext) {
      throw new RecorderError('INSECURE_CONTEXT')
    }
    if (!recordingSupport().supported) throw new RecorderError('RECORDING_UNSUPPORTED')
    if (signal.aborted) throw new RecorderError('MIC_INTERRUPTED')
    // Construct/resume synchronously within the original user gesture, before permission resolves.
    const context = new AudioContext()
    let stream: MediaStream | undefined
    let source: MediaStreamAudioSourceNode | undefined
    let node: AudioWorkletNode | undefined
    let mute: GainNode | undefined
    let closed = false
    let stopping = false
    let complete: (() => void) | undefined
    const stopTracks = () => { for (const track of stream?.getTracks() ?? []) track.stop() }
    const interrupted = () => { if (!stopping && !closed) sink.interrupted() }
    const stateChange = () => { if (!stopping && !closed && context.state !== 'running') interrupted() }
    const cleanup = () => {
      if (closed) return
      closed = true; stopping = true
      signal.removeEventListener('abort', cleanup)
      context.removeEventListener('statechange', stateChange)
      for (const track of stream?.getTracks() ?? []) track.removeEventListener('ended', interrupted)
      stopTracks()
      source?.disconnect(); node?.disconnect(); mute?.disconnect()
      if (node) { node.port.onmessage = null; node.port.close() }
      void context.close().catch(() => {})
      complete?.()
    }
    signal.addEventListener('abort', cleanup, { once: true })
    let resumed: Promise<void>
    try { resumed = context.resume() } catch (error) { cleanup(); throw signal.aborted ? new RecorderError('MIC_INTERRUPTED') : error }
    // A cancelled permission request may never settle; do not leave resume rejection unhandled.
    void resumed.catch(() => {})
    try {
      try { stream = await navigator.mediaDevices.getUserMedia({ audio: { channelCount: { ideal: 1 } }, video: false }) }
      catch (error) {
        if (error instanceof DOMException && ['NotAllowedError', 'SecurityError'].includes(error.name)) throw new RecorderError('MIC_DENIED')
        throw error
      }
      // getUserMedia cannot be aborted. Stop every late track before graph construction.
      if (signal.aborted || closed) { stopTracks(); throw new RecorderError('MIC_INTERRUPTED') }
      checkedSampleRate(context.sampleRate)
      if (!stream.getAudioTracks().length || stream.getAudioTracks().some(track => track.readyState !== 'live')) {
        throw new RecorderError('MIC_INTERRUPTED')
      }
      for (const track of stream.getTracks()) track.addEventListener('ended', interrupted)
      await new Promise<void>((resolve, reject) => {
        let settled = false
        let loadingModule = false
        const done = (error?: unknown) => {
          if (settled) return
          settled = true
          clearTimeout(timer); signal.removeEventListener('abort', abort)
          if (signal.aborted) reject(new RecorderError('MIC_INTERRUPTED'))
          else if (error) reject(error); else resolve()
        }
        const abort = () => done(new RecorderError('MIC_INTERRUPTED'))
        // Keep the original total setup deadline. Waiting for resume is not a
        // resource-load failure; only the addModule stage gets refresh guidance.
        const timer = globalThis.setTimeout(() => done(new RecorderError(loadingModule ? 'RESOURCE_UNAVAILABLE' : 'RECORDING_FAILED')), 10000)
        signal.addEventListener('abort', abort, { once: true })
        void resumed.then(() => {
          if (settled || signal.aborted || closed) throw new RecorderError('MIC_INTERRUPTED')
          loadingModule = true
          const unavailable = () => new RecorderError(signal.aborted ? 'MIC_INTERRUPTED' : 'RESOURCE_UNAVAILABLE')
          try { return context.audioWorklet.addModule(workletUrl).catch(() => { throw unavailable() }) }
          catch { throw unavailable() }
        }).then(() => done(), done)
      })
      if (signal.aborted || closed) throw new RecorderError('MIC_INTERRUPTED')
      source = context.createMediaStreamSource(stream)
      node = new AudioWorkletNode(context, 'fc-bounded-mono-capture', {
        numberOfInputs: 1, numberOfOutputs: 1, outputChannelCount: [1],
        channelCountMode: 'max', channelInterpretation: 'discrete',
        processorOptions: { maxDurationMillis },
      })
      mute = context.createGain(); mute.gain.value = 0
      node.port.onmessage = event => {
        if (closed) return
        const data = event.data
        if (data?.type === 'pcm' && data.chunk instanceof Float32Array && data.sampleRate === context.sampleRate) {
          sink.pcm(data.chunk, data.sampleRate)
        } else if (data?.type === 'stopped') complete?.()
        else if (data?.type === 'limit') sink.limit()
        else interrupted()
      }
      node.onprocessorerror = interrupted
      const session: CaptureSession = {
        sampleRate: context.sampleRate,
        async start() {
          if (closed || signal.aborted || context.state !== 'running') throw new RecorderError('MIC_INTERRUPTED')
          context.addEventListener('statechange', stateChange)
          node!.port.postMessage({ type: 'start' })
          source!.connect(node!); node!.connect(mute!); mute!.connect(context.destination)
        },
        async stop() {
          if (closed) throw new RecorderError('MIC_INTERRUPTED')
          stopping = true
          // Privacy boundary: microphone is released before waiting for any async flush/render.
          stopTracks()
          try {
            await new Promise<void>((resolve, reject) => {
              const timer = globalThis.setTimeout(() => reject(new RecorderError('RECORDING_FAILED')), 1000)
              complete = () => { clearTimeout(timer); resolve() }
              node!.port.postMessage({ type: 'finish' })
            })
            if (signal.aborted) throw new RecorderError('MIC_INTERRUPTED')
          } finally { cleanup() }
        },
        abort: cleanup,
      }
      return session
    } catch (error) {
      stopTracks(); cleanup()
      throw signal.aborted ? new RecorderError('MIC_INTERRUPTED') : error
    }
  }
}
