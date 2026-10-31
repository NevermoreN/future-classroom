import { describe, expect, it, vi } from 'vitest'
import { ProtocolVersionFault } from '../src/api/protocol'
import { ApiError, NetworkFault } from '../src/api/client'
import { MaterialController, type MaterialState } from '../src/api/material-controller'
import type { MaterialApi } from '../src/api/material-client'
import type { MaterialPage, MaterialRecord, MaterialScopeItem, MaterialScopes, UploadReady } from '../src/api/material-protocol'
const target: MaterialScopeItem = { ownerTeacherId: '9', subjectId: '2', ownerDisplayName: '合成老师', subjectName: '合成数学', classes: [{ classId: '1', className: '一班' }, { classId: '2', className: '二班' }], classTotal: 2, nextClassOffset: null }
const row = (): MaterialRecord => ({ materialId: '5', folderId: '4', displayName: '课件.txt', resourceVersionId: '6', kind: 'TXT', bytes: 6, sha256: 'a'.repeat(64), allowedClassIds: ['1'], addedAt: '2026-10-30T10:00:00', addedSequence: '1', status: 'PUBLISHED', revision: '1' })
const scopes = (): MaterialScopes => ({ identityRevision: '1', offset: 0, total: 1, nextOffset: null, items: [target] })
const page = (): MaterialPage => ({ scope: { ownerTeacherId: '9', subjectId: '2' }, folderId: '4', folderRevision: '1', offset: 0, total: 1, items: [row()], nextCursor: null })
const ready: UploadReady = { uploadId: '6', expiresInSeconds: 300, maxChunkBytes: 65536 }
function fixture() {
  const api = { materialScopes: vi.fn<MaterialApi['materialScopes']>().mockResolvedValue(scopes()), materialClasses: vi.fn<MaterialApi['materialClasses']>().mockResolvedValue({ identityRevision: '1', scope: target, offset: 2, total: 2, nextOffset: null, items: [] }), materials: vi.fn<MaterialApi['materials']>().mockResolvedValue(page()), createUpload: vi.fn<MaterialApi['createUpload']>().mockResolvedValue(ready), upload: vi.fn<MaterialApi['upload']>().mockResolvedValue(undefined), cancelUpload: vi.fn<MaterialApi['cancelUpload']>().mockResolvedValue(undefined), append: vi.fn<MaterialApi['append']>().mockImplementation(async (scope, command) => ({ commandId: command.commandId, scope, record: row() })), deleteMaterial: vi.fn<MaterialApi['deleteMaterial']>().mockImplementation(async (scope, command) => ({ commandId: command.commandId, scope, record: { ...row(), status: 'DELETED', revision: (BigInt(command.expectedMaterialRevision) + 1n).toString() } })), materialOutcome: vi.fn<MaterialApi['materialOutcome']>().mockResolvedValue({ commandId: '77', state: 'NOT_FOUND', scope: null, record: null }), download: vi.fn<MaterialApi['download']>().mockResolvedValue(new Blob(['sample'])) }
  let next = 76
  const changed = vi.fn<(state: MaterialState) => void>(), revoked = vi.fn(), controller = new MaterialController(api, 'b'.repeat(64), changed, revoked, () => String(++next))
  return { api, controller, changed, revoked, state: () => controller.snapshot() }
}
async function open(f: ReturnType<typeof fixture>) { await f.controller.loadScopes(); await f.controller.select(f.state().scopes!.items[0]!) }
async function transfer(f: ReturnType<typeof fixture>) { f.controller.pick(new File(['sample'], '课件.txt')); f.controller.setClasses(['1']); await f.controller.uploadFile() }
describe('material drafts, one-shot commands and exact result correlation', () => {
  it('uploads without append and only explicit addition submits a command', async () => {
    const f = fixture(); await open(f); await transfer(f)
    expect(f.state().phase).toBe('uploaded'); expect(f.api.append).not.toHaveBeenCalled(); expect(f.controller.hasRiskyDraft()).toBe(true); expect(f.controller.leave()).toBe(false)
    await f.controller.append(); expect(f.api.append).toHaveBeenCalledOnce(); expect(f.api.append.mock.calls[0]![1]).toEqual({ commandId: '77', uploadId: ready.uploadId, allowedClassIds: ['1'] }); expect(f.state().history?.status).toBe('PUBLISHED'); expect(f.state().notice).toContain('不代表'); expect(f.api.materials).toHaveBeenCalledOnce(); f.controller.dispose()
  })
  it('cancels upload and ignores late success from a transport disregarding abort', async () => {
    const f = fixture(); await open(f); let complete!: () => void
    f.api.upload.mockImplementationOnce(() => new Promise<void>(resolve => { complete = resolve })); f.controller.pick(new File(['sample'], '课件.txt')); const operation = f.controller.uploadFile(); await vi.waitFor(() => expect(f.api.upload).toHaveBeenCalledOnce())
    await f.controller.cancelUpload(); expect(f.api.upload.mock.calls[0]![3].aborted).toBe(true); complete(); await operation
    expect(f.state().phase).toBe('idle'); expect(f.state().upload).toBeNull(); await f.controller.append(); expect(f.api.append).not.toHaveBeenCalled(); f.controller.dispose()
  })
  it('explicitly cancels a completed upload once and cannot replace that upload before discard', async () => {
    const f = fixture(); await open(f); await transfer(f); f.controller.pick(new File(['other'], 'other.txt'))
    expect(f.state().file?.name).toBe('课件.txt'); expect(f.state().upload?.uploadId).toBe('6')
    await f.controller.cancelUpload(); await f.controller.cancelUpload(); await f.controller.append()
    expect(f.api.cancelUpload).toHaveBeenCalledExactlyOnceWith('6', 'b'.repeat(64), expect.any(AbortSignal)); expect(f.state().upload).toBeNull(); expect(f.state().phase).toBe('idle'); expect(f.api.append).not.toHaveBeenCalled(); f.controller.dispose()
  })
  it.each([new ApiError(429, 'CAPACITY'), new NetworkFault()])('unconfirmed cancellation does not recreate an upload or retry a write: %s', async error => {
    const f = fixture(); await open(f); await transfer(f); f.api.cancelUpload.mockRejectedValueOnce(error)
    await f.controller.cancelUpload(); await f.controller.append()
    expect(f.state().upload).toBeNull(); expect(f.state().busy).toBe(false); expect(f.state().notice).toContain('未确认'); expect(f.api.cancelUpload).toHaveBeenCalledOnce(); expect(f.api.append).not.toHaveBeenCalled(); f.controller.dispose()
  })
  it('cancellation before READY has no guessed ID and ignores the late server receipt', async () => {
    const f = fixture(); await open(f); let complete!: (value: UploadReady) => void
    f.api.createUpload.mockImplementationOnce(() => new Promise(resolve => { complete = resolve })); f.controller.pick(new File(['sample'], '课件.txt')); const operation = f.controller.uploadFile()
    await f.controller.cancelUpload(); complete(ready); await operation
    expect(f.api.cancelUpload).not.toHaveBeenCalled(); expect(f.api.upload).not.toHaveBeenCalled(); expect(f.api.append).not.toHaveBeenCalled(); expect(f.state().upload).toBeNull(); f.controller.dispose()
  })
  it('committing or unknown append cannot be cancelled as a temporary upload', async () => {
    const f = fixture(); await open(f); await transfer(f); let fail!: (reason: unknown) => void
    f.api.append.mockImplementationOnce(() => new Promise((_resolve, reject) => { fail = reject })); const operation = f.controller.append()
    await f.controller.cancelUpload(); expect(f.state().phase).toBe('committing'); fail(new NetworkFault()); await operation
    await f.controller.cancelUpload(); expect(f.state().phase).toBe('unknown'); expect(f.api.cancelUpload).not.toHaveBeenCalled(); expect(f.state().pending?.command.commandId).toBe('77'); f.controller.dispose()
  })
  it('cannot append an unconfirmed upload or repeat any write after unknown', async () => {
    const f = fixture(); await open(f); f.controller.pick(new File(['sample'], '课件.txt')); await f.controller.append(); expect(f.api.append).not.toHaveBeenCalled()
    f.api.upload.mockRejectedValueOnce(new NetworkFault()); await f.controller.uploadFile(); await f.controller.append(); expect(f.api.append).not.toHaveBeenCalled()
    await transfer(f); f.api.append.mockRejectedValueOnce(new ApiError(503, 'RESULT_UNKNOWN')); await f.controller.append(); const pending = f.state().pending
    await f.controller.uploadFile(); await f.controller.append(); f.controller.pick(new File(['other'], 'other.txt')); f.controller.setClasses(['2'])
    expect(f.state().phase).toBe('unknown'); expect(f.state().pending).toBe(pending); expect(f.api.append).toHaveBeenCalledOnce(); expect(f.state().file?.name).toBe('课件.txt'); f.controller.dispose()
  })
  it('NOT_FOUND retains the exact command; historical success requires explicit current GET', async () => {
    const f = fixture(); await open(f); await transfer(f); f.api.append.mockRejectedValueOnce(new NetworkFault()); await f.controller.append(); const pending = f.state().pending; await f.controller.query()
    expect(f.state().pending).toBe(pending); expect(f.state().phase).toBe('unknown'); expect(f.state().notice).toContain('不证明'); expect(f.api.append).toHaveBeenCalledOnce()
    f.api.materialOutcome.mockResolvedValueOnce({ commandId: '77', state: 'COMMITTED', scope: target, record: row() }); await f.controller.query(); expect(f.state().phase).toBe('result'); expect(f.state().history?.materialId).toBe('5'); expect(f.api.materials).toHaveBeenCalledOnce()
    const latest = { ...page(), folderRevision: '2', total: 0, items: [] }; f.api.materials.mockResolvedValueOnce(latest); await f.controller.loadPage(null, true)
    expect(f.state().page).toEqual(latest); expect(f.state().pending).toBeNull(); expect(f.state().history).toBeNull(); expect(f.api.append).toHaveBeenCalledOnce(); f.controller.dispose()
  })
  it('foreign committed scope cannot resolve the original unknown command', async () => {
    const f = fixture(); await open(f); await transfer(f); f.api.append.mockRejectedValueOnce(new NetworkFault()); await f.controller.append()
    f.api.materialOutcome.mockResolvedValueOnce({ commandId: '77', state: 'COMMITTED', scope: { ownerTeacherId: '10', subjectId: '2' }, record: row() }); await f.controller.query()
    expect(f.state().phase).toBe('unknown'); expect(f.state().history).toBeNull(); expect(f.state().pending?.command.commandId).toBe('77'); expect(f.api.append).toHaveBeenCalledOnce(); f.controller.dispose()
  })
  it('captures immutable original resource version and revision for one delete', async () => {
    const f = fixture(); await open(f); const original = f.state().page!.items[0]!
    f.api.deleteMaterial.mockRejectedValueOnce(new NetworkFault()); await f.controller.delete(original)
    expect(f.api.deleteMaterial.mock.calls[0]![1]).toEqual({ commandId: '77', materialId: '5', resourceVersionId: '6', expectedMaterialRevision: '1' })
    original.revision = '9'; original.resourceVersionId = '99'; await f.controller.delete(original); expect(f.api.deleteMaterial).toHaveBeenCalledOnce()
    const p = f.state().pending; expect(p?.kind).toBe('DELETE'); if (p?.kind === 'DELETE') expect(p.command.expectedMaterialRevision).toBe('1')
    f.api.materialOutcome.mockResolvedValueOnce({ commandId: '77', state: 'COMMITTED', scope: target, record: { ...row(), status: 'DELETED', revision: '2' } }); await f.controller.query(); expect(f.state().phase).toBe('result'); f.controller.dispose()
  })
  it('a new append receipt cannot resolve as an already revised material', async () => {
    const f = fixture(); await open(f); await transfer(f)
    f.api.append.mockResolvedValueOnce({ commandId: '77', scope: target, record: { ...row(), revision: '2' } }); await f.controller.append()
    expect(f.state().phase).toBe('unknown'); expect(f.state().history).toBeNull(); expect(f.state().pending?.command.commandId).toBe('77'); f.controller.dispose()
  })
  it('delete cannot confirm different content bytes under the original fixed resource version', async () => {
    const f = fixture(); await open(f)
    f.api.deleteMaterial.mockResolvedValueOnce({ commandId: '77', scope: target, record: { ...row(), sha256: 'b'.repeat(64), status: 'DELETED', revision: '2' } }); await f.controller.delete(f.state().page!.items[0]!)
    expect(f.state().phase).toBe('unknown'); expect(f.state().history).toBeNull(); expect(f.state().pending?.command.commandId).toBe('77'); f.controller.dispose()
  })
  it('never deletes a version differing from the loaded row', async () => {
    const f = fixture(); await open(f); await f.controller.delete({ ...f.state().page!.items[0]!, resourceVersionId: '999' }); expect(f.api.deleteMaterial).not.toHaveBeenCalled(); f.controller.dispose()
  })
  it.each([401, 403])('authentication failure %i clears all draft and command mirrors', async status => {
    const f = fixture(); await open(f); f.controller.pick(new File(['sensitive'], 'sensitive.txt')); f.api.createUpload.mockRejectedValueOnce(new ApiError(status, status === 401 ? 'UNAUTHORIZED' : 'FORBIDDEN')); await f.controller.uploadFile()
    expect(f.revoked).toHaveBeenCalledOnce(); expect(f.state().selected).toBeNull(); expect(f.state().page).toBeNull(); expect(f.state().file).toBeNull(); expect(f.state().pending).toBeNull(); expect(f.controller.hasRiskyDraft()).toBe(false); f.controller.dispose()
  })
  it('identity-changed continuation blocks old upload without transferring it', async () => {
    const f = fixture(); f.api.materialScopes.mockResolvedValueOnce({ ...scopes(), total: 3, nextOffset: 1 }); await open(f); f.controller.pick(new File(['sample'], '课件.txt')); await f.controller.loadScopes(1); expect(f.api.materialScopes).toHaveBeenCalledOnce()
    f.api.materialScopes.mockResolvedValueOnce({ ...scopes(), offset: 1, identityRevision: '2', total: 3, nextOffset: 2 }); await f.controller.loadScopes(1, true)
    expect(f.state().phase).toBe('blocked'); expect(f.state().selected?.ownerTeacherId).toBe('9'); await f.controller.uploadFile(); expect(f.api.createUpload).not.toHaveBeenCalled(); f.controller.dispose()
  })
  it('makes all 128 classes reachable only through explicit, sequential continuation', async () => {
    const f = fixture(), classes = Array.from({ length: 20 }, (_, i) => ({ classId: String(i + 1), className: '班' + (i + 1) }))
    f.api.materialScopes.mockResolvedValueOnce({ ...scopes(), items: [{ ...target, classes, classTotal: 128, nextClassOffset: 20 }] }); await open(f)
    expect(f.api.materialClasses).not.toHaveBeenCalled(); f.controller.setClasses(['128']); expect(f.state().classes).toEqual([])
    f.api.materialClasses.mockImplementation(async (scope, offset) => ({ identityRevision: '1', scope, offset, total: 128, nextOffset: offset + 20 < 128 ? offset + 20 : null, items: Array.from({ length: Math.min(20, 128 - offset) }, (_, i) => ({ classId: String(offset + i + 1), className: '班' + (offset + i + 1) })) }))
    for (let offset = 20; offset < 128; offset += 20) await f.controller.loadMoreClasses()
    expect(f.api.materialClasses.mock.calls.map(c => c[1])).toEqual([20, 40, 60, 80, 100, 120]); expect(f.state().selected?.classes).toHaveLength(128); expect(f.state().selected?.nextClassOffset).toBeNull()
    f.controller.setClasses(['128', '1']); expect(f.state().classes).toEqual(['1', '128']); expect(f.api.append).not.toHaveBeenCalled(); f.controller.dispose()
  })
  it.each(['scope', 'identity', 'total', 'repeat', 'offset'])('class continuation %s mismatch blocks old selection without moving the file', async fault => {
    const f = fixture(), classes = Array.from({ length: 20 }, (_, i) => ({ classId: String(i + 1), className: '班' + (i + 1) }))
    f.api.materialScopes.mockResolvedValueOnce({ ...scopes(), items: [{ ...target, classes, classTotal: 40, nextClassOffset: 20 }] }); await open(f); f.controller.pick(new File(['sample'], '课件.txt'))
    const result = { identityRevision: '1', scope: { ownerTeacherId: '9', subjectId: '2' }, offset: 20, total: 40, nextOffset: null, items: Array.from({ length: 20 }, (_, i) => ({ classId: String(i + 21), className: '班' + (i + 21) })) }
    if (fault === 'scope') result.scope.ownerTeacherId = '10'; if (fault === 'identity') result.identityRevision = '2'; if (fault === 'total') result.total = 41; if (fault === 'repeat') result.items[0]!.classId = '20'; if (fault === 'offset') result.offset = 21
    f.api.materialClasses.mockResolvedValueOnce(result); await f.controller.loadMoreClasses()
    expect(f.state().phase).toBe('blocked'); expect(f.state().selected?.classes).toHaveLength(20); expect(f.state().file?.name).toBe('课件.txt'); await f.controller.uploadFile(); expect(f.api.createUpload).not.toHaveBeenCalled(); f.controller.dispose()
  })
  it('keeps actual partial offsets and blocks a stale folder cursor', async () => {
    const f = fixture(); f.api.materialScopes.mockResolvedValueOnce({ ...scopes(), total: 3, nextOffset: 1 }); await f.controller.loadScopes(); f.api.materialScopes.mockResolvedValueOnce({ ...scopes(), offset: 1, total: 3, nextOffset: 2, items: [{ ...target, ownerTeacherId: '10' }] }); await f.controller.loadScopes(1); expect(f.state().previousScopeOffset).toBe(0)
    await f.controller.select(f.state().scopes!.items[0]!); f.api.materials.mockRejectedValueOnce(new ApiError(409, 'STALE_CURSOR')); await f.controller.loadPage({ folderId: '4', folderRevision: '1', offset: 1 }, true)
    expect(f.state().phase).toBe('blocked'); expect(f.state().page).toBeNull(); expect(f.state().notice).toContain('第一页'); f.controller.dispose()
  })
  it('explicit leave aborts in-flight upload and late receipts cannot repopulate state', async () => {
    const f = fixture(); await open(f); let complete!: (ready: UploadReady) => void
    f.api.createUpload.mockImplementationOnce(() => new Promise(resolve => { complete = resolve })); f.controller.pick(new File(['sample'], '课件.txt')); const operation = f.controller.uploadFile()
    expect(f.controller.leave()).toBe(false); expect(f.controller.leave(true)).toBe(true); complete(ready); await operation
    expect(f.api.createUpload.mock.calls[0]![3].aborted).toBe(true); expect(f.api.upload).not.toHaveBeenCalled(); expect(f.state().file).toBeNull(); expect(f.state().selected).toBeNull(); f.controller.dispose()
  })
})

