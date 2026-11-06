import AccountSecurityPanel, { TransferOutcomePanel } from './AccountSecurityPanel'
import RecoveryCodeNotice from './RecoveryCodeNotice'
import ForgotPasswordPanel from './ForgotPasswordPanel'
import RepairPanel from './RepairPanel'
import { repairBootstrap } from './api/repair-client'
import MaintenancePanel from './MaintenancePanel'
import { loadMaintenance } from './api/maintenance-client'
import { useEffect, useRef, useState, type FormEvent } from 'react'
import { ArrowRight, BookOpen, Check, ChevronRight, CircleAlert, FolderOpen, GraduationCap, LoaderCircle, LockKeyhole, MonitorUp, Radio, RefreshCw, Smartphone, Sparkles, UserRound } from './components/icons'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { HttpApi, trustedApiBase } from './api/client'
import { SessionController, type ViewState } from './api/controller'
import TeacherWorkspace from './TeacherWorkspace'
import QuickConnect from './QuickConnect'
import About from './About'
import SoftwareUpdates from './SoftwareUpdates'
import ClassroomHome from './ClassroomHome'
import { installHelpEvent } from './pwa-install'
import { isPwaInstalled } from './PwaInstallPrompt'
function Brand() { return <a className="brand" href={import.meta.env.BASE_URL} aria-label="未来教室首页"><span className="brand-icon"><GraduationCap size={25} strokeWidth={1.8} /></span><span>未来教室<small>TEACHER WORKSPACE</small></span></a> }
function LoginForm({ enabled, onSubmit }: { enabled: boolean; onSubmit: (form: { loginName: string; password: string }) => void }) {
  const [loginName, setLoginName] = useState(''), [password, setPassword] = useState('')
  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); if (!enabled) return
    onSubmit({ loginName, password }); setPassword('')
  }
  return <form onSubmit={submit} className="pair-form">
    <div><label htmlFor="login-name">用户名</label><Input id="login-name" name="username" autoComplete="username" maxLength={64} placeholder="输入已有账号" value={loginName} onChange={e => setLoginName(e.target.value)} required disabled={!enabled} /></div>
    <div><label htmlFor="login-password">密码</label><Input id="login-password" name="password" type="password" autoComplete="current-password" maxLength={128} placeholder="输入账号密码" value={password} onChange={e => setPassword(e.target.value)} required disabled={!enabled} /></div>
    <Button type="submit" className="pair-submit" disabled={!enabled}>登录<ArrowRight size={18} /></Button>
    <p className="form-note"><LockKeyhole size={15} />当前页面无需重复登录，关闭后重新打开需再次登录。</p>
  </form>
}
function SetupForm({ onSubmit }: { onSubmit: (form: { schoolName: string; loginName: string; displayName: string; password: string }) => void }) {
  const [schoolName, setSchoolName] = useState(''), [loginName, setLoginName] = useState(''), [displayName, setDisplayName] = useState(''), [password, setPassword] = useState(''), [confirmation, setConfirmation] = useState(''), [error, setError] = useState('')
  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (password !== confirmation) { setError('两次输入的密码不一致。'); return }
    if ([...password].length < 12 || [...password].length > 128) { setError('管理员密码需要 12–128 个字符。'); return }
    setError(''); onSubmit({ schoolName, loginName, displayName, password }); setPassword(''); setConfirmation('')
  }
  return <form className="pair-form" onSubmit={submit}>{error && <p role="alert" className="notice">{error}</p>}
    <div><label htmlFor="setup-school">学校名称</label><Input id="setup-school" required maxLength={128} value={schoolName} onChange={e => setSchoolName(e.target.value)} /></div>
    <div><label htmlFor="setup-login">管理员用户名</label><Input id="setup-login" name="username" autoComplete="username" required maxLength={64} value={loginName} onChange={e => setLoginName(e.target.value)} /></div>
    <div><label htmlFor="setup-name">管理员显示名称</label><Input id="setup-name" required maxLength={128} value={displayName} onChange={e => setDisplayName(e.target.value)} /></div>
    <div><label htmlFor="setup-password">管理员密码<span>12–128 个字符</span></label><Input id="setup-password" type="password" name="new-password" autoComplete="new-password" required maxLength={256} value={password} onChange={e => setPassword(e.target.value)} /></div>
    <div><label htmlFor="setup-confirm">再次输入密码</label><Input id="setup-confirm" type="password" autoComplete="new-password" required maxLength={256} value={confirmation} onChange={e => setConfirmation(e.target.value)} /></div>
    <Button type="submit">创建管理员并进入设置<ArrowRight size={18} /></Button><p className="form-note">首次初始化只能在白板本机进行，之后可使用此账号登录管理。</p>
  </form>
}
function PasswordForm({ required, busy, notice, onSubmit, onCancel, onLogout }: { required: boolean; busy: boolean; notice?: string | undefined; onSubmit: (form: { currentPassword: string; newPassword: string }) => void; onCancel: () => void; onLogout: () => void }) {
  const [currentPassword, setCurrentPassword] = useState(''), [newPassword, setNewPassword] = useState(''), [confirmation, setConfirmation] = useState(''), [error, setError] = useState('')
  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); if (busy) return
    if (newPassword !== confirmation) { setError('两次输入的新密码不一致。'); return }
    if ([...newPassword].length < 12 || [...newPassword].length > 128) { setError('新密码需要 12–128 个字符。'); return }
    setError(''); onSubmit({ currentPassword, newPassword }); setCurrentPassword(''); setNewPassword(''); setConfirmation('')
  }
  return <main id="main-content" className="password-change-layout"><Card className="pairing-card"><CardHeader><div className="card-icon"><LockKeyhole size={23} /></div><CardTitle>{required ? '设置你的新密码' : '修改账号密码'}</CardTitle><CardDescription>{required ? '此账号需要先更改初始密码，再进入教室管理。' : '修改成功后，本页会自动继续登录；其他旧会话将失效。'}</CardDescription></CardHeader><CardContent>
    <form className="pair-form" onSubmit={submit}>
      {(error || notice) && <p className="notice" role="alert">{error || notice}</p>}
      <div><label htmlFor="current-password">当前密码</label><Input id="current-password" type="password" name="current-password" autoComplete="current-password" maxLength={128} value={currentPassword} onChange={e => setCurrentPassword(e.target.value)} required disabled={busy} /></div>
      <div><label htmlFor="new-password">新密码<span>12–128 个字符</span></label><Input id="new-password" type="password" name="new-password" autoComplete="new-password" maxLength={256} value={newPassword} onChange={e => setNewPassword(e.target.value)} required disabled={busy} /></div>
      <div><label htmlFor="confirm-password">再次输入新密码</label><Input id="confirm-password" type="password" name="confirm-password" autoComplete="new-password" maxLength={256} value={confirmation} onChange={e => setConfirmation(e.target.value)} required disabled={busy} /></div>
      <Button type="submit" disabled={busy}>{busy && <LoaderCircle className="spinner" />}保存新密码</Button>
      {!required && <Button type="button" variant="outline" disabled={busy} onClick={onCancel}>返回管理页面</Button>}
      <Button type="button" variant="ghost" disabled={busy} onClick={onLogout}>退出登录</Button>
    </form>
  </CardContent></Card></main>
}
export default function App() {
  const [route, setRoute] = useState(() => window.location.hash)
  const [installed, setInstalled] = useState(isPwaInstalled)
  useEffect(() => {
    const changed = () => setRoute(window.location.hash)
    window.addEventListener('hashchange', changed)
    const media = window.matchMedia('(display-mode: standalone)')
    const installationChanged = () => setInstalled(isPwaInstalled())
    const didInstall = () => setInstalled(true)
    media.addEventListener('change', installationChanged)
    window.addEventListener('appinstalled', didInstall)
    return () => { window.removeEventListener('hashchange', changed); media.removeEventListener('change', installationChanged); window.removeEventListener('appinstalled', didInstall) }
  }, [])
  if (route === '#/connect') return <QuickConnect />
  if (route === '#/about') return <About />
  const loopback = ['127.0.0.1', 'localhost', '[::1]'].includes(window.location.hostname)
  if (route === '#/updates' && !loopback) return <main className="software-updates"><h1>软件更新</h1><p>请在教室电脑上右键未来教室图标，打开设置后管理软件更新。</p><a href="#/rooms">返回班级首页</a></main>
  if (route === '#/rooms' || !loopback && ['', '#', '#/'].includes(route)) return <ClassroomHome installed={installed} onInstallHelp={() => window.dispatchEvent(new Event(installHelpEvent))} />
  return <>{route === '#/classroom' && <nav className="pwa-help" aria-label="班级切换"><a href="#/rooms">← 返回班级列表</a></nav>}<ManagementApp updates={route === '#/updates'} localControl={loopback} /></>
}
function ManagementApp({ updates, localControl }: { updates: boolean; localControl: boolean }) {
  const tutorial = useRef<HTMLDialogElement>(null)
  const [forgot, setForgot] = useState(false)
  const [repair] = useState(repairBootstrap)
  const [resumeMaintenance, setResumeMaintenance] = useState(() => !!loadMaintenance())
  const [view, setView] = useState<ViewState>({ phase: 'checking' })
  const controller = useRef<SessionController | undefined>(undefined)
  useEffect(() => {
    if (repair) return
    try {
      const api = new HttpApi(trustedApiBase(import.meta.env.BASE_URL, window.location.href))
      const owner = new SessionController(api, setView); controller.current = owner; void owner.connect()
      return () => { owner.dispose(); controller.current = undefined }
    } catch { setView({ phase: 'error', message: '页面的服务地址无效，请使用管理员提供的可信 HTTPS 地址。' }) }
  }, [])
  if (repair) return <RepairPanel boot={repair} />
  if ((resumeMaintenance || !!loadMaintenance()) && (view.phase === 'anonymous' || view.phase === 'error' || view.phase === 'logout-unknown')) return <div className="app-shell"><header className="site-header"><Brand /><span className="workspace-label">本机维护进度</span></header><main id="main-content" className="workspace"><MaintenancePanel session={null} onClose={() => { setResumeMaintenance(false); void controller.current?.connect() }} /></main></div>
  const busy = ['checking', 'logging-in', 'logging-out', 'setting-up'].includes(view.phase)
  const connected = ['anonymous', 'setup', 'setting-up', 'logging-in', 'authenticated', 'logging-out', 'logout-unknown'].includes(view.phase)
  if (updates && view.phase === 'authenticated' && !view.session.mustChangePassword && !view.session.recoveryCode && !view.passwordChange) return <SoftwareUpdates session={view.session} onRevoked={() => controller.current?.invalidate()} />
  return <div className={view.phase === 'authenticated' ? 'app-shell authenticated-shell' : 'app-shell'}><div className="sr-only" role="status" aria-live="polite">{view.phase === 'authenticated' ? '白板已确认登录，管理页面已就绪。' : view.phase === 'logout-unknown' ? '注销结果尚未确认，需要主动核对白板会话。' : ''}</div><a className="skip-link" href="#main-content">跳到主要内容</a>
    <header className="site-header">{view.phase === 'authenticated' ? <div className="teacher-brand"><BookOpen size={30} /><strong>{view.session.principal.role === 'GLOBAL_ADMIN' ? '全局主管理员' : view.session.principal.role === 'TEACHER_ADMIN' ? '教师管理员' : '普通用户'} · {view.session.principal.displayName}</strong></div> : <Brand />}<div className="header-right"><Button type="button" variant="outline" size="sm" aria-haspopup="dialog" onClick={() => tutorial.current?.showModal()}>使用教程</Button><span className={`connection-status ${connected ? 'connected' : ''}`}><span />{connected ? '白板连接已核验' : busy ? '正在检查连接' : '连接待核验'}</span>{view.phase === 'authenticated' ? <span className="teacher-identity"><UserRound size={20} />{view.session.principal.displayName}</span> : <span className="workspace-label">教室管理</span>}</div></header>
    <dialog ref={tutorial} className="tutorial-dialog" aria-labelledby="tutorial-title"><div className="tutorial-dialog-heading"><h2 id="tutorial-title">使用教程</h2><Button type="button" variant="ghost" size="sm" onClick={() => tutorial.current?.close()}>关闭</Button></div><div className="tutorial-dialog-content" /></dialog>
    {view.phase === 'authenticated' && <TransferOutcomePanel session={view.session} />}
    {localControl && view.phase === 'authenticated' && view.session.principal.role === 'GLOBAL_ADMIN' && <nav className="pwa-help" aria-label="软件维护"><a href="#/updates">软件更新</a></nav>}
    {view.phase === 'authenticated' && view.notice && <p className="notice" role="status">{view.notice}</p>}
    {view.phase === 'authenticated' ? view.session.recoveryCode ? <main className="workspace"><RecoveryCodeNotice code={view.session.recoveryCode} onDone={() => controller.current?.acknowledgeRecoveryCode()} /></main> : view.session.mustChangePassword || view.passwordChange ? <>{!view.session.mustChangePassword && <AccountSecurityPanel session={view.session} onRevoked={() => controller.current?.invalidate()} />}<PasswordForm required={!!view.session.mustChangePassword} busy={!!view.passwordBusy} notice={view.passwordNotice} onSubmit={form => { void controller.current?.changePassword(form) }} onCancel={() => controller.current?.cancelPasswordChange()} onLogout={() => { void controller.current?.logout() }} /></> : <TeacherWorkspace onPasswordChange={() => controller.current?.showPasswordChange()} session={view.session} onLogout={() => { void controller.current?.logout() }} onRefresh={() => { void controller.current?.connect() }} onRevoked={() => controller.current?.invalidate()} /> : <main id="main-content" className="pairing-layout">
      <section className="intro-panel"><span className="intro-tag"><Sparkles size={14} />让教学，轻松一步</span><h1>连接你的教室，<br />专注每一堂课<span>。</span></h1><p className="intro-description">从自己的设备安全连接白板。<br />课堂里的准备与协作，从这里开始。</p>
        <div className="classroom-illustration" aria-hidden="true"><div className="board-top"><span /><span /><span /></div><div className="board-content"><span className="board-caption">未来教室</span><div className="board-line long" /><div className="board-line short" /><div className="board-tiles"><span><BookOpen size={28} /></span><span><Radio size={26} /></span><span><FolderOpen size={28} /></span></div></div><div className="device-bubble"><Smartphone size={24} /><span>你的设备</span><i><Check size={12} /></i></div><div className="board-stand" /></div>
        <ol className="pairing-steps"><li><span>01</span><div><strong>使用已有账号</strong><p>输入用户名与密码即可登录</p></div></li><li><span>02</span><div><strong>管理你的教室</strong><p>按账号权限查看和管理内容</p></div></li><li><span>03</span><div><strong>专注当前工作</strong><p>切换页面或刷新无需重复登录</p></div></li></ol>
      </section>
      <Card className="pairing-card"><CardHeader><div className="card-icon"><MonitorUp size={23} strokeWidth={1.6} /></div><CardTitle role="heading" aria-level={2}>{view.phase === 'setup' || view.phase === 'setting-up' ? '初始化未来教室' : '登录未来教室'}</CardTitle><CardDescription>{view.phase === 'setup' || view.phase === 'setting-up' ? '为这台白板设置学校与首位管理员。' : '使用用户名和密码进入教室管理页面。'}</CardDescription></CardHeader><CardContent>
        <div className="feedback-region" aria-live="polite" aria-atomic="true">
          {view.phase === 'checking' && <div className="state-panel"><LoaderCircle className="spinner" size={28} /><h2>正在检查白板连接</h2><p>正在核对服务版本与当前会话。</p></div>}
          {(view.phase === 'anonymous' || view.phase === 'setup') && view.notice && <p className="notice"><CircleAlert size={17} />{view.notice}</p>}
          {view.phase === 'setting-up' && <div className="state-panel"><LoaderCircle className="spinner" size={30} /><h2>正在初始化</h2><p>正在建立学校与管理员账号，请稍候。</p></div>}
          {view.phase === 'logging-in' && <div className="state-panel"><LoaderCircle className="spinner" size={30} /><h2>正在登录</h2><p>正在核对账号，请稍候。</p></div>}
          {view.phase === 'error' && <div className="state-panel error-panel"><CircleAlert size={28} /><h2>{view.refreshRequired ? '需要刷新页面' : '暂时无法连接白板'}</h2><p>{view.message}</p><Button variant="outline" onClick={() => view.refreshRequired ? window.location.reload() : void controller.current?.connect()}><RefreshCw />{view.refreshRequired ? '刷新页面' : '重新连接'}</Button></div>}
          {view.phase === 'logging-out' && <div className="state-panel"><LoaderCircle className="spinner" size={30} /><h2>正在注销账号</h2><p>等待白板确认授权失效。</p></div>}
          {view.phase === 'logout-unknown' && <div className="state-panel error-panel"><CircleAlert size={30} /><h2>注销结果尚未确认</h2><p>页面已清除会话信息，但还不能确认白板是否完成注销。不会自动重复提交。</p><Button variant="outline" onClick={() => { void controller.current?.connect() }}><RefreshCw />核对白板会话</Button></div>}
        </div>
        {view.phase === 'setup' && <><SetupForm onSubmit={form => { void controller.current?.setup(form) }} /><Button variant="ghost" onClick={() => { void controller.current?.connect() }}>重新检查初始化状态</Button></>}
        {view.phase === 'anonymous' && (forgot ? <ForgotPasswordPanel done={() => setForgot(false)} /> : <><LoginForm enabled onSubmit={form => { void controller.current?.login(form) }} /><Button variant="ghost" onClick={() => setForgot(true)}>忘记密码？使用恢复码</Button></>)}
        {view.phase !== 'anonymous' && <div className="secure-footnote"><LockKeyhole size={14} />通过加密连接验证账号</div>}
      </CardContent></Card>
    </main>}
    <footer className="site-footer"><span>未来教室 <span className="footer-dot">·</span> 每一堂课，都有准备</span><span><LockKeyhole size={12} />账号权限由教室管理员管理</span></footer>
  </div>
}
