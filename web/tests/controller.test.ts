import { afterEach, describe, expect, it, vi } from 'vitest'
import { ApiError, NetworkFault, setPageToken, type ServerApi } from '../src/api/client'
import { SessionController, type PageCredentialStore, type ViewState } from '../src/api/controller'
import { ProtocolVersionFault, type PageSession } from '../src/api/protocol'
const proof = 'a'.repeat(128), csrf = 'b'.repeat(64)
const actual: PageSession = { principal: { id: '5', displayName: '白板确认的老师', role: 'TEACHER_ADMIN' }, currentClassId: '8', assignments: [{ classId: '8', subjectId: '9' }], csrf, pageToken: proof }
function fixture(initial?: string) {
  let token = initial
  const credentials: PageCredentialStore = { load: () => token, save: value => { token = value }, clear: () => { token = undefined } }
  const api = { setup: vi.fn<ServerApi['setup']>().mockResolvedValue(actual), status: vi.fn<ServerApi['status']>().mockResolvedValue({ online: true }), session: vi.fn<ServerApi['session']>().mockResolvedValue(actual), create: vi.fn<ServerApi['create']>(), poll: vi.fn<ServerApi['poll']>(), login: vi.fn<ServerApi['login']>().mockResolvedValue(actual), resume: vi.fn<ServerApi['resume']>().mockResolvedValue(actual), changePassword: vi.fn<ServerApi['changePassword']>().mockResolvedValue(actual), logout: vi.fn<ServerApi['logout']>().mockResolvedValue(undefined) }
  const states: ViewState[] = [], controller = new SessionController(api, state => states.push(state), credentials)
  return { api, credentials, states, controller, last: () => states.at(-1)!, token: () => token }
}
const form = { loginName: 'teacher', password: 'password123' }
afterEach(() => setPageToken(undefined))
describe('page-scoped username and password login', () => {
  it('shows local first setup instead of login when the installation is empty', async () => {
    const f = fixture(); f.api.status.mockResolvedValueOnce({ online: true, initialized: false }); await f.controller.connect()
    expect(f.last().phase).toBe('setup'); expect(f.api.resume).not.toHaveBeenCalled()
    await f.controller.setup({ schoolName: '测试学校', loginName: 'admin', displayName: '管理员', password: 'initial-password' })
    expect(f.api.setup).toHaveBeenCalledOnce(); expect(f.last().phase).toBe('authenticated'); expect(f.token()).toBe(proof); f.controller.dispose()
  })
  it('keeps an initialization denial on the setup screen and directs the user to the local machine', async () => {
    const f = fixture(); f.api.status.mockResolvedValueOnce({ online: true, initialized: false }); await f.controller.connect(); f.api.setup.mockRejectedValueOnce(new ApiError(403, 'FORBIDDEN'))
    await f.controller.setup({ schoolName: '测试学校', loginName: 'admin', displayName: '管理员', password: 'initial-password' })
    expect(f.last()).toMatchObject({ phase: 'setup', notice: expect.stringContaining('白板本机') }); expect(f.token()).toBeUndefined(); f.controller.dispose()
  })
  it('does not inherit an existing cookie session in a new tab', async () => {
    const f = fixture(); await f.controller.connect()
    expect(f.last()).toEqual({ phase: 'anonymous' }); expect(f.api.session).not.toHaveBeenCalled(); expect(f.api.resume).not.toHaveBeenCalled()
    await f.controller.login(form)
    expect(f.api.login).toHaveBeenCalledExactlyOnceWith(form, expect.any(AbortSignal))
    expect(f.last()).toMatchObject({ phase: 'authenticated', session: { principal: actual.principal } })
    expect(JSON.stringify(f.last())).not.toContain(proof); expect(f.token()).toBe(proof); f.controller.dispose()
  })
  it('refreshes and reconnects using the page credential without another password submission', async () => {
    const f = fixture(proof); await f.controller.connect(); await f.controller.connect()
    expect(f.api.resume).toHaveBeenCalledTimes(2); expect(f.api.login).not.toHaveBeenCalled(); expect(f.last().phase).toBe('authenticated'); f.controller.dispose()
    const refreshed = new SessionController(f.api, state => f.states.push(state), f.credentials)
    await refreshed.connect(); expect(f.api.resume).toHaveBeenCalledTimes(3); expect(f.last().phase).toBe('authenticated'); refreshed.dispose()
  })
  it('preserves credentials across network failure instead of logging out', async () => {
    const f = fixture(proof); await f.controller.connect(); f.api.resume.mockRejectedValueOnce(new NetworkFault()); await f.controller.connect()
    expect(f.last()).toMatchObject({ phase: 'authenticated', notice: expect.stringContaining('登录已保留') }); expect(f.token()).toBe(proof)
    await f.controller.connect(); expect(f.last().phase).toBe('authenticated'); expect(f.api.login).not.toHaveBeenCalled(); f.controller.dispose()
  })
  it('preserves refresh recovery during initial network failure', async () => {
    const f = fixture(proof); f.api.status.mockRejectedValueOnce(new NetworkFault()); await f.controller.connect()
    expect(f.last().phase).toBe('error'); expect(f.token()).toBe(proof); await f.controller.connect()
    expect(f.last().phase).toBe('authenticated'); f.controller.dispose()
  })
  it('clears revoked credentials and returns to login', async () => {
    const f = fixture(proof); f.api.resume.mockRejectedValueOnce(new ApiError(401, 'UNAUTHORIZED')); await f.controller.connect()
    expect(f.last().phase).toBe('anonymous'); expect(f.token()).toBeUndefined(); f.controller.dispose()
  })
  it('blocks login on incompatible server versions', async () => {
    const f = fixture(); f.api.status.mockRejectedValueOnce(new ProtocolVersionFault()); await f.controller.connect(); await f.controller.login(form)
    expect(f.last()).toMatchObject({ phase: 'error', refreshRequired: true }); expect(f.api.login).not.toHaveBeenCalled(); f.controller.dispose()
  })
  it('keeps wrong credentials out of stored state and permits corrected login', async () => {
    const f = fixture(); await f.controller.connect(); f.api.login.mockRejectedValueOnce(new ApiError(401, 'UNAUTHORIZED')); await f.controller.login(form)
    expect(f.last()).toMatchObject({ phase: 'anonymous', notice: expect.stringContaining('用户名或密码') }); expect(f.token()).toBeUndefined()
    expect(JSON.stringify(f.last())).not.toContain(form.password); await f.controller.login(form); expect(f.last().phase).toBe('authenticated'); f.controller.dispose()
  })
  it('drops local recovery on uncertain logout and does not resend logout', async () => {
    const f = fixture(proof); await f.controller.connect(); f.api.logout.mockRejectedValueOnce(new NetworkFault()); await f.controller.logout()
    expect(f.last().phase).toBe('logout-unknown'); expect(f.token()).toBeUndefined(); await f.controller.connect()
    expect(f.last().phase).toBe('anonymous'); expect(f.api.logout).toHaveBeenCalledTimes(1); expect(f.api.resume).toHaveBeenCalledTimes(1); f.controller.dispose()
  })
  it('does not clear a valid page credential on a forbidden resume response', async () => {
    const f = fixture(proof); await f.controller.connect(); f.api.resume.mockRejectedValueOnce(new ApiError(403, 'FORBIDDEN')); await f.controller.connect()
    expect(f.last().phase).toBe('authenticated'); expect(f.token()).toBe(proof); f.controller.dispose()
  })
  it('keeps first-login sessions restricted until password change succeeds with a new page credential', async () => {
    const f = fixture(); f.api.login.mockResolvedValueOnce({ ...actual, mustChangePassword: true }); await f.controller.connect(); await f.controller.login(form)
    expect(f.last()).toMatchObject({ phase: 'authenticated', session: { mustChangePassword: true } })
    f.controller.cancelPasswordChange(); expect(f.last()).toHaveProperty('session.mustChangePassword', true)
    const nextToken = 'c'.repeat(128)
    f.api.changePassword.mockResolvedValueOnce({ ...actual, pageToken: nextToken, mustChangePassword: false })
    await f.controller.changePassword({ currentPassword: 'initial-password', newPassword: 'updated-password' })
    expect(f.api.changePassword).toHaveBeenCalledExactlyOnceWith({ currentPassword: 'initial-password', newPassword: 'updated-password' }, csrf, expect.any(AbortSignal))
    expect(f.last()).toMatchObject({ phase: 'authenticated', session: { mustChangePassword: false } }); expect(f.token()).toBe(nextToken); f.controller.dispose()
  })
  it('opens voluntary password change and preserves login on incorrect current password', async () => {
    const f = fixture(proof); await f.controller.connect(); f.controller.showPasswordChange()
    expect(f.last()).toMatchObject({ phase: 'authenticated', passwordChange: true })
    f.api.changePassword.mockRejectedValueOnce(new ApiError(403, 'FORBIDDEN'))
    await f.controller.changePassword({ currentPassword: 'wrong-password', newPassword: 'updated-password' })
    expect(f.last()).toMatchObject({ phase: 'authenticated', passwordChange: true, passwordNotice: expect.stringContaining('当前密码') }); expect(f.token()).toBe(proof)
    f.controller.cancelPasswordChange(); expect(f.last()).not.toHaveProperty('passwordChange'); f.controller.dispose()
  })
  it('does not publish late login after invalidation or disposal', async () => {
    for (const stop of ['invalidate', 'dispose'] as const) {
      const f = fixture(); await f.controller.connect(); let finish!: (value: PageSession) => void
      f.api.login.mockImplementationOnce(() => new Promise(resolve => { finish = resolve }))
      const pending = f.controller.login(form); f.controller[stop](); finish(actual); await pending
      expect(f.last().phase).not.toBe('authenticated'); expect(f.token()).toBeUndefined(); f.controller.dispose()
    }
  })
})
