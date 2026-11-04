import { useEffect, useRef } from 'react'
import { AudioLines, CircleAlert, Clock3, Info, LoaderCircle, Mic, Minus, Plus, RefreshCw, Square, UsersRound, Volume2, X } from './components/icons'
import { Button } from './components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from './components/ui/card'
import { newRecordingId, mayRecord, type VoiceController, type VoiceState } from './api/voice-controller'
import { terminal, type VoiceStatus } from './api/voice-protocol'
import { recordingSupport, type RecorderController, type RecorderState } from './voice/recorder'

const labels: Record<VoiceStatus, string> = { QUEUED: '已排队', STARTING: '正在准备播放', PLAYING: '播放中', GAP: '播放间隔', STOPPING: '正在停止', COMPLETED: '播放已结束', CANCELED: '已取消', FAILED: '播放失败', EXPIRED: '已过期', INTERRUPTED: '已中断' }
const failures = { DEVICE: '教室音频设备不可用', SESSION: '教室当前无法播放', REVOKED: '播报授权已失效', DISABLED: '语音播报已关闭', TIMEOUT: '播放超时', CLOCK_REGRESSION: '教室电脑时间发生变化', STORAGE: '录音文件读取失败' }
export default function VoicePane({ state, controller, recorder, recording }: { state: VoiceState; controller: VoiceController | undefined; recorder: RecorderController | undefined; recording: RecorderState }) {
  const gesture = useRef<{ id: string; pointerId: number | null; y: number; key: string | null } | null>(null)
  const caps = state.capabilities, support = recordingSupport()
  const maxSeconds = (state.context?.capabilities.maxDurationMillis ?? caps?.maxDurationMillis ?? 60000) / 1000
  useEffect(() => {
    // Automatic sample/time limits also finish the click-to-record alternative,
    // which has no later pointerup to release its original gesture.
    if (recording.phase === 'idle' && state.phase !== 'recording') gesture.current = null
  }, [recording.phase, state.phase])
  const recordingBusy = recording.phase !== 'idle', canBegin = support.supported && mayRecord(caps) && !state.busy && state.phase === 'idle' && !state.pending
  function start(pointerId: number | null, y = 0, key: string | null = null) {
    if (!canBegin || !controller || !recorder || gesture.current) return
    let id: string
    try { id = newRecordingId() } catch { return }
    const context = controller.beginRecording(id)
    if (!context) return
    gesture.current = { id, pointerId, y, key }; void recorder.start(id, context.capabilities.maxDurationMillis)
  }
  function finish() { const active = gesture.current; gesture.current = null; const id = active?.id ?? recording.recordingId; if (id) void recorder?.finish(id) }
  function cancel() { gesture.current = null; recorder?.cancel(); controller?.cancelUnsent() }
  const recordingLabel = recording.phase === 'requesting' ? '等待麦克风授权，松开即取消' : recording.phase === 'finalizing' ? '正在处理录音' : recording.phase === 'recording' ? recording.cancelGesture ? '松开取消' : '松开发送 · 上滑取消' : '按住说话，松开发送'
  const repeatLocked = recordingBusy || state.phase !== 'idle' || state.busy || !!state.pending
  const activeJob = state.jobs.find(job => ['STARTING', 'PLAYING', 'GAP', 'STOPPING'].includes(job.state))
  const elapsedSeconds = Math.floor(recording.elapsedMillis / 1000)
  return <section className="voice-workspace" aria-label="语音播报">
    <div className="voice-page-title"><Volume2 size={36} /><div><h2>教室语音播报</h2><p>向当前班级的教室设备播放语音</p></div></div>
    <div className="content-toolbar"><span><Volume2 size={17} />目标以这台白板当前绑定班级为准</span><Button variant="outline" disabled={state.busy || !!state.pending || recordingBusy} onClick={() => { void controller?.refresh() }}><RefreshCw />刷新状态</Button></div>
    {state.notice && <p className="content-notice attention" role="status"><CircleAlert size={17} />{state.notice}</p>}
    <div className="voice-target"><strong>播报目标</strong><UsersRound size={24} /><span>{caps?.currentClass?.className ?? '尚未取得白板班级'}<small>当前白板</small></span><span className={`voice-admission ${mayRecord(caps) ? 'available' : ''}`}>{!caps ? '核对中' : mayRecord(caps) ? '可发送' : '暂不可发送'}</span></div>
    <div className="voice-columns">
      <Card className="voice-recorder"><CardHeader><CardTitle role="heading" aria-level={2}>录制语音</CardTitle><CardDescription>录音仅在按住时开启，正常松开后自动发送。</CardDescription></CardHeader><CardContent>
        <div className="voice-target-status"><span>{!caps ? '正在连接教室' : !caps.currentClass ? '请管理员先设置白板班级' : !caps.targetAuthorized ? '你没有向该班级播报的权限' : !caps.enabled ? '白板已关闭远程语音' : caps.admission === 'CLOSED' ? '教室暂不接收新的语音' : caps.deviceAvailability === 'UNAVAILABLE' ? '教室音频设备暂不可用，请稍后刷新。' : caps.deviceAvailability === 'UNVERIFIED' ? '教室音频设备将在播放时检查。' : '可以向当前班级发送语音'}</span></div>
        <div className="voice-repeat"><label htmlFor="voice-repeat">总播放次数</label><div className="voice-repeat-stepper"><button type="button" aria-label="减少播放次数" disabled={repeatLocked || state.repeatTotal <= 1} onClick={() => controller?.setRepeat(state.repeatTotal - 1)}><Minus size={20} /></button><select id="voice-repeat" value={state.repeatTotal} disabled={repeatLocked} onChange={e => controller?.setRepeat(Number(e.target.value))}>{Array.from({ length: caps?.maxRepeatTotal ?? 10 }, (_, i) => <option value={i + 1} key={i + 1}>{i + 1}</option>)}</select><button type="button" aria-label="增加播放次数" disabled={repeatLocked || state.repeatTotal >= (caps?.maxRepeatTotal ?? 10)} onClick={() => controller?.setRepeat(state.repeatTotal + 1)}><Plus size={20} /></button></div><span>默认播放 {caps?.defaultRepeatTotal ?? 3} 遍，包含首次播放</span></div>
        <div className={`voice-recording-stage ${recording.phase === 'recording' ? 'is-recording' : ''}`}><p className="voice-recording-label" role="status">{recording.phase === 'recording' ? recording.cancelGesture ? '松开取消录音' : '正在录音' : recording.phase === 'requesting' ? '等待麦克风授权' : recording.phase === 'finalizing' ? '正在处理录音' : '按住按钮开始录音'}</p><p className="voice-gesture-help">{recordingLabel}</p><div className="voice-recording-control"><AudioLines className="voice-wave" size={76} aria-hidden="true" />
        <button type="button" className={`voice-hold ${recording.phase === 'recording' ? 'is-recording' : ''} ${recording.cancelGesture ? 'is-cancelling' : ''}`} disabled={!canBegin && !recordingBusy || recording.phase === 'finalizing'} aria-label="按住说话，松开发送" aria-describedby="voice-recording-help" onContextMenu={e => e.preventDefault()}
          onPointerDown={e => { if (e.button !== 0 || !e.isPrimary) return; e.preventDefault(); start(e.pointerId, e.clientY); if (gesture.current) { try { e.currentTarget.setPointerCapture(e.pointerId) } catch { cancel() } } }}
          onPointerMove={e => { const active = gesture.current; if (active?.pointerId === e.pointerId) recorder?.setCancelGesture(active.y - e.clientY >= 48) }}
          onPointerUp={e => { if (gesture.current?.pointerId === e.pointerId) { e.preventDefault(); finish() } }}
          onPointerCancel={cancel} onLostPointerCapture={() => { if (gesture.current?.pointerId !== null && gesture.current) cancel() }}
          onKeyDown={e => { if (e.key === 'Escape') { e.preventDefault(); cancel() } else if ([' ', 'Enter'].includes(e.key)) { e.preventDefault(); if (!e.repeat) start(null, 0, e.key) } }}
          onKeyUp={e => { if (gesture.current?.key === e.key) { e.preventDefault(); finish() } }} onBlur={() => { if (gesture.current) cancel() }}>
          {recording.phase === 'requesting' || recording.phase === 'finalizing' ? <LoaderCircle size={45} className="spinner" /> : recording.cancelGesture ? <X size={45} /> : <Mic size={45} />}<strong>{recording.cancelGesture ? '松开取消' : '按住说话'}</strong>
        </button><AudioLines className="voice-wave" size={76} aria-hidden="true" /></div><p className="voice-timer"><strong>{String(Math.floor(elapsedSeconds / 60)).padStart(2, '0')}:{String(elapsedSeconds % 60).padStart(2, '0')}</strong><span> / 最长 {maxSeconds} 秒</span></p></div>
        <p id="voice-recording-help" className="voice-explanation">录音需满 0.5 秒，最长 {maxSeconds} 秒。按住空格或 Enter 也可录音，Esc 取消。</p>
        <div className="voice-alternative"><Button variant="outline" disabled={!canBegin && !['requesting', 'recording'].includes(recording.phase)} onClick={() => { if (recording.phase === 'idle') start(null); else finish() }}>{recording.phase === 'idle' ? <Mic /> : <Square />}{recording.phase === 'idle' ? '点击开始录音' : '结束并发送'}</Button><Button variant="ghost" disabled={!recordingBusy && state.phase !== 'uploading'} onClick={cancel}><X />取消录音</Button></div>
        {!support.supported && <p className="content-notice attention" role="status"><CircleAlert size={17} />{support.reason}</p>}
        <p className="voice-info"><Info size={20} />仅采集当前教师设备麦克风。切换目标、隐藏或离开页面会取消未发送录音并释放麦克风。</p>
      </CardContent></Card>
      <div className="voice-tasks">{activeJob && <section className="voice-playing" aria-label="当前播报状态"><div><span className="voice-playing-icon"><Volume2 size={32} /></span><div><h3>{activeJob.state === 'PLAYING' ? '教室正在播放' : labels[activeJob.state]}</h3><p>第 {activeJob.currentRepeat} 遍 / 共 {activeJob.repeatTotal} 遍</p><p>{caps?.currentClass?.classId === activeJob.classId ? caps.currentClass.className : '发送时的班级'}</p></div><AudioLines size={42} aria-hidden="true" /></div><progress max={activeJob.repeatTotal} value={activeJob.completedRepeats} aria-label="已完成播放遍数" /><p>已完成 {activeJob.completedRepeats} / {activeJob.repeatTotal} 遍</p><Button disabled={state.busy || !!state.pending || recordingBusy || activeJob.state === 'STOPPING'} onClick={() => { if (window.confirm('停止这条语音，并取消剩余播放？')) void controller?.stop(activeJob) }}><Square />停止此条及剩余播放</Button></section>}<Card><CardHeader><div className="voice-task-heading"><Clock3 size={20} /><CardTitle role="heading" aria-level={2}>播报记录</CardTitle><span>待播 {state.jobs.filter(job => job.state === 'QUEUED').length} 条</span></div><CardDescription>已发送的语音不会因关闭网页或断网自动取消。</CardDescription></CardHeader><CardContent>
        {state.pending && <section className="command-receipt" aria-label="原语音命令"><strong>{state.pending.kind === 'SUBMIT' ? '发送结果尚未确认' : '停止结果尚未确认'}</strong><p>{state.pending.retired ? '请查询这条语音当前状态，暂时不能确认它是否已停止。' : '请先查询结果，避免重复发送；暂未查到记录时，也不能确认发送失败。'}</p><Button variant="outline" disabled={state.busy} onClick={() => { void controller?.queryPending() }}><RefreshCw />{state.pending.retired ? '查询这条语音' : '查询结果'}</Button></section>}
        {state.history && <section className="historical-result" aria-label="原语音命令历史回执"><h3>上次确认的结果</h3><p>{labels[state.history.job.state]}；最新播放情况请刷新核对。</p><Button variant="outline" disabled={state.busy || !!state.pending} onClick={() => { void controller?.refresh() }}><RefreshCw />刷新播放情况</Button></section>}
        {state.jobs.length === 0 && <p className="empty-scope">暂无播报记录。发送后请在这里确认是否已排队。</p>}
        <ul className="voice-job-list">{state.jobs.map(job => <li key={job.jobId}><div className="voice-job-top"><strong>{labels[job.state]}</strong><span>{job.completedRepeats} / {job.repeatTotal} 遍完成</span></div><p>{caps?.currentClass?.classId === job.classId ? caps.currentClass.className : '发送时的班级'}</p>{['STARTING', 'PLAYING', 'GAP'].includes(job.state) && <p>{job.state === 'GAP' ? '静音间隔' : `第 ${job.currentRepeat} 遍 / 共 ${job.repeatTotal} 遍`}</p>}{job.failure && <p className="voice-job-error">{failures[job.failure]}</p>}{job.state === 'STOPPING' && <p>已请求停止，请等待教室确认。</p>}{!terminal(job) && job.state !== 'STOPPING' && <Button variant="outline" disabled={state.busy || !!state.pending || recordingBusy} onClick={() => { if (window.confirm('停止这条语音，并取消剩余播放？')) void controller?.stop(job) }}><Square />{job.state === 'QUEUED' ? '取消此待播任务' : '停止这条语音'}</Button>}</li>)}</ul>
        <p className="voice-privacy">教室电脑在线且音频设备可用时播放；发送成功不等于已听到。</p>
      </CardContent></Card></div>
    </div>
  </section>
}
