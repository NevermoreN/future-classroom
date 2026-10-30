import { afterEach, describe, expect, it, vi } from 'vitest'
import { VoiceHttpApi } from '../src/api/voice-client'
import { ApiError, NetworkFault } from '../src/api/client'
import { ProtocolFault } from '../src/api/protocol'
import { csrf, job, json, signal, wav, wireCaps, wireUpload } from './voice-fixtures'
const base = new URL('https://board.school/fc/api/v1/')
const input = { commandId: '71', assetId: '45', expectedAssetRevision: '2', classId: '7', repeatTotal: 3 }
afterEach(() => vi.useRealTimers())
describe('authenticated voice transport', () => {
  it('uses exact shared broadcasts path, captured upload class, credentials and no extra owner/path fields', async () => {
    const fetcher = vi.fn<typeof fetch>().mockResolvedValueOnce(json(wireCaps())).mockResolvedValueOnce(json({ schemaVersion: 1, status: 'VOICE_JOBS', jobs: [] })).mockResolvedValueOnce(json(wireUpload(), 201)).mockResolvedValueOnce(json({ schemaVersion: 1, status: 'COMMITTED', commandId: '71', job: job() })).mockResolvedValueOnce(json({ schemaVersion: 1, status: 'VOICE_JOB', job: job() })).mockResolvedValueOnce(json({ schemaVersion: 1, status: 'CANCEL_REQUESTED', commandId: '81', job: job({ revision: '2', state: 'STOPPING', currentRepeat: 1 }) }))
    const api = new VoiceHttpApi(base, fetcher), blob = wav()
    await api.voiceCapabilities(signal()); await api.voiceJobs(signal()); await api.uploadVoice('7', blob, csrf, signal()); await api.submitVoice(input, csrf, signal()); await api.voiceJob('71', signal()); await api.cancelVoice('71', { commandId: '81', expectedRevision: '1' }, csrf, signal())
    expect(fetcher.mock.calls.map(c => String(c[0]))).toEqual(['voice/capabilities', 'voice/broadcasts', 'voice/uploads?classId=7', 'voice/broadcasts', 'voice/broadcasts/71', 'voice/broadcasts/71/cancel'].map(p => new URL(p, base).href))
    expect(fetcher.mock.calls.every(c => c[1]?.credentials === 'same-origin' && c[1]?.cache === 'no-store' && c[1]?.redirect === 'error')).toBe(true)
    expect(fetcher.mock.calls[2]![1]!.body).toBe(blob)
    expect(new Headers(fetcher.mock.calls[2]![1]!.headers).get('content-type')).toBe('audio/wav')
    for (const index of [2, 3, 5]) expect(new Headers(fetcher.mock.calls[index]![1]!.headers).get('x-fc-csrf')).toBe(csrf)
    expect(JSON.parse(fetcher.mock.calls[3]![1]!.body as string)).toEqual({ schemaVersion: 1, ...input })
    expect(JSON.parse(fetcher.mock.calls[5]![1]!.body as string)).toEqual({ schemaVersion: 1, commandId: '81', expectedRevision: '1' })
  })
  it('rejects duplicate JSON including escaped keys, oversized body, malformed UTF8 and foreign receipt', async () => {
    const value = JSON.stringify(wireCaps()), duplicate = value.replace('"schemaVersion":1', '"schemaVersion":1,"\\u0073chemaVersion":1')
    const responses = [new Response(duplicate, { headers: { 'content-type': 'application/json' } }), new Response(' '.repeat(65537), { headers: { 'content-type': 'application/json' } }), new Response(new Uint8Array([0xff]), { headers: { 'content-type': 'application/json' } })]
    for (const response of responses) { const api = new VoiceHttpApi(base, vi.fn<typeof fetch>().mockResolvedValueOnce(response)); await expect(api.voiceCapabilities(signal())).rejects.toThrow(ProtocolFault) }
    const api = new VoiceHttpApi(base, vi.fn<typeof fetch>().mockResolvedValueOnce(json({ ...wireUpload(), classId: '8' }, 201)))
    await expect(api.uploadVoice('7', wav(), csrf, signal())).rejects.toThrow(ProtocolFault)
  })
  it('purges unauthorized interpretation even for invalid error bodies and never retries unknown submit', async () => {
    for (const status of [401, 403]) { const api = new VoiceHttpApi(base, vi.fn<typeof fetch>().mockResolvedValueOnce(new Response('bad html', { status }))); await expect(api.uploadVoice('7', wav(), csrf, signal())).rejects.toEqual(new ApiError(status, status === 401 ? 'UNAUTHORIZED' : 'FORBIDDEN')) }
    const fetcher = vi.fn<typeof fetch>().mockRejectedValueOnce(new TypeError('lost response')), api = new VoiceHttpApi(base, fetcher)
    await expect(api.submitVoice(input, csrf, signal())).rejects.toThrow(NetworkFault); expect(fetcher).toHaveBeenCalledOnce()
  })
  it('does not send invalid target/WAV/revision or HTTP credentials', async () => {
    const fetcher = vi.fn<typeof fetch>(), api = new VoiceHttpApi(base, fetcher)
    for (const target of ['0', '07', '7&owner=1', '18446744073709551616']) await expect(api.uploadVoice(target, wav(), csrf, signal())).rejects.toThrow(ProtocolFault)
    for (const body of [new Blob([new Uint8Array(16044)], { type: 'audio/webm' }), wav(7999), wav(960001)]) await expect(api.uploadVoice('7', body, csrf, signal())).rejects.toThrow(ProtocolFault)
    await expect(api.submitVoice({ ...input, expectedAssetRevision: '0' }, csrf, signal())).rejects.toThrow(ProtocolFault)
    await expect(new VoiceHttpApi(new URL('http://board.school/fc/api/v1/'), fetcher).uploadVoice('7', wav(), csrf, signal())).rejects.toThrow(ProtocolFault)
    expect(fetcher).not.toHaveBeenCalled()
  })
  it('bounds upload fetch wait and propagates caller abort without automatic replacement', async () => {
    vi.useFakeTimers()
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async (_url, init) => new Promise((_resolve, reject) => { init!.signal!.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError')), { once: true }) }))
    const api = new VoiceHttpApi(base, fetcher), pending = expect(api.uploadVoice('7', wav(), csrf, signal())).rejects.toThrow(NetworkFault)
    await vi.advanceTimersByTimeAsync(120000); await pending; expect(fetcher).toHaveBeenCalledOnce(); expect(vi.getTimerCount()).toBe(0)
    const abort = new AbortController(), cancelled = expect(api.uploadVoice('7', wav(), csrf, abort.signal)).rejects.toMatchObject({ name: 'AbortError' }); abort.abort(); await cancelled; expect(fetcher).toHaveBeenCalledTimes(2)
  })
})
