import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react'
import { Button } from '@/components/ui/button'
import { NumberField, Section } from './AdminForms'
import { PreviewHttpApi, type PreviewGeometry, type PreviewNode } from './api/preview-client'
import { ApiError, isAborted, trustedApiBase } from './api/client'
import type { CanvasFrame, LayoutData, LayoutNode } from './api/admin-client'
import type { Session } from './api/protocol'
import { boundedFrame, singleLayerLayout, setCanvasFrame, snapModule } from './api/canvas-edit'

const names: Record<string, string> = { clock: '时钟', date: '日期日历', schedule: '课程表', homework: '今日作业', materials: '课堂资料', reminder: '共同提醒', date_countdown: '日期倒计时', recess: '课间倒计时', subject_text: '文字内容', subject_image: '图片内容' }
function name(node: LayoutNode) { return node.kind === 'root' ? '桌面整体' : node.kind === 'container' ? '背景分组' : node.kind === 'outlet' ? '课堂内容' : names[node.widget!] ?? '桌面项目' }
interface Gesture { pointer: number; startX: number; startY: number; frame: CanvasFrame; draft: LayoutData; mode: 'move' | 'resize'; scale: number; moved: boolean; latest: CanvasFrame }

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
  const prepared = geometry ? singleLayerLayout(value, geometry) : null, selectedFrame = localFrame ?? prepared?.canvasFrames?.find(frame => frame.nodeId === selected)
  function begin(event: ReactPointerEvent<HTMLDivElement | HTMLButtonElement>, node: LayoutNode, mode: 'move' | 'resize') {
    if (event.button !== 0 || !geometry || !prepared || !surface.current) return
    event.preventDefault(); event.stopPropagation(); onSelect(node.id)
    const frame = prepared.canvasFrames!.find(frame => frame.nodeId === node.id); if (!frame) return
    event.currentTarget.setPointerCapture(event.pointerId)
    gesture.current = { pointer: event.pointerId, startX: event.clientX, startY: event.clientY, frame, draft: prepared, mode, scale: surface.current.getBoundingClientRect().width / geometry.width, moved: false, latest: frame }
  }
  function move(event: ReactPointerEvent) {
    const current = gesture.current; if (!current || current.pointer !== event.pointerId) return
    event.stopPropagation()
    const dx = (event.clientX - current.startX) / current.scale, dy = (event.clientY - current.startY) / current.scale
    current.moved ||= Math.abs(dx) + Math.abs(dy) > 2
    if (current.moved && geometry) {
      current.latest = snapModule(boundedFrame(current.mode === 'move' ? { ...current.frame, x: current.frame.x + dx, y: current.frame.y + dy } : { ...current.frame, width: current.frame.width + dx, height: current.frame.height + dy }), current.draft.canvasFrames!.filter(frame => current.draft.nodes.find(node => node.id === current.draft.root)!.children.includes(frame.nodeId)), geometry, current.mode)
      setLocalFrame(current.latest)
    }
  }
  function finish(event: ReactPointerEvent, cancelled = false) {
    const current = gesture.current; if (!current || current.pointer !== event.pointerId) return
    event.stopPropagation(); gesture.current = null
    if (current.moved && !cancelled) onChange(setCanvasFrame(current.draft, current.latest))
    setLocalFrame(null)
  }
  function update(patch: Partial<CanvasFrame>) { if (prepared && selectedFrame) onChange(setCanvasFrame(prepared, { ...selectedFrame, ...patch })) }
  function rect(node: PreviewNode): PreviewNode {
    const frame = localFrame?.nodeId === node.id ? localFrame : prepared?.canvasFrames?.find(frame => frame.nodeId === node.id)
    return frame ? { ...node, x: frame.x, y: frame.y, width: frame.width, height: frame.height } : node
  }
  return <Section title="桌面画布" description="所有模块直接放在同一层。拖动移动、右下角缩放，靠近其他模块时自动吸附并保留 12 的间距；保存后应用到 Windows 桌面。">
    <div className="admin-action-row canvas-toolbar"><Button type="button" variant="outline" onClick={() => setRefresh(n => n + 1)} disabled={busy}>刷新画布</Button><label>显示比例 <select aria-label="画布显示比例" value={zoom} onChange={e => setZoom(Number(e.target.value))}>{[50, 75, 100, 125, 150, 200].map(n => <option key={n} value={n}>{n}%</option>)}</select></label><span role="status">{notice || (busy ? '正在更新画布…' : geometry ? `${geometry.width} × ${geometry.height} · 单层自由排布` : '正在载入桌面…')}</span></div>
    {notice && <p className="notice" role="alert">{notice}</p>}
    <div className="layout-canvas-workspace"><div className="layout-canvas-scroll">{geometry && <div ref={surface} className="layout-canvas-surface" aria-label="桌面布局画布" style={{ width: `${zoom}%`, aspectRatio: `${geometry.width}/${geometry.height}` }} onPointerDown={event => { if (event.target === event.currentTarget) onSelect(value.root) }}>
      {geometry.nodes.filter(n => prepared?.nodes.find(node => node.id === prepared.root)!.children.includes(n.id)).map(raw => {
        const item = value.nodes.find(node => node.id === raw.id); if (!item) return null
        const n = rect(raw), isSelected = selected === n.id, canEdit = item.widget !== 'recess'
        return <div key={n.id} role="button" tabIndex={0} aria-label={`选择${name(item)} ${item.id}`} aria-pressed={isSelected} className={`layout-canvas-item canvas-${item.kind}${isSelected ? ' selected' : ''}`} style={{ left: `${n.x / geometry.width * 100}%`, top: `${n.y / geometry.height * 100}%`, width: `${n.width / geometry.width * 100}%`, height: `${n.height / geometry.height * 100}%`, zIndex: isSelected ? 200 : undefined }} onClick={event => { event.stopPropagation(); onSelect(n.id) }} onPointerDown={event => canEdit ? begin(event, item, 'move') : onSelect(n.id)} onPointerMove={move} onPointerUp={event => finish(event)} onPointerCancel={event => finish(event, true)} onKeyDown={event => {
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
    </div>}</div><aside className="layout-canvas-inspector"><h3>{node ? name(node) : '选择项目'}</h3><label className="admin-field">当前项目<select aria-label="画布当前项目" value={selected} onChange={e => onSelect(e.target.value)}>{(prepared?.nodes ?? value.nodes).filter(item => item.id === value.root || prepared?.nodes.find(node => node.id === value.root)!.children.includes(item.id)).map(item => <option key={item.id} value={item.id}>{name(item)} · {item.id}</option>)}</select></label>{editable && selectedFrame ? <><div className="canvas-frame-fields">{(['x', 'y', 'width', 'height'] as const).map((key, i) => <NumberField key={key} label={['距离左边', '距离上边', '宽度', '高度'][i]!} min={key === 'width' || key === 'height' ? 8 : 0} max={key === 'width' || key === 'height' ? 8192 : 16376} value={selectedFrame[key]} onChange={value => update({ [key]: value })} />)}</div><Button type="button" variant="destructive" onClick={onRemove} disabled={node.kind === 'outlet'}>删除所选项目</Button>{node.kind === 'outlet' && <p>课堂内容随课程切换，作为整体保留；可移动和缩放。</p>}<p>方向键微调，按住 Shift 每次移动 10。模块靠近时自动吸附，模块之间保留 12 的空隙。</p></> : <p>{node?.widget === 'recess' ? '课间倒计时由程序放在空余位置，显示和大小可在桌面与配色中调整。' : '点击模块即可移动、缩放和删除；也可从上方列表选择被遮住的模块。'}</p>}{node?.widget === 'recess' && <Button type="button" variant="destructive" onClick={onRemove}>删除所选项目</Button>}</aside></div>
  </Section>
}
