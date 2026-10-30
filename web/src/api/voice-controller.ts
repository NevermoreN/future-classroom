import { ApiError, isAborted } from './client'
import { newCommandId } from './content-protocol'
import { ProtocolFault } from './protocol'
import type { VoiceApi } from './voice-client'
import { sameTarget, terminal, type VoiceCapabilities, type VoiceJob, type VoiceSubmit, type VoiceCancel } from './voice-protocol'

export interface RecordingContext { recordingId: string; capabilities: VoiceCapabilities; classId: string; repeatTotal: number }
export type PendingVoice = { kind: 'SUBMIT'; input: VoiceSubmit } | { kind: 'CANCEL'; jobId: string; input: VoiceCancel }
export interface VoiceState {
  capabilities: VoiceCapabilities | null; jobs: VoiceJob[]; repeatTotal: number
  phase: 'idle' | 'recording' | 'uploading' | 'submitting' | 'unknown' | 'blocked'
  context: RecordingContext | null; pending: PendingVoice | null; busy: boolean; notice: string
}
export const emptyVoice: VoiceState = { capabilities: null, jobs: [], repeatTotal: 3, phase: 'idle', context: null, pending: null, busy: false, notice: '' }
export const mayRecord = (c: VoiceCapabilities | null) => !!c?.currentClass && c.enabled && c.admission === 'OPEN' && c.targetAuthorized && c.deviceAvailability !== 'UNAVAILABLE'

