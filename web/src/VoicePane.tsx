import { useRef } from 'react'
import { CircleAlert, Clock3, LoaderCircle, Mic, RefreshCw, Square, Volume2, X } from 'lucide-react'
import { Button } from './components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from './components/ui/card'
import { newCommandId } from './api/content-protocol'
import { mayRecord, type VoiceController, type VoiceState } from './api/voice-controller'
import { terminal, type VoiceStatus } from './api/voice-protocol'
import { recordingSupport, type RecorderController, type RecorderState } from './voice/recorder'

const labels: Record<VoiceStatus, string> = { QUEUED: '已排队', STARTING: '正在准备播放', PLAYING: '播放中', GAP: '遍间静音', STOPPING: '正在等待驱动归还', COMPLETED: '驱动播放完成', CANCELED: '已取消', FAILED: '播放失败', EXPIRED: '已过期', INTERRUPTED: '已中断' }
export default function VoicePane({ state, controller, recorder, recording }: { state: VoiceState; controller: VoiceController | undefined; recorder: RecorderController | undefined; recording: RecorderState }) {
  const gesture = useRef<{ id: string; pointerId: number | null; y: number; key: string | null } | null>(null)
  const caps = state.capabilities, support = recordingSupport()
  const recordingBusy = recording.phase !== 'idle', canBegin = support.supported && mayRecord(caps) && !state.busy && state.phase === 'idle' && !state.pending
  function start(pointerId: number | null, y = 0, key: string | null = null) {
    if (!canBegin || !controller || !recorder || gesture.current) return
    let id: string
    try { id = newCommandId() } catch { return }
    if (!controller.beginRecording(id)) return
    gesture.current = { id, pointerId, y, key }; void recorder.start(id)
  }
  function finish() { const active = gesture.current; gesture.current = null; const id = active?.id ?? recording.recordingId; if (id) void recorder?.finish(id) }
  function cancel() { gesture.current = null; recorder?.cancel(); controller?.cancelUnsent() }
  const recordingLabel = recording.phase === 'requesting' ? '等待麦克风授权，松开即取消' : recording.phase === 'finalizing' ? '正在处理录音' : recording.phase === 'recording' ? recording.cancelGesture ? '松开取消' : '松开发送 · 上滑取消' : '按住说话，松开发送'
  return <section className="voice-workspace" aria-label="语音播报">
    <div className="content-toolbar"><span><Volume2 size={17} />目标以这台白板当前绑定班级为准</span><Button variant="outline" disabled={state.busy || !!state.pending || recordingBusy} onClick={() => { void controller?.refresh() }}><RefreshCw />核对目标与任务</Button></div>
    {state.notice && <p className="content-notice attention" role="status"><CircleAlert size={17} />{state.notice}</p>}
    <div className="voice-columns">
      <Card className="voice-recorder"><CardHeader><p className="eyebrow">教师主动发起 · 独立语音通知</p><CardTitle role="heading" aria-level={2}>{caps?.currentClass?.className ?? '尚未取得白板班级'}</CardTitle><CardDescription>核对班级后录音。发送后按已锁定次数顺序播放，不改变系统音量。</CardDescription></CardHeader><CardContent>
        <div className="voice-target-status"><span>{!caps ? '正在核对服务' : !caps.currentClass ? '白板尚未绑定班级' : !caps.targetAuthorized ? '你没有向该班级提交语音的权限' : !caps.enabled ? '白板已关闭远程语音' : caps.admission === 'CLOSED' ? '白板当前暂停接纳语音' : caps.deviceAvailability === 'UNAVAILABLE' ? '白板输出设备不可用' : caps.deviceAvailability === 'UNVERIFIED' ? '输出设备将在实际播放时核验' : '输出格式已核验，设备状态可能变化'}</span></div>
        <label className="voice-repeat" htmlFor="voice-repeat">总播放次数<select id="voice-repeat" value={state.repeatTotal} disabled={recordingBusy || state.phase !== 'idle' || state.busy || !!state.pending} onChange={e => controller?.setRepeat(Number(e.target.value))}>{Array.from({ length: 10 }, (_, i) => <option value={i + 1} key={i + 1}>{i + 1} 遍{i === 2 ? '（默认）' : ''}</option>)}</select></label>
        <p className="voice-explanation">包含首次，每遍间隔 1 秒。录音开始后锁定目标及次数；修改下次设置不会改变已排队任务。</p>
        <button type="button" className={`voice-hold ${recording.phase === 'recording' ? 'is-recording' : ''} ${recording.cancelGesture ? 'is-cancelling' : ''}`} disabled={!canBegin && !recordingBusy || recording.phase === 'finalizing'} aria-label="按住说话，松开发送" aria-describedby="voice-recording-help" onContextMenu={e => e.preventDefault()}
          onPointerDown={e => { if (e.button !== 0 || !e.isPrimary) return; e.preventDefault(); start(e.pointerId, e.clientY); if (gesture.current) { try { e.currentTarget.setPointerCapture(e.pointerId) } catch { cancel() } } }}
          onPointerMove={e => { const active = gesture.current; if (active?.pointerId === e.pointerId) recorder?.setCancelGesture(active.y - e.clientY >= 48) }}
          onPointerUp={e => { if (gesture.current?.pointerId === e.pointerId) { e.preventDefault(); finish() } }}
          onPointerCancel={cancel} onLostPointerCapture={() => { if (gesture.current?.pointerId !== null && gesture.current) cancel() }}
          onKeyDown={e => { if (e.key === 'Escape') { e.preventDefault(); cancel() } else if ([' ', 'Enter'].includes(e.key)) { e.preventDefault(); if (!e.repeat) start(null, 0, e.key) } }}
          onKeyUp={e => { if (gesture.current?.key === e.key) { e.preventDefault(); finish() } }} onBlur={() => { if (gesture.current) cancel() }}>
          {recording.phase === 'requesting' || recording.phase === 'finalizing' ? <LoaderCircle size={31} className="spinner" /> : recording.cancelGesture ? <X size={31} /> : <Mic size={31} />}<strong>{recordingLabel}</strong><span>{(recording.elapsedMillis / 1000).toFixed(1)} / 60 秒</span>
        </button>
        <p id="voice-recording-help" className="voice-explanation">0.5–60 秒。按住空格或 Enter 也可录音；Esc 取消。达到 60 秒按当前手势发送或丢弃，随后松手不会重复发送。</p>
        <div className="voice-alternative"><Button variant="outline" disabled={!canBegin && !['requesting', 'recording'].includes(recording.phase)} onClick={() => { if (recording.phase === 'idle') start(null); else finish() }}>{recording.phase === 'idle' ? <Mic /> : <Square />}{recording.phase === 'idle' ? '点击开始录音' : '结束并发送'}</Button><Button variant="ghost" disabled={!recordingBusy && state.phase !== 'uploading'} onClick={cancel}><X />取消录音</Button></div>
        {!support.supported && <p className="content-notice attention" role="status"><CircleAlert size={17} />{support.reason}</p>}
        <p className="voice-privacy">仅采集当前教师设备麦克风。切换目标、隐藏或离开页面会取消未发送录音并释放麦克风。没有云端转码或语音识别。</p>
      </CardContent></Card>
      <div className="voice-tasks"><Card><CardHeader><div className="voice-task-heading"><Clock3 size={20} /><CardTitle role="heading" aria-level={2}>白板播报任务</CardTitle></div><CardDescription>排队任务不会因网页断网自动撤回。前台有活动任务时每秒查询，后台停止查询。</CardDescription></CardHeader><CardContent>
        {state.pending && <section className="command-receipt" aria-label="原语音命令"><strong>{state.pending.kind === 'SUBMIT' ? '原播报命令' : '原停止命令'} {state.pending.input.commandId}</strong><p>结果未确认时不补发。当前未查到任务也不证明从未提交。</p><Button variant="outline" disabled={state.busy} onClick={() => { void controller?.queryPending() }}><RefreshCw />手动查询原任务</Button></section>}
        {state.jobs.length === 0 && <p className="empty-scope">当前没有可见任务。上传成功与任务已排队是两个独立阶段。</p>}
        <ul className="voice-job-list">{state.jobs.map(job => <li key={job.jobId}><div className="voice-job-top"><strong>{labels[job.state]}</strong><span>{job.completedRepeats} / {job.repeatTotal} 遍完成</span></div><p>{caps?.currentClass?.classId === job.classId ? caps.currentClass.className : `原班级 ID ${job.classId}`}</p><p className="voice-job-id">任务 {job.jobId} · 修订 {job.revision}</p>{['STARTING', 'PLAYING', 'GAP'].includes(job.state) && <p>{job.state === 'GAP' ? '静音间隔' : `第 ${job.currentRepeat} 遍 / 共 ${job.repeatTotal} 遍`}</p>}{job.failure && <p className="voice-job-error">失败原因：{job.failure}</p>}{job.state === 'STOPPING' && <p>已请求终止或结束，等待设备实际归还后确认终态。</p>}{!terminal(job) && job.state !== 'STOPPING' && <Button variant="outline" disabled={state.busy || !!state.pending || recordingBusy} onClick={() => { if (window.confirm('停止这个任务，并取消它尚未播放的剩余遍数？')) void controller?.stop(job) }}><Square />{job.state === 'QUEUED' ? '取消此待播任务' : '停止此任务'}</Button>}</li>)}</ul>
        <p className="voice-privacy">“驱动播放完成”表示本机驱动报告自然结束，不保证扬声器开启或人耳听到。此页不能控制其他应用或系统音量。</p>
      </CardContent></Card></div>
    </div>
  </section>
}
