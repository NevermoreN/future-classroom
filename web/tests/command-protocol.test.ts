import { describe, expect, it, vi } from 'vitest'
import { commandIdentity, commandTicket, issued } from '../src/api/command-protocol'
import { ApiError, HttpApi } from '../src/api/client'
import { ProtocolFault, ProtocolVersionFault } from '../src/api/protocol'
import { identity, ticket } from './command-fixtures'

const body = () => ({ schemaVersion: 2, status: 'ISSUED', family: 'content', ...identity('18446744073709551615'), expiresInMillis: 86400000 })
const signal = () => new AbortController().signal
const csrf = 'ab'.repeat(32)
const json = (value: unknown, status: number) => new Response(JSON.stringify(value), { status, headers: { 'content-type': 'application/json' } })
describe('server-issued command identity', () => {
  it('keeps epoch and serial bytes canonical and never rounds through Number', () => {
    const maximum = 'c2:' + 'fe'.repeat(16) + ':9223372036854775807'
    expect(commandTicket(maximum)).toBe(maximum)
    expect(issued(body(), 'content')).toEqual(identity('18446744073709551615'))
    expect(commandIdentity(identity('9007199254740993'))).toEqual(identity('9007199254740993'))
    for (const invalid of [null, 1, ticket.toUpperCase(), ticket.replace('c2:', 'c1:'), ticket + '/', ticket.replace(':1', ':01'), 'c2:' + '0'.repeat(32) + ':1', 'c2:' + '12'.repeat(16) + ':0', 'c2:' + '12'.repeat(16) + ':9223372036854775808', ticket.replace('c2:', 'c2%3A')]) expect(() => commandTicket(invalid)).toThrow(ProtocolFault)
  })
  it('requires the exact family, pair, duration and schema, with no authority from syntax alone', () => {
    for (const patch of [{ family: 'voice' }, { family: 'CONTENT' }, { status: 'COMMITTED' }, { expiresInMillis: 86400001 }, { commandId: '01' }, { commandId: '0' }, { commandId: 7 }, { commandTicket: '7' }, { path: '/api/v1/content' }]) expect(() => issued({ ...body(), ...patch }, 'content')).toThrow(ProtocolFault)
    expect(() => issued({ ...body(), schemaVersion: 1 }, 'content')).toThrow(ProtocolVersionFault)
    for (const version of [null, '2', 0, -1, 1.5]) {
      expect(() => issued({ ...body(), schemaVersion: version }, 'content')).toThrow(ProtocolFault)
      expect(() => issued({ ...body(), schemaVersion: version }, 'content')).not.toThrow(ProtocolVersionFault)
    }
    const missing = body() as Record<string, unknown>; delete missing.commandTicket
    expect(() => issued(missing, 'content')).toThrow(ProtocolFault)
  })
  it('issues only through the authenticated CSRF-protected endpoint with no caller nonce', async () => {
    const fetcher = vi.fn<typeof fetch>().mockResolvedValueOnce(json(body(), 201))
    const api = new HttpApi(new URL('https://board.school/fc/api/v1/'), fetcher)
    expect(await api.issueCommand('content', csrf, signal())).toEqual(identity('18446744073709551615'))
    expect(String(fetcher.mock.calls[0]![0])).toBe('https://board.school/fc/api/v1/commands')
    const init = fetcher.mock.calls[0]![1]!
    expect(JSON.parse(init.body as string)).toEqual({ schemaVersion: 2, family: 'content' })
    expect(new Headers(init.headers).get('x-fc-csrf')).toBe(csrf)
    expect(init).toMatchObject({ method: 'POST', credentials: 'same-origin', redirect: 'error', cache: 'no-store' })
  })
  it.each([[410, 'COMMAND_EXPIRED'], [404, 'COMMAND_NOT_ISSUED'], [503, 'COMMAND_WINDOW_FULL'], [503, 'COMMAND_WINDOW_UNAVAILABLE'], [503, 'RESULT_UNKNOWN']] as const)('keeps the exact typed %s/%s error without retries', async (status, code) => {
    const fetcher = vi.fn<typeof fetch>().mockResolvedValueOnce(json({ schemaVersion: 1, code }, status))
    const api = new HttpApi(new URL('https://board.school/fc/api/v1/'), fetcher)
    await expect(api.issueCommand('content', csrf, signal())).rejects.toEqual(new ApiError(status, code))
    expect(fetcher).toHaveBeenCalledOnce()
  })
})
