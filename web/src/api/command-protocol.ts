import { decimal, ProtocolFault, ProtocolVersionFault } from './protocol'

export type CommandFamily = 'content' | 'materials' | 'voice' | 'updates'
export interface CommandIdentity { commandId: string; commandTicket: string }
export interface CommandApi { issueCommand(family: CommandFamily, csrf: string, signal: AbortSignal): Promise<CommandIdentity> }

/** Syntax alone is never proof of server issuance or business authorization. */
export function commandTicket(value: unknown): string {
  if (typeof value !== 'string' || !/^c2:[0-9a-f]{32}:[1-9][0-9]{0,18}$/.test(value)) throw new ProtocolFault()
  const [, epoch, serial] = value.split(':')
  if (epoch === '0'.repeat(32) || BigInt(serial!) > 9223372036854775807n) throw new ProtocolFault()
  return value
}
export function commandVersion(value: unknown): void {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < 1) throw new ProtocolFault()
  if (value !== 2) throw new ProtocolVersionFault()
}
export function commandIdentity(value: CommandIdentity): CommandIdentity {
  return { commandId: decimal(value.commandId), commandTicket: commandTicket(value.commandTicket) }
}
export function sameCommand(a: CommandIdentity, b: CommandIdentity): boolean {
  return a.commandId === b.commandId && a.commandTicket === b.commandTicket
}
export function issued(value: unknown, family: CommandFamily): CommandIdentity {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new ProtocolFault()
  const v = value as Record<string, unknown>, keys = ['schemaVersion', 'status', 'family', 'commandId', 'commandTicket', 'expiresInMillis']
  if (Object.keys(v).length !== keys.length || Object.keys(v).some(k => !keys.includes(k))) throw new ProtocolFault()
  commandVersion(v.schemaVersion)
  if (v.status !== 'ISSUED' || v.family !== family || v.expiresInMillis !== 86400000) throw new ProtocolFault()
  return { commandId: decimal(v.commandId), commandTicket: commandTicket(v.commandTicket) }
}
