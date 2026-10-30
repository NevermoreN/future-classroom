import { checkedSampleRate, mixMono } from './pcm'

declare const sampleRate: number
declare class AudioWorkletProcessor {
  readonly port: MessagePort
}
declare function registerProcessor(name: string, processor: typeof AudioWorkletProcessor): void

class CaptureProcessor extends AudioWorkletProcessor {
  private active = false
  private total = 0
  private used = 0
  private buffer = new Float32Array(4096)
  private readonly limit = checkedSampleRate(sampleRate) * 60
  constructor() {
    super()
    this.port.onmessage = event => {
      if (event.data?.type === 'start') this.active = true
      if (event.data?.type === 'finish') {
        this.active = false
        this.flush()
        this.port.postMessage({ type: 'stopped' })
      }
    }
  }
  private flush() {
    if (!this.used) return
    const chunk = this.used === this.buffer.length ? this.buffer : this.buffer.slice(0, this.used)
    this.port.postMessage({ type: 'pcm', sampleRate, chunk }, [chunk.buffer])
    this.buffer = new Float32Array(4096)
    this.used = 0
  }
  process(inputs: Float32Array[][]): boolean {
    if (!this.active) return true
    const channels = inputs[0]
    if (!channels?.length) return true
    try {
      // Do not assume a render quantum is always 128 frames.
      const mono = mixMono(channels)
      for (let i = 0; i < mono.length && this.total < this.limit; i++) {
        this.buffer[this.used++] = mono[i]!
        this.total++
        if (this.used === this.buffer.length) this.flush()
      }
      if (this.total === this.limit) {
        this.active = false
        this.flush()
        this.port.postMessage({ type: 'limit' })
      }
    } catch {
      this.active = false
      this.port.postMessage({ type: 'error' })
    }
    return true
  }
}
registerProcessor('fc-bounded-mono-capture', CaptureProcessor)
