import { abortError, ApiError, NetworkFault, readJson } from './client'
import { MaterialHttpApi } from './material-client'
import { decimal, errorCode, ProtocolFault, secret } from './protocol'
import * as v from './voice-protocol'

export interface VoiceApi {
  voiceCapabilities(signal: AbortSignal): Promise<v.VoiceCapabilities>
  voiceJobs(signal: AbortSignal): Promise<v.VoiceJob[]>
  uploadVoice(classId: string, wav: Blob, csrf: string, signal: AbortSignal): Promise<v.VoiceUploaded>
  submitVoice(input: v.VoiceSubmit, csrf: string, signal: AbortSignal): Promise<v.VoiceJob>
  voiceJob(id: string, signal: AbortSignal): Promise<v.VoiceJob>
  cancelVoice(id: string, input: v.VoiceCancel, csrf: string, signal: AbortSignal): Promise<v.VoiceJob>
}
export class VoiceHttpApi extends MaterialHttpApi implements VoiceApi {
  async voiceCapabilities(signal: AbortSignal) { const r = await this.request('voice/capabilities', signal); if (r.status !== 200) throw new ProtocolFault(); return v.capabilities(r.value) }
  async voiceJobs(signal: AbortSignal) { const r = await this.request('voice/broadcasts', signal); if (r.status !== 200) throw new ProtocolFault(); return v.jobs(r.value) }
  async uploadVoice(classId: string, wav: Blob, csrf: string, signal: AbortSignal) {
    decimal(classId)
    if (wav.type !== 'audio/wav' || wav.size < 16044 || wav.size > 1920044 || (wav.size - 44) % 2 || this.base.protocol !== 'https:') throw new ProtocolFault()
    if (signal.aborted) throw abortError()
    const operation = new AbortController(), abort = () => operation.abort()
    signal.addEventListener('abort', abort, { once: true }); const timer = setTimeout(abort, 120_000)
    try {
      const response = await this.fetcher(new URL('voice/uploads?classId=' + classId, this.base), { method: 'POST', credentials: 'same-origin', cache: 'no-store', redirect: 'error', referrerPolicy: 'same-origin', headers: { Accept: 'application/json', 'Content-Type': 'audio/wav', 'X-FC-CSRF': secret(csrf) }, body: wav, signal: operation.signal })
      if (response.status === 401 || response.status === 403) { await response.body?.cancel().catch(() => undefined); throw new ApiError(response.status, response.status === 401 ? 'UNAUTHORIZED' : 'FORBIDDEN') }
      const value = await readJson(response, operation.signal)
      if (!response.ok) throw new ApiError(response.status, errorCode(value))
      if (response.status !== 201) throw new ProtocolFault()
      return v.uploaded(value, classId, wav.size)
    } catch (error) {
      if (signal.aborted) throw abortError()
      if (error instanceof ApiError || error instanceof ProtocolFault) throw error
      throw new NetworkFault()
    } finally { clearTimeout(timer); signal.removeEventListener('abort', abort) }
  }
  async submitVoice(input: v.VoiceSubmit, csrf: string, signal: AbortSignal) { const r = await this.request('voice/broadcasts', signal, v.submitInput(input), csrf); if (r.status !== 200) throw new ProtocolFault(); return v.committed(r.value, input) }
  async voiceJob(id: string, signal: AbortSignal) { const r = await this.request('voice/broadcasts/' + decimal(id), signal); if (r.status !== 200) throw new ProtocolFault(); return v.currentJob(r.value, id) }
  async cancelVoice(id: string, input: v.VoiceCancel, csrf: string, signal: AbortSignal) { const r = await this.request('voice/broadcasts/' + decimal(id) + '/cancel', signal, v.cancelInput(input), csrf); if (r.status !== 200) throw new ProtocolFault(); return v.cancelled(r.value, id, input) }
}
