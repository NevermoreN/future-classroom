import type { ComponentKind, LayoutData, LayoutNode } from './admin-client'
export type LayoutAddition = 'container' | 'outlet' | ComponentKind
const contentKinds = ['materials', 'subject_text', 'subject_image']
function descendants(layout: LayoutData, parent: string, found = new Set<string>()): Set<string> { if (found.has(parent)) return found; found.add(parent); layout.nodes.find(node => node.id === parent)?.children.forEach(id => descendants(layout, id, found)); return found }
function glassAncestors(layout: LayoutData, parent: LayoutNode): number { let count = 0, current: LayoutNode | undefined = parent; while (current) { if (current.kind === 'container' && current.material) count++; current = layout.nodes.find(node => node.children.includes(current!.id)) } return count }
function insideContent(layout: LayoutData, id: string, scope: 'public' | 'subject'): boolean { return scope === 'subject' || layout.nodes.some(node => node.kind === 'outlet' && descendants(layout, node.id).has(id)) }
export function allowedLayoutAdditions(layout: LayoutData, parentId: string, scope: 'public' | 'subject'): LayoutAddition[] {
  const parent = layout.nodes.find(node => node.id === parentId)
  if (!parent || parent.kind === 'widget' || layout.nodes.length >= 128) return []
  const content = insideContent(layout, parentId, scope); if (content && glassAncestors(layout, parent) >= 2) return []; const kinds: LayoutAddition[] = ['container']
  if (scope === 'public' && !content && !layout.nodes.some(node => node.kind === 'outlet')) kinds.push('outlet')
  const widgets: ComponentKind[] = content ? ['materials', 'subject_text', 'subject_image'] : ['clock', 'date', 'schedule', 'homework', 'reminder', 'date_countdown', ...(parent.kind === 'root' ? ['recess' as const] : [])]
  return [...kinds, ...widgets.filter(kind => content || !layout.nodes.some(node => node.kind === 'widget' && node.widget === kind))]
}
function container(id: string, children: string[]): LayoutNode { return { id, kind: 'container', children, flow: 'column', padding: 16, gap: 12, width: { kind: 'content', min: 160, preferred: 320, max: 800 }, height: { kind: 'content', min: 64, preferred: 240, max: 1200 }, groupOpacityPercent: 100, material: { surfacePercent: 3, edgePercent: 50 } } }
/** Adding a content card keeps the existing public eight-surface budget: reuse
 * its content glass group or wrap existing outlet children once. */
export function appendLayoutNode(layout: LayoutData, parentId: string, kind: LayoutAddition, scope: 'public' | 'subject'): { value: LayoutData; nodeId: string } {
  if (!allowedLayoutAdditions(layout, parentId, scope).includes(kind)) throw new Error('当前父节点不允许添加此组件，请选择正确的根节点、科目插槽或内容容器。')
  const parent = layout.nodes.find(node => node.id === parentId)!, high = BigInt(layout.nodeIdHighWater)
  if (high >= 18446744073709551614n) throw new Error('布局节点编号已达到上限，不能继续添加。')
  const id = (high + 1n).toString(), widget = kind !== 'container' && kind !== 'outlet'
  const newNode: LayoutNode = kind === 'container' ? container(id, []) : kind === 'outlet' ? { id, kind: 'outlet', children: [], width: { kind: 'content', min: 160, preferred: 320, max: 800 }, height: { kind: 'content', min: 64, preferred: 240, max: 1200 } } : { id, kind: 'widget', widget: kind, children: [] }
  let nodes = [...layout.nodes, newNode], nextHigh = id
  if (widget && contentKinds.includes(kind) && (parent.kind === 'outlet' || scope === 'subject' && parent.kind === 'root' || parent.kind === 'container' && !parent.material && glassAncestors(layout, parent) === 0)) {
    const existing = layout.nodes.find(node => node.kind === 'container' && node.material && descendants(layout, parentId).has(node.id))
    if (existing) nodes = nodes.map(node => node.id === existing.id ? { ...node, children: [...node.children, id] } : node)
    else {
      if (nodes.length >= 128) throw new Error('添加内容时需要一个共享玻璃容器，布局节点数量已达上限。')
      nextHigh = (high + 2n).toString(); nodes = nodes.map(node => node.id === parentId ? { ...node, children: [nextHigh] } : node); nodes.push(container(nextHigh, [...parent.children, id]))
    }
  } else {
    nodes = nodes.map(node => node.id === parentId ? { ...node, children: [...node.children, id] } : node)
  }
  const styles = kind === 'subject_text' || kind === 'subject_image' ? [...layout.contentStyles, { nodeId: id, imageFit: 'contain' as const, component: { kind, visible: true, opacityPercent: 100, fontDip: 22, minWidth: 160, maxWidth: 800, minHeight: 64, maxHeight: 1200 } }] : layout.contentStyles
  return { value: { ...layout, nodeIdHighWater: nextHigh, nodes, contentStyles: styles }, nodeId: id }
}
