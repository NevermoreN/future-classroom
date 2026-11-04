import { ApiError, NetworkFault, abortError, readJson } from './client'
import { adminEnvelope } from './admin-client'
import { decimal, errorCode, ProtocolFault, secret } from './protocol'
export const repairPhases = ['INITIALIZING', 'READY', 'REVIEW_READY', 'PREPARING', 'DRAINING', 'COMMITTING', 'RETAINED', 'FINISHED', 'CANCELLED', 'FAILED'] as const
export type RepairPhase = typeof repairPhases[number]
export interface RepairCredential { session: string; csrf: string }
export interface RepairStatus { sequence: string; phase: RepairPhase; scope: { presentTreeCount: number; scope: string[] } | null; canCancel: boolean; cancellationRequested: boolean; error: string | null; succeeded: boolean | null }
export interface RepairBoot { capability?: string; credential?: RepairCredential }
const storageKey = 'future-classroom.repair-page.v1'
let captured: RepairBoot | null | undefined
export function forgetRepair(): void { try { sessionStorage.removeItem(storageKey) } catch { /* No ordinary account data is changed. */ } }
export function repairBootstrap(): RepairBoot | undefined {
  if (captured !== undefined) return captured ?? undefined
  const match = /^#\/repair\/([0-9a-f]{64})$/.exec(window.location.hash)
  if (match) { window.history.replaceState(null, '', window.location.pathname + window.location.search); captured = { capability: match[1]! }; return captured }
  try { const value = JSON.parse(sessionStorage.getItem(storageKey) ?? 'null') as unknown; object(value); const credential = { session: secret(value.session), csrf: secret(value.csrf) }; captured = { credential }; return captured } catch { captured = null; return undefined }
}
function object(v: unknown): asserts v is Record<string, unknown> { if (typeof v !== 'object' || v === null || Array.isArray(v)) throw new ProtocolFault() }
function status(v: unknown): RepairStatus { object(v); if (v.sequence !== '0') decimal(v.sequence); if (!repairPhases.includes(v.phase as RepairPhase) || typeof v.canCancel !== 'boolean' || typeof v.cancellationRequested !== 'boolean' || v.error !== null && typeof v.error !== 'string' || v.succeeded !== null && typeof v.succeeded !== 'boolean') throw new ProtocolFault(); if (v.scope !== null) { object(v.scope); if (typeof v.scope.presentTreeCount !== 'number' || !Number.isSafeInteger(v.scope.presentTreeCount) || v.scope.presentTreeCount < 0 || v.scope.presentTreeCount > 1024 || !Array.isArray(v.scope.scope) || v.scope.scope.length > 128 || v.scope.scope.some(line => typeof line !== 'string' || line.length > 4096)) throw new ProtocolFault() } return v as unknown as RepairStatus }
export class RepairHttpApi {
  constructor(private readonly base: URL, private readonly fetcher: typeof fetch = globalThis.fetch.bind(globalThis)) {}
  private async request(path: string, credential: RepairCredential | undefined, input: unknown | undefined, signal: AbortSignal): Promise<unknown> {
    if (signal.aborted) throw abortError(); if (this.base.protocol !== 'https:') throw new ProtocolFault()
    const owner = new AbortController(), abort = () => owner.abort(); signal.addEventListener('abort', abort, { once: true }); const timer = setTimeout(abort, 15000)
    try { const headers = new Headers({ Accept: 'application/json' }); if (credential) headers.set('X-FC-Repair-Session', secret(credential.session)); if (input !== undefined) { headers.set('Content-Type', 'application/json'); if (credential) headers.set('X-FC-CSRF', secret(credential.csrf)) }
      const response = await this.fetcher(new URL('repair/' + path, this.base), { method: input === undefined ? 'GET' : 'POST', credentials: 'omit', cache: 'no-store', redirect: 'error', referrerPolicy: 'no-referrer', headers, signal: owner.signal, ...(input === undefined ? {} : { body: JSON.stringify(input) }) })
      if (response.status === 401 || response.status === 403) { await response.body?.cancel().catch(() => undefined); throw new ApiError(response.status, response.status === 401 ? 'UNAUTHORIZED' : 'FORBIDDEN') }
      const value = await readJson(response, owner.signal); if (!response.ok) throw new ApiError(response.status, errorCode(value)); if (response.status !== 200) throw new ProtocolFault(); return value
    } catch (error) { if (signal.aborted) throw abortError(); if (error instanceof ApiError || error instanceof ProtocolFault) throw error; throw new NetworkFault() } finally { clearTimeout(timer); signal.removeEventListener('abort', abort) }
  }
  async exchange(capability: string): Promise<RepairCredential> { const result = adminEnvelope(await this.request('session', undefined, { schemaVersion: 1, capability: secret(capability) }, new AbortController().signal), value => { object(value); return { session: secret(value.session), csrf: secret(value.csrf) } }); try { sessionStorage.setItem(storageKey, JSON.stringify(result.data)) } catch { /* Current-page credential remains in memory. */ } return result.data }
  async read(credential: RepairCredential, signal: AbortSignal): Promise<RepairStatus> { return adminEnvelope(await this.request('status', credential, undefined, signal), value => { object(value); return status(value.status) }).data }
  async act(credential: RepairCredential, action: 'review' | 'confirm' | 'cancel' | 'exit', sequence: string | undefined, signal: AbortSignal): Promise<RepairStatus> { const input = action === 'confirm' ? { schemaVersion: 1, sequence: decimal(sequence), reviewedScope: true, understoodDeletion: true, understoodNewIdentity: true } : { schemaVersion: 1 }; return adminEnvelope(await this.request(action, credential, input, signal), value => { object(value); return status(value.status) }).data }
}
let exchange: Promise<RepairCredential> | undefined
export function beginRepairSession(api: RepairHttpApi, boot: RepairBoot): Promise<RepairCredential> { if (boot.credential) return Promise.resolve(boot.credential); if (!exchange) { const capability = boot.capability; delete boot.capability; if (!capability) return Promise.reject(new ProtocolFault('一次性修复授权尚未确认，请重新从本机启动修复入口。')); exchange = api.exchange(capability) } return exchange }
