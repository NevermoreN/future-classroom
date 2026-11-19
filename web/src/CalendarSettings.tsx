import { useEffect, useRef, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Field, Section } from './AdminForms'
import { adminEnvelope, type AdminCommands } from './api/admin-client'
import { ApiError, HttpApi, isAborted, trustedApiBase } from './api/client'
import { ProtocolFault } from './api/protocol'

type CalendarDraft = Extract<AdminCommands['schedule'], { action: 'saveCalendar' }>['candidate']
type CalendarDay = { kind: 'rest' | 'makeup' | 'conflict'; names: string[] }
type CalendarCandidate = {
  revision: string
  fetchedAt: string
  sources: string[]
  adopted: boolean
  changes: { date: string; before: CalendarDay | null; after: CalendarDay | null; manualOverride: boolean }[] | null
}
type CalendarStatus = {
  year: number
  policyRevision: string
  automatic: boolean
  automaticAuthorized: boolean
  pending: boolean
  error: string | null
  nextAllowed: string | null
  candidate: CalendarCandidate | null
}
type Preview = { result: 'outOfTerm' | 'rest' | 'outsideCoverage' | 'unconfirmed' } | {
  result: 'scheduled'; sourceDate: string; weekday: number; unverified: boolean; rows: { begin: number; end: number; label: string }[]
}
type Action = { action: 'download' } | { action: 'setAutomatic'; expectedPolicy: string; enabled: boolean } | { action: 'adopt'; expectedRevision: string; candidateRevision: string; reviewed: true }
function object(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new ProtocolFault()
  return value as Record<string, unknown>
}
const revision = (value: unknown): value is string => typeof value === 'string' && /^(0|[1-9]\d{0,19})$/.test(value)
const date = (value: unknown): value is string => typeof value === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(value)
const notices = {
  cancelled: '上次下载已取消，已采用的日历仍可使用。',
  network: '上次下载未连接到来源。请检查网络，已采用的日历仍可使用。',
  tls: '上次下载的安全连接校验未通过。',
  source: '上次下载时，来源暂未提供该年度日历。',
  invalid: '来源数据校验未通过。',
  size: '来源文件超出容量。',
  timeout: '上次下载超时，请查看下次允许下载的时间。',
} as const
function parsedDay(input: unknown) {
  if (input === null) return
  const day = object(input)
  if (!['rest', 'makeup', 'conflict'].includes(String(day.kind)) || !Array.isArray(day.names) || day.names.some(name => typeof name !== 'string')) throw new ProtocolFault()
}
class CalendarHttpApi extends HttpApi {
  async publicCalendar(year: number, signal: AbortSignal, csrf?: string, change?: Action) {
    if (!Number.isInteger(year) || year < 1 || year > 9999) throw new ProtocolFault()
    const request = await this.request(`admin/calendar/${String(year).padStart(4, '0')}`, signal, change && { schemaVersion: 1, ...change }, csrf, 4 * 1024 * 1024)
    if (request.status !== 200) throw new ProtocolFault()
    return adminEnvelope(request.value, input => {
      const v = object(input)
      if (v.year !== year || !revision(v.policyRevision) || typeof v.automatic !== 'boolean' || typeof v.automaticAuthorized !== 'boolean' || typeof v.pending !== 'boolean' || v.error !== null && (typeof v.error !== 'string' || !Object.hasOwn(notices, v.error)) || v.nextAllowed !== null && typeof v.nextAllowed !== 'string') throw new ProtocolFault()
      if (v.candidate !== null) {
        const c = object(v.candidate)
        if (!revision(c.revision) || c.revision === '0' || typeof c.fetchedAt !== 'string' || typeof c.adopted !== 'boolean' || !Array.isArray(c.sources) || c.sources.length > 16 || c.sources.some(source => typeof source !== 'string' || !/^https:\/\/www\.gov\.cn\/zhengce\/(content|zhengceku)\/.+\.html?$/.test(source))) throw new ProtocolFault()
        if (c.changes !== null) {
          if (!Array.isArray(c.changes) || c.changes.length > 800) throw new ProtocolFault()
          for (const change of c.changes) {
            const row = object(change)
            if (!date(row.date) || typeof row.manualOverride !== 'boolean') throw new ProtocolFault()
            parsedDay(row.before); parsedDay(row.after)
          }
        }
      }
      return v as unknown as CalendarStatus
    })
  }
  async preview(classId: string, targetDate: string, candidate: CalendarDraft, expectedRevision: string, csrf: string, signal: AbortSignal) {
    const response = await this.request('admin/calendar/preview', signal, { schemaVersion: 1, classId, date: targetDate, candidate, expectedRevision }, csrf, 4 * 1024 * 1024)
    if (response.status !== 200) throw new ProtocolFault()
    return adminEnvelope(response.value, input => {
      const v = object(input)
      if (v.result === 'scheduled') {
        if (!date(v.sourceDate) || typeof v.weekday !== 'number' || !Number.isInteger(v.weekday) || v.weekday < 0 || v.weekday > 6 || typeof v.unverified !== 'boolean' || !Array.isArray(v.rows)) throw new ProtocolFault()
        for (const item of v.rows) {
          const row = object(item)
          if (typeof row.begin !== 'number' || typeof row.end !== 'number' || row.begin < 0 || row.end > 1440 || row.begin >= row.end || typeof row.label !== 'string') throw new ProtocolFault()
        }
      } else if (!['outOfTerm', 'rest', 'outsideCoverage', 'unconfirmed'].includes(String(v.result))) throw new ProtocolFault()
      return v as unknown as Preview
    }).data
  }
}
const useCalendarApi = () => useState(() => new CalendarHttpApi(trustedApiBase(import.meta.env.BASE_URL, window.location.href)))[0]
const dayText = (day: CalendarDay | null) => day ? `${day.kind === 'rest' ? '休息' : day.kind === 'makeup' ? '补班待定' : '来源冲突'} ${day.names.join('、')}` : '无公开安排'

