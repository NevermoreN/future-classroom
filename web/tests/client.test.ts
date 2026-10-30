import { describe, expect, it, vi } from 'vitest'
import { HttpApi, ApiError, readJson, trustedApiBase } from '../src/api/client'
import { MAX_JSON_BYTES, ProtocolFault } from '../src/api/protocol'
const signal = () => new AbortController().signal
function json(value: unknown, status = 200) { return new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json' } }) }
describe('bounded same-origin HTTP', () => {
  it('binds the default native fetch receiver to the global browser context', async () => {
    const original = globalThis.fetch
    globalThis.fetch = function (this: unknown) {
      expect(this).toBe(globalThis)
      return Promise.resolve(json({ schemaVersion: 1, apiVersion: 1, uiCompatibilityVersion: 1, online: true }))
    }
    try { expect(await new HttpApi(new URL('https://board.school/fc/api/v1/')).status(signal())).toEqual({ online: true }) }
    finally { globalThis.fetch = original }
  })
  it('rejects oversize advertised lengths before reading, and stops streaming at 64KiB', async () => {
    const cancel = vi.fn()
    const tooLarge = new Response(new ReadableStream({ cancel }), { headers: { 'content-type': 'application/json', 'content-length': String(MAX_JSON_BYTES + 1) } })
    await expect(readJson(tooLarge, signal())).rejects.toThrow(ProtocolFault); expect(cancel).toHaveBeenCalledOnce()
    const streamingCancel = vi.fn()
    const stream = new ReadableStream<Uint8Array>({ start(c) { c.enqueue(new Uint8Array(MAX_JSON_BYTES)); c.enqueue(new Uint8Array(1)) }, cancel: streamingCancel })
    await expect(readJson(new Response(stream, { headers: { 'content-type': 'application/json' } }), signal())).rejects.toThrow(ProtocolFault)
    expect(streamingCancel).toHaveBeenCalledOnce()
    const exact = '"' + 'a'.repeat(MAX_JSON_BYTES - 2) + '"'
    expect(await readJson(new Response(exact, { headers: { 'content-type': 'application/json' } }), signal())).toHaveLength(MAX_JSON_BYTES - 2)
  })
  it('rejects bad content types, UTF8, duplicate keys and dishonest lengths', async () => {
    for (const response of [new Response('{}'), new Response(new Uint8Array([255]), { headers: { 'content-type': 'application/json' } }), new Response('{"a":1,"a":2}', { headers: { 'content-type': 'application/json' } }), new Response('{}', { headers: { 'content-type': 'application/json', 'content-length': '1' } })]) await expect(readJson(response, signal())).rejects.toThrow(ProtocolFault)
  })
  it('aborts a pending response reader and releases it', async () => {
    const owner = new AbortController(), cancel = vi.fn()
    const response = new Response(new ReadableStream({ cancel }), { headers: { 'content-type': 'application/json' } })
    const pending = readJson(response, owner.signal); owner.abort()
    await expect(pending).rejects.toMatchObject({ name: 'AbortError' }); expect(cancel).toHaveBeenCalledOnce()
  })
  it('keeps secrets in POST bodies, credentials same-origin and CSRF only in private header', async () => {
    const fetcher = vi.fn<typeof fetch>().mockResolvedValueOnce(json({ schemaVersion: 1, status: 'PENDING', requestId: '9', pollSecret: 'a'.repeat(64), retryAfterMs: 500 }, 202)).mockResolvedValueOnce(json({ schemaVersion: 1, status: 'AUTHORIZED' })).mockResolvedValueOnce(json({ schemaVersion: 1, status: 'LOGGED_OUT' }))
    const api = new HttpApi(trustedApiBase('/fc/', 'https://board.school/fc/'), fetcher)
    await api.create({ code: '01234567', loginName: 'teacher', deviceName: '平板' }, signal())
    await api.poll('9', 'a'.repeat(64), signal()); await api.logout('b'.repeat(64), signal())
    expect(fetcher).toHaveBeenCalledTimes(3)
    for (const [url, options] of fetcher.mock.calls) { expect(String(url)).toMatch(/^https:\/\/board.school\/fc\/api\/v1\//); expect(String(url)).not.toContain('Secret'); expect(options?.credentials).toBe('same-origin'); expect(options?.redirect).toBe('error'); expect(options?.method).toBe('POST'); expect(new Headers(options?.headers).has('Origin')).toBe(false) }
    expect(new Headers(fetcher.mock.calls[0]![1]?.headers).has('X-FC-CSRF')).toBe(false)
    expect(new Headers(fetcher.mock.calls[2]![1]?.headers).get('X-FC-CSRF')).toBe('b'.repeat(64))
    expect(fetcher.mock.calls[1]![1]?.body).toBe(JSON.stringify({ schemaVersion: 1, pollSecret: 'a'.repeat(64) }))
  })
  it('does not trust remote bases or malformed auth errors, and never sends credentials over HTTP', async () => {
    for (const base of ['https://other.school/fc/', '//other.school/fc/', '/fc/?secret=x', '/fc/#x']) expect(() => trustedApiBase(base, 'https://board.school/fc/')).toThrow(ProtocolFault)
    const fetcher = vi.fn<typeof fetch>().mockResolvedValueOnce(new Response('not JSON', { status: 403 }))
    await expect(new HttpApi(new URL('https://board.school/fc/api/v1/'), fetcher).session(signal())).rejects.toEqual(new ApiError(403, 'FORBIDDEN'))
    await expect(new HttpApi(new URL('http://127.0.0.1/fc/api/v1/'), fetcher).session(signal())).rejects.toThrow('可信 HTTPS')
    expect(fetcher).toHaveBeenCalledOnce()
  })
})
