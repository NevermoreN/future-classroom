import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { VoiceController, mayRecord, type VoiceState } from '../src/api/voice-controller'
import { ProtocolVersionFault } from '../src/api/protocol'
import { ApiError, NetworkFault } from '../src/api/client'
import type { VoiceJob, VoiceUploaded } from '../src/api/voice-protocol'
import { api, caps, csrf, deferred, job, recording, upload, wav } from './voice-fixtures'
function fixture() { const transport = api(), states: VoiceState[] = [], revoked = vi.fn(), cancelRecorder = vi.fn(), ids = vi.fn().mockReturnValueOnce('71').mockReturnValueOnce('81').mockReturnValue('91'); const controller = new VoiceController(transport, csrf, state => states.push(state), revoked, cancelRecorder, ids); return { api: transport, controller, states, revoked, cancelRecorder, ids, state: () => controller.snapshot() } }
beforeEach(() => vi.useFakeTimers())
afterEach(() => vi.useRealTimers())
describe('voice recording and durable task lifecycle', () => {
  it('uses configured repeat and duration bounds before upload without changing an accepted task', async () => {
    const f = fixture(); f.api.voiceCapabilities.mockResolvedValue(caps({ maxRepeatTotal: 2, defaultRepeatTotal: 2, maxDurationMillis: 1250, maxWavBytes: 40044 }))
    await f.controller.open(); expect(f.state().repeatTotal).toBe(2)
    f.controller.setRepeat(3); expect(f.state().repeatTotal).toBe(2)
    const context = f.controller.beginRecording('configured')!; expect(context.capabilities.maxDurationMillis).toBe(1250)
    await f.controller.recorded({ ...recording('configured'), frames: 20001, wav: wav(20001) })
    expect(f.api.uploadVoice).not.toHaveBeenCalled(); expect(f.api.submitVoice).not.toHaveBeenCalled()
    f.controller.dispose()
  })
  it('cancels an unsent recording when limits change even if a malformed server forgets to advance the epoch', async () => {
    const f = fixture(); await f.controller.open(); const context = f.controller.beginRecording('old-policy')!
    f.api.voiceCapabilities.mockResolvedValue(caps({ maxRepeatTotal: 1, defaultRepeatTotal: 1, maxDurationMillis: 500, maxWavBytes: 16044 }))
    await f.controller.refresh(); expect(f.cancelRecorder).toHaveBeenCalled(); expect(f.state().context).toBeNull(); expect(context.repeatTotal).toBe(3)
    await f.controller.recorded(recording('old-policy')); expect(f.api.uploadVoice).not.toHaveBeenCalled(); expect(f.state().repeatTotal).toBe(1)
    f.controller.dispose()
  })
  it('locks original target and repeat count, never submits at upload acknowledgement alone, finalizes once', async () => {
    const f = fixture(), pending = deferred<VoiceUploaded>(); await f.controller.open(); f.controller.setRepeat(7); const context = f.controller.beginRecording('recording-one')!
    f.controller.setRepeat(1); expect(context.repeatTotal).toBe(7)
    f.api.uploadVoice.mockReturnValueOnce(pending.promise); const sending = f.controller.recorded(recording()); await Promise.resolve(); await Promise.resolve()
    expect(f.state().phase).toBe('uploading'); expect(f.api.submitVoice).not.toHaveBeenCalled()
    await f.controller.recorded(recording()); pending.resolve(upload()); await sending
    expect(f.api.uploadVoice).toHaveBeenCalledOnce(); expect(f.api.submitVoice).toHaveBeenCalledOnce(); expect(f.api.submitVoice.mock.calls[0]![0]).toEqual({ commandId: '71', assetId: '45', expectedAssetRevision: '2', classId: '7', repeatTotal: 7 })
    expect(f.state().jobs[0]?.repeatTotal).toBe(7); expect(f.state().notice).toContain('不能保证'); f.controller.dispose()
  })
  it('rejects stale finalizer and cancels changed target before uploading any bytes', async () => {
    const f = fixture(); await f.controller.open(); f.controller.beginRecording('recording-one')
    await f.controller.recorded(recording('old-recording')); expect(f.api.uploadVoice).not.toHaveBeenCalled()
    f.api.voiceCapabilities.mockResolvedValueOnce(caps({ currentClass: { classId: '8', className: '八班' } })); await f.controller.recorded(recording())
    expect(f.state().phase).toBe('blocked'); expect(f.cancelRecorder).toHaveBeenCalled(); expect(f.api.uploadVoice).not.toHaveBeenCalled(); expect(f.api.submitVoice).not.toHaveBeenCalled(); f.controller.dispose()
  })
  it.each(['identityRevision', 'runtimeGeneration'] as const)('does not adopt changed %s for an old recording', async key => {
    const f = fixture(); await f.controller.open(); f.controller.beginRecording('recording-one'); f.api.voiceCapabilities.mockResolvedValueOnce(caps({ [key]: '100' })); await f.controller.recorded(recording())
    expect(f.api.uploadVoice).not.toHaveBeenCalled(); expect(f.state().context).toBeNull(); f.controller.dispose()
  })
  it('hiding or leaving cancels unsubmitted upload and ignores late receipt/finalizer output', async () => {
    const f = fixture(), pending = deferred<VoiceUploaded>(); await f.controller.open(); f.controller.beginRecording('recording-one'); f.api.uploadVoice.mockReturnValueOnce(pending.promise)
    const sending = f.controller.recorded(recording()); await Promise.resolve(); await Promise.resolve(); expect(f.api.uploadVoice).toHaveBeenCalledOnce()
    expect(f.controller.leave()).toBe(false); f.controller.setVisible(false); expect(f.api.uploadVoice.mock.calls[0]![3].aborted).toBe(true)
    pending.resolve(upload()); await sending; await f.controller.recorded(recording()); await vi.advanceTimersByTimeAsync(60000)
    expect(f.api.submitVoice).not.toHaveBeenCalled(); expect(f.api.voiceJobs).toHaveBeenCalledOnce(); expect(f.state().context).toBeNull(); expect(vi.getTimerCount()).toBe(0); f.controller.dispose()
  })
  it('keeps exact unknown submit command and only manually queries original job, including404', async () => {
    const f = fixture(); await f.controller.open(); f.controller.beginRecording('recording-one'); f.api.submitVoice.mockRejectedValueOnce(new NetworkFault()); await f.controller.recorded(recording())
    const original = f.state().pending; expect(f.state().phase).toBe('unknown'); expect(original?.kind).toBe('SUBMIT')
    f.controller.setRepeat(10); await vi.advanceTimersByTimeAsync(60000); expect(f.api.voiceJob).not.toHaveBeenCalled(); expect(f.api.submitVoice).toHaveBeenCalledOnce(); expect(f.ids).toHaveBeenCalledOnce()
    f.api.voiceJob.mockRejectedValueOnce(new ApiError(404, 'NOT_FOUND')); await f.controller.queryPending(); expect(f.state().pending).toBe(original); expect(f.state().notice).toContain('不证明')
    f.api.voiceJob.mockResolvedValueOnce(job({ state: 'COMPLETED', currentRepeat: 3, completedRepeats: 3, revision: '8' })); await f.controller.queryPending()
    expect(f.api.voiceJob.mock.calls.map(call => call[0])).toEqual(['71', '71']); expect(f.state().pending).toBeNull(); expect(f.state().jobs[0]?.state).toBe('COMPLETED'); expect(f.api.submitVoice).toHaveBeenCalledOnce(); f.controller.dispose()
  })
  it('rejects foreign unknown result and preserves original payload without resending', async () => {
    const f = fixture(); await f.controller.open(); f.controller.beginRecording('recording-one'); f.api.submitVoice.mockRejectedValueOnce(new NetworkFault()); await f.controller.recorded(recording())
    f.api.voiceJob.mockResolvedValueOnce(job({ classId: '8' })); await f.controller.queryPending(); expect(f.state().phase).toBe('unknown'); expect(f.state().jobs).toEqual([]); expect(f.state().pending?.kind).toBe('SUBMIT'); expect(f.api.submitVoice).toHaveBeenCalledOnce(); f.controller.dispose()
  })
  it('allows stopping an old task with recording admission closed and requires actual terminal confirmation', async () => {
    const f = fixture(), active = job({ state: 'PLAYING', currentRepeat: 1 }); f.api.voiceCapabilities.mockResolvedValueOnce(caps({ admission: 'CLOSED', targetAuthorized: false })); f.api.voiceJobs.mockResolvedValueOnce([active]); await f.controller.open()
    expect(mayRecord(f.state().capabilities)).toBe(false); await f.controller.stop(active)
    expect(f.api.cancelVoice.mock.calls[0]![0]).toBe('71'); expect(f.state().jobs[0]?.state).toBe('STOPPING'); expect(f.state().notice).toContain('还不能显示为已停止'); await f.controller.stop(f.state().jobs[0]!); expect(f.api.cancelVoice).toHaveBeenCalledOnce(); f.controller.dispose()
  })
  it('unknown cancel never repeatsPOST, and a still-playing query cannot confirm stopped', async () => {
    const f = fixture(), active = job({ state: 'PLAYING', currentRepeat: 1 }); f.api.voiceJobs.mockResolvedValueOnce([active]); await f.controller.open(); f.api.cancelVoice.mockRejectedValueOnce(new NetworkFault()); await f.controller.stop(active)
    const original = f.state().pending; f.api.voiceJob.mockResolvedValueOnce(job({ state: 'PLAYING', currentRepeat: 1, revision: '2' })); await f.controller.queryPending()
    expect(f.state().pending).toBe(original); expect(f.state().phase).toBe('unknown'); expect(f.state().notice).toContain('尚未终止'); await vi.advanceTimersByTimeAsync(30000); expect(f.api.cancelVoice).toHaveBeenCalledOnce()
    f.api.voiceJob.mockResolvedValueOnce(job({ state: 'CANCELED', currentRepeat: 1, revision: '3' })); await f.controller.queryPending(); expect(f.state().pending).toBeNull(); expect(f.state().jobs[0]?.state).toBe('CANCELED'); f.controller.dispose()
  })
  it.each([401, 403])('HTTP%i clears task/draft/commands and invalidates paired session exactlyonce', async status => {
    const f = fixture(); await f.controller.open(); f.controller.beginRecording('recording-one'); f.api.uploadVoice.mockRejectedValueOnce(new ApiError(status, status === 401 ? 'UNAUTHORIZED' : 'FORBIDDEN')); await f.controller.recorded(recording())
    expect(f.revoked).toHaveBeenCalledOnce(); expect(f.cancelRecorder).toHaveBeenCalled(); expect(f.state().capabilities).toBeNull(); expect(f.state().jobs).toEqual([]); expect(f.state().context).toBeNull(); expect(f.state().pending).toBeNull(); expect(vi.getTimerCount()).toBe(0)
    await f.controller.open(); expect(f.api.voiceJobs).toHaveBeenCalledOnce(); f.controller.dispose()
  })
  it('polls foreground active tasks at mostonce persecond and idle every15sec; background doesnotpoll', async () => {
    const active = fixture(); active.api.voiceJobs.mockResolvedValue([job()]); await active.controller.open(); await vi.advanceTimersByTimeAsync(999); expect(active.api.voiceJobs).toHaveBeenCalledOnce(); await vi.advanceTimersByTimeAsync(1); expect(active.api.voiceJobs).toHaveBeenCalledTimes(2)
    active.controller.setVisible(false); await vi.advanceTimersByTimeAsync(5000); expect(active.api.voiceJobs).toHaveBeenCalledTimes(2); active.controller.dispose()
    const idle = fixture(); await idle.controller.open(); await vi.advanceTimersByTimeAsync(14999); expect(idle.api.voiceJobs).toHaveBeenCalledOnce(); await vi.advanceTimersByTimeAsync(1); expect(idle.api.voiceJobs).toHaveBeenCalledTimes(2); idle.controller.dispose(); expect(vi.getTimerCount()).toBe(0)
  })
  it('automatic recording checkpoints observe actualtarget changes and stop old microphone context', async () => {
    const f = fixture(); f.api.voiceJobs.mockResolvedValue([job()]); await f.controller.open(); f.controller.beginRecording('recording-one'); f.api.voiceCapabilities.mockResolvedValue(caps({ currentClass: { classId: '8', className: '八班' } }))
    await vi.advanceTimersByTimeAsync(1000); expect(f.api.voiceCapabilities).toHaveBeenCalledTimes(2); expect(f.api.voiceJobs).toHaveBeenCalledOnce(); expect(f.state().context).toBeNull(); expect(f.cancelRecorder).toHaveBeenCalled(); await f.controller.recorded(recording()); expect(f.api.uploadVoice).not.toHaveBeenCalled(); f.controller.dispose()
  })

  it('recording an otherwiseidle board changes polling to onecapability checkpoint persecond', async () => {
    const f = fixture(); await f.controller.open(); f.controller.beginRecording('recording-one')
    await vi.advanceTimersByTimeAsync(999); expect(f.api.voiceCapabilities).toHaveBeenCalledOnce(); expect(f.api.voiceJobs).toHaveBeenCalledOnce()
    await vi.advanceTimersByTimeAsync(1); expect(f.api.voiceCapabilities).toHaveBeenCalledTimes(2); expect(f.api.voiceJobs).toHaveBeenCalledOnce()
    await vi.advanceTimersByTimeAsync(1000); expect(f.api.voiceCapabilities).toHaveBeenCalledTimes(3); expect(f.api.voiceJobs).toHaveBeenCalledOnce(); f.controller.dispose()
  })
  it('a malformed committed response remains unknown for the sameoriginal command', async () => {
    const f = fixture(); await f.controller.open(); f.controller.beginRecording('recording-one'); f.api.submitVoice.mockResolvedValueOnce(job({ classId: '8' })); await f.controller.recorded(recording())
    expect(f.state().phase).toBe('unknown'); expect(f.state().pending?.kind).toBe('SUBMIT'); expect(f.state().jobs).toEqual([]); expect(f.api.submitVoice).toHaveBeenCalledOnce(); f.controller.dispose()
  })
  it('CASconflict never claims stopped or retries and explicitrefresh supplies newrevision', async () => {
    const f = fixture(), active = job({ state: 'PLAYING', currentRepeat: 1 }); f.api.voiceJobs.mockResolvedValueOnce([active]); await f.controller.open(); f.api.cancelVoice.mockRejectedValueOnce(new ApiError(409, 'REVISION_CONFLICT')); await f.controller.stop(active)
    expect(f.state().phase).toBe('blocked'); expect(f.state().pending).toBeNull(); expect(f.state().jobs[0]?.state).toBe('PLAYING'); expect(f.api.cancelVoice).toHaveBeenCalledOnce()
    f.api.voiceJobs.mockResolvedValueOnce([job({ state: 'PLAYING', currentRepeat: 2, completedRepeats: 1, revision: '6' })]); await f.controller.refresh(); expect(f.state().jobs[0]?.revision).toBe('6'); expect(f.api.cancelVoice).toHaveBeenCalledOnce(); f.controller.dispose()
  })
  it('latepoll cannot restore jobs or capabilities after confirmedleave', async () => {
    const f = fixture(), pending = deferred<VoiceJob[]>(); f.api.voiceJobs.mockResolvedValueOnce([job()]); await f.controller.open(); f.api.voiceJobs.mockReturnValueOnce(pending.promise)
    await vi.advanceTimersByTimeAsync(1000); const latestSignal = f.api.voiceJobs.mock.calls.at(-1)![0]; expect(f.controller.leave(true)).toBe(true); expect(latestSignal.aborted).toBe(true)
    pending.resolve([job({ revision: '4', state: 'PLAYING', currentRepeat: 1 })]); await Promise.resolve(); await Promise.resolve(); expect(f.state().jobs).toEqual([]); expect(f.state().capabilities).toBeNull(); expect(vi.getTimerCount()).toBe(0); f.controller.dispose()
  })
  it('rejects backwards or terminal-resurrected task progress instead of adopting it', async () => {
    const f = fixture(), old = job({ revision: '8', state: 'COMPLETED', currentRepeat: 3, completedRepeats: 3 }); f.api.voiceJobs.mockResolvedValueOnce([old]); await f.controller.open(); f.api.voiceJobs.mockResolvedValueOnce([job({ revision: '9', state: 'QUEUED' })]); await f.controller.refresh(); expect(f.state().jobs).toEqual([old]); expect(f.state().notice).toContain('无法核对'); f.controller.dispose()
  })
})

