/** Wire values stay unknown until checked. Decimal IDs never pass through Number. */
export const MAX_JSON_BYTES = 65_536
export class ProtocolFault extends Error {
  constructor(message = '白板返回了不兼容或无效的数据，请联系管理员。') { super(message); this.name = 'ProtocolFault' }
}
export class ProtocolVersionFault extends ProtocolFault {
  constructor() { super('白板与页面版本不兼容，请刷新页面；如仍不兼容请联系管理员升级。'); this.name = 'ProtocolVersionFault' }
}
const fail = (): never => { throw new ProtocolFault() }
/** Validate every version's wire type before classifying an incompatible set.
 * A malformed version is never presented as a trustworthy upgrade signal. */
export function protocolVersions(first: unknown, ...others: unknown[]): void {
  const versions = [first, ...others]
  if (versions.some(value => typeof value !== 'number' || !Number.isSafeInteger(value) || value < 1)) fail()
  if (versions.some(value => value !== 1)) throw new ProtocolVersionFault()
}
type ObjectValue = Record<string, unknown>
function object(value: unknown, fields: readonly string[]): ObjectValue {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return fail()
  const keys = Object.keys(value)
  if (keys.length !== fields.length || keys.some(key => !fields.includes(key))) return fail()
  return value as ObjectValue
}
export function decimal(value: unknown): string {
  if (typeof value !== 'string' || !/^[1-9][0-9]{0,19}$/.test(value)
    || value.length === 20 && value > '18446744073709551615') return fail()
  return value
}
export function unicodeText(value: string): string { if (new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(new TextEncoder().encode(value)) !== value) return fail(); return value }
function text(value: unknown, max = 64): string {
  if (typeof value !== 'string' || !value.replace(/\p{White_Space}/gu, '') || [...value].length > max || /[\u0000-\u001f\u007f-\u009f]/u.test(value)) return fail()
  return unicodeText(value)
}
export function secret(value: unknown): string {
  if (typeof value !== 'string' || !/^[0-9a-f]{64}$/.test(value) || /^0+$/.test(value)) return fail()
  return value
}
export interface PublicStatus { online: boolean }
export function publicStatus(value: unknown): PublicStatus {
  const v = object(value, ['schemaVersion', 'apiVersion', 'uiCompatibilityVersion', 'online'])
  protocolVersions(v.schemaVersion, v.apiVersion, v.uiCompatibilityVersion)
  if (typeof v.online !== 'boolean') return fail()
  return { online: v.online }
}
export interface PairingReceipt { requestId: string; pollSecret: string; retryAfterMs: number }
export function pairingReceipt(value: unknown): PairingReceipt {
  const v = object(value, ['schemaVersion', 'status', 'requestId', 'pollSecret', 'retryAfterMs'])
  protocolVersions(v.schemaVersion)
  if (v.status !== 'PENDING' || v.retryAfterMs !== 500) return fail()
  return { requestId: decimal(v.requestId), pollSecret: secret(v.pollSecret), retryAfterMs: v.retryAfterMs }
}
export type PollResult = { status: 'PENDING'; retryAfterMs: number } | { status: 'AUTHORIZED' }
export function pollResult(value: unknown): PollResult {
  if (typeof value !== 'object' || value === null || !('status' in value)) return fail()
  if (value.status === 'AUTHORIZED') { fixedStatus(value, 'AUTHORIZED'); return { status: 'AUTHORIZED' } }
  const v = object(value, ['schemaVersion', 'status', 'retryAfterMs'])
  protocolVersions(v.schemaVersion)
  if (v.status !== 'PENDING' || v.retryAfterMs !== 500) return fail()
  return { status: 'PENDING', retryAfterMs: v.retryAfterMs }
}
export type Role = 'GLOBAL_ADMIN' | 'TEACHER_ADMIN' | 'USER'
export interface Session {
  principal: { id: string; displayName: string; role: Role }
  currentClassId: string | null
  assignments: { classId: string; subjectId: string }[]
  csrf: string
}
export function session(value: unknown): Session {
  const v = object(value, ['schemaVersion', 'status', 'principal', 'currentClassId', 'assignments', 'csrf'])
  protocolVersions(v.schemaVersion)
  if (v.status !== 'AUTHENTICATED') return fail()
  const p = object(v.principal, ['id', 'displayName', 'role'])
  if (p.role !== 'GLOBAL_ADMIN' && p.role !== 'TEACHER_ADMIN' && p.role !== 'USER') return fail()
  if (!Array.isArray(v.assignments) || v.assignments.length > 4096) return fail()
  const seen = new Set<string>()
  const assignments = v.assignments.map(item => {
    const a = object(item, ['classId', 'subjectId'])
    const classId = decimal(a.classId), subjectId = decimal(a.subjectId)
    const key = `${classId}:${subjectId}`
    if (seen.has(key)) return fail()
    seen.add(key)
    return { classId, subjectId }
  })
  return {
    principal: { id: decimal(p.id), displayName: text(p.displayName, 128), role: p.role },
    currentClassId: v.currentClassId === null ? null : decimal(v.currentClassId),
    assignments, csrf: secret(v.csrf),
  }
}
function fixedStatus(value: unknown, expected: string): void {
  const v = object(value, ['schemaVersion', 'status']); protocolVersions(v.schemaVersion)
  if (v.status !== expected) fail()
}
export function loggedOut(value: unknown): void { fixedStatus(value, 'LOGGED_OUT') }
export const errorCodes = ['INVALID_REQUEST', 'UNAUTHORIZED', 'FORBIDDEN', 'NOT_FOUND', 'METHOD_NOT_ALLOWED', 'RATE_LIMITED', 'SERVICE_UNAVAILABLE', 'RESULT_UNKNOWN', 'REVISION_CONFLICT', 'DAY_CHANGED', 'IDEMPOTENCY_CONFLICT', 'STALE_CURSOR', 'UPLOAD_EXPIRED', 'UPLOAD_NOT_READY', 'INVALID_RESOURCE', 'CAPACITY', 'VOICE_DISABLED', 'TARGET_CHANGED', 'INVALID_WAV', 'QUEUE_FULL', 'OWNER_LIMIT', 'EXPIRED'] as const
export type ErrorCode = typeof errorCodes[number]
export function errorCode(value: unknown): ErrorCode {
  const v = object(value, ['schemaVersion', 'code']); protocolVersions(v.schemaVersion)
  if (typeof v.code !== 'string' || !errorCodes.includes(v.code as ErrorCode)) return fail()
  return v.code as ErrorCode
}
export interface PairingForm { code: string; loginName: string; deviceName: string }
export function validateForm(form: PairingForm): void {
  if (typeof form.code !== 'string' || !/^[0-9]{8}$/.test(form.code)) fail()
  text(form.loginName); text(form.deviceName)
}

