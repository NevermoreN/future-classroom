import { useEffect, useState } from 'react'
import { ArrowRight, Download, RefreshCw, Search, Star } from 'lucide-react'
import brand from './assets/icons/brand.png'
import { Button } from '@/components/ui/button'
import { readJson, trustedApiBase } from './api/client'
import { classroomDirectory, mergeClassrooms, readSavedClassrooms, saveClassrooms, type ClassroomDirectory, type SavedClassroom } from './classroom-directory'
import './ClassroomHome.css'

export default function ClassroomHome({ onInstallHelp, installed = false }: { onInstallHelp: () => void; installed?: boolean }) {
  const [saved, setSaved] = useState<SavedClassroom[]>(() => {
    try { return readSavedClassrooms(window.localStorage) } catch { return [] }
  })
  const [directory, setDirectory] = useState<ClassroomDirectory | null>(null)
  const [loading, setLoading] = useState(true), [notice, setNotice] = useState(''), [refresh, setRefresh] = useState(0)
  const [search, setSearch] = useState(''), [filter, setFilter] = useState('all'), [confirm, setConfirm] = useState<string | null>(null)
  function persist(next: SavedClassroom[]) {
    setSaved(next)
    try { if (!saveClassrooms(window.localStorage, next)) setNotice('浏览器未允许保存收藏，当前页面仍可使用。') }
    catch { setNotice('浏览器未允许保存收藏，当前页面仍可使用。') }
  }
  useEffect(() => {
    const controller = new AbortController()
    const timeout = window.setTimeout(() => controller.abort(), 12_000)
    setLoading(true); setNotice('')
    async function load() {
      try {
        const base = trustedApiBase(import.meta.env.BASE_URL, window.location.href)
        const response = await fetch(new URL('classrooms', base), { signal: controller.signal, credentials: 'omit', cache: 'no-store', redirect: 'error' })
        if (!response.ok) throw new Error()
        const next = classroomDirectory(await readJson(response, controller.signal))
        if (controller.signal.aborted) return
        setDirectory(next)
        setSaved(previous => {
          const merged = mergeClassrooms(next.rooms, previous)
          try { saveClassrooms(window.localStorage, merged) } catch { /* The directory remains usable without persistence. */ }
          return merged
        })
      } catch {
        if (active) { setDirectory(null); setNotice('暂时无法连接教室。下方保留本机收藏和最近名单，连接状态未知；请连接学校网络后刷新。') }
      } finally { if (active) setLoading(false); window.clearTimeout(timeout) }
    }
    let active = true
    void load()
    return () => { active = false; controller.abort(); window.clearTimeout(timeout) }
  }, [refresh])
  useEffect(() => {
    const update = () => setRefresh(value => value + 1)
    const visible = () => { if (document.visibilityState === 'visible' && navigator.onLine !== false) update() }
    const timer = window.setInterval(visible, 15_000)
    window.addEventListener('online', update)
    window.addEventListener('offline', update)
    document.addEventListener('visibilitychange', visible)
    return () => { window.clearInterval(timer); window.removeEventListener('online', update); window.removeEventListener('offline', update); document.removeEventListener('visibilitychange', visible) }
  }, [])
  const rooms = saved.filter(room => `${room.schoolName} ${room.className ?? ''} ${room.address}`.toLowerCase().includes(search.toLowerCase())
    && (filter === 'all' || (filter === 'favorite' ? room.favorite : room.recent)))
  function remember(room: SavedClassroom) {
    persist([{ ...room, recent: true }, ...saved.filter(item => item.address !== room.address)])
  }
  return <main id="main-content" className="classroom-home">
    <header className="classroom-home-header"><span className="classroom-home-mark"><img src={brand} alt="" /></span><div><p>未来教室</p><h1>选择班级</h1><p>选择班级后打开该教室网站。</p></div></header>
    {!installed && <section className="classroom-install"><div><h2><Download size={20} aria-hidden="true" /> 添加到手机，班级随手可达</h2><p>把这个入口添加到主屏幕，保留收藏和最近使用的班级。离线时可以查看已保存名单，进入教室需要连接学校网络。</p></div><Button type="button" onClick={onInstallHelp}>安装到手机 / 查看步骤</Button></section>}
    <section className="classroom-directory" aria-label="班级列表">
      <div className="classroom-toolbar"><label className="classroom-search"><Search size={18} aria-hidden="true" /><span className="sr-only">搜索学校、班级或地址</span><input value={search} onChange={event => setSearch(event.target.value)} placeholder="搜索学校或班级" /></label><Button type="button" variant="outline" disabled={loading} onClick={() => setRefresh(value => value + 1)}><RefreshCw size={16} aria-hidden="true" />{loading ? '正在查找…' : '刷新班级'}</Button></div>
      <div className="classroom-filters" aria-label="筛选班级">{[['all', '全部班级'], ['favorite', '我的收藏'], ['recent', '最近使用']].map(([value, text]) => <button key={value} type="button" aria-pressed={filter === value} onClick={() => setFilter(value!)}>{text}</button>)}</div>
      <p className="classroom-status" role="status">{notice || (loading ? '正在读取学校网络中的班级…' : directory?.discoveryAvailable ? '显示本教室及当前发现的班级。其他班级的名称和地址请在首次进入时核对。' : '当前仅能确认本教室，尚未发现其他班级。收藏中未被再次发现的班级，连接状态未知。')}</p>
      <div className="classroom-grid">{rooms.map(room => {
        const found = directory?.rooms.find(item => item.address === room.address), current = found?.isCurrent === true
        const checked = current || room.recent || confirm === room.address
        return <article className="classroom-card" key={room.address}>
          <div className="classroom-card-top"><span>{current ? '本教室' : found ? '局域网发现 · 待连接' : '已保存 · 状态未知'}</span><Button type="button" variant="ghost" size="icon" aria-label={`${room.favorite ? '取消收藏' : '收藏'}${room.className ?? '未命名班级'}`} aria-pressed={room.favorite} onClick={() => persist(saved.map(item => item.address === room.address ? { ...item, favorite: !item.favorite } : item))}><Star size={19} fill={room.favorite ? 'currentColor' : 'none'} aria-hidden="true" /></Button></div>
          <p className="classroom-school">{room.schoolName}</p><h2>{room.className ?? '尚未设置班级名称'}</h2><p className="classroom-address">{room.address}</p>
          {checked ? <>{!current && confirm === room.address && !room.recent && <p className="classroom-confirm">请核对上方班级和地址。发现信息不代表身份已验证；按浏览器提示信任学校证书，并使用该教室账号登录。</p>}<Button asChild><a href={current ? '#/classroom' : `${room.address}#/classroom`} {...(!current ? { target: '_blank', rel: 'noopener noreferrer' } : {})} onClick={() => remember(room)}>进入班级<ArrowRight aria-hidden="true" /></a></Button></> : <Button type="button" variant="outline" onClick={() => setConfirm(room.address)}>核对班级并进入<ArrowRight aria-hidden="true" /></Button>}
        </article>
      })}</div>
      {!loading && rooms.length === 0 && <p className="classroom-empty">{saved.length ? '没有符合条件的班级。请更换搜索词或筛选。' : '尚无可用班级名单。请连接学校网络，打开教室二维码中的地址后刷新。'}</p>}
    </section>
    <footer className="classroom-home-footer">收藏仅保存在此浏览器中。各教室分别登录，账号与教学资料不会保存到离线入口。微信内请使用右上角菜单，在系统浏览器中打开后添加到主屏幕。</footer>
  </main>
}
