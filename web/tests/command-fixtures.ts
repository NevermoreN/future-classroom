import type { CommandIdentity } from '../src/api/command-protocol'
export const ticket = 'c2:' + '12'.repeat(16) + ':1'
export const identity = (commandId = '77'): CommandIdentity => ({ commandId, commandTicket: ticket })
