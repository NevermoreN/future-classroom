import { identity, ticket } from './command-fixtures'
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
      version => m.committed({ schemaVersion: version, status: 'COMMITTED', commandId: '7', commandTicket: ticket, scope: target, record: row() }, identity('7'), target),
      version => m.outcome({ schemaVersion: version, status: 'COMMAND_RESULT', commandId: '7', commandTicket: ticket, state: 'NOT_FOUND', scope: null, record: null }, identity('7')),
      version => m.cancelled({ schemaVersion: version, status: 'UPLOAD_CANCELLED', uploadId: '8' }, '8'),
      version => m.classPage({ schemaVersion: version, status: 'MATERIAL_CLASSES', identityRevision: '1', scope: target, offset: 0, total: 1, nextOffset: null, items: [{ classId: '1', className: '班级' }] }, target, 0),
    ]
    for (const [index, read] of readers.entries()) {
      const expected = [4, 5].includes(index) ? 2 : 1
      expect(() => read(expected)).not.toThrow()
      expect(() => read(expected === 1 ? 2 : 1)).toThrow(ProtocolVersionFault)
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
  it('rejects member versions above folder revision and sequence above its batch bound', () => {
    expect(() => m.page({ ...page(), items: [{ ...row(), revision: '8' }] }, target, null)).toThrow(ProtocolFault)
    expect(() => m.page({ ...page(), items: [{ ...row(), addedSequence: '141' }] }, target, null)).toThrow(ProtocolFault)
  })
  it('accepts twenty ordered members from one folder revision and keeps exact continuation snapshots', () => {
    const items = Array.from({ length: 20 }, (_, i) => ({ ...row(), materialId: String(20 - i), resourceVersionId: String(120 - i), addedSequence: String(20 - i) }))
    const wire = { ...page(), folderRevision: '1', total: 20, items }
    expect(m.page(wire, target, null).items).toEqual(items)
    const first = { ...wire, items: items.slice(0, 5), nextCursor: { folderId: '8', folderRevision: '1', offset: 5 } }
    const parsed = m.page(first, target, null)
    expect(parsed.nextCursor).toEqual(first.nextCursor)
    const next = { ...wire, offset: 5, items: items.slice(5, 10), nextCursor: { folderId: '8', folderRevision: '1', offset: 10 } }
    expect(m.page(next, target, parsed.nextCursor).items).toEqual(items.slice(5, 10))
    expect(() => m.page(next, target, { ...first.nextCursor, folderRevision: '2' })).toThrow(ProtocolFault)
  })
  it('rejects batch sequence21 at revision1 while retaining member revision, unique, order and zero checks', () => {
    const wire = { ...page(), folderRevision: '1', items: [{ ...row(), addedSequence: '20' }] }
    for (const changed of [
      { items: [{ ...wire.items[0], addedSequence: '21' }] },
      { items: [{ ...wire.items[0], addedSequence: '0' }] },
      { folderRevision: '0' },
      { items: [{ ...wire.items[0], revision: '2' }] },
      { total: 2, items: [wire.items[0], { ...wire.items[0], addedSequence: '19' }] },
      { total: 2, items: [{ ...wire.items[0], addedSequence: '19' }, { ...wire.items[0], materialId: '12' }] },
    ]) expect(() => m.page({ ...wire, ...changed }, target, null)).toThrow(ProtocolFault)
  })
  it('compares the twenty-per-revision bound exactly at u64 limits without Number rounding or wrap', () => {
    const max = '18446744073709551615'
    const wire = { ...page(), folderRevision: '922337203685477580', items: [{ ...row(), addedSequence: '18446744073709551600' }] }
    expect(m.page(wire, target, null).items[0]?.addedSequence).toBe('18446744073709551600')
    expect(() => m.page({ ...wire, items: [{ ...row(), addedSequence: '18446744073709551601' }] }, target, null)).toThrow(ProtocolFault)
    for (const revision of ['922337203685477581', max]) {
      expect(m.page({ ...wire, folderRevision: revision, items: [{ ...row(), addedSequence: max }] }, target, null).items[0]?.addedSequence).toBe(max)
    }
    expect(() => m.page({ ...wire, folderRevision: max, items: [{ ...row(), addedSequence: '18446744073709551616' }] }, target, null)).toThrow(ProtocolFault)
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
    expect(() => m.outcome({ schemaVersion: 2, status: 'COMMAND_RESULT', commandId: '9', commandTicket: ticket, state: 'NOT_FOUND', scope: null, record: null }, identity('8'))).toThrow(ProtocolFault)
    expect(() => m.outcome({ schemaVersion: 2, status: 'COMMAND_RESULT', commandId: '8', commandTicket: ticket, state: 'NOT_FOUND', scope: target, record: row() }, identity('8'))).toThrow(ProtocolFault)
  })
})

