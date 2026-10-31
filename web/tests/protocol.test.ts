import { describe, expect, it } from 'vitest'
import { decimal, errorCode, loggedOut, pairingReceipt, pollResult, ProtocolFault, ProtocolVersionFault, protocolVersions, publicStatus, session, strictJson } from '../src/api/protocol'
const csrf = 'ab'.repeat(32)
export const sessionData = () => ({ schemaVersion: 1, status: 'AUTHENTICATED', principal: { id: '18446744073709551615', displayName: '张老师', role: 'TEACHER_ADMIN' }, currentClassId: '2', assignments: [{ classId: '2', subjectId: '4' }], csrf })
describe('strict versioned responses', () => {
  it('separates supported versions, typed upgrade signals and malformed version values', () => {
    const status = { schemaVersion: 1, apiVersion: 1, uiCompatibilityVersion: 1, online: true }
    expect(() => protocolVersions(1, 1, 1)).not.toThrow()
    for (const key of ['schemaVersion', 'apiVersion', 'uiCompatibilityVersion']) {
      for (const version of [2, 37, Number.MAX_SAFE_INTEGER]) {
        const read = () => publicStatus({ ...status, [key]: version })
        expect(read).toThrow(ProtocolVersionFault)
        expect(read).toThrow('白板与页面版本不兼容，请刷新页面；如仍不兼容请联系管理员升级。')
      }
      for (const version of [undefined, null, '1', '2', true, 0, -1, 1.5, NaN, Infinity, Number.MAX_SAFE_INTEGER + 1, {}, []]) {
        const read = () => publicStatus({ ...status, [key]: version })
        expect(read).toThrow(ProtocolFault)
        expect(read).not.toThrow(ProtocolVersionFault)
      }
      const missing = { ...status } as Record<string, unknown>; delete missing[key]
      expect(() => publicStatus(missing)).not.toThrow(ProtocolVersionFault)
      expect(() => publicStatus(missing)).toThrow(ProtocolFault)
    }
    // Every advertised version must be well-typed before any is trusted as an
    // upgrade signal, even when an earlier member is a positive new version.
    expect(() => publicStatus({ ...status, schemaVersion: 2, apiVersion: '1' })).not.toThrow(ProtocolVersionFault)
    expect(() => publicStatus({ ...status, schemaVersion: 2, apiVersion: '1' })).toThrow(ProtocolFault)
    expect(() => publicStatus({ ...status, schemaVersion: 2, extra: true })).not.toThrow(ProtocolVersionFault)
    const fault = new ProtocolVersionFault()
    expect(fault).toBeInstanceOf(ProtocolFault); expect(fault.name).toBe('ProtocolVersionFault')
  })
  it('uses the same typed schema check for pairing, session, logout and error envelopes', () => {
    const readers: ((version: unknown) => unknown)[] = [
      version => pairingReceipt({ schemaVersion: version, status: 'PENDING', requestId: '1', pollSecret: csrf, retryAfterMs: 500 }),
      version => pollResult({ schemaVersion: version, status: 'PENDING', retryAfterMs: 500 }),
      version => pollResult({ schemaVersion: version, status: 'AUTHORIZED' }),
      version => session({ ...sessionData(), schemaVersion: version }),
      version => loggedOut({ schemaVersion: version, status: 'LOGGED_OUT' }),
      version => errorCode({ schemaVersion: version, code: 'RESULT_UNKNOWN' }),
    ]
    for (const read of readers) {
      expect(() => read(1)).not.toThrow()
      expect(() => read(2)).toThrow(ProtocolVersionFault)
      expect(() => read('2')).toThrow(ProtocolFault)
      expect(() => read('2')).not.toThrow(ProtocolVersionFault)
    }
  })
  it('preserves all 64 ID bits and rejects noncanonical or numeric IDs', () => {
    expect(decimal('18446744073709551615')).toBe('18446744073709551615')
    for (const value of ['0', '01', '+1', '1e2', '18446744073709551616', 1, null]) expect(() => decimal(value)).toThrow(ProtocolFault)
    expect(session(sessionData()).principal.id).toBe('18446744073709551615')
  })
  it('requires exact fields, types and compatible API/UI/schema versions', () => {
    const status = { schemaVersion: 1, apiVersion: 1, uiCompatibilityVersion: 1, online: true }
    expect(publicStatus(status)).toEqual({ online: true })
    for (const key of Object.keys(status)) {
      const missing = { ...status } as Record<string, unknown>; delete missing[key]
      expect(() => publicStatus(missing)).toThrow(ProtocolFault)
    }
    for (const key of ['schemaVersion', 'apiVersion', 'uiCompatibilityVersion']) expect(() => publicStatus({ ...status, [key]: 2 })).toThrow(ProtocolFault)
    expect(() => publicStatus({ ...status, online: 'true' })).toThrow(ProtocolFault)
    expect(() => publicStatus({ ...status, schoolName: 'leaked' })).toThrow(ProtocolFault)
    expect(() => session({ ...sessionData(), extra: true })).toThrow(ProtocolFault)
    expect(() => session({ ...sessionData(), csrf: csrf.toUpperCase() })).toThrow(ProtocolFault)
    expect(() => session({ ...sessionData(), principal: { ...sessionData().principal, role: 'ADMIN' } })).toThrow(ProtocolFault)
    expect(() => session({ ...sessionData(), assignments: [...sessionData().assignments, ...sessionData().assignments] })).toThrow(ProtocolFault)
    expect(() => session({ ...sessionData(), principal: { ...sessionData().principal, displayName: 'x\n' } })).toThrow(ProtocolFault)
  })
  it('never accepts a fake grant, unknown status or extra bearer field', () => {
    expect(pairingReceipt({ schemaVersion: 1, status: 'PENDING', requestId: '1', pollSecret: csrf, retryAfterMs: 500 }).requestId).toBe('1')
    expect(pollResult({ schemaVersion: 1, status: 'AUTHORIZED' })).toEqual({ status: 'AUTHORIZED' })
    for (const value of [{ schemaVersion: 1, status: 'AUTHORIZED', cookie: csrf }, { schemaVersion: 1, status: 'PENDING', retryAfterMs: 1 }, { schemaVersion: 1, status: 'GRANTED' }]) expect(() => pollResult(value)).toThrow(ProtocolFault)
    expect(errorCode({ schemaVersion: 1, code: 'RESULT_UNKNOWN' })).toBe('RESULT_UNKNOWN')
    expect(() => errorCode({ schemaVersion: 1, code: 'EVERYTHING_OK' })).toThrow(ProtocolFault)
  })
  it('rejects duplicate/escaped keys, excessive containers and trailing JSON', () => {
    for (const source of ['{"a":1,"a":2}', '{"a":1,"\\u0061":2}', '{"x":{"a":1,"a":2}}', '[1,]', '{}{}', '[NaN]', '['.repeat(34) + '0' + ']'.repeat(34), '[' + Array(4097).fill('0').join(',') + ']']) expect(() => strictJson(source)).toThrow(ProtocolFault)
    expect(strictJson('{"中文":[1,true,null,"\\n"]}')).toEqual({ 中文: [1, true, null, '\n'] })
  })
})
