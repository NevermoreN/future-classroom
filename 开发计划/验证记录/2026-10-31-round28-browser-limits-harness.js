async (page) => {
  const browser = page.context().browser();
  const checks = [], results = [], contexts = [];
  const check = (name, value) => { if (!value) throw new Error(name); checks.push(name); };
  const baseCaps = { schemaVersion: 1, status: 'VOICE_CAPABILITIES', identityRevision: '9', runtimeGeneration: '3', serverNow: '2026-10-31T12:00:00', currentClass: { classId: '7', className: 'Synthetic policy class' }, enabled: true, admission: 'OPEN', targetAuthorized: true, deviceAvailability: 'UNVERIFIED', defaultRepeatTotal: 2, minRepeatTotal: 1, maxRepeatTotal: 2, minDurationMillis: 500, maxDurationMillis: 1250, sampleRate: 16000, channels: 1, bitsPerSample: 16, maxWavBytes: 40044, uploadTtlSeconds: 600 };
  async function fixture(overrides = {}) {
    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, serviceWorkers: 'block', ignoreHTTPSErrors: true });
    contexts.push(context);
    const caps = { ...baseCaps, ...overrides }, uploaded = [], submitted = [], errors = [], jobs = [];
    await context.addInitScript(() => {
      const state = { starts: 0, active: 0, stopped: 0, moduleErrors: [], modules: [] };
      const addModule = AudioWorklet.prototype.addModule;
      AudioWorklet.prototype.addModule = function(...args) { state.modules.push(String(args[0])); return addModule.apply(this, args).catch(error => { state.moduleErrors.push(error.name + ': ' + error.message); throw error; }); };
      window.__fcPolicyQa = state;
      Object.defineProperty(navigator.mediaDevices, 'getUserMedia', { configurable: true, value: async () => {
        state.starts++; state.active++;
        const owner = new AudioContext(), source = owner.createConstantSource(), destination = owner.createMediaStreamDestination();
        source.offset.value = 0; source.connect(destination); source.start(); await owner.resume();
        let closed = false;
        for (const track of destination.stream.getTracks()) {
          const stop = track.stop.bind(track);
          track.stop = () => { stop(); if (!closed) { closed = true; state.active--; state.stopped++; source.stop(); void owner.close(); } };
        }
        return destination.stream;
      } });
    });
    await context.route('https://172.30.0.2:36029/**', async route => {
      const request = route.request(), path = new URL(request.url()).pathname;
      const json = (body, status = 200) => route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) });
      if (path === '/fc/api/v1/status') return json({ schemaVersion: 1, apiVersion: 1, uiCompatibilityVersion: 1, online: true });
      if (path === '/fc/api/v1/session') return json({ schemaVersion: 1, status: 'AUTHENTICATED', principal: { id: '1', displayName: 'Synthetic policy teacher', role: 'TEACHER_ADMIN' }, currentClassId: '7', assignments: [{ classId: '7', subjectId: '2' }], csrf: 'ab'.repeat(32) });
      if (path === '/fc/api/v1/voice/capabilities') return json(caps);
      if (path === '/fc/api/v1/voice/broadcasts' && request.method() === 'GET') return json({ schemaVersion: 1, status: 'VOICE_JOBS', jobs });
      if (path === '/fc/api/v1/voice/uploads') {
        const body = request.postDataBuffer();
        check('real Worker WAV is within captured configured bound', body && body.length >= 16044 && body.length <= caps.maxWavBytes && body.toString('ascii', 0, 4) === 'RIFF' && body.toString('ascii', 8, 12) === 'WAVE');
        const frames = (body.length - 44) / 2;
        check('canonical fixed header length and PCM format agree with actual bytes', body.readUInt32LE(40) === frames * 2 && body.readUInt32LE(24) === 16000 && body.readUInt16LE(22) === 1 && body.readUInt16LE(34) === 16);
        uploaded.push({ bytes: body.length, frames });
        return json({ schemaVersion: 1, status: 'VOICE_UPLOADED', assetId: String(40 + uploaded.length), assetRevision: '2', classId: '7', bytes: body.length, durationMillis: Math.floor(frames / 16), expiresAt: '2026-10-31T12:10:00' }, 201);
      }
      if (path === '/fc/api/v1/voice/broadcasts' && request.method() === 'POST') {
        const input = request.postDataJSON(); submitted.push(input);
        const job = { jobId: input.commandId, commandId: input.commandId, classId: input.classId, repeatTotal: input.repeatTotal, completedRepeats: 0, currentRepeat: 0, state: 'QUEUED', revision: String(submitted.length), failure: null };
        jobs.push(job); return json({ schemaVersion: 1, status: 'COMMITTED', commandId: input.commandId, job });
      }
      if (path.startsWith('/fc/api/')) throw new Error('unexpected synthetic API route: ' + path);
      const resource = await fetch('http://172.30.0.2:36028/' + (path === '/fc/' ? 'index.html' : path.replace(/^\/fc\//, '')));
      return route.fulfill({ status: resource.status, headers: { 'content-type': resource.headers.get('content-type') ?? 'application/octet-stream' }, body: await resource.text() });
    });
    const tab = await context.newPage(); tab.on('pageerror', error => errors.push(error.message));
    await tab.goto('https://172.30.0.2:36029/fc/');
    await tab.getByRole('button', { name: '打开语音播报' }).click();
    await tab.locator('#voice-repeat').waitFor();
    return { context, tab, caps, uploaded, submitted, errors };
  }
  try {
    const f = await fixture();
    await f.tab.waitForFunction(() => document.querySelectorAll('#voice-repeat option').length === 2);
    check('configured repeat selector contains exactly1and2 with default2', await f.tab.locator('#voice-repeat').inputValue() === '2');
    check('configured1250ms is visibly explained', (await f.tab.locator('#voice-recording-help').innerText()).includes('1.25'));
    check('390px mobile page has no horizontal overflow', await f.tab.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    check('bottom shared reminder remains available', await f.tab.getByRole('button', { name: /管理今日提醒/ }).count() > 0);
    for (let attempt = 1; attempt <= 2; attempt++) {
      await f.tab.getByRole('button', { name: '点击开始录音', exact: true }).click();
      await f.tab.waitForFunction(count => window.__fcPolicyQa.starts === count, attempt);
      await f.tab.waitForFunction(() => document.querySelector('[role="status"]') !== null);
      await f.tab.waitForFunction(() => document.body.innerText.includes('白板已确认任务持久化。'), undefined, { timeout: 10000 });
      await f.tab.waitForFunction(() => window.__fcPolicyQa.active === 0);
      check('automatic limit submitted exactly once attempt' + attempt, f.submitted.length === attempt && f.uploaded.length === attempt);
      check('captured repeat2 survives actual Worker upload attempt' + attempt, f.submitted[attempt - 1].repeatTotal === 2);
    }
    await f.tab.setViewportSize({ width: 320, height: 800 });
    check('320px mobile page has no horizontal overflow', await f.tab.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    check('real browser worklet/worker had no page errors', f.errors.length === 0);
    results.push({ case: 'configured actual browser zero-source capture, auto-finish and next explicit click', uploads: f.uploaded, commands: f.submitted.length, microphone: await f.tab.evaluate(() => window.__fcPolicyQa), pageErrors: f.errors });

    const changed = await fixture();
    await changed.tab.getByRole('button', { name: '点击开始录音', exact: true }).click();
    await changed.tab.waitForFunction(() => window.__fcPolicyQa.active === 1);
    Object.assign(changed.caps, { runtimeGeneration: '4', maxRepeatTotal: 1, defaultRepeatTotal: 1, maxDurationMillis: 500, maxWavBytes: 16044 });
    await changed.tab.waitForFunction(() => document.body.innerText.includes('播放限制已变化'), undefined, { timeout: 10000 });
    await changed.tab.waitForFunction(() => window.__fcPolicyQa.active === 0);
    check('observed changed policy cancels unsent real recording', changed.uploaded.length === 0 && changed.submitted.length === 0);
    check('changed repeat ceiling is visible after cancellation', await changed.tab.locator('#voice-repeat option').count() === 1);
    check('policy cancellation has no page errors', changed.errors.length === 0);
    results.push({ case: 'policy epoch/duration/repeat change while recording', uploads: 0, commands: 0, microphone: await changed.tab.evaluate(() => window.__fcPolicyQa), pageErrors: changed.errors });

    const invalid = await fixture({ maxWavBytes: 40046 });
    await invalid.tab.waitForFunction(() => document.body.innerText.includes('无法核对语音服务'));
    check('contradictory policy cannot enable recording', await invalid.tab.getByRole('button', { name: '点击开始录音', exact: true }).isDisabled());
    check('invalid policy never requests even synthetic microphone', await invalid.tab.evaluate(() => window.__fcPolicyQa.starts === 0));
    results.push({ case: 'malformed configured capabilities fail closed', pageErrors: invalid.errors });
    return { status: 'PASS', checkCount: checks.length, checks, results, scope: 'actual Chromium DOM and real bundled Worklet/OfflineAudio/Worker on zero-source synthetic MediaStream; own Linux static self-signed HTTPS fixture with ignoreHTTPSErrors=true and intercepted API routes; no trusted production TLS/listener/hardware microphone/Win7/Win10/Seewo claim' };
  } catch (error) {
    const diagnostics = [];
    for (const context of contexts) for (const tab of context.pages()) diagnostics.push({ url: tab.url(), body: (await tab.locator('body').innerText()).slice(-7000), state: await tab.evaluate(() => window.__fcPolicyQa) });
    return { status: 'FAIL', error: String(error.message), checks, results, diagnostics };
  } finally {
    await Promise.all(contexts.map(context => context.close()));
  }
}
