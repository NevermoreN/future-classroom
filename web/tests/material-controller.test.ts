import { identity, ticket } from './command-fixtures'
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
  let next = 76
  let batchFiles: { displayName: string; bytes: number }[] = []
  const api = {
    deleteBatch: vi.fn<MaterialApi['deleteBatch']>().mockImplementation(async (scope, command) => ({ ...identity(command.commandId), scope, records: command.targets.map(t => ({ ...row(), materialId: t.materialId, resourceVersionId: t.resourceVersionId, status: 'DELETED' as const, revision: (BigInt(t.expectedMaterialRevision) + 1n).toString() })) })),
    deleteBatchOutcome: vi.fn<MaterialApi['deleteBatchOutcome']>().mockResolvedValue({ ...identity('77'), state: 'NOT_FOUND', scope: null, records: null }),
    createBatch: vi.fn<MaterialApi['createBatch']>().mockImplementation(async (_scope, files) => { batchFiles = files.map(f => ({ ...f })); return { batchId: '50', expiresInSeconds: 300, maxChunkBytes: 65536, items: batchFiles.map((f, i) => ({ ...f, uploadId: String(600 + i) })) } }),
    uploadBatchMember: vi.fn<MaterialApi['uploadBatchMember']>().mockResolvedValue(undefined),
    cancelBatch: vi.fn<MaterialApi['cancelBatch']>().mockResolvedValue(undefined),
    appendBatch: vi.fn<MaterialApi['appendBatch']>().mockImplementation(async (scope, command) => ({ ...identity(command.commandId), scope, records: batchFiles.map((f, i) => ({ ...row(), materialId: String(500 + i), resourceVersionId: command.uploadIds[i]!, displayName: f.displayName, bytes: f.bytes, addedSequence: String(i + 1), allowedClassIds: command.allowedClassIds.slice() })) })),
    batchOutcome: vi.fn<MaterialApi['batchOutcome']>().mockResolvedValue({ ...identity('77'), state: 'NOT_FOUND', scope: null, records: null }),
    issueCommand: vi.fn<MaterialApi['issueCommand']>().mockImplementation(async () => identity(String(++next))), materialScopes: vi.fn<MaterialApi['materialScopes']>().mockResolvedValue(scopes()), materialClasses: vi.fn<MaterialApi['materialClasses']>().mockResolvedValue({ identityRevision: '1', scope: target, offset: 2, total: 2, nextOffset: null, items: [] }), materials: vi.fn<MaterialApi['materials']>().mockResolvedValue(page()), createUpload: vi.fn<MaterialApi['createUpload']>().mockResolvedValue(ready), upload: vi.fn<MaterialApi['upload']>().mockResolvedValue(undefined), cancelUpload: vi.fn<MaterialApi['cancelUpload']>().mockResolvedValue(undefined), append: vi.fn<MaterialApi['append']>().mockImplementation(async (scope, command) => ({ commandId: command.commandId, commandTicket: ticket, scope, record: row() })), deleteMaterial: vi.fn<MaterialApi['deleteMaterial']>().mockImplementation(async (scope, command) => ({ commandId: command.commandId, commandTicket: ticket, scope, record: { ...row(), status: 'DELETED', revision: (BigInt(command.expectedMaterialRevision) + 1n).toString() } })), materialOutcome: vi.fn<MaterialApi['materialOutcome']>().mockResolvedValue({ commandId: '77', commandTicket: ticket, state: 'NOT_FOUND', scope: null, record: null }), download: vi.fn<MaterialApi['download']>().mockResolvedValue(new Blob(['sample'])) }
  const changed = vi.fn<(state: MaterialState) => void>(), revoked = vi.fn(), controller = new MaterialController(api, 'b'.repeat(64), changed, revoked)
  return { api, controller, changed, revoked, state: () => controller.snapshot() }
}
async function open(f: ReturnType<typeof fixture>) { await f.controller.loadScopes(); await f.controller.select(f.state().scopes!.items[0]!) }
async function transfer(f: ReturnType<typeof fixture>) { f.controller.pick(new File(['sample'], '课件.txt')); f.controller.setClasses(['1']); await f.controller.uploadFile() }
describe('material drafts, one-shot commands and exact result correlation', () => {
  it('uploads without append and only explicit addition submits a command', async () => {
    const f = fixture(); await open(f); await transfer(f)
    expect(f.state().phase).toBe('uploaded'); expect(f.api.append).not.toHaveBeenCalled(); expect(f.controller.hasRiskyDraft()).toBe(true); expect(f.controller.leave()).toBe(false)
    await f.controller.append(); expect(f.api.append).toHaveBeenCalledOnce(); expect(f.api.append.mock.calls[0]![1]).toEqual({ commandId: '77', commandTicket: ticket, uploadId: ready.uploadId, allowedClassIds: ['1'] }); expect(f.state().history?.status).toBe('PUBLISHED'); expect(f.state().notice).toContain('不代表'); expect(f.api.materials).toHaveBeenCalledOnce(); f.controller.dispose()
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
    f.api.materialOutcome.mockResolvedValueOnce({ commandId: '77', commandTicket: ticket, state: 'COMMITTED', scope: target, record: row() }); await f.controller.query(); expect(f.state().phase).toBe('result'); expect(f.state().history?.materialId).toBe('5'); expect(f.api.materials).toHaveBeenCalledOnce()
    const latest = { ...page(), folderRevision: '2', total: 0, items: [] }; f.api.materials.mockResolvedValueOnce(latest); await f.controller.loadPage(null, true)
    expect(f.state().page).toEqual(latest); expect(f.state().pending).toBeNull(); expect(f.state().history).toBeNull(); expect(f.api.append).toHaveBeenCalledOnce(); f.controller.dispose()
  })
  it('foreign committed scope cannot resolve the original unknown command', async () => {
    const f = fixture(); await open(f); await transfer(f); f.api.append.mockRejectedValueOnce(new NetworkFault()); await f.controller.append()
    f.api.materialOutcome.mockResolvedValueOnce({ commandId: '77', commandTicket: ticket, state: 'COMMITTED', scope: { ownerTeacherId: '10', subjectId: '2' }, record: row() }); await f.controller.query()
    expect(f.state().phase).toBe('unknown'); expect(f.state().history).toBeNull(); expect(f.state().pending?.command.commandId).toBe('77'); expect(f.api.append).toHaveBeenCalledOnce(); f.controller.dispose()
  })
  it('captures immutable original resource version and revision for one delete', async () => {
    const f = fixture(); await open(f); const original = f.state().page!.items[0]!
    f.api.deleteMaterial.mockRejectedValueOnce(new NetworkFault()); await f.controller.delete(original)
    expect(f.api.deleteMaterial.mock.calls[0]![1]).toEqual({ commandId: '77', commandTicket: ticket, materialId: '5', resourceVersionId: '6', expectedMaterialRevision: '1' })
    original.revision = '9'; original.resourceVersionId = '99'; await f.controller.delete(original); expect(f.api.deleteMaterial).toHaveBeenCalledOnce()
    const p = f.state().pending; expect(p?.kind).toBe('DELETE'); if (p?.kind === 'DELETE') expect(p.command.expectedMaterialRevision).toBe('1')
    f.api.materialOutcome.mockResolvedValueOnce({ commandId: '77', commandTicket: ticket, state: 'COMMITTED', scope: target, record: { ...row(), status: 'DELETED', revision: '2' } }); await f.controller.query(); expect(f.state().phase).toBe('result'); f.controller.dispose()
  })
  it('a new append receipt cannot resolve as an already revised material', async () => {
    const f = fixture(); await open(f); await transfer(f)
    f.api.append.mockResolvedValueOnce({ commandId: '77', commandTicket: ticket, scope: target, record: { ...row(), revision: '2' } }); await f.controller.append()
    expect(f.state().phase).toBe('unknown'); expect(f.state().history).toBeNull(); expect(f.state().pending?.command.commandId).toBe('77'); f.controller.dispose()
  })
  it('delete cannot confirm different content bytes under the original fixed resource version', async () => {
    const f = fixture(); await open(f)
    f.api.deleteMaterial.mockResolvedValueOnce({ commandId: '77', commandTicket: ticket, scope: target, record: { ...row(), sha256: 'b'.repeat(64), status: 'DELETED', revision: '2' } }); await f.controller.delete(f.state().page!.items[0]!)
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
    f.api.materialOutcome.mockResolvedValueOnce({ commandId: '77', commandTicket: ticket, state: 'COMMITTED', scope: target, record: row() }); await f.controller.query()
    expect(f.state().pending).toBe(pending); expect(f.state().history?.materialId).toBe('5'); expect(f.state().file).toBe(file); expect(f.state().upload).toBe(upload); expect(f.state().classes).toBe(classes); expect(f.state().notice).toContain(fault.message)
    expect(f.api.materialOutcome.mock.calls.map(call => call[0])).toEqual([identity('77'), identity('77')]); await f.controller.append(); expect(f.api.append).toHaveBeenCalledOnce(); expect(f.controller.leave()).toBe(false); f.controller.dispose()
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
    await f.controller.query(); expect(f.api.materialOutcome.mock.calls.map(call => call[0])).toEqual([identity('77'), identity('77')]); expect(f.state().pending).toBe(pending); await f.controller.delete(row()); expect(f.api.deleteMaterial).toHaveBeenCalledOnce(); f.controller.dispose()
  })
})

