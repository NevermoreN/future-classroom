import { decimal, errorCode, loggedOut, MAX_JSON_BYTES, pairingReceipt, pollResult, ProtocolFault, publicStatus, secret, session, strictJson, validateForm, type ErrorCode, type PairingForm, type PairingReceipt, type PollResult, type PublicStatus, type Session } from './protocol'
export class ApiError extends Error {
  constructor(public readonly status: number, public readonly code: ErrorCode) { super('白板未能完成此次请求。'); this.name = 'ApiError' }
}
export class NetworkFault extends Error {
  constructor() { super('无法确认白板的响应，请检查网络后主动核对。'); this.name = 'NetworkFault' }
}
export const abortError = () => new DOMException('操作已取消', 'AbortError')
export const isAborted = (error: unknown) => error instanceof DOMException && error.name === 'AbortError'
export async function readJson(response: Response, signal: AbortSignal): Promise<unknown> {
  const type = response.headers.get('content-type')?.toLowerCase().replace(/\s/g, '')
  if (type !== 'application/json' && type !== 'application/json;charset=utf-8') throw new ProtocolFault()
  const length = response.headers.get('content-length')
  if (length !== null && (!/^(0|[1-9][0-9]*)$/.test(length) || BigInt(length) > BigInt(MAX_JSON_BYTES))) {
    await response.body?.cancel(); throw new ProtocolFault()
  }
  if (!response.body) throw new ProtocolFault()
  const reader = response.body.getReader(), chunks: Uint8Array[] = []
  let total = 0
  const cancel = () => { void reader.cancel().catch(() => undefined) }
  signal.addEventListener('abort', cancel, { once: true })
  try {
    while (true) {
      if (signal.aborted) throw abortError()
      const { done, value } = await reader.read()
      if (signal.aborted) throw abortError()
      if (done) break
      if (value.byteLength > MAX_JSON_BYTES - total) { await reader.cancel(); throw new ProtocolFault() }
      total += value.byteLength; chunks.push(value)
    }
    if (length !== null && BigInt(total) !== BigInt(length)) throw new ProtocolFault()
    const bytes = new Uint8Array(total); let offset = 0
    for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.byteLength }
    try { return strictJson(new TextDecoder('utf-8', { fatal: true }).decode(bytes)) } catch { throw new ProtocolFault() }
  } finally { signal.removeEventListener('abort', cancel); reader.releaseLock() }
}
/** Static deployment owns this base; user inputs, URL queries and responses cannot
 * select another origin, port, transport, or API path. No Vite proxy in production. */
export function trustedApiBase(base: string, pageUrl: string): URL {
  const page = new URL(pageUrl), url = new URL(base, page)
  if (url.origin !== page.origin || !/^https?:$/.test(url.protocol) || url.search || url.hash
    || url.username || url.password || !url.pathname.endsWith('/') || /[%\\]/.test(url.pathname)) throw new ProtocolFault()
  return new URL('api/v1/', url)
}
export interface ServerApi {
  status(signal: AbortSignal): Promise<PublicStatus>
  session(signal: AbortSignal): Promise<Session>
  create(form: PairingForm, signal: AbortSignal): Promise<PairingReceipt>
  poll(id: string, proof: string, signal: AbortSignal): Promise<PollResult>
  logout(csrf: string, signal: AbortSignal): Promise<void>
}
export class HttpApi implements ServerApi {
  constructor(private readonly base: URL, private readonly fetcher: typeof fetch = globalThis.fetch.bind(globalThis)) {}
  private async request(path: string, signal: AbortSignal, input?: unknown, csrf?: string): Promise<{ status: number; value: unknown }> {
    if (signal.aborted) throw abortError()
    if (this.base.protocol !== 'https:') throw new ProtocolFault('请使用管理员提供的可信 HTTPS 地址。HTTP 预览不会发送配对码或会话凭据。')
    const controller = new AbortController()
    const abort = () => controller.abort()
    signal.addEventListener('abort', abort, { once: true })
    const timeout = setTimeout(abort, 15_000)
    try {
      const headers = new Headers({ Accept: 'application/json' })
      if (input !== undefined) headers.set('Content-Type', 'application/json')
      if (csrf !== undefined) headers.set('X-FC-CSRF', secret(csrf))
      const response = await this.fetcher(new URL(path, this.base), {
        method: input === undefined ? 'GET' : 'POST', credentials: 'same-origin', cache: 'no-store',
        redirect: 'error', referrerPolicy: 'same-origin', headers, signal: controller.signal,
        ...(input === undefined ? {} : { body: JSON.stringify(input) }),
      })
      // A malformed error body cannot keep an unauthorized local session alive.
      if (response.status === 401 || response.status === 403) { await response.body?.cancel(); throw new ApiError(response.status, response.status === 401 ? 'UNAUTHORIZED' : 'FORBIDDEN') }
      const value = await readJson(response, controller.signal)
      if (!response.ok) throw new ApiError(response.status, errorCode(value))
      return { status: response.status, value }
    } catch (error) {
      if (signal.aborted) throw abortError()
      if (error instanceof ApiError || error instanceof ProtocolFault) throw error
      throw new NetworkFault()
    } finally { clearTimeout(timeout); signal.removeEventListener('abort', abort) }
  }
  async status(signal: AbortSignal) { const r = await this.request('status', signal); if (r.status !== 200) throw new ProtocolFault(); return publicStatus(r.value) }
  async session(signal: AbortSignal) { const r = await this.request('session', signal); if (r.status !== 200) throw new ProtocolFault(); return session(r.value) }
  async create(form: PairingForm, signal: AbortSignal) {
    validateForm(form)
    const r = await this.request('pairing/requests', signal, { schemaVersion: 1, code: form.code, loginName: form.loginName, deviceName: form.deviceName })
    if (r.status !== 202) throw new ProtocolFault()
    return pairingReceipt(r.value)
  }
  async poll(id: string, proof: string, signal: AbortSignal) {
    const r = await this.request(`pairing/requests/${decimal(id)}/poll`, signal, { schemaVersion: 1, pollSecret: secret(proof) })
    const result = pollResult(r.value)
    if (r.status !== (result.status === 'PENDING' ? 202 : 200)) throw new ProtocolFault()
    return result
  }
  async logout(csrf: string, signal: AbortSignal) {
    const r = await this.request('session/logout', signal, { schemaVersion: 1 }, csrf)
    if (r.status !== 200) throw new ProtocolFault()
    loggedOut(r.value)
  }
}
