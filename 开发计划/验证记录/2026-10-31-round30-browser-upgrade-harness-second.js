async (page) => {
  const server = {"pid": 20166, "procStart": "136390668", "port": 45636, "controlToken": "53083efa83c44da6b7215ecf3977ff2c", "scope": "owned synthetic static HTTPS fixture; no OS trust mutation"};
  const base = `https://172.30.0.2:${server.port}`;
  const browser = page.context().browser(), checks = [], cases = [], contexts = [];
  const check = (name, value) => { if (!value) throw new Error(name); checks.push(name); };
  const caps = { schemaVersion: 1, status: 'VOICE_CAPABILITIES', identityRevision: '9', runtimeGeneration: '3', serverNow: '2026-10-31T12:00:00', currentClass: { classId: '7', className: '合成升级班级' }, enabled: true, admission: 'OPEN', targetAuthorized: true, deviceAvailability: 'UNVERIFIED', defaultRepeatTotal: 2, minRepeatTotal: 1, maxRepeatTotal: 2, minDurationMillis: 500, maxDurationMillis: 3000, sampleRate: 16000, channels: 1, bitsPerSample: 16, maxWavBytes: 96044, uploadTtlSeconds: 600 };
  const contentKey = { kind: 'HOMEWORK', classId: '7', subjectId: '2', ownerTeacherId: '9', date: '2026-10-31' };
  const document = { schemaVersion: 1, status: 'CONTENT', today: '2026-10-31', serverNow: '2026-10-31T12:00:00', expiresAt: '2026-11-01T00:00:00', key: contentKey, revision: '0', text: '', lastActorId: null, updatedAt: null };
  async function deploy(context, name) { const response = await context.request.put(`${base}/__qa/${server.controlToken}/deployment/${name}`, { data: '' }); check(`owned fixture deployment ${name} switched`, response.ok() && (await response.json()).active === name); }
  async function fixture(options = {}) {
    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, serviceWorkers: 'block', ignoreHTTPSErrors: true }); contexts.push(context);
    const state = { apiVersion: options.apiVersion ?? 1, apiCalls: [], posts: [], uploads: [], commands: [], queries: [], errors: [], failed: [] };
    await context.addInitScript(() => {
      const state = { starts: 0, active: 0, stopped: 0, contextCount: 0, contextClosed: 0, ownerClosed: 0, modules: [], workers: [], liveWorkers: 0 };
      window.__fcUpgradeQa = state;
      const OriginalAudioContext = window.AudioContext, OriginalWorker = window.Worker;
      window.AudioContext = class extends OriginalAudioContext {
        constructor(...args) { super(...args); state.contextCount++; const close = this.close.bind(this); let closed = false; this.close = () => { const result = close(); if (!closed) { closed = true; void result.then(() => state.contextClosed++); } return result; }; }
      };
      const addModule = AudioWorklet.prototype.addModule;
      AudioWorklet.prototype.addModule = function(...args) { state.modules.push(String(args[0])); return addModule.apply(this, args); };
      window.Worker = class extends OriginalWorker {
        constructor(url, opts) { super(url, opts); state.workers.push(String(url)); state.liveWorkers++; const terminate = this.terminate.bind(this); let closed = false; this.terminate = () => { terminate(); if (!closed) { closed = true; state.liveWorkers--; } }; }
      };
      Object.defineProperty(navigator.mediaDevices, 'getUserMedia', { configurable: true, value: async () => {
        state.starts++; state.active++;
        const owner = new OriginalAudioContext(), source = owner.createConstantSource(), destination = owner.createMediaStreamDestination();
        source.offset.value = 0; source.connect(destination); source.start(); await owner.resume(); let closed = false;
        for (const track of destination.stream.getTracks()) { const stop = track.stop.bind(track); track.stop = () => { stop(); if (!closed) { closed = true; state.active--; state.stopped++; source.stop(); void owner.close().then(() => state.ownerClosed++); } }; }
        return destination.stream;
      } });
    });
    await context.route(`${base}/fc/api/**`, async route => {
      const req = route.request(), path = new URL(req.url()).pathname; state.apiCalls.push({ path, method: req.method() });
      const json = (body, status = 200) => route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) });
      if (req.method() === 'POST') state.posts.push(path);
      if (path === '/fc/api/v1/status') return json({ schemaVersion: 1, apiVersion: state.apiVersion, uiCompatibilityVersion: 1, online: true });
      if (path === '/fc/api/v1/session') return json({ schemaVersion: 1, status: 'AUTHENTICATED', principal: { id: '9', displayName: '合成升级老师', role: 'TEACHER_ADMIN' }, currentClassId: '7', assignments: [{ classId: '7', subjectId: '2' }], csrf: 'ab'.repeat(32) });
      if (path === '/fc/api/v1/voice/capabilities') return json(caps);
      if (path === '/fc/api/v1/voice/broadcasts' && req.method() === 'GET') return json({ schemaVersion: 1, status: 'VOICE_JOBS', jobs: [] });
      if (path === '/fc/api/v1/voice/uploads') { const body = req.postDataBuffer(); state.uploads.push({ bytes: body?.length, frames: body ? (body.length - 44) / 2 : null }); return json({ schemaVersion: 1, status: 'VOICE_UPLOADED', assetId: '44', assetRevision: '1', classId: '7', bytes: body.length, durationMillis: Math.floor((body.length - 44) / 32), expiresAt: '2026-10-31T12:10:00' }, 201); }
      if (path === '/fc/api/v1/voice/broadcasts' && req.method() === 'POST') { const input = req.postDataJSON(); state.commands.push(input); return json({ schemaVersion: 1, status: 'COMMITTED', commandId: input.commandId, job: { jobId: input.commandId, commandId: input.commandId, classId: '7', repeatTotal: input.repeatTotal, completedRepeats: 0, currentRepeat: 0, state: 'QUEUED', revision: '1', failure: null } }); }
      if (path === '/fc/api/v1/content/homework/scopes/0') return json({ schemaVersion: 1, status: 'SCOPES', kind: 'HOMEWORK', today: '2026-10-31', identityRevision: '9', offset: 0, total: 1, nextOffset: null, items: [{ classId: '7', className: '合成升级班级', subjectId: '2', subjectName: '合成数学', ownerTeacherId: '9', ownerDisplayName: '合成升级老师' }] });
      if (path === '/fc/api/v1/content/homework/7/2/9/2026-10-31') {
        if (req.method() === 'GET') return json(document);
        const input = req.postDataJSON(); state.commands.push(input); return json({ schemaVersion: 2, status: 'COMMITTED', commandId: input.commandId, content: { ...document, revision: '1', text: input.text, lastActorId: '9', updatedAt: '2026-10-31T12:00:00' } });
      }
      if (path.startsWith('/fc/api/v1/content/commands/')) { const commandId = path.split('/').at(-1); state.queries.push(commandId); return json({ schemaVersion: 1, status: 'COMMAND_RESULT', commandId, state: 'NOT_FOUND', content: null }); }
      throw new Error('unexpected synthetic API: ' + req.method() + ' ' + path);
    });
    const tab = await context.newPage(); tab.on('pageerror', e => state.errors.push(e.message)); tab.on('response', r => { if (r.status() >= 400) state.failed.push({ url: r.url(), status: r.status() }); });
    return { context, tab, state };
  }
  async function home(f, deployment = 'b') { await deploy(f.context, deployment); const response = await f.tab.goto(base + '/fc/'); check('fixture uses exact restrictive production CSP', response.headers()['content-security-policy'].includes("worker-src 'self'") && !response.headers()['content-security-policy'].includes('unsafe-inline')); await f.tab.getByRole('button', { name: '打开语音播报' }).waitFor(); }
  async function voice(f, deployment = 'a') { await home(f, deployment); await f.tab.getByRole('button', { name: '打开语音播报' }).click(); await f.tab.getByRole('button', { name: '点击开始录音', exact: true }).waitFor(); await f.tab.waitForFunction(() => !document.querySelector('.voice-alternative button').disabled); }
  async function released(f) { await f.tab.waitForFunction(() => { const q = window.__fcUpgradeQa; return q.active === 0 && q.contextCount === q.contextClosed && q.starts === q.stopped && q.starts === q.ownerClosed && q.liveWorkers === 0; }); return f.tab.evaluate(() => window.__fcUpgradeQa); }
  try {
    const version = await fixture({ apiVersion: 2 }); await deploy(version.context, 'b'); await version.tab.goto(base + '/fc/'); await version.tab.getByRole('heading', { name: '需要刷新教师工作台' }).waitFor();
    check('incompatible initial API has explicit refresh and upgrade message', (await version.tab.locator('body').innerText()).includes('请联系管理员升级'));
    check('incompatible status never requests session or pairing', version.state.apiCalls.length === 1 && version.state.posts.length === 0);
    version.state.apiVersion = 1; await version.tab.getByRole('button', { name: '刷新页面', exact: true }).click(); await version.tab.getByRole('button', { name: '打开语音播报' }).waitFor();
    check('only explicit reload checks compatible status and session again without POST', version.state.apiCalls.filter(x => x.path.endsWith('/status')).length === 2 && version.state.posts.length === 0);
    cases.push({ case: 'API version mismatch explicit reload', apiCalls: version.state.apiCalls }); await version.context.close();

    const entry = await fixture(); await deploy(entry.context, 'a'); let retiredEntry = null;
    await entry.context.route('**/assets/previous-index-*.js', async route => { retiredEntry = route.request().url(); await deploy(entry.context, 'b'); await route.continue(); });
    await entry.tab.goto(base + '/fc/'); await entry.tab.getByRole('heading', { name: '正在载入教师工作台' }).waitFor();
    check('retired entry module leaves self-contained visible HTML recovery', retiredEntry !== null && (await entry.tab.locator('body').innerText()).includes('白板已更新') && entry.state.apiCalls.length === 0);
    check('entry recovery link is same-origin canonical root', await entry.tab.getByRole('link', { name: '重新打开教师工作台' }).getAttribute('href') === './');
    await entry.tab.getByRole('link', { name: '重新打开教师工作台' }).click(); await entry.tab.getByRole('button', { name: '打开语音播报' }).waitFor(); check('entry reload reaches current bundle without replay', entry.state.posts.length === 0);
    cases.push({ case: 'entry asset retired between HTML and module fetch', retiredEntry, failures: entry.state.failed }); await entry.context.close();

    const worklet = await fixture(); await voice(worklet); await deploy(worklet.context, 'b'); await worklet.tab.getByRole('button', { name: '点击开始录音', exact: true }).click(); await worklet.tab.getByText(/录音页面资源无法加载或运行/).waitFor();
    const workletState = await released(worklet); check('old page requests retired Worklet and releases every synthetic track/context', workletState.modules.length === 1 && workletState.modules[0].includes('previous-pcm-worklet-') && workletState.starts === 1 && workletState.stopped === 1);
    check('missing Worklet produces no upload or broadcast', worklet.state.posts.length === 0);
    check('mobile recovery has no horizontal overflow', await worklet.tab.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    await worklet.tab.getByRole('button', { name: '刷新页面', exact: true }).click(); await worklet.tab.getByRole('button', { name: '打开语音播报' }).waitFor();
    cases.push({ case: 'opened previous page then upgrade, lazy Worklet retired', microphone: workletState, failures: worklet.state.failed, pageErrors: worklet.state.errors }); await worklet.context.close();

    const worker = await fixture(); await voice(worker); await worker.tab.getByRole('button', { name: '点击开始录音', exact: true }).click(); await worker.tab.waitForFunction(() => document.querySelector('.voice-hold').classList.contains('is-recording')); await worker.tab.waitForTimeout(650); await deploy(worker.context, 'b'); await worker.tab.getByRole('button', { name: '结束并发送', exact: true }).click(); await worker.tab.getByText(/录音页面资源无法加载或运行/).waitFor();
    const workerState = await released(worker); check('old Worker is terminated after package changes during recording', workerState.workers.length === 1 && workerState.workers[0].includes('previous-wav-worker-') && workerState.liveWorkers === 0);
    check('missing Worker produces no upload or broadcast', worker.state.posts.length === 0);
    cases.push({ case: 'recording on previous package then upgrade before lazy Worker fetch', microphone: workerState, failures: worker.state.failed, pageErrors: worker.state.errors }); await worker.context.close();

    const draft = await fixture(); await home(draft); await draft.tab.getByRole('button', { name: '管理今日作业', exact: true }).click(); await draft.tab.locator('.scope-option').click(); await draft.tab.locator('#content-text').fill('合成升级草稿：保留原命令');
    let dialogs = []; const dismiss = async dialog => { dialogs.push({ type: dialog.type(), message: dialog.message() }); await dialog.dismiss(); }; draft.tab.on('dialog', dismiss);
    const calls = draft.state.apiCalls.length; await draft.tab.getByRole('button', { name: '刷新页面', exact: true }).click(); check('cancel refresh keeps unsaved draft and performs no request', await draft.tab.locator('#content-text').inputValue() === '合成升级草稿：保留原命令' && draft.state.apiCalls.length === calls && dialogs.length === 1);
    await draft.tab.getByRole('button', { name: '保存当天内容', exact: true }).click(); await draft.tab.getByText(/白板与页面版本不兼容/).first().waitFor(); const original = draft.state.commands[0];
    check('version mismatch retains original draft and unknown command', original && await draft.tab.locator('#content-text').inputValue() === original.text && (await draft.tab.getByRole('region', { name: '原提交命令' }).innerText()).includes(original.commandId));
    check('version mismatch disables new content save', await draft.tab.getByRole('button', { name: '保存当天内容', exact: true }).isDisabled());
    await draft.tab.getByRole('button', { name: '手动查询原命令', exact: true }).click(); await draft.tab.getByText(/当前未查到原命令结果/).waitFor(); check('only explicit GET uses original ID, without new POST', draft.state.queries.length === 1 && draft.state.queries[0] === original.commandId && draft.state.posts.length === 1);
    await draft.tab.getByRole('button', { name: '刷新页面', exact: true }).click(); check('cancel reload preserves original command and draft after unknown response', dialogs.length === 2 && await draft.tab.locator('#content-text').inputValue() === original.text);
    draft.tab.off('dialog', dismiss); draft.tab.on('dialog', dialog => dialog.accept()); await draft.tab.getByRole('button', { name: '刷新页面', exact: true }).click(); await draft.tab.getByRole('button', { name: '打开语音播报' }).waitFor(); check('confirmed reload discards memory and never replays old POST', draft.state.posts.length === 1 && await draft.tab.locator('#content-text').count() === 0);
    cases.push({ case: 'dirty/unknown command refresh consent and original-ID manual query', dialogs, original, apiCalls: draft.state.apiCalls }); await draft.context.close();

    const success = await fixture(); await voice(success, 'b'); await success.tab.getByRole('button', { name: '点击开始录音', exact: true }).click(); await success.tab.waitForFunction(() => document.querySelector('.voice-hold').classList.contains('is-recording')); await success.tab.waitForTimeout(650); await success.tab.getByRole('button', { name: '结束并发送', exact: true }).click(); await success.tab.waitForFunction(() => document.querySelector('.voice-job-list li') !== null); const microphone = await released(success);
    check('new deployment real Worklet/render/Worker succeeds after refresh implementation', success.state.uploads.length === 1 && success.state.commands.length === 1 && microphone.modules[0].includes('/assets/pcm-worklet-') && !microphone.modules[0].includes('previous-') && microphone.workers[0].includes('/assets/wav-worker-') && !microphone.workers[0].includes('previous-'));
    check('successful recording has no unhandled page error', success.state.errors.length === 0);
    check('no credentials/drafts in browser storage', await success.tab.evaluate(() => localStorage.length === 0 && sessionStorage.length === 0));
    cases.push({ case: 'current bundled recording positive control', microphone, uploads: success.state.uploads, commands: success.state.commands.length, pageErrors: success.state.errors }); await success.context.close();
    return { status: 'PASS', browserVersion: browser.version(), checks, checkCount: checks.length, cases, scope: 'Actual Chromium DOM and bundled Worklet/OfflineAudio/Worker, zero-source synthetic MediaStream, synthetic two-package static HTTPS switch (previous URLs renamed), API intercepted, ignoreHTTPSErrors=true fixture only; not Rust native browser TLS, real hardware, installer or three-OS acceptance' };
  } catch (error) {
    const diagnostics = [];
    for (const context of contexts) for (const tab of context.pages()) diagnostics.push({ url: tab.url(), body: (await tab.locator('body').innerText()).slice(-6500), microphone: await tab.evaluate(() => window.__fcUpgradeQa) });
    return { status: 'FAIL', error: String(error.message), checks, cases, diagnostics };
  } finally { for (const context of contexts) { try { await context.close(); } catch {} } }
}
