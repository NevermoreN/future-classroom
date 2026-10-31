async (page) => {
  const server = {"fixtureStage": 2, "pid": 21837, "procStart": "136429042", "port": 46606, "controlToken": "57438f9814a44a7db396e2266562765b", "scope": "owned synthetic static HTTPS fixture; no OS trust mutation"}, manifest = {"schemaVersion": 1, "apiVersion": 1, "uiCompatibilityVersion": 1, "assets": [{"path": "assets/previous-favicon-CXuYoSfB.svg", "sizeBytes": 282, "sha256": "46593a2ec46a77d65026d609dcd5630c447360522518d745458091a9df2f7f8a"}, {"path": "assets/previous-index-C3GlEF2q.js", "sizeBytes": 409114, "sha256": "93c86f93c49ba00f36078e5346efc24d36827ff8a582f219afc75942f3a01c43"}, {"path": "assets/previous-index-DFFVdc8-.css", "sizeBytes": 38836, "sha256": "4ecac3be1f360a932da9dd7cb7b2a7df35a47766ef3dd70bab57a505a0714de8"}, {"path": "assets/previous-pcm-worklet-B7E4LRoP.js", "sizeBytes": 1724, "sha256": "9934c276d98e30992dc8148ffc3a0dfdc178fed9aacbb8227255bede45c1fcfa"}, {"path": "assets/previous-wav-worker-LSywWbpJ.js", "sizeBytes": 884, "sha256": "a0e364aa7c0d888d0f7118b86d873c95df95e461303546a8ad1d56d5038e1886"}, {"path": "index.html", "sizeBytes": 1259, "sha256": "fa44966777d87f0834955d51653b1c01e26bf4d7731c58bbb87ece27f3b618df"}]};
  const base = `https://172.30.0.2:${server.port}`, parentBrowser = page.context().browser(), contexts = [], checks = [], cases = [];
  const launchErrors = [];let browser;
  for (const spec of [{channel:'chrome'},{channel:'chrome-dev'},{executablePath:'/usr/bin/chromium'},{executablePath:'/usr/bin/chromium-browser'}]) {
    try {browser=await parentBrowser.browserType().launch({...spec,headless:true,args:['--ignore-certificate-errors-spki-list=SsuG+jtAuUlpnPvkMrz0mNYMtUPB3eo13xkbVqAEmV0=']});break;}catch(error){launchErrors.push({spec,message:String(error.message).slice(0,800)});}
  }
  if(!browser)throw new Error('No known installed disposable Chromium executable: '+JSON.stringify(launchErrors));
  const check = (name, value) => { if (!value) throw new Error(name); checks.push(name); };
  const previousMain = manifest.assets.find(a => /^assets\/previous-index-.*\.js$/.test(a.path));
  async function deploy(f, active) { const r = await f.context.request.put(`${base}/__qa/${server.controlToken}/deployment/${active}`, { data: '' }); if (!r.ok() || (await r.json()).active !== active) throw new Error('owned fixture deployment switch failed'); }
  async function fixture() {
    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, serviceWorkers: 'block', ignoreHTTPSErrors: false });contexts.push(context);
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
    const tab = await context.newPage(), trace = [], cached = [], errors = [], posts = [], urls = new Map();
    const session = await context.newCDPSession(tab); await session.send('Network.enable');
    session.on('Network.requestWillBeSent', e => urls.set(e.requestId, e.request.url));
    session.on('Network.requestServedFromCache', e => cached.push({ requestId: e.requestId, url: urls.get(e.requestId) }));
    session.on('Network.responseReceived', e => trace.push({ url: e.response.url, status: e.response.status, fromDiskCache: e.response.fromDiskCache, fromPrefetchCache: e.response.fromPrefetchCache, mimeType: e.response.mimeType }));
    tab.on('pageerror', e => errors.push(e.message));tab.on('request', r => { if (r.method() === 'POST') posts.push(r.url()); });
    return { context, tab, trace, cached, errors, posts };
  }
  async function home(f) { const r = await f.tab.goto(base + '/fc/'); await f.tab.getByRole('button', { name: '打开语音播报' }).waitFor();return r; }
  async function voice(f) { await f.tab.getByRole('button', { name: '打开语音播报' }).click(); await f.tab.waitForFunction(() => !document.querySelector('.voice-alternative button').disabled); }
  async function hashFetch(f) { return f.tab.evaluate(async path => { const r = await fetch(path, { cache: 'default' }), data = await r.arrayBuffer(); const hash = [...new Uint8Array(await crypto.subtle.digest('SHA-256', data))].map(x => x.toString(16).padStart(2, '0')).join('');return { status: r.status, bytes: data.byteLength, sha256: hash, cacheControl: r.headers.get('cache-control') }; }, '/fc/' + previousMain.path); }
  async function release(f) { await f.tab.waitForFunction(() => { const q=window.__fcUpgradeQa;return q.active===0 && q.starts===q.stopped && q.starts===q.ownerClosed && q.contextCount===q.contextClosed && q.liveWorkers===0; });return f.tab.evaluate(() => window.__fcUpgradeQa); }
  try {
    const f = await fixture();await deploy(f,'a');const initial = await home(f);check('no-route fixture delivers strict CSP and no-cache entry', initial.headers()['cache-control']==='no-cache' && initial.headers()['content-security-policy'].includes("worker-src 'self'") && !initial.headers()['content-security-policy'].includes('unsafe-inline'));
    const before = await hashFetch(f);check('unrouted browser fetch sees exact cached previous main bytes', before.status===200 && before.bytes===previousMain.sizeBytes && before.sha256===previousMain.sha256 && before.cacheControl.includes('immutable'));
    check('CDP observes a real HTTP cache service for previous main', f.cached.some(x=>x.url===base+'/fc/'+previousMain.path) || f.trace.some(x=>x.url===base+'/fc/'+previousMain.path && x.fromDiskCache));
    const revalidated=await home(f);check('same entry remains rendered after browser revalidation', revalidated.headers()['etag']===initial.headers()['etag']);await voice(f);await deploy(f,'b');const after=await hashFetch(f);check('retired previous JS is still available from browser HTTP cache', after.sha256===previousMain.sha256 && after.status===200);
    await f.tab.getByRole('button',{name:'点击开始录音',exact:true}).click();await f.tab.getByText(/录音页面资源无法加载或运行/).waitFor();const stopped=await release(f);check('uncached old Worklet failure releases all tracks and contexts', stopped.starts===1 && stopped.stopped===1 && stopped.modules[0].includes('previous-pcm-worklet-'));
    check('unrouted old Worklet failure never uploads or submits', f.posts.length===0);
    const navigation=f.tab.waitForNavigation();await f.tab.getByRole('button',{name:'刷新页面',exact:true}).click();const current=await navigation;await f.tab.getByRole('button',{name:'打开语音播报'}).waitFor();check('explicit refresh revalidates old entry into changed current HTML', current.headers()['etag']!==initial.headers()['etag']);check('changed HTML selects current main asset instead of cached previous main', f.trace.some(x=>/\/assets\/index-.*\.js$/.test(new URL(x.url).pathname) && x.status===200));
    check('unrouted Worklet scenario has no unhandled page error', f.errors.length===0);cases.push({case:'real HTTP cache enabled, entry revalidation, retired lazy Worklet and explicit refresh',before,after,initialEtag:initial.headers()['etag'],currentEtag:current.headers()['etag'],microphone:stopped,cacheEvents:f.cached,network:f.trace,posts:f.posts,pageErrors:f.errors});await f.context.close();
    const w=await fixture();await deploy(w,'a');await home(w);await voice(w);await w.tab.getByRole('button',{name:'点击开始录音',exact:true}).click();await w.tab.waitForFunction(()=>document.querySelector('.voice-hold').classList.contains('is-recording'));await w.tab.waitForTimeout(650);await deploy(w,'b');await w.tab.getByRole('button',{name:'结束并发送',exact:true}).click();await w.tab.getByText(/录音页面资源无法加载或运行/).waitFor();const ended=await release(w);check('unrouted upgrade after capture retires lazy old Worker safely', ended.workers.length===1 && ended.workers[0].includes('previous-wav-worker-') && ended.liveWorkers===0 && ended.active===0);check('unrouted Worker failure never uploads or submits',w.posts.length===0);check('unrouted Worker scenario has no unhandled page error',w.errors.length===0);cases.push({case:'real HTTP cache enabled, upgrade between capture and Worker',microphone:ended,cacheEvents:w.cached,network:w.trace,posts:w.posts,pageErrors:w.errors});await w.context.close();
    return {status:'PASS',browserVersion:browser.version(),launchErrors,checks,checkCount:checks.length,cases,scope:'Actual Chromium HTTP cache with no context/page routing and no cache disabling, strict same-origin static fixture plus synthetic server API, previous URLs renamed then actually retired; ignoreHTTPSErrors=false; dedicated browser permits only exact owned certificate SPKI SsuG+jtAuUlpnPvkMrz0mNYMtUPB3eo13xkbVqAEmV0= via scoped Chromium flag, then closes. Not Rust native browser TLS, MSI, teacher hardware or formal three-OS compatibility.'};
  }catch(error){const diagnostics=[];for(const c of contexts)for(const p of c.pages())diagnostics.push({url:p.url(),body:(await p.locator('body').innerText()).slice(-5000),microphone:await p.evaluate(()=>window.__fcUpgradeQa)});return {status:'FAIL',error:String(error.message),checks,cases,diagnostics};}
  finally{for(const c of contexts)try{await c.close();}catch{}await browser.close();}
}
