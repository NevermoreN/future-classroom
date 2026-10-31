import { describe, expect, it, vi } from 'vitest'
import { ApiError, HttpApi } from '../src/api/client'
import { ProtocolFault, ProtocolVersionFault } from '../src/api/protocol'
import { SubjectVisualController, type SubjectVisualState } from '../src/api/subject-visual-controller'
import { subjectVisual, previewStyle, type HomeworkTarget, type SubjectVisualPreview } from '../src/api/subject-visual-protocol'
const target: HomeworkTarget = { kind: 'HOMEWORK', classId: '1', subjectId: '2', ownerTeacherId: '3', date: '2026-11-01' }
function wire() { return { schemaVersion: 1, status: 'SUBJECT_VISUAL', key: { ...target }, subjectId: '2', subjectName: '数学', subjectRevision: '18446744073709551615', displayRevision: '4', rulesVersion: 1, effectiveRgb: '#C98282', mode: 'FILLED', homeworkFillPercent: 0, courseFillPercent: 20, homeworkComponentOpacityPercent: 50, courseComponentOpacityPercent: 100, homeworkVisible: true, courseVisible: false } }
function deferred<T>() { let resolve!: (v: T) => void, reject!: (error: unknown) => void; const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no }); return { promise, resolve, reject } }
describe('school subject visual read-only preview', () => {
  it('binds exact homework scope and keeps independent 64-bit revisions; rejects CSS, versions, extra authority and mixed modes', () => {
    expect(subjectVisual(wire(), target).subjectRevision).toBe('18446744073709551615')
    for (const patch of [{ subjectId: '3' }, { key: { ...target, ownerTeacherId: '4' } }, { key: { ...target, date: '2026-11-02' } }, { mode: 'FILLED;url(x)' }, { effectiveRgb: 'red' }, { effectiveRgb: '#c98282' }, { effectiveRgb: 'url(https://x)' }, { homeworkFillPercent: 101 }, { courseComponentOpacityPercent: NaN }, { homeworkVisible: 'true' }, { subjectRevision: '0' }, { displayRevision: '04' }, { palette: [] }]) {
      expect(() => subjectVisual({ ...wire(), ...patch }, target)).toThrow(ProtocolFault)
    }
    for (const name of ['schemaVersion', 'rulesVersion']) {
      expect(() => subjectVisual({ ...wire(), [name]: 2 }, target)).toThrow(ProtocolVersionFault)
      expect(() => subjectVisual({ ...wire(), [name]: '2' }, target)).not.toThrow(ProtocolVersionFault)
      expect(() => subjectVisual({ ...wire(), [name]: '2' }, target)).toThrow(ProtocolFault)
    }
  })
  it('keeps zero fill FILLED without border, separates whole alpha, and restores stored RGB when toggling mode', () => {
    const preview = subjectVisual(wire(), target)
    expect(previewStyle(preview, 'homework')).toEqual({ componentOpacity: 0.5, backgroundColor: 'rgba(201, 130, 130, 0)', border: 'none' })
    expect(previewStyle(preview, 'course')).toEqual({ componentOpacity: 1, backgroundColor: 'rgba(201, 130, 130, 0.2)', border: 'none' })
    const outlined = subjectVisual({ ...wire(), mode: 'OUTLINED' }, target)
    expect(previewStyle(outlined, 'course')).toEqual({ componentOpacity: 1, backgroundColor: 'rgba(201, 130, 130, 0)', border: '1px solid currentColor' })
    expect(outlined.effectiveRgb).toBe(preview.effectiveRgb)
    const invisible = subjectVisual({ ...wire(), homeworkComponentOpacityPercent: 0, homeworkFillPercent: 100 }, target)
    expect(previewStyle(invisible, 'homework')).toEqual({ componentOpacity: 0, backgroundColor: 'rgba(201, 130, 130, 1)', border: 'none' })
  })
  it('rereads unchanged exact scope without keeping an old receipt; aborts and rejects late scope results', async () => {
    const first = deferred<SubjectVisualPreview>(), second = deferred<SubjectVisualPreview>(), third = deferred<SubjectVisualPreview>()
    const api = { subjectVisual: vi.fn().mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise).mockReturnValueOnce(third.promise) }, states: SubjectVisualState[] = [], revoked = vi.fn()
    const owner = new SubjectVisualController(api, state => states.push(state), revoked)
    const a = owner.load(target), b = owner.load(target)
    expect(api.subjectVisual.mock.calls[0]![1].aborted).toBe(true)
    expect(states.at(-1)!.preview).toBeNull()
    second.resolve(subjectVisual(wire(), target)); await b
    first.resolve(subjectVisual({ ...wire(), displayRevision: '1' }, target)); await a
    expect(states.at(-1)!.preview?.displayRevision).toBe('4')
    const other = { ...target, subjectId: '9' }, c = owner.load(other)
    expect(states.at(-1)!.preview).toBeNull()
    owner.dispose(); third.resolve(subjectVisual({ ...wire(), subjectId: '9', key: other }, other)); await c
    expect(states.at(-1)!.phase).toBe('loading'); expect(revoked).not.toHaveBeenCalled()
  })
  it('clears preview on denied current read and never publishes or acts on a late denial after disposal', async () => {
    const pending = deferred<SubjectVisualPreview>(), states: SubjectVisualState[] = [], revoked = vi.fn()
    const api = { subjectVisual: vi.fn().mockRejectedValueOnce(new ApiError(403, 'FORBIDDEN')).mockReturnValueOnce(pending.promise) }
    const owner = new SubjectVisualController(api, s => states.push(s), revoked)
    await owner.load(target)
    expect(states.at(-1)?.preview).toBeNull(); expect(states.at(-1)?.phase).toBe('failed'); expect(revoked).toHaveBeenCalledTimes(1)
    const other = new SubjectVisualController(api, s => states.push(s), revoked), result = other.load(target)
    other.dispose(); pending.reject(new ApiError(401, 'UNAUTHORIZED')); await result
    expect(revoked).toHaveBeenCalledTimes(1)
  })
  it('uses only same-origin GET for the exact key without a mutation payload', async () => {
    const fetcher = vi.fn<typeof fetch>().mockResolvedValue(new Response(JSON.stringify(wire()), { headers: { 'Content-Type': 'application/json' } }))
    const api = new HttpApi(new URL('https://board.school/fc/api/v1/'), fetcher)
    expect((await api.subjectVisual(target, new AbortController().signal)).subjectId).toBe('2')
    const [url, init] = fetcher.mock.calls[0]!
    expect(String(url)).toBe('https://board.school/fc/api/v1/content/homework/1/2/3/2026-11-01/visual')
    expect(init?.method).toBe('GET'); expect(init?.credentials).toBe('same-origin'); expect(init?.body).toBeUndefined()
    expect(new Headers(init?.headers).has('X-FC-CSRF')).toBe(false)
  })
})
