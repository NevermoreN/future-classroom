import { ApiError, abortError, HttpApi, NetworkFault, readJson } from './client'
import { decimal, errorCode, ProtocolFault, secret } from './protocol'
import * as m from './material-protocol'
export interface AppendMaterial { commandId: string; uploadId: string; allowedClassIds: string[] }
export interface DeleteMaterial { commandId: string; materialId: string; resourceVersionId: string; expectedMaterialRevision: string }
export interface MaterialApi {
  materialScopes(offset: number, signal: AbortSignal): Promise<m.MaterialScopes>
  materialClasses(scope: m.MaterialScope, offset: number, signal: AbortSignal): Promise<m.MaterialClasses>
  materials(scope: m.MaterialScope, cursor: m.MaterialCursor | null, signal: AbortSignal): Promise<m.MaterialPage>
  createUpload(scope: m.MaterialScope, file: File, csrf: string, signal: AbortSignal): Promise<m.UploadReady>
  upload(id: string, file: File, csrf: string, signal: AbortSignal): Promise<void>
  cancelUpload(id: string, csrf: string, signal: AbortSignal): Promise<void>
  append(scope: m.MaterialScope, input: AppendMaterial, csrf: string, signal: AbortSignal): Promise<m.MaterialCommitted>
  deleteMaterial(scope: m.MaterialScope, input: DeleteMaterial, csrf: string, signal: AbortSignal): Promise<m.MaterialCommitted>
  materialOutcome(id: string, signal: AbortSignal): Promise<m.MaterialOutcome>
  download(scope: m.MaterialScope, record: m.MaterialRecord, signal: AbortSignal): Promise<Blob>
}
export class MaterialHttpApi extends HttpApi implements MaterialApi {
  async materialScopes(offset: number, signal: AbortSignal) {
    if (!Number.isSafeInteger(offset) || offset < 0 || offset > 4096) throw new ProtocolFault()
    const r = await this.request('materials/scopes/' + offset, signal); if (r.status !== 200) throw new ProtocolFault(); return m.scopes(r.value, offset)
  }
  async materialClasses(scope: m.MaterialScope, offset: number, signal: AbortSignal) {
    if (!Number.isSafeInteger(offset) || offset < 0 || offset > 128) throw new ProtocolFault()
    const r = await this.request('materials/classes/' + decimal(scope.ownerTeacherId) + '/' + decimal(scope.subjectId) + '/' + offset, signal); if (r.status !== 200) throw new ProtocolFault(); return m.classPage(r.value, scope, offset)
  }
  async materials(scope: m.MaterialScope, cursor: m.MaterialCursor | null, signal: AbortSignal) {
    const suffix = cursor ? '/' + decimal(cursor.folderId) + '/' + decimal(cursor.folderRevision) + '/' + cursor.offset : ''
    const r = await this.request(m.folderPath(scope) + suffix, signal); if (r.status !== 200) throw new ProtocolFault(); return m.page(r.value, scope, cursor)
  }
  async createUpload(scope: m.MaterialScope, file: File, csrf: string, signal: AbortSignal) {
    if (!file.size || file.size > m.fileLimit(file.name)) throw new ProtocolFault()
    const r = await this.request('materials/uploads/' + decimal(scope.ownerTeacherId) + '/' + decimal(scope.subjectId), signal, { schemaVersion: 1, displayName: m.fileName(file.name), bytes: file.size }, csrf)
    if (r.status !== 201) throw new ProtocolFault(); return m.ready(r.value)
  }
  private async binary<T>(path: string, signal: AbortSignal, perform: (response: Response, signal: AbortSignal) => Promise<T>, file?: File, csrf?: string): Promise<T> {
    if (signal.aborted) throw abortError()
    if (this.base.protocol !== 'https:') throw new ProtocolFault('请使用可信 HTTPS 地址，HTTP 预览不会传送文件或凭据。')
    const controller = new AbortController(), abort = () => controller.abort(); signal.addEventListener('abort', abort, { once: true }); const timer = setTimeout(abort, 120_000)
    try {
      const headers = new Headers({ Accept: file ? 'application/json' : 'application/octet-stream' })
      if (file) { headers.set('Content-Type', 'application/octet-stream'); headers.set('X-FC-CSRF', secret(csrf)) }
      const response = await this.fetcher(new URL(path, this.base), { method: file ? 'POST' : 'GET', credentials: 'same-origin', cache: 'no-store', redirect: 'error', referrerPolicy: 'same-origin', headers, signal: controller.signal, ...(file ? { body: file } : {}) })
      if (response.status === 401 || response.status === 403) { await response.body?.cancel().catch(() => undefined); throw new ApiError(response.status, response.status === 401 ? 'UNAUTHORIZED' : 'FORBIDDEN') }
      if (!response.ok) throw new ApiError(response.status, errorCode(await readJson(response, controller.signal)))
      return await perform(response, controller.signal)
    } catch (error) { if (signal.aborted) throw abortError(); if (error instanceof ApiError || error instanceof ProtocolFault) throw error; throw new NetworkFault() }
    finally { clearTimeout(timer); signal.removeEventListener('abort', abort) }
  }
  async upload(id: string, file: File, csrf: string, signal: AbortSignal) {
    if (!file.size || file.size > m.fileLimit(file.name)) throw new ProtocolFault()
    await this.binary('materials/uploads/' + decimal(id) + '/bytes', signal, async (r, s) => { if (r.status !== 200) throw new ProtocolFault(); m.uploaded(await readJson(r, s), id, file.size) }, file, csrf)
  }
  async cancelUpload(id: string, csrf: string, signal: AbortSignal) { const r = await this.request('materials/uploads/' + decimal(id) + '/cancel', signal, { schemaVersion: 1 }, csrf); if (r.status !== 200) throw new ProtocolFault(); m.cancelled(r.value, id) }
  async append(scope: m.MaterialScope, input: AppendMaterial, csrf: string, signal: AbortSignal) {
    const r = await this.request(m.folderPath(scope) + '/append', signal, { schemaVersion: 1, ...input }, csrf); if (r.status !== 200) throw new ProtocolFault(); return m.committed(r.value, input.commandId, scope)
  }
  async deleteMaterial(scope: m.MaterialScope, input: DeleteMaterial, csrf: string, signal: AbortSignal) {
    const r = await this.request(m.folderPath(scope) + '/delete', signal, { schemaVersion: 1, ...input }, csrf); if (r.status !== 200) throw new ProtocolFault(); return m.committed(r.value, input.commandId, scope)
  }
  async materialOutcome(id: string, signal: AbortSignal) { const r = await this.request('materials/commands/' + decimal(id), signal); if (r.status !== 200) throw new ProtocolFault(); return m.outcome(r.value, id) }
  async download(scope: m.MaterialScope, record: m.MaterialRecord, signal: AbortSignal): Promise<Blob> {
    record = m.record(record)
    return this.binary('materials/download/' + decimal(scope.ownerTeacherId) + '/' + decimal(scope.subjectId) + '/' + decimal(record.materialId) + '/' + decimal(record.resourceVersionId), signal, async (response, operation) => {
      if (response.status !== 200 || response.headers.get('content-type') !== 'application/octet-stream' || response.headers.get('content-length') !== String(record.bytes) || response.headers.get('x-fc-sha256') !== record.sha256 || !response.headers.get('content-disposition')?.startsWith('attachment;') || !response.body) { await response.body?.cancel(); throw new ProtocolFault() }
      const reader = response.body.getReader(), chunks: BlobPart[] = []; let total = 0
      const cancel = () => { void reader.cancel().catch(() => undefined) }; operation.addEventListener('abort', cancel, { once: true })
      try {
        while (true) { if (operation.aborted) throw abortError(); const { done, value } = await reader.read(); if (operation.aborted) throw abortError(); if (done) break; if (value.byteLength > record.bytes - total) { await reader.cancel(); throw new ProtocolFault() }; total += value.byteLength; chunks.push(new Uint8Array(value)) }
        if (total !== record.bytes) throw new ProtocolFault()
        return new Blob(chunks, { type: 'application/octet-stream' })
      } finally { operation.removeEventListener('abort', cancel); reader.releaseLock() }
    })
  }
}
