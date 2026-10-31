import { ApiError, isAborted, type ServerApi, abortError } from './client'
import { ProtocolFault, ProtocolVersionFault, validateForm, type PairingForm, type Session } from './protocol'
export type ViewState =
  | { phase: 'checking' }
  | { phase: 'anonymous'; notice?: string }
  | { phase: 'creating' }
  | { phase: 'waiting'; requestId: string }
  | { phase: 'authenticated'; session: Session; notice?: string }
  | { phase: 'logging-out' }
  | { phase: 'logout-unknown' }
  | { phase: 'expired' }
  | { phase: 'error'; message: string; canPair: boolean; refreshRequired?: boolean }
export function wait(milliseconds: number, signal: AbortSignal): Promise<void> {
  if (signal.aborted) return Promise.reject(abortError())
  return new Promise((resolve, reject) => {
    const abort = () => { clearTimeout(timer); reject(abortError()) }
    const timer = setTimeout(() => { signal.removeEventListener('abort', abort); resolve() }, milliseconds)
    signal.addEventListener('abort', abort, { once: true })
  })
}
/** One owner per mounted screen. Only server responses create an authenticated
 * view. A poll secret lives in one async stack, never in observable UI state. */
export class PairingController {
  private state: ViewState = { phase: 'checking' }
  private operation: AbortController | undefined
  private sequence = 0
  private disposed = false
  private compatible = false
  constructor(private readonly api: ServerApi, private readonly changed: (state: ViewState) => void,
    private readonly now = () => performance.now()) {}
  private publish(state: ViewState) { if (!this.disposed) { this.state = state; this.changed(state) } }
  private begin(): { controller: AbortController; id: number } {
    this.operation?.abort(); const controller = new AbortController(); this.operation = controller
    return { controller, id: ++this.sequence }
  }
  private current(id: number) { return !this.disposed && id === this.sequence }
  private refreshOnVersionFault(error: unknown, uncertain = ''): boolean {
    if (!(error instanceof ProtocolVersionFault)) return false
    this.compatible = false
    this.publish({ phase: 'error', message: error.message + uncertain, canPair: false, refreshRequired: true })
    return true
  }
  async connect(): Promise<void> {
    if (this.disposed || this.state.phase === 'creating' || this.state.phase === 'waiting' || this.state.phase === 'logging-out') return
    const { controller, id } = this.begin(); this.publish({ phase: 'checking' }); this.compatible = false
    try {
      const status = await this.api.status(controller.signal)
      if (!status.online) throw new Error('offline')
      if (!this.current(id)) return
      this.compatible = true
      const session = await this.api.session(controller.signal)
      if (this.current(id)) this.publish({ phase: 'authenticated', session })
    } catch (error) {
      if (!this.current(id) || isAborted(error)) return
      if (this.refreshOnVersionFault(error)) return
      if (this.compatible && error instanceof ApiError && (error.status === 401 || error.status === 403)) {
        this.publish({ phase: 'anonymous' }); return
      }
      this.publish({ phase: 'error', message: error instanceof ProtocolFault ? error.message : '暂时无法连接白板。请使用管理员提供的可信 HTTPS 地址，检查网络后重新检查连接。', canPair: false })
    }
  }
  async create(form: PairingForm): Promise<void> {
    if (this.disposed || this.state.phase !== 'anonymous' || !this.compatible) return
    try { validateForm(form) } catch { this.publish({ phase: 'anonymous', notice: '请输入 8 位数字配对码，以及有效的账号和设备名称（各不超过 64 字）。' }); return }
    const { controller, id } = this.begin(); this.publish({ phase: 'creating' })
    const deadline = this.now() + 120_000
    let timedOut = false
    const timer = setTimeout(() => { timedOut = true; controller.abort() }, 120_000)
    try {
      // Exactly once. Timeouts and unknown outcomes never re-enter this call.
      const receipt = await this.api.create(form, controller.signal)
      if (!this.current(id)) return
      this.publish({ phase: 'waiting', requestId: receipt.requestId })
      let delay = Math.max(500, receipt.retryAfterMs)
      while (true) {
        const remaining = deadline - this.now()
        if (remaining <= delay) { timedOut = true; throw abortError() }
        await wait(delay, controller.signal)
        if (!this.current(id)) return
        const result = await this.api.poll(receipt.requestId, receipt.pollSecret, controller.signal)
        if (!this.current(id)) return
        if (result.status === 'AUTHORIZED') {
          // HttpOnly Set-Cookie is handled by the browser. This read, rather
          // than the submitted login name, determines the actual account.
          const session = await this.api.session(controller.signal)
          if (this.current(id)) this.publish({ phase: 'authenticated', session })
          return
        }
        delay = Math.max(500, result.retryAfterMs)
      }
    } catch (error) {
      if (!this.current(id)) return
      if (timedOut) { this.publish({ phase: 'expired' }); return }
      if (isAborted(error)) return
      if (this.refreshOnVersionFault(error, '配对结果尚未确认，页面不会自动重发。请在白板核对申请。')) return
      if (error instanceof ApiError && (error.status === 401 || error.status === 403)) {
        this.publish({ phase: 'anonymous', notice: '申请已失效或授权被拒绝。请让白板管理员重新开启配对窗口后再申请。' }); return
      }
      this.publish({ phase: 'error', canPair: true, message: error instanceof ProtocolFault ? error.message : '配对申请的结果尚未确认。请在白板核对申请，并重新开启配对窗口后手动申请；页面不会重复提交。' })
    } finally { clearTimeout(timer) }
  }
  cancelPairing(): void {
    if (!['waiting', 'creating', 'expired', 'error'].includes(this.state.phase) || !this.compatible) return
    this.operation?.abort(); this.sequence++
    this.publish({ phase: 'anonymous', notice: '已停止等待。白板上的原申请将按时失效；重新申请前请让管理员开启新配对窗口。' })
  }
  async logout(): Promise<void> {
    if (this.disposed || this.state.phase !== 'authenticated') return
    const csrf = this.state.session.csrf
    const { controller, id } = this.begin(); this.publish({ phase: 'logging-out' })
    try {
      await this.api.logout(csrf, controller.signal)
      if (this.current(id)) this.publish({ phase: 'anonymous', notice: '白板已确认注销账号，原有渠道授权已失效。' })
    } catch (error) {
      if (!this.current(id) || isAborted(error)) return
      if (this.refreshOnVersionFault(error, '注销结果尚未确认，页面不会自动重发。刷新后请主动核对会话。')) return
      if (error instanceof ApiError && (error.status === 401 || error.status === 403)) {
        this.publish({ phase: 'anonymous', notice: '会话不可用，页面已清除会话信息。请重新配对。' }); return
      }
      // Discard the local account/CSRF copy. Only an explicit GET recheck may
      // recover a still-live server session; never claim success or resend POST.
      this.publish({ phase: 'logout-unknown' })
    }
  }
  invalidate(notice = '白板会话已失效，页面已停止操作并清除会话信息。请重新配对。'): void {
    if (this.disposed) return
    this.operation?.abort(); this.sequence++; this.publish({ phase: 'anonymous', notice })
  }
  dispose(): void { this.disposed = true; this.sequence++; this.operation?.abort(); this.operation = undefined }
}