describe('voice version recovery stops activity without discarding original command metadata', () => {
  it('keeps incompatible submit metadata, stops all polling/new POST, and allows only explicit original-ID GET', async () => {
    const f = fixture(), fault = new ProtocolVersionFault(); await f.controller.open(); f.controller.beginRecording('recording-one'); f.api.submitVoice.mockRejectedValueOnce(fault); await f.controller.recorded(recording())
    const pending = f.state().pending; expect(pending?.kind).toBe('SUBMIT'); expect(f.state().notice).toContain(fault.message); expect(f.state().phase).toBe('unknown'); expect(vi.getTimerCount()).toBe(0)
    expect(f.controller.beginRecording('new-recording')).toBeNull(); await f.controller.recorded(recording()); await f.controller.refresh(); await f.controller.open(); await f.controller.stop(job()); await vi.advanceTimersByTimeAsync(60000)
    expect(f.api.submitVoice).toHaveBeenCalledOnce(); expect(f.api.cancelVoice).not.toHaveBeenCalled(); expect(f.api.voiceJob).not.toHaveBeenCalled(); expect(f.api.voiceJobs).toHaveBeenCalledOnce(); expect(f.ids).toHaveBeenCalledOnce()
    f.api.voiceJob.mockRejectedValueOnce(new ApiError(404, 'NOT_FOUND')); await f.controller.queryPending(); expect(f.state().pending).toBe(pending); expect(f.state().notice).toContain('不证明'); expect(f.state().notice).toContain(fault.message)
    await f.controller.queryPending(); expect(f.api.voiceJob.mock.calls.map(call => call[0])).toEqual(['71', '71']); expect(f.state().jobs[0]?.jobId).toBe('71'); expect(f.state().pending).toBe(pending); expect(f.state().notice).toContain(fault.message); expect(vi.getTimerCount()).toBe(0); expect(f.controller.leave()).toBe(false); f.controller.dispose()
  })
  it('a typed version failure during cancellation preserves exact cancel metadata without retrying POST', async () => {
    const f = fixture(), active = job({ state: 'PLAYING', currentRepeat: 1 }); f.api.voiceJobs.mockResolvedValueOnce([active]); await f.controller.open(); f.api.cancelVoice.mockRejectedValueOnce(new ProtocolVersionFault()); await f.controller.stop(active)
    const pending = f.state().pending; expect(pending?.kind).toBe('CANCEL'); expect(f.state().jobs).toEqual([active]); expect(f.state().notice).toContain('刷新页面')
    await f.controller.stop(active); await vi.advanceTimersByTimeAsync(30000); expect(f.api.cancelVoice).toHaveBeenCalledOnce(); expect(f.api.voiceJob).not.toHaveBeenCalled()
    f.api.voiceJob.mockResolvedValueOnce(job({ state: 'CANCELED', currentRepeat: 1, revision: '3' })); await f.controller.queryPending(); expect(f.state().pending).toBe(pending); expect(f.state().jobs[0]?.state).toBe('CANCELED'); expect(f.state().notice).toContain('管理员升级'); expect(f.api.voiceJob.mock.calls[0]![0]).toBe('71'); expect(vi.getTimerCount()).toBe(0); f.controller.dispose()
  })
  it('an automatic recording capability check actually cancels capture and never schedules another poll', async () => {
    const f = fixture(); await f.controller.open(); f.controller.beginRecording('recording-one'); const captured = f.state().capabilities
    f.api.voiceCapabilities.mockRejectedValueOnce(new ProtocolVersionFault()); await vi.advanceTimersByTimeAsync(1000)
    expect(f.cancelRecorder).toHaveBeenCalledOnce(); expect(f.state().context).toBeNull(); expect(f.state().capabilities).toBe(captured); expect(f.state().notice).toContain('刷新页面'); expect(vi.getTimerCount()).toBe(0)
    await f.controller.recorded(recording()); f.controller.setVisible(false); f.controller.setVisible(true); await vi.advanceTimersByTimeAsync(60000)
    expect(f.controller.beginRecording('new')).toBeNull(); expect(f.api.voiceCapabilities).toHaveBeenCalledTimes(2); expect(f.api.voiceJobs).toHaveBeenCalledOnce(); expect(f.api.uploadVoice).not.toHaveBeenCalled(); expect(f.api.submitVoice).not.toHaveBeenCalled(); f.controller.dispose()
  })
  it('a version fault while watching accepted jobs retains those jobs and stops periodic requests', async () => {
    const f = fixture(), accepted = job(); f.api.voiceJobs.mockResolvedValueOnce([accepted]); await f.controller.open(); f.api.voiceJobs.mockRejectedValueOnce(new ProtocolVersionFault()); await vi.advanceTimersByTimeAsync(1000)
    expect(f.state().jobs).toEqual([accepted]); expect(f.state().notice).toContain('管理员升级'); await vi.advanceTimersByTimeAsync(60000); expect(f.api.voiceJobs).toHaveBeenCalledTimes(2); expect(f.api.cancelVoice).not.toHaveBeenCalled(); expect(vi.getTimerCount()).toBe(0); f.controller.dispose()
  })
  it('pre-upload version mismatch releases unsent capture without uploading bytes', async () => {
    const f = fixture(); await f.controller.open(); f.controller.beginRecording('recording-one'); f.api.voiceCapabilities.mockRejectedValueOnce(new ProtocolVersionFault()); await f.controller.recorded(recording())
    expect(f.cancelRecorder).toHaveBeenCalledOnce(); expect(f.state().context).toBeNull(); expect(f.state().pending).toBeNull(); expect(f.state().notice).toContain('刷新页面'); expect(f.api.uploadVoice).not.toHaveBeenCalled(); expect(f.api.submitVoice).not.toHaveBeenCalled(); expect(vi.getTimerCount()).toBe(0); f.controller.dispose()
  })
  it('manual status-query version errors never discard an older unknown command or restart polling', async () => {
    const f = fixture(); await f.controller.open(); f.controller.beginRecording('recording-one'); f.api.submitVoice.mockRejectedValueOnce(new NetworkFault()); await f.controller.recorded(recording()); const pending = f.state().pending
    f.api.voiceJob.mockRejectedValueOnce(new ProtocolVersionFault()); await f.controller.queryPending(); expect(f.state().pending).toBe(pending); expect(f.state().notice).toContain('刷新页面')
    await f.controller.queryPending(); expect(f.api.voiceJob.mock.calls.map(call => call[0])).toEqual(['71', '71']); expect(f.state().pending).toBe(pending); expect(f.api.submitVoice).toHaveBeenCalledOnce(); expect(vi.getTimerCount()).toBe(0); f.controller.dispose()
  })
})
