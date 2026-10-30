import { content, scopes, type ContentKey, type ContentKind } from '../src/api/content-protocol'
export const homeworkKey: ContentKey = { kind: 'HOMEWORK', classId: '1', subjectId: '2', ownerTeacherId: '9', date: '2026-10-30' }
export function wireDocument(target: ContentKey = homeworkKey, revision = '0', text = '') {
  const [year, month, day] = target.date.split('-').map(Number)
  const date = new Date(0); date.setUTCFullYear(year!, month! - 1, day! + 1); date.setUTCHours(0, 0, 0, 0)
  return { schemaVersion: 1, status: 'CONTENT', today: target.date, serverNow: `${target.date}T10:00:00`, expiresAt: `${date.getUTCFullYear().toString().padStart(4, '0')}-${(date.getUTCMonth() + 1).toString().padStart(2, '0')}-${date.getUTCDate().toString().padStart(2, '0')}T00:00:00`, key: target, revision, text, lastActorId: revision === '0' ? null : '9', updatedAt: revision === '0' ? null : `${target.date}T09:59:59` }
}
export const document = (target: ContentKey = homeworkKey, revision = '0', text = '') => content(wireDocument(target, revision, text))
export function wireScopes(kind: ContentKind = 'HOMEWORK', offset = 0, total = 1, identityRevision = '1', today = '2026-10-30') {
  const length = Math.min(20, total - offset)
  const items = Array.from({ length }, (_, i) => kind === 'HOMEWORK' ? { classId: String(offset + i + 1), className: `合成班级 ${offset + i + 1}`, subjectId: '2', subjectName: '合成数学', ownerTeacherId: '9', ownerDisplayName: '合成老师' } : { classId: String(offset + i + 1), className: `合成班级 ${offset + i + 1}` })
  return { schemaVersion: 1, status: 'SCOPES', kind, today, identityRevision, offset, total, nextOffset: offset + length < total ? offset + length : null, items }
}
export const page = (kind: ContentKind = 'HOMEWORK', offset = 0, total = 1, identityRevision = '1', today = '2026-10-30') => scopes(wireScopes(kind, offset, total, identityRevision, today), kind, offset)
