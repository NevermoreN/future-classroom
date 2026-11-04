import { describe, expect, it } from 'vitest'
import { boundedFrame, canvasLayout, setCanvasFrame } from '../src/api/canvas-edit'
import { appendLayoutNode } from '../src/api/layout-edit'
import type { LayoutData } from '../src/api/admin-client'
import type { PreviewGeometry } from '../src/api/preview-client'

const layout: LayoutData = { root: '1', nodeIdHighWater: '4', contentStyles: [], nodes: [
  { id: '1', kind: 'root', children: ['2', '4'], flow: 'column', padding: 16, gap: 12, anchor: 'top_right', width: { kind: 'fixed', value: 400 }, height: { kind: 'fixed', value: 900 } },
  { id: '2', kind: 'container', children: ['3'], flow: 'column', padding: 16, gap: 12, width: { kind: 'fixed', value: 300 }, height: { kind: 'fixed', value: 200 }, material: null, groupOpacityPercent: 100 },
  { id: '3', kind: 'widget', widget: 'clock', children: [] }, { id: '4', kind: 'widget', widget: 'date', children: [] },
] }
const geometry: PreviewGeometry = { width: 1920, height: 1080, sourcePublicationRevision: '1', identityRevision: '1', displayRevision: '1', publicLayoutRevision: '1', subjectProfileRevision: null, nodes: [
  { id: '1', parentId: null, kind: 'root', x: 1500, y: 0, width: 400, height: 900, clip: { x: 0, y: 0, width: 1920, height: 1080 }, text: null, fontDip: null, textTruncated: false },
  { id: '2', parentId: '1', kind: 'container', x: 1516, y: 16, width: 300, height: 200, clip: { x: 1516, y: 16, width: 300, height: 200 }, text: null, fontDip: null, textTruncated: false },
  { id: '3', parentId: '2', kind: 'clock', x: 1532, y: 32, width: 260, height: 100, clip: { x: 1532, y: 32, width: 260, height: 100 }, text: '09:30', fontDip: 24, textTruncated: false },
] }
describe('desktop canvas editing', () => {
  it('preserves screen positions and nested offsets when converting automatic layout, including hidden siblings', () => {
    const next = canvasLayout(layout, geometry)
    expect(next.canvasFrames?.find(f => f.nodeId === '2')).toMatchObject({ x: 1516, y: 16 })
    expect(next.canvasFrames?.find(f => f.nodeId === '3')).toMatchObject({ x: 16, y: 16 })
    expect(next.canvasFrames?.map(f => f.nodeId)).toEqual(['2', '3', '4'])
    expect(layout.canvasFrames).toBeUndefined()
  })
  it('keeps complete frame siblings after adding a component and retains edits on refresh', () => {
    const converted = canvasLayout(layout, geometry), added = appendLayoutNode(converted, '1', 'schedule', 'public')
    expect(added.value.canvasFrames?.find(f => f.nodeId === added.nodeId)).toBeDefined()
    const frame = { nodeId: '3', x: 24, y: 30, width: 180, height: 80 }, moved = setCanvasFrame(added.value, frame)
    expect(canvasLayout(moved, geometry).canvasFrames?.find(f => f.nodeId === '3')).toEqual(frame)
  })
  it('keeps dragged and resized coordinates integral and within the shared protocol bounds', () => {
    expect(boundedFrame({ nodeId: '3', x: -3.4, y: 16382.6, width: 0, height: 100 })).toEqual({ nodeId: '3', x: 0, y: 16376, width: 8, height: 8 })
  })
})
