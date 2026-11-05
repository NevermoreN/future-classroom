import { describe, expect, it } from 'vitest'
import { boundedFrame, canvasLayout, setCanvasFrame, singleLayerLayout, snapModule, MODULE_GAP } from '../src/api/canvas-edit'
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
  it('flattens legacy groups while keeping module screen positions and edited nested offsets', () => {
    const flat = singleLayerLayout(layout, geometry)
    expect(flat.nodes.find(node => node.id === '1')!.children).toEqual(['3', '4'])
    expect(flat.nodes.some(node => node.kind === 'container')).toBe(false)
    expect(flat.canvasFrames?.find(frame => frame.nodeId === '3')).toMatchObject({ x: 1532, y: 32, width: 260, height: 100 })
    const edited = setCanvasFrame(canvasLayout(layout, geometry), { nodeId: '3', x: 40, y: 50, width: 260, height: 100 })
    expect(singleLayerLayout(edited, geometry).canvasFrames?.find(frame => frame.nodeId === '3')).toMatchObject({ x: 1556, y: 66 })
    expect(singleLayerLayout(flat, geometry)).toEqual(flat)
    expect(layout.nodes[1]!.children).toEqual(['3'])
  })
  it('retains the course outlet and its content as one atomic desktop module', () => {
    const content: LayoutData = { ...layout, nodeIdHighWater: '6', nodes: [...layout.nodes.map(node => node.id === '2' ? { ...node, children: ['3', '5'] } : node), { id: '5', kind: 'outlet', children: ['6'], width: { kind: 'fixed', value: 300 }, height: { kind: 'fixed', value: 200 } }, { id: '6', kind: 'widget', widget: 'materials', children: [] }] }
    const flat = singleLayerLayout(content, geometry)
    expect(flat.nodes.find(node => node.id === '1')!.children).toEqual(['3', '5', '4'])
    expect(flat.nodes.find(node => node.id === '5')!.children).toEqual(['6'])
    expect(flat.canvasFrames?.map(frame => frame.nodeId)).toEqual(['3', '5', '4'])
  })
  it('snaps side-by-side and stacked modules with a small gap and aligned edges', () => {
    const other = { nodeId: 'a', x: 100, y: 100, width: 200, height: 100 }, viewport = { width: 1920, height: 1080 }
    expect(snapModule({ nodeId: 'b', x: 316, y: 103, width: 160, height: 80 }, [other], viewport, 'move')).toMatchObject({ x: 300 + MODULE_GAP, y: 100 })
    expect(snapModule({ nodeId: 'b', x: 104, y: 216, width: 160, height: 80 }, [other], viewport, 'move')).toMatchObject({ x: 100, y: 200 + MODULE_GAP })
    expect(snapModule({ nodeId: 'b', x: 10, y: 100, width: 74, height: 100 }, [other], viewport, 'resize').width).toBe(100 - MODULE_GAP - 10)
  })
  it('does not attract distant modules and snaps exactly to the desktop edge', () => {
    const frame = { nodeId: 'b', x: 316, y: 700, width: 160, height: 80 }, viewport = { width: 1920, height: 1080 }
    expect(snapModule(frame, [{ nodeId: 'a', x: 100, y: 100, width: 200, height: 100 }], viewport, 'move')).toEqual(frame)
    expect(snapModule({ ...frame, x: 4 }, [], viewport, 'move').x).toBe(0)
  })
})
