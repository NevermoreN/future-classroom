import { afterEach, expect, it, vi } from 'vitest'

afterEach(() => vi.unstubAllGlobals())
it('the real worklet batches variable quanta, explicit mono and an exact 60s hard cap without speaker output', async () => {
  class Base {
    port = { onmessage: null as ((event: { data: unknown }) => void) | null, postMessage: vi.fn() }
  }
  type Processor = Base & { process(inputs: Float32Array[][]): boolean }
  let ProcessorClass!: new () => Processor
  vi.stubGlobal('sampleRate', 8000); vi.stubGlobal('AudioWorkletProcessor', Base)
  vi.stubGlobal('registerProcessor', (name: string, processor: new () => Processor) => {
    expect(name).toBe('fc-bounded-mono-capture'); ProcessorClass = processor
  })
  await import('../src/voice/pcm-worklet')
  const processor = new ProcessorClass()
  processor.process([[new Float32Array(17).fill(1)]]); expect(processor.port.postMessage).not.toHaveBeenCalled()
  processor.port.onmessage!({ data: { type: 'start' } })
  processor.process([[new Float32Array(17).fill(1), new Float32Array(17).fill(-1)]])
  processor.port.onmessage!({ data: { type: 'finish' } })
  const mono = processor.port.postMessage.mock.calls[0]![0] as { type: string; sampleRate: number; chunk: Float32Array }
  expect(mono.type).toBe('pcm'); expect(mono.sampleRate).toBe(8000); expect(Array.from(mono.chunk)).toEqual(new Array(17).fill(0))
  expect(processor.port.postMessage.mock.calls[1]![0]).toEqual({ type: 'stopped' })
  const maximum = new ProcessorClass(); maximum.port.onmessage!({ data: { type: 'start' } })
  for (let i = 0; i < 120; i++) maximum.process([[new Float32Array(4096).fill(0.5)]])
  const messages = maximum.port.postMessage.mock.calls.map(call => call[0] as { type: string; chunk?: Float32Array })
  expect(messages.filter(m => m.type === 'limit')).toHaveLength(1)
  expect(messages.filter(m => m.type === 'pcm').reduce((sum, m) => sum + m.chunk!.length, 0)).toBe(480000)
  expect(messages.filter(m => m.type === 'pcm').every(m => m.chunk!.length <= 4096)).toBe(true)
  const invalid = new ProcessorClass(); invalid.port.onmessage!({ data: { type: 'start' } }); invalid.process([[new Float32Array([NaN])]])
  expect(invalid.port.postMessage).toHaveBeenCalledWith({ type: 'error' })
})
