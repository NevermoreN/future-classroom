import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ApiError, NetworkFault, type ServerApi } from '../src/api/client'
import { PairingController, type ViewState } from '../src/api/controller'
import { ProtocolFault, ProtocolVersionFault, publicStatus, type Session } from '../src/api/protocol'
const proof = 'a'.repeat(64), csrf = 'b'.repeat(64)
const actual: Session = { principal: { id: '5', displayName: '白板确认的老师', role: 'TEACHER_ADMIN' }, currentClassId: '8', assignments: [{ classId: '8', subjectId: '9' }], csrf }
function fixture() {
  const api = { status: vi.fn<ServerApi['status']>().mockResolvedValue({ online: true }), session: vi.fn<ServerApi['session']>().mockRejectedValue(new ApiError(401, 'UNAUTHORIZED')), create: vi.fn<ServerApi['create']>().mockResolvedValue({ requestId: '1', pollSecret: proof, retryAfterMs: 500 }), poll: vi.fn<ServerApi['poll']>().mockResolvedValue({ status: 'PENDING', retryAfterMs: 500 }), logout: vi.fn<ServerApi['logout']>().mockResolvedValue(undefined) }
  const states: ViewState[] = [], controller = new PairingController(api, state => states.push(state), () => Date.now())
  return { api, states, controller, last: () => states.at(-1)! }
}
const form = { code: '01234567', loginName: 'client-claimed-name', deviceName: '办公室设备' }
beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(0) })
afterEach(() => { vi.useRealTimers() })
describe('pairing ownership and uncertain outcomes', () => {
  it.each(['schemaVersion', 'apiVersion', 'uiCompatibilityVersion'])('blocks session/pairing before compatible %s and exposes only a typed refresh signal', async key => {
    const f = fixture()
    f.api.status.mockImplementationOnce(async () => publicStatus({ schemaVersion: 1, apiVersion: 1, uiCompatibilityVersion: 1, online: true, [key]: 2 }))
    await f.controller.connect()
    expect(f.last()).toEqual({ phase: 'error', message: new ProtocolVersionFault().message, canPair: false, refreshRequired: true })
    f.controller.cancelPairing(); await f.controller.create(form)
    await vi.advanceTimersByTimeAsync(120_000)
    expect(f.api.status).toHaveBeenCalledOnce(); expect(f.api.session).not.toHaveBeenCalled()
    expect(f.api.create).not.toHaveBeenCalled(); expect(f.api.poll).not.toHaveBeenCalled()
    expect(vi.getTimerCount()).toBe(0); f.controller.dispose()
  })
  it('does not promote malformed versions or message text into a refresh-required error', async () => {
    for (const failure of [new ProtocolFault(new ProtocolVersionFault().message), new Error(new ProtocolVersionFault().message)]) {
      const f = fixture(); f.api.status.mockRejectedValueOnce(failure)
      await f.controller.connect()
      expect(f.last().phase).toBe('error'); expect(f.last()).not.toHaveProperty('refreshRequired')
      await f.controller.create(form)
      expect(f.api.session).not.toHaveBeenCalled(); expect(f.api.create).not.toHaveBeenCalled(); f.controller.dispose()
    }
    const f = fixture()
    f.api.status.mockImplementationOnce(async () => publicStatus({ schemaVersion: 1, apiVersion: '2', uiCompatibilityVersion: 1, online: true }))
    await f.controller.connect()
    expect(f.last().phase).toBe('error'); expect(f.last()).not.toHaveProperty('refreshRequired')
    expect(f.api.session).not.toHaveBeenCalled(); f.controller.dispose()
  })
  it('keeps a version-unknown pairing POST single-shot and requires refresh instead of re-pairing', async () => {
    const f = fixture(); await f.controller.connect(); f.api.create.mockRejectedValueOnce(new ProtocolVersionFault())
    await f.controller.create(form)
    expect(f.last()).toMatchObject({ phase: 'error', canPair: false, refreshRequired: true })
    expect(f.last()).toHaveProperty('message', expect.stringContaining('配对结果尚未确认'))
    expect(f.last()).toHaveProperty('message', expect.stringContaining('不会自动重发'))
    f.controller.cancelPairing(); await f.controller.create(form); await vi.advanceTimersByTimeAsync(120_000)
    expect(f.api.create).toHaveBeenCalledOnce(); expect(f.api.poll).not.toHaveBeenCalled()
    expect(f.api.session).toHaveBeenCalledOnce(); expect(vi.getTimerCount()).toBe(0); f.controller.dispose()
  })
  it('stops polling on a typed version fault without sending another pairing request', async () => {
    const f = fixture(); await f.controller.connect(); f.api.poll.mockRejectedValueOnce(new ProtocolVersionFault())
    const work = f.controller.create(form); await vi.advanceTimersByTimeAsync(500); await work
    expect(f.last()).toMatchObject({ phase: 'error', canPair: false, refreshRequired: true })
    await vi.advanceTimersByTimeAsync(120_000)
    expect(f.api.create).toHaveBeenCalledOnce(); expect(f.api.poll).toHaveBeenCalledOnce()
    expect(f.api.session).toHaveBeenCalledOnce(); expect(JSON.stringify(f.last())).not.toContain(proof)
    expect(vi.getTimerCount()).toBe(0); f.controller.dispose()
  })
  it('does not infer an authenticated view when the granted session has an incompatible version', async () => {
    const f = fixture(); await f.controller.connect()
    f.api.poll.mockResolvedValueOnce({ status: 'AUTHORIZED' }); f.api.session.mockRejectedValueOnce(new ProtocolVersionFault())
    const work = f.controller.create(form); await vi.advanceTimersByTimeAsync(500); await work
    expect(f.last()).toMatchObject({ phase: 'error', canPair: false, refreshRequired: true })
    expect(f.api.session).toHaveBeenCalledTimes(2)
    await vi.advanceTimersByTimeAsync(120_000)
    expect(f.api.create).toHaveBeenCalledOnce(); expect(f.api.poll).toHaveBeenCalledOnce(); f.controller.dispose()
  })
  it('keeps logout result unknown on a version fault, clears the mirror and never resends POST', async () => {
    const f = fixture(); f.api.session.mockResolvedValueOnce(actual); await f.controller.connect()
    f.api.logout.mockRejectedValueOnce(new ProtocolVersionFault()); await f.controller.logout()
    expect(f.last()).toMatchObject({ phase: 'error', canPair: false, refreshRequired: true })
    expect(f.last()).toHaveProperty('message', expect.stringContaining('注销结果尚未确认'))
    expect(f.last()).toHaveProperty('message', expect.stringContaining('不会自动重发'))
    expect(JSON.stringify(f.last())).not.toContain(actual.csrf)
    expect(JSON.stringify(f.last())).not.toContain(actual.principal.displayName)
    await f.controller.logout(); f.controller.cancelPairing(); await f.controller.create(form)
    await vi.advanceTimersByTimeAsync(120_000)
    expect(f.api.logout).toHaveBeenCalledOnce(); expect(f.api.create).not.toHaveBeenCalled()
    expect(f.api.session).toHaveBeenCalledOnce(); expect(f.api.status).toHaveBeenCalledOnce(); f.controller.dispose()
  })
  it('explicit revocation clears the authenticated mirror and stops existing operations', async () => {
    const f = fixture(); f.api.session.mockResolvedValueOnce(actual); await f.controller.connect()
    expect(f.last().phase).toBe('authenticated'); f.controller.invalidate()
    expect(f.last().phase).toBe('anonymous'); expect(JSON.stringify(f.last())).not.toContain(actual.principal.displayName)
    expect(JSON.stringify(f.last())).not.toContain(actual.csrf); f.controller.dispose()
  })
  it('creates once, polls at >=500ms, and learns identity only from GETsession', async () => {
    const f = fixture(); await f.controller.connect()
    const work = f.controller.create(form); await vi.advanceTimersByTimeAsync(0)
    void f.controller.create(form)
    expect(f.api.create).toHaveBeenCalledOnce(); expect(f.api.poll).not.toHaveBeenCalled()
    await vi.advanceTimersByTimeAsync(499); expect(f.api.poll).not.toHaveBeenCalled()
    await vi.advanceTimersByTimeAsync(1); expect(f.api.poll).toHaveBeenCalledTimes(1)
    f.api.poll.mockResolvedValueOnce({ status: 'AUTHORIZED' }); f.api.session.mockResolvedValueOnce(actual)
    await vi.advanceTimersByTimeAsync(500); await work
    expect(f.last()).toEqual({ phase: 'authenticated', session: actual })
    expect(f.api.poll.mock.calls.every(call => call[1] === proof)).toBe(true)
    expect(JSON.stringify(f.states)).not.toContain(proof)
    expect(JSON.stringify(f.states)).not.toContain(form.loginName)
    f.controller.dispose()
  })
  it('unmount cancels a timer and ignores a late create completion', async () => {
    const f = fixture(); await f.controller.connect()
    let complete!: (receipt: { requestId: string; pollSecret: string; retryAfterMs: number }) => void
    f.api.create.mockImplementationOnce(() => new Promise(resolve => { complete = resolve }))
    const work = f.controller.create(form), count = f.states.length
    f.controller.dispose(); expect(f.api.create.mock.calls[0]![1].aborted).toBe(true)
    complete({ requestId: '1', pollSecret: proof, retryAfterMs: 500 }); await work
    await vi.advanceTimersByTimeAsync(2000)
    expect(f.states).toHaveLength(count); expect(f.api.poll).not.toHaveBeenCalled()
    expect(vi.getTimerCount()).toBe(0)
  })
  it('stops pending polling at 120s without recreating an application', async () => {
    const f = fixture(); await f.controller.connect(); const work = f.controller.create(form)
    await vi.advanceTimersByTimeAsync(120_000); await work
    expect(f.last().phase).toBe('expired'); expect(f.api.create).toHaveBeenCalledOnce()
    const count = f.api.poll.mock.calls.length; await vi.advanceTimersByTimeAsync(120_000)
    expect(f.api.poll).toHaveBeenCalledTimes(count); f.controller.dispose()
  })
  it('network-unknown creation never automatically resends or polls', async () => {
    const f = fixture(); await f.controller.connect(); f.api.create.mockRejectedValueOnce(new NetworkFault())
    await f.controller.create(form); await vi.advanceTimersByTimeAsync(120_000)
    expect(f.last().phase).toBe('error'); expect(f.api.create).toHaveBeenCalledOnce(); expect(f.api.poll).not.toHaveBeenCalled(); f.controller.dispose()
  })
  it('auth rejection stops all automatic polling and discards prior server data', async () => {
    const f = fixture(); await f.controller.connect(); f.api.poll.mockRejectedValueOnce(new ApiError(403, 'FORBIDDEN'))
    const work = f.controller.create(form); await vi.advanceTimersByTimeAsync(500); await work
    expect(f.last().phase).toBe('anonymous'); await vi.advanceTimersByTimeAsync(3000)
    expect(f.api.poll).toHaveBeenCalledOnce(); f.controller.dispose()
  })
  it('503 RESULT_UNKNOWN logout is not success, is not resent, and requires explicit safe recheck', async () => {
    const f = fixture(); f.api.session.mockResolvedValueOnce(actual); await f.controller.connect()
    f.api.logout.mockRejectedValueOnce(new ApiError(503, 'RESULT_UNKNOWN'))
    await f.controller.logout(); await vi.advanceTimersByTimeAsync(10000)
    expect(f.last()).toEqual({ phase: 'logout-unknown' }); expect(f.api.logout).toHaveBeenCalledOnce()
    expect(f.api.session).toHaveBeenCalledOnce()
    await f.controller.connect(); expect(f.last().phase).toBe('anonymous')
    expect(f.api.logout).toHaveBeenCalledOnce(); f.controller.dispose()
  })
})
