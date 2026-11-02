import { commandRejected, commandRetired, ApiError, isAborted } from './client'
import { commandIdentity, sameCommand } from './command-protocol'
import { ProtocolFault, ProtocolVersionFault } from './protocol'
import { batchDeleteRecords, batchFiles, batchRecords, record as materialRecord, MAX_BATCH_FILES, type BatchDeleteCommitted, type BatchReady, type BatchFile, type BatchCommitted, fileLimit, sameScope, type MaterialScopes, type MaterialScopeItem, type MaterialRecord, type MaterialPage, type MaterialCursor, type UploadReady, type MaterialCommitted } from './material-protocol'
import type { AppendMaterialBatch, AppendMaterial, DeleteMaterial, DeleteMaterialBatch, MaterialApi } from './material-client'
export type PendingMaterial = ({ kind: 'APPEND'; scope: MaterialScopeItem; command: AppendMaterial; fileName: string; bytes: number } | { kind: 'DELETE'; scope: MaterialScopeItem; command: DeleteMaterial; original: MaterialRecord }) & { retired?: boolean }
export interface PendingMaterialBatch { scope: MaterialScopeItem; command: AppendMaterialBatch; files: (BatchFile & { uploadId: string })[]; retired?: boolean }
export interface PendingMaterialDeleteBatch { scope: MaterialScopeItem; command: DeleteMaterialBatch; originals: MaterialRecord[]; retired?: boolean }
export interface MaterialState { deleteSelection: MaterialRecord[]; pendingDeleteBatch: PendingMaterialDeleteBatch | null; deleteBatchHistory: MaterialRecord[] | null; files: File[]; batch: BatchReady | null; batchClasses: string[]; batchUploaded: number; pendingBatch: PendingMaterialBatch | null; batchHistory: MaterialRecord[] | null; scopes: MaterialScopes | null; selected: MaterialScopeItem | null; page: MaterialPage | null; file: File | null; classes: string[]; upload: UploadReady | null; phase: 'idle' | 'uploading' | 'upload-failed' | 'uploaded' | 'committing' | 'unknown' | 'result' | 'blocked'; pending: PendingMaterial | null; history: MaterialRecord | null; busy: boolean; notice: string; previousScopeOffset: number | null; previousCursor: MaterialCursor | null; canPrevious: boolean }
export const emptyMaterials: MaterialState = { deleteSelection: [], pendingDeleteBatch: null, deleteBatchHistory: null, files: [], batch: null, batchClasses: [], batchUploaded: 0, pendingBatch: null, batchHistory: null, scopes: null, selected: null, page: null, file: null, classes: [], upload: null, phase: 'idle', pending: null, history: null, busy: false, notice: '', previousScopeOffset: null, previousCursor: null, canPrevious: false }
export class MaterialController {
  private state = emptyMaterials
  private operation: AbortController | undefined
  private sequence = 0
  private disposed = false
  private incompatible = ''
  private scopeOffsets: number[] = []
  private cursors: (MaterialCursor | null)[] = []
  constructor(private api: MaterialApi, private csrf: string, private changed: (s: MaterialState) => void, private revoked: () => void) {}
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
    this.publish({ busy: false, phase: this.hasPending() ? 'unknown' : 'blocked', notice: this.hasPending() ? '提交结果未知。原命令、文件和选择已保留，只可手动查询，不会重发。' : '原文件和班级选择已保留，刷新页面前不能继续上传或提交。' })
    return true
  }
  private begin() { this.operation?.abort(); this.operation = new AbortController(); return { id: ++this.sequence, signal: this.operation.signal } }
  private current(id: number) { return !this.disposed && id === this.sequence }
  private unauthorized(e: unknown): boolean { if (!(e instanceof ApiError) || e.status !== 401 && e.status !== 403) return false; this.dispose(); this.revoked(); return true }
  private hasPending() { return !!this.state.pending || !!this.state.pendingBatch || !!this.state.pendingDeleteBatch }
  hasRiskyDraft() { return this.state.deleteSelection.length > 0 || this.state.files.length > 0 || !!this.state.batch || !!this.state.pendingBatch || !!this.state.file || !!this.state.upload || this.hasPending() || this.state.phase === 'uploading' || this.state.phase === 'committing' }
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
  async select(scope: MaterialScopeItem, confirmed = false) { if (this.incompatible || !this.state.scopes?.items.some(s => sameScope(s, scope)) || this.state.phase === 'blocked' || this.hasRiskyDraft() && !confirmed) return; this.cursors = []; this.publish({ selected: scope, page: null, deleteSelection: [], pendingDeleteBatch: null, deleteBatchHistory: null, files: [], batch: null, batchClasses: [], batchUploaded: 0, pendingBatch: null, batchHistory: null, file: null, upload: null, pending: null, history: null, classes: [], phase: 'idle' }); await this.loadPage(null, true) }
  async loadMoreClasses() {
    const selected = this.state.selected, offset = selected?.nextClassOffset
    if (this.incompatible || !selected || offset === null || offset === undefined || this.state.busy || this.hasPending() || this.state.phase === 'blocked' || this.disposed) return
    const { id, signal } = this.begin(); this.publish({ busy: true })
    try {
      const page = await this.api.materialClasses(selected, offset, signal); if (!this.current(id)) return
      if (!sameScope(page.scope, selected) || page.offset !== offset || page.total !== selected.classTotal || page.identityRevision !== this.state.scopes?.identityRevision || selected.classes.length !== offset || page.items.length === 0 && offset < page.total || page.items.some((c,i) => BigInt(c.classId) <= BigInt((i ? page.items[i-1] : selected.classes.at(-1))!.classId))) throw new ProtocolFault()
      this.publish({ busy: false, selected: { ...selected, classes: [...selected.classes, ...page.items], nextClassOffset: page.nextOffset }, notice: '已加载更多真实任教班级，请明确选择可见班级。' })
    } catch (e) { if (!this.current(id) || isAborted(e) || this.unauthorized(e) || this.versionFault(e)) return; this.publish({ busy: false, phase: 'blocked', notice: '任教班级或身份范围已变化，旧班级选择已停止使用。请重新加载范围，不会替换文件或自动提交。' }) }
  }
  async loadPage(cursor: MaterialCursor | null = null, confirmed = false) {
    const selected = this.state.selected; if (this.incompatible || !selected || this.disposed || this.hasRiskyDraft() && !confirmed) return
    const previous = this.state.page, previousPhase = this.state.phase, { id, signal } = this.begin(); this.publish({ busy: true, notice: '' })
    try {
      const page = await this.api.materials(selected, cursor, signal); if (!this.current(id)) return
      if (!sameScope(page.scope, selected)) throw new ProtocolFault()
      if (!cursor) this.cursors = []; else if (previous?.nextCursor && cursor.offset === previous.nextCursor.offset) this.cursors.push(previous.offset === 0 ? null : { folderId: previous.folderId, folderRevision: previous.folderRevision, offset: previous.offset }); else if (this.cursors.length) this.cursors.pop()
      const keptBatch = this.state.pendingBatch ? { classes: this.state.classes, files: this.state.files, batch: this.state.batch, batchClasses: this.state.batchClasses, batchUploaded: this.state.batchUploaded, pendingBatch: this.state.pendingBatch, batchHistory: this.state.batchHistory, phase: this.state.phase } : { files: [], batch: null, batchClasses: [], batchUploaded: 0, pendingBatch: null, batchHistory: null }
      const keptDelete = this.state.pendingDeleteBatch ? { pendingDeleteBatch: this.state.pendingDeleteBatch, deleteBatchHistory: this.state.deleteBatchHistory, phase: this.state.phase } : { pendingDeleteBatch: null, deleteBatchHistory: null }
      this.publish({ busy: false, page, deleteSelection: [], file: null, classes: [], upload: null, pending: null, history: null, phase: 'idle', previousCursor: this.cursors.at(-1) ?? null, canPrevious: this.cursors.length > 0, ...keptBatch, ...keptDelete })
    } catch (e) { if (!this.current(id) || isAborted(e) || this.unauthorized(e) || this.versionFault(e)) return; this.publish({ busy: false, page: null, phase: this.state.pendingBatch || this.state.pendingDeleteBatch ? previousPhase : 'blocked', notice: e instanceof ApiError && e.code === 'STALE_CURSOR' ? '目录已变化，旧分页已停止。请明确从第一页重读。' : '无法读取目录，请重新核对范围。' }) }
  }
  pick(file: File | null) { if (this.state.deleteSelection.length || this.incompatible || this.state.busy || this.hasPending() || this.state.upload || this.state.files.length > 0 || this.state.batch || this.state.phase === 'blocked') return; let valid = true; try { valid = !file || file.size > 0 && file.size <= fileLimit(file.name) } catch { valid = false }; if (!valid) { this.publish({ file: null, upload: null, phase: 'idle', notice: '请选择允许格式的本机文件：单文件最多50MiB，图片最多10MiB；图片完整解码与8MP校验由白板执行。' }); return }; this.publish({ file, upload: null, phase: 'idle', notice: '' }) }
  setClasses(ids: string[]) { const selected = this.state.selected; if (this.state.batch || this.incompatible || !selected || this.state.busy || this.hasPending() || this.state.phase === 'blocked') return; if (new Set(ids).size !== ids.length || ids.some(id => !selected.classes.some(c => c.classId === id))) return; this.publish({ classes: ids.slice().sort((a,b) => BigInt(a) < BigInt(b) ? -1 : 1) }) }
  async uploadFile() {
    const { selected, file } = this.state; if (this.incompatible || !selected || !file || this.state.busy || this.hasPending() || this.state.phase === 'blocked') return
    const { id, signal } = this.begin(); this.publish({ busy: true, phase: 'uploading', upload: null, notice: '正在上传文件，尚未添加到课堂目录。' })
    try { const receipt = await this.api.createUpload(selected, file, this.csrf, signal); if (!this.current(id)) return; this.publish({ upload: receipt }); await this.api.upload(receipt.uploadId, file, this.csrf, signal); if (!this.current(id)) return; this.publish({ busy: false, phase: 'uploaded', upload: receipt, notice: '文件传输已完成，尚未添加。选择可见班级后，点击“校验并添加”。' }) }
    catch (e) { if (!this.current(id) || isAborted(e) || this.unauthorized(e) || this.versionFault(e)) return; this.publish({ busy: false, phase: 'idle', upload: null, notice: '上传未确认，尚未发送添加命令。临时文件由白板到期清理；可以明确重新上传，不会自动重试。' }) }
  }
  async cancelUpload() {
    if (this.incompatible || this.state.batch || this.state.files.length || !['uploading', 'uploaded'].includes(this.state.phase) || this.hasPending()) return
    const receipt = this.state.upload; this.operation?.abort(); this.sequence++
    this.publish({ busy: false, phase: 'idle', upload: null, notice: '已停止本页上传，未发送添加命令。未知临时内容会到期清理。' })
    if (!receipt) return
    const { id, signal } = this.begin(); this.publish({ busy: true, notice: '正在请求清理原临时文件，此操作不会撤回已保存资料。' })
    try { await this.api.cancelUpload(receipt.uploadId, this.csrf, signal); if (!this.current(id)) return; this.publish({ busy: false, notice: '白板已确认取消原临时文件；未发送添加命令。' }) }
    catch (e) { if (!this.current(id) || isAborted(e) || this.unauthorized(e) || this.versionFault(e)) return; this.publish({ busy: false, notice: '临时文件取消结果未确认。不会自动重试，白板将按期限清理；未发送添加命令。' }) }
  }
  async append() {
    const { selected, file, upload, classes } = this.state; if (this.incompatible || !selected || !file || !upload || !classes.length || this.hasPending() || this.state.deleteSelection.length || this.state.busy || this.state.phase !== 'uploaded') return
    await this.issueAndSubmit({ kind: 'APPEND', scope: { ...selected }, command: { uploadId: upload.uploadId, allowedClassIds: classes.slice() }, fileName: file.name, bytes: file.size })
  }
  async delete(record: MaterialRecord) {
    const selected = this.state.selected; if (this.incompatible || !selected || this.state.busy || this.hasRiskyDraft() || !this.state.page?.items.some(r => r.materialId === record.materialId && r.revision === record.revision && r.resourceVersionId === record.resourceVersionId)) return
    await this.issueAndSubmit({ kind: 'DELETE', scope: { ...selected }, command: { materialId: record.materialId, resourceVersionId: record.resourceVersionId, expectedMaterialRevision: record.revision }, original: { ...record, allowedClassIds: record.allowedClassIds.slice() } })
  }
  private async issueAndSubmit(captured: { kind: 'APPEND'; scope: MaterialScopeItem; command: Omit<AppendMaterial, 'commandId' | 'commandTicket'>; fileName: string; bytes: number } | { kind: 'DELETE'; scope: MaterialScopeItem; command: Omit<DeleteMaterial, 'commandId' | 'commandTicket'>; original: MaterialRecord }) {
    const { id, signal } = this.begin(); this.publish({ busy: true, phase: 'committing', notice: '正在领取原操作的服务器票据，尚未提交目录变更。' })
    try {
      const pair = commandIdentity(await this.api.issueCommand('materials', this.csrf, signal))
      if (!this.current(id)) return
      const pending: PendingMaterial = captured.kind === 'APPEND' ? { ...captured, command: { ...captured.command, ...pair } } : { ...captured, command: { ...captured.command, ...pair } }
      await this.submit(pending)
    } catch (error) {
      if (!this.current(id) || isAborted(error) || this.unauthorized(error) || this.versionFault(error)) return
      this.publish({ busy: false, phase: captured.kind === 'APPEND' ? 'uploaded' : 'idle', notice: '未取得可用服务器票据，目录变更尚未提交。原文件和选择保留，请明确操作，不会自动重试。' })
    }
  }
  private verify(result: MaterialCommitted, p: PendingMaterial) {
    if (!sameCommand(result, p.command) || !sameScope(result.scope, p.scope)) throw new ProtocolFault()
    const r = result.record
    if (p.kind === 'DELETE') {
      const o = p.original
      this.verifyDeleted(r, o)
    } else if (r.revision !== '1' || r.resourceVersionId !== p.command.uploadId || r.displayName !== p.fileName || r.bytes !== p.bytes || r.status !== 'PUBLISHED' || r.allowedClassIds.length !== p.command.allowedClassIds.length || r.allowedClassIds.some((id,i) => id !== p.command.allowedClassIds[i])) throw new ProtocolFault()
  }
  private async submit(pending: PendingMaterial) {
    if (this.incompatible || this.disposed) return
    const { id, signal } = this.begin(); this.publish({ busy: true, phase: 'committing', pending, history: null, notice: '正在校验并保存此次命令，不会自动重复提交。' })
    try { const result = pending.kind === 'APPEND' ? await this.api.append(pending.scope, pending.command, this.csrf, signal) : await this.api.deleteMaterial(pending.scope, pending.command, this.csrf, signal); if (!this.current(id)) return; this.verify(result, pending); this.publish({ busy: false, phase: 'result', history: result.record, file: null, upload: null, notice: '白板已确认此次命令持久保存。请明确重新读取目录核对；此结果不代表当前正在屏幕显示。' }) }
    catch (e) { if (!this.current(id) || isAborted(e) || this.unauthorized(e) || this.versionFault(e)) return; const definite = commandRejected(e), retired = commandRetired(e); this.publish({ busy: false, phase: definite ? 'blocked' : 'unknown', pending: retired ? { ...pending, retired: true } : definite ? null : pending, notice: retired ? '原票据已过期或未获发行，已停止此操作。原文件、选择和票据保留，请明确重读目录核对。' : definite ? '此次命令被拒绝。请重新核对目录、权限或临时文件有效期，再明确操作。' : '提交结果未知。原命令保留在本页，只能手动查询；不会自动重发或生成新命令。' }) }
  }
  async query() {
    const p = this.state.pending; if (!p || p.retired || this.state.busy || !['unknown', 'result', 'blocked'].includes(this.state.phase)) return
    const { id, signal } = this.begin(); this.publish({ busy: true })
    try { const result = await this.api.materialOutcome(commandIdentity(p.command), signal); if (!this.current(id)) return; if (!sameCommand(result, p.command)) throw new ProtocolFault(); if (result.state === 'NOT_FOUND') { this.publish({ busy: false, phase: 'unknown', notice: '当前未查到原命令，不证明从未提交；不会因此重发。' }); return }; this.verify(result, p); this.publish({ busy: false, phase: 'result', history: result.record, ...(this.incompatible ? {} : { file: null, upload: null }), notice: '已查到原命令保存结果。它可能已被后续操作替代，请明确读取当前目录。' }) }
    catch (e) { if (!this.current(id) || isAborted(e) || this.unauthorized(e) || this.versionFault(e)) return; if (commandRetired(e)) { this.publish({ busy: false, phase: 'blocked', pending: { ...p, retired: true }, notice: '原票据已过期或未获发行，已停止查询。请明确读取当前目录核对。' }); return }; this.publish({ busy: false, phase: 'unknown', notice: '查询未能确认原命令，请稍后主动查询。不会重发。' }) }
  }
  pickFiles(files: readonly File[]) {
    if (this.state.deleteSelection.length || this.disposed || this.incompatible || this.state.busy || this.hasPending() || this.state.batch || this.state.file || this.state.upload || this.state.phase === 'blocked') return
    try { batchFiles(files.map(file => ({ displayName: file.name, bytes: file.size }))) }
    catch { this.publish({ notice: '一次请选择1–20个支持的文件，单件最多50MiB，图片最多10MiB。原选择没有改变。' }); return }
    this.publish({ files: files.slice(), batchUploaded: 0, batchHistory: null, phase: 'idle', notice: '请检查下方顺序和可见班级；最后一项添加后会排在目录最前面。' })
  }
  moveFile(index: number, direction: -1 | 1) {
    if (!this.canEditBatch() || direction !== -1 && direction !== 1 || !Number.isInteger(index) || index < 0 || index >= this.state.files.length || index + direction < 0 || index + direction >= this.state.files.length) return
    const files = this.state.files.slice(); [files[index], files[index + direction]] = [files[index + direction]!, files[index]!]; this.publish({ files })
  }
  removeFile(index: number) {
    if (!this.canEditBatch() || !Number.isInteger(index) || index < 0 || index >= this.state.files.length) return
    this.publish({ files: this.state.files.filter((_, i) => i !== index) })
  }
  private canEditBatch() { return !this.disposed && !this.incompatible && !this.state.busy && !this.hasPending() && !this.state.deleteSelection.length && !this.state.batch && this.state.phase !== 'blocked' }
  async uploadBatch() {
    const { selected, files, classes } = this.state
    if (!selected || !files.length || files.length > MAX_BATCH_FILES || !classes.length || !this.canEditBatch() || this.state.file || this.state.upload) return
    const original = files.slice(), allowed = classes.slice(), manifest = batchFiles(original.map(f => ({ displayName: f.name, bytes: f.size })))
    const { id, signal } = this.begin(); this.publish({ busy: true, phase: 'uploading', batchUploaded: 0, batchClasses: allowed, notice: '按已确认顺序上传，全部完成前不会添加到目录。' })
    try {
      const batch = await this.api.createBatch(selected, manifest, allowed, this.csrf, signal)
      if (!this.current(id)) return
      if (batch.items.length !== original.length || new Set(batch.items.map(f => f.uploadId)).size !== original.length || batch.items.some((f, i) => f.displayName !== manifest[i]!.displayName || f.bytes !== manifest[i]!.bytes)) throw new ProtocolFault()
      this.publish({ batch })
      for (const [i, file] of original.entries()) {
        await this.api.uploadBatchMember(batch.batchId, batch.items[i]!.uploadId, file, this.csrf, signal)
        if (!this.current(id)) return
        this.publish({ batchUploaded: i + 1, notice: `已上传 ${i + 1}/${original.length}，尚未添加到目录。` })
      }
      this.publish({ busy: false, phase: 'uploaded', notice: '全部文件已上传。请点击“校验并整批添加”，只会保存一次。' })
    } catch (e) {
      if (!this.current(id) || isAborted(e) || this.unauthorized(e) || this.versionFault(e)) return
      this.publish({ busy: false, phase: this.state.batch ? 'upload-failed' : 'idle', notice: this.state.batch ? '上传未完成，整批尚未添加。请取消这批上传后再明确操作，不会自动重传。' : '没有收到上传确认，尚未添加。临时内容由白板到期清理，不会自动重试。' })
    }
  }
  async cancelBatch() {
    if (this.incompatible || this.disposed || this.hasPending() || !['uploading', 'uploaded', 'upload-failed'].includes(this.state.phase)) return
    const batch = this.state.batch; this.operation?.abort(); this.sequence++
    this.publish({ busy: false, phase: batch ? 'upload-failed' : 'idle', notice: '已停止上传，整批尚未添加。' })
    if (!batch) return
    const { id, signal } = this.begin(); this.publish({ busy: true })
    try { await this.api.cancelBatch(batch.batchId, this.csrf, signal); if (!this.current(id)) return; this.publish({ busy: false, phase: 'idle', batch: null, batchClasses: [], batchUploaded: 0, notice: '白板已清理这批临时文件，尚未添加到目录。' }) }
    catch (e) { if (!this.current(id) || isAborted(e) || this.unauthorized(e) || this.versionFault(e)) return; this.publish({ busy: false, phase: 'upload-failed', notice: '清理结果尚未确认，这批文件仍保留原编号；不会重新上传或添加，可再次明确请求清理。' }) }
  }
  async appendBatch() {
    const { selected, batch, files, batchClasses } = this.state
    if (this.incompatible || this.disposed || !selected || !batch || !files.length || !batchClasses.length || this.hasPending() || this.state.busy || this.state.phase !== 'uploaded' || this.state.batchUploaded !== files.length) return
    const captured = { scope: { ...selected }, files: batch.items.map(f => ({ ...f })), command: { batchId: batch.batchId, uploadIds: batch.items.map(f => f.uploadId), allowedClassIds: batchClasses.slice() } }
    const { id, signal } = this.begin(); this.publish({ busy: true, phase: 'committing', notice: '正在准备整批添加，尚未收到保存结果。' })
    try {
      const pair = commandIdentity(await this.api.issueCommand('materials', this.csrf, signal)); if (!this.current(id)) return
      const pending: PendingMaterialBatch = { ...captured, command: { ...captured.command, ...pair } }
      await this.submitBatch(pending)
    } catch (e) { if (!this.current(id) || isAborted(e) || this.unauthorized(e) || this.versionFault(e)) return; this.publish({ busy: false, phase: 'uploaded', notice: '白板暂时不能接收这次添加，整批尚未提交。文件与选择保留，请稍后明确操作。' }) }
  }
  private verifyBatch(result: BatchCommitted, p: PendingMaterialBatch) {
    if (!sameCommand(result, p.command) || !sameScope(result.scope, p.scope)) throw new ProtocolFault()
    const records = batchRecords(result.records)
    if (records.length !== p.files.length || records.some((r, i) => { const f = p.files[i]!; return r.resourceVersionId !== f.uploadId || r.displayName !== f.displayName || r.bytes !== f.bytes || r.allowedClassIds.length !== p.command.allowedClassIds.length || r.allowedClassIds.some((c, n) => c !== p.command.allowedClassIds[n]) })) throw new ProtocolFault()
    return records
  }
  private async submitBatch(pending: PendingMaterialBatch) {
    const { id, signal } = this.begin(); this.publish({ busy: true, phase: 'committing', pendingBatch: pending, batchHistory: null, notice: '正在校验并整批保存，不会自动重复提交。' })
    try {
      const result = await this.api.appendBatch(pending.scope, pending.command, this.csrf, signal); if (!this.current(id)) return
      const records = this.verifyBatch(result, pending); this.publish({ busy: false, phase: 'result', batchHistory: records, files: [], batch: null, notice: '这一批已全部保存。下方是本次结果；请另行读取当前目录核对。' })
    } catch (e) {
      if (!this.current(id) || isAborted(e) || this.unauthorized(e) || this.versionFault(e)) return
      const retired = commandRetired(e), definite = commandRejected(e)
      this.publish({ busy: false, phase: definite ? 'blocked' : 'unknown', pendingBatch: retired ? { ...pending, retired: true } : definite ? null : pending, notice: retired ? '原操作已过期或未获发行，已停止。保留原文件，请读取当前目录核对，不会重新添加。' : definite ? '整批添加被拒绝，请核对权限、文件或目录；不会自动重试。' : '这批保存结果未知。原文件与顺序已保留，只能手动查询，不会重新添加。' })
    }
  }
  async queryBatch() {
    const p = this.state.pendingBatch; if (!p || p.retired || this.disposed || this.state.busy || !['unknown', 'result', 'blocked'].includes(this.state.phase)) return
    const { id, signal } = this.begin(); this.publish({ busy: true })
    try {
      const result = await this.api.batchOutcome(p.scope, commandIdentity(p.command), signal); if (!this.current(id)) return
      if (!sameCommand(result, p.command)) throw new ProtocolFault()
      if (result.state === 'NOT_FOUND') { this.publish({ busy: false, phase: 'unknown', notice: '尚未查到这批结果，不证明没有保存；不会重新添加。' }); return }
      const records = this.verifyBatch(result, p); this.publish({ busy: false, phase: 'result', batchHistory: records, ...(this.incompatible ? {} : { files: [], batch: null }), notice: '已查到原批次全部保存的结果。它与当前目录分开显示，不会重新添加。' })
    } catch (e) {
      if (!this.current(id) || isAborted(e) || this.unauthorized(e) || this.versionFault(e)) return
      if (commandRetired(e)) { this.publish({ busy: false, phase: 'blocked', pendingBatch: { ...p, retired: true }, notice: '原操作已过期或未获发行，已停止查询。请读取当前目录核对。' }); return }
      this.publish({ busy: false, phase: 'unknown', notice: '仍无法确认这批结果，请稍后手动查询，不会重新添加。' })
    }
  }
  private canSelectForDeletion() {
    return !this.disposed && !this.incompatible && !!this.state.selected && !this.state.busy && !this.hasPending() && !this.state.files.length && !this.state.batch && !this.state.file && !this.state.upload && this.state.phase !== 'blocked'
  }
  toggleDelete(record: MaterialRecord, checked: boolean) {
    if (!this.canSelectForDeletion()) return
    const current = this.state.page?.items.find(r => r.materialId === record.materialId && r.resourceVersionId === record.resourceVersionId && r.revision === record.revision)
    if (!current) return
    const selection = this.state.deleteSelection.filter(r => r.materialId !== current.materialId)
    if (checked) selection.push(materialRecord(current))
    if (selection.length > MAX_BATCH_FILES) return
    this.publish({ deleteSelection: selection, deleteBatchHistory: null, notice: '' })
  }
  selectPageForDeletion() {
    if (!this.canSelectForDeletion() || !this.state.page?.items.length || this.state.page.items.length > MAX_BATCH_FILES) return
    this.publish({ deleteSelection: this.state.page.items.map(materialRecord), deleteBatchHistory: null, notice: '' })
  }
  clearDeleteSelection() { if (this.canSelectForDeletion()) this.publish({ deleteSelection: [], notice: '' }) }
  private verifyDeleted(r: MaterialRecord, o: MaterialRecord) {
    if (r.materialId !== o.materialId || r.folderId !== o.folderId || r.displayName !== o.displayName || r.resourceVersionId !== o.resourceVersionId || r.kind !== o.kind || r.bytes !== o.bytes || r.sha256 !== o.sha256 || r.addedAt !== o.addedAt || r.addedSequence !== o.addedSequence || r.allowedClassIds.length !== o.allowedClassIds.length || r.allowedClassIds.some((id, i) => id !== o.allowedClassIds[i]) || r.revision !== (BigInt(o.revision) + 1n).toString() || r.status !== 'DELETED') throw new ProtocolFault()
  }
  private verifyDeleteBatch(result: BatchDeleteCommitted, pending: PendingMaterialDeleteBatch) {
    if (!sameCommand(result, pending.command) || !sameScope(result.scope, pending.scope)) throw new ProtocolFault()
    const records = batchDeleteRecords(result.records)
    if (records.length !== pending.originals.length) throw new ProtocolFault()
    records.forEach((r, i) => this.verifyDeleted(r, pending.originals[i]!))
    return records
  }
  async deleteBatch() {
    if (!this.canSelectForDeletion() || !this.state.deleteSelection.length) return
    const selected = this.state.selected!, originals = this.state.deleteSelection.map(materialRecord)
    if (originals.some(o => !this.state.page?.items.some(r => r.materialId === o.materialId && r.resourceVersionId === o.resourceVersionId && r.revision === o.revision))) return
    const captured = { scope: { ...selected }, originals, targets: originals.map(r => ({ materialId: r.materialId, resourceVersionId: r.resourceVersionId, expectedMaterialRevision: r.revision })) }
    const { id, signal } = this.begin(); this.publish({ busy: true, phase: 'committing', notice: '正在准备删除所选资料，尚未收到保存结果。' })
    try {
      const pair = commandIdentity(await this.api.issueCommand('materials', this.csrf, signal)); if (!this.current(id)) return
      await this.submitDeleteBatch({ scope: captured.scope, originals: captured.originals, command: { ...pair, targets: captured.targets } })
    } catch (e) { if (!this.current(id) || isAborted(e) || this.unauthorized(e) || this.versionFault(e)) return; this.publish({ busy: false, phase: 'idle', notice: '暂时不能接收这次删除，尚未提交。原选择保留，请稍后明确操作。' }) }
  }
  private async submitDeleteBatch(pending: PendingMaterialDeleteBatch) {
    const { id, signal } = this.begin(); this.publish({ busy: true, phase: 'committing', pendingDeleteBatch: pending, deleteBatchHistory: null, notice: '正在一起删除所选资料，不会自动重复提交。' })
    try {
      const result = await this.api.deleteBatch(pending.scope, pending.command, this.csrf, signal); if (!this.current(id)) return
      const records = this.verifyDeleteBatch(result, pending)
      this.publish({ busy: false, phase: 'result', deleteSelection: [], deleteBatchHistory: records, notice: '所选资料已全部删除。请另行读取当前目录核对。' })
    } catch (e) {
      if (!this.current(id) || isAborted(e) || this.unauthorized(e) || this.versionFault(e)) return
      const definite = commandRejected(e), retired = commandRetired(e)
      this.publish({ busy: false, phase: definite ? 'blocked' : 'unknown', pendingDeleteBatch: retired ? { ...pending, retired: true } : definite ? null : pending, notice: retired ? '这次操作已过期或未获接收，请读取当前目录核对。不会重新删除。' : definite ? '整批删除被拒绝，请重新核对目录与权限；不会自动重试。' : '删除结果未知。原选择已保留，只能手动查询这次结果，不会重新删除。' })
    }
  }
  async queryDeleteBatch() {
    const pending = this.state.pendingDeleteBatch
    if (!pending || pending.retired || this.disposed || this.state.busy || !['unknown', 'result', 'blocked'].includes(this.state.phase)) return
    const { id, signal } = this.begin(); this.publish({ busy: true })
    try {
      const result = await this.api.deleteBatchOutcome(pending.scope, commandIdentity(pending.command), signal); if (!this.current(id)) return
      if (!sameCommand(result, pending.command)) throw new ProtocolFault()
      if (result.state === 'NOT_FOUND') { this.publish({ busy: false, phase: 'unknown', notice: '尚未查到这次删除结果，不证明没有保存；不会重新删除。' }); return }
      const records = this.verifyDeleteBatch(result, pending)
      this.publish({ busy: false, phase: 'result', deleteBatchHistory: records, deleteSelection: [], notice: '已查到原操作全部删除的结果。它与当前目录分开显示。' })
    } catch (e) {
      if (!this.current(id) || isAborted(e) || this.unauthorized(e) || this.versionFault(e)) return
      if (commandRetired(e)) { this.publish({ busy: false, phase: 'blocked', pendingDeleteBatch: { ...pending, retired: true }, notice: '这次操作已过期或未获接收，已停止查询。请读取当前目录核对。' }); return }
      this.publish({ busy: false, phase: 'unknown', notice: '仍无法确认这次删除，请稍后手动查询，不会重新删除。' })
    }
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
