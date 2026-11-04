import { useEffect, useRef, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Section } from './AdminForms'
import { AdminHttpApi, type DisplayData, type LayoutData } from './api/admin-client'
import { ApiError, isAborted, trustedApiBase } from './api/client'
import { PreviewHttpApi, type PreviewGeometry } from './api/preview-client'
import type { ProfileKey, SubjectProfileData } from './api/profile-admin-client'
import type { Session } from './api/protocol'
export default function ServerLayoutPreview({ session, onRevoked, layout, revision, display, profileKey, materials }: { session: Session; onRevoked: () => void; layout?: LayoutData; revision?: string; display?: DisplayData; profileKey?: ProfileKey; materials?: SubjectProfileData['materials'] }) {
  const [api] = useState(() => new PreviewHttpApi(trustedApiBase(import.meta.env.BASE_URL, window.location.href))), [admin] = useState(() => new AdminHttpApi(trustedApiBase(import.meta.env.BASE_URL, window.location.href)))
  const [geometry, setGeometry] = useState<PreviewGeometry | null>(null), [busy, setBusy] = useState(false), [notice, setNotice] = useState(''), [rendered, setRendered] = useState('')
  const operation = useRef<AbortController | null>(null), input = JSON.stringify({ layout, revision, display, profileKey, materials })
  useEffect(() => () => operation.current?.abort(), [])
  async function refresh() {
    if (busy) return
    const controller = new AbortController(); operation.current = controller; setBusy(true); setNotice('')
    try {
      const current = layout ? null : await admin.read('layout', controller.signal)
      const result = await api.preview({ layout: layout ?? current!.data, expectedRevision: revision ?? current!.revision, key: profileKey, display, materials }, session.csrf, controller.signal)
      if (!controller.signal.aborted) { setGeometry(result); setRendered(input) }
    } catch (error) { if (!isAborted(error)) { if (error instanceof ApiError && error.status === 401) onRevoked(); else setNotice(error instanceof ApiError && error.status === 409 ? '桌面配置已变化，请重新读取后更新预览。' : '无法生成预览，请检查设置和连接。') } }
    finally { if (!controller.signal.aborted) setBusy(false) }
  }
  return <Section title="桌面预览"><div className="admin-action-row"><Button type="button" variant="outline" disabled={busy} onClick={() => { void refresh() }}>{busy ? '正在生成…' : '更新桌面预览'}</Button><span role="status">{notice || (geometry ? rendered === input ? '当前设置的预览' : '设置已修改，请更新预览' : '点击更新查看当前内容与排布')}</span></div>{geometry && <div className="admin-server-preview" style={{ aspectRatio: `${geometry.width}/${geometry.height}` }}>{geometry.nodes.map(n => <div key={n.id} title={`${n.kind} #${n.id}`} className={`admin-server-preview-node preview-${n.kind}`} style={{ left: `${n.x / geometry.width * 100}%`, top: `${n.y / geometry.height * 100}%`, width: `${n.width / geometry.width * 100}%`, height: `${n.height / geometry.height * 100}%`, clipPath: `inset(${Math.max(0, n.clip.y - n.y) / (n.height || 1) * 100}% ${Math.max(0, n.x + n.width - n.clip.x - n.clip.width) / (n.width || 1) * 100}% ${Math.max(0, n.y + n.height - n.clip.y - n.clip.height) / (n.height || 1) * 100}% ${Math.max(0, n.clip.x - n.x) / (n.width || 1) * 100}%)`, fontSize: `${(n.fontDip ?? 18) / geometry.width * 100}cqw` }}>{n.text ?? ({ clock: '时钟', date: '日期', schedule: '课程表', materials: '课堂资料', homework: '今日作业', reminder: '提醒', subject_text: '文字', subject_image: '图片', recess: '课间', date_countdown: '倒计时' } as Record<string, string>)[n.kind] ?? ''}</div>)}</div>}{geometry?.nodes.some(n => n.textTruncated) && <p>长文本仅显示开头，预览尺寸按完整内容计算。</p>}</Section>
}