export class VoiceController {
  private state: VoiceState = emptyVoice
  private operation: AbortController | undefined
  private poll: AbortController | undefined
  private timer: ReturnType<typeof setTimeout> | undefined
  private sequence = 0
  private disposed = false
  private active = false
  private visible = true
  constructor(private api: VoiceApi, private csrf: string, private changed: (s: VoiceState) => void, private revoked: () => void, private cancelRecorder: () => void, private commandId = newCommandId) {}
  snapshot() { return this.state }
  private publish(patch: Partial<VoiceState>) { if (!this.disposed) { this.state = { ...this.state, ...patch }; this.changed(this.state) } }
  private begin() { this.operation?.abort(); this.poll?.abort(); this.operation = new AbortController(); return { id: ++this.sequence, signal: this.operation.signal } }
  private current(id: number) { return !this.disposed && this.sequence === id }
  private unauthorized(e: unknown) { if (!(e instanceof ApiError) || e.status !== 401 && e.status !== 403) return false; this.dispose(); this.revoked(); return true }
  hasRiskyDraft() { return !!this.state.context || !!this.state.pending || this.state.busy }
  setRepeat(value: number) { if (this.state.phase === 'idle' && !this.state.busy && Number.isInteger(value) && value >= 1 && value <= 10) this.publish({ repeatTotal: value }) }
  private checkUpdate(next: VoiceJob, previous: VoiceJob) {
    if (next.jobId !== previous.jobId || next.commandId !== previous.commandId || next.classId !== previous.classId || next.repeatTotal !== previous.repeatTotal || BigInt(next.revision) < BigInt(previous.revision) || next.completedRepeats < previous.completedRepeats || terminal(previous) && JSON.stringify(next) !== JSON.stringify(previous) || next.revision === previous.revision && JSON.stringify(next) !== JSON.stringify(previous)) throw new ProtocolFault()
  }
  private mergeJob(job: VoiceJob) {
    const previous = this.state.jobs.find(j => j.jobId === job.jobId)
    if (previous) this.checkUpdate(job, previous)
    return [job, ...this.state.jobs.filter(j => j.jobId !== job.jobId)].slice(0, 24)
  }
  private acceptList(jobs: VoiceJob[]) {
    for (const next of jobs) { const old = this.state.jobs.find(j => j.jobId === next.jobId); if (old) this.checkUpdate(next, old) }
    // A missing row is never interpreted as a successful cancel or submit.
    this.publish({ jobs })
  }
  private acceptCapabilities(capabilities: VoiceCapabilities) {
    const context = this.state.context
    if (context && (!sameTarget(context.capabilities, capabilities) || !mayRecord(capabilities))) {
      this.cancelRecorder(); this.publish({ capabilities, context: null, phase: 'blocked', notice: '白板班级、授权或播放状态已变化，本次未发送录音已取消。请重新核对。' }); return false
    }
    this.publish({ capabilities }); return true
  }
  async open() { if (this.disposed) return; this.active = true; await this.refresh(); this.schedule() }
  async refresh() {
    if (this.disposed || this.state.busy || this.state.pending) return
    const { id, signal } = this.begin(); this.publish({ busy: true })
    try {
      const [caps, jobs] = await Promise.all([this.api.voiceCapabilities(signal), this.api.voiceJobs(signal)])
      if (!this.current(id)) return
      const accepted = this.acceptCapabilities(caps); this.acceptList(jobs)
      this.publish({ busy: false, ...(accepted && !this.state.context ? { phase: 'idle' as const, notice: '' } : {}) })
    } catch (error) { if (!this.current(id) || isAborted(error) || this.unauthorized(error)) return; this.publish({ busy: false, notice: '无法核对语音服务。已提交任务不会因断网自动撤回；可以主动刷新。' }) }
  }
  beginRecording(recordingId: string): RecordingContext | null {
    if (this.disposed || !this.active || !this.visible || this.state.busy || this.state.pending || this.state.phase !== 'idle' || !mayRecord(this.state.capabilities)) return null
    const c = this.state.capabilities!, context: RecordingContext = { recordingId, capabilities: c, classId: c.currentClass!.classId, repeatTotal: this.state.repeatTotal }
    this.publish({ context, phase: 'recording', notice: '' }); this.schedule(); return context
  }
  cancelUnsent(message = '本次未发送录音已取消。') {
    if (!['recording', 'uploading'].includes(this.state.phase)) return
    this.operation?.abort(); this.sequence++; this.cancelRecorder()
    this.publish({ context: null, busy: false, phase: 'idle', notice: message })
    this.schedule()
  }
  async recorded(result: { recordingId: string; wav: Blob; frames: number; sampleRate: 16000 }) {
    const context = this.state.context
    if (!context || context.recordingId !== result.recordingId || this.state.phase !== 'recording' || this.disposed) return
    if (!Number.isSafeInteger(result.frames) || result.frames < 8000 || result.frames > 960000 || result.wav.type !== 'audio/wav' || result.wav.size !== 44 + result.frames * 2 || result.sampleRate !== 16000) { this.cancelUnsent('录音格式或时长无效，未上传或提交。'); return }
    const { id, signal } = this.begin(); this.publish({ busy: true, phase: 'uploading', notice: '正在核对白板目标并上传录音，尚未排队。' })
    let input: VoiceSubmit | undefined
    try {
      const caps = await this.api.voiceCapabilities(signal)
      if (!this.current(id)) return
      if (!sameTarget(context.capabilities, caps) || !mayRecord(caps)) { this.acceptCapabilities(caps); this.publish({ busy: false }); return }
      this.acceptCapabilities(caps)
      const receipt = await this.api.uploadVoice(context.classId, result.wav, this.csrf, signal)
      if (!this.current(id)) return
      if (receipt.classId !== context.classId || receipt.bytes !== result.wav.size || receipt.durationMillis !== Math.floor(result.frames / 16)) throw new ProtocolFault()
      input = { commandId: this.commandId(), assetId: receipt.assetId, expectedAssetRevision: receipt.assetRevision, classId: context.classId, repeatTotal: context.repeatTotal }
      this.publish({ context: null, phase: 'submitting', pending: { kind: 'SUBMIT', input }, notice: '正在提交原命令；上传完成不等于已排队。' })
      const job = await this.api.submitVoice(input, this.csrf, signal)
      if (!this.current(id)) return
      this.verifySubmit(job, input)
      this.publish({ jobs: this.mergeJob(job), pending: null, busy: false, phase: 'idle', notice: '白板已确认任务持久化。排队和驱动播放完成均不能保证扬声器已被听到。' })
    } catch (error) {
      if (!this.current(id) || isAborted(error) || this.unauthorized(error)) return
      const definite = error instanceof ApiError && [400, 409, 429].includes(error.status)
      this.publish({ busy: false, context: null, phase: input && !definite ? 'unknown' : 'blocked', pending: input && !definite ? { kind: 'SUBMIT', input } : null, notice: input && !definite ? '提交结果未知。仅手动查询原任务，不会自动重发或生成新命令。' : '本次上传或提交未能完成。不会自动重试；临时录音由白板到期清理，请重新核对后明确操作。' })
    } finally { this.schedule() }
  }
  private verifySubmit(job: VoiceJob, input: VoiceSubmit) { if (job.jobId !== input.commandId || job.commandId !== input.commandId || job.classId !== input.classId || job.repeatTotal !== input.repeatTotal) throw new ProtocolFault() }
  async stop(job: VoiceJob) {
    if (this.disposed || this.state.busy || this.state.pending || terminal(job) || job.state === 'STOPPING' || !this.state.jobs.some(j => j.jobId === job.jobId && j.revision === job.revision)) return
    if (this.state.context) this.cancelUnsent('已取消未发送录音，正在处理原任务的停止请求。')
    let input: VoiceCancel
    try { input = { commandId: this.commandId(), expectedRevision: job.revision } } catch { this.publish({ notice: '无法生成安全停止命令，请核对浏览器。' }); return }
    const { id, signal } = this.begin(); this.publish({ busy: true, pending: { kind: 'CANCEL', input, jobId: job.jobId }, notice: '正在请求停止此任务及其剩余遍数。' })
    try {
      const next = await this.api.cancelVoice(job.jobId, input, this.csrf, signal)
      if (!this.current(id)) return
      this.checkUpdate(next, job)
      if (BigInt(next.revision) <= BigInt(input.expectedRevision) || next.state !== 'STOPPING' && !terminal(next)) throw new ProtocolFault()
      this.publish({ jobs: this.mergeJob(next), pending: null, busy: false, phase: 'idle', notice: terminal(next) ? '白板已确认此任务终态。' : '停止请求已提交，正在等待驱动归还；还不能显示为已停止。' })
    } catch (error) {
      if (!this.current(id) || isAborted(error) || this.unauthorized(error)) return
      const definite = error instanceof ApiError && [400, 409, 429].includes(error.status)
      this.publish({ busy: false, phase: definite ? 'blocked' : 'unknown', pending: definite ? null : { kind: 'CANCEL', input, jobId: job.jobId }, notice: definite ? '停止请求被拒绝或修订已变化，请主动读取当前任务再操作。' : '停止结果未知。仅手动读取原任务，不会自动重发停止请求。' })
    } finally { this.schedule() }
  }
  async queryPending() {
    const pending = this.state.pending
    if (!pending || this.state.busy || this.disposed) return
    const jobId = pending.kind === 'SUBMIT' ? pending.input.commandId : pending.jobId
    const { id, signal } = this.begin(); this.publish({ busy: true })
    try {
      const job = await this.api.voiceJob(jobId, signal)
      if (!this.current(id)) return
      if (pending.kind === 'SUBMIT') this.verifySubmit(job, pending.input)
      const jobs = this.mergeJob(job), resolved = pending.kind === 'SUBMIT' || terminal(job)
      this.publish({ jobs, busy: false, pending: resolved ? null : pending, phase: resolved ? 'idle' : 'unknown', notice: resolved ? '已查到原任务当前持久状态；不会补发播报。' : '当前任务尚未终止，停止结果仍未确认。不会自动重复停止请求，请继续主动查询。' })
    } catch (error) {
      if (!this.current(id) || isAborted(error) || this.unauthorized(error)) return
      this.publish({ busy: false, phase: 'unknown', notice: error instanceof ApiError && error.status === 404 ? '当前未查到原任务，不证明从未提交；不会因此补发。' : '查询未能确认原任务，请稍后主动查询。' })
    } finally { this.schedule() }
  }
  private schedule() {
    clearTimeout(this.timer)
    if (this.disposed || !this.active || !this.visible || this.state.pending) return
    this.timer = setTimeout(() => { void this.pollJobs() }, this.state.context || this.state.jobs.some(j => !terminal(j)) ? 1000 : 15000)
  }
  private async pollJobs() {
    if (this.disposed || !this.active || !this.visible) return
    if (this.state.busy || this.state.pending) { this.schedule(); return }
    this.poll?.abort(); const operation = new AbortController(); this.poll = operation; const sequence = this.sequence
    try {
      if (this.state.context) {
        const caps = await this.api.voiceCapabilities(operation.signal)
        if (this.disposed || operation.signal.aborted || sequence !== this.sequence || !this.visible || !this.active) return
        this.acceptCapabilities(caps)
      } else {
        const jobs = await this.api.voiceJobs(operation.signal)
        if (this.disposed || operation.signal.aborted || sequence !== this.sequence || !this.visible || !this.active) return
        this.acceptList(jobs)
      }
    } catch (error) { if (this.disposed || operation.signal.aborted || sequence !== this.sequence || isAborted(error) || this.unauthorized(error)) return; this.publish({ notice: '状态更新中断，已接纳任务不会自动重发；请主动核对。' }) }
    finally { this.schedule() }
  }
  setVisible(visible: boolean) { this.visible = visible; if (!visible) { clearTimeout(this.timer); this.poll?.abort(); this.cancelUnsent('页面已隐藏，本次未发送录音已取消并释放麦克风。') } else this.schedule() }
  leave(confirmed = false) {
    if (this.hasRiskyDraft() && !confirmed) return false
    this.active = false; clearTimeout(this.timer); this.poll?.abort(); this.operation?.abort(); this.sequence++; this.cancelRecorder(); this.publish(emptyVoice); return true
  }
  dispose() { if (this.disposed) return; this.leave(true); this.csrf = ''; this.disposed = true }
}
