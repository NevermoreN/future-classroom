import { useEffect, useRef, useState } from 'react'
import { ArrowLeft, ArrowRight, BookOpen, Check, ChevronRight, CircleAlert, ClipboardList, FolderOpen, GraduationCap, History, LoaderCircle, LockKeyhole, LogOut, Radio, RefreshCw, Save, ShieldCheck, Trash2 } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { trustedApiBase } from './api/client'
import { ContentController, draftValid, type ContentState, type EditorState } from './api/content-controller'
import type { ContentKind, ScopeItem } from './api/content-protocol'
import MaterialsPane from './MaterialsPane'
import { VoiceHttpApi } from './api/voice-client'
import { VoiceController, emptyVoice, type VoiceState } from './api/voice-controller'
import { RecorderController, type RecorderState } from './voice/recorder'
import VoicePane from './VoicePane'
import { MaterialController, emptyMaterials, type MaterialState } from './api/material-controller'
import type { Role, Session } from './api/protocol'
const roles: Record<Role, string> = { GLOBAL_ADMIN: '全局主管理员', TEACHER_ADMIN: '教师管理员', USER: '普通用户' }
const empty: ContentState = { kind: 'HOMEWORK', page: null, editor: null, busy: false, pageBlocked: false, notice: '', previousOffset: null }
function Editor({ editor: e, busy, controller }: { editor: EditorState; busy: boolean; controller: ContentController | undefined }) {
  const bytes = new TextEncoder().encode(e.draft).byteLength
  const editable = !busy && !e.expired && ['ready', 'conflict', 'merge'].includes(e.phase)
  const canSave = !busy && !e.expired && e.phase === 'ready' && draftValid(e.draft)
  return <Card className="content-editor"><CardHeader><div className="editor-heading"><div><p className="eyebrow">{e.loaded.key.kind === 'HOMEWORK' ? '老师本人当天作业' : '班级共同当天提醒'}</p><CardTitle role="heading" aria-level={2}>{e.scope.className}{e.scope.kind === 'HOMEWORK' ? ` · ${e.scope.subjectName}` : ' · 今日提醒'}</CardTitle>{e.scope.kind === 'HOMEWORK' && <CardDescription>内容所属老师：{e.scope.ownerDisplayName}</CardDescription>}</div><span className={`editor-badge ${e.expired ? 'expired' : ''}`}>{e.expired ? '旧草稿已过期' : `修订 ${e.loaded.revision}`}</span></div><p className="server-date">白板日期 {e.loaded.today}<span>有效至 {e.loaded.expiresAt.replace('T', ' ')}</span></p></CardHeader><CardContent>
    <div aria-live="polite" aria-atomic="true">{e.notice && <p className={`content-notice ${['unknown', 'conflict', 'expired', 'scope-changed'].includes(e.phase) || e.expired ? 'attention' : ''}`}><CircleAlert size={17} />{e.notice}</p>}</div>
    <label className="editor-label" htmlFor="content-text">{e.phase === 'merge' ? '你的草稿（保留，尚未提交）' : '正文（纯文本）'}<span>{bytes.toLocaleString()} / 16,384 字节</span></label>
    <textarea id="content-text" className={`content-textarea ${bytes > 16384 ? 'invalid' : ''}`} rows={10} maxLength={16384} value={e.draft} onChange={event => controller?.edit(event.target.value)} readOnly={!editable} aria-describedby="content-text-help" aria-invalid={!draftValid(e.draft)} placeholder={e.scope.kind === 'HOMEWORK' ? '填写这位老师今天布置的作业…' : '填写这个班级今天的共同提醒…'} />
    <p id="content-text-help" className="editor-help">以白板日期为准；仅支持纯文本和正常换行。保存表示持久化成功，不代表内容此刻正在白板上显示。</p>
    {e.pending && <section className="command-receipt" aria-label="原提交命令"><strong>原命令 {e.pending.command.commandId}</strong><p>原日期 {e.pending.key.date} · 原修订 {e.pending.command.expectedRevision}</p><p>原正文只保留在本页内存，离页可能丢失查询入口；未确认的命令不会重新发送。</p><Button variant="outline" disabled={busy || !['unknown', 'result'].includes(e.phase)} onClick={() => { void controller?.query() }}><History />手动查询原命令</Button></section>}
    {e.history && <section className="historical-result"><h3><Check size={17} />原命令已保存的结果（不代表当前最新）</h3><p>修订 {e.history.revision} · {e.history.updatedAt?.replace('T', ' ')}</p><pre>{e.history.text || '（原结果为空内容）'}</pre><Button variant="outline" disabled={busy || e.expired} onClick={() => { void controller?.loadLatest() }}><RefreshCw />读取当前内容核对</Button></section>}
    {e.latest && <section className="merge-panel"><h3>当前服务器内容 · 修订 {e.latest.revision}</h3><pre>{e.latest.text || '（当前为空内容）'}</pre><p>请对照上方草稿，明确选择。这里只更新编辑基线，不会提交。</p><div><Button variant="outline" disabled={busy || e.expired} onClick={() => controller?.acknowledgeMerge(true)}>采用服务器内容，放弃草稿</Button><Button disabled={busy || e.expired} onClick={() => controller?.acknowledgeMerge(false)}>已核对合并，保留我的草稿</Button></div></section>}
    <div className="editor-actions"><Button disabled={!canSave} onClick={() => { void controller?.save() }}>{busy && e.phase === 'saving' ? <LoaderCircle className="spinner" /> : <Save />}保存当天内容</Button><Button variant="outline" disabled={busy || e.expired || e.phase !== 'ready'} onClick={() => { if (window.confirm('将以当前修订提交空内容，并替换本页草稿。确认清空今天的这个槽位？')) void controller?.save(true) }}><Trash2 />清空当天内容</Button>{['ready', 'conflict'].includes(e.phase) && <Button variant="ghost" disabled={busy || e.expired} onClick={() => { void controller?.loadLatest() }}><RefreshCw />{e.phase === 'conflict' ? '载入最新修订（保留草稿）' : '读取当前内容'}</Button>}</div>
  </CardContent></Card>
}
export default function TeacherWorkspace({ session, onLogout, onRefresh, onRevoked }: { session: Session; onLogout: () => void; onRefresh: () => void; onRevoked: () => void }) {
  const [page, setPage] = useState<'HOME' | 'MATERIALS' | 'VOICE' | ContentKind>('HOME'), [state, setState] = useState<ContentState>(empty)
  const [materials, setMaterials] = useState<MaterialState>(emptyMaterials), materialOwner = useRef<MaterialController | undefined>(undefined)
  const [api] = useState(() => new VoiceHttpApi(trustedApiBase(import.meta.env.BASE_URL, window.location.href)))
  const [voice, setVoice] = useState<VoiceState>(emptyVoice), voiceOwner = useRef<VoiceController | undefined>(undefined)
  const [recording, setRecording] = useState<RecorderState>({ phase: 'idle', recordingId: null, elapsedMillis: 0, cancelGesture: false }), recorder = useRef<RecorderController | undefined>(undefined)
  const owner = useRef<ContentController | undefined>(undefined), revoke = useRef(onRevoked)
  revoke.current = onRevoked
  useEffect(() => {
    const controller = new ContentController(api, session.csrf, setState, () => revoke.current()); owner.current = controller
    const materialController = new MaterialController(api, session.csrf, setMaterials, () => revoke.current()); materialOwner.current = materialController
    const voiceController = new VoiceController(api, session.csrf, setVoice, () => revoke.current(), () => recorder.current?.cancel()); voiceOwner.current = voiceController
    const recorderController = new RecorderController({ onState: next => { setRecording(next); if (next.phase === 'idle') queueMicrotask(() => { if (voiceController.snapshot().phase === 'recording') voiceController.cancelUnsent('本次未发送录音已取消并释放麦克风。') }) }, onWav: result => { void voiceController.recorded(result) }, onError: error => voiceController.cancelUnsent(error.message) }); recorder.current = recorderController
    const visibility = () => voiceController.setVisible(document.visibilityState === 'visible'); document.addEventListener('visibilitychange', visibility); visibility()
    const beforeUnload = (event: BeforeUnloadEvent) => { if (controller.hasRiskyDraft() || materialController.hasRiskyDraft() || voiceController.hasRiskyDraft()) { event.preventDefault(); event.returnValue = '' } }
    window.addEventListener('beforeunload', beforeUnload)
    return () => { window.removeEventListener('beforeunload', beforeUnload); document.removeEventListener('visibilitychange', visibility); voiceController.dispose(); recorderController.dispose(); controller.dispose(); materialController.dispose(); owner.current = undefined; materialOwner.current = undefined; voiceOwner.current = undefined; recorder.current = undefined }
  }, [api, session.csrf])
  const canManage = session.principal.role !== 'USER'
  function consent() { return !(owner.current?.hasRiskyDraft() || materialOwner.current?.hasRiskyDraft() || voiceOwner.current?.hasRiskyDraft()) || window.confirm('本页有未保存草稿或结果尚未确认的命令。离开会丢弃本页内存信息，不会撤销已经提交的命令，也不会自动保存。确认离开？') }
  function open(kind: ContentKind) { if (!canManage || !consent()) return; materialOwner.current?.leave(true); voiceOwner.current?.leave(true); setPage(kind); void owner.current?.loadScopes(kind, 0, true) }
  function home() { if (consent()) { owner.current?.leave(true); materialOwner.current?.leave(true); voiceOwner.current?.leave(true); setPage('HOME') } }
  function openMaterials() { if (!canManage || !consent()) return; owner.current?.leave(true); voiceOwner.current?.leave(true); setPage('MATERIALS'); void materialOwner.current?.loadScopes(0, true) }
  function openVoice() { if (!canManage || !consent()) return; owner.current?.leave(true); materialOwner.current?.leave(true); setPage('VOICE'); void voiceOwner.current?.open() }
  function select(item: ScopeItem) { if (consent()) void owner.current?.select(item, true) }
  function reload(offset = 0) { if (page !== 'HOME' && page !== 'MATERIALS' && page !== 'VOICE' && consent()) void owner.current?.loadScopes(page, offset, true) }
  const scopes = state.page
  return <main id="main-content" className="workspace teacher-workspace">
    <div className="workspace-heading"><div><p className="eyebrow">{page === 'HOME' ? '你的课堂，从这里连接' : '以白板实际日期与授权范围为准'}</p><h1>{page === 'HOME' ? `你好，${session.principal.displayName}` : page === 'VOICE' ? '语音播报' : page === 'MATERIALS' ? '我的课堂资料' : page === 'HOMEWORK' ? '我的今日作业' : '管理今日提醒'}<span className="hello-dot">。</span></h1><p>{page === 'HOME' ? '账号与内容范围由白板确认，提交结果与屏幕显示分开核验。' : page === 'VOICE' ? '核对白板当前班级，按住说话、松开发送。' : page === 'MATERIALS' ? '选择老师与科目目录，上传、校验后添加；下载到当前设备。' : page === 'HOMEWORK' ? '选择实际老师、班级和科目；每次保存检查当前修订。' : '共同提醒覆盖受管班级，不受你本人任教班级限制。'}</p></div><div className="workspace-top-actions">{page !== 'HOME' && <Button variant="outline" onClick={home}><ArrowLeft />返回工作台</Button>}<Button variant="outline" onClick={() => { if (consent()) { owner.current?.leave(true); materialOwner.current?.leave(true); voiceOwner.current?.leave(true); onRefresh() } }}><RefreshCw />核对会话</Button></div></div>
    {page === 'HOME' ? <>
      <section className="identity-summary" aria-label="当前授权"><div className="avatar"><GraduationCap size={28} /></div><div><strong>{session.principal.displayName}</strong><span>{roles[session.principal.role]}</span></div><div className="summary-divider" /><div><small>白板当前班级</small><strong>{session.currentClassId === null ? '尚未绑定班级' : `班级 ID ${session.currentClassId}`}</strong></div><span className="verified"><ShieldCheck size={15} />白板已确认</span></section>
      <div className="section-label"><h2>教师工作台</h2><span>保存、查询与结果核对</span></div>
      <div className="feature-grid">
        <Card className="feature-card"><CardContent><span className="feature-icon blue"><ClipboardList size={24} /></span><h3>我的今日作业</h3><p>读取和管理真实任教老师的当天作业</p><Button variant="ghost" disabled={!canManage} onClick={() => open('HOMEWORK')}>{canManage ? '管理今日作业' : '当前账号无管理权限'}<ChevronRight size={16} /></Button></CardContent></Card>
        <Card className="feature-card"><CardContent><span className="feature-icon violet"><FolderOpen size={24} /></span><h3>我的课堂资料</h3><p>上传、管理真实课堂文件，下载到当前设备</p><Button disabled={!canManage} variant="ghost" onClick={openMaterials}>管理课堂资料<ChevronRight size={16} /></Button></CardContent></Card>
        <Card className="feature-card"><CardContent><span className="feature-icon peach"><Radio size={24} /></span><h3>语音播报</h3><p>向已授权的班级发起播报</p><Button disabled={!canManage} variant="ghost" onClick={openVoice}>打开语音播报<ChevronRight size={16} /></Button></CardContent></Card>
        <Card className="feature-card"><CardContent><span className="feature-icon green"><BookOpen size={24} /></span><h3>今日提醒</h3><p>管理本安装学校共同的当天提醒</p><Button variant="ghost" disabled={!canManage} onClick={() => open('REMINDER')}>{canManage ? '查看共同提醒' : '当前账号无管理权限'}<ChevronRight size={16} /></Button></CardContent></Card>
      </div>
      <Card className="scope-card"><CardHeader><CardTitle role="heading" aria-level={2}>内容按白板授权读取</CardTitle><CardDescription>班级、科目和内容所属老师的名称由服务返回。进入作业或提醒后分页加载，不以页面输入扩展权限。</CardDescription></CardHeader><CardContent><p className="empty-scope">未保存草稿仅保留在本页内存。切换、刷新或离页前会提示；撤销会话后立即停止操作并清除编辑信息。</p></CardContent></Card>
    </> : page === 'VOICE' ? <VoicePane state={voice} controller={voiceOwner.current} recorder={recorder.current} recording={recording} /> : page === 'MATERIALS' ? <MaterialsPane state={materials} controller={materialOwner.current} consent={consent} /> : <>
      <div className="content-toolbar"><span><BookOpen size={16} />{scopes && scopes.kind === page ? `白板日期 ${scopes.today}` : '正在核对白板日期与范围'}</span><Button variant="outline" disabled={state.busy} onClick={() => reload()}><RefreshCw />重新加载范围</Button></div>
      {state.notice && <p className="content-notice attention" role="status"><CircleAlert size={17} />{state.notice}</p>}
      {state.busy && !scopes && <p className="loading-content" role="status"><LoaderCircle className="spinner" />正在读取白板授权范围…</p>}
      {scopes && scopes.kind === page && <Card className="scope-picker"><CardHeader><div className="scope-picker-heading"><div><CardTitle role="heading" aria-level={2}>{page === 'HOMEWORK' ? '选择实际作业范围' : '选择共同提醒班级'}</CardTitle><CardDescription>{scopes.total === 0 ? '当前没有可管理的有效范围。' : `第 ${scopes.offset + 1}–${scopes.offset + scopes.items.length} 项，共 ${scopes.total} 项（每页最多 20 项）`}</CardDescription></div></div></CardHeader><CardContent><div className="scope-options">{scopes.items.map(item => <button type="button" className={`scope-option ${state.editor && state.editor.scope.classId === item.classId && (item.kind !== 'HOMEWORK' || state.editor.scope.kind === 'HOMEWORK' && state.editor.scope.subjectId === item.subjectId && state.editor.scope.ownerTeacherId === item.ownerTeacherId) ? 'selected' : ''}`} key={item.kind === 'HOMEWORK' ? `${item.classId}/${item.subjectId}/${item.ownerTeacherId}` : item.classId} disabled={state.busy || state.pageBlocked} onClick={() => select(item)}><strong>{item.className}</strong>{item.kind === 'HOMEWORK' && <><span>{item.subjectName}</span><small>{item.ownerDisplayName}</small></>}<ChevronRight size={15} /></button>)}</div><div className="scope-pagination"><Button variant="outline" disabled={state.busy || state.pageBlocked || state.previousOffset === null} onClick={() => reload(state.previousOffset!)}><ArrowLeft />上一页</Button><Button variant="outline" disabled={state.busy || state.pageBlocked || scopes.nextOffset === null} onClick={() => reload(scopes.nextOffset!)}>下一页<ArrowRight /></Button></div></CardContent></Card>}
      {state.editor && <Editor editor={state.editor} busy={state.busy} controller={owner.current} />}
      {state.busy && scopes && <p className="loading-content" role="status"><LoaderCircle className="spinner" />正在向白板核对…</p>}
    </>}
    <div className="logout-row"><LockKeyhole size={16} /><p>注销会使这个账号的全部渠道授权失效。</p><Button variant="outline" onClick={() => { if (consent()) { owner.current?.leave(true); materialOwner.current?.leave(true); voiceOwner.current?.leave(true); onLogout() } }}><LogOut />注销账号</Button></div>
    <aside className="bottom-reminder-action" aria-label="今日提醒快捷入口"><div><BookOpen size={19} /><span>班级共同提醒<small>按白板当天日期生效</small></span></div><Button disabled={!canManage} onClick={() => open('REMINDER')}>管理今日提醒<ChevronRight size={17} /></Button></aside>
  </main>
}
