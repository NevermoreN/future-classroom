import { describe, expect, it } from 'vitest'
import { encodeWav, mixMono, outputFrames } from '../src/voice/pcm'

describe('canonical recording PCM, mono mix and exact WAV framing', () => {
  it('mixes every actual channel rather than trusting the requested mono constraint', () => {
    expect(Array.from(mixMono([new Float32Array([1, -1, 0.5]), new Float32Array([-1, 0, 0.5])]))).toEqual([0, -0.5, 0.5])
    expect(Array.from(mixMono([new Float32Array([1]), new Float32Array([0]), new Float32Array([-1])]))).toEqual([0])
  })
  it('uses real block length and rejects inconsistent, excessive or nonfinite channel data', () => {
    expect(mixMono([new Float32Array(17)]).length).toBe(17)
    expect(() => mixMono([])).toThrow('INVALID_PCM'); expect(() => mixMono([new Float32Array(2), new Float32Array(3)])).toThrow('INVALID_PCM')
    expect(() => mixMono([new Float32Array([Infinity])])).toThrow('INVALID_PCM'); expect(() => mixMono(Array.from({ length: 33 }, () => new Float32Array(1)))).toThrow('INVALID_PCM')
  })
  it.each([8000, 16000, 22050, 44100, 48000, 96000])('maps actual %i Hz duration to bounded 16kHz frames', rate => {
    expect(outputFrames(rate / 2, rate)).toBe(8000); expect(outputFrames(rate * 60, rate)).toBe(960000)
    expect(() => outputFrames(rate / 2 - 1, rate)).toThrow('DURATION_OUT_OF_RANGE'); expect(() => outputFrames(rate * 60 + 1, rate)).toThrow('DURATION_OUT_OF_RANGE')
  })
  it.each([7999, 960001])('rejects invalid output frame count %i', frames => { expect(() => encodeWav(new Float32Array(frames))).toThrow('DURATION_OUT_OF_RANGE') })
  it.each([8000, 960000])('writes exactly one 44-byte PCM16LE mono header at %i frames', frames => {
    const samples = new Float32Array(frames); samples.set([-2, -1, -0.5, 0, 0.5, 1, 2])
    const wav = encodeWav(samples), bytes = new Uint8Array(wav), view = new DataView(wav), ascii = (start: number, end: number) => String.fromCharCode(...bytes.slice(start, end))
    expect(wav.byteLength).toBe(44 + frames * 2); expect(ascii(0, 4)).toBe('RIFF'); expect(view.getUint32(4, true)).toBe(wav.byteLength - 8); expect(ascii(8, 12)).toBe('WAVE'); expect(ascii(12, 16)).toBe('fmt ')
    expect(view.getUint32(16, true)).toBe(16); expect(view.getUint16(20, true)).toBe(1); expect(view.getUint16(22, true)).toBe(1); expect(view.getUint32(24, true)).toBe(16000)
    expect(view.getUint32(28, true)).toBe(32000); expect(view.getUint16(32, true)).toBe(2); expect(view.getUint16(34, true)).toBe(16); expect(ascii(36, 40)).toBe('data'); expect(view.getUint32(40, true)).toBe(frames * 2)
    expect(Array.from({ length: 7 }, (_, i) => view.getInt16(44 + i * 2, true))).toEqual([-32768, -32768, -16384, 0, 16384, 32767, 32767])
  })
  it('never encodes NaN or infinity as apparently valid silence', () => {
    for (const value of [NaN, Infinity, -Infinity]) { const samples = new Float32Array(8000); samples[7999] = value; expect(() => encodeWav(samples)).toThrow('INVALID_PCM') }
  })
})