describe('material server ticket generation boundary', () => {
  it('never binds a late ticket to a replaced scope or file after discard', async () => {
    const f = fixture(); await open(f); await transfer(f)
    let issue!: (pair: ReturnType<typeof identity>) => void
    f.api.issueCommand.mockImplementationOnce(() => new Promise(resolve => { issue = resolve }))
    const sending = f.controller.append(); expect(f.api.append).not.toHaveBeenCalled()
    expect(f.controller.leave(true)).toBe(true); issue(identity()); await sending
    expect(f.api.append).not.toHaveBeenCalled(); expect(f.state().pending).toBeNull(); f.controller.dispose()
  })
  it('retains upload and exact classes when no usable ticket is delivered', async () => {
    const f = fixture(); await open(f); await transfer(f); f.api.issueCommand.mockRejectedValueOnce(new ApiError(503, 'COMMAND_WINDOW_FULL')); await f.controller.append()
    expect(f.state().phase).toBe('uploaded'); expect(f.state().upload?.uploadId).toBe('6'); expect(f.state().classes).toEqual(['1']); expect(f.api.append).not.toHaveBeenCalled(); expect(f.api.issueCommand).toHaveBeenCalledExactlyOnceWith('materials', 'b'.repeat(64), expect.any(AbortSignal)); f.controller.dispose()
  })
  it('stops an expired original pair without a replacement issuance or POST', async () => {
    const f = fixture(); await open(f); await transfer(f); f.api.append.mockRejectedValueOnce(new NetworkFault()); await f.controller.append()
    f.api.materialOutcome.mockRejectedValueOnce(new ApiError(410, 'COMMAND_EXPIRED')); await f.controller.query()
    expect(f.state().pending?.retired).toBe(true); expect(f.state().file?.name).toBe('课件.txt')
    await f.controller.query(); await f.controller.append(); expect(f.api.materialOutcome).toHaveBeenCalledOnce(); expect(f.api.append).toHaveBeenCalledOnce(); expect(f.api.issueCommand).toHaveBeenCalledOnce(); f.controller.dispose()
  })
  it('does not accept a NOT_FOUND for another ticket', async () => {
    const f = fixture(); await open(f); await transfer(f); f.api.append.mockRejectedValueOnce(new NetworkFault()); await f.controller.append()
    f.api.materialOutcome.mockResolvedValueOnce({ ...identity(), commandTicket: ticket.replace('12', '34'), state: 'NOT_FOUND', scope: null, record: null }); await f.controller.query()
    expect(f.state().phase).toBe('unknown'); expect(f.state().pending?.command.commandTicket).toBe(ticket); expect(f.api.issueCommand).toHaveBeenCalledOnce(); f.controller.dispose()
  })
})


