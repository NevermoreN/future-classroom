import ServerLayoutPreview from './ServerLayoutPreview'
import { useEffect, useRef, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Field, NumberField, Toggle, Section } from './AdminForms'
import LayoutTreeEditor from './LayoutTreeEditor'
import { useResource } from './useAdminResource'
import { MaterialHttpApi } from './api/material-client'
import { ApiError, isAborted, trustedApiBase } from './api/client'
import { type AdminDocument, type IdentityData } from './api/admin-client'
import { ProfileAdminHttpApi, type ProfileKey, type ProfilePage, type SubjectProfileData, type ProfileCandidate } from './api/profile-admin-client'
import { ProtocolFault, type Session } from './api/protocol'
export const emptyProfileKey = (): ProfileKey => ({ classId: '', subjectId: '', teacherId: '' })
export const completeProfileKey = (key: ProfileKey) => !!key.classId && !!key.subjectId && !!key.teacherId
export function SubjectScopePicker({ identity, value, onChange }: { identity: IdentityData | null; value: ProfileKey; onChange: (key: ProfileKey) => void }) {
  return <div className="admin-form-grid"><Field label="班级"><select value={value.classId} onChange={e => onChange({ ...value, classId: e.target.value, teacherId: '' })}><option value="">选择班级</option>{identity?.classes.filter(c => c.enabled || c.id === value.classId).map(c => <option key={c.id} value={c.id}>{c.name}</option>)}</select></Field><Field label="科目"><select value={value.subjectId} onChange={e => onChange({ ...value, subjectId: e.target.value, teacherId: '' })}><option value="">选择科目</option>{identity?.subjects.filter(s => s.enabled || s.id === value.subjectId).map(s => <option key={s.id} value={s.id}>{s.name}</option>)}</select></Field><Field label="任课老师"><select value={value.teacherId} onChange={e => onChange({ ...value, teacherId: e.target.value })}><option value="">选择已授权任课老师</option>{identity?.accounts.filter(a => a.enabled && identity.assignments.some(scope => scope.enabled && scope.teacherId === a.id && scope.classId === value.classId && scope.subjectId === value.subjectId)).map(a => <option key={a.id} value={a.id}>{a.displayName}</option>)}</select></Field></div>
}
export default function SubjectProfilesPanel({ session, onRevoked, onDirty }: { session: Session; onRevoked: () => void; onDirty: (dirty: boolean) => void }) {
  const identity = useResource('identity', session, onRevoked), [api] = useState(() => new ProfileAdminHttpApi(trustedApiBase(import.meta.env.BASE_URL, window.location.href))), [materials] = useState(() => new MaterialHttpApi(trustedApiBase(import.meta.env.BASE_URL, window.location.href)))
  const [scope, setScope] = useState<ProfileKey>(emptyProfileKey), [document, setDocument] = useState<AdminDocument<SubjectProfileData> | null>(null), [candidate, setCandidate] = useState<ProfileCandidate | null>(null), [list, setList] = useState<ProfilePage | null>(null), [after, setAfter] = useState('0'), [history, setHistory] = useState<string[]>([]), [busy, setBusy] = useState(false), [notice, setNotice] = useState(''), [dirty, setDirty] = useState(false), [uncertain, setUncertain] = useState(false)
  const operation = useRef<AbortController | null>(null), live = useRef(true), locked = useRef(false), currentScope = useRef<ProfileKey | null>(null)
  useEffect(() => { live.current = true; return () => { live.current = false; operation.current?.abort() } }, [])
  function changed(next: ProfileCandidate) { setCandidate(next); setDirty(true); onDirty(true) }
  function failure(error: unknown, write: boolean) {
    if (isAborted(error)) return
    if (error instanceof ApiError && error.status === 401) { onRevoked(); return }
    if (error instanceof ApiError && error.status === 403) { setNotice('该操作没有权限，当前账号登录已保留。'); return }
    if (write && (!(error instanceof ApiError) || error.status === 409)) { setUncertain(true); setNotice('保存结果未确认或布局版本已变化，请重新读取并核对；不会自动重复保存。'); return }
    setNotice(error instanceof ProtocolFault ? error.message : error instanceof ApiError && error.status === 400 ? '布局未被接受，请检查任教关系、布局限制和资料设置。' : '暂时无法读取或保存学科布局，请检查连接。')
  }
  async function listing(next: string, previous: string[]) {
    if (locked.current) return
    const controller = new AbortController(); operation.current?.abort(); operation.current = controller; locked.current = true; setBusy(true)
    try { const result = await api.profiles(next, controller.signal); if (live.current) { setList(result.data); setAfter(next); setHistory(previous) } }
    catch (error) { if (live.current) failure(error, false) }
    finally { locked.current = false; if (live.current) setBusy(false) }
  }
  useEffect(() => { void listing('0', []) }, [])
  async function read(key = scope) {
    if (locked.current || !completeProfileKey(key) || dirty && !window.confirm('重新读取会丢弃尚未保存的学科布局，确认继续？')) return
    const controller = new AbortController(); operation.current?.abort(); operation.current = controller; locked.current = true; setBusy(true); setNotice('')
    try { const result = await api.profile(key, controller.signal); if (live.current) { setDocument(result); setCandidate({ enabled: result.data.enabled, layout: structuredClone(result.data.layout), folderId: result.data.folderId, materials: structuredClone(result.data.materials) }); currentScope.current = key; setScope(key); setUncertain(false); setDirty(false); onDirty(false) } }
    catch (error) { if (live.current) failure(error, false) }
    finally { locked.current = false; if (live.current) setBusy(false) }
  }
  async function save() {
    if (!document || !candidate || !currentScope.current || locked.current || uncertain) return
    const controller = new AbortController(); operation.current = controller; locked.current = true; setBusy(true); setNotice('')
    try { const result = await api.saveProfile(currentScope.current, document.revision, candidate, session.csrf, controller.signal); if (live.current) { setDocument(result); setCandidate({ enabled: result.data.enabled, layout: structuredClone(result.data.layout), folderId: result.data.folderId, materials: structuredClone(result.data.materials) }); setDirty(false); onDirty(false); setNotice('学科独立布局已保存。只有匹配班级、科目和任课老师时才采用此布局。') } }
    catch (error) { if (live.current) failure(error, true) }
    finally { locked.current = false; if (live.current) setBusy(false) }
  }
  async function attachFolder() {
    if (!candidate || !currentScope.current || locked.current) return
    const controller = new AbortController(); operation.current = controller; locked.current = true; setBusy(true)
    try { const folder = await materials.materials({ ownerTeacherId: currentScope.current.teacherId, subjectId: currentScope.current.subjectId }, null, controller.signal); if (live.current) changed({ ...candidate, folderId: folder.folderId }) }
    catch (error) { if (live.current) failure(error, false) }
    finally { locked.current = false; if (live.current) setBusy(false) }
  }
  const people = identity.document?.data
  return <div><p className="admin-resource-status" role="status">{busy ? '正在核对学科布局…' : notice || '每个班级、科目和老师组合单独保存布局。'}</p><Section title="选择学科布局"><SubjectScopePicker identity={people ?? null} value={scope} onChange={key => setScope(key)} /><div className="admin-action-row"><Button disabled={!completeProfileKey(scope) || busy} onClick={() => { void read() }}>读取 / 创建该组合布局</Button><Button variant="outline" disabled={busy} onClick={() => { void listing('0', []) }}>刷新已有布局列表</Button></div>{list && <><div className="admin-subject-list">{list.items.map(item => <button type="button" key={item.id} disabled={busy} onClick={() => { void read(item.key) }}>{people?.classes.find(c => c.id === item.key.classId)?.name ?? item.key.classId} · {people?.subjects.find(s => s.id === item.key.subjectId)?.name ?? item.key.subjectId} · {people?.accounts.find(a => a.id === item.key.teacherId)?.displayName ?? item.key.teacherId}{item.enabled ? '' : '（停用）'}</button>)}</div><div className="admin-action-row"><span>共 {list.total} 个独立布局</span><Button variant="outline" disabled={busy || !history.length} onClick={() => { void listing(history.at(-1)!, history.slice(0, -1)) }}>上一页</Button><Button variant="outline" disabled={busy || !list.nextAfter} onClick={() => { void listing(list.nextAfter!, [...history, after]) }}>下一页</Button></div></>}</Section>{candidate && document && <form onSubmit={e => { e.preventDefault(); void save() }}><fieldset className="admin-fieldset" disabled={busy || uncertain}><Section title={document.data.exists ? '学科布局设置' : '新的学科布局'} description="仅当前选择的组合受影响；关闭独立布局时采用公共科目插槽内容。"><Toggle label="启用该组合的独立布局" value={candidate.enabled} onChange={enabled => changed({ ...candidate, enabled })} /><p>已读取：班级 {people?.classes.find(c => c.id === currentScope.current?.classId)?.name} · {people?.subjects.find(s => s.id === currentScope.current?.subjectId)?.name} · {people?.accounts.find(a => a.id === currentScope.current?.teacherId)?.displayName}</p></Section><LayoutTreeEditor value={candidate.layout} scope="subject" onChange={layout => changed({ ...candidate, layout })} /><ServerLayoutPreview session={session} onRevoked={onRevoked} layout={candidate.layout} revision={document.revision} profileKey={currentScope.current!} materials={candidate.materials} /><Section title="课堂资料展示"><div className="admin-action-row"><span>{candidate.folderId ? '已关联这位老师的科目资料目录' : '尚未关联资料目录'}</span><Button type="button" variant="outline" onClick={() => { void attachFolder() }}>关联这位老师的科目资料</Button><Button type="button" variant="outline" disabled={!candidate.folderId} onClick={() => changed({ ...candidate, folderId: null })}>解除关联</Button></div><div className="admin-form-grid"><NumberField label="显示资料行数" min={1} max={5} value={candidate.materials.visibleRows} onChange={visibleRows => changed({ ...candidate, materials: { ...candidate.materials, visibleRows } })} /><NumberField label="资料行间距 DIP" max={64} value={candidate.materials.rowGapDip} onChange={rowGapDip => changed({ ...candidate, materials: { ...candidate.materials, rowGapDip } })} />{(['opacityPercent', 'fontDip', 'minWidth', 'maxWidth', 'minHeight', 'maxHeight'] as const).map((key, i) => <NumberField key={key} label={['不透明度 %', '字号 DIP', '最小宽', '最大宽', '最小高', '最大高'][i]!} value={candidate.materials.component[key]} max={key === 'opacityPercent' ? 100 : key === 'fontDip' ? 72 : key.includes('Height') ? 2160 : 4096} onChange={value => changed({ ...candidate, materials: { ...candidate.materials, component: { ...candidate.materials.component, [key]: value } } })} />)}</div><Toggle label="显示资料组件" value={candidate.materials.component.visible} onChange={visible => changed({ ...candidate, materials: { ...candidate.materials, component: { ...candidate.materials.component, visible } } })} /></Section></fieldset><div className="admin-save-bar"><span>{dirty ? '学科布局尚未保存' : '当前为白板保存的版本'}</span><Button type="button" variant="outline" disabled={busy} onClick={() => { if (currentScope.current) void read(currentScope.current) }}>重新读取</Button><Button type="submit" disabled={busy || uncertain || !dirty}>{busy ? '正在保存…' : '保存学科布局'}</Button></div></form>}</div>
}
