import { useEffect, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { readJson, trustedApiBase } from './api/client'
import { ProtocolFault } from './api/protocol'
import './About.css'

export interface AboutInfo { schemaVersion: 1; version: string; automaticUpdates: boolean; updateAvailable: boolean; updateMessage: string }

export function aboutInfo(raw: unknown): AboutInfo {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) throw new ProtocolFault()
  const value = raw as Record<string, unknown>
  const fields = ['schemaVersion', 'version', 'automaticUpdates', 'updateAvailable', 'updateMessage']
  if (Object.keys(value).length !== fields.length || Object.keys(value).some(key => !fields.includes(key))
    || value.schemaVersion !== 1 || typeof value.automaticUpdates !== 'boolean' || typeof value.updateAvailable !== 'boolean'
    || typeof value.version !== 'string' || !/^[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?$/.test(value.version) || value.version.length > 128
    || typeof value.updateMessage !== 'string' || value.updateMessage.length === 0 || value.updateMessage.length > 1024
    || /[\u0000-\u001f\u007f]/u.test(value.updateMessage)) throw new ProtocolFault()
  return value as unknown as AboutInfo
}

export default function About() {
  const [info, setInfo] = useState<AboutInfo | null>(null)
  const [error, setError] = useState(''), [busy, setBusy] = useState(true)
  const [refresh, setRefresh] = useState(0), [requestedUpdate, setRequestedUpdate] = useState(false)
  useEffect(() => {
    const controller = new AbortController()
    setBusy(true); setError('')
    async function load() {
      try {
        const base = trustedApiBase(import.meta.env.BASE_URL, window.location.href)
        const response = await fetch(new URL('about', base), { signal: controller.signal, credentials: 'omit', cache: 'no-store', redirect: 'error' })
        if (!response.ok) throw new Error('读取失败')
        const next = aboutInfo(await readJson(response, controller.signal))
        if (!controller.signal.aborted) setInfo(next)
      } catch {
        if (!controller.signal.aborted) setError('暂时无法读取本机版本和更新状态，请确认未来教室仍在运行，然后重试。')
      } finally { if (!controller.signal.aborted) setBusy(false) }
    }
    void load()
    return () => controller.abort()
  }, [refresh])
  return <main id="main-content" className="about-page"><Card className="about-card"><CardHeader><CardTitle role="heading" aria-level={1}>关于未来教室</CardTitle><CardDescription>查看本机版本与更新状态</CardDescription></CardHeader><CardContent>
    <p className="about-version">{info ? `版本 ${info.version}` : busy ? '正在读取版本…' : '版本暂时无法读取'}</p>
    {info && <p className="about-update-status">自动更新：{info.automaticUpdates ? '默认开启' : '已关闭'}</p>}
    <p className="about-message" role={error ? 'alert' : 'status'} aria-live="polite">{error || (busy && requestedUpdate ? '正在读取更新状态…' : info?.updateMessage || '正在读取更新状态…')}</p>
    <div className="about-actions"><Button type="button" disabled={busy} onClick={() => { setRequestedUpdate(true); setBusy(true); setRefresh(value => value + 1) }}>{busy && requestedUpdate ? '正在读取…' : '立即更新'}</Button>
      {error && <Button type="button" variant="outline" disabled={busy} onClick={() => { setBusy(true); setRefresh(value => value + 1) }}>重新读取</Button>}
    </div>
  </CardContent></Card></main>
}
