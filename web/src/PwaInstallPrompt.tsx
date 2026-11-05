import { useEffect, useRef, useState } from 'react'
import { installDismissKey, installGuidance, installHelpEvent } from './pwa-install'
import './pwa-install.css'

type InstallEvent = Event & { prompt(): Promise<void>; userChoice: Promise<{ outcome: 'accepted' | 'dismissed' }> }
export function isPwaInstalled(): boolean { return window.matchMedia('(display-mode: standalone)').matches || !!(navigator as Navigator & { standalone?: boolean }).standalone }
function dismissed(): boolean { try { return sessionStorage.getItem(installDismissKey) === 'yes' } catch { return false } }

export default function PwaInstallPrompt() {
  const [installed, setInstalled] = useState(isPwaInstalled)
  const [deferred, setDeferred] = useState<InstallEvent | null>(null)
  const pending = useRef<InstallEvent | null>(null)
  const [hidden, setHidden] = useState(dismissed)
  const [details, setDetails] = useState(false)
  const [busy, setBusy] = useState(false)
  const [notice, setNotice] = useState('')
  const guidance = installGuidance({ userAgent: navigator.userAgent, platform: navigator.platform, touchPoints: navigator.maxTouchPoints, standalone: installed })
  useEffect(() => {
    const media = window.matchMedia('(display-mode: standalone)')
    function beforeInstall(event: Event) { event.preventDefault(); pending.current = event as InstallEvent; setDeferred(event as InstallEvent) }
    function didInstall() { setInstalled(true); pending.current = null; setDeferred(null) }
    function changed() { setInstalled(isPwaInstalled()) }
    function showHelp() { void install() }
    window.addEventListener('beforeinstallprompt', beforeInstall)
    window.addEventListener('appinstalled', didInstall)
    window.addEventListener(installHelpEvent, showHelp)
    media.addEventListener('change', changed)
    return () => { window.removeEventListener('beforeinstallprompt', beforeInstall); window.removeEventListener('appinstalled', didInstall); window.removeEventListener(installHelpEvent, showHelp); media.removeEventListener('change', changed) }
  }, [])
  function later() { setHidden(true); setDetails(false); try { sessionStorage.setItem(installDismissKey, 'yes') } catch { /* The prompt still stays dismissed for this mounted page. */ } }
  async function install() {
    const event = pending.current
    if (!event) { setDetails(true); return }
    // Consume the single-use event before opening the prompt, in the original click gesture.
    pending.current = null; setDeferred(null); setBusy(true); setNotice('')
    try { await event.prompt(); const choice = await event.userChoice; if (choice.outcome === 'accepted') setHidden(true); else { setDetails(true); setNotice('已取消安装，可以继续使用网页；不会再次自动弹出安装窗口。') } }
    catch { setDetails(true); setNotice('浏览器暂时未能打开安装窗口，请按下面的方法添加，或继续使用网页。') }
    finally { setBusy(false) }
  }
  if (installed) return null
  if ((hidden || !guidance.mobile) && !details) return <div className="pwa-help"><button type="button" disabled={busy} onClick={() => { void install() }}>添加到桌面 / 安装帮助</button></div>
  return <aside className="pwa-install" aria-label="添加到桌面提示"><div><strong>添加到手机桌面，下次直接打开</strong><p className="pwa-install-steps">{guidance.steps}</p>{details && <div className="pwa-install-details"><p>无需安装也可正常使用，安装不会代替账号登录。</p><p>首次使用请先由管理员协助信任学校证书，再通过可信 HTTPS 地址访问。安装到桌面与证书信任是两步操作。</p><p>添加后先进入班级列表。入口界面和最近班级信息保存在手机本地，查看及管理教室时仍需连接学校网络。</p>{!window.isSecureContext && <p>当前页面不是可信安全连接，无法安装应用。请使用管理员提供并已完成证书信任的 HTTPS 地址。</p>}</div>}{notice && <p role="status">{notice}</p>}</div><div className="pwa-install-actions"><button type="button" disabled={busy} onClick={() => { void install() }}>{deferred ? '安装到桌面' : '连接与安装说明'}</button><button type="button" onClick={later}>稍后</button></div></aside>
}

export function registerPwaWorker() {
  if (!import.meta.env.PROD || !window.isSecureContext || !('serviceWorker' in navigator)) return
  // Failure never blocks the existing website. No credentials, sessions or teaching data enter the worker.
  void navigator.serviceWorker.register(`${import.meta.env.BASE_URL}sw.js`, { scope: import.meta.env.BASE_URL, updateViaCache: 'none' }).catch(() => {})
}
