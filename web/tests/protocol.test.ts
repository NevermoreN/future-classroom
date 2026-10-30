import { describe, expect, it } from 'vitest'
import { decimal, errorCode, pairingReceipt, pollResult, ProtocolFault, publicStatus, session, strictJson } from '../src/api/protocol'
const csrf = 'ab'.repeat(32)
export const sessionData = () => ({ schemaVersion: 1, status: 'AUTHENTICATED', principal: { id: '18446744073709551615', displayName: '张老师', role: 'TEACHER_ADMIN' }, currentClassId: '2', assignments: [{ classId: '2', subjectId: '4' }], csrf })
describe('strict versioned responses', () => {
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
