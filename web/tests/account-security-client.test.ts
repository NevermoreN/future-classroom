import { afterEach, describe, expect, it, vi } from 'vitest'
import { AccountSecurityHttpApi } from '../src/api/account-security-client'
import { pageSession, ProtocolFault } from '../src/api/protocol'
import { setPageToken } from '../src/api/client'
const signal = () => new AbortController().signal
const json = (data: unknown) => new Response(JSON.stringify({ schemaVersion: 1, revision: '4', data }), { headers: { 'Content-Type': 'application/json' } })
const base = new URL('https://127.0.0.1/fc/api/v1/')
const code = 'FC1-' + 'c'.repeat(64)
afterEach(() => setPageToken(undefined))
describe('existing account credential operations', () => {
  it('retains a transfer result ticket before submitting the explicit double-confirmed credential action', async () => {
    const ticket = 'a'.repeat(64), fetcher = vi.fn<typeof fetch>().mockResolvedValueOnce(json({ ticket })).mockResolvedValueOnce(json({ state: 'COMMITTED', revision: null })).mockResolvedValueOnce(json({ state: 'COMMITTED', revision: '5' })), api = new AccountSecurityHttpApi(base, fetcher)
    setPageToken('b'.repeat(128))
    expect(await api.prepare('4', '2', 'd'.repeat(64), signal())).toBe(ticket)
    await api.transfer(ticket, 'current-password-2026', 'd'.repeat(64), signal())
    expect(JSON.parse(fetcher.mock.calls[1]![1]!.body as string)).toMatchObject({ action: 'transfer', ticket, reviewedTarget: true, understoodRevocation: true })
    expect(await api.outcome(ticket, 'd'.repeat(64), signal())).toBe('COMMITTED')
    expect(JSON.parse(fetcher.mock.calls[2]![1]!.body as string)).not.toHaveProperty('currentPassword')
  })
  it('sends recovery credentials once without adding them to normal username-password login', async () => {
    const fetcher = vi.fn<typeof fetch>().mockResolvedValueOnce(json({ recoveryCode: code })), api = new AccountSecurityHttpApi(base, fetcher)
    expect(await api.recover('admin', code, 'new-password-2026', signal())).toBe(code)
    expect(JSON.parse(fetcher.mock.calls[0]![1]!.body as string)).toEqual({ schemaVersion: 1, loginName: 'admin', recoveryCode: code, newPassword: 'new-password-2026' })
  })
  it('accepts only a validated one-time setup code in the authenticated response', () => {
    const session = { schemaVersion: 1, status: 'AUTHENTICATED', principal: { id: '1', displayName: 'Admin', role: 'GLOBAL_ADMIN' }, currentClassId: null, assignments: [], csrf: 'b'.repeat(64), pageToken: 'd'.repeat(128), recoveryCode: code }
    expect(pageSession(session).recoveryCode).toBe(code)
    expect(() => pageSession({ ...session, recoveryCode: 'bad' })).toThrow(ProtocolFault)
  })
})
