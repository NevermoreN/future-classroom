import { ApiError, commandRejected, commandRetired, isAborted } from './client'
import { commandIdentity } from './command-protocol'
import { ProtocolFault, ProtocolVersionFault } from './protocol'
import type { VoiceApi } from './voice-client'
import { sameTarget, terminal, type VoiceCapabilities, type VoiceJob, type VoiceSubmit, type VoiceCancel } from './voice-protocol'

/** Local recorder generation only; never sent as a command identity. */
export const newRecordingId = () => globalThis.crypto.randomUUID()

export interface RecordingContext { recordingId: string; capabilities: VoiceCapabilities; classId: string; repeatTotal: number }
export type PendingVoice = ({ kind: 'SUBMIT'; input: VoiceSubmit } | { kind: 'CANCEL'; jobId: string; input: VoiceCancel }) & { retired?: boolean }
export interface VoiceState {
  capabilities: VoiceCapabilities | null; jobs: VoiceJob[]; repeatTotal: number
  phase: 'idle' | 'recording' | 'uploading' | 'submitting' | 'unknown' | 'blocked'
  history: { command: PendingVoice; job: VoiceJob } | null; context: RecordingContext | null; pending: PendingVoice | null; busy: boolean; notice: string
}
export const emptyVoice: VoiceState = { capabilities: null, jobs: [], repeatTotal: 3, phase: 'idle', history: null, context: null, pending: null, busy: false, notice: '' }
export const mayRecord = (c: VoiceCapabilities | null) => !!c?.currentClass && c.enabled && c.admission === 'OPEN' && c.targetAuthorized && c.deviceAvailability !== 'UNAVAILABLE'

