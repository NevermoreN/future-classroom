import { describe, expect, it } from 'vitest'
import * as m from '../src/api/material-protocol'
import { ProtocolFault, ProtocolVersionFault, strictJson } from '../src/api/protocol'
const target = { ownerTeacherId: '18446744073709551615', subjectId: '9007199254740993' }
function row() { return { materialId: '18446744073709551614', folderId: '8', displayName: '课件.pdf', resourceVersionId: '18446744073709551613', kind: 'PDF', bytes: 17, sha256: 'a'.repeat(64), allowedClassIds: ['9007199254740993'], addedAt: '2026-10-30T10:00:00', addedSequence: '7', status: 'PUBLISHED', revision: '1' } }
function group() { return { ...target, ownerDisplayName: '老师', subjectName: '数学', classes: [{ classId: '9007199254740993', className: '一班' }], classTotal: 1, nextClassOffset: null } }
function scopes() { return { schemaVersion: 1, status: 'MATERIAL_SCOPES', identityRevision: '18446744073709551615', offset: 0, total: 1, nextOffset: null, items: [group()] } }
function page() { return { schemaVersion: 1, status: 'MATERIALS', scope: target, folderId: '8', folderRevision: '7', offset: 0, total: 1, items: [row()], nextCursor: null } }
describe('materials wire values and immutable directory snapshots', () => {
  it('requires refresh for typed new schema versions across every material response envelope', () => {
    const readers: ((version: unknown) => unknown)[] = [
      version => m.scopes({ ...scopes(), schemaVersion: version }, 0),
      version => m.page({ ...page(), schemaVersion: version }, target, null),
      version => m.ready({ schemaVersion: version, status: 'UPLOAD_READY', uploadId: '8', expiresInSeconds: 300, maxChunkBytes: 65536 }),
      version => m.uploaded({ schemaVersion: version, status: 'UPLOADED', uploadId: '8', bytes: 17 }, '8', 17),
      version => m.committed({ schemaVersion: version, status: 'COMMITTED', commandId: '7', scope: target, record: row() }, '7', target),
      version => m.outcome({ schemaVersion: version, status: 'COMMAND_RESULT', commandId: '7', state: 'NOT_FOUND', scope: null, record: null }, '7'),
      version => m.cancelled({ schemaVersion: version, status: 'UPLOAD_CANCELLED', uploadId: '8' }, '8'),
      version => m.classPage({ schemaVersion: version, status: 'MATERIAL_CLASSES', identityRevision: '1', scope: target, offset: 0, total: 1, nextOffset: null, items: [{ classId: '1', className: '班级' }] }, target, 0),
    ]
    for (const read of readers) {
      expect(() => read(1)).not.toThrow()
      expect(() => read(2)).toThrow(ProtocolVersionFault)
      for (const invalid of [undefined, '2', null, 0, -1, 1.5]) {
        expect(() => read(invalid)).toThrow(ProtocolFault)
        expect(() => read(invalid)).not.toThrow(ProtocolVersionFault)
      }
    }
    const missing = { ...page() } as Record<string, unknown>; delete missing.schemaVersion
    expect(() => m.page(missing, target, null)).toThrow(ProtocolFault)
    expect(() => m.page(missing, target, null)).not.toThrow(ProtocolVersionFault)
  })
  it('preserves full u64 identities and accepts grouped classes without Number conversion', () => {
    expect(m.scopes(scopes(), 0).items[0]).toEqual(group())
    expect(m.page(page(), target, null).items[0]?.materialId).toBe('18446744073709551614')
    expect(m.folderPath(target)).toBe('materials/folders/18446744073709551615/9007199254740993')
  })
  it.each(['0', '01', '18446744073709551616', 9007199254740992])('rejects invalid or numeric identity %s', id => {
    expect(() => m.scope({ ...target, ownerTeacherId: id })).toThrow(ProtocolFault)
  })
  it.each(['../课件.pdf', 'CON.txt', 'LPT¹.pdf', 'abc\u0000.txt', 'abc\u0085.txt', 'bad\n.txt', 'bad.txt ', 'bad.txt.', 'x.exe', '😀'.repeat(177) + '.txt'])('rejects unsafe file name %s', name => {
    expect(() => m.fileName(name)).toThrow(ProtocolFault)
  })
  it('keeps exact boundary file sizes and rejects zero, fractional and excessive bytes', () => {
    expect(m.fileLimit('图.JPG')).toBe(10 * 1024 * 1024)
    expect(m.fileLimit('课件.PDF')).toBe(50 * 1024 * 1024)
    for (const bytes of [0, -1, 0.5, 50 * 1024 * 1024 + 1]) expect(() => m.record({ ...row(), bytes })).toThrow(ProtocolFault)
    expect(m.record({ ...row(), bytes: 50 * 1024 * 1024 }).bytes).toBe(50 * 1024 * 1024)
    expect(() => m.record({ ...row(), displayName: '图.png', kind: 'PNG', bytes: 10 * 1024 * 1024 + 1 })).toThrow(ProtocolFault)
  })
  it('rejects duplicate/unsorted grouped scopes and classes, controls and empty classes', () => {
    for (const items of [[group(), group()], [{ ...group(), ownerTeacherId: '9' }, { ...group(), ownerTeacherId: '8' }], [{ ...group(), classes: [] }], [{ ...group(), classes: [{ classId: '2', className: '二班' }, { classId: '1', className: '一班' }] }], [{ ...group(), subjectName: '数学\u007f' }]]) expect(() => m.scopes({ ...scopes(), items, total: items.length }, 0)).toThrow(ProtocolFault)
  })
  it('accepts full valid owner/class names while keeping the narrower subject bound', () => {
    const full = { ...scopes(), items: [{ ...group(), ownerDisplayName: '师'.repeat(128), subjectName: '科'.repeat(64), classes: [{ classId: '1', className: '班'.repeat(128) }] }] }
    expect(m.scopes(full, 0).items[0]?.ownerDisplayName).toHaveLength(128)
    expect(() => m.scopes({ ...full, items: [{ ...full.items[0], subjectName: '科'.repeat(65) }] }, 0)).toThrow(ProtocolFault)
  })
  it('only acknowledges cancellation of the exact allocated upload ID', () => {
    expect(() => m.cancelled({ schemaVersion: 1, status: 'UPLOAD_CANCELLED', uploadId: '7' }, '7')).not.toThrow()
    expect(() => m.cancelled({ schemaVersion: 1, status: 'UPLOAD_CANCELLED', uploadId: '8' }, '7')).toThrow(ProtocolFault)
    expect(() => m.cancelled({ schemaVersion: 1, status: 'UPLOAD_CANCELLED', uploadId: '7', shellPath: 'C:/school' }, '7')).toThrow(ProtocolFault)
  })
  it('exposes classTotal and bounded class pages instead of silently losing the 128th class', () => {
    const classes = Array.from({ length: 20 }, (_, i) => ({ classId: String(i + 1), className: '班'.repeat(128) }))
    const first = m.scopes({ ...scopes(), items: [{ ...group(), classes, classTotal: 128, nextClassOffset: 20 }] }, 0)
    expect(first.items[0]?.classes).toHaveLength(20); expect(first.items[0]?.classTotal).toBe(128); expect(first.items[0]?.nextClassOffset).toBe(20)
    const last = { schemaVersion: 1, status: 'MATERIAL_CLASSES', identityRevision: '1', scope: target, offset: 120, total: 128, nextOffset: null, items: Array.from({ length: 8 }, (_, i) => ({ classId: String(121 + i), className: '班' + (121 + i) })) }
    expect(m.classPage(last, target, 120).items.at(-1)?.classId).toBe('128')
    for (const changed of [{ scope: { ...target, subjectId: '3' } }, { offset: 119 }, { nextOffset: 128 }, { items: last.items.slice(0, 7) }, { items: [last.items[1], last.items[0], ...last.items.slice(2)] }]) expect(() => m.classPage({ ...last, ...changed }, target, 120)).toThrow(ProtocolFault)
  })
  it('accepts an actual partial continuation offset rather than assuming twenty rows', () => {
    const first = { ...scopes(), total: 9, nextOffset: 1 }
    expect(m.scopes(first, 0).nextOffset).toBe(1)
    expect(() => m.scopes({ ...first, nextOffset: 20 }, 0)).toThrow(ProtocolFault)
    expect(() => m.scopes({ ...first, items: [] }, 0)).toThrow(ProtocolFault)
  })
  it('rejects wrong folder, deleted members, duplicate sequence, cursor revision and cursor offset', () => {
    const mutations = [{ ...page(), folderId: '0' }, { ...page(), folderRevision: '0' }, { ...page(), items: [{ ...row(), folderId: '9' }] }, { ...page(), items: [{ ...row(), status: 'DELETED' }] }, { ...page(), total: 2, items: [row(), { ...row(), materialId: '11' }] }, { ...page(), total: 2, nextCursor: { folderId: '8', folderRevision: '6', offset: 1 } }, { ...page(), total: 2, nextCursor: { folderId: '8', folderRevision: '7', offset: 20 } }]
    for (const wire of mutations) expect(() => m.page(wire, target, null)).toThrow(ProtocolFault)
    expect(() => m.page({ ...page(), offset: 1, total: 2 }, target, { folderId: '8', folderRevision: '6', offset: 1 })).toThrow(ProtocolFault)
  })
  it('rejects member versions and append sequence newer than their folder snapshot', () => {
    expect(() => m.page({ ...page(), items: [{ ...row(), revision: '8' }] }, target, null)).toThrow(ProtocolFault)
    expect(() => m.page({ ...page(), items: [{ ...row(), addedSequence: '8' }] }, target, null)).toThrow(ProtocolFault)
  })
  it('rejects mismatched extension/kind, malformed SHA, duplicate classes and unknown fields', () => {
    for (const changed of [{ kind: 'TXT' }, { sha256: 'A'.repeat(64) }, { allowedClassIds: ['1', '1'] }, { privatePath: 'C:\\school' }]) expect(() => m.record({ ...row(), ...changed })).toThrow(ProtocolFault)
    expect(() => strictJson('{"schemaVersion":1,"schemaVersion":1}')).toThrow(ProtocolFault)
  })
  it('validates upload receipt constants and original command identity', () => {
    const ready = { schemaVersion: 1, status: 'UPLOAD_READY', uploadId: '18446744073709551615', expiresInSeconds: 300, maxChunkBytes: 65536 }
    expect(m.ready(ready).uploadId).toBe('18446744073709551615')
    for (const changed of [{ expiresInSeconds: 301 }, { maxChunkBytes: 65537 }, { uploadId: '0' }]) expect(() => m.ready({ ...ready, ...changed })).toThrow(ProtocolFault)
    expect(() => m.uploaded({ schemaVersion: 1, status: 'UPLOADED', uploadId: '9', bytes: 17 }, '8', 17)).toThrow(ProtocolFault)
    expect(() => m.outcome({ schemaVersion: 1, status: 'COMMAND_RESULT', commandId: '9', state: 'NOT_FOUND', scope: null, record: null }, '8')).toThrow(ProtocolFault)
    expect(() => m.outcome({ schemaVersion: 1, status: 'COMMAND_RESULT', commandId: '8', state: 'NOT_FOUND', scope: target, record: row() }, '8')).toThrow(ProtocolFault)
  })
})