async function transferBatch(f: ReturnType<typeof fixture>, files = [new File(['one'], 'first.txt'), new File(['second'], 'last.txt')]) {
  f.controller.pickFiles(files); f.controller.setClasses(['1']); await f.controller.uploadBatch()
}
function historicalBatch(f: ReturnType<typeof fixture>) {
  const p = f.state().pendingBatch!
  return { ...identity(p.command.commandId), state: 'COMMITTED' as const, scope: p.scope, records: p.files.map((file, i) => ({ ...row(), materialId: String(500 + i), resourceVersionId: file.uploadId, displayName: file.displayName, bytes: file.bytes, addedSequence: String(i + 1), allowedClassIds: p.command.allowedClassIds.slice() })) }
}
describe('ordered atomic material batch workflow (synthetic API, not server execution)', () => {
  it('reviews, reorders and removes before capturing one immutable group and one explicit command', async () => {
    const f = fixture(); await open(f)
    const first = new File(['one'], 'first.txt'), last = new File(['last'], 'last.txt'), removed = new File(['x'], 'remove.txt')
    f.controller.pickFiles([first, removed, last]); f.controller.moveFile(2, -1); f.controller.removeFile(2); f.controller.moveFile(0, 1); f.controller.setClasses(['2', '1'])
    expect(f.state().files).toEqual([last, first]); await f.controller.uploadBatch()
    expect(f.api.createBatch).toHaveBeenCalledExactlyOnceWith(expect.objectContaining(target), [{ displayName: 'last.txt', bytes: 4 }, { displayName: 'first.txt', bytes: 3 }], ['1', '2'], 'b'.repeat(64), expect.any(AbortSignal))
    f.controller.moveFile(0, 1); f.controller.removeFile(0); f.controller.setClasses(['2'])
    expect(f.state().files).toEqual([last, first]); expect(f.state().batchClasses).toEqual(['1', '2']); expect(f.api.issueCommand).not.toHaveBeenCalled(); expect(f.api.appendBatch).not.toHaveBeenCalled()
    await f.controller.appendBatch(); await f.controller.appendBatch()
    expect(f.api.issueCommand).toHaveBeenCalledOnce(); expect(f.api.appendBatch).toHaveBeenCalledExactlyOnceWith(expect.objectContaining(target), { ...identity('77'), batchId: '50', uploadIds: ['600', '601'], allowedClassIds: ['1', '2'] }, 'b'.repeat(64), expect.any(AbortSignal)); expect(f.api.append).not.toHaveBeenCalled()
    expect(f.state().batchHistory?.map(r => r.displayName)).toEqual(['last.txt', 'first.txt']); expect(f.state().page?.items).toEqual([row()]); f.controller.dispose()
  })
  it('uploads at most twenty files strictly serially and never auto-commits the ready group', async () => {
    const f = fixture(); await open(f); const files = Array.from({ length: 20 }, (_, i) => new File(['x'], `file-${i}.txt`))
    f.controller.pickFiles(files); f.controller.setClasses(['1'])
    let release!: () => void; f.api.uploadBatchMember.mockImplementationOnce(() => new Promise(resolve => { release = resolve }))
    const operation = f.controller.uploadBatch(); await vi.waitFor(() => expect(f.api.uploadBatchMember).toHaveBeenCalledOnce())
    expect(f.state().batchUploaded).toBe(0); expect(f.api.appendBatch).not.toHaveBeenCalled(); release(); await operation
    expect(f.api.uploadBatchMember.mock.calls.map(c => c[2])).toEqual(files); expect(f.state().batchUploaded).toBe(20); expect(f.state().phase).toBe('uploaded'); expect(f.api.issueCommand).not.toHaveBeenCalled(); await f.controller.appendBatch(); expect(f.state().batchHistory).toHaveLength(20); f.controller.dispose()
  })
  it('rejects zero, twenty-one or a bad member together while keeping the original draft', async () => {
    const f = fixture(); await open(f); const original = [new File(['x'], 'ok.txt')]; f.controller.pickFiles(original)
    for (const files of [[], Array.from({ length: 21 }, () => new File(['x'], 'same.txt')), [new File(['x'], 'bad.exe')], [new File([], 'zero.txt')]]) { f.controller.pickFiles(files); expect(f.state().files).toEqual(original) }
    expect(f.api.createBatch).not.toHaveBeenCalled(); expect(f.api.issueCommand).not.toHaveBeenCalled(); f.controller.dispose()
  })
  it('keeps a failed whole group until explicit cleanup and never retries a member or sends a command', async () => {
    const f = fixture(); await open(f); f.api.uploadBatchMember.mockResolvedValueOnce(undefined).mockRejectedValueOnce(new NetworkFault()); await transferBatch(f)
    expect(f.state().phase).toBe('upload-failed'); expect(f.state().batchUploaded).toBe(1); await f.controller.uploadBatch(); await f.controller.appendBatch(); expect(f.api.createBatch).toHaveBeenCalledOnce(); expect(f.api.issueCommand).not.toHaveBeenCalled()
    f.api.cancelBatch.mockRejectedValueOnce(new NetworkFault()); await f.controller.cancelBatch(); expect(f.state().batch?.batchId).toBe('50'); await f.controller.uploadBatch(); expect(f.api.createBatch).toHaveBeenCalledOnce()
    await f.controller.cancelBatch(); expect(f.state().batch).toBeNull(); expect(f.state().files).toHaveLength(2); expect(f.api.cancelBatch.mock.calls.map(c => c[0])).toEqual(['50', '50']); f.controller.dispose()
  })
  it('does not guess a batch ID or send late bytes after cancellation before READY', async () => {
    const f = fixture(); await open(f); let ready!: (value: Awaited<ReturnType<MaterialApi['createBatch']>>) => void
    f.api.createBatch.mockImplementationOnce(() => new Promise(resolve => { ready = resolve })); f.controller.pickFiles([new File(['x'], 'one.txt')]); f.controller.setClasses(['1']); const uploading = f.controller.uploadBatch(); await f.controller.cancelBatch()
    ready({ batchId: '51', expiresInSeconds: 300, maxChunkBytes: 65536, items: [{ uploadId: '601', displayName: 'one.txt', bytes: 1 }] }); await uploading
    expect(f.api.uploadBatchMember).not.toHaveBeenCalled(); expect(f.api.cancelBatch).not.toHaveBeenCalled(); expect(f.state().batch).toBeNull(); expect(f.api.issueCommand).not.toHaveBeenCalled(); f.controller.dispose()
  })
  it('keeps the original pair through UNKNOWN, NOT_FOUND and current reads, then shows its separate history', async () => {
    const f = fixture(); await open(f); await transferBatch(f); f.api.appendBatch.mockRejectedValueOnce(new ApiError(503, 'RESULT_UNKNOWN')); await f.controller.appendBatch(); const p = f.state().pendingBatch
    await f.controller.appendBatch(); await f.controller.append(); await f.controller.uploadBatch(); await f.controller.cancelBatch(); await f.controller.queryBatch()
    expect(f.state().pendingBatch).toBe(p); expect(f.state().phase).toBe('unknown'); expect(f.api.appendBatch).toHaveBeenCalledOnce(); expect(f.api.issueCommand).toHaveBeenCalledOnce(); expect(f.api.cancelBatch).not.toHaveBeenCalled()
    await f.controller.loadPage(null, true); expect(f.state().pendingBatch).toBe(p); expect(f.state().phase).toBe('unknown'); expect(f.state().batchHistory).toBeNull()
    expect(f.state().classes).toEqual(['1']); f.api.materials.mockRejectedValueOnce(new NetworkFault()); await f.controller.loadPage(null, true); expect(f.state().pendingBatch).toBe(p); expect(f.state().phase).toBe('unknown')
    f.api.batchOutcome.mockResolvedValueOnce(historicalBatch(f)); await f.controller.queryBatch(); expect(f.state().batchHistory).toHaveLength(2); expect(f.state().page).toBeNull(); expect(f.api.batchOutcome.mock.calls.map(c => c[1])).toEqual([identity('77'), identity('77')]); f.controller.dispose()
  })
  it.each(['order', 'classes', 'version', 'count', 'sequence', 'folder', 'time'])('cannot accept a malformed whole historical result: %s', async fault => {
    const f = fixture(); await open(f); await transferBatch(f); f.api.appendBatch.mockRejectedValueOnce(new NetworkFault()); await f.controller.appendBatch(); const result = historicalBatch(f)
    if (fault === 'order') result.records.reverse(); if (fault === 'classes') result.records[1]!.allowedClassIds = ['2']; if (fault === 'version') result.records[1]!.resourceVersionId = '999'; if (fault === 'count') result.records.pop(); if (fault === 'sequence') result.records[1]!.addedSequence = '4'; if (fault === 'folder') result.records[1]!.folderId = '99'; if (fault === 'time') result.records[1]!.addedAt = '2026-10-30T11:00:00'
    f.api.batchOutcome.mockResolvedValueOnce(result); await f.controller.queryBatch(); expect(f.state().phase).toBe('unknown'); expect(f.state().batchHistory).toBeNull(); expect(f.api.appendBatch).toHaveBeenCalledOnce(); f.controller.dispose()
  })
  it('expires the original batch without replacing its ticket and ignores late issuance after leave', async () => {
    const f = fixture(); await open(f); await transferBatch(f); f.api.appendBatch.mockRejectedValueOnce(new NetworkFault()); await f.controller.appendBatch(); f.api.batchOutcome.mockRejectedValueOnce(new ApiError(410, 'COMMAND_EXPIRED')); await f.controller.queryBatch(); await f.controller.queryBatch(); await f.controller.appendBatch()
    expect(f.state().pendingBatch?.retired).toBe(true); expect(f.api.batchOutcome).toHaveBeenCalledOnce(); expect(f.api.issueCommand).toHaveBeenCalledOnce(); f.controller.dispose()
    const late = fixture(); await open(late); await transferBatch(late); let issue!: (value: ReturnType<typeof identity>) => void; late.api.issueCommand.mockImplementationOnce(() => new Promise(resolve => { issue = resolve })); const op = late.controller.appendBatch(); late.controller.leave(true); issue(identity()); await op; expect(late.api.appendBatch).not.toHaveBeenCalled(); late.controller.dispose()
  })
})