describe('material version recovery keeps files and original commands', () => {
  it('preserves an incompatible append and file selection without another POST, including after manual result GET', async () => {
    const f = fixture(), fault = new ProtocolVersionFault(); await open(f); await transfer(f)
    const file = f.state().file, upload = f.state().upload, classes = f.state().classes
    f.api.append.mockRejectedValueOnce(fault); await f.controller.append(); const pending = f.state().pending
    expect(pending?.command.commandId).toBe('77'); expect(f.state().notice).toContain(fault.message); expect(f.state().phase).toBe('unknown')
    expect(f.state().file).toBe(file); expect(f.state().upload).toBe(upload); expect(f.state().classes).toBe(classes)
    await f.controller.append(); await f.controller.uploadFile(); await f.controller.cancelUpload(); await f.controller.delete(row()); await f.controller.loadPage(null, true); await f.controller.loadScopes(0, true)
    f.controller.pick(new File(['other'], 'other.txt')); f.controller.setClasses(['2'])
    expect(f.api.append).toHaveBeenCalledOnce(); expect(f.api.createUpload).toHaveBeenCalledOnce(); expect(f.api.cancelUpload).not.toHaveBeenCalled(); expect(f.api.deleteMaterial).not.toHaveBeenCalled(); expect(f.api.materials).toHaveBeenCalledOnce(); expect(f.api.materialScopes).toHaveBeenCalledOnce()
    await f.controller.query(); expect(f.state().pending).toBe(pending); expect(f.state().notice).toContain('不证明'); expect(f.state().notice).toContain(fault.message)
    f.api.materialOutcome.mockResolvedValueOnce({ commandId: '77', state: 'COMMITTED', scope: target, record: row() }); await f.controller.query()
    expect(f.state().pending).toBe(pending); expect(f.state().history?.materialId).toBe('5'); expect(f.state().file).toBe(file); expect(f.state().upload).toBe(upload); expect(f.state().classes).toBe(classes); expect(f.state().notice).toContain(fault.message)
    expect(f.api.materialOutcome.mock.calls.map(call => call[0])).toEqual(['77', '77']); await f.controller.append(); expect(f.api.append).toHaveBeenCalledOnce(); expect(f.controller.leave()).toBe(false); f.controller.dispose()
  })
  it('keeps an unsubmitted file and classes on a typed upload failure and does not recreate the stage', async () => {
    const f = fixture(); await open(f); const file = new File(['sample'], '课件.txt'); f.controller.pick(file); f.controller.setClasses(['1'])
    f.api.createUpload.mockRejectedValueOnce(new ProtocolVersionFault()); await f.controller.uploadFile()
    expect(f.state().file).toBe(file); expect(f.state().classes).toEqual(['1']); expect(f.state().phase).toBe('blocked'); expect(f.state().pending).toBeNull(); expect(f.state().notice).toContain('刷新页面')
    await f.controller.uploadFile(); await f.controller.append(); await f.controller.delete(row()); expect(f.api.createUpload).toHaveBeenCalledOnce(); expect(f.api.upload).not.toHaveBeenCalled(); expect(f.api.append).not.toHaveBeenCalled(); expect(f.api.deleteMaterial).not.toHaveBeenCalled(); f.controller.dispose()
  })
  it('keeps exact unknown delete metadata when command-query decoding reports a version change', async () => {
    const f = fixture(); await open(f); f.api.deleteMaterial.mockRejectedValueOnce(new NetworkFault()); await f.controller.delete(row()); const pending = f.state().pending
    f.api.materialOutcome.mockRejectedValueOnce(new ProtocolVersionFault()); await f.controller.query(); expect(f.state().pending).toBe(pending); expect(f.state().notice).toContain('管理员升级')
    await f.controller.query(); expect(f.api.materialOutcome.mock.calls.map(call => call[0])).toEqual(['77', '77']); expect(f.state().pending).toBe(pending); await f.controller.delete(row()); expect(f.api.deleteMaterial).toHaveBeenCalledOnce(); f.controller.dispose()
  })
})
