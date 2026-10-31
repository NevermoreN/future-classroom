import { key, revision, sameKey, type ContentKey } from './content-protocol'
import { decimal, ProtocolFault, protocolVersions, unicodeText } from './protocol'
export type HomeworkTarget = Extract<ContentKey, { kind: 'HOMEWORK' }>
export interface SubjectVisualPreview {
  key: HomeworkTarget; subjectId: string; subjectName: string; subjectRevision: string; displayRevision: string
  rulesVersion: 1; effectiveRgb: string; mode: 'FILLED' | 'OUTLINED'
  homeworkFillPercent: number; courseFillPercent: number
  homeworkComponentOpacityPercent: number; courseComponentOpacityPercent: number
  homeworkVisible: boolean; courseVisible: boolean
}
export interface SubjectVisualApi { subjectVisual(target: HomeworkTarget, signal: AbortSignal): Promise<SubjectVisualPreview> }
const fields = ['schemaVersion', 'status', 'key', 'subjectId', 'subjectName', 'subjectRevision', 'displayRevision', 'rulesVersion', 'effectiveRgb', 'mode', 'homeworkFillPercent', 'courseFillPercent', 'homeworkComponentOpacityPercent', 'courseComponentOpacityPercent', 'homeworkVisible', 'courseVisible']
export function subjectVisual(value: unknown, target: HomeworkTarget): SubjectVisualPreview {
  if (typeof value !== 'object' || !value || Array.isArray(value) || Object.keys(value).length !== fields.length || Object.keys(value).some(k => !fields.includes(k))) throw new ProtocolFault()
  const v = value as Record<string, unknown>
  protocolVersions(v.schemaVersion, v.rulesVersion)
  const bound = key(v.key)
  if (v.status !== 'SUBJECT_VISUAL' || bound.kind !== 'HOMEWORK' || !sameKey(bound, target)
    || decimal(v.subjectId) !== bound.subjectId || typeof v.subjectName !== 'string' || !v.subjectName.trim()
    || [...v.subjectName].length > 64 || /[\u0000-\u001f\u007f-\u009f]/u.test(v.subjectName)
    || typeof v.effectiveRgb !== 'string' || !/^#[0-9A-F]{6}$/.test(v.effectiveRgb)
    || !['FILLED', 'OUTLINED'].includes(v.mode as string)
    || ['homeworkFillPercent', 'courseFillPercent', 'homeworkComponentOpacityPercent', 'courseComponentOpacityPercent'].some(k => !Number.isInteger(v[k]) || (v[k] as number) < 0 || (v[k] as number) > 100)
    || typeof v.homeworkVisible !== 'boolean' || typeof v.courseVisible !== 'boolean') throw new ProtocolFault()
  return { key: bound, subjectId: decimal(v.subjectId), subjectName: unicodeText(v.subjectName), subjectRevision: decimal(v.subjectRevision), displayRevision: revision(v.displayRevision), rulesVersion: 1,
    effectiveRgb: v.effectiveRgb, mode: v.mode as 'FILLED' | 'OUTLINED', homeworkFillPercent: v.homeworkFillPercent as number, courseFillPercent: v.courseFillPercent as number,
    homeworkComponentOpacityPercent: v.homeworkComponentOpacityPercent as number, courseComponentOpacityPercent: v.courseComponentOpacityPercent as number,
    homeworkVisible: v.homeworkVisible, courseVisible: v.courseVisible }
}
/** Fill and whole-component alpha are independent. 0% fill remains FILLED;
 * text does not inherit the fill alpha. Only canonical derived RGB is accepted. */
export function previewStyle(preview: SubjectVisualPreview, kind: 'homework' | 'course') {
  const fill = kind === 'homework' ? preview.homeworkFillPercent : preview.courseFillPercent
  const component = kind === 'homework' ? preview.homeworkComponentOpacityPercent : preview.courseComponentOpacityPercent
  const rgb = [1, 3, 5].map(offset => parseInt(preview.effectiveRgb.slice(offset, offset + 2), 16)).join(', ')
  return { componentOpacity: component / 100,
    backgroundColor: `rgba(${rgb}, ${preview.mode === 'FILLED' ? fill / 100 : 0})`,
    border: preview.mode === 'FILLED' ? 'none' : '1px solid currentColor' }
}