it('a failed current-scope read cannot disable manual lookup of the original unknown batch', async () => {
  const f = fixture(); await open(f); await transferBatch(f); f.api.appendBatch.mockRejectedValueOnce(new NetworkFault()); await f.controller.appendBatch(); const pending = f.state().pendingBatch
  f.api.materialScopes.mockRejectedValueOnce(new NetworkFault()); await f.controller.loadScopes(0, true)
  expect(f.state().phase).toBe('blocked'); expect(f.state().pendingBatch).toBe(pending)
  f.api.batchOutcome.mockResolvedValueOnce(historicalBatch(f)); await f.controller.queryBatch()
  expect(f.state().phase).toBe('result'); expect(f.state().batchHistory).toHaveLength(2); expect(f.api.issueCommand).toHaveBeenCalledOnce(); expect(f.api.appendBatch).toHaveBeenCalledOnce(); f.controller.dispose()
})

it('single-file UNKNOWN remains manually queryable by its original pair after a failed scope GET', async () => {
  const f = fixture(); await open(f); await transfer(f); f.api.append.mockRejectedValueOnce(new NetworkFault()); await f.controller.append(); const pending = f.state().pending
  f.api.materialScopes.mockRejectedValueOnce(new NetworkFault()); await f.controller.loadScopes(0, true)
  expect(f.state().phase).toBe('blocked'); expect(f.state().pending).toBe(pending)
  await f.controller.append(); await f.controller.uploadFile(); await f.controller.delete(row())
  expect(f.api.issueCommand).toHaveBeenCalledOnce(); expect(f.api.append).toHaveBeenCalledOnce(); expect(f.api.deleteMaterial).not.toHaveBeenCalled()
  f.api.materialOutcome.mockResolvedValueOnce({ ...identity('77'), state: 'COMMITTED', scope: target, record: row() }); await f.controller.query()
  expect(f.api.materialOutcome).toHaveBeenCalledExactlyOnceWith(identity('77'), expect.any(AbortSignal)); expect(f.state().phase).toBe('result'); expect(f.state().pending).toBe(pending); expect(f.state().history).toEqual(row()); expect(f.api.issueCommand).toHaveBeenCalledOnce(); expect(f.api.append).toHaveBeenCalledOnce(); f.controller.dispose()
})
it.each([401, 403])('single-file blocked original-query authentication failure %i still revokes all mirrors', async status => {
  const f = fixture(); await open(f); await transfer(f); f.api.append.mockRejectedValueOnce(new NetworkFault()); await f.controller.append()
  f.api.materialScopes.mockRejectedValueOnce(new NetworkFault()); await f.controller.loadScopes(0, true)
  f.api.materialOutcome.mockRejectedValueOnce(new ApiError(status, status === 401 ? 'UNAUTHORIZED' : 'FORBIDDEN')); await f.controller.query()
  expect(f.revoked).toHaveBeenCalledOnce(); expect(f.state().selected).toBeNull(); expect(f.state().pending).toBeNull(); expect(f.controller.hasRiskyDraft()).toBe(false); expect(f.api.issueCommand).toHaveBeenCalledOnce(); expect(f.api.append).toHaveBeenCalledOnce(); f.controller.dispose()
})