export class VoiceController {
  private state: VoiceState = emptyVoice
  private operation: AbortController | undefined
  private poll: AbortController | undefined
  private timer: ReturnType<typeof setTimeout> | undefined
  private sequence = 0
  private disposed = false
  private incompatible = ''
  private active = false
  private visible = true
  constructor(private api: VoiceApi, private csrf: string, private changed: (s: VoiceState) => void, private revoked: () => void, private cancelRecorder: () => void) {}
  snapshot() { return this.state }
  private publish(patch: Partial<VoiceState>) {
    if (this.disposed) return
    if (this.incompatible) patch = { ...patch, notice: this.versionNotice(patch.notice ?? this.state.notice) }
    this.state = { ...this.state, ...patch }; this.changed(this.state)
  }
  private versionNotice(message: string) { return message.includes(this.incompatible) ? message : `${this.incompatible}${message ? ` ${message}` : ''}` }
  private versionFault(error: unknown): boolean {
    if (!(error instanceof ProtocolVersionFault)) return false
    this.incompatible = error.message
    this.operation?.abort(); this.poll?.abort(); this.sequence++; clearTimeout(this.timer); this.timer = undefined
    if (this.state.context || ['recording', 'uploading'].includes(this.state.phase)) this.cancelRecorder()
    this.publish({ busy: false, context: null, phase: this.state.pending ? 'unknown' : 'blocked', notice: this.state.pending ? '原命令结果尚未确认，元数据已保留；仅可手动查询原任务，不会重发。' : '自动查询已停止，本次未发送录音已取消；刷新页面前不能继续录音或提交。' })
    return true
  }
  private begin() { this.operation?.abort(); this.poll?.abort(); this.operation = new AbortController(); return { id: ++this.sequence, signal: this.operation.signal } }
  private current(id: number) { return !this.disposed && this.sequence === id }
  private unauthorized(e: unknown) { if (!(e instanceof ApiError) || e.status !== 401 && e.status !== 403) return false; this.dispose(); this.revoked(); return true }
  hasRiskyDraft() { return !!this.state.context || !!this.state.pending || this.state.busy }
  setRepeat(value: number) { if (!this.incompatible && this.state.phase === 'idle' && !this.state.busy && Number.isInteger(value) && value >= 1 && value <= (this.state.capabilities?.maxRepeatTotal ?? 10)) this.publish({ repeatTotal: value }) }
  private checkUpdate(next: VoiceJob, previous: VoiceJob) {
    if (next.jobId !== previous.jobId || next.commandId !== previous.commandId || next.commandTicket !== previous.commandTicket || next.classId !== previous.classId || next.repeatTotal !== previous.repeatTotal || BigInt(next.revision) < BigInt(previous.revision) || next.completedRepeats < previous.completedRepeats || terminal(previous) && JSON.stringify(next) !== JSON.stringify(previous) || next.revision === previous.revision && JSON.stringify(next) !== JSON.stringify(previous)) throw new ProtocolFault()
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
    const repeatTotal = this.state.repeatTotal <= capabilities.maxRepeatTotal ? this.state.repeatTotal : capabilities.defaultRepeatTotal
    if (context && (!sameTarget(context.capabilities, capabilities) || !mayRecord(capabilities))) {
      this.cancelRecorder(); this.publish({ capabilities, repeatTotal, context: null, phase: 'blocked', notice: '白板班级、授权或播放限制已变化，本次未发送录音已取消。请重新核对。' }); return false
    }
    this.publish({ capabilities, repeatTotal }); return true
  }
  async open() { if (this.disposed || this.incompatible) return; this.active = true; await this.refresh(); this.schedule() }
  async refresh() {
    if (this.disposed || this.incompatible || this.state.busy || this.state.pending) return
    const { id, signal } = this.begin(); this.publish({ busy: true })
    try {
      const [caps, jobs] = await Promise.all([this.api.voiceCapabilities(signal), this.api.voiceJobs(signal)])
      if (!this.current(id)) return
      const accepted = this.acceptCapabilities(caps); this.acceptList(jobs)
      this.publish({ busy: false, ...(accepted && !this.state.context ? { phase: 'idle' as const, notice: '' } : {}) })
    } catch (error) { if (!this.current(id) || isAborted(error) || this.unauthorized(error) || this.versionFault(error)) return; this.publish({ busy: false, notice: '无法核对语音服务。已提交任务不会因断网自动撤回；可以主动刷新。' }) }
  }
  beginRecording(recordingId: string): RecordingContext | null {
    if (this.disposed || this.incompatible || !this.active || !this.visible || this.state.busy || this.state.pending || this.state.phase !== 'idle' || !mayRecord(this.state.capabilities)) return null
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
    if (this.incompatible || !context || context.recordingId !== result.recordingId || this.state.phase !== 'recording' || this.disposed) return
    if (!Number.isSafeInteger(result.frames) || result.frames < 8000 || result.frames > context.capabilities.maxDurationMillis * 16 || result.wav.type !== 'audio/wav' || result.wav.size !== 44 + result.frames * 2 || result.wav.size > context.capabilities.maxWavBytes || result.sampleRate !== 16000 || context.repeatTotal > context.capabilities.maxRepeatTotal) { this.cancelUnsent('录音格式或时长超过白板允许范围，未上传或提交。'); return }
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
      const captured = { assetId: receipt.assetId, expectedAssetRevision: receipt.assetRevision, classId: context.classId, repeatTotal: context.repeatTotal }
      const pair = commandIdentity(await this.api.issueCommand('voice', this.csrf, signal))
      if (!this.current(id)) return
      input = { ...pair, ...captured }
      this.publish({ context: null, phase: 'submitting', pending: { kind: 'SUBMIT', input }, notice: '正在提交原命令；上传完成不等于已排队。' })
      const job = await this.api.submitVoice(input, this.csrf, signal)
      if (!this.current(id)) return
      this.verifySubmit(job, input)
      this.publish({ jobs: this.mergeJob(job), pending: null, busy: false, phase: 'idle', notice: '白板已确认任务持久化。排队和驱动播放完成均不能保证扬声器已被听到。' })
    } catch (error) {
      if (!this.current(id) || isAborted(error) || this.unauthorized(error) || this.versionFault(error)) return
      const definite = commandRejected(error), retired = commandRetired(error)
      this.publish({ busy: false, context: null, phase: input && !definite ? 'unknown' : 'blocked', pending: input && retired ? { kind: 'SUBMIT', input, retired: true } : input && !definite ? { kind: 'SUBMIT', input } : null, notice: retired ? '原票据已过期或未获发行，此操作已停止；保留原票据，只可明确核对当前任务，不会重发。' : input && !definite ? '提交结果未知。仅手动查询原任务，不会自动重发或生成新命令。' : '本次上传或提交未能完成。不会自动重试；临时录音由白板到期清理，请重新核对后明确操作。' })
    } finally { this.schedule() }
  }
  private verifySubmit(job: VoiceJob, input: VoiceSubmit) { if (job.jobId !== input.commandId || job.commandId !== input.commandId || job.commandTicket !== input.commandTicket || job.classId !== input.classId || job.repeatTotal !== input.repeatTotal) throw new ProtocolFault() }
  async stop(job: VoiceJob) {
    if (this.disposed || this.incompatible || this.state.busy || this.state.pending || terminal(job) || job.state === 'STOPPING' || !this.state.jobs.some(j => j.jobId === job.jobId && j.commandTicket === job.commandTicket && j.revision === job.revision)) return
    if (this.state.context) this.cancelUnsent('已取消未发送录音，正在处理原任务的停止请求。')
    const captured = { jobId: job.jobId, jobCommandTicket: job.commandTicket, expectedRevision: job.revision }
    let input: VoiceCancel | undefined
    const { id, signal } = this.begin(); this.publish({ busy: true, notice: '正在领取原任务停止操作的服务器票据。' })
    try {
      const pair = commandIdentity(await this.api.issueCommand('voice', this.csrf, signal))
      if (!this.current(id)) return
      input = { ...pair, jobCommandTicket: captured.jobCommandTicket, expectedRevision: captured.expectedRevision }
      this.publish({ pending: { kind: 'CANCEL', input, jobId: captured.jobId }, notice: '正在请求停止此任务及其剩余遍数。' })
      const next = await this.api.cancelVoice(captured.jobId, input, this.csrf, signal)
      if (!this.current(id)) return
      this.checkUpdate(next, job)
      if (BigInt(next.revision) <= BigInt(input.expectedRevision) || next.state !== 'STOPPING' && !terminal(next)) throw new ProtocolFault()
      this.publish({ jobs: this.mergeJob(next), pending: null, busy: false, phase: 'idle', notice: terminal(next) ? '白板已确认此任务终态。' : '停止请求已提交，正在等待驱动归还；还不能显示为已停止。' })
    } catch (error) {
      if (!this.current(id) || isAborted(error) || this.unauthorized(error) || this.versionFault(error)) return
      const definite = commandRejected(error), retired = commandRetired(error)
      this.publish({ busy: false, phase: !input || definite ? 'blocked' : 'unknown', pending: input && retired ? { kind: 'CANCEL', input, jobId: captured.jobId, retired: true } : input && !definite ? { kind: 'CANCEL', input, jobId: captured.jobId } : null, notice: !input ? '未取得服务器票据，停止请求尚未发送；请明确核对任务后操作。' : retired ? '原票据已过期或未获发行，已停止此操作。原任务的实际所有权与状态仍需独立核对。' : definite ? '停止请求被拒绝或修订已变化，请主动读取当前任务再操作。' : '停止结果未知。仅手动读取原任务，不会自动重发停止请求。' })
    } finally { this.schedule() }
  }
  async queryPending() {
    const pending = this.state.pending
    if (!pending || this.state.busy || this.disposed) return
    const { id, signal } = this.begin(); this.publish({ busy: true })
    try {
      if (pending.retired) {
        const jobId = pending.kind === 'SUBMIT' ? pending.input.commandId : pending.jobId
        const submitTicket = pending.kind === 'SUBMIT' ? pending.input.commandTicket : pending.input.jobCommandTicket
        const job = await this.api.voiceJob(jobId, submitTicket, signal)
        if (!this.current(id)) return
        if (pending.kind === 'SUBMIT') this.verifySubmit(job, pending.input)
        else if (job.jobId !== pending.jobId || job.commandTicket !== pending.input.jobCommandTicket) throw new ProtocolFault()
        this.publish({ jobs: this.mergeJob(job), busy: false, phase: 'blocked', notice: '已独立读取原任务的当前状态；原命令票据仍已停止使用，不会重发或据此推断停止成功。' }); return
      }
      const result = await this.api.voiceOutcome(commandIdentity(pending.input), signal)
      if (!this.current(id)) return
      if (result.commandId !== pending.input.commandId || result.commandTicket !== pending.input.commandTicket) throw new ProtocolFault()
      if (result.state === 'NOT_FOUND') { this.publish({ busy: false, phase: 'unknown', notice: '当前未查到原命令，不证明从未提交；不会因此补发或停止任务。' }); return }
      const job = result.job
      if (pending.kind === 'SUBMIT') this.verifySubmit(job, pending.input)
      else if (job.jobId !== pending.jobId || job.commandTicket !== pending.input.jobCommandTicket || BigInt(job.revision) <= BigInt(pending.input.expectedRevision) || job.state !== 'STOPPING' && !terminal(job)) throw new ProtocolFault()
      this.publish({ history: { command: pending, job }, busy: false, pending: this.incompatible ? pending : null, phase: this.incompatible ? 'unknown' : 'idle', notice: '已查到原命令的历史持久回执；它不代表当前播放或停止状态。请明确刷新当前任务，不会补发操作。' })
    } catch (error) {
      if (!this.current(id) || isAborted(error) || this.unauthorized(error) || this.versionFault(error)) return
      if (commandRetired(error)) { this.publish({ busy: false, pending: { ...pending, retired: true }, phase: 'blocked', notice: '原票据已过期或未获发行，历史查询已停止。保留原元数据，可明确读取原任务当前状态；不推断所有权已归还。' }); return }
      this.publish({ busy: false, phase: pending.retired ? 'blocked' : 'unknown', notice: '查询未能确认原命令或任务，请稍后主动核对；不会重发。' })
    } finally { this.schedule() }
  }
  private schedule() {
    clearTimeout(this.timer)
    if (this.disposed || this.incompatible || !this.active || !this.visible || this.state.pending) return
    this.timer = setTimeout(() => { void this.pollJobs() }, this.state.context || this.state.jobs.some(j => !terminal(j)) ? 1000 : 15000)
  }
  private async pollJobs() {
    if (this.disposed || this.incompatible || !this.active || !this.visible) return
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
    } catch (error) { if (this.disposed || operation.signal.aborted || sequence !== this.sequence || isAborted(error) || this.unauthorized(error) || this.versionFault(error)) return; this.publish({ notice: '状态更新中断，已接纳任务不会自动重发；请主动核对。' }) }
    finally { this.schedule() }
  }
  setVisible(visible: boolean) { this.visible = visible; if (!visible) { clearTimeout(this.timer); this.poll?.abort(); this.cancelUnsent('页面已隐藏，本次未发送录音已取消并释放麦克风。') } else this.schedule() }
  leave(confirmed = false) {
    if (this.hasRiskyDraft() && !confirmed) return false
    this.active = false; clearTimeout(this.timer); this.poll?.abort(); this.operation?.abort(); this.sequence++; this.cancelRecorder(); this.publish(emptyVoice); return true
  }
  dispose() { if (this.disposed) return; this.leave(true); this.csrf = ''; this.disposed = true }
}
