import { useEffect, useRef, useState } from 'react'
import { RefreshCw } from 'lucide-react'
import { Button } from './components/ui/button'
import { contentPath, type ContentDocument } from './api/content-protocol'
import { SubjectVisualController, type SubjectVisualState } from './api/subject-visual-controller'
import { previewStyle, type HomeworkTarget, type SubjectVisualApi } from './api/subject-visual-protocol'
export default function SubjectVisualPreview({ api, target, source, expired, onRevoked }: { api: SubjectVisualApi; target: HomeworkTarget; source: ContentDocument; expired: boolean; onRevoked: () => void }) {
  const [state, setState] = useState<{ view: SubjectVisualState; source: ContentDocument; refresh: number; api: SubjectVisualApi } | null>(null), [refresh, setRefresh] = useState(0)
  const revoke = useRef(onRevoked); revoke.current = onRevoked
  const path = contentPath(target)
  useEffect(() => {
    if (expired) return
    const controller = new SubjectVisualController(api, view => setState({ view, source, refresh, api }), () => revoke.current())
    void controller.load(target)
    return () => controller.dispose()
    // Explicit content reloads change source even when the exact key is unchanged.
    // Text typing preserves source and never starts a preview read.
  }, [api, path, target, source, refresh, expired])
  const current = !expired && state !== null && state.source === source && state.refresh === refresh && state.api === api && state.view.path === path ? state.view : null, preview = current?.preview
  return <section className="my-4 rounded-xl border p-4" aria-label="学校科目配色只读预览" data-testid="subject-visual-preview">
    <div className="flex items-center justify-between gap-3"><h3 className="font-semibold">学校科目配色 · 只读预览</h3><Button data-testid="subject-visual-refresh" variant="outline" disabled={expired || current?.phase === 'loading'} onClick={() => setRefresh(n => n + 1)}><RefreshCw />重新读取配色</Button></div>
    <p className="editor-help">学校基础展示样例；颜色和外观由本地主管理员维护。不代表当前布局或已经上屏，正文草稿不会改变。</p>
    {expired ? <p role="status">原日期已过期，请重新读取当天范围。</p> : current?.notice ? <p role="status">{current.notice}</p> : !preview ? <p role="status">正在读取同版科目与显示设置…</p> : <>
      <p className="editor-help" data-testid="subject-visual-version" data-subject-revision={preview.subjectRevision} data-display-revision={preview.displayRevision} data-rules-version={preview.rulesVersion}>科目修订 {preview.subjectRevision} · 显示修订 {preview.displayRevision} · 颜色规则 {preview.rulesVersion} · {preview.mode === 'FILLED' ? '填色无边框' : '透明底柔和描边'}</p>
      {(['homework', 'course'] as const).map(kind => {
        const style = previewStyle(preview, kind), fill = kind === 'homework' ? preview.homeworkFillPercent : preview.courseFillPercent
        const opacity = kind === 'homework' ? preview.homeworkComponentOpacityPercent : preview.courseComponentOpacityPercent
        const visible = kind === 'homework' ? preview.homeworkVisible : preview.courseVisible
        return <div className="my-3" key={kind}>
          <p className="editor-help">{kind === 'homework' ? '作业科目标记' : '课程胶囊'}：填充 {fill}% · 组件整体 {opacity}% · 配置{visible ? '显示' : '隐藏'}（0%完全透明，100%不透明）</p>
          <div className="rounded-lg bg-white p-3 text-slate-950">
            <div style={{ opacity: style.componentOpacity }} data-testid={`${kind}-visual-component`} data-preview-component={kind} data-component-visible={visible}>
              <span className="inline-block min-w-28 rounded-full px-4 py-2 text-center" data-testid={`${kind}-visual-pill`} data-preview-mode={preview.mode} data-fill-percent={fill} data-effective-rgb={preview.effectiveRgb} style={{ backgroundColor: style.backgroundColor, border: style.border }}>{preview.subjectName}</span>
            </div>
          </div>
        </div>
      })}
      <p className="editor-help">样例使用浅色背景核对配色。填充透明度只影响底色，文字保持清晰；组件整体透明度只作用于样例，不影响操作按钮和说明。隐藏组件仍提供外观样例供核对。</p>
    </>}
  </section>
}
