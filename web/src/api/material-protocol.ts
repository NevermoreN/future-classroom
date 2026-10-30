import { decimal, ProtocolFault, unicodeText } from './protocol'
import { civilTime } from './content-protocol'
export const MAX_FILE_BYTES = 50 * 1024 * 1024
export interface MaterialScope { ownerTeacherId: string; subjectId: string }
export interface MaterialScopeItem extends MaterialScope { ownerDisplayName: string; subjectName: string; classes: { classId: string; className: string }[]; classTotal: number; nextClassOffset: number | null }
export interface MaterialScopes { identityRevision: string; offset: number; total: number; nextOffset: number | null; items: MaterialScopeItem[] }
export const kinds = ['TXT', 'PDF', 'PNG', 'JPEG', 'BMP', 'PPTX', 'DOCX', 'XLSX'] as const
export interface MaterialRecord { materialId: string; folderId: string; displayName: string; resourceVersionId: string; kind: typeof kinds[number]; bytes: number; sha256: string; allowedClassIds: string[]; addedAt: string; addedSequence: string; status: 'PUBLISHED' | 'DELETED'; revision: string }
export interface MaterialCursor { folderId: string; folderRevision: string; offset: number }
export interface MaterialPage { scope: MaterialScope; folderId: string; folderRevision: string; offset: number; total: number; items: MaterialRecord[]; nextCursor: MaterialCursor | null }
export interface UploadReady { uploadId: string; expiresInSeconds: 300; maxChunkBytes: 65536 }
export interface MaterialCommitted { commandId: string; scope: MaterialScope; record: MaterialRecord }
export type MaterialOutcome = { commandId: string; state: 'COMMITTED'; scope: MaterialScope; record: MaterialRecord } | { commandId: string; state: 'NOT_FOUND'; scope: null; record: null }
const fail = (): never => { throw new ProtocolFault() }
function object(x: unknown, keys: readonly string[]): Record<string, unknown> { if (!x || typeof x !== 'object' || Array.isArray(x) || Object.keys(x).length !== keys.length || Object.keys(x).some(k => !keys.includes(k))) return fail(); return x as Record<string, unknown> }
function schema(x: Record<string, unknown>, status: string) { if (x.schemaVersion !== 1 || x.status !== status) fail() }
function count(x: unknown, max: number): number { if (typeof x !== 'number' || !Number.isSafeInteger(x) || x < 0 || x > max) return fail(); return x }
function name(x: unknown, max = 64): string { if (typeof x !== 'string' || !x.replace(/\p{White_Space}/gu, '') || [...x].length > max || /[\u0000-\u001f\u007f-\u009f]/u.test(x)) return fail(); return unicodeText(x) }
export function fileName(x: unknown): string {
  if (typeof x !== 'string' || x.length === 0 || [...x].length > 180 || new TextEncoder().encode(x).length > 720 || /[<>:"/\\|?*\u0000-\u001f\u007f-\u009f]/u.test(x) || /[ .]$/.test(x) || !/\.(?:txt|pdf|png|jpe?g|bmp|pptx|docx|xlsx)$/i.test(x)) return fail()
  const stem = x.split('.')[0]!.trimEnd().toUpperCase()
  if (/^(?:CON|PRN|AUX|NUL|(?:COM|LPT)[1-9¹²³])$/.test(stem)) return fail()
  return unicodeText(x)
}
export function fileLimit(name: string): number { fileName(name); return /\.(?:png|jpe?g|bmp)$/i.test(name) ? 10 * 1024 * 1024 : MAX_FILE_BYTES }
export function scope(x: unknown): MaterialScope { const v = object(x, ['ownerTeacherId', 'subjectId']); return { ownerTeacherId: decimal(v.ownerTeacherId), subjectId: decimal(v.subjectId) } }
export function scopeKey(s: MaterialScope): string { return s.ownerTeacherId + '/' + s.subjectId }
export function sameScope(a: MaterialScope, b: MaterialScope): boolean { return scopeKey(a) === scopeKey(b) }
export function scopes(x: unknown, offset: number): MaterialScopes {
  const v = object(x, ['schemaVersion', 'status', 'identityRevision', 'offset', 'total', 'nextOffset', 'items']); schema(v, 'MATERIAL_SCOPES')
  if (v.offset !== offset || !Array.isArray(v.items) || v.items.length > 20) return fail()
  const total = count(v.total, 4096); if (offset > total || offset + v.items.length > total) return fail()
  let prev: MaterialScope | undefined
  const items = v.items.map(raw => {
    const s = object(raw, ['ownerTeacherId', 'ownerDisplayName', 'subjectId', 'subjectName', 'classes', 'classTotal', 'nextClassOffset']); const target = { ownerTeacherId: decimal(s.ownerTeacherId), subjectId: decimal(s.subjectId) }
    if (prev && (BigInt(prev.ownerTeacherId) > BigInt(target.ownerTeacherId) || prev.ownerTeacherId === target.ownerTeacherId && BigInt(prev.subjectId) >= BigInt(target.subjectId))) fail(); prev = target
    const classTotal = count(s.classTotal, 128); if (!classTotal || !Array.isArray(s.classes) || s.classes.length !== Math.min(20, classTotal) || s.nextClassOffset !== (classTotal > 20 ? 20 : null)) return fail()
    let last = 0n
    const classes = s.classes.map(rawClass => { const c = object(rawClass, ['classId', 'className']), id = decimal(c.classId); if (BigInt(id) <= last) fail(); last = BigInt(id); return { classId: id, className: name(c.className, 128) } })
    return { ...target, ownerDisplayName: name(s.ownerDisplayName, 128), subjectName: name(s.subjectName), classes, classTotal, nextClassOffset: s.nextClassOffset as number | null }
  })
  const end = offset + items.length, expectedNext = end < total ? end : null
  if (v.nextOffset !== expectedNext || items.length === 0 && offset < total) fail()
  return { identityRevision: decimal(v.identityRevision), offset, total, nextOffset: expectedNext, items }
}
export function record(x: unknown): MaterialRecord {
  const v = object(x, ['materialId', 'folderId', 'displayName', 'resourceVersionId', 'kind', 'bytes', 'sha256', 'allowedClassIds', 'addedAt', 'addedSequence', 'status', 'revision'])
  const displayName = fileName(v.displayName), bytes = count(v.bytes, fileLimit(displayName)); if (!bytes || !kinds.includes(v.kind as typeof kinds[number]) || typeof v.sha256 !== 'string' || !/^[a-f0-9]{64}$/.test(v.sha256) || v.status !== 'PUBLISHED' && v.status !== 'DELETED') return fail()
  const ext = displayName.split('.').at(-1)!.toUpperCase().replace('JPG', 'JPEG'); if (v.kind !== ext) fail()
  if (!Array.isArray(v.allowedClassIds) || v.allowedClassIds.length < 1 || v.allowedClassIds.length > 128) return fail()
  const classes = v.allowedClassIds.map(decimal); if (new Set(classes).size !== classes.length) fail()
  return { materialId: decimal(v.materialId), folderId: decimal(v.folderId), displayName, resourceVersionId: decimal(v.resourceVersionId), kind: v.kind as MaterialRecord['kind'], bytes, sha256: v.sha256, allowedClassIds: classes, addedAt: civilTime(v.addedAt), addedSequence: decimal(v.addedSequence), status: v.status, revision: decimal(v.revision) }
}
export function cursor(x: unknown): MaterialCursor { const v = object(x, ['folderId', 'folderRevision', 'offset']); return { folderId: decimal(v.folderId), folderRevision: decimal(v.folderRevision), offset: count(v.offset, 2048) } }
export function page(x: unknown, target: MaterialScope, expected: MaterialCursor | null): MaterialPage {
  const v = object(x, ['schemaVersion', 'status', 'scope', 'folderId', 'folderRevision', 'offset', 'total', 'items', 'nextCursor']); schema(v, 'MATERIALS'); const s = scope(v.scope)
  if (!sameScope(s, target) || !Array.isArray(v.items) || v.items.length > 20) return fail()
  const folderId = decimal(v.folderId), folderRevision = decimal(v.folderRevision), offset = count(v.offset, 2048), total = count(v.total, 2048)
  if (expected ? folderId !== expected.folderId || folderRevision !== expected.folderRevision || offset !== expected.offset : offset !== 0) fail()
  const items = v.items.map(record); let last = 18446744073709551616n; const ids = new Set<string>()
  for (const r of items) { if (BigInt(r.revision) > BigInt(folderRevision) || BigInt(r.addedSequence) > BigInt(folderRevision) || r.folderId !== folderId || r.status !== 'PUBLISHED' || BigInt(r.addedSequence) >= last || ids.has(r.materialId)) fail(); last = BigInt(r.addedSequence); ids.add(r.materialId) }
  if (offset > total || offset + items.length > total || !items.length && offset < total) fail()
  const next = v.nextCursor === null ? null : cursor(v.nextCursor), end = offset + items.length
  if (end < total ? !next || next.folderId !== folderId || next.folderRevision !== folderRevision || next.offset !== end : next !== null) fail()
  return { scope: s, folderId, folderRevision, offset, total, items, nextCursor: next }
}
export function ready(x: unknown): UploadReady { const v = object(x, ['schemaVersion', 'status', 'uploadId', 'expiresInSeconds', 'maxChunkBytes']); schema(v, 'UPLOAD_READY'); if (v.expiresInSeconds !== 300 || v.maxChunkBytes !== 65536) fail(); return { uploadId: decimal(v.uploadId), expiresInSeconds: 300, maxChunkBytes: 65536 } }
export function uploaded(x: unknown, id: string, bytes: number): void { const v = object(x, ['schemaVersion', 'status', 'uploadId', 'bytes']); schema(v, 'UPLOADED'); if (v.uploadId !== id || v.bytes !== bytes) fail() }
export function committed(x: unknown, id: string, target: MaterialScope): MaterialCommitted { const v = object(x, ['schemaVersion', 'status', 'commandId', 'scope', 'record']); schema(v, 'COMMITTED'); const s = scope(v.scope); if (v.commandId !== id || !sameScope(s, target)) fail(); return { commandId: decimal(v.commandId), scope: s, record: record(v.record) } }
export function outcome(x: unknown, id: string): MaterialOutcome {
  const v = object(x, ['schemaVersion', 'status', 'commandId', 'state', 'scope', 'record']); schema(v, 'COMMAND_RESULT'); if (v.commandId !== id) fail()
  if (v.state === 'NOT_FOUND' && v.scope === null && v.record === null) return { commandId: id, state: 'NOT_FOUND', scope: null, record: null }
  if (v.state !== 'COMMITTED') return fail()
  return { commandId: id, state: 'COMMITTED', scope: scope(v.scope), record: record(v.record) }
}
export function folderPath(s: MaterialScope): string { return 'materials/folders/' + decimal(s.ownerTeacherId) + '/' + decimal(s.subjectId) }

export function cancelled(x: unknown, id: string): void { const v = object(x, ['schemaVersion', 'status', 'uploadId']); schema(v, 'UPLOAD_CANCELLED'); if (v.uploadId !== id) fail() }

export interface MaterialClasses { identityRevision: string; scope: MaterialScope; offset: number; total: number; nextOffset: number | null; items: { classId: string; className: string }[] }
export function classPage(x: unknown, target: MaterialScope, offset: number): MaterialClasses {
  const v = object(x, ['schemaVersion', 'status', 'identityRevision', 'scope', 'offset', 'total', 'nextOffset', 'items']); schema(v, 'MATERIAL_CLASSES')
  const s = scope(v.scope), total = count(v.total, 128)
  if (!sameScope(s, target) || v.offset !== offset || count(offset, 128) > total || !Array.isArray(v.items) || v.items.length !== Math.min(20, total-offset)) return fail()
  let last = 0n
  const items = v.items.map(raw => { const c = object(raw, ['classId', 'className']), id = decimal(c.classId); if (BigInt(id) <= last) fail(); last = BigInt(id); return { classId: id, className: name(c.className, 128) } })
  const nextOffset = offset + items.length < total ? offset + items.length : null; if (v.nextOffset !== nextOffset) fail()
  return { identityRevision: decimal(v.identityRevision), scope: s, offset, total, nextOffset, items }
}