/** JSON.parse alone discards duplicate keys. Preflight decodes keys before
 * comparison, including escaped spellings; depth/items match the Rust budget. */
export function strictJson(source: string): unknown {
  let i = 0
  const whitespace = () => { while (i < source.length && /[\t\n\r ]/.test(source[i]!)) i++ }
  function string(): string {
    const start = i++
    let closed = false
    while (i < source.length) {
      const c = source[i++]!
      if (c === '"') { closed = true; break }
      if (c === '\\') i++
    }
    if (!closed) return fail()
    try { const value: unknown = JSON.parse(source.slice(start, i)); return typeof value === 'string' ? value : fail() } catch { return fail() }
  }
  function value(depth: number): void {
    if (depth > 32) fail()
    whitespace()
    const c = source[i]
    if (c === '"') { string(); return }
    if (c === '{' || c === '[') {
      const end = c === '{' ? '}' : ']'; i++; whitespace()
      const keys = new Set<string>(); let count = 0
      if (source[i] === end) { i++; return }
      while (true) {
        if (++count > 4096) fail()
        if (c === '{') {
          if (source[i] !== '"') fail()
          const key = string(); if (keys.has(key)) fail(); keys.add(key)
          whitespace(); if (source[i++] !== ':') fail()
        }
        value(depth + 1); whitespace()
        if (source[i] === end) { i++; return }
        if (source[i++] !== ',') fail()
        whitespace()
      }
    }
    const match = /^(?:null|true|false|-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?)/.exec(source.slice(i))
    if (!match) fail()
    i += match![0].length
  }
  value(0); whitespace(); if (i !== source.length) fail()
  try { return JSON.parse(source) as unknown } catch { return fail() }
}
