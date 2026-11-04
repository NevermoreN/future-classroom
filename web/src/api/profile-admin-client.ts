import { HttpApi, ApiError, NetworkFault, abortError, pageHeaders, readJson } from './client'
import { adminEnvelope, parseComponentStyle, parseLayoutData, type AdminDocument, type ComponentStyle, type LayoutData } from './admin-client'
import { decimal, errorCode, ProtocolFault, protocolVersions, secret } from './protocol'
export interface ProfileKey { classId: string; subjectId: string; teacherId: string }
export interface SubjectProfileData { exists: boolean; key: ProfileKey; enabled: boolean; layoutRevision: string; layout: LayoutData; folderId: string | null; materials: { component: ComponentStyle; visibleRows: number; rowGapDip: number } }
export interface ProfilePage { items: { id: string; key: ProfileKey; revision: string; enabled: boolean }[]; total: number; nextAfter: string | null }
export type ProfileCandidate = Pick<SubjectProfileData, 'enabled' | 'layout' | 'folderId' | 'materials'>
export type ContentSlot = { scope: 'public'; nodeId: string } | { scope: 'subject'; nodeId: string; key: ProfileKey }
export interface ContentImage { mediaVersionId: string; width: number; height: number; bytes: number; sha256: string; format: 'png' | 'jpeg' | 'bmp' }
export interface SlotContent { kind: 'text' | 'images'; text: string | null; pages: ContentImage[]; previousRevision: string | null; viewRevision: string; currentPage: number; pending: boolean; canPage?: boolean; updatedAt: string | null }
export type ImagePageInput = { source: 'existing'; mediaVersionId: string } | { source: 'upload'; uploadId: string }
export interface ImageUploadBatch { batchId: string; uploads: { id: string; name: string; bytes: number }[] }
export type ContentChange = { action: 'publishText'; candidate: { text: string } } | { action: 'clear'; candidate: Record<string, never> } | { action: 'undo'; candidate: { targetRevision: string } } | { action: 'page'; candidate: { page: number; expectedViewRevision: string } } | { action: 'publishImages'; candidate: { batchId: string | null; pages: ImagePageInput[] } }
function obj(value: unknown): asserts value is Record<string, unknown> { if (typeof value !== 'object' || value === null || Array.isArray(value)) throw new ProtocolFault() }
function bool(value: unknown): asserts value is boolean { if (typeof value !== 'boolean') throw new ProtocolFault() }
function text(value: unknown, max: number): asserts value is string { if (typeof value !== 'string' || [...value].length > max) throw new ProtocolFault() }
function number(value: unknown, max: number): asserts value is number { if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < 0 || value > max) throw new ProtocolFault() }
function revision(value: unknown): asserts value is string { if (value !== '0') decimal(value) }
function key(value: unknown): ProfileKey { obj(value); return { classId: decimal(value.classId), subjectId: decimal(value.subjectId), teacherId: decimal(value.teacherId) } }
function list(value: unknown): ProfilePage {
  obj(value); if (!Array.isArray(value.items) || value.items.length > 20) throw new ProtocolFault()
  number(value.total, 4096); if (value.nextAfter !== null) decimal(value.nextAfter)
  const items = value.items.map(item => { obj(item); revision(item.revision); bool(item.enabled); return { id: decimal(item.id), key: key(item.key), revision: item.revision, enabled: item.enabled } })
  return { items, total: value.total, nextAfter: value.nextAfter as string | null }
}
function profile(value: unknown): SubjectProfileData {
  obj(value); bool(value.exists); bool(value.enabled); revision(value.layoutRevision); const layout = parseLayoutData(value.layout)
  if (value.folderId !== null) decimal(value.folderId)
  obj(value.materials); const component = parseComponentStyle(value.materials.component); number(value.materials.visibleRows, 5); number(value.materials.rowGapDip, 64)
  return { exists: value.exists, key: key(value.key), enabled: value.enabled, layoutRevision: value.layoutRevision, layout, folderId: value.folderId as string | null, materials: { component, visibleRows: value.materials.visibleRows, rowGapDip: value.materials.rowGapDip } }
}
function content(value: unknown): SlotContent {
  obj(value); if (value.kind !== 'text' && value.kind !== 'images') throw new ProtocolFault()
  if (value.text !== null) { text(value.text, 10000); if (new TextEncoder().encode(value.text).length > 40000) throw new ProtocolFault() }
  if (!Array.isArray(value.pages) || value.pages.length > 50) throw new ProtocolFault()
  const pages = value.pages.map(page => { obj(page); decimal(page.mediaVersionId); number(page.width, 8000000); number(page.height, 8000000); number(page.bytes, 10 * 1024 * 1024); text(page.sha256, 64); if (!/^[0-9a-f]{64}$/.test(page.sha256) || !['png', 'jpeg', 'bmp'].includes(String(page.format))) throw new ProtocolFault(); return page as unknown as ContentImage })
  if (value.previousRevision !== null) revision(value.previousRevision)
  revision(value.viewRevision); number(value.currentPage, 49); bool(value.pending); if ('canPage' in value) bool(value.canPage); if (value.updatedAt !== null) text(value.updatedAt, 64)
  return { kind: value.kind, text: value.text as string | null, pages, previousRevision: value.previousRevision as string | null, viewRevision: value.viewRevision, currentPage: value.currentPage, pending: value.pending, ...('canPage' in value ? { canPage: value.canPage as boolean } : {}), updatedAt: value.updatedAt as string | null }
}
export function profilePath(value: ProfileKey): string { return `admin/profile/${decimal(value.classId)}/${decimal(value.subjectId)}/${decimal(value.teacherId)}` }
export function contentPath(value: ContentSlot): string { return value.scope === 'public' ? `admin/content/public/${decimal(value.nodeId)}` : `admin/content/subject/${decimal(value.key.classId)}/${decimal(value.key.subjectId)}/${decimal(value.key.teacherId)}/${decimal(value.nodeId)}` }
export class ProfileAdminHttpApi extends HttpApi {
  async profiles(after: string, signal: AbortSignal): Promise<AdminDocument<ProfilePage>> { revision(after); const result = await this.request('admin/profiles/' + after, signal); if (result.status !== 200) throw new ProtocolFault(); return adminEnvelope(result.value, list) }
  async profile(scope: ProfileKey, signal: AbortSignal): Promise<AdminDocument<SubjectProfileData>> { const result = await this.request(profilePath(scope), signal, undefined, undefined, 4 * 1024 * 1024); if (result.status !== 200) throw new ProtocolFault(); const document = adminEnvelope(result.value, profile); if (JSON.stringify(document.data.key) !== JSON.stringify(key(scope))) throw new ProtocolFault(); return document }
  async saveProfile(scope: ProfileKey, expectedRevision: string, candidate: ProfileCandidate, csrf: string, signal: AbortSignal): Promise<AdminDocument<SubjectProfileData>> { revision(expectedRevision); const result = await this.request(profilePath(scope), signal, { schemaVersion: 1, expectedRevision, action: 'save', candidate }, csrf, 4 * 1024 * 1024); if (result.status !== 200) throw new ProtocolFault(); return adminEnvelope(result.value, profile) }
  async readSlotContent(slot: ContentSlot, signal: AbortSignal): Promise<AdminDocument<SlotContent>> { const result = await this.request(contentPath(slot), signal); if (result.status !== 200) throw new ProtocolFault(); return adminEnvelope(result.value, content) }
  async writeContent(slot: ContentSlot, expectedRevision: string, change: ContentChange, csrf: string, signal: AbortSignal): Promise<AdminDocument<SlotContent>> { revision(expectedRevision); const result = await this.request(contentPath(slot), signal, { schemaVersion: 1, expectedRevision, ...change }, csrf); if (result.status !== 200) throw new ProtocolFault(); return adminEnvelope(result.value, content) }
  async createImageBatch(slot: ContentSlot, expectedRevision: string, files: readonly File[], csrf: string, signal: AbortSignal): Promise<ImageUploadBatch> {
    revision(expectedRevision)
    if (!files.length || files.length > 50 || files.some(file => !file.size || file.size > 10 * 1024 * 1024 || !/\.(?:png|jpe?g|bmp)$/i.test(file.name))) throw new ProtocolFault()
    const items = files.map(file => ({ name: file.name, bytes: file.size })), result = await this.request(contentPath(slot) + '/uploads', signal, { schemaVersion: 1, expectedRevision, items }, csrf)
    if (result.status !== 201) throw new ProtocolFault()
    obj(result.value); protocolVersions(result.value.schemaVersion)
    const batchId = decimal(result.value.batchId), uploads = result.value.uploads
    if (!Array.isArray(uploads) || uploads.length !== items.length) throw new ProtocolFault()
    const seen = new Set<string>()
    return { batchId, uploads: uploads.map((value, index) => { obj(value); const id = decimal(value.id); if (seen.has(id) || value.name !== items[index]!.name || value.bytes !== items[index]!.bytes) throw new ProtocolFault(); seen.add(id); return { id, ...items[index]! } }) }
  }
  async uploadImage(slot: ContentSlot, batchId: string, uploadId: string, file: File, csrf: string, signal: AbortSignal): Promise<void> {
    if (this.base.protocol !== 'https:' || file.size === 0 || file.size > 10 * 1024 * 1024) throw new ProtocolFault()
    if (signal.aborted) throw abortError()
    const owner = new AbortController(), abort = () => owner.abort(); signal.addEventListener('abort', abort, { once: true }); const timer = setTimeout(abort, 120000)
    try {
      for (let offset = 0; offset < file.size; offset += 512 * 1024) {
        const response = await this.fetcher(new URL(contentPath(slot) + '/uploads/' + decimal(batchId) + '/' + decimal(uploadId) + '/bytes', this.base), { method: 'POST', credentials: 'same-origin', cache: 'no-store', redirect: 'error', referrerPolicy: 'same-origin', headers: { ...pageHeaders(), 'X-FC-CSRF': secret(csrf), Accept: 'application/json', 'Content-Type': 'application/octet-stream', 'X-FC-Upload-Offset': String(offset) }, body: file.slice(offset, Math.min(offset + 512 * 1024, file.size)), signal: owner.signal })
      if (response.status === 401 || response.status === 403) { await response.body?.cancel().catch(() => undefined); throw new ApiError(response.status, response.status === 401 ? 'UNAUTHORIZED' : 'FORBIDDEN') }
      const value = await readJson(response, owner.signal)
      if (!response.ok) throw new ApiError(response.status, errorCode(value))
      obj(value); protocolVersions(value.schemaVersion)
      if (response.status !== 200 || value.status !== 'UPLOADED' || Object.keys(value).sort().join(',') !== 'schemaVersion,status') throw new ProtocolFault()
      }
    } catch (error) { if (signal.aborted) throw abortError(); if (error instanceof ApiError || error instanceof ProtocolFault) throw error; throw new NetworkFault() }
    finally { clearTimeout(timer); signal.removeEventListener('abort', abort) }
  }
  async cancelImageBatch(slot: ContentSlot, batchId: string, csrf: string, signal: AbortSignal): Promise<void> {
    const result = await this.request(contentPath(slot) + '/uploads/' + decimal(batchId) + '/cancel', signal, { schemaVersion: 1 }, csrf)
    obj(result.value); protocolVersions(result.value.schemaVersion)
    if (result.status !== 200 || result.value.status !== 'CANCELLED' || Object.keys(result.value).sort().join(',') !== 'schemaVersion,status') throw new ProtocolFault()
  }
  async image(slot: ContentSlot, image: ContentImage, signal: AbortSignal): Promise<Blob> {
    if (this.base.protocol !== 'https:') throw new ProtocolFault('请使用可信 HTTPS 地址读取图片。')
    if (signal.aborted) throw abortError()
    const owner = new AbortController(), abort = () => owner.abort(); signal.addEventListener('abort', abort, { once: true }); const timer = setTimeout(abort, 120000)
    try {
      const chunks: Uint8Array<ArrayBuffer>[] = [], type = image.format === 'jpeg' ? 'image/jpeg' : 'image/' + image.format
      if (!Number.isSafeInteger(image.bytes) || image.bytes < 1 || image.bytes > 10 * 1024 * 1024) throw new ProtocolFault()
      for (let offset = 0; offset < image.bytes; offset += 512 * 1024) {
        const end = Math.min(image.bytes - 1, offset + 512 * 1024 - 1), length = end - offset + 1
        const response = await this.fetcher(new URL(contentPath(slot) + '/media/' + decimal(image.mediaVersionId), this.base), { credentials: 'same-origin', cache: 'no-store', redirect: 'error', referrerPolicy: 'same-origin', headers: { ...pageHeaders(), Accept: 'image/png,image/jpeg,image/bmp', Range: `bytes=${offset}-${end}` }, signal: owner.signal })
        if (response.status === 401 || response.status === 403) { await response.body?.cancel().catch(() => undefined); throw new ApiError(response.status, response.status === 401 ? 'UNAUTHORIZED' : 'FORBIDDEN') }
        if (!response.body) throw new NetworkFault()
        if (response.status !== 206 || response.headers.get('content-type')?.split(';')[0] !== type || response.headers.get('content-length') !== String(length) || response.headers.get('content-range') !== `bytes ${offset}-${end}/${image.bytes}`) { await response.body.cancel(); throw new ProtocolFault() }
        const reader = response.body.getReader(); let bytes = 0
        try { while (true) { const next = await reader.read(); if (next.done) break; bytes += next.value.byteLength; if (bytes > length) { await reader.cancel(); throw new ProtocolFault() } chunks.push(new Uint8Array(next.value)) } } finally { reader.releaseLock() }
        if (bytes !== length) throw new ProtocolFault()
      }
      return new Blob(chunks, { type })
    } catch (error) { if (signal.aborted) throw abortError(); if (error instanceof ApiError || error instanceof ProtocolFault) throw error; throw new NetworkFault() }
    finally { clearTimeout(timer); signal.removeEventListener('abort', abort) }
  }
}
