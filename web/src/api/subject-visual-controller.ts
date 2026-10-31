import { ApiError, isAborted } from './client'
import { contentPath, sameKey } from './content-protocol'
import { ProtocolFault } from './protocol'
import type { HomeworkTarget, SubjectVisualApi, SubjectVisualPreview } from './subject-visual-protocol'
export interface SubjectVisualState { path: string; phase: 'loading' | 'ready' | 'failed'; preview: SubjectVisualPreview | null; notice: string }
/** One explicit preview read; no polling, palette write, content write or draft. */
export class SubjectVisualController {
  private operation: AbortController | undefined
  private sequence = 0
  private disposed = false
  constructor(private api: SubjectVisualApi, private changed: (s: SubjectVisualState) => void, private revoked: () => void) {}
  async load(target: HomeworkTarget) {
    if (this.disposed) return
    this.operation?.abort(); const controller = new AbortController(); this.operation = controller; const sequence = ++this.sequence
    const path = contentPath(target)
    this.changed({ path, phase: 'loading', preview: null, notice: '' })
    try {
      const preview = await this.api.subjectVisual(target, controller.signal)
      if (this.disposed || controller.signal.aborted || sequence !== this.sequence) return
      if (!sameKey(preview.key, target) || preview.subjectId !== target.subjectId) throw new ProtocolFault()
      this.changed({ path, phase: 'ready', preview, notice: '' })
    } catch (error) {
      if (this.disposed || controller.signal.aborted || sequence !== this.sequence || isAborted(error)) return
      this.changed({ path, phase: 'failed', preview: null, notice: error instanceof ProtocolFault ? error.message : '无法读取学校配色预览。正文草稿不受影响，可主动重新读取。' })
      if (error instanceof ApiError && [401, 403].includes(error.status)) { this.dispose(); this.revoked() }
    }
  }
  dispose() { this.disposed = true; this.operation?.abort(); this.sequence++ }
}
