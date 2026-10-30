import type { VoiceApi } from '../src/api/voice-client'
import type { VoiceCapabilities, VoiceJob, VoiceUploaded } from '../src/api/voice-protocol'
import { vi } from 'vitest'
export const csrf = 'ab'.repeat(32)
export const caps = (patch: Partial<VoiceCapabilities> = {}): VoiceCapabilities => ({ identityRevision: '9', runtimeGeneration: '3', serverNow: '2026-10-30T12:00:00', currentClass: { classId: '7', className: '合成七班' }, enabled: true, admission: 'OPEN', targetAuthorized: true, deviceAvailability: 'AVAILABLE', ...patch })
export const wireCaps = () => ({ schemaVersion: 1, status: 'VOICE_CAPABILITIES', ...caps(), defaultRepeatTotal: 3, minRepeatTotal: 1, maxRepeatTotal: 10, minDurationMillis: 500, maxDurationMillis: 60000, sampleRate: 16000, channels: 1, bitsPerSample: 16, maxWavBytes: 1920044, uploadTtlSeconds: 600 })
export const job = (patch: Partial<VoiceJob> = {}): VoiceJob => ({ jobId: '71', commandId: '71', classId: '7', repeatTotal: 3, completedRepeats: 0, currentRepeat: 0, state: 'QUEUED', revision: '1', failure: null, ...patch })
export const upload = (patch: Partial<VoiceUploaded> = {}): VoiceUploaded => ({ assetId: '45', assetRevision: '2', classId: '7', bytes: 16044, durationMillis: 500, expiresAt: '2026-10-30T12:10:00', ...patch })
export const wireUpload = () => ({ schemaVersion: 1, status: 'VOICE_UPLOADED', ...upload() })
export const wav = (frames = 8000) => new Blob([new Uint8Array(44 + frames * 2)], { type: 'audio/wav' })
export const recording = (recordingId = 'recording-one') => ({ recordingId, wav: wav(), frames: 8000, sampleRate: 16000 as const })
export const signal = () => new AbortController().signal
export const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } })
export function api() {
  return { voiceCapabilities: vi.fn<VoiceApi['voiceCapabilities']>().mockResolvedValue(caps()), voiceJobs: vi.fn<VoiceApi['voiceJobs']>().mockResolvedValue([]), uploadVoice: vi.fn<VoiceApi['uploadVoice']>().mockResolvedValue(upload()), submitVoice: vi.fn<VoiceApi['submitVoice']>().mockImplementation(async input => job({ jobId: input.commandId, commandId: input.commandId, classId: input.classId, repeatTotal: input.repeatTotal })), voiceJob: vi.fn<VoiceApi['voiceJob']>().mockResolvedValue(job()), cancelVoice: vi.fn<VoiceApi['cancelVoice']>().mockImplementation(async (id, input) => job({ jobId: id, commandId: id, state: 'STOPPING', currentRepeat: 1, revision: (BigInt(input.expectedRevision) + 1n).toString() })) }
}
export function deferred<T>() { let resolve!: (value: T) => void; let reject!: (error: unknown) => void; const promise = new Promise<T>((a, b) => { resolve = a; reject = b }); return { promise, resolve, reject } }
