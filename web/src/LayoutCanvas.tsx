import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react'
import { Button } from '@/components/ui/button'
import { NumberField, Section } from './AdminForms'
import { PreviewHttpApi, type PreviewGeometry, type PreviewNode } from './api/preview-client'
import { ApiError, isAborted, trustedApiBase } from './api/client'
import type { CanvasFrame, LayoutData, LayoutNode } from './api/admin-client'
import type { Session } from './api/protocol'
import { boundedFrame, canvasLayout, setCanvasFrame } from './api/canvas-edit'

const names: Record<string, string> = { clock: '时钟', date: '日期日历', schedule: '课程表', homework: '今日作业', materials: '课堂资料', reminder: '共同提醒', date_countdown: '日期倒计时', recess: '课间倒计时', subject_text: '文字内容', subject_image: '图片内容' }
function name(node: LayoutNode) { return node.kind === 'root' ? '桌面整体' : node.kind === 'container' ? '背景分组' : node.kind === 'outlet' ? '随课程切换的区域' : names[node.widget!] ?? '桌面项目' }
interface Gesture { pointer: number; startX: number; startY: number; frame: CanvasFrame; draft: LayoutData; mode: 'move' | 'resize'; scale: number; moved: boolean }

export default function LayoutCanvas({ value, revision, session, onRevoked, selected, onSelect, onChange, onRemove, onGeometry }: { value: LayoutData; revision: string; session: Session; onRevoked: () => void; selected: string; onSelect: (id: string) => void; onChange: (next: LayoutData) => void; onRemove: () => void; onGeometry: (geometry: PreviewGeometry) => void }) {
  const [api] = useState(() => new PreviewHttpApi(trustedApiBase(import.meta.env.BASE_URL, window.location.href)))
  const [geometry, setGeometry] = useState<PreviewGeometry | null>(null), [notice, setNotice] = useState(''), [busy, setBusy] = useState(false), [zoom, setZoom] = useState(100), [localFrame, setLocalFrame] = useState<CanvasFrame | null>(null), [refresh, setRefresh] = useState(0)
  const surface = useRef<HTMLDivElement>(null), gesture = useRef<Gesture | null>(null)
  useEffect(() => {
    const controller = new AbortController(), timer = window.setTimeout(async () => {
      setBusy(true); setNotice('')
      try { const next = await api.preview({ layout: value, expectedRevision: revision }, session.csrf, controller.signal); if (!controller.signal.aborted) { setGeometry(next); onGeometry(next) } }
      catch (error) { if (!isAborted(error) && !controller.signal.aborted) { if (error instanceof ApiError && error.status === 401) onRevoked(); else setNotice(error instanceof ApiError && error.status === 409 ? '桌面设置已变化，请重新读取后继续。' : '无法载入桌面画布，请检查连接或设置后重试。') } }
      finally { if (!controller.signal.aborted) setBusy(false) }
    }, 250)
    return () => { window.clearTimeout(timer); controller.abort() }
  }, [value, revision, session.csrf, api, refresh])
  const node = value.nodes.find(node => node.id === selected), editable = node && node.kind !== 'root' && node.widget !== 'recess'
  const prepared = geometry ? canvasLayout(value, geometry) : null, selectedFrame = localFrame ?? prepared?.canvasFrames?.find(frame => frame.nodeId === selected)
  function begin(event: ReactPointerEvent<HTMLDivElement | HTMLButtonElement>, node: LayoutNode, mode: 'move' | 'resize') {
    if (event.button !== 0 || !geometry || !prepared || !surface.current) return
    event.preventDefault(); event.stopPropagation(); onSelect(node.id)
    const frame = prepared.canvasFrames!.find(frame => frame.nodeId === node.id); if (!frame) return
    event.currentTarget.setPointerCapture(event.pointerId)
    gesture.current = { pointer: event.pointerId, startX: event.clientX, startY: event.clientY, frame, draft: prepared, mode, scale: surface.current.getBoundingClientRect().width / geometry.width, moved: false }
  }
  function move(event: ReactPointerEvent) {
    const current = gesture.current; if (!current || current.pointer !== event.pointerId) return
    event.stopPropagation()
    const dx = (event.clientX - current.startX) / current.scale, dy = (event.clientY - current.startY) / current.scale
    current.moved ||= Math.abs(dx) + Math.abs(dy) > 2
    if (current.moved) setLocalFrame(boundedFrame(current.mode === 'move' ? { ...current.frame, x: current.frame.x + dx, y: current.frame.y + dy } : { ...current.frame, width: current.frame.width + dx, height: current.frame.height + dy }))
  }
  function finish(event: ReactPointerEvent, cancelled = false) {
    const current = gesture.current; if (!current || current.pointer !== event.pointerId) return
    event.stopPropagation(); gesture.current = null
    if (current.moved && localFrame && !cancelled) onChange(setCanvasFrame(current.draft, localFrame))
    setLocalFrame(null)
  }
  function update(patch: Partial<CanvasFrame>) { if (prepared && selectedFrame) onChange(setCanvasFrame(prepared, { ...selectedFrame, ...patch })) }
  function rect(node: PreviewNode): PreviewNode {
    if (!localFrame || !gesture.current || !geometry) return node
    const active = geometry.nodes.find(n => n.id === localFrame.nodeId); if (!active) return node
    let parent = value.nodes.find(n => n.children.includes(node.id)), descendant = node.id === active.id
    while (!descendant && parent) { descendant = parent.id === active.id; parent = value.nodes.find(n => n.children.includes(parent!.id)) }
    if (!descendant) return node
    const original = gesture.current.frame, x = node.x + localFrame.x - original.x, y = node.y + localFrame.y - original.y
    return { ...node, x, y, ...(node.id === active.id ? { width: localFrame.width, height: localFrame.height } : {}) }
  }
  function clipping(node: PreviewNode) {
    let clip = node.clip
    if (localFrame && geometry) {
      let left = 0, top = 0, right = geometry.width, bottom = geometry.height, parentId = node.parentId
      while (parentId && parentId !== value.root) {
        const parent = geometry.nodes.find(n => n.id === parentId); if (!parent) break
        const bounds = rect(parent); left = Math.max(left, bounds.x); top = Math.max(top, bounds.y); right = Math.min(right, bounds.x + bounds.width); bottom = Math.min(bottom, bounds.y + bounds.height); parentId = parent.parentId
      }
      clip = { x: left, y: top, width: Math.max(0, right - left), height: Math.max(0, bottom - top) }
    }
    return `inset(${Math.max(0, clip.y - node.y) / Math.max(1, node.height) * 100}% ${Math.max(0, node.x + node.width - clip.x - clip.width) / Math.max(1, node.width) * 100}% ${Math.max(0, node.y + node.height - clip.y - clip.height) / Math.max(1, node.height) * 100}% ${Math.max(0, clip.x - node.x) / Math.max(1, node.width) * 100}%)`
  }
  return <Section title="桌面画布" description="拖动项目调整位置，拖动右下角调整大小。修改后点击保存设置，才会应用到 Windows 桌面。">
    <div className="admin-action-row canvas-toolbar"><Button type="button" variant="outline" disabled={!geometry || busy || !!value.canvasFrames?.length} onClick={() => prepared && onChange(prepared)}>启用自由排布</Button><Button type="button" variant="outline" disabled={!value.canvasFrames?.length} onClick={() => { if (window.confirm('恢复按分组自动排列？组件会保留，手动设置的位置和尺寸将清除；保存后生效。')) onChange({ ...value, canvasFrames: [] }) }}>恢复自动排列</Button><Button type="button" variant="outline" onClick={() => setRefresh(n => n + 1)} disabled={busy}>刷新画布</Button><label>显示比例 <select aria-label="画布显示比例" value={zoom} onChange={e => setZoom(Number(e.target.value))}>{[50, 75, 100, 125, 150, 200].map(n => <option key={n} value={n}>{n}%</option>)}</select></label><span role="status">{notice || (busy ? '正在更新画布…' : geometry ? `${geometry.width} × ${geometry.height} · ${(value.canvasFrames?.length ?? 0) ? '自由排布' : '当前自动排布，可直接拖动'}` : '正在载入桌面…')}</span></div>
    {notice && <p className="notice" role="alert">{notice}</p>}
    <div className="layout-canvas-workspace"><div className="layout-canvas-scroll">{geometry && <div ref={surface} className="layout-canvas-surface" aria-label="桌面布局画布" style={{ width: `${zoom}%`, aspectRatio: `${geometry.width}/${geometry.height}` }} onPointerDown={event => { if (event.target === event.currentTarget) onSelect(value.root) }}>
      {geometry.nodes.filter(n => n.id !== value.root).map(raw => {
        const item = value.nodes.find(node => node.id === raw.id); if (!item) return null
        const n = rect(raw), isSelected = selected === n.id, canEdit = item.widget !== 'recess'
        return <div key={n.id} role="button" tabIndex={0} aria-label={`选择${name(item)} ${item.id}`} aria-pressed={isSelected} className={`layout-canvas-item canvas-${item.kind}${isSelected ? ' selected' : ''}`} style={{ left: `${n.x / geometry.width * 100}%`, top: `${n.y / geometry.height * 100}%`, width: `${n.width / geometry.width * 100}%`, height: `${n.height / geometry.height * 100}%`, clipPath: clipping(n), zIndex: isSelected ? 200 : undefined }} onClick={event => { event.stopPropagation(); onSelect(n.id) }} onPointerDown={event => canEdit ? begin(event, item, 'move') : onSelect(n.id)} onPointerMove={move} onPointerUp={event => finish(event)} onPointerCancel={event => finish(event, true)} onKeyDown={event => {
          if (event.key === 'Escape') { gesture.current = null; setLocalFrame(null); return }
          if (event.key === 'Delete' && isSelected) { event.preventDefault(); onRemove(); return }
          const directions: Record<string, [number, number]> = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] }, direction = directions[event.key]
          if (isSelected && selectedFrame && direction) { event.preventDefault(); const step = event.shiftKey ? 10 : 1; update({ x: selectedFrame.x + direction[0] * step, y: selectedFrame.y + direction[1] * step }) }
        }}>
          <span className="layout-canvas-caption">{name(item)}</span>{item.kind === 'widget' && <span className="layout-canvas-content">{n.text || name(item)}</span>}
          {isSelected && canEdit && <button type="button" className="layout-canvas-resize" aria-label={`调整${name(item)}大小`} onPointerDown={event => begin(event, item, 'resize')} onPointerMove={move} onPointerUp={event => finish(event)} onPointerCancel={event => finish(event, true)} />}
        </div>
      })}
      <span className="layout-canvas-desktop-label">Windows 桌面</span>
    </div>}</div><aside className="layout-canvas-inspector"><h3>{node ? name(node) : '选择项目'}</h3><label className="admin-field">当前项目<select aria-label="画布当前项目" value={selected} onChange={e => onSelect(e.target.value)}>{value.nodes.map(item => <option key={item.id} value={item.id}>{name(item)} · {item.id}</option>)}</select></label>{editable && selectedFrame ? <><div className="canvas-frame-fields">{(['x', 'y', 'width', 'height'] as const).map((key, i) => <NumberField key={key} label={['距离左边', '距离上边', '宽度', '高度'][i]!} min={key === 'width' || key === 'height' ? 8 : 0} max={key === 'width' || key === 'height' ? 8192 : 16376} value={selectedFrame[key]} onChange={value => update({ [key]: value })} />)}</div><Button type="button" variant="destructive" onClick={onRemove} disabled={node.kind === 'outlet'}>删除所选项目</Button>{node.kind === 'outlet' && <p>课程切换区域需要保留；可以移动、缩放，或删除其中的组件。</p>}<p>方向键微调，按住 Shift 每次移动 10。分组移动会带动其中的组件；分组内的位置从该分组左上角算起。</p></> : <p>{node?.widget === 'recess' ? '课间倒计时由程序放在空余位置，显示和大小可在桌面与配色中调整。' : '点击组件或背景分组即可移动、缩放和删除；也可从上方列表选择被遮住的项目。'}</p>}{node?.widget === 'recess' && <Button type="button" variant="destructive" onClick={onRemove}>删除所选项目</Button>}</aside></div>
  </Section>
}
