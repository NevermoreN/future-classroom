export const MAX_RAW_BYTES = 32 * 1024 * 1024
export const OUTPUT_RATE = 16000 as const
export const MIN_FRAMES = 8000
export const MAX_FRAMES = 960000

export function checkedSampleRate(rate: number): number {
  // Higher device rates exceed the bounded capture/normalization working set.
  if (!Number.isInteger(rate) || rate < 8000 || rate > 96000) throw new Error('RECORDING_UNSUPPORTED')
  return rate
}

export function outputFrames(frames: number, rate: number): number {
  checkedSampleRate(rate)
  if (!Number.isSafeInteger(frames) || frames < rate / 2 || frames > rate * 60 || frames * 4 > MAX_RAW_BYTES) {
    throw new Error('DURATION_OUT_OF_RANGE')
  }
  const result = Math.round(frames * OUTPUT_RATE / rate)
  if (result < MIN_FRAMES || result > MAX_FRAMES) throw new Error('DURATION_OUT_OF_RANGE')
  return result
}

/** Arithmetic mean, independent of requested device channelCount or speaker layouts. */
export function mixMono(channels: readonly Float32Array[]): Float32Array {
  if (channels.length < 1 || channels.length > 32) throw new Error('INVALID_PCM')
  const length = channels[0]!.length
  if (length > 4096 || channels.some(c => c.length !== length)) throw new Error('INVALID_PCM')
  const mono = new Float32Array(length)
  for (let i = 0; i < length; i++) {
    let sum = 0
    for (const channel of channels) {
      const sample = channel[i]!
      if (!Number.isFinite(sample)) throw new Error('INVALID_PCM')
      sum += sample / channels.length
    }
    mono[i] = Math.max(-1, Math.min(1, sum))
  }
  return mono
}

/** The only accepted output container: PCM16LE, mono 16kHz, exactly one 44-byte header. */
export function encodeWav(pcm: Float32Array): ArrayBuffer {
  if (pcm.length < MIN_FRAMES || pcm.length > MAX_FRAMES) throw new Error('DURATION_OUT_OF_RANGE')
  const result = new ArrayBuffer(44 + pcm.length * 2)
  const view = new DataView(result)
  const text = (offset: number, value: string) => {
    for (let i = 0; i < value.length; i++) view.setUint8(offset + i, value.charCodeAt(i))
  }
  text(0, 'RIFF'); view.setUint32(4, result.byteLength - 8, true); text(8, 'WAVE')
  text(12, 'fmt '); view.setUint32(16, 16, true); view.setUint16(20, 1, true)
  view.setUint16(22, 1, true); view.setUint32(24, OUTPUT_RATE, true)
  view.setUint32(28, 32000, true); view.setUint16(32, 2, true); view.setUint16(34, 16, true)
  text(36, 'data'); view.setUint32(40, pcm.length * 2, true)
  for (let i = 0; i < pcm.length; i++) {
    const value = pcm[i]!
    if (!Number.isFinite(value)) throw new Error('INVALID_PCM')
    const bounded = Math.max(-1, Math.min(1, value))
    view.setInt16(44 + i * 2, Math.round(bounded * (bounded < 0 ? 32768 : 32767)), true)
  }
  return result
}
