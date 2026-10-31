import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ApiError, NetworkFault, type ContentApi } from '../src/api/client'
import { ProtocolVersionFault } from '../src/api/protocol'
import { ContentController, type ContentState } from '../src/api/content-controller'
import { content, normalizedBodyText, type ContentDocument } from '../src/api/content-protocol'
import { document, homeworkKey, page, wireDocument } from './content-fixtures'
function fixture() {
  let id = 76
  const api = { scopes: vi.fn<ContentApi['scopes']>().mockResolvedValue(page()), content: vi.fn<ContentApi['content']>().mockResolvedValue(document()), saveContent: vi.fn<ContentApi['saveContent']>().mockImplementation(async (key, command) => ({ commandId: command.commandId, content: document(key, (BigInt(command.expectedRevision) + 1n).toString(), normalizedBodyText(command.text)) })), commandResult: vi.fn<ContentApi['commandResult']>().mockResolvedValue({ commandId: '77', state: 'NOT_FOUND', content: null }) }
  const states: ContentState[] = [], revoked = vi.fn(), controller = new ContentController(api, 'b'.repeat(64), state => states.push(state), revoked, () => String(++id))
  return { api, controller, states, revoked, state: () => states.at(-1)!, editor: () => states.at(-1)!.editor! }
}
async function open(f: ReturnType<typeof fixture>) { await f.controller.loadScopes('HOMEWORK'); await f.controller.select(f.state().page!.items[0]!) }
beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(new Date('2040-12-29T00:00:00Z')) })
afterEach(() => { vi.useRealTimers() })
describe('content draft and command lifecycle', () => {
  it('rejects duplicated/rewound continuation scope keys and requires page reload', async () => {
    const f = fixture(); f.api.scopes.mockResolvedValueOnce(page('HOMEWORK', 0, 21)); await f.controller.loadScopes('HOMEWORK')
    const bad = page('HOMEWORK', 20, 21); bad.items = [page().items[0]!]; f.api.scopes.mockResolvedValueOnce(bad)
    await f.controller.loadScopes('HOMEWORK', 20)
    expect(f.state().pageBlocked).toBe(true); expect(f.state().page?.offset).toBe(0); f.controller.dispose()
  })
  it('partial-page previous navigation uses observed offsets rather than guessing twenty', async () => {
    const f = fixture(); const first = page('HOMEWORK', 0, 6); first.items = first.items.slice(0, 3); first.nextOffset = 3
    f.api.scopes.mockResolvedValueOnce(first); await f.controller.loadScopes('HOMEWORK')
    f.api.scopes.mockResolvedValueOnce(page('HOMEWORK', 3, 6)); await f.controller.loadScopes('HOMEWORK', 3)
    expect(f.state().previousOffset).toBe(0); f.controller.dispose()
  })
  it('a wrong content key blocks an old editor until explicit reload', async () => {
    const f = fixture(); await open(f); f.api.content.mockResolvedValueOnce(document({ ...homeworkKey, classId: '2' }))
    await f.controller.select(f.state().page!.items[0]!); expect(f.state().pageBlocked).toBe(true)
    expect(f.editor().phase).toBe('scope-changed'); await f.controller.save(); expect(f.api.saveContent).not.toHaveBeenCalled(); f.controller.dispose()
  })
  it('keeps raw Unicode whitespace for an unknown command and validates same-ID query normalization', async () => {
    const f = fixture(); await open(f); const raw = ' \t\n　\ufeff正文\ufeff　\r '
    f.controller.edit(raw); f.api.saveContent.mockRejectedValueOnce(new NetworkFault()); await f.controller.save()
    expect(f.editor().pending?.command.text).toBe(raw)
    f.api.commandResult.mockResolvedValueOnce({ commandId: '77', state: 'COMMITTED', content: document(homeworkKey, '1', '\ufeff正文\ufeff') }); await f.controller.query()
    expect(f.editor().phase).toBe('result'); expect(f.editor().pending?.command.text).toBe(raw)
    expect(f.editor().history?.text).toBe('\ufeff正文\ufeff'); expect(f.api.saveContent).toHaveBeenCalledOnce(); f.controller.dispose()
  })
  it('pure allowed Unicode whitespace is a clear and preserves positive revision', async () => {
    const f = fixture(); await open(f); f.controller.edit(' \t\n　 '); await f.controller.save()
    expect(f.api.saveContent.mock.calls[0]![1].text).toBe(' \t\n　 ')
    expect(f.editor().loaded.text).toBe(''); expect(f.editor().loaded.revision).toBe('1'); f.controller.dispose()
  })
  it('saves once with captured CAS, and clear preserves the new nonzero revision', async () => {
    const f = fixture(); f.api.content.mockResolvedValueOnce(document(homeworkKey, '7', '原正文')); await open(f)
    f.controller.edit('  新正文  '); await f.controller.save()
    expect(f.api.saveContent.mock.calls[0]![1]).toEqual({ commandId: '77', expectedRevision: '7', text: '  新正文  ' })
    expect(f.editor().draft).toBe('新正文'); expect(f.editor().loaded.revision).toBe('8'); expect(f.controller.hasRiskyDraft()).toBe(false)
    await f.controller.save(true)
    expect(f.api.saveContent.mock.calls[1]![1]).toEqual({ commandId: '78', expectedRevision: '8', text: '' })
    expect(f.editor().loaded.revision).toBe('9'); expect(f.editor().loaded.text).toBe(''); expect(f.editor().notice).toContain('不代表')
    f.controller.dispose()
  })
  it('unknown preserves exact original payload and NOT_FOUND never causes resubmission', async () => {
    const f = fixture(); await open(f); f.controller.edit('原始草稿'); f.api.saveContent.mockRejectedValueOnce(new ApiError(503, 'RESULT_UNKNOWN'))
    await f.controller.save(); const original = f.editor().pending
    expect(f.editor().phase).toBe('unknown'); f.controller.edit('偷偷替换'); expect(f.editor().draft).toBe('原始草稿')
    await f.controller.query(); await vi.advanceTimersByTimeAsync(2000)
    expect(f.editor().phase).toBe('unknown'); expect(f.editor().notice).toContain('不证明'); expect(f.editor().pending).toBe(original)
    expect(f.api.saveContent).toHaveBeenCalledOnce(); expect(f.api.commandResult).toHaveBeenCalledWith('77', expect.any(AbortSignal))
    f.controller.dispose()
  })
  it('shows command COMMITTED as historical and requires latest GET plus explicit merge before new command', async () => {
    const f = fixture(); await open(f); f.controller.edit('自己的草稿'); f.api.saveContent.mockRejectedValueOnce(new NetworkFault()); await f.controller.save()
    f.api.commandResult.mockResolvedValueOnce({ commandId: '77', state: 'COMMITTED', content: document(homeworkKey, '1', '自己的草稿') }); await f.controller.query()
    expect(f.editor().history?.revision).toBe('1'); expect(f.editor().loaded.revision).toBe('0'); expect(f.editor().notice).toContain('不代表当前最新')
    await f.controller.save(); expect(f.api.saveContent).toHaveBeenCalledOnce()
    f.api.content.mockResolvedValueOnce(document(homeworkKey, '2', '其他人更新')); await f.controller.loadLatest()
    expect(f.editor().latest?.revision).toBe('2'); expect(f.editor().draft).toBe('自己的草稿')
    await f.controller.save(); expect(f.api.saveContent).toHaveBeenCalledOnce()
    f.controller.acknowledgeMerge(false); await f.controller.save()
    expect(f.api.saveContent.mock.calls[1]![1]).toEqual({ commandId: '78', expectedRevision: '2', text: '自己的草稿' }); f.controller.dispose()
  })
  it('conflict keeps draft and cannot overwrite before fresh GET and explicit confirmation', async () => {
    const f = fixture(); await open(f); f.controller.edit('冲突草稿'); f.api.saveContent.mockRejectedValueOnce(new ApiError(409, 'REVISION_CONFLICT')); await f.controller.save()
    expect(f.editor().phase).toBe('conflict'); expect(f.editor().draft).toBe('冲突草稿')
    await f.controller.save(); expect(f.api.saveContent).toHaveBeenCalledOnce()
    f.api.content.mockResolvedValueOnce(document(homeworkKey, '3', '服务器正文')); await f.controller.loadLatest()
    expect(f.editor().phase).toBe('merge'); f.controller.acknowledgeMerge(true)
    expect(f.editor().draft).toBe('服务器正文'); expect(f.editor().loaded.revision).toBe('3'); expect(f.api.saveContent).toHaveBeenCalledOnce(); f.controller.dispose()
  })
  it('a foreign receipt or wrong revision cannot resolve the original unknown command', async () => {
    const f = fixture(); await open(f); f.controller.edit('原草稿'); f.api.saveContent.mockRejectedValueOnce(new NetworkFault()); await f.controller.save()
    f.api.commandResult.mockResolvedValueOnce({ commandId: '77', state: 'COMMITTED', content: document({ ...homeworkKey, classId: '2' }, '1', '他人') }); await f.controller.query()
    expect(f.editor().phase).toBe('unknown'); expect(f.editor().history).toBeNull(); expect(f.editor().pending?.command.commandId).toBe('77'); expect(f.api.saveContent).toHaveBeenCalledOnce(); f.controller.dispose()
  })
  it('DAY_CHANGED preserves old draft/date, and explicit new-day selection starts from new server content', async () => {
    const f = fixture(); await open(f); f.controller.edit('昨日未保存'); f.api.saveContent.mockRejectedValueOnce(new ApiError(409, 'DAY_CHANGED')); await f.controller.save()
    expect(f.editor().expired).toBe(true); expect(f.editor().loaded.key.date).toBe('2026-10-30'); expect(f.editor().draft).toBe('昨日未保存')
    await f.controller.save(); expect(f.api.saveContent).toHaveBeenCalledOnce()
    f.api.scopes.mockResolvedValueOnce(page('HOMEWORK', 0, 1, '1', '2026-10-31')); await f.controller.loadScopes('HOMEWORK'); expect(f.api.scopes).toHaveBeenCalledOnce()
    await f.controller.loadScopes('HOMEWORK', 0, true); expect(f.state().editor).toBeNull()
    f.api.content.mockResolvedValueOnce(document({ ...homeworkKey, date: '2026-10-31' })); await f.controller.select(f.state().page!.items[0]!)
    expect(f.editor().draft).toBe(''); expect(f.editor().loaded.key.date).toBe('2026-10-31'); f.controller.dispose()
  })
  it('server midnight monotonic timer expires regardless of teacher wall date and never auto-submits', async () => {
    const f = fixture(); f.api.content.mockResolvedValueOnce(content({ ...wireDocument(), serverNow: '2026-10-30T23:59:59' })); await open(f); f.controller.edit('午夜草稿')
    await vi.advanceTimersByTimeAsync(1000); expect(f.editor().expired).toBe(true); expect(f.editor().draft).toBe('午夜草稿')
    await f.controller.save(); expect(f.api.saveContent).not.toHaveBeenCalled(); f.controller.dispose(); expect(vi.getTimerCount()).toBe(0)
  })
  it.each([401, 403])('HTTP %i clears all draft/command state and revokes the pairing controller mirror', async status => {
    const f = fixture(); await open(f); f.controller.edit('敏感草稿'); f.api.saveContent.mockRejectedValueOnce(new ApiError(status, status === 401 ? 'UNAUTHORIZED' : 'FORBIDDEN')); await f.controller.save()
    expect(f.state().editor).toBeNull(); expect(f.state().page).toBeNull(); expect(f.revoked).toHaveBeenCalledOnce(); expect(JSON.stringify(f.state())).not.toContain('敏感草稿'); f.controller.dispose()
  })
  it('identity revision change invalidates pagination and blocks old target with reviewable draft', async () => {
    const f = fixture(); f.api.scopes.mockResolvedValueOnce(page('HOMEWORK', 0, 21, '1')); await open(f); f.controller.edit('范围变更草稿')
    await f.controller.loadScopes('HOMEWORK', 20); expect(f.api.scopes).toHaveBeenCalledOnce()
    f.api.scopes.mockResolvedValueOnce(page('HOMEWORK', 20, 21, '2')); await f.controller.loadScopes('HOMEWORK', 20, true)
    expect(f.state().pageBlocked).toBe(true); expect(f.editor().phase).toBe('scope-changed'); expect(f.editor().draft).toBe('范围变更草稿')
    await f.controller.save(); expect(f.api.saveContent).not.toHaveBeenCalled(); f.controller.dispose()
  })
  it('rejects switches without explicit discard and ignores late aborted responses', async () => {
    const f = fixture(); await open(f); f.controller.edit('未保存')
    expect(f.controller.leave()).toBe(false); await f.controller.loadScopes('REMINDER'); expect(f.api.scopes).toHaveBeenCalledOnce()
    expect(f.controller.leave(true)).toBe(true)
    let complete!: (value: ContentDocument) => void
    f.api.content.mockImplementationOnce(() => new Promise(resolve => { complete = resolve }))
    const old = f.controller.select(f.state().page!.items[0]!); f.controller.dispose(); complete(document()); await old
    expect(f.api.content.mock.calls.at(-1)![1].aborted).toBe(true); expect(vi.getTimerCount()).toBe(0)
  })
})

