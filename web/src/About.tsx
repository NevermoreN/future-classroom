import { useEffect, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { readJson, trustedApiBase } from './api/client'
import { ProtocolFault } from './api/protocol'
import './About.css'

export interface AboutInfo { schemaVersion: 1; version: string; automaticUpdates: boolean; updateAvailable: boolean; updateMessage: string; actionToken: string; phase: string; receivedBytes: number; totalBytes: number; groupId: string | null }
export function aboutInfo(raw: unknown): AboutInfo {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) throw new ProtocolFault()
  const value = raw as Record<string, unknown>
  const fields = ['schemaVersion', 'version', 'automaticUpdates', 'updateAvailable', 'updateMessage', 'actionToken', 'phase', 'receivedBytes', 'totalBytes', 'groupId']
  if (Object.keys(value).length !== fields.length || Object.keys(value).some(key => !fields.includes(key))
    || value.schemaVersion !== 1 || typeof value.automaticUpdates !== 'boolean' || typeof value.updateAvailable !== 'boolean'
    || typeof value.version !== 'string' || !/^[0-9]+\.[0-9]+\.[0-9]+(?:\.[0-9]+)?(?:[-+][0-9A-Za-z.-]+)?$/.test(value.version) || value.version.length > 128
    || typeof value.updateMessage !== 'string' || value.updateMessage.length === 0 || value.updateMessage.length > 1024
    || /[\u0000-\u001f\u007f]/u.test(value.updateMessage)
    || typeof value.actionToken !== 'string' || !/^(?:[0-9a-f]{64})?$/.test(value.actionToken)
    || typeof value.phase !== 'string' || !/^[A-Za-z0-9_-]{1,64}$/.test(value.phase)
    || ![value.receivedBytes, value.totalBytes].every(n => typeof n === 'number' && Number.isSafeInteger(n) && n >= 0 && n <= 134217728)
    || value.groupId !== null && (typeof value.groupId !== 'string' || !/^[0-9a-f]{32}$/.test(value.groupId))) throw new ProtocolFault()
  return value as unknown as AboutInfo
}
export default function About() {
  const [info, setInfo] = useState<AboutInfo | null>(null)
  const [error, setError] = useState(''), [busy, setBusy] = useState(false)
  const [refresh, setRefresh] = useState(0)
  useEffect(() => {
    const controller = new AbortController()
    const timer = setInterval(() => setRefresh(value => value + 1), 3000)
    async function load() {
      try {
        const response = await fetch(new URL('about', trustedApiBase(import.meta.env.BASE_URL, window.location.href)), { signal: controller.signal, credentials: 'omit', cache: 'no-store', redirect: 'error' })
        if (!response.ok) throw new Error('读取失败')
        const next = aboutInfo(await readJson(response, controller.signal))
        if (!controller.signal.aborted) { setInfo(next); setError('') }
      } catch { if (!controller.signal.aborted) setError('暂时无法连接未来教室。如果正在安装更新，程序重新启动后此页会自动恢复。') }
    }
    void load()
    return () => { clearInterval(timer); controller.abort() }
  }, [refresh])
  async function check() {
    if (!info?.actionToken || busy) return
    setBusy(true); setError('')
    const controller = new AbortController(), timer = setTimeout(() => controller.abort(), 15000)
    try {
      const base = trustedApiBase(import.meta.env.BASE_URL, window.location.href)
      if (base.protocol !== 'https:') throw new ProtocolFault()
      const response = await fetch(new URL('about/check', base), { method: 'POST', credentials: 'omit', cache: 'no-store', redirect: 'error', signal: controller.signal, headers: { 'Content-Type': 'application/json', 'X-FC-Update': info.actionToken }, body: JSON.stringify({ schemaVersion: 1 }) })
      if (!response.ok) throw new Error('请求未确认')
      setInfo(aboutInfo(await readJson(response, controller.signal))); setRefresh(value => value + 1)
    } catch { setError('更新检查尚未确认，请查看下方状态。程序重启后会自动重新连接。') }
    finally { clearTimeout(timer); setBusy(false) }
  }
  return <main id="main-content" className="about-page"><Card className="about-card"><CardHeader><CardTitle role="heading" aria-level={1}>关于未来教室</CardTitle><CardDescription>查看本机版本与更新状态</CardDescription></CardHeader><CardContent>
    <p className="about-version">{info ? `版本 ${info.version}` : '正在读取版本…'}</p>
    {info && <p className="about-update-status">自动更新：{info.automaticUpdates ? '已开启' : '已关闭'}</p>}
    <p className="about-message" role={error ? 'alert' : 'status'} aria-live="polite">{error || info?.updateMessage || '正在读取更新状态…'}</p>
    {info && info.totalBytes > 0 && <p>下载进度：{Math.min(100, Math.floor(info.receivedBytes / info.totalBytes * 100))}%</p>}
    <div className="about-actions"><Button type="button" disabled={busy || !info?.actionToken} onClick={() => { void check() }}>{busy ? '正在检查…' : '立即更新'}</Button><a href="#/updates">软件更新设置</a></div>
  </CardContent></Card></main>
}
