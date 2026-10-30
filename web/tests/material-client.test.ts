import { afterEach, describe, expect, it, vi } from 'vitest'
import { ApiError, NetworkFault } from '../src/api/client'
import { MaterialHttpApi } from '../src/api/material-client'
import { ProtocolFault } from '../src/api/protocol'
import type { MaterialRecord } from '../src/api/material-protocol'
const target = { ownerTeacherId: '9007199254740993', subjectId: '18446744073709551615' }, csrf = 'b'.repeat(64)
const signal = () => new AbortController().signal
const json = (x: unknown, status = 200) => new Response(JSON.stringify(x), { status, headers: { 'content-type': 'application/json' } })
const file = () => new File(['sample'], '课件.txt', { type: 'text/plain' })
const record = (): MaterialRecord => ({ materialId: '9', folderId: '4', displayName: '课件.txt', resourceVersionId: '10', kind: 'TXT', bytes: 6, sha256: 'a'.repeat(64), allowedClassIds: ['1'], addedAt: '2026-10-30T10:00:00', addedSequence: '1', status: 'PUBLISHED', revision: '1' })
function headers(extra: Record<string, string> = {}) { return { 'content-type': 'application/octet-stream', 'content-length': '6', 'x-fc-sha256': 'a'.repeat(64), 'content-disposition': 'attachment; filename="lesson.txt"', ...extra } }
afterEach(() => vi.useRealTimers())
describe('material HTTP transport has bounded bodies and fixed-version destinations', () => {
  it('separates READY metadata, raw File body, explicit append and original-command GET', async () => {
    const id = '18446744073709551615', cmd = '9007199254740993', f = file()
    const fetcher = vi.fn<typeof fetch>().mockResolvedValueOnce(json({ schemaVersion: 1, status: 'UPLOAD_READY', uploadId: id, expiresInSeconds: 300, maxChunkBytes: 65536 }, 201)).mockResolvedValueOnce(json({ schemaVersion: 1, status: 'UPLOADED', uploadId: id, bytes: f.size })).mockResolvedValueOnce(json({ schemaVersion: 1, status: 'COMMITTED', commandId: cmd, scope: target, record: record() })).mockResolvedValueOnce(json({ schemaVersion: 1, status: 'COMMAND_RESULT', commandId: cmd, state: 'NOT_FOUND', scope: null, record: null }))
    const api = new MaterialHttpApi(new URL('https://board.school/fc/api/v1/'), fetcher)
    await api.createUpload(target, f, csrf, signal()); await api.upload(id, f, csrf, signal())
    expect(fetcher).toHaveBeenCalledTimes(2)
    await api.append(target, { commandId: cmd, uploadId: id, allowedClassIds: ['1'] }, csrf, signal()); await api.materialOutcome(cmd, signal())
    expect(fetcher.mock.calls.map(c => String(c[0]))).toEqual([`https://board.school/fc/api/v1/materials/uploads/${target.ownerTeacherId}/${target.subjectId}`, `https://board.school/fc/api/v1/materials/uploads/${id}/bytes`, `https://board.school/fc/api/v1/materials/folders/${target.ownerTeacherId}/${target.subjectId}/append`, `https://board.school/fc/api/v1/materials/commands/${cmd}`])
    expect(JSON.parse(fetcher.mock.calls[0]![1]!.body as string)).toEqual({ schemaVersion: 1, displayName: f.name, bytes: 6 })
    expect(fetcher.mock.calls[1]![1]?.body).toBe(f)
    expect(new Headers(fetcher.mock.calls[1]![1]?.headers).get('content-type')).toBe('application/octet-stream')
    expect(new Headers(fetcher.mock.calls[1]![1]?.headers).get('x-fc-csrf')).toBe(csrf)
    expect(JSON.parse(fetcher.mock.calls[2]![1]!.body as string)).toEqual({ schemaVersion: 1, commandId: cmd, uploadId: id, allowedClassIds: ['1'] })
    expect(fetcher.mock.calls.every(c => c[1]?.credentials === 'same-origin' && c[1]?.redirect === 'error' && c[1]?.cache === 'no-store')).toBe(true)
    expect(fetcher.mock.calls[3]![1]?.method).toBe('GET')
  })
  it('reads the exact class-page scope and offset with no credentials in query parameters', async () => {
    const wire = { schemaVersion: 1, status: 'MATERIAL_CLASSES', identityRevision: '1', scope: target, offset: 20, total: 21, nextOffset: null, items: [{ classId: '21', className: '班'.repeat(128) }] }
    const fetcher = vi.fn<typeof fetch>().mockResolvedValueOnce(json(wire)).mockResolvedValueOnce(json({ ...wire, scope: { ...target, subjectId: '3' } }))
    const api = new MaterialHttpApi(new URL('https://board.school/fc/api/v1/'), fetcher)
    expect((await api.materialClasses(target, 20, signal())).items[0]?.classId).toBe('21'); await expect(api.materialClasses(target, 20, signal())).rejects.toThrow(ProtocolFault)
    expect(String(fetcher.mock.calls[0]![0])).toBe(`https://board.school/fc/api/v1/materials/classes/${target.ownerTeacherId}/${target.subjectId}/20`); expect(fetcher.mock.calls[0]![1]?.method).toBe('GET')
  })
  it('cancels the exact upload with one CSRF POST and rejects a mismatched acknowledgement', async () => {
    const fetcher = vi.fn<typeof fetch>().mockResolvedValueOnce(json({ schemaVersion: 1, status: 'UPLOAD_CANCELLED', uploadId: '7' })).mockResolvedValueOnce(json({ schemaVersion: 1, status: 'UPLOAD_CANCELLED', uploadId: '8' }))
    const api = new MaterialHttpApi(new URL('https://board.school/fc/api/v1/'), fetcher)
    await api.cancelUpload('7', csrf, signal()); await expect(api.cancelUpload('7', csrf, signal())).rejects.toThrow(ProtocolFault)
    expect(String(fetcher.mock.calls[0]![0])).toBe('https://board.school/fc/api/v1/materials/uploads/7/cancel'); expect(JSON.parse(fetcher.mock.calls[0]![1]!.body as string)).toEqual({ schemaVersion: 1 }); expect(fetcher.mock.calls[0]![1]?.method).toBe('POST'); expect(new Headers(fetcher.mock.calls[0]![1]?.headers).get('x-fc-csrf')).toBe(csrf); expect(fetcher).toHaveBeenCalledTimes(2)
  })
  it('refuses HTTP metadata, bytes and download without sending credentials or bytes', async () => {
    const fetcher = vi.fn<typeof fetch>(), api = new MaterialHttpApi(new URL('http://board.school/fc/api/v1/'), fetcher)
    await expect(api.createUpload(target, file(), csrf, signal())).rejects.toThrow(ProtocolFault)
    await expect(api.upload('1', file(), csrf, signal())).rejects.toThrow(ProtocolFault)
    await expect(api.cancelUpload('1', csrf, signal())).rejects.toThrow(ProtocolFault)
    await expect(api.download(target, record(), signal())).rejects.toThrow(ProtocolFault)
    expect(fetcher).not.toHaveBeenCalled()
  })
  it.each([401, 403])('preserves authentication failure %i even if response stream cancel rejects', async status => {
    const body = new ReadableStream<Uint8Array>({ cancel() { return Promise.reject(new Error('cancel failed')) } })
    const fetcher = vi.fn<typeof fetch>().mockResolvedValue(new Response(body, { status, headers: { 'content-type': 'text/html' } }))
    const api = new MaterialHttpApi(new URL('https://board.school/fc/api/v1/'), fetcher)
    await expect(api.download(target, record(), signal())).rejects.toEqual(new ApiError(status, status === 401 ? 'UNAUTHORIZED' : 'FORBIDDEN'))
    expect(fetcher).toHaveBeenCalledOnce()
  })
  it('requires status 201 READY and never treats JSON upload-ID/name/byte mismatch as accepted', async () => {
    const ready = { schemaVersion: 1, status: 'UPLOAD_READY', uploadId: '1', expiresInSeconds: 300, maxChunkBytes: 65536 }
    const fetcher = vi.fn<typeof fetch>().mockResolvedValueOnce(json(ready, 200)).mockResolvedValueOnce(json({ schemaVersion: 1, status: 'UPLOADED', uploadId: '2', bytes: 6 })).mockResolvedValueOnce(json({ schemaVersion: 1, status: 'UPLOADED', uploadId: '1', bytes: 7 }))
    const api = new MaterialHttpApi(new URL('https://board.school/fc/api/v1/'), fetcher)
    await expect(api.createUpload(target, file(), csrf, signal())).rejects.toThrow(ProtocolFault)
    await expect(api.upload('1', file(), csrf, signal())).rejects.toThrow(ProtocolFault)
    await expect(api.upload('1', file(), csrf, signal())).rejects.toThrow(ProtocolFault)
    expect(fetcher).toHaveBeenCalledTimes(3)
  })
  it('caps actual JSON stream bytes at 64KiB despite a missing content-length', async () => {
    const cancel = vi.fn(), body = new ReadableStream<Uint8Array>({ start(c) { c.enqueue(new Uint8Array(65537)) }, cancel })
    const fetcher = vi.fn<typeof fetch>().mockResolvedValue(new Response(body, { headers: { 'content-type': 'application/json' } }))
    await expect(new MaterialHttpApi(new URL('https://board.school/fc/api/v1/'), fetcher).materialScopes(0, signal())).rejects.toThrow(ProtocolFault)
    expect(cancel).toHaveBeenCalledOnce()
  })
  it('validates fixed-version download metadata before allocating its body', async () => {
    for (const changed of [{ 'content-type': 'text/plain' }, { 'content-length': '7' }, { 'x-fc-sha256': 'b'.repeat(64) }, { 'content-disposition': 'inline' }]) {
      const cancel = vi.fn(), body = new ReadableStream<Uint8Array>({ cancel })
      const fetcher = vi.fn<typeof fetch>().mockResolvedValue(new Response(body, { headers: headers(changed) }))
      await expect(new MaterialHttpApi(new URL('https://board.school/fc/api/v1/'), fetcher).download(target, record(), signal())).rejects.toThrow(ProtocolFault)
      expect(cancel).toHaveBeenCalledOnce()
    }
  })
  it('rejects truncated and overflowing downloads, cancels overflow, and returns exact bytes only', async () => {
    const cancel = vi.fn(), overflow = new ReadableStream<Uint8Array>({ start(c) { c.enqueue(new Uint8Array(7)) }, cancel })
    const fetcher = vi.fn<typeof fetch>().mockResolvedValueOnce(new Response('short', { headers: headers() })).mockResolvedValueOnce(new Response(overflow, { headers: headers() })).mockResolvedValueOnce(new Response('sample', { headers: headers() }))
    const api = new MaterialHttpApi(new URL('https://board.school/fc/api/v1/'), fetcher)
    await expect(api.download(target, record(), signal())).rejects.toThrow(ProtocolFault)
    await expect(api.download(target, record(), signal())).rejects.toThrow(ProtocolFault); expect(cancel).toHaveBeenCalledOnce()
    const result = await api.download(target, record(), signal()); expect(await result.text()).toBe('sample')
    expect(String(fetcher.mock.calls[2]![0])).toBe(`https://board.school/fc/api/v1/materials/download/${target.ownerTeacherId}/${target.subjectId}/9/10`)
  })
  it('aborts binary upload at the absolute 120s budget with no retry', async () => {
    vi.useFakeTimers()
    const fetcher = vi.fn<typeof fetch>().mockImplementation((_url, init) => new Promise((_resolve, reject) => init?.signal?.addEventListener('abort', () => reject(new DOMException('stopped', 'AbortError')), { once: true })))
    const api = new MaterialHttpApi(new URL('https://board.school/fc/api/v1/'), fetcher), operation = api.upload('1', file(), csrf, signal()), rejected = expect(operation).rejects.toThrow(NetworkFault)
    await vi.advanceTimersByTimeAsync(119999); expect(fetcher.mock.calls[0]![1]?.signal?.aborted).toBe(false)
    await vi.advanceTimersByTimeAsync(1); await rejected; expect(fetcher).toHaveBeenCalledOnce(); expect(vi.getTimerCount()).toBe(0)
  })
  it('download progress cannot renew the absolute binary deadline and cancellation frees the reader', async () => {
    vi.useFakeTimers(); let writer!: ReadableStreamDefaultController<Uint8Array>
    const cancel = vi.fn(), body = new ReadableStream<Uint8Array>({ start(c) { writer = c }, cancel })
    const fetcher = vi.fn<typeof fetch>().mockResolvedValue(new Response(body, { headers: headers() }))
    const api = new MaterialHttpApi(new URL('https://board.school/fc/api/v1/'), fetcher), operation = api.download(target, record(), signal()), rejected = expect(operation).rejects.toThrow(NetworkFault)
    await vi.advanceTimersByTimeAsync(60000); writer.enqueue(new Uint8Array(1)); await vi.advanceTimersByTimeAsync(59000); writer.enqueue(new Uint8Array(1)); await vi.advanceTimersByTimeAsync(1000); await rejected
    expect(cancel).toHaveBeenCalledOnce(); expect(body.locked).toBe(false); expect(fetcher).toHaveBeenCalledOnce(); expect(vi.getTimerCount()).toBe(0)
  })
})
