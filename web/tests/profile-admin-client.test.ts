import { afterEach, describe, expect, it, vi } from 'vitest'
import { ProfileAdminHttpApi, type ContentImage, type SubjectProfileData } from '../src/api/profile-admin-client'
import { setPageToken } from '../src/api/client'
import { ProtocolFault } from '../src/api/protocol'
const signal = () => new AbortController().signal
const key = { classId: '1', subjectId: '2', teacherId: '3' }, token = 'a'.repeat(128), csrf = 'b'.repeat(64)
const style = { kind: 'materials' as const, visible: true, opacityPercent: 100, fontDip: 22, minWidth: 160, maxWidth: 800, minHeight: 64, maxHeight: 1200 }
const profile: SubjectProfileData = { exists: false, key, enabled: true, layoutRevision: '0', layout: { nodeIdHighWater: '1', root: '1', nodes: [{ id: '1', kind: 'root', children: [], flow: 'column', anchor: 'top_left', padding: 16, gap: 12, width: { kind: 'fixed', value: 400 }, height: { kind: 'fixed', value: 600 } }], contentStyles: [] }, folderId: null, materials: { component: style, visibleRows: 5, rowGapDip: 8 } }
const json = (data: unknown, revision = '0') => new Response(JSON.stringify({ schemaVersion: 1, revision, data }), { headers: { 'content-type': 'application/json' } })
afterEach(() => setPageToken(undefined))
describe('subject profile and independent content APIs', () => {
  it('reads a server default candidate and saves it at the same canonical subject key', async () => {
    const fetcher = vi.fn<typeof fetch>().mockResolvedValueOnce(json(profile)).mockResolvedValueOnce(json({ ...profile, exists: true }, '1'))
    const api = new ProfileAdminHttpApi(new URL('https://board.school/fc/api/v1/'), fetcher)
    setPageToken(token)
    const current = await api.profile(key, signal()), { exists: _, key: __, layoutRevision: ___, ...candidate } = current.data
    expect(current.revision).toBe('0'); expect(current.data.exists).toBe(false)
    expect((await api.saveProfile(key, current.revision, candidate, csrf, signal())).data.exists).toBe(true)
    expect(String(fetcher.mock.calls[0]![0])).toMatch(/admin\/profile\/1\/2\/3$/)
    expect(JSON.parse(fetcher.mock.calls[1]![1]!.body as string)).toMatchObject({ schemaVersion: 1, expectedRevision: '0', action: 'save', candidate: { enabled: true } })
    expect(new Headers(fetcher.mock.calls[1]![1]?.headers).get('X-FC-Page')).toBe(token)
  })
  it('rejects a profile returned for another teacher rather than binding it to the selected scope', async () => {
    const api = new ProfileAdminHttpApi(new URL('https://board.school/fc/api/v1/'), vi.fn<typeof fetch>().mockResolvedValue(json({ ...profile, key: { ...key, teacherId: '4' } })))
    await expect(api.profile(key, signal())).rejects.toBeInstanceOf(ProtocolFault)
  })
  it('uses explicit content and view revisions for text, undo and paging', async () => {
    const content = { kind: 'text', text: '实际正文', pages: [], previousRevision: '6', viewRevision: '8', currentPage: 0, pending: false, updatedAt: '2026-11-04T10:00:00' }
    const fetcher = vi.fn<typeof fetch>().mockImplementation(async () => json(content, '9')), api = new ProfileAdminHttpApi(new URL('https://board.school/fc/api/v1/'), fetcher)
    const slot = { scope: 'subject' as const, key, nodeId: '12' }
    expect((await api.readSlotContent(slot, signal())).data.text).toBe('实际正文')
    await api.writeContent(slot, '9', { action: 'undo', candidate: { targetRevision: '6' } }, csrf, signal())
    expect(String(fetcher.mock.calls[1]![0])).toMatch(/admin\/content\/subject\/1\/2\/3\/12$/)
    expect(JSON.parse(fetcher.mock.calls[1]![1]!.body as string)).toEqual({ schemaVersion: 1, expectedRevision: '9', action: 'undo', candidate: { targetRevision: '6' } })
  })
  it('fetches actual saved image bytes with the page credential and verifies the advertised metadata', async () => {
    const image: ContentImage = { mediaVersionId: '7', width: 1, height: 1, bytes: 3, sha256: 'c'.repeat(64), format: 'png' }, fetcher = vi.fn<typeof fetch>().mockResolvedValueOnce(new Response(new Uint8Array([1, 2, 3]), { status: 206, headers: { 'content-type': 'image/png', 'content-length': '3', 'content-range': 'bytes 0-2/3' } })).mockResolvedValueOnce(new Response(new Uint8Array([1, 2]), { headers: { 'content-type': 'image/png' } }))
    const api = new ProfileAdminHttpApi(new URL('https://board.school/fc/api/v1/'), fetcher), slot = { scope: 'public' as const, nodeId: '2' }; setPageToken(token)
    expect((await api.image(slot, image, signal())).size).toBe(3)
    expect(String(fetcher.mock.calls[0]![0])).toMatch(/admin\/content\/public\/2\/media\/7$/)
    expect(new Headers(fetcher.mock.calls[0]![1]?.headers).get('X-FC-Page')).toBe(token)
    await expect(api.image(slot, image, signal())).rejects.toBeInstanceOf(ProtocolFault)
  })
})