describe('bounded fixed-record batch deletion', () => {
  function second(): MaterialRecord { return { ...row(), materialId: '8', resourceVersionId: '9', displayName: '同名课件.txt', addedAt: '2026-10-30T09:00:00', addedSequence: '2', revision: '3' } }
  async function deletionFixture() {
    const f = fixture(), originals = [row(), second()]
    f.api.materials.mockResolvedValue({ ...page(), folderRevision: '3', total: 2, items: originals })
    f.api.deleteBatch.mockImplementation(async (scope, command) => ({ ...identity(command.commandId), scope, records: command.targets.map(t => ({ ...originals.find(r => r.materialId === t.materialId)!, status: 'DELETED', revision: (BigInt(t.expectedMaterialRevision) + 1n).toString() })) }))
    await open(f); return { ...f, originals }
  }
  it('captures selected fixed records in order and submits one batch only after explicit confirmation', async () => {
    const f = await deletionFixture()
    f.controller.toggleDelete(f.originals[1]!, true); f.controller.toggleDelete(f.originals[0]!, true)
    expect(f.state().deleteSelection.map(r => r.materialId)).toEqual(['8', '5'])
    expect(f.api.issueCommand).not.toHaveBeenCalled(); expect(f.controller.hasRiskyDraft()).toBe(true)
    await f.controller.deleteBatch()
    expect(f.api.deleteBatch).toHaveBeenCalledExactlyOnceWith(target, { ...identity('77'), targets: [{ materialId: '8', resourceVersionId: '9', expectedMaterialRevision: '3' }, { materialId: '5', resourceVersionId: '6', expectedMaterialRevision: '1' }] }, 'b'.repeat(64), expect.any(AbortSignal))
    expect(f.state().deleteBatchHistory?.map(r => [r.materialId, r.revision, r.status])).toEqual([['8', '4', 'DELETED'], ['5', '2', 'DELETED']])
    expect(f.state().page?.items).toEqual(f.originals); expect(f.api.deleteMaterial).not.toHaveBeenCalled(); expect(f.api.materials).toHaveBeenCalledOnce()
    await f.controller.deleteBatch(); expect(f.api.issueCommand).toHaveBeenCalledOnce(); f.controller.dispose()
  })
  it('selects at most the current twenty records, can remove one, and requires consent to clear on directory read', async () => {
    const f = fixture(), rows = Array.from({ length: 20 }, (_, i) => ({ ...row(), materialId: String(i + 1), resourceVersionId: String(i + 101), addedSequence: String(20 - i) }))
    f.api.materials.mockResolvedValue({ ...page(), total: 20, items: rows }); await open(f)
    f.controller.selectPageForDeletion(); expect(f.state().deleteSelection).toHaveLength(20)
    f.controller.toggleDelete(rows[4]!, false); expect(f.state().deleteSelection).toHaveLength(19)
    f.controller.toggleDelete({ ...rows[0]!, materialId: '999' }, true); expect(f.state().deleteSelection).toHaveLength(19)
    await f.controller.loadPage(); expect(f.api.materials).toHaveBeenCalledOnce()
    await f.controller.loadPage(null, true); expect(f.state().deleteSelection).toEqual([])
    f.controller.selectPageForDeletion(); f.controller.clearDeleteSelection(); expect(f.controller.hasRiskyDraft()).toBe(false); expect(f.api.deleteBatch).not.toHaveBeenCalled(); f.controller.dispose()
  })
  it('retains unknown deletion and queries the original pair after a current-directory read without replay', async () => {
    const f = await deletionFixture(); f.controller.selectPageForDeletion(); f.api.deleteBatch.mockRejectedValueOnce(new NetworkFault()); await f.controller.deleteBatch()
    const pending = f.state().pendingDeleteBatch; expect(f.state().phase).toBe('unknown')
    await f.controller.loadPage(null, true); expect(f.state().pendingDeleteBatch).toBe(pending); expect(f.state().phase).toBe('unknown'); expect(f.state().deleteBatchHistory).toBeNull()
    f.controller.selectPageForDeletion(); f.controller.pickFiles([new File(['x'], 'x.txt')]); await f.controller.deleteBatch(); await f.controller.delete(row())
    await f.controller.queryDeleteBatch(); expect(f.state().phase).toBe('unknown')
    f.api.deleteBatchOutcome.mockResolvedValueOnce({ ...identity('77'), state: 'COMMITTED', scope: target, records: f.originals.map(r => ({ ...r, status: 'DELETED', revision: (BigInt(r.revision) + 1n).toString() })) })
    await f.controller.queryDeleteBatch(); expect(f.state().phase).toBe('result'); expect(f.state().deleteBatchHistory).toHaveLength(2)
    expect(f.api.deleteBatchOutcome).toHaveBeenLastCalledWith(target, identity('77'), expect.any(AbortSignal)); expect(f.api.issueCommand).toHaveBeenCalledOnce(); expect(f.api.deleteBatch).toHaveBeenCalledOnce(); expect(f.api.deleteMaterial).not.toHaveBeenCalled(); f.controller.dispose()
  })
  it.each(['displayName', 'sha256', 'addedSequence', 'allowedClassIds', 'revision', 'materialId'])('treats changed original %s in a reply as unknown', async field => {
    const f = await deletionFixture(); f.controller.selectPageForDeletion()
    const changed = { ...f.originals[0]!, status: 'DELETED' as const, revision: '2', [field]: field === 'allowedClassIds' ? ['2'] : field === 'sha256' ? 'b'.repeat(64) : field === 'displayName' ? 'wrong.txt' : '99' }
    f.api.deleteBatch.mockResolvedValueOnce({ ...identity('77'), scope: target, records: [changed as MaterialRecord, { ...f.originals[1]!, status: 'DELETED', revision: '4' }] })
    await f.controller.deleteBatch(); expect(f.state().phase).toBe('unknown'); expect(f.state().deleteBatchHistory).toBeNull(); expect(f.state().pendingDeleteBatch?.originals).toEqual(f.originals); f.controller.dispose()
  })
  it.each([401, 403])('clears deletion mirrors on query revocation %i', async status => {
    const f = await deletionFixture(); f.controller.selectPageForDeletion(); f.api.deleteBatch.mockRejectedValueOnce(new NetworkFault()); await f.controller.deleteBatch()
    f.api.deleteBatchOutcome.mockRejectedValueOnce(new ApiError(status, status === 401 ? 'UNAUTHORIZED' : 'FORBIDDEN')); await f.controller.queryDeleteBatch()
    expect(f.revoked).toHaveBeenCalledOnce(); expect(f.state().pendingDeleteBatch).toBeNull(); expect(f.state().deleteSelection).toEqual([]); f.controller.dispose()
  })
})
