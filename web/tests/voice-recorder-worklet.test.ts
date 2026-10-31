import { afterEach, expect, it, vi } from 'vitest'

afterEach(() => vi.unstubAllGlobals())

async function realProcessor(rate: number) {
  vi.resetModules()
  class Base {
    port = { onmessage: null as ((event: { data: unknown }) => void) | null, postMessage: vi.fn() }
  }
  type Processor = Base & { process(inputs: Float32Array[][]): boolean }
  let ProcessorClass!: new (options?: AudioWorkletNodeOptions) => Processor
  vi.stubGlobal('sampleRate', rate); vi.stubGlobal('AudioWorkletProcessor', Base)
  vi.stubGlobal('registerProcessor', (name: string, processor: typeof ProcessorClass) => {
    expect(name).toBe('fc-bounded-mono-capture'); ProcessorClass = processor
  })
  await import('../src/voice/pcm-worklet')
  return ProcessorClass
}

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

it.each([[500, 11025], [1250, 11025], [60000, 8000]])(
  'the actual worklet truncates %i ms at %i Hz to exact mono frames and reports its limit once', async (maxDurationMillis, rate) => {
    const Processor = await realProcessor(rate)
    const processor = new Processor({ processorOptions: { maxDurationMillis } })
    processor.port.onmessage!({ data: { type: 'start' } })
    const expected = Math.floor(rate * maxDurationMillis / 1000)
    const quanta = [17, 128, 4096, 311]
    let supplied = 0, turn = 0
    while (supplied <= expected + 4096) {
      const count = quanta[turn++ % quanta.length]!
      processor.process([[new Float32Array(count).fill(0.2), new Float32Array(count).fill(0.6)]])
      supplied += count
    }
    const beforeLate = processor.port.postMessage.mock.calls.length
    processor.port.onmessage!({ data: { type: 'start' } })
    processor.process([[new Float32Array(4096).fill(1)]])
    expect(processor.port.postMessage.mock.calls).toHaveLength(beforeLate)
    processor.port.onmessage!({ data: { type: 'finish' } })
    const messages = processor.port.postMessage.mock.calls.map(call => call[0] as { type: string; sampleRate?: number; chunk?: Float32Array })
    const pcm = messages.filter(message => message.type === 'pcm')
    expect(pcm.reduce((total, message) => total + message.chunk!.length, 0)).toBe(expected)
    expect(pcm.every(message => message.sampleRate === rate && message.chunk!.length > 0 && message.chunk!.length <= 4096)).toBe(true)
    expect(pcm[0]!.chunk![0]).toBeCloseTo(0.4)
    expect(pcm[pcm.length - 1]!.chunk!.at(-1)).toBeCloseTo(0.4)
    expect(messages.filter(message => message.type === 'limit')).toHaveLength(1)
    expect(messages.filter(message => message.type === 'stopped')).toHaveLength(1)
    expect(messages.filter(message => message.type === 'error')).toHaveLength(0)
  },
)

it('uses the old 60 second default when processor options are omitted or contain no limit', async () => {
  const Processor = await realProcessor(8000)
  for (const options of [undefined, {}, { processorOptions: {} }]) {
    const processor = new Processor(options)
    processor.port.onmessage!({ data: { type: 'start' } })
    for (let i = 0; i < 118; i++) processor.process([[new Float32Array(4096)]])
    const messages = processor.port.postMessage.mock.calls.map(call => call[0] as { type: string; chunk?: Float32Array })
    expect(messages.filter(message => message.type === 'pcm').reduce((total, message) => total + message.chunk!.length, 0)).toBe(480000)
    expect(messages.filter(message => message.type === 'limit')).toHaveLength(1)
  }
})

it.each([0, 499, 60001, 500.5, NaN, Infinity, -Infinity, Number.MAX_SAFE_INTEGER + 1, '500', null, true, {}])(
  'the actual worklet rejects malformed configured duration %s during construction', async maxDurationMillis => {
    const Processor = await realProcessor(8000)
    expect(() => new Processor({ processorOptions: { maxDurationMillis } })).toThrow('DURATION_OUT_OF_RANGE')
  },
)

it('flushes a short partial batch on explicit stop without a limit or allowing a later restart', async () => {
  const Processor = await realProcessor(8000), processor = new Processor({ processorOptions: { maxDurationMillis: 500 } })
  processor.port.onmessage!({ data: { type: 'start' } })
  processor.process([[new Float32Array(17).fill(0.5)]])
  expect(processor.port.postMessage).not.toHaveBeenCalled()
  processor.port.onmessage!({ data: { type: 'finish' } })
  const count = processor.port.postMessage.mock.calls.length
  processor.port.onmessage!({ data: { type: 'start' } })
  processor.process([[new Float32Array(4096).fill(1)]])
  expect(processor.port.postMessage.mock.calls).toHaveLength(count)
  const messages = processor.port.postMessage.mock.calls.map(call => call[0] as { type: string; chunk?: Float32Array })
  expect(messages.filter(message => message.type === 'pcm').map(message => message.chunk!.length)).toEqual([17])
  expect(messages.filter(message => message.type === 'limit')).toHaveLength(0)
  expect(messages.filter(message => message.type === 'stopped')).toHaveLength(1)
})
