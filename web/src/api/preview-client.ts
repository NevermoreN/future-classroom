import { HttpApi } from './client'
import { adminEnvelope, type DisplayData, type LayoutData } from './admin-client'
import { decimal, ProtocolFault } from './protocol'
import type { ProfileKey, SubjectProfileData } from './profile-admin-client'
export interface PreviewSource { publicationRevision: string; identityRevision: string; displayRevision: string; publicLayoutRevision: string }
export interface PreviewNode { id: string; parentId: string | null; kind: string; x: number; y: number; width: number; height: number; clip: { x: number; y: number; width: number; height: number }; text: string | null; fontDip: number | null; textTruncated: boolean }
export interface PreviewGeometry { width: number; height: number; nodes: PreviewNode[]; sourcePublicationRevision: string; identityRevision: string; displayRevision: string; publicLayoutRevision: string; subjectProfileRevision: string | null }
function object(v: unknown): asserts v is Record<string, unknown> { if (!v || typeof v !== 'object' || Array.isArray(v)) throw new ProtocolFault() }
function revision(v: unknown) { if (v !== '0') decimal(v) }
function source(v: unknown): PreviewSource { object(v); for (const k of ['publicationRevision', 'identityRevision', 'displayRevision', 'publicLayoutRevision']) revision(v[k]); return v as unknown as PreviewSource }
function finite(v: unknown, positive = false): asserts v is number { if (typeof v !== 'number' || !Number.isFinite(v) || Math.abs(v) > 1000000 || positive && v < 0) throw new ProtocolFault() }
export function parsePreview(v: unknown): PreviewGeometry {
  object(v); finite(v.width, true); finite(v.height, true); if (!v.width || !v.height || !Array.isArray(v.nodes) || v.nodes.length > 128) throw new ProtocolFault()
  for (const k of ['sourcePublicationRevision', 'identityRevision', 'displayRevision', 'publicLayoutRevision']) revision(v[k]); if (v.subjectProfileRevision !== null) revision(v.subjectProfileRevision)
  for (const n of v.nodes) { object(n); decimal(n.id); if (n.parentId !== null) decimal(n.parentId); if (typeof n.kind !== 'string' || n.kind.length > 64 || typeof n.textTruncated !== 'boolean') throw new ProtocolFault(); for (const k of ['x', 'y', 'width', 'height']) finite(n[k], k === 'width' || k === 'height'); object(n.clip); for (const k of ['x', 'y', 'width', 'height']) finite(n.clip[k], k === 'width' || k === 'height'); if (n.text !== null && (typeof n.text !== 'string' || [...n.text].length > 4096)) throw new ProtocolFault(); if (n.fontDip !== null) finite(n.fontDip, true) }
  return v as unknown as PreviewGeometry
}
export class PreviewHttpApi extends HttpApi {
  async preview(input: { layout: LayoutData; expectedRevision: string; key?: ProfileKey | undefined; display?: DisplayData | undefined; materials?: SubjectProfileData['materials'] | undefined }, csrf: string, signal: AbortSignal): Promise<PreviewGeometry> {
    revision(input.expectedRevision)
    const read = await this.request('admin/preview/source', signal); if (read.status !== 200) throw new ProtocolFault()
    const revisions = adminEnvelope(read.value, source).data
    const suffix = input.key ? 'subject/' + [input.key.classId, input.key.subjectId, input.key.teacherId].map(x => decimal(x)).join('/') : 'public'
    const result = await this.request('admin/preview/' + suffix, signal, { schemaVersion: 1, expectedRevision: input.expectedRevision, action: 'preview', candidate: { layout: input.layout, display: input.display ?? null, materials: input.materials ?? null, expectedPublicationRevision: revisions.publicationRevision, expectedIdentityRevision: revisions.identityRevision, expectedDisplayRevision: revisions.displayRevision } }, csrf, 4 * 1024 * 1024)
    if (result.status !== 200) throw new ProtocolFault()
    return adminEnvelope(result.value, parsePreview).data
  }
}
