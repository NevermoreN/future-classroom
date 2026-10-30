import { decimal, ProtocolFault, unicodeText } from './protocol'
const fail = (): never => { throw new ProtocolFault() }
export type ContentKind = 'HOMEWORK' | 'REMINDER'
export type ContentKey = { kind: 'HOMEWORK'; classId: string; subjectId: string; ownerTeacherId: string; date: string } | { kind: 'REMINDER'; classId: string; date: string }
export type ScopeItem = { kind: 'HOMEWORK'; classId: string; className: string; subjectId: string; subjectName: string; ownerTeacherId: string; ownerDisplayName: string } | { kind: 'REMINDER'; classId: string; className: string }
export interface ScopePage { kind: ContentKind; today: string; identityRevision: string; offset: number; total: number; nextOffset: number | null; items: ScopeItem[] }
export interface ContentDocument { today: string; serverNow: string; expiresAt: string; key: ContentKey; revision: string; text: string; lastActorId: string | null; updatedAt: string | null }
export interface ContentCommand { commandId: string; expectedRevision: string; text: string }
export interface Committed { commandId: string; content: ContentDocument }
export type CommandResult = { commandId: string; state: 'COMMITTED'; content: ContentDocument } | { commandId: string; state: 'NOT_FOUND'; content: null }
function object(value: unknown, fields: readonly string[]): Record<string, unknown> {
  if (typeof value !== 'object' || value === null || Array.isArray(value) || Object.keys(value).length !== fields.length || Object.keys(value).some(k => !fields.includes(k))) return fail()
  return value as Record<string, unknown>
}
function schema(value: Record<string, unknown>, status: string) { if (value.schemaVersion !== 1 || value.status !== status) fail() }
export function revision(value: unknown): string { return value === '0' ? '0' : decimal(value) }
const monthLength = (y: number, m: number) => m === 2 ? (y % 4 === 0 && (y % 100 !== 0 || y % 400 === 0) ? 29 : 28) : [4, 6, 9, 11].includes(m) ? 30 : 31
export function civilDate(value: unknown): string {
  if (typeof value !== 'string' || !/^\d{4}-\d{2}-\d{2}$/.test(value)) return fail()
  const [y, m, d] = value.split('-').map(Number)
  if (!y || y > 9999 || !m || m > 12 || !d || d > monthLength(y, m)) return fail()
  return value
}
export function civilTime(value: unknown): string {
  if (typeof value !== 'string' || !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}$/.test(value)) return fail()
  civilDate(value.slice(0, 10))
  const [h, m, s] = value.slice(11).split(':').map(Number)
  if (h! > 23 || m! > 59 || s! > 59) return fail()
  return value
}
export function nextDay(date: string): string {
  civilDate(date); let [year, month, day] = date.split('-').map(Number) as [number, number, number]
  if (++day > monthLength(year, month)) { day = 1; if (++month > 12) { month = 1; year++ } }
  if (year > 9999) return fail()
  return `${String(year).padStart(4, '0')}-${String(month).padStart(2, '0')}-${String(day).padStart(2, '0')}`
}
export function bodyText(value: unknown): string {
  if (typeof value !== 'string' || /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f-\u009f]/u.test(value)) return fail()
  const encoded = new TextEncoder().encode(value)
  if (encoded.byteLength > 16_384 || new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(encoded) !== value) return fail()
  return value
}
export function normalizedBodyText(raw: string): string { return bodyText(raw).replace(/^\p{White_Space}+|\p{White_Space}+$/gu, '') }
function name(value: unknown, max = 64): string {
  if (typeof value !== 'string' || !value.replace(/\p{White_Space}/gu, '') || [...value].length > max || /[\u0000-\u001f\u007f-\u009f]/u.test(value)) return fail()
  return unicodeText(value)
}
export function key(value: unknown): ContentKey {
  if (typeof value !== 'object' || value === null || !('kind' in value)) return fail()
  if (value.kind === 'HOMEWORK') {
    const v = object(value, ['kind', 'classId', 'subjectId', 'ownerTeacherId', 'date'])
    return { kind: 'HOMEWORK', classId: decimal(v.classId), subjectId: decimal(v.subjectId), ownerTeacherId: decimal(v.ownerTeacherId), date: civilDate(v.date) }
  }
  const v = object(value, ['kind', 'classId', 'date']); if (v.kind !== 'REMINDER') return fail()
  return { kind: 'REMINDER', classId: decimal(v.classId), date: civilDate(v.date) }
}
export function sameKey(a: ContentKey, b: ContentKey): boolean {
  return a.kind === b.kind && a.classId === b.classId && a.date === b.date && (a.kind === 'REMINDER' || b.kind === 'HOMEWORK' && a.subjectId === b.subjectId && a.ownerTeacherId === b.ownerTeacherId)
}
export function scopeKey(item: ScopeItem): string { return item.kind === 'HOMEWORK' ? `${item.classId}/${item.subjectId}/${item.ownerTeacherId}` : item.classId }
export function compareScopes(a: ScopeItem, b: ScopeItem): number {
  if (a.kind !== b.kind) throw new ProtocolFault()
  const left = scopeKey(a).split('/'), right = scopeKey(b).split('/')
  for (let i = 0; i < left.length; i++) { if (BigInt(left[i]!) < BigInt(right[i]!)) return -1; if (BigInt(left[i]!) > BigInt(right[i]!)) return 1 }
  return 0
}
export function keyFor(item: ScopeItem, date: string): ContentKey { return item.kind === 'HOMEWORK' ? { kind: item.kind, classId: item.classId, subjectId: item.subjectId, ownerTeacherId: item.ownerTeacherId, date: civilDate(date) } : { kind: item.kind, classId: item.classId, date: civilDate(date) } }
export function contentPath(target: ContentKey): string {
  const k = key(target)
  return k.kind === 'HOMEWORK' ? `content/homework/${k.classId}/${k.subjectId}/${k.ownerTeacherId}/${k.date}` : `content/reminders/${k.classId}/${k.date}`
}
export function scopes(value: unknown, expectedKind: ContentKind, expectedOffset: number): ScopePage {
  const v = object(value, ['schemaVersion', 'status', 'kind', 'today', 'identityRevision', 'offset', 'total', 'nextOffset', 'items']); schema(v, 'SCOPES')
  const max = expectedKind === 'HOMEWORK' ? 4096 : 128
  if (v.kind !== expectedKind || v.offset !== expectedOffset || !Number.isSafeInteger(v.offset) || expectedOffset < 0 || expectedOffset > 4096 || !Number.isSafeInteger(v.total) || (v.total as number) < expectedOffset || (v.total as number) > max || !Array.isArray(v.items) || v.items.length > 20 || expectedOffset + v.items.length > (v.total as number) || v.items.length === 0 && expectedOffset < (v.total as number)) return fail()
  const end = expectedOffset + v.items.length
  if (v.nextOffset !== (end < (v.total as number) ? end : null)) return fail()
  const seen = new Set<string>(); let previous: string[] | undefined
  const items: ScopeItem[] = v.items.map(item => {
    const r = object(item, expectedKind === 'HOMEWORK' ? ['classId', 'className', 'subjectId', 'subjectName', 'ownerTeacherId', 'ownerDisplayName'] : ['classId', 'className'])
    const normalized: ScopeItem = expectedKind === 'HOMEWORK' ? { kind: expectedKind, classId: decimal(r.classId), className: name(r.className, 128), subjectId: decimal(r.subjectId), subjectName: name(r.subjectName), ownerTeacherId: decimal(r.ownerTeacherId), ownerDisplayName: name(r.ownerDisplayName, 128) } : { kind: expectedKind, classId: decimal(r.classId), className: name(r.className, 128) }
    const ids = scopeKey(normalized).split('/'), id = ids.join('/')
    if (seen.has(id)) return fail()
    if (previous) { let comparison = 0; for (let i = 0; i < ids.length; i++) { comparison = BigInt(ids[i]!) < BigInt(previous[i]!) ? -1 : BigInt(ids[i]!) > BigInt(previous[i]!) ? 1 : 0; if (comparison) break } if (comparison <= 0) return fail() }
    previous = ids; seen.add(id); return normalized
  })
  return { kind: expectedKind, today: civilDate(v.today), identityRevision: revision(v.identityRevision), offset: expectedOffset, total: v.total as number, nextOffset: v.nextOffset as number | null, items }
}
export function content(value: unknown): ContentDocument {
  const v = object(value, ['schemaVersion', 'status', 'today', 'serverNow', 'expiresAt', 'key', 'revision', 'text', 'lastActorId', 'updatedAt']); schema(v, 'CONTENT')
  const target = key(v.key), today = civilDate(v.today), serverNow = civilTime(v.serverNow), expiresAt = civilTime(v.expiresAt), rev = revision(v.revision), text = bodyText(v.text)
  if (target.date !== today || serverNow.slice(0, 10) !== today || expiresAt !== `${nextDay(target.date)}T00:00:00`) return fail()
  if (rev === '0') { if (text !== '' || v.lastActorId !== null || v.updatedAt !== null) return fail() }
  else if (v.lastActorId === null || v.updatedAt === null) return fail()
  const lastActorId = v.lastActorId === null ? null : decimal(v.lastActorId), updatedAt = v.updatedAt === null ? null : civilTime(v.updatedAt)
  if (updatedAt && (updatedAt.slice(0, 10) !== today)) return fail()
  return { key: target, today, serverNow, expiresAt, revision: rev, text, lastActorId, updatedAt }
}
export function committed(value: unknown, originalId: string, originalKey: ContentKey, originalRevision: string, originalText: string): Committed {
  const v = object(value, ['schemaVersion', 'status', 'commandId', 'content']); schema(v, 'COMMITTED')
  const result = content(v.content)
  if (decimal(v.commandId) !== originalId || !sameKey(result.key, originalKey) || BigInt(result.revision) !== BigInt(revision(originalRevision)) + 1n || result.text !== normalizedBodyText(originalText)) return fail()
  return { commandId: originalId, content: result }
}
export function commandResult(value: unknown, originalId: string): CommandResult {
  const v = object(value, ['schemaVersion', 'status', 'commandId', 'state', 'content']); schema(v, 'COMMAND_RESULT')
  if (decimal(v.commandId) !== originalId) return fail()
  if (v.state === 'NOT_FOUND' && v.content === null) return { commandId: originalId, state: 'NOT_FOUND', content: null }
  if (v.state !== 'COMMITTED' || v.content === null) return fail()
  return { commandId: originalId, state: 'COMMITTED', content: content(v.content) }
}
export function newCommandId(random: Pick<Crypto, 'getRandomValues'> = globalThis.crypto): string {
  for (let attempt = 0; attempt < 16; attempt++) { const words = random.getRandomValues(new Uint32Array(2)); const id = BigInt(words[0]!) << 32n | BigInt(words[1]!); if (id) return id.toString() }
  throw new ProtocolFault('无法生成安全的命令编号，请刷新页面后重试。')
}
/** Uses only the already validated server civil date/time, never the teacher's
 * wall clock/timezone. The midnight timer subsequently uses monotonic time. */
export function remainingToday(document: ContentDocument): number {
  const parts = document.serverNow.slice(11).split(':').map(Number)
  return (86_400 - (parts[0]! * 3600 + parts[1]! * 60 + parts[2]!)) * 1000
}
