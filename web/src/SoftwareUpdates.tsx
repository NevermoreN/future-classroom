import { useEffect, useRef, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { ApiError, pageHeaders, readJson, trustedApiBase } from './api/client'
import { decimal, errorCode, ProtocolFault, type Session } from './api/protocol'
import { issued } from './api/command-protocol'
import { aboutInfo, type AboutInfo } from './About'
import './SoftwareUpdates.css'
interface Policy { enabled: boolean; download: boolean; share: boolean; installMode: 'automatic' | 'manual' | 'notify' | 'maintenance' }
interface Status extends AboutInfo, Policy { revision: string; policyRevision: string }
function status(raw: unknown): Status {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) throw new ProtocolFault()
  const value = raw as Record<string, unknown>, { revision, policyRevision, enabled, download, share, installMode, ...runtime } = value
  aboutInfo(runtime); if (revision !== '0') decimal(revision); if (policyRevision !== '0') decimal(policyRevision)
  if (typeof enabled !== 'boolean' || typeof download !== 'boolean' || typeof share !== 'boolean' || !['automatic', 'manual', 'notify', 'maintenance'].includes(installMode as string)) throw new ProtocolFault()
  return value as unknown as Status
}
export default function SoftwareUpdates({ session, onRevoked }: { session: Session; onRevoked: () => void }) {
  const [info, setInfo] = useState<Status | null>(null), [draft, setDraft] = useState<Policy | null>(null)
  const [busy, setBusy] = useState(false), [notice, setNotice] = useState(''), [uncertain, setUncertain] = useState(false)
  const [group, setGroup] = useState(''), [exported, setExported] = useState(''), [file, setFile] = useState<File | null>(null), [progress, setProgress] = useState(0)
  const active = useRef<AbortController | null>(null), uploadToken = useRef<string | null>(null)
  const base = () => trustedApiBase(import.meta.env.BASE_URL, window.location.href)
  async function request(path: string, signal: AbortSignal, input?: unknown, binary?: Blob): Promise<unknown> {
    if (base().protocol !== 'https:') throw new ProtocolFault()
    const headers = new Headers({ Accept: 'application/json', ...pageHeaders() })
    if (input !== undefined || binary) { headers.set('X-FC-CSRF', session.csrf); headers.set('Content-Type', binary ? 'application/octet-stream' : 'application/json') }
    const response = await fetch(new URL(path, base()), { method: input === undefined && !binary ? 'GET' : 'POST', credentials: 'same-origin', cache: 'no-store', redirect: 'error', signal, headers, ...(binary ? { body: binary } : input === undefined ? {} : { body: JSON.stringify(input) }) })
    if (response.status === 401 || response.status === 403) { await response.body?.cancel(); onRevoked(); throw new ApiError(response.status, response.status === 401 ? 'UNAUTHORIZED' : 'FORBIDDEN') }
    const value = await readJson(response, signal)
    if (!response.ok) throw new ApiError(response.status, errorCode(value))
    return value
  }
  async function load(signal: AbortSignal, resetDraft = false) {
    const next = status(await request('updates/status', signal)); setInfo(next)
    if (resetDraft) setDraft({ enabled: next.enabled, download: next.download, share: next.share, installMode: next.installMode })
  }
  useEffect(() => {
    const controller = new AbortController()
    void load(controller.signal, true).catch(() => { if (!controller.signal.aborted) setNotice('暂时无法读取更新状态，请稍后重新读取。') })
    let polling = false
    const timer = setInterval(() => { if (!active.current && !polling) { polling = true; void load(controller.signal).catch(() => undefined).finally(() => { polling = false }) } }, 3000)
    return () => { clearInterval(timer); controller.abort(); active.current?.abort() }
  }, [])
  async function perform(action: (signal: AbortSignal) => Promise<void>) {
    if (busy || uncertain) return
    const controller = new AbortController(); active.current = controller; setBusy(true); setNotice('')
    const timer = setTimeout(() => controller.abort(), 600000)
    try { await action(controller.signal); await load(controller.signal, true) }
    catch (error) {
      if (error instanceof ApiError && [400, 409, 429].includes(error.status)) setNotice(error.status === 409 ? '设置已变化或原命令已有结果，请重新读取后再操作。' : '请求未被接受，请检查输入或稍后重试。')
      else if (!(error instanceof ApiError && [401, 403].includes(error.status))) { setUncertain(true); setNotice('尚未确认这次操作的结果，已停止重复提交。请先重新读取状态核对。') }
    } finally { clearTimeout(timer); active.current = null; setBusy(false) }
  }
  async function command(signal: AbortSignal) {
    const identity = issued(await request('commands', signal, { schemaVersion: 2, family: 'updates' }), 'updates')
    if (!info) throw new ProtocolFault()
    return { schemaVersion: 2, ...identity, expectedRevision: info.revision }
  }
  function downloadGroup() {
    if (!exported) return
    const url = URL.createObjectURL(new Blob([`${exported}\n`], { type: 'text/plain;charset=utf-8' })), anchor = document.createElement('a')
    anchor.href = url; anchor.download = '未来教室-学校更新群组.txt'; anchor.click(); setTimeout(() => URL.revokeObjectURL(url), 1000)
  }
  async function importFile(signal: AbortSignal) {
    if (!file || file.size < 1 || file.size > 134217728) throw new ProtocolFault()
    const begin = await request('updates/import/begin', signal, { ...await command(signal), length: file.size }) as { schemaVersion?: number; uploadToken?: unknown }
    if (begin.schemaVersion !== 1 || typeof begin.uploadToken !== 'string' || !/^[0-9a-f]{32}$/.test(begin.uploadToken)) throw new ProtocolFault()
    const token = begin.uploadToken; uploadToken.current = token; setProgress(0)
    try {
      for (let offset = 0; offset < file.size; offset += 65536) {
        const end = Math.min(file.size, offset + 65536)
        const reply = await request(`updates/import/${token}/bytes/${offset}`, signal, undefined, file.slice(offset, end)) as { schemaVersion?: number; receivedBytes?: number }
        if (reply.schemaVersion !== 1 || reply.receivedBytes !== end) throw new ProtocolFault()
        setProgress(Math.floor(end / file.size * 100))
      }
      const reply = await request(`updates/import/${token}/finish`, signal, { schemaVersion: 1 }) as { status?: string }
      if (reply.status !== 'QUEUED') throw new ProtocolFault()
      uploadToken.current = null; setNotice('更新包已上传，正在校验官方签名和本机升级资格。请查看当前状态。'); setFile(null)
    } catch (error) {
      const cleanup = new AbortController(), timeout = setTimeout(() => cleanup.abort(), 15000)
      try { await request(`updates/import/${token}/cancel`, cleanup.signal, { schemaVersion: 1 }); uploadToken.current = null } catch { /* The original upload remains bounded and expires on the service. */ }
      finally { clearTimeout(timeout) }
      throw error
    }
  }
  if (session.principal.role !== 'GLOBAL_ADMIN') return <main className="software-updates"><p>只有全局主管理员可以管理软件更新。</p><a href="#/">返回教室管理</a></main>
  const disabled = busy || uncertain || !info
  return <main id="main-content" className="software-updates"><div className="software-updates-heading"><h1>软件更新</h1><a href="#/">返回教室管理</a></div>
    <p role="status" aria-live="polite">{notice || info?.updateMessage || '正在读取更新状态…'}</p>
    <Card><CardHeader><CardTitle>当前版本与更新</CardTitle></CardHeader><CardContent><p>版本 {info?.version ?? '正在读取…'}</p><p>更新范围：{info ? info.groupId ? `自定义学校群组 ${info.groupId}` : '当前局域网（默认，无需配置）' : '正在读取…'}</p>{info && info.totalBytes > 0 && <p>下载进度 {Math.min(100, Math.floor(info.receivedBytes / info.totalBytes * 100))}%</p>}
      <div className="update-actions"><Button disabled={disabled} onClick={() => { void perform(async signal => { await request('updates/check', signal, await command(signal)); setNotice('已发起检查，请查看更新状态。') }) }}>立即更新</Button><Button variant="outline" disabled={busy} onClick={() => { const controller = new AbortController(); void load(controller.signal, true).then(() => { setUncertain(false); setNotice('已重新读取当前状态，请核对后继续。') }).catch(() => setNotice('尚未恢复连接，安装更新时请稍候。')) }}>重新读取状态</Button></div>
    </CardContent></Card>
    <Card><CardHeader><CardTitle>自动更新</CardTitle></CardHeader><CardContent>{draft && <form onSubmit={event => { event.preventDefault(); void perform(async signal => { await request('updates/policy', signal, { ...await command(signal), ...draft }); setNotice('更新设置已保存。') }) }}><fieldset disabled={disabled} className="update-fields">
      <label><input type="checkbox" checked={draft.enabled} onChange={event => setDraft({ ...draft, enabled: event.target.checked, download: event.target.checked && draft.installMode !== 'notify', share: event.target.checked })} />启用内网更新（默认开启）</label>
      <label><input type="checkbox" disabled={!draft.enabled || draft.installMode === 'notify'} checked={draft.download} onChange={event => setDraft({ ...draft, download: event.target.checked })} />自动下载合格的新版</label>
      <label><input type="checkbox" disabled={!draft.enabled} checked={draft.share} onChange={event => setDraft({ ...draft, share: event.target.checked })} />向同一更新范围内的电脑分享已验证更新包</label>
      <label>安装方式<select value={draft.installMode} onChange={event => setDraft({ ...draft, installMode: event.target.value as Policy['installMode'], download: event.target.value === 'notify' ? false : draft.enabled })}><option value="automatic">下载后立即安装并重启（默认）</option><option value="notify">仅提醒有新版</option>{!['automatic', 'notify'].includes(draft.installMode) && <option value={draft.installMode} disabled>旧安装方式（兼容保留，当前不自动安装）</option>}</select></label>
      <p>安装完成后自动启用局域网更新，无需注册账号、创建或加入群组。自动安装时未来教室会短暂退出，完成后立即重新启动，保留课程与资料；已主动关闭的设置保持关闭。</p><Button type="submit">保存更新设置</Button>
    </fieldset></form>}</CardContent></Card>
    <details><summary>高级设置：使用独立学校更新群组（可选）</summary><Card><CardHeader><CardTitle>独立学校更新群组</CardTitle></CardHeader><CardContent><p>默认直接与局域网内其他电脑分发官方更新，不需要此项操作。仅在需要隔离分发范围时，创建或加入同一独立群组；已有群组继续保留。</p>
      <div className="update-actions"><Button disabled={disabled} onClick={() => { if (window.confirm('创建新的学校更新群组？其他教室需要导入这个群组文件。')) void perform(async signal => { await request('updates/group/create', signal, await command(signal)); setNotice('学校更新群组已创建，可导出群组文件给其他教室。') }) }}>创建学校群组</Button><Button variant="outline" disabled={disabled || !info?.groupId} onClick={() => { void perform(async signal => { const reply = await request('updates/group/export', signal) as { schemaVersion?: number; group?: unknown }; if (reply.schemaVersion !== 1 || typeof reply.group !== 'string' || reply.group.length > 160) throw new ProtocolFault(); setExported(reply.group); setNotice('群组文件已准备好，点击保存群组文件。') }) }}>准备导出群组</Button></div>
      {exported && <div className="update-actions"><Button onClick={downloadGroup}>保存群组文件</Button><Button variant="ghost" onClick={() => setExported('')}>清除本页群组密钥</Button></div>}
      <label>选择学校群组文件<Input type="file" accept=".txt,text/plain" disabled={disabled} onChange={event => { const selected = event.target.files?.[0]; if (selected && selected.size <= 4096) void selected.text().then(text => setGroup(text.trim())); else setNotice('请选择有效的学校群组文件。') }} /></label>
      <label>或粘贴学校群组内容<textarea rows={3} maxLength={160} value={group} disabled={disabled} onChange={event => setGroup(event.target.value.trim())} autoComplete="off" spellCheck={false} /></label>
      <Button disabled={disabled || !group.startsWith('fc-update-group-v1:')} onClick={() => { void perform(async signal => { await request('updates/group/join', signal, { ...await command(signal), group }); setGroup(''); setNotice('已加入学校更新群组。') }) }}>加入这个学校群组</Button><p>群组文件含有本校共享密钥，只交给本校管理员。</p>
    </CardContent></Card></details>
    <Card><CardHeader><CardTitle>导入官方更新包</CardTitle></CardHeader><CardContent><p>没有公网时，可在一台教室导入完整的 .fcupdate 更新包。验证通过后，同一更新范围内的其他电脑自动从内网获取，无需逐台导入。</p><Input type="file" accept=".fcupdate" disabled={disabled} onChange={event => { const selected = event.target.files?.[0] ?? null; if (selected && selected.size > 134217728) { setNotice('更新包不能超过 128 MiB。'); setFile(null) } else { setFile(selected); setProgress(0) } }} />
      {file && <p>{file.name} · {(file.size / 1048576).toFixed(1)} MiB</p>}{busy && uploadToken.current && <p>上传进度 {progress}%</p>}
      <div className="update-actions"><Button disabled={disabled || !file} onClick={() => { void perform(importFile) }}>上传并验证更新包</Button>{busy && uploadToken.current && <Button variant="outline" onClick={() => active.current?.abort()}>取消上传</Button>}</div>
    </CardContent></Card>
  </main>
}