export function PublicCalendarSettings({ csrf, dirty, updated }: { csrf: string; dirty: boolean; updated: () => void }) {
  const api = useCalendarApi()
  const [year, setYear] = useState(String(new Date().getFullYear())), [value, setValue] = useState<Awaited<ReturnType<CalendarHttpApi['publicCalendar']>> | null>(null)
  const [busy, setBusy] = useState(false), [notice, setNotice] = useState(''), [page, setPage] = useState(0)
  const running = useRef<AbortController | null>(null)
  const valid = /^\d{4}$/.test(year) && Number(year) > 0
  useEffect(() => () => { running.current?.abort() }, [])
  async function read(action?: Action) {
    if (!valid || running.current || action && dirty) return
    const controller = new AbortController(); running.current = controller
    setBusy(true); setNotice(action?.action === 'download' ? '正在读取公开节假日资料…' : '')
    try {
      const next = await api.publicCalendar(Number(year), controller.signal, action ? csrf : undefined, action)
      if (controller.signal.aborted) return
      setValue(next); setPage(0)
      if (action?.action === 'adopt') { updated(); setNotice('已采用本年度候选。手工调休安排仍优先。') }
      if (action?.action === 'download' && !next.data.candidate && !next.data.error) setNotice('暂未收到新资料，请查看当前下载状态。')
    } catch (error) {
      if (isAborted(error)) return
      setValue(null)
      setNotice(error instanceof ApiError && error.status === 403 ? '请在教室电脑通过托盘打开本机设置，下载和采用公共日历。' : action ? '操作结果待确认。请先读取当前状态，再决定是否重试。' : '暂时无法读取公开日历状态。')
    } finally {
      if (running.current === controller) running.current = null
      if (!controller.signal.aborted) setBusy(false)
    }
  }
  const result = value?.data, candidate = result?.candidate
  const pageSize = 12, changes = candidate?.changes, total = changes?.length ?? 0
  return <Section title="获取公共节假日" description="在教室电脑获取并核对来源。补班仍需指定使用哪一天的课表。">
    <div className="admin-action-row">
      <Field label="年度"><Input type="number" min={1} max={9999} value={year} disabled={busy} onChange={e => { setYear(e.target.value); setValue(null); setNotice('') }} /></Field>
      <Button type="button" variant="outline" disabled={!valid || busy} onClick={() => { void read() }}>读取状态</Button>
      <Button type="button" disabled={!valid || dirty || busy} onClick={() => { void read({ action: 'download' }) }}>下载候选</Button>
    </div>
    {dirty && <p>请先保存或放弃课表改动，再下载或采用公共日历。</p>}
    {notice && <p role="status">{notice}</p>}
    {result && <>
      {result.pending && <p>已有下载任务，稍后可读取状态。</p>}
      {result.error && <p>{notices[result.error as keyof typeof notices]}</p>}
      <p>{result.nextAllowed && <>下次允许下载 {result.nextAllowed}。 </>}自动获取并采用符合条件的放假变化{result.automaticAuthorized ? '已开启' : result.automatic ? '已暂停，需重新授权' : '已关闭'}。</p>
      <Button type="button" variant="outline" disabled={dirty || busy} onClick={() => {
        if (result.automaticAuthorized || window.confirm('允许自动下载、采用符合条件的纯放假变化？补班和手工安排仍由学校确认。')) void read({ action: 'setAutomatic', expectedPolicy: result.policyRevision, enabled: !result.automaticAuthorized })
      }}>{result.automaticAuthorized ? '关闭自动获取' : result.automatic ? '重新授权' : '开启自动获取'}</Button>
      {candidate && <>
        <p>候选获取时间 {candidate.fetchedAt}。{candidate.adopted && '这份候选已采用。'}</p>
        <ul>{candidate.sources.map(source => <li key={source}><a href={source} target="_blank" rel="noreferrer">核对通知来源</a></li>)}</ul>
        {changes === null ? <p>候选暂不能与已有日历合并，请先核对现有年度资料。</p> : total === 0 ? <p>与当前使用的公开日历相比，日期安排没有变化。</p> : <>
          <div className="admin-table-scroll"><table className="admin-table"><thead><tr><th>日期</th><th>原安排</th><th>新安排</th></tr></thead><tbody>
            {changes?.slice(page * pageSize, (page + 1) * pageSize).map(change => <tr key={change.date}><td>{change.date}</td><td>{dayText(change.before)}</td><td>{dayText(change.after)}{change.manualOverride && ' · 已有人工安排优先'}</td></tr>)}
          </tbody></table></div>
          <div className="admin-action-row"><Button type="button" variant="outline" disabled={page === 0} onClick={() => setPage(page - 1)}>上一页</Button><span>共 {total} 项</span><Button type="button" variant="outline" disabled={(page + 1) * pageSize >= total} onClick={() => setPage(page + 1)}>下一页</Button></div>
        </>}
        <Button type="button" disabled={!value || dirty || busy || candidate.adopted || changes === null} onClick={() => {
          if (value && window.confirm('已核对候选差异和通知来源，确认采用？公开补班日仍需指定课表来源。')) void read({ action: 'adopt', expectedRevision: value.revision, candidateRevision: candidate.revision, reviewed: true })
        }}>采用这份候选</Button>
      </>}
    </>}
  </Section>
}

