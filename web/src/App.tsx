import { useEffect, useRef, useState, type FormEvent } from 'react'
import { ArrowRight, BookOpen, Check, ChevronRight, CircleAlert, ClipboardList, FolderOpen, GraduationCap, LoaderCircle, LockKeyhole, LogOut, MonitorUp, Radio, RefreshCw, ShieldCheck, Smartphone, Sparkles, X } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { HttpApi, trustedApiBase } from './api/client'
import { PairingController, type ViewState } from './api/controller'
import type { Role, Session } from './api/protocol'
const roleName: Record<Role, string> = { GLOBAL_ADMIN: '全局主管理员', TEACHER_ADMIN: '教师管理员', USER: '普通用户' }
const features = [
  { icon: ClipboardList, title: '我的今日作业', description: '按真实任教范围管理当天作业', accent: 'blue' },
  { icon: FolderOpen, title: '我的课堂资料', description: '整理课堂需要的文件与资料', accent: 'violet' },
  { icon: Radio, title: '语音播报', description: '向已授权的班级发起播报', accent: 'peach' },
  { icon: BookOpen, title: '今日提醒', description: '管理班级当天的共同提醒', accent: 'green' },
] as const
function Brand() { return <a className="brand" href={import.meta.env.BASE_URL} aria-label="未来教室首页"><span className="brand-icon"><GraduationCap size={25} strokeWidth={1.8} /></span><span>未来教室<small>TEACHER WORKSPACE</small></span></a> }
function SessionView({ session, onLogout, onRefresh }: { session: Session; onLogout: () => void; onRefresh: () => void }) {
  return <main id="main-content" className="workspace">
    <div className="workspace-heading"><div><p className="eyebrow">你的课堂，从这里连接</p><h1>你好，{session.principal.displayName}<span className="hello-dot">。</span></h1><p>以下账号与任教范围由白板确认。</p></div><Button variant="outline" onClick={onRefresh}><RefreshCw />核对会话</Button></div>
    <section className="identity-summary" aria-label="当前授权"><div className="avatar"><GraduationCap size={28} /></div><div><strong>{session.principal.displayName}</strong><span>{roleName[session.principal.role]}</span></div><div className="summary-divider" /><div><small>白板当前班级</small><strong>{session.currentClassId === null ? '尚未绑定班级' : `班级 ID ${session.currentClassId}`}</strong></div><span className="verified"><ShieldCheck size={15} />白板已确认</span></section>
    <div className="section-label"><h2>教师工作台</h2><span>业务功能正在接入</span></div>
    <div className="feature-grid">{features.map(({ icon: Icon, title, description, accent }) => <Card key={title} className="feature-card"><CardContent><span className={`feature-icon ${accent}`}><Icon size={24} strokeWidth={1.7} /></span><h3>{title}</h3><p>{description}</p><Button disabled variant="ghost">尚未接入服务<ChevronRight size={16} /></Button></CardContent></Card>)}</div>
    <Card className="scope-card"><CardHeader><CardTitle>当前任教范围</CardTitle><CardDescription>按白板返回的班级与科目 ID 显示，不推测名称。</CardDescription></CardHeader><CardContent>{session.assignments.length === 0 ? <p className="empty-scope">当前账号没有可显示的任教范围。请联系本地管理员核对。</p> : <ul className="scope-list">{session.assignments.map(a => <li key={`${a.classId}:${a.subjectId}`}><BookOpen size={17} /><span>班级 ID {a.classId}</span><span>科目 ID {a.subjectId}</span></li>)}</ul>}</CardContent></Card>
    <div className="logout-row"><LockKeyhole size={16} /><p>使用完毕请注销。注销会使这个账号的全部渠道授权失效。</p><Button variant="outline" onClick={onLogout}><LogOut />注销账号</Button></div>
  </main>
}
function PairingForm({ enabled, onSubmit }: { enabled: boolean; onSubmit: (form: { code: string; loginName: string; deviceName: string }) => void }) {
  const [code, setCode] = useState(''), [loginName, setLoginName] = useState(''), [deviceName, setDeviceName] = useState('')
  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); if (!enabled) return
    onSubmit({ code, loginName, deviceName }); setCode('')
  }
  return <form onSubmit={submit} className="pair-form">
    <div><label htmlFor="pair-code">配对码<span>向白板管理员获取</span></label><Input id="pair-code" name="pair-code" className="code-input" inputMode="numeric" pattern="[0-9]{8}" minLength={8} maxLength={8} autoComplete="one-time-code" spellCheck={false} placeholder="8 位数字配对码" value={code} onChange={e => setCode(e.target.value)} required disabled={!enabled} aria-describedby="pair-code-help" /><p id="pair-code-help" className="field-help">管理员需在白板开启短时配对窗口。</p></div>
    <div><label htmlFor="login-name">教师账号</label><Input id="login-name" name="username" autoComplete="username" maxLength={64} placeholder="输入已有的教师账号" value={loginName} onChange={e => setLoginName(e.target.value)} required disabled={!enabled} /></div>
    <div><label htmlFor="device-name">设备名称</label><Input id="device-name" name="device-name" autoComplete="off" maxLength={64} placeholder="例如：办公室平板" value={deviceName} onChange={e => setDeviceName(e.target.value)} required disabled={!enabled} /></div>
    <Button type="submit" className="pair-submit" disabled={!enabled}>申请连接<ArrowRight size={18} /></Button>
    <p className="form-note"><ShieldCheck size={15} />申请后，由白板管理员确认账号与设备。</p>
  </form>
}
export default function App() {
  const [view, setView] = useState<ViewState>({ phase: 'checking' })
  const controller = useRef<PairingController | undefined>(undefined)
  useEffect(() => {
    try {
      const api = new HttpApi(trustedApiBase(import.meta.env.BASE_URL, window.location.href))
      const owner = new PairingController(api, setView); controller.current = owner; void owner.connect()
      return () => { owner.dispose(); controller.current = undefined }
    } catch { setView({ phase: 'error', message: '页面的服务地址无效，请使用管理员提供的可信 HTTPS 地址。', canPair: false }) }
  }, [])
  const busy = ['checking', 'creating', 'waiting', 'logging-out'].includes(view.phase)
  const connected = ['anonymous', 'creating', 'waiting', 'authenticated', 'logging-out', 'logout-unknown', 'expired'].includes(view.phase)
  return <div className="app-shell"><div className="sr-only" role="status" aria-live="polite">{view.phase === 'authenticated' ? '白板已确认登录，教师工作台已就绪。' : view.phase === 'logout-unknown' ? '注销结果尚未确认，需要主动核对白板会话。' : ''}</div><a className="skip-link" href="#main-content">跳到主要内容</a>
    <header className="site-header"><Brand /><div className="header-right"><span className={`connection-status ${connected ? 'connected' : ''}`}><span />{connected ? '白板连接已核验' : busy ? '正在检查连接' : '连接待核验'}</span><span className="workspace-label">教师工作台</span></div></header>
    {view.phase === 'authenticated' ? <SessionView session={view.session} onLogout={() => { void controller.current?.logout() }} onRefresh={() => { void controller.current?.connect() }} /> : <main id="main-content" className="pairing-layout">
      <section className="intro-panel"><span className="intro-tag"><Sparkles size={14} />让教学，轻松一步</span><h1>连接你的教室，<br />专注每一堂课<span>。</span></h1><p className="intro-description">从自己的设备安全连接白板。<br />课堂里的准备与协作，从这里开始。</p>
        <div className="classroom-illustration" aria-hidden="true"><div className="board-top"><span /><span /><span /></div><div className="board-content"><span className="board-caption">未来教室</span><div className="board-line long" /><div className="board-line short" /><div className="board-tiles"><span><BookOpen size={28} /></span><span><Radio size={26} /></span><span><FolderOpen size={28} /></span></div></div><div className="device-bubble"><Smartphone size={24} /><span>你的设备</span><i><Check size={12} /></i></div><div className="board-stand" /></div>
        <ol className="pairing-steps"><li><span>01</span><div><strong>获取配对码</strong><p>请管理员在白板开启配对</p></div></li><li><span>02</span><div><strong>提交连接申请</strong><p>填写账号与当前设备名称</p></div></li><li><span>03</span><div><strong>等待本地确认</strong><p>管理员批准后即可连接</p></div></li></ol>
      </section>
      <Card className="pairing-card"><CardHeader><div className="card-icon"><MonitorUp size={23} strokeWidth={1.6} /></div><CardTitle>连接白板</CardTitle><CardDescription>使用已有教师账号，提交设备配对申请。</CardDescription></CardHeader><CardContent>
        <div className="feedback-region" aria-live="polite" aria-atomic="true">
          {view.phase === 'checking' && <div className="state-panel"><LoaderCircle className="spinner" size={28} /><h2>正在检查白板连接</h2><p>正在核对服务版本与当前会话。</p></div>}
          {view.phase === 'anonymous' && view.notice && <p className="notice"><CircleAlert size={17} />{view.notice}</p>}
          {view.phase === 'creating' && <div className="state-panel"><LoaderCircle className="spinner" size={30} /><h2>正在提交申请</h2><p>此次申请只提交一次，请稍候。</p><Button variant="outline" onClick={() => controller.current?.cancelPairing()}>停止等待</Button></div>}
          {view.phase === 'waiting' && <div className="state-panel waiting-panel"><div className="waiting-icon"><ShieldCheck size={35} /></div><span className="status-pill">申请已送达</span><h2>等待白板管理员批准</h2><p>请让管理员在白板核对你的教师账号和设备名称。配对窗口最长 2 分钟。</p><p className="request-id">申请编号 {view.requestId}</p><Button variant="outline" onClick={() => controller.current?.cancelPairing()}><X />停止等待</Button><small>停止等待不会替你批准或撤销白板上的申请。</small></div>}
          {view.phase === 'expired' && <div className="state-panel"><CircleAlert size={30} /><h2>本次等待已超时</h2><p>页面不会自动重新申请。请让管理员开启新的配对窗口，再主动申请。</p><Button onClick={() => controller.current?.cancelPairing()}>重新填写</Button></div>}
          {view.phase === 'error' && <div className="state-panel error-panel"><CircleAlert size={28} /><h2>{view.canPair ? '配对结果未确认' : '暂时无法连接白板'}</h2><p>{view.message}</p><Button variant="outline" onClick={() => view.canPair ? controller.current?.cancelPairing() : void controller.current?.connect()}><RefreshCw />{view.canPair ? '返回配对' : '重新检查连接'}</Button></div>}
          {view.phase === 'logging-out' && <div className="state-panel"><LoaderCircle className="spinner" size={30} /><h2>正在注销账号</h2><p>等待白板确认授权失效。</p></div>}
          {view.phase === 'logout-unknown' && <div className="state-panel error-panel"><CircleAlert size={30} /><h2>注销结果尚未确认</h2><p>页面已清除会话信息，但还不能确认白板是否完成注销。不会自动重复提交。</p><Button variant="outline" onClick={() => { void controller.current?.connect() }}><RefreshCw />核对白板会话</Button></div>}
        </div>
        {view.phase === 'anonymous' && <PairingForm enabled onSubmit={form => { void controller.current?.create(form) }} />}
        {view.phase !== 'anonymous' && <div className="secure-footnote"><LockKeyhole size={14} />只通过白板确认授权</div>}
      </CardContent></Card>
    </main>}
    <footer className="site-footer"><span>未来教室 <span className="footer-dot">·</span> 每一堂课，都有准备</span><span><LockKeyhole size={12} />账号与范围由白板本地管理</span></footer>
  </div>
}
