import { describe, expect, it, vi } from 'vitest'
import { ApiError, HttpApi } from '../src/api/client'
import { ProtocolFault } from '../src/api/protocol'
import { homeworkKey, wireDocument, wireScopes } from './content-fixtures'
const signal = () => new AbortController().signal
const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } })
describe('content request transport and result correlation', () => {
  it('uses same-origin exact resource paths, real scope fields, CSRF and only strict write payload', async () => {
    const fetcher = vi.fn<typeof fetch>().mockResolvedValueOnce(json(wireScopes())).mockResolvedValueOnce(json(wireDocument())).mockResolvedValueOnce(json({ schemaVersion: 1, status: 'COMMITTED', commandId: '18446744073709551615', content: wireDocument(homeworkKey, '1', '正文') })).mockResolvedValueOnce(json({ schemaVersion: 1, status: 'COMMAND_RESULT', commandId: '18446744073709551615', state: 'COMMITTED', content: wireDocument(homeworkKey, '1', '正文') }))
    const api = new HttpApi(new URL('https://board.school/fc/api/v1/'), fetcher)
    await api.scopes('HOMEWORK', 0, signal()); await api.content(homeworkKey, signal())
    await api.saveContent(homeworkKey, { commandId: '18446744073709551615', expectedRevision: '0', text: '正文' }, 'b'.repeat(64), signal())
    await api.commandResult('18446744073709551615', signal())
    expect(fetcher.mock.calls.map(call => String(call[0]))).toEqual(['https://board.school/fc/api/v1/content/homework/scopes/0', 'https://board.school/fc/api/v1/content/homework/1/2/9/2026-10-30', 'https://board.school/fc/api/v1/content/homework/1/2/9/2026-10-30', 'https://board.school/fc/api/v1/content/commands/18446744073709551615'])
    expect(JSON.parse(fetcher.mock.calls[2]![1]!.body as string)).toEqual({ schemaVersion: 1, commandId: '18446744073709551615', expectedRevision: '0', text: '正文' })
    expect(new Headers(fetcher.mock.calls[2]![1]!.headers).get('x-fc-csrf')).toBe('b'.repeat(64))
    expect(fetcher.mock.calls.every(call => call[1]?.credentials === 'same-origin')).toBe(true)
    expect(fetcher.mock.calls.filter(call => call[1]?.method === 'POST')).toHaveLength(1)
  })
  it('retains typed conflict/unknown errors with no automatic retry', async () => {
    const fetcher = vi.fn<typeof fetch>().mockResolvedValueOnce(json({ schemaVersion: 1, code: 'REVISION_CONFLICT' }, 409)).mockResolvedValueOnce(json({ schemaVersion: 1, code: 'RESULT_UNKNOWN' }, 503))
    const api = new HttpApi(new URL('https://board.school/fc/api/v1/'), fetcher), command = { commandId: '7', expectedRevision: '0', text: '' }
    await expect(api.saveContent(homeworkKey, command, 'b'.repeat(64), signal())).rejects.toEqual(new ApiError(409, 'REVISION_CONFLICT'))
    await expect(api.saveContent(homeworkKey, command, 'b'.repeat(64), signal())).rejects.toEqual(new ApiError(503, 'RESULT_UNKNOWN'))
    expect(fetcher).toHaveBeenCalledTimes(2)
  })
  it('blocks wrong scope kind/key/command identity instead of adopting another target', async () => {
    const fetcher = vi.fn<typeof fetch>().mockResolvedValueOnce(json(wireScopes('REMINDER'))).mockResolvedValueOnce(json(wireDocument({ ...homeworkKey, classId: '2' }))).mockResolvedValueOnce(json({ schemaVersion: 1, status: 'COMMAND_RESULT', commandId: '8', state: 'NOT_FOUND', content: null }))
    const api = new HttpApi(new URL('https://board.school/fc/api/v1/'), fetcher)
    await expect(api.scopes('HOMEWORK', 0, signal())).rejects.toThrow(ProtocolFault)
    await expect(api.content(homeworkKey, signal())).rejects.toThrow(ProtocolFault)
    await expect(api.commandResult('7', signal())).rejects.toThrow(ProtocolFault)
  })
})
