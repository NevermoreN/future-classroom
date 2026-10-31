import { ApiError, isAborted, type ContentApi } from './client'
import { ProtocolFault, ProtocolVersionFault } from './protocol'
import { bodyText, compareScopes, keyFor, newCommandId, normalizedBodyText, remainingToday, sameKey, scopeKey, type Committed, type ContentCommand, type ContentDocument, type ContentKey, type ContentKind, type ScopeItem, type ScopePage } from './content-protocol'

export type EditorPhase = 'ready' | 'saving' | 'conflict' | 'merge' | 'unknown' | 'querying' | 'result' | 'expired' | 'scope-changed'
export interface PendingCommand { readonly key: ContentKey; readonly command: ContentCommand }
export interface EditorState {
  scope: ScopeItem; loaded: ContentDocument; draft: string; phase: EditorPhase; expired: boolean
  pending: PendingCommand | null; history: ContentDocument | null; latest: ContentDocument | null; notice: string
}
export interface ContentState {
  kind: ContentKind; page: ScopePage | null; editor: EditorState | null; busy: boolean; pageBlocked: boolean; notice: string; previousOffset: number | null
}
export function draftValid(text: string): boolean { try { bodyText(text); return true } catch { return false } }
export class ContentController {
  private state: ContentState = { kind: 'HOMEWORK', page: null, editor: null, busy: false, pageBlocked: false, notice: '', previousOffset: null }
  private operation: AbortController | undefined
  private sequence = 0
  private offsets: number[] = []
  private disposed = false
  private incompatible = ''
  private midnight: ReturnType<typeof setTimeout> | undefined
  constructor(private readonly api: ContentApi, private csrf: string,
    private readonly changed: (state: ContentState) => void, private readonly revoked: () => void,
    private readonly commandId = newCommandId) {}
  private publish(patch: Partial<ContentState>) {
    if (this.disposed) return
    if (this.incompatible) {
      patch = { ...patch, notice: this.versionNotice(patch.notice ?? this.state.notice) }
      if (patch.editor) patch.editor = { ...patch.editor, notice: this.versionNotice(patch.editor.notice) }
    }
    this.state = { ...this.state, ...patch }; this.changed(this.state)
  }
  private versionNotice(message: string) { return message.includes(this.incompatible) ? message : `${this.incompatible}${message ? ` ${message}` : ''}` }
  private versionFault(error: unknown): boolean {
    if (!(error instanceof ProtocolVersionFault)) return false
    this.incompatible = error.message
    this.operation?.abort(); this.sequence++; this.clearTimer()
    const editor = this.state.editor
    this.publish({ busy: false, pageBlocked: true, editor: editor ? { ...editor, phase: editor.pending ? 'unknown' : 'scope-changed', notice: editor.pending ? '提交结果尚未确认。原命令和草稿保留；只可手动查询原命令，不会重发。' : '原草稿已保留，刷新页面前不能继续提交。' } : null })
    return true
  }
  private begin() { this.operation?.abort(); const controller = new AbortController(); this.operation = controller; return { controller, id: ++this.sequence } }
  private current(id: number) { return !this.disposed && id === this.sequence }
  private clearTimer() { if (this.midnight !== undefined) clearTimeout(this.midnight); this.midnight = undefined }
  private arm(document: ContentDocument) {
    this.clearTimer()
    this.midnight = setTimeout(() => {
      const editor = this.state.editor
      if (editor && sameKey(editor.loaded.key, document.key)) this.publish({ editor: { ...editor, expired: true, phase: editor.pending ? editor.phase : 'expired', notice: '白板所报日期的有效时段已结束。旧草稿保留供核对，不会自动转发到新日。' } })
    }, remainingToday(document))
  }
  private unauthorized(error: unknown): boolean {
    if (!(error instanceof ApiError) || error.status !== 401 && error.status !== 403) return false
    this.operation?.abort(); this.sequence++; this.clearTimer(); this.csrf = ''
    this.publish({ page: null, editor: null, busy: false, notice: '', pageBlocked: true }); this.disposed = true; this.revoked(); return true
  }
  hasRiskyDraft(): boolean {
    const e = this.state.editor
    return !!e && (e.draft !== e.loaded.text || !!e.pending || e.phase === 'merge' || e.phase === 'conflict')
  }
  async loadScopes(kind: ContentKind, offset = 0, discardConfirmed = false): Promise<void> {
    if (this.disposed || this.incompatible || this.hasRiskyDraft() && !discardConfirmed) return
    const previous = this.state.page, { controller, id } = this.begin()
    this.publish({ kind, busy: true, notice: '' })
    try {
      const page = await this.api.scopes(kind, offset, controller.signal)
      if (!this.current(id)) return
      if (page.kind !== kind || page.offset !== offset) throw new ProtocolFault()
      if (offset !== 0 && previous && (previous.kind !== page.kind || previous.identityRevision !== page.identityRevision || previous.today !== page.today || previous.total !== page.total)) {
        const editor = this.state.editor
        this.publish({ busy: false, pageBlocked: true, notice: '授权范围或白板日期已变化，旧分页已停止使用。请从第一页重新核对；不会把旧草稿绑定到新目标。', editor: editor ? { ...editor, phase: 'scope-changed', notice: '授权范围发生变化，当前草稿暂不可提交。' } : null }); return
      }
      if (previous && previous.kind === page.kind && page.offset === previous.nextOffset && previous.items.length && page.items.length && compareScopes(previous.items.at(-1)!, page.items[0]!) >= 0) throw new ProtocolFault()
      if (offset === 0 || previous?.kind !== kind) this.offsets = []
      else if (previous && offset === previous.nextOffset) this.offsets.push(previous.offset)
      else if (offset === this.offsets.at(-1)) this.offsets.pop()
      else this.offsets = []
      this.clearTimer(); this.publish({ page, editor: null, busy: false, pageBlocked: false, notice: '', previousOffset: this.offsets.at(-1) ?? null })
    } catch (error) {
      if (!this.current(id) || isAborted(error) || this.unauthorized(error) || this.versionFault(error)) return
      this.publish({ busy: false, pageBlocked: true, editor: this.state.editor ? { ...this.state.editor, phase: 'scope-changed' } : null, notice: error instanceof ProtocolFault ? error.message : '无法读取授权范围，请主动重新加载。旧列表不能用于提交。' })
    }
  }
  async select(scope: ScopeItem, discardConfirmed = false): Promise<void> {
    if (this.disposed || this.incompatible || this.state.busy || this.state.pageBlocked || this.hasRiskyDraft() && !discardConfirmed) return
    const page = this.state.page
    if (!page || page.kind !== scope.kind || !page.items.some(item => scopeKey(item) === scopeKey(scope))) return
    const key = keyFor(scope, page.today), { controller, id } = this.begin()
    this.publish({ busy: true, notice: '' })
    try {
      const document = await this.api.content(key, controller.signal)
      if (!this.current(id)) return
      if (!sameKey(document.key, key)) throw new ProtocolFault()
      this.publish({ busy: false, editor: { scope, loaded: document, draft: document.text, phase: 'ready', expired: false, pending: null, latest: null, history: null, notice: '' } }); this.arm(document)
    } catch (error) {
      if (!this.current(id) || isAborted(error) || this.unauthorized(error) || this.versionFault(error)) return
      this.publish({ busy: false, notice: error instanceof ApiError && error.code === 'DAY_CHANGED' ? '白板日期已经变化。请重新加载当天范围，旧日期不会自动更改。' : error instanceof ProtocolFault ? error.message : '读取内容失败，请重新选择或核对连接。', pageBlocked: error instanceof ProtocolFault || error instanceof ApiError && error.code === 'DAY_CHANGED', editor: error instanceof ProtocolFault && this.state.editor ? { ...this.state.editor, phase: 'scope-changed' } : this.state.editor })
    }
  }
  edit(draft: string): void {
    const e = this.state.editor
    if (this.incompatible || !e || !['ready', 'conflict', 'merge'].includes(e.phase) || e.expired || this.state.busy) return
    this.publish({ editor: { ...e, draft } })
  }
  async save(clear = false): Promise<void> {
    const e = this.state.editor
    if (this.disposed || this.incompatible || !e || e.phase !== 'ready' || e.expired || this.state.busy || this.state.pageBlocked) return
    const text = clear ? '' : e.draft
    if (!draftValid(text)) { this.publish({ editor: { ...e, notice: '正文最多 16384 个 UTF-8 字节，不能包含不可显示的控制字符。' } }); return }
    if (e.loaded.revision === '18446744073709551615') { this.publish({ editor: { ...e, notice: '内容修订已到上限，请联系管理员。' } }); return }
    let pending: PendingCommand
    try { pending = Object.freeze({ key: Object.freeze({ ...e.loaded.key }), command: Object.freeze({ commandId: this.commandId(), expectedRevision: e.loaded.revision, text }) }) }
    catch { this.publish({ editor: { ...e, notice: '无法生成安全命令编号，请刷新并核对连接后重试。' } }); return }
    const { controller, id } = this.begin()
    this.publish({ busy: true, editor: { ...e, draft: text, phase: 'saving', pending, latest: null, history: null, notice: '正在提交此次命令，不会自动重复发送。' } })
    try {
      const result = await this.api.saveContent(pending.key, pending.command, this.csrf, controller.signal)
      if (!this.current(id)) return
      this.verify(result, pending)
      const current = this.state.editor!
      this.publish({ busy: false, editor: { ...current, loaded: result.content, draft: result.content.text, phase: current.expired ? 'expired' : 'ready', pending: null, notice: '白板已确认持久保存。此结果不代表内容此刻正在屏幕上显示。' } })
      if (!current.expired) this.arm(result.content)
    } catch (error) {
      if (!this.current(id) || isAborted(error) || this.unauthorized(error) || this.versionFault(error)) return
      const current = this.state.editor!
      if (error instanceof ApiError && error.status === 409) {
        const expired = error.code === 'DAY_CHANGED'
        this.publish({ busy: false, editor: { ...current, phase: expired ? 'expired' : 'conflict', expired: current.expired || expired, pending: null, notice: expired ? '白板日期已经变化。旧草稿已过期，不能自动提交到新日。' : error.code === 'IDEMPOTENCY_CONFLICT' ? '命令编号与已有内容冲突。草稿已保留，请明确载入最新内容并核对后再提交。' : '其他人已更新内容。草稿已保留，请载入最新修订后明确核对合并。' } }); return
      }
      if (error instanceof ApiError && error.status === 400) { this.publish({ busy: false, editor: { ...current, phase: 'conflict', pending: null, notice: '请求被拒绝。草稿已保留，请检查正文并载入当前内容核对。' } }); return }
      this.publish({ busy: false, editor: { ...current, phase: 'unknown', notice: '提交结果尚未确认。原命令、日期、修订和正文已保留在本页内存；只会按你的操作查询，不会重发。' } })
    }
  }
  private verify(result: Committed, pending: PendingCommand) {
    if (result.commandId !== pending.command.commandId || !sameKey(result.content.key, pending.key) || BigInt(result.content.revision) !== BigInt(pending.command.expectedRevision) + 1n || result.content.text !== normalizedBodyText(pending.command.text)) throw new ProtocolFault()
  }
  async query(): Promise<void> {
    const e = this.state.editor
    if (this.disposed || !e?.pending || this.state.busy || !['unknown', 'result'].includes(e.phase)) return
    const pending = e.pending, { controller, id } = this.begin()
    this.publish({ busy: true, editor: { ...e, phase: 'querying' } })
    try {
      const result = await this.api.commandResult(pending.command.commandId, controller.signal)
      if (!this.current(id)) return
      if (result.commandId !== pending.command.commandId) throw new ProtocolFault()
      const current = this.state.editor!
      if (result.state === 'NOT_FOUND') { this.publish({ busy: false, editor: { ...current, phase: 'unknown', notice: '当前未查到原命令结果。这不证明从未写入，排队中的提交可能尚未完成；不会因此重发。' } }); return }
      this.verify({ commandId: result.commandId, content: result.content }, pending)
      this.publish({ busy: false, editor: { ...current, phase: 'result', history: result.content, notice: '已查到原命令的持久结果。这是提交时的结果，不代表当前最新内容；请明确读取当前内容核对。' } })
    } catch (error) {
      if (!this.current(id) || isAborted(error) || this.unauthorized(error) || this.versionFault(error)) return
      const current = this.state.editor!
      this.publish({ busy: false, editor: { ...current, phase: 'unknown', expired: current.expired || error instanceof ApiError && error.code === 'DAY_CHANGED', notice: error instanceof ApiError && error.code === 'DAY_CHANGED' ? '原命令所属日期已过期，服务器不会返回旧日正文。旧草稿保留供审阅，不会发送到新日。' : error instanceof ProtocolFault ? '命令结果与原提交不匹配，已停止使用；原命令仍保留供核对。' : '查询尚未完成。原命令仍保留，请稍后主动查询；不会重发。' } })
    }
  }
  async loadLatest(): Promise<void> {
    const e = this.state.editor
    if (this.disposed || this.incompatible || !e || this.state.busy || !['ready', 'conflict', 'result', 'merge'].includes(e.phase) || e.expired) return
    const { controller, id } = this.begin(); this.publish({ busy: true })
    try {
      const latest = await this.api.content(e.loaded.key, controller.signal)
      if (!this.current(id)) return
      if (!sameKey(latest.key, e.loaded.key)) throw new ProtocolFault()
      this.publish({ busy: false, editor: { ...this.state.editor!, latest, phase: 'merge', notice: '最新内容已载入。请对照两份正文，明确选择采用服务器内容，或保留并合并你的草稿。此处不会提交。' } }); this.arm(latest)
    } catch (error) {
      if (!this.current(id) || isAborted(error) || this.unauthorized(error) || this.versionFault(error)) return
      const current = this.state.editor!
      this.publish({ busy: false, editor: { ...current, expired: current.expired || error instanceof ApiError && error.code === 'DAY_CHANGED', phase: error instanceof ApiError && error.code === 'DAY_CHANGED' ? 'expired' : current.phase, notice: error instanceof ApiError && error.code === 'DAY_CHANGED' ? '白板日期已变化，旧草稿不能自动转移到新日。' : error instanceof ProtocolFault ? error.message : '无法核对当前内容，请主动重试读取。' } })
    }
  }
  acknowledgeMerge(useServer: boolean): void {
    const e = this.state.editor
    if (this.incompatible || !e?.latest || e.phase !== 'merge' || this.state.busy || e.expired) return
    this.publish({ editor: { ...e, loaded: e.latest, draft: useServer ? e.latest.text : e.draft, phase: 'ready', pending: null, latest: null, history: null, notice: useServer ? '已采用当前服务器内容，尚未产生新的提交。' : '已明确核对最新修订。你的草稿保持不变，只有点击保存才会生成新命令。' } })
  }
  leave(discardConfirmed = false): boolean {
    if (this.hasRiskyDraft() && !discardConfirmed) return false
    this.operation?.abort(); this.sequence++; this.clearTimer(); this.publish({ editor: null, busy: false, notice: '' }); return true
  }
  dispose(): void { this.disposed = true; this.sequence++; this.operation?.abort(); this.clearTimer(); this.csrf = ''; this.state = { kind: 'HOMEWORK', page: null, editor: null, busy: false, pageBlocked: true, notice: '', previousOffset: null } }
}