describe('strict ordered material batch envelopes', () => {
  const files = [{ displayName: 'first.pdf', bytes: 17 }, { displayName: 'last.pdf', bytes: 17 }]
  const ready = () => ({ schemaVersion: 1, status: 'MATERIAL_BATCH_READY', batchId: '18446744073709551615', expiresInSeconds: 300, maxChunkBytes: 65536, items: files.map((f, i) => ({ uploadId: String(100 + i), ...f })) })
  const records = () => files.map((f, i) => ({ ...row(), materialId: String(200 + i), resourceVersionId: String(100 + i), displayName: f.displayName, bytes: f.bytes, addedSequence: String(10 + i) }))
  it('binds READY to all ordered names/sizes with distinct canonical member identities', () => {
    expect(m.batchReady(ready(), files).items.map(i => i.uploadId)).toEqual(['100', '101'])
    for (const changed of [{ items: ready().items.slice(0, 1) }, { items: ready().items.slice().reverse() }, { items: ready().items.map(i => ({ ...i, uploadId: '100' })) }, { extra: true }, { expiresInSeconds: 301 }, { batchId: '0' }]) expect(() => m.batchReady({ ...ready(), ...changed }, files)).toThrow(ProtocolFault)
    for (const input of [[], Array.from({ length: 21 }, () => files[0]!), [{ displayName: 'x.png', bytes: 10 * 1024 * 1024 + 1 }]]) expect(() => m.batchFiles(input)).toThrow(ProtocolFault)
    expect(() => m.batchReady({ ...ready(), schemaVersion: 2 }, files)).toThrow(ProtocolVersionFault)
  })
  it('checks member/cancellation acknowledgements against both original group and member', () => {
    const ack = { schemaVersion: 1, status: 'MATERIAL_BATCH_MEMBER_UPLOADED', batchId: '8', uploadId: '9', bytes: 17 }
    expect(() => m.batchUploaded(ack, '8', '9', 17)).not.toThrow()
    for (const changed of [{ batchId: '7' }, { uploadId: '10' }, { bytes: 18 }, { extra: true }]) expect(() => m.batchUploaded({ ...ack, ...changed }, '8', '9', 17)).toThrow(ProtocolFault)
    expect(() => m.batchCancelled({ schemaVersion: 1, status: 'MATERIAL_BATCH_CANCELLED', batchId: '8' }, '8')).not.toThrow()
    expect(() => m.batchCancelled({ schemaVersion: 1, status: 'MATERIAL_BATCH_CANCELLED', batchId: '7' }, '8')).toThrow(ProtocolFault)
  })
  it('requires a complete ordered initial publication under the original scope and full command pair', () => {
    const wire = { schemaVersion: 2, status: 'BATCH_COMMITTED', ...identity(), scope: target, records: records() }
    expect(m.batchCommitted(wire, identity(), target).records).toHaveLength(2)
    for (const changed of [{ records: records().reverse() }, { records: [records()[0], records()[0]] }, { records: [] }, { records: records().map(r => ({ ...r, resourceVersionId: '100' })) }, { records: records().map(r => ({ ...r, status: 'DELETED' })) }, { records: [{ ...records()[0], revision: '2' }] }, { commandTicket: ticket.replace('12', '34') }, { scope: { ...target, subjectId: '1' } }, { extra: true }]) expect(() => m.batchCommitted({ ...wire, ...changed }, identity(), target)).toThrow(ProtocolFault)
    const unknown = { schemaVersion: 2, status: 'COMMAND_BATCH_RESULT', ...identity(), state: 'NOT_FOUND', scope: null, records: null }
    expect(m.batchOutcome(unknown, identity(), target).state).toBe('NOT_FOUND')
    expect(() => m.batchOutcome({ ...unknown, records: [] }, identity(), target)).toThrow(ProtocolFault)
    expect(() => m.batchOutcome({ ...unknown, commandId: '999' }, identity(), target)).toThrow(ProtocolFault)
    expect(m.batchOutcome({ ...wire, status: 'COMMAND_BATCH_RESULT', state: 'COMMITTED' }, identity(), target).state).toBe('COMMITTED')
  })
})

