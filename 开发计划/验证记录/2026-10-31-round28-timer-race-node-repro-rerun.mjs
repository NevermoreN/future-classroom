// Explicit rerun of the earlier in-memory source reproduction. The earlier
// terminal output was not saved as a file; this file captures a new execution.
// No microphone, browser, filesystem mutation or project test runner is used.
import { readFileSync } from 'node:fs'
import { stripTypeScriptTypes } from 'node:module'
console.log(JSON.stringify({ observation: 'explicit-rerun', startedAt: new Date().toISOString() }))
const root = 'web/src/voice/'
const load = source => import('data:text/javascript;base64,' + Buffer.from(stripTypeScriptTypes(source, { mode: 'transform' })).toString('base64'))
const pcm = await load(readFileSync(root + 'pcm.ts', 'utf8'))
const ports = await load(readFileSync(root + 'ports.ts', 'utf8'))
globalThis.__voiceTimerReview = { ...pcm, ...ports, BrowserCapturePort: class {}, BrowserWavNormalizer: class {}, recordingSupport() {} }
const source = readFileSync(root + 'recorder.ts', 'utf8').replace(/^import[\s\S]*?from ['"][^'"]+['"]\n/gm, '')
const { RecorderController } = await load('const { BrowserCapturePort, BrowserWavNormalizer, checkedMaxDurationMillis, checkedSampleRate, MAX_RAW_BYTES, outputFrames, RecorderError, recordingSupport } = globalThis.__voiceTimerReview;\n' + source)
for (const [rate, finalFrames] of [[16000, 7936], [11025, 5376]]) {
  let sink, timer
  const errors = [], wav = []
  const recorder = new RecorderController({
    capturePort: { open: async next => { sink = next; return { sampleRate: rate, start: async () => {}, stop: async () => sink.pcm(new Float32Array(finalFrames - 4096), rate), abort() {} } } },
    normalizer: { normalize: async (chunks, sourceRate) => { const frames = pcm.outputFrames(chunks.reduce((sum, chunk) => sum + chunk.length, 0), sourceRate); return { wav: new Blob([pcm.encodeWav(new Float32Array(frames))], {type:'audio/wav'}), frames, sampleRate: 16000 } } },
    clock: { setTimeout(callback, milliseconds) { timer = callback; if (milliseconds !== 500) throw Error('wrong timer'); return callback }, clearTimeout() {} },
    onError: error => errors.push(error.code), onWav: result => wav.push(result.frames),
  })
  await recorder.start('timer-race-' + rate, 500)
  sink.pcm(new Float32Array(4096), rate)
  timer()
  await Promise.resolve(); await Promise.resolve(); await Promise.resolve()
  console.log(JSON.stringify({ source: 'actual RecorderController + actual pcm rules; controlled capture and wall timer', rate, configuredMillis:500, finalFrames, requiredFrames:Math.floor(rate / 2), errors, emittedWavFrames:wav, recorderPhase:recorder.state.phase }))
  recorder.dispose()
}
delete globalThis.__voiceTimerReview
