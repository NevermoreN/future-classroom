import { useEffect, useRef, useState, type FormEvent } from 'react'
import { ArrowRight, BookOpen, Check, ChevronRight, CircleAlert, ClipboardList, FolderOpen, GraduationCap, LoaderCircle, LockKeyhole, LogOut, MonitorUp, Radio, RefreshCw, ShieldCheck, Smartphone, Sparkles, X } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { HttpApi, trustedApiBase } from './api/client'
import { PairingController, type ViewState } from './api/controller'
import TeacherWorkspace from './TeacherWorkspace'
function Brand() { return <a className="brand" href={import.meta.env.BASE_URL} aria-label="未来教室首页"><span className="brand-icon"><GraduationCap size={25} strokeWidth={1.8} /></span><span>未来教室<small>TEACHER WORKSPACE</small></span></a> }
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
  return <div className={view.phase === 'authenticated' ? 'app-shell authenticated-shell' : 'app-shell'}><div className="sr-only" role="status" aria-live="polite">{view.phase === 'authenticated' ? '白板已确认登录，教师工作台已就绪。' : view.phase === 'logout-unknown' ? '注销结果尚未确认，需要主动核对白板会话。' : ''}</div><a className="skip-link" href="#main-content">跳到主要内容</a>
    <header className="site-header"><Brand /><div className="header-right"><span className={`connection-status ${connected ? 'connected' : ''}`}><span />{connected ? '白板连接已核验' : busy ? '正在检查连接' : '连接待核验'}</span><span className="workspace-label">教师工作台</span></div></header>
    {view.phase === 'authenticated' ? <TeacherWorkspace session={view.session} onLogout={() => { void controller.current?.logout() }} onRefresh={() => { void controller.current?.connect() }} onRevoked={() => controller.current?.invalidate()} /> : <main id="main-content" className="pairing-layout">
      <section className="intro-panel"><span className="intro-tag"><Sparkles size={14} />让教学，轻松一步</span><h1>连接你的教室，<br />专注每一堂课<span>。</span></h1><p className="intro-description">从自己的设备安全连接白板。<br />课堂里的准备与协作，从这里开始。</p>
        <div className="classroom-illustration" aria-hidden="true"><div className="board-top"><span /><span /><span /></div><div className="board-content"><span className="board-caption">未来教室</span><div className="board-line long" /><div className="board-line short" /><div className="board-tiles"><span><BookOpen size={28} /></span><span><Radio size={26} /></span><span><FolderOpen size={28} /></span></div></div><div className="device-bubble"><Smartphone size={24} /><span>你的设备</span><i><Check size={12} /></i></div><div className="board-stand" /></div>
        <ol className="pairing-steps"><li><span>01</span><div><strong>获取配对码</strong><p>请管理员在白板开启配对</p></div></li><li><span>02</span><div><strong>提交连接申请</strong><p>填写账号与当前设备名称</p></div></li><li><span>03</span><div><strong>等待本地确认</strong><p>管理员批准后即可连接</p></div></li></ol>
      </section>
      <Card className="pairing-card"><CardHeader><div className="card-icon"><MonitorUp size={23} strokeWidth={1.6} /></div><CardTitle role="heading" aria-level={2}>连接白板</CardTitle><CardDescription>使用已有教师账号，提交设备配对申请。</CardDescription></CardHeader><CardContent>
        <div className="feedback-region" aria-live="polite" aria-atomic="true">
          {view.phase === 'checking' && <div className="state-panel"><LoaderCircle className="spinner" size={28} /><h2>正在检查白板连接</h2><p>正在核对服务版本与当前会话。</p></div>}
          {view.phase === 'anonymous' && view.notice && <p className="notice"><CircleAlert size={17} />{view.notice}</p>}
          {view.phase === 'creating' && <div className="state-panel"><LoaderCircle className="spinner" size={30} /><h2>正在提交申请</h2><p>此次申请只提交一次，请稍候。</p><Button variant="outline" onClick={() => controller.current?.cancelPairing()}>停止等待</Button></div>}
          {view.phase === 'waiting' && <div className="state-panel waiting-panel"><div className="waiting-icon"><ShieldCheck size={35} /></div><span className="status-pill">申请已送达</span><h2>等待白板管理员批准</h2><p>请让管理员在白板核对你的教师账号和设备名称。配对窗口最长 2 分钟。</p><p className="request-id">申请编号 {view.requestId}</p><Button variant="outline" onClick={() => controller.current?.cancelPairing()}><X />停止等待</Button><small>停止等待不会替你批准或撤销白板上的申请。</small></div>}
          {view.phase === 'expired' && <div className="state-panel"><CircleAlert size={30} /><h2>本次等待已超时</h2><p>页面不会自动重新申请。请让管理员开启新的配对窗口，再主动申请。</p><Button onClick={() => controller.current?.cancelPairing()}>重新填写</Button></div>}
          {view.phase === 'error' && <div className="state-panel error-panel"><CircleAlert size={28} /><h2>{view.refreshRequired ? '需要刷新教师工作台' : view.canPair ? '配对结果未确认' : '暂时无法连接白板'}</h2><p>{view.message}</p><Button variant="outline" onClick={() => view.refreshRequired ? window.location.reload() : view.canPair ? controller.current?.cancelPairing() : void controller.current?.connect()}><RefreshCw />{view.refreshRequired ? '刷新页面' : view.canPair ? '返回配对' : '重新检查连接'}</Button></div>}
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
