import { useEffect, useState } from 'react'
import { QRCodeSVG } from 'qrcode.react'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { readJson, trustedApiBase } from './api/client'
import { ProtocolFault } from './api/protocol'
import './QuickConnect.css'

export interface ConnectionInfo { schoolName: string; className: string | null; entryUrl: string | null; unavailableReason: string | null }

export function connectionInfo(raw: unknown): ConnectionInfo {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) throw new ProtocolFault()
  const value = raw as Record<string, unknown>
  const fields = ['schemaVersion', 'schoolName', 'className', 'entryUrl', 'unavailableReason']
  if (Object.keys(value).length !== fields.length || Object.keys(value).some(key => !fields.includes(key))) throw new ProtocolFault()
  if (value.schemaVersion !== 1) throw new ProtocolFault()
  const text = (item: unknown, nullable: boolean): item is string | null => item === null ? nullable : typeof item === 'string' && item.length > 0 && item.length <= 512 && !/[\u0000-\u001f\u007f]/u.test(item)
  if (!text(value.schoolName, false) || !text(value.className, true) || !text(value.unavailableReason, true)) throw new ProtocolFault()
  if (value.entryUrl !== null) {
    if (typeof value.entryUrl !== 'string' || value.entryUrl.length > 1024) throw new ProtocolFault()
    let url: URL
    try { url = new URL(value.entryUrl) } catch { throw new ProtocolFault() }
    const host = url.hostname.toLowerCase()
    const v4 = /^(\d{1,3}\.){3}\d{1,3}$/.test(host), v6 = /^\[[0-9a-f:]+\]$/.test(host)
    if (url.protocol !== 'https:' || url.username || url.password || url.search || url.hash || url.pathname !== '/fc/'
      || (!v4 && !v6) || /^(127\.|0\.|169\.254\.)/.test(host) || (v4 && Number(host.split('.')[0]) >= 224)
      || ['[::]', '[::1]'].includes(host) || host.startsWith('[ff') || host.startsWith('[::ffff:')) throw new ProtocolFault()
  } else if (value.unavailableReason === null) throw new ProtocolFault()
  return value as unknown as ConnectionInfo
}

export function ConnectionDetails({ info }: { info: ConnectionInfo }) {
  const [copyNotice, setCopyNotice] = useState('')
  useEffect(() => setCopyNotice(''), [info.entryUrl])
  async function copy() {
    try { await navigator.clipboard.writeText(info.entryUrl!); setCopyNotice('地址已复制。') }
    catch { setCopyNotice('未能自动复制，请选中下方完整地址后复制。') }
  }
  return <>
    <p className="quick-connect-class"><strong>{info.schoolName}</strong><span>{info.className ?? '尚未绑定班级'}</span></p>
    {info.entryUrl ? <>
      <div className="quick-connect-qr"><QRCodeSVG value={info.entryUrl} size={288} marginSize={4} level="M" title="扫码打开本教室网站" /></div>
      <p className="quick-connect-kind">本教室直连</p>
      <p>请使用手机相机扫码，或复制完整地址在浏览器中打开。</p>
      <p className="quick-connect-address"><a href={info.entryUrl} target="_blank" rel="noopener noreferrer">{info.entryUrl}</a></p>
      <div className="quick-connect-actions"><Button type="button" onClick={() => { void copy() }}>复制地址</Button><a className="quick-connect-open" href={info.entryUrl} target="_blank" rel="noopener noreferrer">打开网站</a></div>
      <p role="status">{copyNotice}</p>
      <p className="quick-connect-note">手机需连接学校网络。入口已由本机确认准备好，手机是否能访问还取决于校园网络和 Wi-Fi 隔离设置。</p>
    </> : <p className="notice" role="status">{info.unavailableReason} 请管理员完成连接设置，再刷新本页。</p>}
    <section className="quick-connect-trust"><h2>首次使用</h2><ol><li>先请学校管理员提供学校根证书及指纹，按照手机系统说明安装并信任。不要忽略证书警告继续登录。</li><li>打开网站后，可按提示“安装到手机”或“添加到主屏幕”，以后从图标直接打开；也可以稍后安装，先使用网页。</li><li>登录时使用已有用户名和密码。二维码仅包含公开网址，不会自动登录。</li></ol></section>
  </>
}

export default function QuickConnect() {
  const [info, setInfo] = useState<ConnectionInfo | null>(null), [error, setError] = useState(''), [refresh, setRefresh] = useState(0)
  useEffect(() => {
    const update = () => { if (document.visibilityState === 'visible') setRefresh(value => value + 1) }
    const timer = window.setInterval(update, 5000)
    document.addEventListener('visibilitychange', update)
    return () => { window.clearInterval(timer); document.removeEventListener('visibilitychange', update) }
  }, [])
  useEffect(() => {
    const controller = new AbortController()
    setError('')
    async function load() {
      try {
        const base = trustedApiBase(import.meta.env.BASE_URL, window.location.href)
        const response = await fetch(new URL('quick-connect', base), { signal: controller.signal, credentials: 'omit', cache: 'no-store', redirect: 'error' })
        if (!response.ok) throw new Error('连接信息尚未就绪，请稍后刷新。')
        const next = connectionInfo(await readJson(response, controller.signal))
        if (!controller.signal.aborted) setInfo(next)
      } catch { if (!controller.signal.aborted) { setInfo(null); setError('暂时无法读取连接信息。请在教室电脑上通过托盘“快捷连接”打开，并稍后刷新。') } }
    }
    void load()
    return () => controller.abort()
  }, [refresh])
  return <main id="main-content" className="quick-connect-page"><Card className="quick-connect-card"><CardHeader><CardTitle>快捷连接</CardTitle><CardDescription>老师扫码即可打开教室网站，无需手动输入地址。</CardDescription></CardHeader><CardContent>
    {info ? <ConnectionDetails info={info} /> : <p role="status">{error || '正在读取可用入口…'}</p>}
    <Button type="button" variant="outline" onClick={() => setRefresh(value => value + 1)}>刷新连接信息</Button>
  </CardContent></Card></main>
}
