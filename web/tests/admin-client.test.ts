import { afterEach, describe, expect, it, vi } from 'vitest'
import { AdminHttpApi, type DisplayData, type IdentityData } from '../src/api/admin-client'
import { setPageToken } from '../src/api/client'
import { ProtocolFault } from '../src/api/protocol'
const signal = () => new AbortController().signal
const envelope = (data: unknown, revision = '1') => ({ schemaVersion: 1, revision, data })
const response = (data: unknown) => new Response(JSON.stringify(data), { headers: { 'Content-Type': 'application/json' } })
const display: DisplayData = { targetMonitor: null, clockMode: 'digital24', clockSeconds: false, recessEnabled: true, subjectFill: true, courseFillPercent: 20, homeworkFillPercent: 3, textTone: 'dark', glassSurfacePercent: 3, edgeReflectionPercent: 50, components: ['clock', 'date', 'schedule', 'homework', 'materials', 'reminder', 'date_countdown', 'recess'].map(kind => ({ kind: kind as DisplayData['components'][number]['kind'], visible: true, opacityPercent: 100, fontDip: 22, minWidth: 160, maxWidth: 800, minHeight: 64, maxHeight: 1200 })), countdown: { target: null, label: '目标日期', expired: 'hide' } }
afterEach(() => setPageToken(undefined))
describe('typed administrator API', () => {
  it('preserves decimal revisions and sends explicit actions with page and CSRF credentials', async () => {
    const revision = '9007199254740993', fetcher = vi.fn<typeof fetch>().mockResolvedValueOnce(response(envelope(display, revision))).mockResolvedValueOnce(response(envelope({ ...display, clockSeconds: true }, '9007199254740994')))
    const api = new AdminHttpApi(new URL('https://board.school/fc/api/v1/'), fetcher), page = 'a'.repeat(128), csrf = 'b'.repeat(64)
    setPageToken(page)
    const current = await api.read('display', signal()); expect(current.revision).toBe(revision)
    const next = await api.write('display', current.revision, { action: 'save', candidate: { ...current.data, clockSeconds: true } }, csrf, signal())
    expect(next.data.clockSeconds).toBe(true); expect(next.revision).toBe('9007199254740994')
    const request = fetcher.mock.calls[1]![1]!
    expect(JSON.parse(request.body as string)).toMatchObject({ schemaVersion: 1, expectedRevision: revision, action: 'save', candidate: { clockSeconds: true } })
    expect(new Headers(request.headers).get('X-FC-Page')).toBe(page); expect(new Headers(request.headers).get('X-FC-CSRF')).toBe(csrf)
  })
  it('supports complete bounded identity documents above the ordinary teacher JSON budget', async () => {
    const identity: IdentityData = { schoolName: '测试学校', currentClassId: null, classes: [], subjects: [], assignments: [], defaultTeachers: [], accounts: Array.from({ length: 512 }, (_, n) => ({ id: String(n + 1), loginName: `teacher-${n}`, displayName: '教师'.repeat(50), role: 'TEACHER_ADMIN', enabled: true, mustChangePassword: false, scopes: [] })) }
    expect(new TextEncoder().encode(JSON.stringify(envelope(identity))).length).toBeGreaterThan(65536)
    const api = new AdminHttpApi(new URL('https://board.school/fc/api/v1/'), vi.fn<typeof fetch>().mockResolvedValue(response(envelope(identity))))
    expect((await api.read('identity', signal())).data.accounts).toHaveLength(512)
  })
  it('rejects malformed public DTOs before a form can use them', async () => {
    for (const data of [{ ...display, components: [{ ...display.components[0], opacityPercent: 101 }] }, { ...display, clockMode: 'unknown' }, { ...display, countdown: { target: null, label: 10, expired: 'hide' } }]) {
      const api = new AdminHttpApi(new URL('https://board.school/fc/api/v1/'), vi.fn<typeof fetch>().mockResolvedValue(response(envelope(data))))
      await expect(api.read('display', signal())).rejects.toBeInstanceOf(ProtocolFault)
    }
  })
  it('caps administrator responses at four MiB before streaming advertised oversized bodies', async () => {
    const cancel = vi.fn(), oversized = new Response(new ReadableStream({ cancel }), { headers: { 'Content-Type': 'application/json', 'Content-Length': String(4 * 1024 * 1024 + 1) } })
    const api = new AdminHttpApi(new URL('https://board.school/fc/api/v1/'), vi.fn<typeof fetch>().mockResolvedValue(oversized))
    await expect(api.read('identity', signal())).rejects.toBeInstanceOf(ProtocolFault); expect(cancel).toHaveBeenCalledOnce()
  })
})
