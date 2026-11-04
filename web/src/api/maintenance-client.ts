import { HttpApi, ApiError, NetworkFault, abortError, isAborted, pageHeaders, readJson } from './client'
import { adminEnvelope, type AdminDocument } from './admin-client'
import { decimal, errorCode, ProtocolFault, secret } from './protocol'
export const MAX_BACKUP_BYTES = 18 * 1024 ** 3 + 8 * 1024 ** 2 + 20
export const maintenancePhases = ['WAITING', 'READY_FOR_UPLOAD', 'UPLOADING', 'UPLOADED', 'WORKING', 'MAPPING_REQUIRED', 'REVIEW_READY', 'CONFIRMING', 'DRAINING', 'PREPARING_DATABASE', 'SEALING', 'COMMITTING', 'RETAINED', 'FINISHED', 'CANCELLED', 'FAILED'] as const
export type MaintenancePhase = typeof maintenancePhases[number]
export type MaintenanceKind = 'EXPORT' | 'IMPORT' | 'RESET'
export interface MaintenanceReview { schoolId: string; sourceRevision: string; currentRevision: string; documentCount: string; materialCount: number; profileMediaCount: number; payloadBytes: string; databaseBytes: string; materialBytes: string; profileMediaBytes: string }
export interface MaintenanceCatalog { schoolId: string; schoolName: string; classes: { id: string; name: string }[]; subjects: { id: string; name: string }[]; principals: { id: string; name: string }[] }
export interface MappingReview { source: MaintenanceCatalog; target: MaintenanceCatalog; currentRevision: string }
export interface MaintenanceSnapshot { id: string; kind: MaintenanceKind; phase: MaintenancePhase; canCancel: boolean; cancellationRequested: boolean; expectedRevision: string; fileName: string | null; bytes: string | null; sha256: string | null; resetReview: { presentTreeCount: number; scope: string[] } | null; review: MaintenanceReview | null; mapping: MappingReview | null; error: string | null; restartRequired: boolean }
export interface MaintenanceCredential { id: string; proof: string }
export interface MaintenanceMapping { expectedRevision: string; sourceSchoolId: string; targetSchoolId: string; classes: { source: string; target: string }[]; subjects: { source: string; target: string }[]; principals: { source: string; target: string }[] }
export type CreateMaintenance = { expectedRevision: string; kind: 'EXPORT' | 'RESET' } | { expectedRevision: string; kind: 'IMPORT'; mode: 'ORDINARY' | 'MAPPED'; fileName: string; bytes: string }
const credentialKey = 'future-classroom.maintenance-task.v1'
export function rememberMaintenance(value: MaintenanceCredential): void { try { sessionStorage.setItem(credentialKey, JSON.stringify({ schemaVersion: 1, id: decimal(value.id), proof: secret(value.proof) })) } catch { /* The current mounted page retains its job capability in memory. */ } }
export function loadMaintenance(): MaintenanceCredential | undefined { try { const value = JSON.parse(sessionStorage.getItem(credentialKey) ?? 'null') as unknown; obj(value); if (value.schemaVersion !== 1) return undefined; return { id: decimal(value.id), proof: secret(value.proof) } } catch { return undefined } }
export function forgetMaintenance(): void { try { sessionStorage.removeItem(credentialKey) } catch { /* Never touch normal account credentials here. */ } }
export const maintenanceTerminal = (phase: MaintenancePhase) => ['FINISHED', 'CANCELLED', 'FAILED'].includes(phase)
function obj(v: unknown): asserts v is Record<string, unknown> { if (typeof v !== 'object' || v === null || Array.isArray(v)) throw new ProtocolFault() }
function str(v: unknown, max = 4096): asserts v is string { if (typeof v !== 'string' || [...v].length > max) throw new ProtocolFault() }
function rev(v: unknown): asserts v is string { if (v !== '0') decimal(v) }
function count(v: unknown, max: number): asserts v is number { if (typeof v !== 'number' || !Number.isSafeInteger(v) || v < 0 || v > max) throw new ProtocolFault() }
function catalog(v: unknown): MaintenanceCatalog { obj(v); decimal(v.schoolId); str(v.schoolName, 128); for (const [key, max] of [['classes', 128], ['subjects', 256], ['principals', 512]] as const) { const items = v[key]; if (!Array.isArray(items) || items.length > max) throw new ProtocolFault(); const ids = new Set<string>(); items.forEach(item => { obj(item); const id = decimal(item.id); if (ids.has(id)) throw new ProtocolFault(); ids.add(id); str(item.name, 128) }) } return v as unknown as MaintenanceCatalog }
function snapshot(v: unknown): MaintenanceSnapshot {
  obj(v); decimal(v.id); if (!['EXPORT', 'IMPORT', 'RESET'].includes(String(v.kind)) || !maintenancePhases.includes(v.phase as MaintenancePhase)) throw new ProtocolFault(); rev(v.expectedRevision)
  for (const key of ['canCancel', 'cancellationRequested', 'restartRequired']) if (typeof v[key] !== 'boolean') throw new ProtocolFault()
  if (v.fileName !== null) { str(v.fileName, 256); if (/[\\/\u0000-\u001f]/.test(v.fileName)) throw new ProtocolFault() }
  if (v.bytes !== null) { rev(v.bytes); if (BigInt(v.bytes) > BigInt(MAX_BACKUP_BYTES)) throw new ProtocolFault() }
  if (v.sha256 !== null) { str(v.sha256, 64); if (!/^[0-9a-f]{64}$/.test(v.sha256)) throw new ProtocolFault() }
  if (v.error !== null) str(v.error)
  if (v.review !== null) { obj(v.review); decimal(v.review.schoolId); for (const key of ['sourceRevision', 'currentRevision', 'documentCount', 'payloadBytes', 'databaseBytes', 'materialBytes', 'profileMediaBytes']) rev(v.review[key]); count(v.review.materialCount, 1000000); count(v.review.profileMediaCount, 1000000) }
  if (v.mapping !== null) { obj(v.mapping); catalog(v.mapping.source); catalog(v.mapping.target); rev(v.mapping.currentRevision) }
  if (v.resetReview !== null) { obj(v.resetReview); count(v.resetReview.presentTreeCount, 1024); if (!Array.isArray(v.resetReview.scope) || v.resetReview.scope.length > 128) throw new ProtocolFault(); v.resetReview.scope.forEach(item => str(item)) }
  return v as unknown as MaintenanceSnapshot
}
export class MaintenanceHttpApi extends HttpApi {
  private async maintenanceRequest(path: string, method: 'GET' | 'POST', signal: AbortSignal, csrf?: string, credential?: MaintenanceCredential, input?: unknown, file?: File): Promise<unknown> {
    if (signal.aborted) throw abortError(); if (this.base.protocol !== 'https:') throw new ProtocolFault('维护操作需要可信 HTTPS 本机入口。')
    const owner = new AbortController(), abort = () => owner.abort(); signal.addEventListener('abort', abort, { once: true }); const timer = setTimeout(abort, file ? 30 * 60 * 1000 : 15000)
    try {
      const headers = new Headers({ Accept: 'application/json', ...pageHeaders() }); if (csrf) headers.set('X-FC-CSRF', secret(csrf)); if (credential) headers.set('X-FC-Maintenance-Proof', secret(credential.proof)); if (method === 'POST') headers.set('Content-Type', file ? 'application/octet-stream' : 'application/json')
      const response = await this.fetcher(new URL('admin/maintenance/jobs' + path, this.base), { method, credentials: 'same-origin', cache: 'no-store', redirect: 'error', referrerPolicy: 'same-origin', headers, signal: owner.signal, ...(method === 'GET' ? {} : { body: file ?? JSON.stringify(input) }) })
      if (response.status === 401 || response.status === 403) { await response.body?.cancel().catch(() => undefined); throw new ApiError(response.status, response.status === 401 ? 'UNAUTHORIZED' : 'FORBIDDEN') }
      const value = await readJson(response, owner.signal, 4 * 1024 * 1024); if (!response.ok) throw new ApiError(response.status, errorCode(value)); if (response.status !== 200) throw new ProtocolFault(); return value
    } catch (error) { if (signal.aborted) throw abortError(); if (error instanceof ApiError || error instanceof ProtocolFault) throw error; throw new NetworkFault() }
    finally { clearTimeout(timer); signal.removeEventListener('abort', abort) }
  }
  async createJob(input: CreateMaintenance, csrf: string, signal: AbortSignal): Promise<AdminDocument<{ job: MaintenanceSnapshot; proof: string }>> {
    rev(input.expectedRevision)
    if (input.kind === 'IMPORT') { if (!input.fileName || /[\\/:\u0000-\u001f]/.test(input.fileName) || new TextEncoder().encode(input.fileName).length > 256 || !/^[1-9][0-9]*$/.test(input.bytes) || BigInt(input.bytes) > BigInt(MAX_BACKUP_BYTES)) throw new ProtocolFault() }
    return adminEnvelope(await this.maintenanceRequest('', 'POST', signal, csrf, undefined, { schemaVersion: 1, ...input }), value => { obj(value); return { job: snapshot(value.job), proof: secret(value.proof) } })
  }
  async readStatus(credential: MaintenanceCredential, signal: AbortSignal): Promise<AdminDocument<{ job: MaintenanceSnapshot }>> { return this.readJob(await this.maintenanceRequest('/' + decimal(credential.id), 'GET', signal, undefined, credential), credential) }
  async upload(credential: MaintenanceCredential, file: File, csrf: string, signal: AbortSignal): Promise<AdminDocument<{ job: MaintenanceSnapshot }>> { if (!file.size || file.size > MAX_BACKUP_BYTES) throw new ProtocolFault(); return this.readJob(await this.maintenanceRequest('/' + decimal(credential.id) + '/upload', 'POST', signal, csrf, credential, undefined, file), credential) }
  async action(credential: MaintenanceCredential, action: 'preflight' | 'cancel' | 'mapping' | 'confirm', input: MaintenanceMapping | { expectedRevision: string; reviewedData: true; understoodReplacement: true } | undefined, csrf: string | undefined, signal: AbortSignal): Promise<AdminDocument<{ job: MaintenanceSnapshot }>> { return this.readJob(await this.maintenanceRequest('/' + decimal(credential.id) + '/' + action, 'POST', signal, csrf, credential, { schemaVersion: 1, ...input }), credential) }
  private readJob(value: unknown, credential: MaintenanceCredential): AdminDocument<{ job: MaintenanceSnapshot }> { return adminEnvelope(value, data => { obj(data); const job = snapshot(data.job); if (job.id !== credential.id) throw new ProtocolFault(); return { job } }) }
  fileRequest(credential: MaintenanceCredential): { url: string; headers: Record<string, string> } { return { url: new URL('admin/maintenance/jobs/' + decimal(credential.id) + '/file', this.base).href, headers: { ...pageHeaders(), 'X-FC-Maintenance-Proof': secret(credential.proof), Accept: 'application/octet-stream' } } }
}
