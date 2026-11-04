import { ApiError, isAborted, setPageToken, type ServerApi } from './client'
import { ProtocolFault, ProtocolVersionFault, pageCredential, validateLogin, validatePasswordChange, validateSetup, type LoginForm, type PageSession, type PasswordChangeForm, type Session, type SetupForm } from './protocol'
export type ViewState =
  | { phase: 'checking' }
  | { phase: 'anonymous'; notice?: string }
  | { phase: 'setup'; notice?: string }
  | { phase: 'setting-up' }
  | { phase: 'logging-in' }
  | { phase: 'authenticated'; session: Session; notice?: string; passwordChange?: boolean; passwordBusy?: boolean; passwordNotice?: string }
  | { phase: 'logging-out' }
  | { phase: 'logout-unknown' }
  | { phase: 'error'; message: string; refreshRequired?: boolean }
export interface PageCredentialStore { load(): string | undefined; save(token: string): void; clear(): void }
const storageKey = 'future-classroom.page-session.v1'
/** sessionStorage follows the tab lifetime and survives refresh. Passwords and
 * account records never enter browser storage. No unload/close heuristics. */
export const pageCredentialStore: PageCredentialStore = {
  load() { try { const value = sessionStorage.getItem(storageKey); return value === null ? undefined : pageCredential(value) } catch { return undefined } },
  save(token) { try { sessionStorage.setItem(storageKey, pageCredential(token)) } catch { /* Current page stays usable when browser storage is unavailable. */ } },
  clear() { try { sessionStorage.removeItem(storageKey) } catch { /* In-memory credential is cleared independently. */ } },
}
export class SessionController {
  private state: ViewState = { phase: 'checking' }
  private operation: AbortController | undefined
  private sequence = 0
  private disposed = false
  private compatible = false
  private pageToken: string | undefined
  constructor(private readonly api: ServerApi, private readonly changed: (state: ViewState) => void,
    private readonly credentials: PageCredentialStore = pageCredentialStore) {}
  private publish(state: ViewState) { if (!this.disposed) { this.state = state; this.changed(state) } }
  private begin() { this.operation?.abort(); const controller = new AbortController(); this.operation = controller; return { controller, id: ++this.sequence } }
  private current(id: number) { return !this.disposed && id === this.sequence }
  private accept(session: PageSession) {
    this.pageToken = session.pageToken
    setPageToken(session.pageToken); this.credentials.save(session.pageToken)
    const { pageToken: _, ...account } = session
    this.publish({ phase: 'authenticated', session: account })
  }
  acknowledgeRecoveryCode(): void {
    if (this.state.phase !== 'authenticated') return
    const { recoveryCode: _, ...session } = this.state.session
    this.publish({ ...this.state, session })
  }
  private clear() { this.pageToken = undefined; this.credentials.clear(); setPageToken(undefined) }
  private failed(error: unknown, previous?: Session) {
    if (error instanceof ApiError && error.status === 401) {
      this.clear(); this.publish({ phase: 'anonymous', notice: '账号或会话不可用，请重新登录。' }); return
    }
    if (previous && !(error instanceof ProtocolFault)) {
      this.publish({ phase: 'authenticated', session: previous, notice: '连接暂时中断，当前页面登录已保留。请检查网络后重试。' }); return
    }
    this.publish({ phase: 'error', message: error instanceof ProtocolFault ? error.message : '暂时无法连接白板。登录信息已保留，请检查网络后重新连接。', ...(error instanceof ProtocolVersionFault ? { refreshRequired: true } : {}) })
  }
  async connect(): Promise<void> {
    if (this.disposed || this.state.phase === 'logging-in' || this.state.phase === 'logging-out' || this.state.phase === 'setting-up') return
    const previous = this.state.phase === 'authenticated' ? this.state.session : undefined
    const { controller, id } = this.begin(); this.publish({ phase: 'checking' }); this.compatible = false
    try {
      const status = await this.api.status(controller.signal)
      if (!status.online) throw new Error('offline')
      if (!this.current(id)) return
      this.compatible = true
      if (status.initialized === false) { this.clear(); this.publish({ phase: 'setup' }); return }
      const token = this.pageToken ?? this.credentials.load()
      // Never allow an existing cookie alone to log a new tab into the account.
      if (!token) { setPageToken(undefined); this.publish({ phase: 'anonymous' }); return }
      setPageToken(token)
      const account = await this.api.resume(token, controller.signal)
      if (this.current(id)) this.accept(account)
    } catch (error) { if (this.current(id) && !isAborted(error)) this.failed(error, previous) }
  }
  async login(form: LoginForm): Promise<void> {
    if (this.disposed || this.state.phase !== 'anonymous' || !this.compatible) return
    try { validateLogin(form) } catch { this.publish({ phase: 'anonymous', notice: '请输入有效的用户名和密码。' }); return }
    const { controller, id } = this.begin(); this.publish({ phase: 'logging-in' })
    try {
      const account = await this.api.login(form, controller.signal)
      if (this.current(id)) this.accept(account)
    } catch (error) {
      if (!this.current(id) || isAborted(error)) return
      if (error instanceof ProtocolFault) this.failed(error)
      else this.publish({ phase: 'anonymous', notice: error instanceof ApiError
        ? error.status === 429 ? '尝试次数较多，请稍后再登录。'
          : [400, 401, 403].includes(error.status) ? '用户名或密码不正确，或账号已停用。' : '白板暂时无法完成登录，请稍后重试。'
        : '登录未完成，请检查网络后重试。' })
    }
  }
  async setup(form: SetupForm): Promise<void> {
    if (this.disposed || this.state.phase !== 'setup' || !this.compatible) return
    try { validateSetup(form) } catch { this.publish({ phase: 'setup', notice: '请输入学校名称、管理员用户名、显示名称和 12–128 个字符的密码。' }); return }
    const { controller, id } = this.begin(); this.publish({ phase: 'setting-up' })
    try { const account = await this.api.setup(form, controller.signal); if (this.current(id)) this.accept(account) }
    catch (error) {
      if (!this.current(id) || isAborted(error)) return
      if (error instanceof ApiError && error.status === 409) { this.publish({ phase: 'anonymous', notice: '本机已经完成初始化，请使用已有账号登录。' }); return }
      this.publish({ phase: 'setup', notice: error instanceof ApiError && error.status === 403 ? '请在白板本机通过托盘打开设置，完成首次初始化。' : error instanceof ApiError && error.status === 400 ? '初始化资料未被接受，请检查必填项和密码长度。' : '初始化结果尚未确认，请重新检查连接核对当前状态。不会自动重复创建管理员。' })
    }
  }
  async logout(): Promise<void> {
    if (this.disposed || this.state.phase !== 'authenticated') return
    const csrf = this.state.session.csrf
    const { controller, id } = this.begin(); this.publish({ phase: 'logging-out' }); this.credentials.clear()
    try {
      await this.api.logout(csrf, controller.signal)
      if (this.current(id)) this.publish({ phase: 'anonymous', notice: '已退出登录。' })
    } catch (error) {
      if (!this.current(id) || isAborted(error)) return
      if (error instanceof ApiError && [401, 403].includes(error.status)) this.publish({ phase: 'anonymous', notice: '原会话已失效，请重新登录。' })
      else this.publish({ phase: 'logout-unknown' })
    } finally { if (this.current(id)) this.clear() }
  }
  showPasswordChange(): void {
    if (this.state.phase !== 'authenticated' || this.state.passwordBusy) return
    this.publish({ phase: 'authenticated', session: this.state.session, passwordChange: true })
  }
  cancelPasswordChange(): void {
    if (this.state.phase !== 'authenticated' || this.state.passwordBusy || this.state.session.mustChangePassword) return
    this.publish({ phase: 'authenticated', session: this.state.session })
  }
  async changePassword(form: PasswordChangeForm): Promise<void> {
    if (this.disposed || this.state.phase !== 'authenticated' || this.state.passwordBusy) return
    const session = this.state.session
    try { validatePasswordChange(form) } catch {
      this.publish({ phase: 'authenticated', session, passwordChange: true, passwordNotice: '请输入当前密码，并设置 12–128 个字符的新密码。' }); return
    }
    const { controller, id } = this.begin()
    this.publish({ phase: 'authenticated', session, passwordChange: true, passwordBusy: true })
    try {
      const account = await this.api.changePassword(form, session.csrf, controller.signal)
      if (this.current(id)) this.accept(account)
    } catch (error) {
      if (!this.current(id) || isAborted(error)) return
      if (error instanceof ApiError && error.status === 401) { this.invalidate(); return }
      this.publish({ phase: 'authenticated', session, passwordChange: true, passwordNotice: error instanceof ApiError && [400, 403].includes(error.status)
        ? '当前密码不正确，或新密码不符合要求，请核对后重试。'
        : '尚未确认密码修改结果，请检查连接。若已生效，请使用新密码重新登录；页面不会自动重复提交。' })
    }
  }
  invalidate(notice = '账号授权已失效，请重新登录。'): void {
    if (this.disposed) return
    this.operation?.abort(); this.sequence++; this.clear(); this.publish({ phase: 'anonymous', notice })
  }
  dispose(): void { this.disposed = true; this.sequence++; this.operation?.abort(); this.operation = undefined }
}
