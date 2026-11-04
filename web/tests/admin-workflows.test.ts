import { describe, expect, it, vi } from 'vitest'
import { MaintenanceHttpApi, type MaintenanceSnapshot } from '../src/api/maintenance-client'
import { RepairHttpApi, beginRepairSession } from '../src/api/repair-client'
import { PreviewHttpApi, parsePreview } from '../src/api/preview-client'
import { allowedLayoutAdditions, appendLayoutNode } from '../src/api/layout-edit'
import type { LayoutData } from '../src/api/admin-client'
import { ProtocolFault } from '../src/api/protocol'
const signal = () => new AbortController().signal
const json = (data: unknown) => new Response(JSON.stringify({ schemaVersion: 1, revision: '4', data }), { headers: { 'Content-Type': 'application/json' } })
const base = new URL('https://127.0.0.1/fc/api/v1/')
const layout: LayoutData = { root: '1', nodeIdHighWater: '3', contentStyles: [], nodes: [{ id: '1', kind: 'root', children: ['2'], flow: 'column', padding: 8, gap: 8, anchor: 'top_left', width: { kind: 'fixed', value: 1920 }, height: { kind: 'fixed', value: 1080 } }, { id: '2', kind: 'outlet', children: ['3'], width: { kind: 'fixed', value: 300 }, height: { kind: 'fixed', value: 600 } }, { id: '3', kind: 'widget', widget: 'materials', children: [] }] }
const job: MaintenanceSnapshot = { id: '2', kind: 'RESET', phase: 'REVIEW_READY', canCancel: true, cancellationRequested: false, expectedRevision: '4', fileName: null, bytes: null, sha256: null, resetReview: { presentTreeCount: 2, scope: ['账号', '资料'] }, review: null, mapping: null, error: null, restartRequired: false }
describe('administrator workflows', () => {
  it('keeps maintenance status and cancel available through independent proof, with positive JSON action body', async () => {
    const fetcher = vi.fn<typeof fetch>().mockResolvedValueOnce(json({ job })).mockResolvedValueOnce(json({ job: { ...job, cancellationRequested: true } }))
    const api = new MaintenanceHttpApi(base, fetcher), credential = { id: '2', proof: 'b'.repeat(64) }
    expect((await api.readStatus(credential, signal())).data.job.phase).toBe('REVIEW_READY')
    await api.action(credential, 'cancel', undefined, undefined, signal())
    const request = fetcher.mock.calls[1]![1]!
    expect(JSON.parse(request.body as string)).toEqual({ schemaVersion: 1 }); expect(new Headers(request.headers).get('X-FC-Maintenance-Proof')).toBe(credential.proof)
    expect(new Headers(request.headers).has('X-FC-CSRF')).toBe(false)
  })
  it('uses repair authority without ordinary cookies and exchanges a one-time capability just once', async () => {
    const credentials = { session: 'b'.repeat(64), csrf: 'c'.repeat(64) }, fetcher = vi.fn<typeof fetch>().mockResolvedValue(json(credentials)), api = new RepairHttpApi(base, fetcher), boot = { capability: 'd'.repeat(64) }
    expect(await Promise.all([beginRepairSession(api, boot), beginRepairSession(api, boot)])).toEqual([credentials, credentials]); expect(fetcher).toHaveBeenCalledOnce()
    const request = fetcher.mock.calls[0]![1]!
    expect(request.credentials).toBe('omit'); expect(new Headers(request.headers).has('X-FC-Page')).toBe(false); expect(boot.capability).toBeUndefined()
  })
  it('groups outlet content once and does not offer fixed widgets inside it', () => {
    expect(allowedLayoutAdditions(layout, '1', 'public')).not.toContain('subject_text'); expect(allowedLayoutAdditions(layout, '2', 'public')).not.toContain('clock')
    const added = appendLayoutNode(layout, '2', 'subject_text', 'public'), wrapper = added.value.nodes.find(n => n.kind === 'container')!
    expect(wrapper.children).toEqual(['3', added.nodeId]); expect(added.value.nodes.find(n => n.id === '2')!.children).toEqual([wrapper.id])
    const second = appendLayoutNode(added.value, '2', 'subject_image', 'public'); expect(second.value.nodes.filter(n => n.kind === 'container')).toHaveLength(1); expect(second.value.nodes.find(n => n.id === wrapper.id)!.children).toContain(second.nodeId)
    expect(layout.nodes).toHaveLength(3)
  })
  it('uses fresh source revisions for preview and rejects invalid geometry', async () => {
    const geometry = { width: 1920, height: 1080, nodes: [], sourcePublicationRevision: '4', identityRevision: '2', displayRevision: '0', publicLayoutRevision: '3', subjectProfileRevision: null }
    const fetcher = vi.fn<typeof fetch>().mockResolvedValueOnce(json({ publicationRevision: '4', identityRevision: '2', displayRevision: '0', publicLayoutRevision: '3' })).mockResolvedValueOnce(json(geometry))
    expect(await new PreviewHttpApi(base, fetcher).preview({ layout, expectedRevision: '3' }, 'b'.repeat(64), signal())).toEqual(geometry)
    expect(JSON.parse(fetcher.mock.calls[1]![1]!.body as string).candidate).toMatchObject({ expectedPublicationRevision: '4', expectedIdentityRevision: '2', expectedDisplayRevision: '0', display: null, materials: null })
    expect(() => parsePreview({ ...geometry, width: 0 })).toThrow(ProtocolFault)
  })
})
