import { describe, expect, it } from 'vitest'
import { readJson } from '../src/api/client'
import { ProtocolFault, session } from '../src/api/protocol'
import { scopes } from '../src/api/content-protocol'
import { fileName } from '../src/api/material-protocol'
describe('Rust and browser Unicode boundary compatibility', () => {
  it('rejects a transport BOM instead of silently stripping bytes Rust JSON rejects', async () => {
    const response = new Response(new Uint8Array([0xef, 0xbb, 0xbf, ...new TextEncoder().encode('{"schemaVersion":1}')]), { headers: { 'Content-Type': 'application/json' } })
    await expect(readJson(response, new AbortController().signal)).rejects.toBeInstanceOf(ProtocolFault)
  })
  it('accepts a valid 128 scalar display name and rejects malformed UTF16 or overlimit names', () => {
    const input = { schemaVersion: 1, status: 'AUTHENTICATED', principal: { id: '1', displayName: '师'.repeat(128), role: 'TEACHER_ADMIN' }, currentClassId: null, assignments: [], csrf: 'a'.repeat(64) }
    expect(session(input).principal.displayName).toHaveLength(128)
    expect(() => session({ ...input, principal: { ...input.principal, displayName: '师'.repeat(129) } })).toThrow(ProtocolFault)
    expect(() => session({ ...input, principal: { ...input.principal, displayName: '\ud800' } })).toThrow(ProtocolFault)
    expect(session({ ...input, principal: { ...input.principal, displayName: '\ufeff' } }).principal.displayName).toBe('\ufeff')
  })
  it('preserves legitimate 128 scalar class/owner names in the existing homework API', () => {
    const input = { schemaVersion: 1, status: 'SCOPES', kind: 'HOMEWORK', today: '2026-10-30', identityRevision: '1', offset: 0, total: 1, nextOffset: null, items: [{ classId: '1', className: '班'.repeat(128), ownerTeacherId: '2', ownerDisplayName: '师'.repeat(128), subjectId: '3', subjectName: '数学' }] }
    expect(scopes(input, 'HOMEWORK', 0).items[0]!.className).toHaveLength(128)
    expect(() => scopes({ ...input, items: [{ ...input.items[0], subjectName: '科'.repeat(65) }] }, 'HOMEWORK', 0)).toThrow(ProtocolFault)
  })
  it('never converts a malformed filename to replacement characters during transport', () => {
    expect(() => fileName('\ud800.txt')).toThrow(ProtocolFault)
    expect(fileName('合法😀.txt')).toBe('合法😀.txt')
  })
})