const dayNames = ['周一', '周二', '周三', '周四', '周五', '周六', '周日']
const previewText = { outOfTerm: '该班级在这一天没有启用的学期。', rest: '这一天安排为休息。', outsideCoverage: '日期不在校历覆盖范围内，桌面将显示待确认。', unconfirmed: '这一天的课表仍待确认，请核对补课来源。' }
const time = (minute: number) => `${String(Math.floor(minute / 60)).padStart(2, '0')}:${String(minute % 60).padStart(2, '0')}`

export function CalendarPreview({ classId, targetDate, candidate, expectedRevision, csrf }: { classId: string; targetDate: string; candidate: CalendarDraft; expectedRevision: string; csrf: string }) {
  const api = useCalendarApi(), [preview, setPreview] = useState<Preview | null>(null), [busy, setBusy] = useState(false), [error, setError] = useState('')
  const { calendarMode, automaticGaps, schoolCalendar, publicCalendar } = candidate
  const running = useRef<AbortController | null>(null)
  useEffect(() => {
    setPreview(null); setBusy(false); setError('')
    return () => { running.current?.abort(); running.current = null }
  }, [classId, targetDate, calendarMode, automaticGaps, schoolCalendar, publicCalendar, expectedRevision])
  async function load() {
    const controller = new AbortController(); running.current = controller
    setPreview(null); setBusy(true); setError('')
    try { const result = await api.preview(classId, targetDate, candidate, expectedRevision, csrf, controller.signal); if (!controller.signal.aborted) setPreview(result) }
    catch (failure) { if (!isAborted(failure)) setError(failure instanceof ApiError && failure.status === 409 ? '课表已变化，请重新读取后预览。' : '暂时无法预览，请核对覆盖日期和课表来源。') }
    finally { if (!controller.signal.aborted) setBusy(false) }
  }
  return <>
    <Button type="button" variant="outline" disabled={!classId || !targetDate || busy} onClick={() => { void load() }}>预览这一天的课表</Button>
    {error && <p role="alert">{error}</p>}
    {preview && <div role="status">
      {preview.result === 'scheduled' ? <>
        <p>{targetDate} 使用 {preview.sourceDate}（{dayNames[preview.weekday]}）的课表。{preview.unverified && '公开节假日尚未核验。'}</p>
        {preview.rows.length ? <ul>{preview.rows.map((row, i) => <li key={i}>{time(row.begin)}–{time(row.end)} {row.label}</li>)}</ul> : <p>这一天没有启用的课节。</p>}
      </> : <p>{previewText[preview.result]}</p>}
    </div>}
  </>
}
