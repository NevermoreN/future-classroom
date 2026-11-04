import { useCallback, useEffect, useRef, useState } from 'react'
import { AdminHttpApi, type AdminCommands, type AdminDocument, type AdminResources } from './api/admin-client'
import { ApiError, isAborted, trustedApiBase } from './api/client'
import { ProtocolFault, type Session } from './api/protocol'
export function useResource<R extends keyof AdminResources>(resource: R, session: Session, revoked: () => void) {
  const [api] = useState(() => new AdminHttpApi(trustedApiBase(import.meta.env.BASE_URL, window.location.href)))
  const [document, setDocument] = useState<AdminDocument<AdminResources[R]> | null>(null), [busy, setBusy] = useState(false), [notice, setNotice] = useState(''), [uncertain, setUncertain] = useState(false)
  const operation = useRef<AbortController | null>(null), live = useRef(true), sequence = useRef(0), writing = useRef(false), onRevoked = useRef(revoked); onRevoked.current = revoked
  function failure(error: unknown, write: boolean) {
    if (isAborted(error)) return
    if (error instanceof ApiError && error.status === 401) { onRevoked.current(); return }
    if (error instanceof ApiError && error.status === 403) { setNotice('当前账号无权执行此操作，登录仍然保留。'); return }
    if (error instanceof ApiError && error.status === 409) { setNotice('设置已被其他操作修改。请重新读取最新版本，核对后再保存。'); setUncertain(true); return }
    if (write && !(error instanceof ApiError)) { setUncertain(true); setNotice('保存结果尚未确认。请重新读取实际设置核对；不会自动重复提交。'); return }
    setNotice(error instanceof ProtocolFault ? error.message : error instanceof ApiError && error.status === 400 ? '设置未被接受，请检查必填项、数值范围、时间重叠和引用关系。' : '暂时无法读取或保存设置，请检查连接后重试。')
  }
  const load = useCallback(async () => {
    if (writing.current) return
    operation.current?.abort(); const owner = new AbortController(), id = ++sequence.current; operation.current = owner; setBusy(true); setNotice('')
    try { const next = await api.read(resource, owner.signal); if (live.current && sequence.current === id) { setDocument(next); setUncertain(false) } }
    catch (error) { if (live.current && sequence.current === id) failure(error, false) }
    finally { if (live.current && sequence.current === id) setBusy(false) }
  }, [api, resource])
  useEffect(() => { live.current = true; void load(); return () => { live.current = false; sequence.current++; operation.current?.abort() } }, [load])
  async function save(command: R extends keyof AdminCommands ? AdminCommands[R] : never) {
    if (!document || busy || uncertain || writing.current || resource === 'status') return false
    writing.current = true; const owner = new AbortController(), id = ++sequence.current; operation.current?.abort(); operation.current = owner; setBusy(true); setNotice('')
    try {
      const next = await api.write(resource as keyof AdminCommands, document.revision, command as AdminCommands[keyof AdminCommands], session.csrf, owner.signal)
      if (!live.current || sequence.current !== id) return false
      setDocument(next as AdminDocument<AdminResources[R]>); setNotice('已保存，设置以白板返回的最新版本为准。'); return true
    } catch (error) { if (live.current && sequence.current === id) failure(error, true); return false }
    finally { writing.current = false; if (live.current && sequence.current === id) setBusy(false) }
  }
  return { document, busy, notice, uncertain, load, save }
}
