import { ApiError, isAborted } from './client'
import { newCommandId } from './content-protocol'
import { ProtocolFault, ProtocolVersionFault } from './protocol'
import { fileLimit, sameScope, type MaterialScopes, type MaterialScopeItem, type MaterialRecord, type MaterialPage, type MaterialCursor, type UploadReady, type MaterialCommitted } from './material-protocol'
import type { AppendMaterial, DeleteMaterial, MaterialApi } from './material-client'
export type PendingMaterial = { kind: 'APPEND'; scope: MaterialScopeItem; command: AppendMaterial; fileName: string; bytes: number } | { kind: 'DELETE'; scope: MaterialScopeItem; command: DeleteMaterial; original: MaterialRecord }
export interface MaterialState { scopes: MaterialScopes | null; selected: MaterialScopeItem | null; page: MaterialPage | null; file: File | null; classes: string[]; upload: UploadReady | null; phase: 'idle' | 'uploading' | 'uploaded' | 'committing' | 'unknown' | 'result' | 'blocked'; pending: PendingMaterial | null; history: MaterialRecord | null; busy: boolean; notice: string; previousScopeOffset: number | null; previousCursor: MaterialCursor | null; canPrevious: boolean }
export const emptyMaterials: MaterialState = { scopes: null, selected: null, page: null, file: null, classes: [], upload: null, phase: 'idle', pending: null, history: null, busy: false, notice: '', previousScopeOffset: null, previousCursor: null, canPrevious: false }
export class MaterialController {
  private state = emptyMaterials
  private operation: AbortController | undefined
  private sequence = 0
  private disposed = false
  private incompatible = ''
  private scopeOffsets: number[] = []
  private cursors: (MaterialCursor | null)[] = []
  constructor(private api: MaterialApi, private csrf: string, private changed: (s: MaterialState) => void, private revoked: () => void, private commandId = newCommandId) {}
  snapshot() { return this.state }
  private publish(patch: Partial<MaterialState>) {
    if (this.disposed) return
    if (this.incompatible) patch = { ...patch, notice: this.versionNotice(patch.notice ?? this.state.notice) }
    this.state = { ...this.state, ...patch }; this.changed(this.state)
  }
  private versionNotice(message: string) { return message.includes(this.incompatible) ? message : `${this.incompatible}${message ? ` ${message}` : ''}` }
  private versionFault(error: unknown): boolean {
    if (!(error instanceof ProtocolVersionFault)) return false
    this.incompatible = error.message
    this.operation?.abort(); this.sequence++
    this.publish({ busy: false, phase: this.state.pending ? 'unknown' : 'blocked', notice: this.state.pending ? '提交结果未知。原命令、文件和选择已保留，只可手动查询，不会重发。' : '原文件和班级选择已保留，刷新页面前不能继续上传或提交。' })
    return true
  }
  private begin() { this.operation?.abort(); this.operation = new AbortController(); return { id: ++this.sequence, signal: this.operation.signal } }
  private current(id: number) { return !this.disposed && id === this.sequence }
  private unauthorized(e: unknown): boolean { if (!(e instanceof ApiError) || e.status !== 401 && e.status !== 403) return false; this.dispose(); this.revoked(); return true }
  hasRiskyDraft() { return !!this.state.file || !!this.state.upload || !!this.state.pending || this.state.phase === 'uploading' || this.state.phase === 'committing' }
  async loadScopes(offset = 0, confirmed = false) {
    if (this.disposed || this.incompatible || this.hasRiskyDraft() && !confirmed) return
    const previous = this.state.scopes, { id, signal } = this.begin(); this.publish({ busy: true, notice: '' })
    try {
      const page = await this.api.materialScopes(offset, signal); if (!this.current(id)) return
      if (page.offset !== offset) throw new ProtocolFault()
      if (offset && previous && (previous.identityRevision !== page.identityRevision || previous.total !== page.total)) { this.publish({ busy: false, phase: 'blocked', notice: '授权范围已变化，请从第一页重新核对。旧文件和选择不会转交其他老师。' }); return }
      if (offset === 0) this.scopeOffsets = []; else if (offset === previous?.nextOffset) this.scopeOffsets.push(previous.offset); else if (offset === this.scopeOffsets.at(-1)) this.scopeOffsets.pop()
      this.cursors = []; this.publish({ ...emptyMaterials, scopes: page, previousScopeOffset: this.scopeOffsets.at(-1) ?? null })
    } catch (e) { if (!this.current(id) || isAborted(e) || this.unauthorized(e) || this.versionFault(e)) return; this.publish({ busy: false, phase: 'blocked', notice: '无法核对授权范围，请主动重新加载。' }) }
  }
  async select(scope: MaterialScopeItem, confirmed = false) { if (this.incompatible || !this.state.scopes?.items.some(s => sameScope(s, scope)) || this.state.phase === 'blocked' || this.hasRiskyDraft() && !confirmed) return; this.cursors = []; this.publish({ selected: scope, page: null, file: null, upload: null, pending: null, history: null, classes: [], phase: 'idle' }); await this.loadPage(null, true) }
  async loadMoreClasses() {
    const selected = this.state.selected, offset = selected?.nextClassOffset
    if (this.incompatible || !selected || offset === null || offset === undefined || this.state.busy || this.state.pending || this.state.phase === 'blocked' || this.disposed) return
    const { id, signal } = this.begin(); this.publish({ busy: true })
    try {
      const page = await this.api.materialClasses(selected, offset, signal); if (!this.current(id)) return
      if (!sameScope(page.scope, selected) || page.offset !== offset || page.total !== selected.classTotal || page.identityRevision !== this.state.scopes?.identityRevision || selected.classes.length !== offset || page.items.length === 0 && offset < page.total || page.items.some((c,i) => BigInt(c.classId) <= BigInt((i ? page.items[i-1] : selected.classes.at(-1))!.classId))) throw new ProtocolFault()
      this.publish({ busy: false, selected: { ...selected, classes: [...selected.classes, ...page.items], nextClassOffset: page.nextOffset }, notice: '已加载更多真实任教班级，请明确选择可见班级。' })
    } catch (e) { if (!this.current(id) || isAborted(e) || this.unauthorized(e) || this.versionFault(e)) return; this.publish({ busy: false, phase: 'blocked', notice: '任教班级或身份范围已变化，旧班级选择已停止使用。请重新加载范围，不会替换文件或自动提交。' }) }
  }
  async loadPage(cursor: MaterialCursor | null = null, confirmed = false) {
    const selected = this.state.selected; if (this.incompatible || !selected || this.disposed || this.hasRiskyDraft() && !confirmed) return
    const previous = this.state.page, { id, signal } = this.begin(); this.publish({ busy: true, notice: '' })
    try {
      const page = await this.api.materials(selected, cursor, signal); if (!this.current(id)) return
      if (!sameScope(page.scope, selected)) throw new ProtocolFault()
      if (!cursor) this.cursors = []; else if (previous?.nextCursor && cursor.offset === previous.nextCursor.offset) this.cursors.push(previous.offset === 0 ? null : { folderId: previous.folderId, folderRevision: previous.folderRevision, offset: previous.offset }); else if (this.cursors.length) this.cursors.pop()
      this.publish({ busy: false, page, file: null, classes: [], upload: null, pending: null, history: null, phase: 'idle', previousCursor: this.cursors.at(-1) ?? null, canPrevious: this.cursors.length > 0 })
    } catch (e) { if (!this.current(id) || isAborted(e) || this.unauthorized(e) || this.versionFault(e)) return; this.publish({ busy: false, page: null, phase: 'blocked', notice: e instanceof ApiError && e.code === 'STALE_CURSOR' ? '目录已变化，旧分页已停止。请明确从第一页重读。' : '无法读取目录，请重新核对范围。' }) }
  }
  pick(file: File | null) { if (this.incompatible || this.state.busy || this.state.pending || this.state.upload || this.state.phase === 'blocked') return; let valid = true; try { valid = !file || file.size > 0 && file.size <= fileLimit(file.name) } catch { valid = false }; if (!valid) { this.publish({ file: null, upload: null, phase: 'idle', notice: '请选择允许格式的本机文件：单文件最多50MiB，图片最多10MiB；图片完整解码与8MP校验由白板执行。' }); return }; this.publish({ file, upload: null, phase: 'idle', notice: '' }) }
  setClasses(ids: string[]) { const selected = this.state.selected; if (this.incompatible || !selected || this.state.busy || this.state.pending || this.state.phase === 'blocked') return; if (new Set(ids).size !== ids.length || ids.some(id => !selected.classes.some(c => c.classId === id))) return; this.publish({ classes: ids.slice().sort((a,b) => BigInt(a) < BigInt(b) ? -1 : 1) }) }
  async uploadFile() {
    const { selected, file } = this.state; if (this.incompatible || !selected || !file || this.state.busy || this.state.pending || this.state.phase === 'blocked') return
    const { id, signal } = this.begin(); this.publish({ busy: true, phase: 'uploading', upload: null, notice: '正在上传文件，尚未添加到课堂目录。' })
    try { const receipt = await this.api.createUpload(selected, file, this.csrf, signal); if (!this.current(id)) return; this.publish({ upload: receipt }); await this.api.upload(receipt.uploadId, file, this.csrf, signal); if (!this.current(id)) return; this.publish({ busy: false, phase: 'uploaded', upload: receipt, notice: '文件传输已完成，尚未添加。选择可见班级后，点击“校验并添加”。' }) }
    catch (e) { if (!this.current(id) || isAborted(e) || this.unauthorized(e) || this.versionFault(e)) return; this.publish({ busy: false, phase: 'idle', upload: null, notice: '上传未确认，尚未发送添加命令。临时文件由白板到期清理；可以明确重新上传，不会自动重试。' }) }
  }
  async cancelUpload() {
    if (this.incompatible || !['uploading', 'uploaded'].includes(this.state.phase) || this.state.pending) return
    const receipt = this.state.upload; this.operation?.abort(); this.sequence++
    this.publish({ busy: false, phase: 'idle', upload: null, notice: '已停止本页上传，未发送添加命令。未知临时内容会到期清理。' })
    if (!receipt) return
    const { id, signal } = this.begin(); this.publish({ busy: true, notice: '正在请求清理原临时文件，此操作不会撤回已保存资料。' })
    try { await this.api.cancelUpload(receipt.uploadId, this.csrf, signal); if (!this.current(id)) return; this.publish({ busy: false, notice: '白板已确认取消原临时文件；未发送添加命令。' }) }
    catch (e) { if (!this.current(id) || isAborted(e) || this.unauthorized(e) || this.versionFault(e)) return; this.publish({ busy: false, notice: '临时文件取消结果未确认。不会自动重试，白板将按期限清理；未发送添加命令。' }) }
  }
  async append() {
    const { selected, file, upload, classes } = this.state; if (this.incompatible || !selected || !file || !upload || !classes.length || this.state.busy || this.state.phase !== 'uploaded') return
    let commandId: string; try { commandId = this.commandId() } catch { this.publish({ notice: '无法生成安全命令编号，请核对浏览器后重试。' }); return }
    await this.submit({ kind: 'APPEND', scope: { ...selected }, command: { commandId, uploadId: upload.uploadId, allowedClassIds: classes.slice() }, fileName: file.name, bytes: file.size })
  }
  async delete(record: MaterialRecord) {
    const selected = this.state.selected; if (this.incompatible || !selected || this.state.busy || this.hasRiskyDraft() || !this.state.page?.items.some(r => r.materialId === record.materialId && r.revision === record.revision && r.resourceVersionId === record.resourceVersionId)) return
    let commandId: string; try { commandId = this.commandId() } catch { this.publish({ notice: '无法生成安全命令编号。' }); return }
    await this.submit({ kind: 'DELETE', scope: { ...selected }, command: { commandId, materialId: record.materialId, resourceVersionId: record.resourceVersionId, expectedMaterialRevision: record.revision }, original: { ...record } })
  }
  private verify(result: MaterialCommitted, p: PendingMaterial) {
    if (result.commandId !== p.command.commandId || !sameScope(result.scope, p.scope)) throw new ProtocolFault()
    const r = result.record
    if (p.kind === 'DELETE') {
      const o = p.original
      if (r.materialId !== o.materialId || r.folderId !== o.folderId || r.displayName !== o.displayName || r.resourceVersionId !== o.resourceVersionId || r.kind !== o.kind || r.bytes !== o.bytes || r.sha256 !== o.sha256 || r.addedAt !== o.addedAt || r.addedSequence !== o.addedSequence || r.allowedClassIds.length !== o.allowedClassIds.length || r.allowedClassIds.some((id,i) => id !== o.allowedClassIds[i]) || r.revision !== (BigInt(o.revision)+1n).toString() || r.status !== 'DELETED') throw new ProtocolFault()
    } else if (r.revision !== '1' || r.resourceVersionId !== p.command.uploadId || r.displayName !== p.fileName || r.bytes !== p.bytes || r.status !== 'PUBLISHED' || r.allowedClassIds.length !== p.command.allowedClassIds.length || r.allowedClassIds.some((id,i) => id !== p.command.allowedClassIds[i])) throw new ProtocolFault()
  }
  private async submit(pending: PendingMaterial) {
    if (this.incompatible || this.disposed) return
    const { id, signal } = this.begin(); this.publish({ busy: true, phase: 'committing', pending, history: null, notice: '正在校验并保存此次命令，不会自动重复提交。' })
    try { const result = pending.kind === 'APPEND' ? await this.api.append(pending.scope, pending.command, this.csrf, signal) : await this.api.deleteMaterial(pending.scope, pending.command, this.csrf, signal); if (!this.current(id)) return; this.verify(result, pending); this.publish({ busy: false, phase: 'result', history: result.record, file: null, upload: null, notice: '白板已确认此次命令持久保存。请明确重新读取目录核对；此结果不代表当前正在屏幕显示。' }) }
    catch (e) { if (!this.current(id) || isAborted(e) || this.unauthorized(e) || this.versionFault(e)) return; const definite = e instanceof ApiError && (e.status === 400 || e.status === 409 || e.status === 429); this.publish({ busy: false, phase: definite ? 'blocked' : 'unknown', pending: definite ? null : pending, notice: definite ? '此次命令被拒绝。请重新核对目录、权限或临时文件有效期，再明确操作。' : '提交结果未知。原命令保留在本页，只能手动查询；不会自动重发或生成新命令。' }) }
  }
  async query() {
    const p = this.state.pending; if (!p || this.state.busy || !['unknown', 'result'].includes(this.state.phase)) return
    const { id, signal } = this.begin(); this.publish({ busy: true })
    try { const result = await this.api.materialOutcome(p.command.commandId, signal); if (!this.current(id)) return; if (result.state === 'NOT_FOUND') { this.publish({ busy: false, phase: 'unknown', notice: '当前未查到原命令，不证明从未提交；不会因此重发。' }); return }; this.verify(result, p); this.publish({ busy: false, phase: 'result', history: result.record, ...(this.incompatible ? {} : { file: null, upload: null }), notice: '已查到原命令保存结果。它可能已被后续操作替代，请明确读取当前目录。' }) }
    catch (e) { if (!this.current(id) || isAborted(e) || this.unauthorized(e) || this.versionFault(e)) return; this.publish({ busy: false, phase: 'unknown', notice: '查询未能确认原命令，请稍后主动查询。不会重发。' }) }
  }
  async download(record: MaterialRecord): Promise<Blob | null> {
    const selected = this.state.selected; if (this.incompatible || !selected || this.state.busy || this.state.phase === 'blocked') return null
    const { id, signal } = this.begin(); this.publish({ busy: true, notice: '正在下载固定版本到当前设备。' })
    try { const blob = await this.api.download(selected, record, signal); if (!this.current(id)) return null; this.publish({ busy: false, notice: '下载内容已接收，将由当前浏览器保存。' }); return blob }
    catch (e) { if (!this.current(id) || isAborted(e) || this.unauthorized(e) || this.versionFault(e)) return null; this.publish({ busy: false, notice: '下载未完成，没有生成下载文件；请重新核对目录与授权。' }); return null }
  }
  leave(confirmed = false) { if (this.hasRiskyDraft() && !confirmed) return false; this.operation?.abort(); this.sequence++; this.publish(emptyMaterials); this.scopeOffsets = []; this.cursors = []; return true }
  dispose() { this.operation?.abort(); this.sequence++; this.csrf = ''; this.state = emptyMaterials; this.changed(this.state); this.disposed = true }
}