describe('independent batch-delete wire contract', () => {
  const deleted = () => [{ ...row(), status: 'DELETED', revision: '2' }, { ...row(), materialId: '9', resourceVersionId: '10', addedSequence: '3', addedAt: '2026-10-29T09:00:00', allowedClassIds: ['2'], status: 'DELETED', revision: '5' }]
  const committed = () => ({ schemaVersion: 2, status: 'BATCH_DELETE_COMMITTED', ...identity(), scope: target, records: deleted() })
  it('accepts existing records in selection order without requiring shared time, classes, revision or consecutive sequence', () => {
    expect(m.batchDeleteCommitted(committed(), identity(), target).records.map(r => r.materialId)).toEqual([row().materialId, '9'])
    const history = { ...committed(), status: 'COMMAND_BATCH_DELETE_RESULT', state: 'COMMITTED' }
    expect(m.batchDeleteOutcome(history, identity(), target).state).toBe('COMMITTED')
    expect(() => m.batchRecords(deleted())).toThrow(ProtocolFault)
    expect(() => m.batchCommitted(committed(), identity(), target)).toThrow(ProtocolFault)
  })
  it('validates bounded fixed targets and rejects duplicate IDs, versions, unknown fields and unsafe numbers', () => {
    const first = { materialId: '1', resourceVersionId: '2', expectedMaterialRevision: '3' }
    expect(m.deleteTargets(Array.from({ length: 20 }, (_, i) => ({ ...first, materialId: String(i + 1), resourceVersionId: String(i + 101) })))).toHaveLength(20)
    for (const targets of [[], Array(21).fill(first), [first, first], [first, { ...first, materialId: '3' }], [{ ...first, extra: true }], [{ ...first, expectedMaterialRevision: '0' }], [{ ...first, resourceVersionId: 2 }]]) expect(() => m.deleteTargets(targets)).toThrow(ProtocolFault)
  })
  it('rejects wrong envelope, pair, scope, count, published state, duplicate resources and initial revision', () => {
    for (const patch of [{ status: 'BATCH_COMMITTED' }, { commandId: '999' }, { scope: { ...target, subjectId: '1' } }, { records: [] }, { records: Array(21).fill(deleted()[0]) }, { records: [deleted()[0], deleted()[0]] }, { records: [{ ...deleted()[0], status: 'PUBLISHED' }] }, { records: [{ ...deleted()[0], revision: '1' }] }, { records: [{ ...deleted()[0], folderId: '9' }, deleted()[1]] }, { records: [deleted()[0], { ...deleted()[1], resourceVersionId: deleted()[0]!.resourceVersionId }] }, { extra: true }]) expect(() => m.batchDeleteCommitted({ ...committed(), ...patch }, identity(), target)).toThrow(ProtocolFault)
    expect(() => m.batchDeleteCommitted({ ...committed(), schemaVersion: 1 }, identity(), target)).toThrow(ProtocolVersionFault)
  })
  it('accepts NOT_FOUND only with null scope and records under the original pair', () => {
    const wire = { schemaVersion: 2, status: 'COMMAND_BATCH_DELETE_RESULT', ...identity(), state: 'NOT_FOUND', scope: null, records: null }
    expect(m.batchDeleteOutcome(wire, identity(), target).state).toBe('NOT_FOUND')
    for (const patch of [{ records: [] }, { scope: target }, { commandId: '999' }, { status: 'COMMAND_BATCH_RESULT' }]) expect(() => m.batchDeleteOutcome({ ...wire, ...patch }, identity(), target)).toThrow(ProtocolFault)
  })
})
