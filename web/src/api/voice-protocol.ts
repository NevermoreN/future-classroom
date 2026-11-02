import { commandIdentity, commandTicket, commandVersion, sameCommand, type CommandIdentity } from './command-protocol'
import { civilTime } from './content-protocol'
import { decimal, ProtocolFault, protocolVersions, unicodeText } from './protocol'

const fail = (): never => { throw new ProtocolFault() }
function object(value: unknown, keys: readonly string[]): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value) || Object.keys(value).length !== keys.length || Object.keys(value).some(k => !keys.includes(k))) return fail()
  return value as Record<string, unknown>
}
function envelope(value: unknown, status: string, keys: string[], version = 1) {
  const v = object(value, ['schemaVersion', 'status', ...keys])
  if (version === 2) commandVersion(v.schemaVersion); else protocolVersions(v.schemaVersion)
  if (v.status !== status) return fail()
  return v
}
function integer(value: unknown, min: number, max: number): number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < min || value > max) return fail()
  return value
}
function bool(value: unknown): boolean { return typeof value === 'boolean' ? value : fail() }
function label(value: unknown): string {
  if (typeof value !== 'string' || [...value].length > 128 || !value.replace(/\p{White_Space}/gu, '') || /[\u0000-\u001f\u007f-\u009f]/u.test(value)) return fail()
  return unicodeText(value)
}
export interface VoiceCapabilities {
  identityRevision: string; runtimeGeneration: string; serverNow: string
  currentClass: { classId: string; className: string } | null
  enabled: boolean; admission: 'OPEN' | 'CLOSED'; targetAuthorized: boolean
  deviceAvailability: 'UNVERIFIED' | 'AVAILABLE' | 'UNAVAILABLE'
  defaultRepeatTotal: number; minRepeatTotal: 1; maxRepeatTotal: number
  minDurationMillis: 500; maxDurationMillis: number; maxWavBytes: number
}
export function capabilities(value: unknown): VoiceCapabilities {
  const v = envelope(value, 'VOICE_CAPABILITIES', ['identityRevision', 'runtimeGeneration', 'serverNow', 'currentClass', 'enabled', 'admission', 'targetAuthorized', 'deviceAvailability', 'defaultRepeatTotal', 'minRepeatTotal', 'maxRepeatTotal', 'minDurationMillis', 'maxDurationMillis', 'sampleRate', 'channels', 'bitsPerSample', 'maxWavBytes', 'uploadTtlSeconds'])
  for (const [key, expected] of Object.entries({ minRepeatTotal: 1, minDurationMillis: 500, sampleRate: 16000, channels: 1, bitsPerSample: 16, uploadTtlSeconds: 600 })) if (v[key] !== expected) fail()
  const maxRepeatTotal = integer(v.maxRepeatTotal, 1, 10), maxDurationMillis = integer(v.maxDurationMillis, 500, 60000)
  const defaultRepeatTotal = integer(v.defaultRepeatTotal, 1, maxRepeatTotal), maxWavBytes = integer(v.maxWavBytes, 16044, 1920044)
  if (defaultRepeatTotal !== Math.min(3, maxRepeatTotal) || maxWavBytes !== 44 + maxDurationMillis * 32) fail()
  if (v.admission !== 'OPEN' && v.admission !== 'CLOSED' || !['UNVERIFIED', 'AVAILABLE', 'UNAVAILABLE'].includes(String(v.deviceAvailability))) return fail()
  let currentClass: VoiceCapabilities['currentClass'] = null
  if (v.currentClass !== null) { const c = object(v.currentClass, ['classId', 'className']); currentClass = { classId: decimal(c.classId), className: label(c.className) } }
  const targetAuthorized = bool(v.targetAuthorized)
  if (targetAuthorized && !currentClass) return fail()
  return { identityRevision: decimal(v.identityRevision), runtimeGeneration: decimal(v.runtimeGeneration), serverNow: civilTime(v.serverNow), currentClass, enabled: bool(v.enabled), admission: v.admission, targetAuthorized, deviceAvailability: v.deviceAvailability as VoiceCapabilities['deviceAvailability'], defaultRepeatTotal, minRepeatTotal: 1, maxRepeatTotal, minDurationMillis: 500, maxDurationMillis, maxWavBytes }
}
export interface VoiceUploaded { assetId: string; assetRevision: string; classId: string; bytes: number; durationMillis: number; expiresAt: string }
export function uploaded(value: unknown, classId: string, bytes: number): VoiceUploaded {
  const v = envelope(value, 'VOICE_UPLOADED', ['assetId', 'assetRevision', 'classId', 'bytes', 'durationMillis', 'expiresAt'])
  const size = integer(v.bytes, 16044, 1920044), duration = integer(v.durationMillis, 500, 60000)
  if (size !== bytes || (size - 44) % 2 || duration !== Math.floor((size - 44) / 32) || decimal(v.classId) !== classId) return fail()
  return { assetId: decimal(v.assetId), assetRevision: decimal(v.assetRevision), classId, bytes: size, durationMillis: duration, expiresAt: civilTime(v.expiresAt) }
}
export const voiceStates = ['QUEUED', 'STARTING', 'PLAYING', 'GAP', 'STOPPING', 'COMPLETED', 'CANCELED', 'FAILED', 'EXPIRED', 'INTERRUPTED'] as const
export type VoiceStatus = typeof voiceStates[number]
export const voiceFailures = ['DEVICE', 'SESSION', 'REVOKED', 'DISABLED', 'TIMEOUT', 'CLOCK_REGRESSION', 'STORAGE'] as const
export interface VoiceJob {
  jobId: string; commandId: string; commandTicket: string | null; classId: string; repeatTotal: number; completedRepeats: number; currentRepeat: number
  state: VoiceStatus; revision: string; failure: typeof voiceFailures[number] | null
}
export const terminal = (job: VoiceJob) => ['COMPLETED', 'CANCELED', 'FAILED', 'EXPIRED', 'INTERRUPTED'].includes(job.state)
export function job(value: unknown): VoiceJob {
  const v = object(value, ['jobId', 'commandId', 'commandTicket', 'classId', 'repeatTotal', 'completedRepeats', 'currentRepeat', 'state', 'revision', 'failure'])
  const jobId = decimal(v.jobId), commandId = decimal(v.commandId), repeatTotal = integer(v.repeatTotal, 1, 10)
  const completedRepeats = integer(v.completedRepeats, 0, repeatTotal), currentRepeat = integer(v.currentRepeat, 0, repeatTotal)
  if (jobId !== commandId || !voiceStates.includes(v.state as VoiceStatus) || v.failure !== null && !voiceFailures.includes(v.failure as typeof voiceFailures[number]) || currentRepeat < completedRepeats || v.state === 'COMPLETED' && completedRepeats !== repeatTotal || v.state === 'QUEUED' && (currentRepeat !== 0 || completedRepeats !== 0)) return fail()
  if (['STARTING', 'PLAYING'].includes(String(v.state)) && (currentRepeat !== completedRepeats + 1 || completedRepeats >= repeatTotal)
    || v.state === 'GAP' && (currentRepeat !== completedRepeats || completedRepeats < 1 || completedRepeats >= repeatTotal)) return fail()
  return { jobId, commandId, commandTicket: v.commandTicket === null ? null : commandTicket(v.commandTicket), classId: decimal(v.classId), repeatTotal, completedRepeats, currentRepeat, state: v.state as VoiceStatus, revision: decimal(v.revision), failure: v.failure as VoiceJob['failure'] }
}
export function currentJob(value: unknown, id: string, submitTicket: string | null): VoiceJob { const v = envelope(value, 'VOICE_JOB', ['job'], 2); const j = job(v.job); return j.jobId === decimal(id) && j.commandTicket === (submitTicket === null ? null : commandTicket(submitTicket)) ? j : fail() }
export function jobs(value: unknown): VoiceJob[] {
  const v = envelope(value, 'VOICE_JOBS', ['jobs'], 2)
  if (!Array.isArray(v.jobs) || v.jobs.length > 24) return fail()
  const result = v.jobs.map(job)
  if (new Set(result.map(j => j.jobId)).size !== result.length || result.filter(j => !terminal(j)).length > 4 || result.filter(terminal).length > 20) return fail()
  return result
}
export interface VoiceSubmit extends CommandIdentity { assetId: string; expectedAssetRevision: string; classId: string; repeatTotal: number }
export interface VoiceCancel extends CommandIdentity { jobCommandTicket: string | null; expectedRevision: string }
export function submitInput(input: VoiceSubmit) { return { schemaVersion: 2, ...commandIdentity(input), assetId: decimal(input.assetId), expectedAssetRevision: decimal(input.expectedAssetRevision), classId: decimal(input.classId), repeatTotal: integer(input.repeatTotal, 1, 10) } }
export function cancelInput(input: VoiceCancel) { return { schemaVersion: 2, ...commandIdentity(input), jobCommandTicket: input.jobCommandTicket === null ? null : commandTicket(input.jobCommandTicket), expectedRevision: decimal(input.expectedRevision) } }
export function committed(value: unknown, input: VoiceSubmit) {
  const v = envelope(value, 'COMMITTED', ['commandId', 'commandTicket', 'job'], 2), j = job(v.job)
  if (!sameCommand({ commandId: decimal(v.commandId), commandTicket: commandTicket(v.commandTicket) }, input) || j.jobId !== input.commandId || j.commandTicket !== input.commandTicket || j.classId !== input.classId || j.repeatTotal !== input.repeatTotal) return fail()
  return j
}
export function cancelled(value: unknown, id: string, input: VoiceCancel) {
  const v = envelope(value, 'CANCEL_REQUESTED', ['commandId', 'commandTicket', 'job'], 2), j = job(v.job)
  if (!sameCommand({ commandId: decimal(v.commandId), commandTicket: commandTicket(v.commandTicket) }, input) || j.jobId !== id || j.commandTicket !== input.jobCommandTicket || BigInt(j.revision) <= BigInt(input.expectedRevision) || j.state !== 'STOPPING' && !terminal(j)) return fail()
  return j
}
export type VoiceOutcome = (CommandIdentity & { state: 'COMMITTED'; job: VoiceJob }) | (CommandIdentity & { state: 'NOT_FOUND'; job: null })
export function outcome(value: unknown, original: CommandIdentity): VoiceOutcome {
  const v = envelope(value, 'COMMAND_RESULT', ['commandId', 'commandTicket', 'state', 'job'], 2)
  const pair = { commandId: decimal(v.commandId), commandTicket: commandTicket(v.commandTicket) }
  if (!sameCommand(pair, commandIdentity(original))) return fail()
  if (v.state === 'NOT_FOUND' && v.job === null) return { ...pair, state: 'NOT_FOUND', job: null }
  if (v.state !== 'COMMITTED') return fail()
  return { ...pair, state: 'COMMITTED', job: job(v.job) }
}
export function sameTarget(a: VoiceCapabilities, b: VoiceCapabilities) {
  return a.currentClass?.classId === b.currentClass?.classId && a.identityRevision === b.identityRevision && a.runtimeGeneration === b.runtimeGeneration && a.maxRepeatTotal === b.maxRepeatTotal && a.maxDurationMillis === b.maxDurationMillis
}
