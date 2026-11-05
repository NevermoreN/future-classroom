import type { CanvasFrame, LayoutData } from './admin-client'
import type { PreviewGeometry } from './preview-client'

/** Keep the existing measured positions when switching from automatic layout. */
export function canvasLayout(layout: LayoutData, geometry: PreviewGeometry): LayoutData {
  const frames = layout.canvasFrames ?? []
  const nodes = new Map(geometry.nodes.map(node => [node.id, node]))
  return { ...layout, canvasFrames: layout.nodes.filter(node => node.id !== layout.root && node.widget !== 'recess').map((node, index) => {
    const existing = frames.find(frame => frame.nodeId === node.id)
    if (existing) return existing
    const measured = nodes.get(node.id), parent = layout.nodes.find(parent => parent.children.includes(node.id)), parentRect = parent?.id === layout.root ? null : nodes.get(parent?.id ?? '')
    return boundedFrame({ nodeId: node.id, x: measured ? measured.x - (parentRect?.x ?? 0) : 24 + index % 4 * 24, y: measured ? measured.y - (parentRect?.y ?? 0) : 24 + index % 4 * 24, width: measured?.width || 280, height: measured?.height || 160 })
  }) }
}

export function boundedFrame(frame: CanvasFrame): CanvasFrame {
  const x = Math.max(0, Math.min(16376, Math.round(frame.x))), y = Math.max(0, Math.min(16376, Math.round(frame.y)))
  return { nodeId: frame.nodeId, x, y, width: Math.max(8, Math.min(8192, 16384 - x, Math.round(frame.width))), height: Math.max(8, Math.min(8192, 16384 - y, Math.round(frame.height))) }
}

export function setCanvasFrame(layout: LayoutData, frame: CanvasFrame): LayoutData {
  return { ...layout, canvasFrames: [...(layout.canvasFrames ?? []).filter(value => value.nodeId !== frame.nodeId), boundedFrame(frame)] }
}

/** Public desktop modules share screen coordinates; course content remains an atomic outlet. */
export function singleLayerLayout(layout: LayoutData, geometry: PreviewGeometry): LayoutData {
  const measured = new Map(geometry.nodes.map(node => [node.id, node]))
  const frames = new Map((layout.canvasFrames ?? []).map(frame => [frame.nodeId, frame]))
  const root = layout.nodes.find(node => node.id === layout.root)!
  function position(id: string): { x: number; y: number } {
    const frame = frames.get(id)
    if (!frame) return measured.get(id) ?? { x: 24, y: 24 }
    const parent = layout.nodes.find(node => node.children.includes(id))
    const origin = parent && parent.id !== layout.root ? position(parent.id) : { x: 0, y: 0 }
    return { x: origin.x + frame.x, y: origin.y + frame.y }
  }
  const modules: string[] = [], retained = new Set([layout.root])
  function keep(id: string) { retained.add(id); layout.nodes.find(node => node.id === id)?.children.forEach(keep) }
  function collect(id: string) {
    const node = layout.nodes.find(node => node.id === id)!
    if (node.kind === 'container') node.children.forEach(collect)
    else { modules.push(id); keep(id) }
  }
  root.children.forEach(collect)
  const moduleFrames = modules.flatMap(id => {
    const node = layout.nodes.find(node => node.id === id)!
    if (node.widget === 'recess') return []
    const rect = frames.get(id) ?? measured.get(id)
    return [boundedFrame({ nodeId: id, ...position(id), width: rect?.width || 280, height: rect?.height || 160 })]
  })
  return { ...layout, nodes: layout.nodes.filter(node => retained.has(node.id)).map(node => node.id === layout.root ? { ...node, children: modules } : node), contentStyles: layout.contentStyles.filter(style => retained.has(style.nodeId)), canvasFrames: [...moduleFrames, ...(layout.canvasFrames ?? []).filter(frame => retained.has(frame.nodeId) && !modules.includes(frame.nodeId))] }
}

export const MODULE_GAP = 12
/** Coordinates are desktop DIP, independent of canvas zoom. */
export function snapModule(frame: CanvasFrame, others: CanvasFrame[], viewport: { width: number; height: number }, mode: 'move' | 'resize'): CanvasFrame {
  const threshold = 8
  const xs = mode === 'move' ? [0, viewport.width - frame.width] : [viewport.width]
  const ys = mode === 'move' ? [0, viewport.height - frame.height] : [viewport.height]
  for (const other of others) {
    if (other.nodeId === frame.nodeId) continue
    const overlapsY = frame.y < other.y + other.height + MODULE_GAP + threshold && frame.y + frame.height > other.y - MODULE_GAP - threshold
    const overlapsX = frame.x < other.x + other.width + MODULE_GAP + threshold && frame.x + frame.width > other.x - MODULE_GAP - threshold
    if (overlapsY) xs.push(...(mode === 'move' ? [other.x - frame.width - MODULE_GAP, other.x + other.width + MODULE_GAP, other.x, other.x + other.width - frame.width] : [other.x - MODULE_GAP, other.x + other.width]))
    if (overlapsX) ys.push(...(mode === 'move' ? [other.y - frame.height - MODULE_GAP, other.y + other.height + MODULE_GAP, other.y, other.y + other.height - frame.height] : [other.y - MODULE_GAP, other.y + other.height]))
  }
  function nearest(value: number, targets: number[], min: number, max: number) {
    let result = value, distance = threshold + 1
    for (const target of targets) {
      const delta = Math.abs(target - value)
      if (target >= min && target <= max && delta < distance) { result = target; distance = delta }
    }
    return result
  }
  const x = mode === 'move' ? frame.x : frame.x + frame.width, y = mode === 'move' ? frame.y : frame.y + frame.height
  const snappedX = nearest(x, xs, mode === 'move' ? 0 : frame.x + 8, mode === 'move' ? viewport.width - frame.width : viewport.width)
  const snappedY = nearest(y, ys, mode === 'move' ? 0 : frame.y + 8, mode === 'move' ? viewport.height - frame.height : viewport.height)
  return boundedFrame(mode === 'move' ? { ...frame, x: snappedX, y: snappedY } : { ...frame, width: snappedX - frame.x, height: snappedY - frame.y })
}
