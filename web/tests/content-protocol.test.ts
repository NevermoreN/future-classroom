import { describe, expect, it } from 'vitest'
import { bodyText, civilDate, civilTime, commandResult, committed, content, contentPath, newCommandId, nextDay, normalizedBodyText, revision, scopes } from '../src/api/content-protocol'
import { ProtocolFault } from '../src/api/protocol'
import { homeworkKey, wireDocument, wireScopes } from './content-fixtures'
describe('teacher content strict wire contract', () => {
  it('keeps command/revision 64 bits, allows only canonical zero revisions, never IDs', () => {
    expect(revision('0')).toBe('0'); expect(revision('18446744073709551615')).toBe('18446744073709551615')
    for (const invalid of ['00', '01', '-1', 0, '18446744073709551616']) expect(() => revision(invalid)).toThrow(ProtocolFault)
    expect(() => contentPath({ ...homeworkKey, classId: '0' })).toThrow(ProtocolFault)
    expect(contentPath(homeworkKey)).toBe('content/homework/1/2/9/2026-10-30')
  })
  it('validates Gregorian local dates/times without teacher timezone or rollover normalization', () => {
    for (const valid of ['0001-01-01', '0004-02-29', '2000-02-29', '9999-12-31']) expect(civilDate(valid)).toBe(valid)
    for (const invalid of ['0000-01-01', '1900-02-29', '2026-02-29', '2026-04-31', '2026-1-01', '2026-10-30Z']) expect(() => civilDate(invalid)).toThrow(ProtocolFault)
    for (const invalid of ['2026-10-30T24:00:00', '2026-10-30T00:60:00', '2026-10-30T00:00:60', '2026-10-30T12:00:00Z', '2026-10-30T12:00:00+08:00']) expect(() => civilTime(invalid)).toThrow(ProtocolFault)
    expect(nextDay('2026-12-31')).toBe('2027-01-01'); expect(() => nextDay('9999-12-31')).toThrow(ProtocolFault)
  })
  it('bounds exact UTF8 bytes and rejects control/unpaired-surrogate input, permits normal linebreaks', () => {
    expect(bodyText('🙂'.repeat(4096))).toHaveLength(8192)
    expect(bodyText('a\t\n\r字')).toBe('a\t\n\r字')
    expect(bodyText('\ufeff正文\ufeff')).toBe('\ufeff正文\ufeff')
    expect(normalizedBodyText(' \t\n　正文　\r ')).toBe('正文')
    expect(normalizedBodyText(' \t\n　 ')).toBe('')
    expect(normalizedBodyText(' \ufeff正文\ufeff ')).toBe('\ufeff正文\ufeff')
    for (const invalid of ['🙂'.repeat(4097), 'a'.repeat(16385), '\u0000', '\u000b', '\u007f', '\u0085', '\ud800']) expect(() => bodyText(invalid)).toThrow(ProtocolFault)
  })
  it('requires exact page kind/offset, numeric order, bounded nonstalling continuation and names', () => {
    expect(scopes(wireScopes('HOMEWORK', 0, 21), 'HOMEWORK', 0).nextOffset).toBe(20)
    expect(scopes(wireScopes('HOMEWORK', 20, 21), 'HOMEWORK', 20).items).toHaveLength(1)
    for (const bad of [
      { ...wireScopes(), offset: '0' }, { ...wireScopes(), offset: 1 }, { ...wireScopes(), total: 4097 },
      { ...wireScopes('HOMEWORK', 0, 21), nextOffset: 19 }, { ...wireScopes('HOMEWORK', 0, 21), items: [] },
      { ...wireScopes(), role: 'GLOBAL_ADMIN' }, { ...wireScopes(), items: [{ ...wireScopes().items[0], role: 'GLOBAL_ADMIN' }] },
      { ...wireScopes('HOMEWORK', 0, 2), items: wireScopes('HOMEWORK', 0, 2).items.reverse() },
      { ...wireScopes('HOMEWORK', 0, 2), items: [wireScopes().items[0], wireScopes().items[0]] },
      { ...wireScopes(), items: [{ ...wireScopes().items[0], className: 'x\n' }] },
    ]) expect(() => scopes(bad, 'HOMEWORK', 0)).toThrow(ProtocolFault)
    expect(() => scopes(wireScopes('HOMEWORK'), 'REMINDER', 0)).toThrow(ProtocolFault)
    expect(() => scopes({ ...wireScopes('REMINDER'), total: 129 }, 'REMINDER', 0)).toThrow(ProtocolFault)
  })
  it('distinguishes nonexistent revision0 from cleared nonzero records and rejects inconsistent authority metadata', () => {
    expect(content(wireDocument()).revision).toBe('0')
    expect(content(wireDocument(homeworkKey, '7', '')).revision).toBe('7')
    const rollback = content({ ...wireDocument(homeworkKey, '1', '保留同日正文'), serverNow: '2026-10-30T08:55:00', updatedAt: '2026-10-30T09:00:00' })
    expect(rollback.updatedAt).toBe('2026-10-30T09:00:00'); expect(rollback.today).toBe('2026-10-30'); expect(rollback.text).toBe('保留同日正文')
    for (const bad of [
      { ...wireDocument(), text: 'hidden' }, { ...wireDocument(), lastActorId: '9' },
      { ...wireDocument(homeworkKey, '1'), updatedAt: null }, { ...wireDocument(homeworkKey, '1'), lastActorId: null },
      { ...wireDocument(), today: '2026-10-31' }, { ...wireDocument(), expiresAt: '2026-11-01T00:00:00' },
      { ...wireDocument(), serverNow: '2026-10-31T10:00:00' },
      { ...wireDocument(), key: { ...homeworkKey, actor: '1' } }, { ...wireDocument(), visible: true },
    ]) expect(() => content(bad)).toThrow(ProtocolFault)
  })
  it('correlates committed ID/key/next revision and keeps result NOT_FOUND distinct from commit', () => {
    const good = { schemaVersion: 1, status: 'COMMITTED', commandId: '18446744073709551615', content: wireDocument(homeworkKey, '1', '作业') }
    expect(committed(good, good.commandId, homeworkKey, '0', '作业').content.text).toBe('作业')
    for (const bad of [{ ...good, commandId: '1' }, { ...good, content: wireDocument({ ...homeworkKey, classId: '2' }, '1') }, { ...good, content: wireDocument(homeworkKey, '2') }]) expect(() => committed(bad, good.commandId, homeworkKey, '0', '作业')).toThrow(ProtocolFault)
    expect(commandResult({ schemaVersion: 1, status: 'COMMAND_RESULT', commandId: '7', state: 'NOT_FOUND', content: null }, '7').state).toBe('NOT_FOUND')
    expect(() => commandResult({ schemaVersion: 1, status: 'COMMAND_RESULT', commandId: '7', state: 'NOT_FOUND', content: wireDocument() }, '7')).toThrow(ProtocolFault)
    expect(() => commandResult({ schemaVersion: 1, status: 'COMMAND_RESULT', commandId: '8', state: 'COMMITTED', content: wireDocument() }, '7')).toThrow(ProtocolFault)
  })
  it('uses CSPRNG 64 bits without rounding or zero fallback', () => {
    let calls = 0
    const random = { getRandomValues: <T extends ArrayBufferView | null>(array: T): T => { const values = array as Uint32Array; values.fill(calls++ === 0 ? 0 : 0xffffffff); return array } }
    expect(newCommandId(random)).toBe('18446744073709551615'); expect(calls).toBe(2)
    const zero = { getRandomValues: <T extends ArrayBufferView | null>(array: T): T => array }
    expect(() => newCommandId(zero)).toThrow(ProtocolFault)
  })
})