describe('controlled image upload batches', () => {
  it('binds manifest, binary bytes and publication to the exact content slot and revision', async () => {
    const file = new File([new Uint8Array([1, 2, 3])], 'lesson.png', { type: 'image/png' }), slot = { scope: 'public' as const, nodeId: '12' }
    const document = { kind: 'images', text: null, pages: [{ mediaVersionId: '7', width: 1, height: 1, bytes: 3, sha256: 'c'.repeat(64), format: 'png' }], previousRevision: null, viewRevision: '1', currentPage: 0, pending: false, canPage: false, updatedAt: null }
    const fixed = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status, headers: { 'content-type': 'application/json' } })
    const fetcher = vi.fn<typeof fetch>().mockResolvedValueOnce(fixed({ schemaVersion: 1, batchId: '6', uploads: [{ id: '7', name: file.name, bytes: file.size }] }, 201)).mockResolvedValueOnce(fixed({ schemaVersion: 1, status: 'UPLOADED' })).mockResolvedValueOnce(json(document, '1'))
    const api = new ProfileAdminHttpApi(new URL('https://board.school/fc/api/v1/'), fetcher); setPageToken(token)
    const batch = await api.createImageBatch(slot, '0', [file], csrf, signal()); await api.uploadImage(slot, batch.batchId, batch.uploads[0]!.id, file, csrf, signal())
    expect((await api.writeContent(slot, '0', { action: 'publishImages', candidate: { batchId: batch.batchId, pages: [{ source: 'upload', uploadId: '7' }] } }, csrf, signal())).data.pages[0]?.mediaVersionId).toBe('7')
    expect(JSON.parse(fetcher.mock.calls[0]![1]!.body as string)).toEqual({ schemaVersion: 1, expectedRevision: '0', items: [{ name: 'lesson.png', bytes: 3 }] })
    expect(String(fetcher.mock.calls[1]![0])).toMatch(/admin\/content\/public\/12\/uploads\/6\/7\/bytes$/)
    expect((fetcher.mock.calls[1]![1]!.body as Blob).size).toBe(file.size); expect(new Headers(fetcher.mock.calls[1]![1]?.headers).get('X-FC-Upload-Offset')).toBe('0'); expect(new Headers(fetcher.mock.calls[1]![1]?.headers).get('content-type')).toBe('application/octet-stream')
    expect(new Headers(fetcher.mock.calls[1]![1]?.headers).get('X-FC-Page')).toBe(token); expect(new Headers(fetcher.mock.calls[1]![1]?.headers).get('X-FC-CSRF')).toBe(csrf)
  })
  it('splits large image uploads into sequential 512KiB chunks without automatic retries', async () => {
    const file = new File([new Uint8Array(524289)], 'page.png'), fetcher = vi.fn<typeof fetch>().mockImplementation(async () => new Response(JSON.stringify({ schemaVersion: 1, status: 'UPLOADED' }), { headers: { 'content-type': 'application/json' } }))
    const api = new ProfileAdminHttpApi(new URL('https://board.school/fc/api/v1/'), fetcher)
    await api.uploadImage({ scope: 'public', nodeId: '12' }, '6', '7', file, csrf, signal())
    expect(fetcher).toHaveBeenCalledTimes(2)
    expect(fetcher.mock.calls.map(call => new Headers(call[1]?.headers).get('X-FC-Upload-Offset'))).toEqual(['0', '524288'])
    expect(fetcher.mock.calls.map(call => (call[1]?.body as Blob).size)).toEqual([524288, 1])
  })
  it('uses explicit cancellation and rejects an unconfirmed cleanup response', async () => {
    const fixed = (status: string) => new Response(JSON.stringify({ schemaVersion: 1, status }), { headers: { 'content-type': 'application/json' } })
    const fetcher = vi.fn<typeof fetch>().mockResolvedValueOnce(fixed('CANCELLED')).mockResolvedValueOnce(fixed('CANCELLING')), api = new ProfileAdminHttpApi(new URL('https://board.school/fc/api/v1/'), fetcher), slot = { scope: 'public' as const, nodeId: '12' }
    await api.cancelImageBatch(slot, '6', csrf, signal()); expect(String(fetcher.mock.calls[0]![0])).toMatch(/uploads\/6\/cancel$/)
    await expect(api.cancelImageBatch(slot, '6', csrf, signal())).rejects.toBeInstanceOf(ProtocolFault)
  })
})
