import { useEffect, useRef, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Field, Section } from './AdminForms'
import { SubjectScopePicker, completeProfileKey, emptyProfileKey } from './SubjectProfilesPanel'
import { useResource } from './useAdminResource'
import { AdminHttpApi, type AdminDocument, type LayoutData } from './api/admin-client'
import { ProfileAdminHttpApi, type ContentChange, type ContentImage, type ContentSlot, type ProfileKey, type SlotContent, type ImageUploadBatch, type ImagePageInput } from './api/profile-admin-client'
import { ApiError, isAborted, trustedApiBase } from './api/client'
import { ProtocolFault, type Session } from './api/protocol'
export function validateIndependentText(text: string): void {
  if ([...text].length > 10000 || new TextEncoder().encode(text).length > 40000 || /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f-\u009f]/u.test(text)) throw new Error('正文最多 10,000 个字符、40,000 字节，只接受正常换行和制表符。')
  if (new TextDecoder('utf-8', { fatal: true }).decode(new TextEncoder().encode(text)) !== text) throw new Error('正文包含无法保存的文字编码。')
}
export function decodeIndependentTxt(bytes: Uint8Array): string {
  if (bytes.length > 40003) throw new Error('TXT 超过文字组件容量，请缩小后导入。')
  const encoding = bytes[0] === 255 && bytes[1] === 254 ? 'utf-16le' : bytes[0] === 254 && bytes[1] === 255 ? 'utf-16be' : 'utf-8'
  let text: string
  try { text = new TextDecoder(encoding, { fatal: true }).decode(bytes) } catch { throw new Error('TXT 编码无效；请使用 UTF-8，或带 BOM 的 UTF-16 文件。') }
  validateIndependentText(text); return text
}
function ImagePreview({ api, slot, image, revoked }: { api: ProfileAdminHttpApi; slot: ContentSlot; image: ContentImage; revoked: () => void }) {
  const [url, setUrl] = useState(''), [notice, setNotice] = useState('')
  useEffect(() => {
    const owner = new AbortController(); let objectUrl: string | undefined
    setUrl(''); setNotice('正在读取已保存的原图…')
    void api.image(slot, image, owner.signal).then(blob => { if (owner.signal.aborted) return; objectUrl = URL.createObjectURL(blob); setUrl(objectUrl); setNotice('') }).catch(error => { if (isAborted(error)) return; if (error instanceof ApiError && error.status === 401) revoked(); else setNotice('原图读取未完成，请重新读取内容后重试。') })
    return () => { owner.abort(); if (objectUrl) URL.revokeObjectURL(objectUrl) }
  }, [api, slot, image.mediaVersionId])
  return <div className="independent-image-preview">{url ? <img src={url} alt={`已保存图片，${image.width} × ${image.height} 像素`} /> : <p role="status">{notice}</p>}</div>
}
type DraftImage = { source: 'existing'; image: ContentImage } | { source: 'upload'; file: File; url: string }
export default function IndependentContentPanel({ session, onRevoked, onDirty }: { session: Session; onRevoked: () => void; onDirty: (dirty: boolean) => void }) {
  const identity = useResource('identity', session, onRevoked), [api] = useState(() => new ProfileAdminHttpApi(trustedApiBase(import.meta.env.BASE_URL, window.location.href))), [admin] = useState(() => new AdminHttpApi(trustedApiBase(import.meta.env.BASE_URL, window.location.href)))
  const [scope, setScope] = useState<'public' | 'subject'>('public'), [key, setKey] = useState<ProfileKey>(emptyProfileKey), [layout, setLayout] = useState<LayoutData | null>(null), [layoutScope, setLayoutScope] = useState<{ scope: 'public' } | { scope: 'subject'; key: ProfileKey }>({ scope: 'public' }), [slot, setSlot] = useState<ContentSlot | null>(null), [document, setDocument] = useState<AdminDocument<SlotContent> | null>(null), [draft, setDraft] = useState(''), [dirty, setDirty] = useState(false), [busy, setBusy] = useState(false), [uncertain, setUncertain] = useState(false), [notice, setNotice] = useState(''), [previewPage, setPreviewPage] = useState(0), [imagePages, setImagePages] = useState<DraftImage[]>([]), [imageBatchId, setImageBatchId] = useState<string | null>(null), [imageUploading, setImageUploading] = useState(false)
  const operation = useRef<AbortController | null>(null), live = useRef(true), locked = useRef(false), selection = useRef(0), imageUrls = useRef(new Set<string>()), imageBatch = useRef<{ slot: ContentSlot; batch: ImageUploadBatch } | null>(null)
  useEffect(() => { live.current = true; return () => { live.current = false; operation.current?.abort(); imageUrls.current.forEach(url => URL.revokeObjectURL(url)); imageUrls.current.clear() } }, [])
  function resetImages(data: SlotContent) { imageUrls.current.forEach(url => URL.revokeObjectURL(url)); imageUrls.current.clear(); setImagePages(data.pages.map(image => ({ source: 'existing', image }))) }
  function consent() { return !dirty || window.confirm('当前文字草稿尚未保存，确认放弃并读取其他内容？') }
  function failed(error: unknown, write: boolean) {
    if (isAborted(error)) return
    if (error instanceof ApiError && error.status === 401) { onRevoked(); return }
    if (error instanceof ApiError && error.status === 403) { setNotice('当前账号无权读取或修改此内容，登录已保留。'); return }
    if (write && (!(error instanceof ApiError) || error.status === 409)) { setUncertain(true); setNotice('提交结果尚未确认或内容已有变化。请重新读取并核对，不会自动重复提交。'); return }
    setNotice(error instanceof ProtocolFault ? error.message : error instanceof Error && !(error instanceof ApiError) ? error.message : '内容操作未完成，请检查节点类型、容量和当前版本。')
  }
  async function chooseScope() {
    if (imageBatch.current) { setNotice('请先取消并清理当前图片上传批次，再切换布局。'); return }
    if (locked.current || scope === 'subject' && !completeProfileKey(key) || !consent()) return
    const controller = new AbortController(); operation.current?.abort(); operation.current = controller; locked.current = true; setBusy(true); setNotice('')
    try {
      const next = scope === 'public' ? (await admin.read('layout', controller.signal)).data : (await api.profile(key, controller.signal)).data.layout
      if (live.current) { selection.current++; setLayout(next); setLayoutScope(scope === 'public' ? { scope } : { scope, key: { ...key } }); setSlot(null); setDocument(null); imageUrls.current.forEach(url => URL.revokeObjectURL(url)); imageUrls.current.clear(); setImagePages([]); setDirty(false); onDirty(false); setUncertain(false) }
    } catch (error) { if (live.current) failed(error, false) }
    finally { locked.current = false; if (live.current) setBusy(false) }
  }
  async function load(target: ContentSlot) {
    if (imageBatch.current && slot?.nodeId !== target.nodeId) { setNotice('请先取消当前图片上传批次，再切换节点。'); return }
    if (locked.current || !consent()) return
    const controller = new AbortController(); operation.current?.abort(); operation.current = controller; locked.current = true; setBusy(true); setNotice('')
    try { const result = await api.readSlotContent(target, controller.signal); if (live.current) { selection.current++; setSlot(target); setDocument(result); resetImages(result.data); setDraft(result.data.text ?? ''); setDirty(false); onDirty(false); setUncertain(false); setPreviewPage(result.data.currentPage) } }
    catch (error) { if (live.current) failed(error, false) }
    finally { locked.current = false; if (live.current) setBusy(false) }
  }
  async function save(change: ContentChange) {
    if (locked.current || !slot || !document || uncertain) return
    try { if (change.action === 'publishText') validateIndependentText(change.candidate.text) } catch (error) { failed(error, false); return }
    const controller = new AbortController(); operation.current = controller; locked.current = true; setBusy(true); setNotice('')
    try { const result = await api.writeContent(slot, document.revision, change, session.csrf, controller.signal); if (live.current) { setDocument(result); resetImages(result.data); setDraft(result.data.text ?? ''); setDirty(false); onDirty(false); setPreviewPage(result.data.currentPage); setNotice('内容已保存。') } }
    catch (error) { if (live.current) failed(error, true) }
    finally { locked.current = false; if (live.current) setBusy(false) }
  }
  async function importTxt(file: File) {
    if (busy || !document || document.data.kind !== 'text') return
    const generation = selection.current
    try { if (!/\.txt$/i.test(file.name) || file.size > 40003) throw new Error('请选择容量内的 TXT 文件。'); const text = decodeIndependentTxt(new Uint8Array(await file.arrayBuffer())); if (live.current && selection.current === generation) { setDraft(text); setDirty(true); onDirty(true); setNotice('TXT 已读入草稿，点击保存后才会发布到桌面。') } } catch (error) { failed(error, false) }
  }
  function chooseImages(files: File[], replace = false) {
    if (!files.length || busy || imageBatch.current) return
    if (files.length + (replace ? 0 : imagePages.length) > 50 || files.some(file => file.size === 0 || file.size > 10 * 1024 * 1024 || !/\.(?:png|jpe?g|bmp)$/i.test(file.name))) { setNotice('最多 50 页，支持 PNG/JPEG/BMP，每张最多 10MiB；请重新选择。'); return }
    const selected: DraftImage[] = files.map(file => { const url = URL.createObjectURL(file); imageUrls.current.add(url); return { source: 'upload', file, url } })
    if (replace) { imagePages.forEach(page => { if (page.source === 'upload') { URL.revokeObjectURL(page.url); imageUrls.current.delete(page.url) } }); setPreviewPage(0) }
    setImagePages(pages => [...(replace ? [] : pages), ...selected]); setDirty(true); onDirty(true); setNotice('图片已加入本页草稿，请调整顺序后明确发布；服务端会再次校验实际格式和像素。')
  }
  function moveImage(index: number, direction: number) {
    const target = index + direction
    if (target < 0 || target >= imagePages.length) return
    const pages = [...imagePages]; [pages[index], pages[target]] = [pages[target]!, pages[index]!]; setImagePages(pages); setPreviewPage(target); setDirty(true); onDirty(true)
  }
  function removeImage(index: number) {
    const page = imagePages[index]
    if (page?.source === 'upload') { URL.revokeObjectURL(page.url); imageUrls.current.delete(page.url) }
    setImagePages(pages => pages.filter((_, i) => i !== index)); setPreviewPage(old => Math.max(0, Math.min(old, imagePages.length - 2))); setDirty(true); onDirty(true)
  }
  async function publishImages() {
    if (!slot || !document || !imagePages.length || locked.current || uncertain || imageBatch.current) return
    const owner = new AbortController(); operation.current = owner; locked.current = true; setBusy(true); setImageUploading(true); setNotice('正在准备图片发布…')
    try {
      const additions = imagePages.filter((page): page is Extract<DraftImage, { source: 'upload' }> => page.source === 'upload')
      let batch: ImageUploadBatch | null = null
      if (additions.length) {
        batch = await api.createImageBatch(slot, document.revision, additions.map(page => page.file), session.csrf, owner.signal)
        imageBatch.current = { slot, batch }; if (live.current) setImageBatchId(batch.batchId)
        for (let index = 0; index < additions.length; index++) {
          if (live.current) setNotice(`正在上传第 ${index + 1} / ${additions.length} 张图片…`)
          await api.uploadImage(slot, batch.batchId, batch.uploads[index]!.id, additions[index]!.file, session.csrf, owner.signal)
        }
      }
      let nextUpload = 0
      const pages: ImagePageInput[] = imagePages.map(page => page.source === 'existing' ? { source: 'existing', mediaVersionId: page.image.mediaVersionId } : { source: 'upload', uploadId: batch!.uploads[nextUpload++]!.id })
      if (live.current) setNotice('图片已上传，正在校验并发布完整页序…')
      const result = await api.writeContent(slot, document.revision, { action: 'publishImages', candidate: { batchId: batch?.batchId ?? null, pages } }, session.csrf, owner.signal)
      imageBatch.current = null
      if (live.current) { setImageBatchId(null); setDocument(result); resetImages(result.data); setPreviewPage(result.data.currentPage); setDirty(false); onDirty(false); setNotice('图片及页序已保存。') }
    } catch (error) { if (live.current) { failed(error, true); if (imageBatch.current) { setUncertain(true); setNotice('图片批次尚未完成确认。可先重新读取内容核对，或取消上传并释放暂存文件；不会自动重复发布。') } } }
    finally { if (operation.current === owner) { locked.current = false; if (live.current) { setBusy(false); setImageUploading(false) } } }
  }
  async function cancelImages() {
    const active = imageBatch.current
    operation.current?.abort()
    if (!active) { locked.current = false; setBusy(false); setImageUploading(false); setUncertain(true); setNotice('已停止等待图片上传。若批次申请已经送达，服务端会清理未完成的暂存；请重新读取核对。'); return }
    const owner = new AbortController(); operation.current = owner; locked.current = true; setBusy(true); setNotice('正在取消并清理图片暂存…')
    try { await api.cancelImageBatch(active.slot, active.batch.batchId, session.csrf, owner.signal); imageBatch.current = null; if (live.current) { setImageBatchId(null); setUncertain(true); setNotice('服务端已确认取消并清理图片批次，请重新读取节点内容核对。') } }
    catch (error) { if (live.current) { failed(error, false); setUncertain(true); setNotice('尚未确认图片批次清理，请保留本页并稍后再次取消或重新读取；不会声称已释放暂存。') } }
    finally { if (operation.current === owner) { locked.current = false; if (live.current) { setBusy(false); setImageUploading(false) } } }
  }
  const nodes = layout?.nodes.filter(n => n.kind === 'widget' && (n.widget === 'subject_text' || n.widget === 'subject_image')) ?? [], content = document?.data, image = imagePages[previewPage]
  return <div><p role="status" className="admin-resource-status">{busy ? '正在读取或保存独立内容…' : notice || '为公共或学科布局中实际存在的文字、图片节点管理内容。'}</p><Section title="选择实际内容节点"><Field label="内容所属布局"><select disabled={busy} value={scope} onChange={e => setScope(e.target.value as 'public' | 'subject')}><option value="public">公共布局</option><option value="subject">班级 / 科目 / 老师独立布局</option></select></Field>{scope === 'subject' && <SubjectScopePicker identity={identity.document?.data ?? null} value={key} onChange={setKey} />}<Button type="button" disabled={busy || scope === 'subject' && !completeProfileKey(key)} onClick={() => { void chooseScope() }}>读取该布局的内容节点</Button>{layout && <div className="admin-subject-list">{nodes.length ? nodes.map(node => <button type="button" disabled={busy} key={node.id} onClick={() => { void load(layoutScope.scope === 'public' ? { scope: 'public', nodeId: node.id } : { scope: 'subject', key: layoutScope.key, nodeId: node.id }) }}>{node.widget === 'subject_text' ? '文字' : '图片'}节点 #{node.id}{slot?.nodeId === node.id ? '（已选）' : ''}</button>) : <p>此布局没有文字或图片节点。请先在组件布局中添加对应组件并保存。</p>}</div>}</Section>{slot && document && content && <><Section title={content.kind === 'text' ? '独立文字与 TXT' : '分页图片'} description={`节点 #${slot.nodeId} · 内容修订 ${document.revision}${content.updatedAt ? ` · 更新于 ${content.updatedAt}` : ''}`}><fieldset className="admin-fieldset" disabled={busy || uncertain || content.pending}>{content.kind === 'text' ? <form onSubmit={e => { e.preventDefault(); void save({ action: 'publishText', candidate: { text: draft } }) }}><Field label="纯文本正文"><textarea className="content-textarea" rows={12} maxLength={20000} value={draft} onChange={e => { setDraft(e.target.value); setDirty(true); onDirty(true) }} /></Field><p>{[...draft].length.toLocaleString()} / 10,000 字符 · {new TextEncoder().encode(draft).length.toLocaleString()} / 40,000 字节</p><Field label="导入 TXT（只读入草稿）"><Input type="file" accept=".txt,text/plain" onChange={e => { const file = e.target.files?.[0]; e.target.value = ''; if (file) void importTxt(file) }} /></Field><div className="admin-save-bar"><span>{dirty ? '文字草稿尚未保存' : '当前为已保存文字'}</span><Button type="submit" disabled={!dirty}>保存文字</Button></div><details open><summary>正文预览</summary><pre className="independent-text-preview">{draft || '（空内容）'}</pre></details></form>  : <><p>已保存 {content.pages.length} 页；当前草稿 {imagePages.length} 页。每张最多 10MiB、800 万像素。</p><Field label="添加图片（可多选）"><Input type="file" accept=".png,.jpg,.jpeg,.bmp,image/png,image/jpeg,image/bmp" multiple disabled={imageBatchId !== null} onChange={e => { const files = Array.from(e.target.files ?? []); e.target.value = ''; chooseImages(files) }} /></Field><Field label="替换全部图片（当前已保存页序将在发布后替换）"><Input type="file" accept=".png,.jpg,.jpeg,.bmp,image/png,image/jpeg,image/bmp" multiple disabled={imageBatchId !== null} onChange={e => { const files = Array.from(e.target.files ?? []); e.target.value = ''; chooseImages(files, true) }} /></Field><ol className="independent-image-pages">{imagePages.map((page, index) => <li key={page.source === 'existing' ? page.image.mediaVersionId + '-' + index : page.url}><button type="button" className={previewPage === index ? 'selected' : ''} onClick={() => setPreviewPage(index)}>第 {index + 1} 页 · {page.source === 'existing' ? `已保存 ${page.image.format.toUpperCase()}` : page.file.name}</button><Button type="button" variant="ghost" disabled={index === 0} onClick={() => moveImage(index, -1)}>上移</Button><Button type="button" variant="ghost" disabled={index + 1 === imagePages.length} onClick={() => moveImage(index, 1)}>下移</Button><Button type="button" variant="ghost" onClick={() => removeImage(index)}>移除</Button></li>)}</ol>{image ? <>{image.source === 'existing' ? <ImagePreview api={api} slot={slot} image={image.image} revoked={onRevoked} /> : <div className="independent-image-preview"><img src={image.url} alt={`尚未保存的图片：${image.file.name}`} /></div>}<div className="admin-action-row"><Button type="button" variant="outline" disabled={previewPage === 0} onClick={() => setPreviewPage(page => page - 1)}>预览上一页</Button><span>{previewPage + 1} / {imagePages.length}</span><Button type="button" variant="outline" disabled={previewPage + 1 >= imagePages.length} onClick={() => setPreviewPage(page => page + 1)}>预览下一页</Button><Button type="button" disabled={dirty || previewPage === content.currentPage || !content.canPage} onClick={() => { void save({ action: 'page', candidate: { page: previewPage, expectedViewRevision: content.viewRevision } }) }}>让白板显示此页</Button></div>{!content.canPage && <p>当前未取得此节点的桌面翻页能力，可在本页预览；仅实际发布中的节点可控制白板页码。</p>}</> : <p>没有图片页；清空已保存内容请使用下方“清空节点内容”。</p>}<div className="admin-save-bar"><span>{dirty ? '图片与页序尚未保存' : '当前为已保存页序'}</span><Button type="button" disabled={!dirty || !imagePages.length || imageBatchId !== null} onClick={() => { void publishImages() }}>发布图片与页序</Button></div></>}</fieldset>{(imageUploading || imageBatchId) && <Button type="button" variant="outline" onClick={() => { void cancelImages() }}>取消图片上传并清理暂存</Button>}<div className="admin-action-row"><Button type="button" variant="outline" disabled={busy} onClick={() => { void load(slot) }}>重新读取内容</Button><Button type="button" variant="outline" disabled={busy || uncertain || content.pending || imageBatchId !== null} onClick={() => { if (window.confirm('清空当前节点的全部已保存文字或图片？旧版本仅按系统保留规则支持撤销。')) void save({ action: 'clear', candidate: {} }) }}>清空节点内容</Button><Button type="button" variant="outline" disabled={busy || uncertain || content.pending || content.previousRevision === null} onClick={() => { if (window.confirm(`恢复上一个保留版本 ${content.previousRevision}？此操作会创建新的内容版本。`)) void save({ action: 'undo', candidate: { targetRevision: content.previousRevision! } }) }}>撤销上次发布</Button></div>{content.pending && <p className="notice">该节点存在正在处理的发布任务，请稍后重新读取状态。</p>}</Section></>}</div>
}
