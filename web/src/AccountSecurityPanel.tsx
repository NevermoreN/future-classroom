import { useEffect, useRef, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Field, Section, Toggle } from './AdminForms'
import RecoveryCodeNotice from './RecoveryCodeNotice'
import { AccountSecurityHttpApi, clearTransferTicket, loadTransferTicket, localAccountControl } from './api/account-security-client'
import { ApiError, isAborted, trustedApiBase } from './api/client'
import type { AdminDocument, IdentityData } from './api/admin-client'
import type { Session } from './api/protocol'
export function TransferOutcomePanel({ session }: { session: Session }) {
  const [ticket, setTicket] = useState(loadTransferTicket), [notice, setNotice] = useState(''), [busy, setBusy] = useState(false)
  const [api] = useState(() => new AccountSecurityHttpApi(trustedApiBase(import.meta.env.BASE_URL, window.location.href))), operation = useRef<AbortController | null>(null)
  useEffect(() => () => operation.current?.abort(), [])
  if (!ticket) return null
  async function read() { if (busy || !ticket) return; const owner = new AbortController(); operation.current = owner; setBusy(true); try { const result = await api.outcome(ticket, session.csrf, owner.signal); if (!owner.signal.aborted) setNotice(result === 'COMMITTED' ? '已确认原主管理员交接提交成功；当前权限以本次登录账号为准。' : result === 'NOT_PREPARED' ? '原交接没有进入提交准备阶段。请核对当前账号状态后再决定下一步。' : '尚未找到原交接的提交结果，请保留查询入口并核对。') } catch (e) { if (!isAborted(e)) setNotice('无法查询。请在白板本机使用发起交接的原账号重新登录；此入口只查询本人这次交接，不提供管理目录。') } finally { if (!owner.signal.aborted) setBusy(false) } }
  return <Section title="本人上次交接结果"><p role="status">{notice || '本页保留了原交接查询凭据，确认结果前不会自动再次交接。'}</p><div className="admin-action-row"><Button type="button" disabled={busy || !localAccountControl()} onClick={() => { void read() }}>查询本人原交接结果</Button><Button type="button" variant="outline" disabled={busy} onClick={() => { if (window.confirm('结束本页原交接结果查询？此操作不会撤销交接。')) { clearTransferTicket(); setTicket(undefined) } }}>结束结果查询</Button></div>{!localAccountControl() && <p>请从教室电脑托盘打开本机 Web 设置后查询。</p>}</Section>
}
export default function AccountSecurityPanel({ session, identity, onRevoked, onDirty = () => undefined }: { session: Session; identity?: AdminDocument<IdentityData>; onRevoked: () => void; onDirty?: (dirty: boolean) => void }) {
  const [api] = useState(() => new AccountSecurityHttpApi(trustedApiBase(import.meta.env.BASE_URL, window.location.href))), [password, setPassword] = useState(''), [target, setTarget] = useState(''), [reviewed, setReviewed] = useState(false), [revocation, setRevocation] = useState(false), [busy, setBusy] = useState(false), [uncertain, setUncertain] = useState(false), [notice, setNotice] = useState(''), [code, setCode] = useState<string | null>(null)
  const operation = useRef<AbortController | null>(null), locked = useRef(false)
  useEffect(() => () => operation.current?.abort(), [])
  async function run(kind: 'rotate' | 'transfer') {
    if (locked.current || uncertain || !password || kind === 'transfer' && (!identity || !reviewed || !revocation)) return
    const owner = new AbortController(); operation.current = owner; locked.current = true; setBusy(true); setNotice(''); const secret = password; setPassword(''); onDirty(false)
    let sent = false
    try {
      if (kind === 'rotate') { sent = true; const next = await api.rotate(secret, session.csrf, owner.signal); if (!owner.signal.aborted) { setCode(next); onDirty(true) } }
      else { const ticket = await api.prepare(identity!.revision, target, session.csrf, owner.signal); sent = true; await api.transfer(ticket, secret, session.csrf, owner.signal); if (!owner.signal.aborted) onRevoked() }
    } catch (e) { if (!isAborted(e)) { if (sent && (!(e instanceof ApiError) || e.status >= 500)) { setUncertain(true); setNotice(kind === 'transfer' ? '交接结果尚未确认。请重新登录原账号，通过“本人上次交接结果”查询；不会自动重发。' : '恢复码轮换结果尚未确认。请重新登录；旧码可能已失效，可再次明确轮换生成新码。') } else setNotice(e instanceof ApiError && e.status === 429 ? '核验次数较多，请稍后再试。' : e instanceof ApiError && e.status === 409 ? '账号目录已有变化，请重新读取后核对。' : '操作未完成，请核对当前密码、账号状态和本机入口。') } }
    finally { locked.current = false; if (!owner.signal.aborted) setBusy(false) }
  }
  if (code) return <RecoveryCodeNotice code={code} onDone={() => { setCode(null); onDirty(false); onRevoked() }} />
  return <Section title="账号安全"><fieldset className="admin-fieldset" disabled={busy || uncertain}><form onSubmit={e => { e.preventDefault(); void run('rotate') }}><Field label="当前账号密码"><Input type="password" autoComplete="current-password" maxLength={128} value={password} onChange={e => { setPassword(e.target.value); onDirty(!!e.target.value) }} /></Field><p>重新生成仅属于自己的恢复码，成功后旧恢复码失效，其他登录会话会撤销。新码只展示一次。</p><Button type="submit" disabled={!password} onClick={e => { if (!window.confirm('生成新的本人恢复码并作废旧码？请准备离线保存。')) e.preventDefault() }}>核验密码并轮换本人恢复码</Button></form>{identity && <><h3>交接全局主管理员</h3><p>交接保留双方密码、恢复码与任教关系，撤销双方旧会话。你的全局管理权限将移交给所选账号。</p>{!localAccountControl() ? <p className="notice">请从教室电脑托盘的“查看状态 / 设置”打开本机 Web，执行主管理员交接。</p> : <><Field label="接任账号"><select value={target} onChange={e => { setTarget(e.target.value); setReviewed(false); setRevocation(false) }}><option value="">选择已完成首次改密的启用账号</option>{identity.data.accounts.filter(a => a.id !== session.principal.id && a.enabled && !a.mustChangePassword && a.role !== 'GLOBAL_ADMIN').map(a => <option value={a.id} key={a.id}>{a.displayName} · {a.loginName}</option>)}</select></Field><Toggle label="我已核对接任账号，确认将全局管理权限交给此人" value={reviewed} onChange={setReviewed} /><Toggle label="我理解本人将失去全局管理权限，双方旧登录将被撤销" value={revocation} onChange={setRevocation} /><Button type="button" disabled={!target || !password || !reviewed || !revocation || !!loadTransferTicket()} onClick={() => { void run('transfer') }}>核验当前密码并明确交接</Button>{loadTransferTicket() && <p>请先核对并结束上次交接结果查询，再发起新交接。</p>}</>}</>}</fieldset><p role="status">{notice}</p>{uncertain && <Button type="button" onClick={onRevoked}>重新登录并核对</Button>}</Section>
}
