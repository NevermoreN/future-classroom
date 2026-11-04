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
