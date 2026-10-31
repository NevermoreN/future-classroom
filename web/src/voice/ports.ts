export type RecorderErrorCode = 'INSECURE_CONTEXT' | 'RECORDING_UNSUPPORTED' | 'MIC_DENIED' |
  'MIC_INTERRUPTED' | 'DURATION_OUT_OF_RANGE' | 'RAW_LIMIT' | 'INVALID_PCM' | 'RECORDING_FAILED'

const messages: Record<RecorderErrorCode, string> = {
  INSECURE_CONTEXT: '录音需要可信 HTTPS 连接。',
  RECORDING_UNSUPPORTED: '当前浏览器不支持录音，请使用已验证浏览器。',
  MIC_DENIED: '未获得麦克风权限，请允许后重新按住录音。',
  MIC_INTERRUPTED: '麦克风或页面已中断，本次录音已取消。',
  DURATION_OUT_OF_RANGE: '录音时长超出白板允许范围，请核对当前上限后重新录音。',
  RAW_LIMIT: '录音缓存已达上限，本次录音已取消。',
  INVALID_PCM: '录音数据无效，请重新录音。',
  RECORDING_FAILED: '录音处理失败，请重新录音。',
}

export class RecorderError extends Error {
  constructor(readonly code: RecorderErrorCode) { super(messages[code]); this.name = 'RecorderError' }
}

export interface CaptureSink {
  pcm(chunk: Float32Array, sampleRate: number): void
  interrupted(): void
  limit(): void
}
export interface CaptureSession {
  readonly sampleRate: number
  start(): Promise<void>
  /** Immediately releases microphone tracks; resolves after the worklet's final chunk. */
  stop(): Promise<void>
  /** Immediate cancellation, without flushing or sending audio. Idempotent. */
  abort(): void
}
export interface CapturePort {
  open(sink: CaptureSink, signal: AbortSignal, maxDurationMillis?: number): Promise<CaptureSession>
}
export interface NormalizedWav { wav: Blob; frames: number; sampleRate: 16000 }
export interface WavNormalizer {
  normalize(chunks: Float32Array[], sampleRate: number, signal: AbortSignal): Promise<NormalizedWav>
}
export interface RecorderClock {
  setTimeout(callback: () => void, milliseconds: number): unknown
  clearTimeout(timer: unknown): void
}