describe('explicit version recovery preserves content commands', () => {
  it('keeps the original save and draft, blocks new writes, and only queries the original ID manually', async () => {
    const f = fixture(), fault = new ProtocolVersionFault(); await open(f); f.controller.edit('版本升级前的草稿')
    f.api.saveContent.mockRejectedValueOnce(fault); await f.controller.save()
    const original = f.editor().pending
    expect(original?.command.commandId).toBe('77'); expect(f.editor().draft).toBe('版本升级前的草稿')
    expect(f.editor().notice).toContain(fault.message); expect(f.state().notice).toContain('刷新页面'); expect(f.editor().phase).toBe('unknown')
    expect(vi.getTimerCount()).toBe(0); expect(f.controller.leave()).toBe(false)
    f.controller.edit('替换草稿'); await f.controller.save(); await f.controller.save(true); await f.controller.loadLatest(); await f.controller.loadScopes('HOMEWORK', 0, true)
    expect(f.editor().pending).toBe(original); expect(f.editor().draft).toBe('版本升级前的草稿'); expect(f.api.saveContent).toHaveBeenCalledOnce(); expect(f.api.scopes).toHaveBeenCalledOnce(); expect(f.api.content).toHaveBeenCalledOnce()
    await vi.advanceTimersByTimeAsync(60000); expect(f.api.commandResult).not.toHaveBeenCalled()
    await f.controller.query(); expect(f.editor().pending).toBe(original); expect(f.editor().notice).toContain('不证明'); expect(f.editor().notice).toContain(fault.message)
    f.api.commandResult.mockResolvedValueOnce({ commandId: '77', state: 'COMMITTED', content: document(homeworkKey, '1', '版本升级前的草稿') }); await f.controller.query()
    expect(f.editor().history?.text).toBe('版本升级前的草稿'); expect(f.editor().pending).toBe(original); expect(f.editor().notice).toContain(fault.message)
    expect(f.api.commandResult.mock.calls.map(call => call[0])).toEqual(['77', '77']); await f.controller.save(); expect(f.api.saveContent).toHaveBeenCalledOnce(); f.controller.dispose()
  })
  it('a version fault during a read retains the local draft and never treats a reload as recovery', async () => {
    const f = fixture(); await open(f); f.controller.edit('未提交的本地正文'); const loaded = f.editor().loaded
    f.api.content.mockRejectedValueOnce(new ProtocolVersionFault()); await f.controller.loadLatest()
    expect(f.editor().loaded).toBe(loaded); expect(f.editor().draft).toBe('未提交的本地正文'); expect(f.editor().pending).toBeNull(); expect(f.editor().notice).toContain('管理员升级')
    await f.controller.loadScopes('REMINDER', 0, true); await f.controller.save(); expect(f.api.scopes).toHaveBeenCalledOnce(); expect(f.api.saveContent).not.toHaveBeenCalled(); expect(f.controller.hasRiskyDraft()).toBe(true); f.controller.dispose()
  })
  it('a typed version failure of an explicit unknown-result query keeps its old command ID', async () => {
    const f = fixture(); await open(f); f.controller.edit('等待核对'); f.api.saveContent.mockRejectedValueOnce(new NetworkFault()); await f.controller.save(); const original = f.editor().pending
    f.api.commandResult.mockRejectedValueOnce(new ProtocolVersionFault()); await f.controller.query()
    expect(f.editor().pending).toBe(original); expect(f.editor().notice).toContain('刷新页面'); await f.controller.query(); expect(f.api.commandResult.mock.calls.map(call => call[0])).toEqual(['77', '77']); expect(f.api.saveContent).toHaveBeenCalledOnce(); f.controller.dispose()
  })
})
