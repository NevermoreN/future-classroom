import { describe, expect, it } from 'vitest'
import * as voice from '../src/api/voice-protocol'
import { ProtocolFault } from '../src/api/protocol'
import { caps, job, wireCaps, wireUpload } from './voice-fixtures'
const input = { commandId: '71', assetId: '45', expectedAssetRevision: '2', classId: '7', repeatTotal: 3 }
describe('voice wire boundary and state meaning', () => {
  it('retains u64 identifiers without numeric coercion and requires exact capabilities', () => {
    const wire = wireCaps(); wire.identityRevision = '18446744073709551615'; wire.currentClass!.classId = '18446744073709551615'
    expect(voice.capabilities(wire).currentClass?.classId).toBe('18446744073709551615')
    for (const bad of [0, '0', '07', '+7', '7e0', '18446744073709551616']) expect(() => voice.capabilities({ ...wire, identityRevision: bad })).toThrow(ProtocolFault)
    for (const field of Object.keys(wire)) { const missing = { ...wire } as Record<string, unknown>; delete missing[field]; expect(() => voice.capabilities(missing)).toThrow(ProtocolFault) }
    expect(() => voice.capabilities({ ...wire, audioPath: 'C:/school/private.wav' })).toThrow(ProtocolFault)
    expect(() => voice.capabilities({ ...wire, maxDurationMillis: 60001 })).toThrow(ProtocolFault)
    expect(() => voice.capabilities({ ...wire, targetAuthorized: true, currentClass: null })).toThrow(ProtocolFault)
  })
  it('rejects malformed UTF16, control characters and ambiguous civil dates in labels/capabilities', () => {
    for (const name of ['\ud800', 'x\u0085', ' ', 'x'.repeat(129)]) expect(() => voice.capabilities({ ...wireCaps(), currentClass: { classId: '7', className: name } })).toThrow(ProtocolFault)
    for (const date of ['2026-02-29T12:00:00', '2026-10-30T12:00:00Z', '2026-10-30T24:00:00']) expect(() => voice.capabilities({ ...wireCaps(), serverNow: date })).toThrow(ProtocolFault)
  })
  it('correlates upload receipt with exact captured target and actual WAV frame count', () => {
    expect(voice.uploaded(wireUpload(), '7', 16044).durationMillis).toBe(500)
    for (const patch of [{ classId: '8' }, { bytes: 16046 }, { durationMillis: 501 }, { assetRevision: '0' }, { bytes: 1920046 }, { expiresAt: '2026-10-30T12:00:00Z' }]) expect(() => voice.uploaded({ ...wireUpload(), ...patch }, '7', 16044)).toThrow(ProtocolFault)
    expect(voice.uploaded({ ...wireUpload(), bytes: 1920044, durationMillis: 60000 }, '7', 1920044).bytes).toBe(1920044)
  })
  it('distinguishes STOPPING from released terminal states and rejects impossible playback counters', () => {
    expect(voice.terminal(voice.job(job({ state: 'STOPPING', currentRepeat: 1 })))).toBe(false)
    expect(voice.terminal(voice.job(job({ state: 'CANCELED', currentRepeat: 1 })))).toBe(true)
    for (const patch of [{ state: 'COMPLETED', completedRepeats: 2 }, { state: 'PLAYING', currentRepeat: 0 }, { state: 'PLAYING', completedRepeats: 3, currentRepeat: 3 }, { state: 'QUEUED', completedRepeats: 1 }, { state: 'GAP', currentRepeat: 0 }, { jobId: '72' }, { revision: '0' }, { failure: 'UNKNOWN_FAILURE' }]) expect(() => voice.job({ ...job(), ...patch })).toThrow(ProtocolFault)
  })
  it('rejects duplicate task rows, too many active jobs and internal authorization fields', () => {
    expect(() => voice.jobs({ schemaVersion: 1, status: 'VOICE_JOBS', jobs: [job(), job()] })).toThrow(ProtocolFault)
    expect(() => voice.jobs({ schemaVersion: 1, status: 'VOICE_JOBS', jobs: Array.from({ length: 5 }, (_, i) => job({ jobId: String(i + 1), commandId: String(i + 1) })) })).toThrow(ProtocolFault)
    expect(() => voice.job({ ...job(), authorization: { principalId: '1' } })).toThrow(ProtocolFault)
    expect(() => voice.jobs({ schemaVersion: 1, status: 'VOICE_JOBS', jobs: Array.from({ length: 25 }, (_, i) => job({ jobId: String(i + 1), commandId: String(i + 1), state: 'CANCELED' })) })).toThrow(ProtocolFault)
  })
  it('requires nonzero CAS revisions and bounded repeat input before any fetch', () => {
    expect(voice.submitInput(input)).toEqual({ schemaVersion: 1, ...input })
    for (const total of [0, 11, 1.5, NaN]) expect(() => voice.submitInput({ ...input, repeatTotal: total })).toThrow(ProtocolFault)
    expect(() => voice.submitInput({ ...input, expectedAssetRevision: '0' })).toThrow(ProtocolFault)
    expect(() => voice.cancelInput({ commandId: '81', expectedRevision: '0' })).toThrow(ProtocolFault)
  })
  it('rejects foreign committed task and refuses active playback as cancellation acknowledgement', () => {
    const receipt = { schemaVersion: 1, status: 'COMMITTED', commandId: '71', job: job() }
    expect(voice.committed(receipt, input).jobId).toBe('71')
    for (const patch of [{ classId: '8' }, { repeatTotal: 10 }, { jobId: '72', commandId: '72' }]) expect(() => voice.committed({ ...receipt, job: job(patch) }, input)).toThrow(ProtocolFault)
    const cancel = { commandId: '81', expectedRevision: '1' }
    expect(voice.cancelled({ schemaVersion: 1, status: 'CANCEL_REQUESTED', commandId: '81', job: job({ state: 'STOPPING', currentRepeat: 1, revision: '2' }) }, '71', cancel).state).toBe('STOPPING')
    for (const state of ['QUEUED', 'STARTING', 'PLAYING', 'GAP'] as const) expect(() => voice.cancelled({ schemaVersion: 1, status: 'CANCEL_REQUESTED', commandId: '81', job: job({ state, currentRepeat: state === 'QUEUED' ? 0 : 1, completedRepeats: state === 'GAP' ? 1 : 0, revision: '2' }) }, '71', cancel)).toThrow(ProtocolFault)
    expect(() => voice.currentJob({ schemaVersion: 1, status: 'VOICE_JOB', job: job() }, '72')).toThrow(ProtocolFault)
  })
  it('tracks identity and runtime generation as well as actual class during recording', () => {
    expect(voice.sameTarget(caps(), caps())).toBe(true)
    for (const next of [caps({ identityRevision: '10' }), caps({ runtimeGeneration: '4' }), caps({ currentClass: { classId: '8', className: '八班' } })]) expect(voice.sameTarget(caps(), next)).toBe(false)
  })
})
