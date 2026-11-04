import { HttpApi } from './client'
import { adminEnvelope } from './admin-client'
import { ProtocolFault, secret } from './protocol'
function object(v: unknown): asserts v is Record<string, unknown> { if (!v || typeof v !== 'object' || Array.isArray(v)) throw new ProtocolFault() }
export function recoveryCode(v: unknown): string { if (typeof v !== 'string' || !/^FC1-[0-9a-f]{64}$/.test(v)) throw new ProtocolFault(); return v }
export const localAccountControl = () => ['127.0.0.1', '[::1]', 'localhost'].includes(window.location.hostname)
const ticketKey = 'future-classroom.account-transfer.v1'
export function loadTransferTicket(): string | undefined { try { return secret(sessionStorage.getItem(ticketKey)) } catch { return undefined } }
export function clearTransferTicket() { try { sessionStorage.removeItem(ticketKey) } catch { /* Page memory still releases the result. */ } }
export class AccountSecurityHttpApi extends HttpApi {
  private async act(input: object, csrf: string, signal: AbortSignal): Promise<unknown> { const r = await this.request('admin/account-security', signal, { schemaVersion: 1, ...input }, csrf); if (r.status !== 200) throw new ProtocolFault(); return adminEnvelope(r.value, v => v).data }
  async rotate(currentPassword: string, csrf: string, signal: AbortSignal): Promise<string> { const value = await this.act({ action: 'rotateRecovery', currentPassword }, csrf, signal); object(value); return recoveryCode(value.recoveryCode) }
  async prepare(expectedRevision: string, targetId: string, csrf: string, signal: AbortSignal): Promise<string> { const value = await this.act({ action: 'prepareTransfer', expectedRevision, targetId }, csrf, signal); object(value); const ticket = secret(value.ticket); try { sessionStorage.setItem(ticketKey, ticket) } catch { /* Current component retains the proof in memory. */ } return ticket }
  async transfer(ticket: string, currentPassword: string, csrf: string, signal: AbortSignal): Promise<void> { const value = await this.act({ action: 'transfer', ticket: secret(ticket), currentPassword, reviewedTarget: true, understoodRevocation: true }, csrf, signal); object(value); if (value.state !== 'COMMITTED') throw new ProtocolFault() }
  async outcome(ticket: string, csrf: string, signal: AbortSignal): Promise<'COMMITTED' | 'NOT_PREPARED' | 'NOT_FOUND'> { const value = await this.act({ action: 'outcome', ticket: secret(ticket) }, csrf, signal); object(value); if (!['COMMITTED', 'NOT_PREPARED', 'NOT_FOUND'].includes(String(value.state))) throw new ProtocolFault(); return value.state as 'COMMITTED' | 'NOT_PREPARED' | 'NOT_FOUND' }
  async recover(loginName: string, code: string, newPassword: string, signal: AbortSignal): Promise<string> { const r = await this.request('session/recover', signal, { schemaVersion: 1, loginName, recoveryCode: code, newPassword }); if (r.status !== 200) throw new ProtocolFault(); return adminEnvelope(r.value, v => { object(v); return recoveryCode(v.recoveryCode) }).data }
}
