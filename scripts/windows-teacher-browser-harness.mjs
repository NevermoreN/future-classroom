#!/usr/bin/env node
/** Explicit QA only. Real DOM actions; business fetch/XHR and cookie injection forbidden.
 * Private stdin INIT + fixed native ACKs. All phases share one original deadline.
 * The independent original Edge Job remains responsible for physical release.
 */
import {createHash} from 'node:crypto';
import {performance} from 'node:perf_hooks';
import {fileURLToPath} from 'node:url';
import {resolve} from 'node:path';
const MAX_LINE=65536, MAX_CDP=8388608;
const hash=b=>createHash('sha256').update(b).digest('hex');
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
class Fault extends Error { constructor(code,method,nativeCode){super(code);this.code=code;if(method)this.method=method;if(Number.isInteger(nativeCode))this.nativeCode=nativeCode;} }
function check(v,c){if(!v)throw new Fault(c);}
function shape(v,keys,c){check(v&&typeof v==='object'&&!Array.isArray(v)&&Object.keys(v).sort().join(',')===[...keys].sort().join(','),c);}
export function pair(v){ const m=typeof v?.commandTicket==='string'&&/^c2:([0-9a-f]{32}):([1-9][0-9]{0,18})$/.exec(v.commandTicket);check(typeof v?.commandId==='string'&&/^[1-9][0-9]{0,19}$/.test(v.commandId)&&BigInt(v.commandId)<=18446744073709551615n&&m&&m[1]!=='0'.repeat(32)&&BigInt(m[2])<=9223372036854775807n,'PAIR_INVALID');return{commandId:v.commandId,commandTicket:v.commandTicket}; }
export function manifest(v){shape(v,['schemaVersion','apiVersion','uiCompatibilityVersion','assets'],'MANIFEST_SHAPE');check(v.schemaVersion===1&&v.apiVersion===1&&v.uiCompatibilityVersion===1&&Array.isArray(v.assets)&&v.assets.length>0&&v.assets.length<=64,'MANIFEST_VERSION');let total=0;const seen=new Set();for(const a of v.assets){shape(a,['path','sizeBytes','sha256'],'ASSET_SHAPE');check(typeof a.path==='string'&&(a.path==='index.html'||/^assets\/[A-Za-z0-9_][A-Za-z0-9._-]{0,127}\.(js|css|svg|png|jpg|jpeg|webp)$/.test(a.path))&&!seen.has(a.path.toLowerCase())&&Number.isSafeInteger(a.sizeBytes)&&a.sizeBytes>0&&/^[0-9a-f]{64}$/.test(a.sha256),'ASSET_INVALID');seen.add(a.path.toLowerCase());total+=a.sizeBytes;check(total<=5242880,'BUNDLE_BOUND');}check(seen.has('index.html')&&v.assets.some(a=>a.path.endsWith('.js')),'MANIFEST_ENTRY');return v;}
export function validateInit(v){shape(v,['type','marker','cdpUrl','origin','maxMillis','pairingCode','loginName','materialPath','materialName','materialSha256','materialBytes','batchFiles','browserOwnerMarker','downloadPath','oldManifest','newManifest','expectedCsp'],'INIT_SHAPE');check(v.type==='INIT'&&/^FutureClassroom-Test-TLS-[0-9a-f]{32}$/.test(v.marker)&&/^wss?:/.test(v.cdpUrl),'INIT_IDENTITY');const c=new URL(v.cdpUrl),o=new URL(v.origin);check(c.protocol==='ws:'&&['127.0.0.1','[::1]'].includes(c.hostname)&&/^\/devtools\/browser\/[0-9a-fA-F-]{16,80}$/.test(c.pathname)&&!c.username&&!c.password&&!c.search&&!c.hash&&c.port,'CDP_ENDPOINT');check(o.protocol==='https:'&&['localhost','127.0.0.1'].includes(o.hostname)&&o.port&&o.pathname==='/'&&!o.search&&!o.hash&&!o.username&&!o.password&&v.origin===o.origin,'ORIGIN');check(Number.isSafeInteger(v.maxMillis)&&v.maxMillis>0&&v.maxMillis<=120000&&/^[0-9]{8}$/.test(v.pairingCode)&&v.loginName==='synthetic-ordinary-teacher','INIT_BOUNDS');check(typeof v.materialPath==='string'&&/^[A-Z]:\\/.test(v.materialPath)&&!/[\r\n\0]/.test(v.materialPath)&&v.materialPath.endsWith('\\'+v.materialName)&&v.materialPath.split('\\').filter(x=>x===v.marker).length===1&&!v.materialPath.split('\\').some(x=>x==='..'||x==='.')&&v.materialName==='g03-teacher-material.txt'&&v.materialBytes>0&&v.materialBytes<=4096&&/^[0-9a-f]{64}$/.test(v.materialSha256),'MATERIAL_INPUT');check(Array.isArray(v.batchFiles)&&v.batchFiles.length===20,'BATCH_INPUT_COUNT');for(const [index,item]of v.batchFiles.entries()){shape(item,['path','displayName','bytes','sha256'],'BATCH_INPUT_SHAPE');const number=String(index+1).padStart(2,'0'),name='g03-batch-'+number+'.txt',raw=Buffer.from('Synthetic future classroom batch member '+number+'.\n');check(item.displayName===name&&item.path===v.materialPath.slice(0,-v.materialName.length)+name&&item.bytes===raw.length&&item.bytes<=4096&&item.sha256===hash(raw),'FIXED_OWNED_BATCH_INPUT');}check(/^FutureClassroom-Test-Browser-[0-9a-f]{32}$/.test(v.browserOwnerMarker)&&typeof v.downloadPath==='string'&&/^[A-Z]:\\/.test(v.downloadPath)&&v.downloadPath.endsWith('\\'+v.browserOwnerMarker+'\\downloads')&&!v.downloadPath.split('\\').some(x=>x==='..'||x==='.')&&!/[\r\n\0]/.test(v.downloadPath),'OWNED_DOWNLOAD_PATH');manifest(v.oldManifest);manifest(v.newManifest);check(v.oldManifest.assets.some(a=>a.path.endsWith('.js')&&!v.newManifest.assets.some(b=>b.path===a.path&&b.sha256===a.sha256)),'DISTINCT_REAL_BUNDLE_REQUIRED');check(typeof v.expectedCsp==='string'&&v.expectedCsp.length<=2048&&v.expectedCsp.includes("script-src 'self'")&&v.expectedCsp.includes("connect-src 'self'")&&!v.expectedCsp.includes('unsafe-inline')&&!v.expectedCsp.includes('unsafe-eval'),'CSP');return v;}
class Inbox {
  constructor(stream) {
    this.stream = stream; this.buffer = ''; this.queue = []; this.waiter = null; this.received = 0;
    this.ended = false; this.fault = null; this.onFault = () => {};
    stream.setEncoding('utf8');
    this.data = chunk => {
      try {
        this.buffer += chunk;
        check(Buffer.byteLength(this.buffer) <= MAX_LINE * 2, 'INPUT_BUDGET');
        for (let end; (end = this.buffer.indexOf('\n')) !== -1;) {
          const line = this.buffer.slice(0, end).replace(/\r$/, ''); this.buffer = this.buffer.slice(end + 1);
          check(++this.received <= 12 && Buffer.byteLength(line) <= MAX_LINE && line.length > 0, 'INPUT_SEQUENCE');
          let value; try { value = JSON.parse(line); } catch { throw new Fault('INPUT_JSON'); }
          const item = { value, at: performance.now() };
          if (this.waiter) { const waiter = this.waiter; this.waiter = null; waiter.resolve(item); }
          else this.queue.push(item);
        }
        check(Buffer.byteLength(this.buffer) <= MAX_LINE, 'INPUT_BUDGET');
      } catch (error) { this.fail(error); }
    };
    this.end = () => { this.ended = true; if (this.buffer) this.fail(new Fault('INPUT_TRUNCATED')); this.waiter?.reject(new Fault('INPUT_EOF')); this.waiter = null; };
    stream.on('data', this.data); stream.on('end', this.end);
  }
  fail(error) { this.fault = error instanceof Fault ? error : new Fault('INPUT_FAILURE'); this.waiter?.reject(this.fault); this.waiter = null; this.onFault(this.fault); }
  next(until) {
    if (this.fault) return Promise.reject(this.fault);
    if (this.queue.length) return Promise.resolve(this.queue.shift());
    if (this.ended) return Promise.reject(new Fault('INPUT_EOF'));
    return new Promise((resolve, reject) => {
      const ms = until - performance.now(); if (ms <= 0) { reject(new Fault('INPUT_DEADLINE')); return; }
      const timer = setTimeout(() => { this.waiter = null; reject(new Fault('INPUT_DEADLINE')); }, ms);
      this.waiter = { resolve: value => { clearTimeout(timer); resolve(value); }, reject: error => { clearTimeout(timer); reject(error); } };
    });
  }
  close() { this.stream.off('data', this.data); this.stream.off('end', this.end); this.stream.pause(); this.buffer = ''; this.queue.length = 0; }
}

class Cdp {
  constructor(url, event, failed) {
    this.socket = new WebSocket(url); this.nextId = 0; this.pending = new Map(); this.event = event; this.failed = failed;
    this.closed = false; this.opened = false;
    this.socket.addEventListener('message', message => {
      try {
        check(typeof message.data === 'string' && Buffer.byteLength(message.data) <= MAX_CDP, 'CDP_MESSAGE_BUDGET');
        let frame; try { frame = JSON.parse(message.data); } catch { throw new Fault('CDP_JSON'); }
        if (Number.isSafeInteger(frame.id)) {
          const pending = this.pending.get(frame.id); if (!pending) return;
          this.pending.delete(frame.id); clearTimeout(pending.timer);
          if (frame.error) pending.reject(new Fault('CDP_COMMAND_REJECTED', pending.method, frame.error.code));
          else if ((frame.sessionId ?? undefined) !== pending.sessionId) pending.reject(new Fault('CDP_SESSION_MISMATCH', pending.method));
          else pending.resolve(frame.result ?? {});
        } else if (typeof frame.method === 'string') this.event(frame.method, frame.params ?? {}, frame.sessionId);
      } catch (error) { this.failed(error instanceof Fault ? error : new Fault('CDP_EVENT_FAILURE')); }
    });
    this.socket.addEventListener('error', () => this.abort(new Fault('CDP_SOCKET_ERROR')));
    this.socket.addEventListener('close', () => { this.closed = true; this.abort(new Fault('CDP_SOCKET_CLOSED')); });
  }
  open(until) {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { this.socket.close(); reject(new Fault('CDP_CONNECT_DEADLINE')); }, Math.max(1, until - performance.now()));
      const finish = fn => value => { clearTimeout(timer); fn(value); };
      this.socket.addEventListener('open', finish(() => { this.opened = true; resolve(); }), { once: true });
      this.socket.addEventListener('error', finish(() => reject(new Fault('CDP_CONNECT_ERROR'))), { once: true });
      this.socket.addEventListener('close', finish(() => reject(new Fault('CDP_CONNECT_CLOSED'))), { once: true });
    });
  }
  send(method, params, sessionId, until) {
    if (this.closed || this.socket.readyState !== WebSocket.OPEN) return Promise.reject(new Fault('CDP_NOT_OPEN', method));
    const remaining = until - performance.now(); if (remaining <= 0) return Promise.reject(new Fault('RUN_DEADLINE', method));
    if (this.pending.size >= 32) return Promise.reject(new Fault('CDP_PENDING_BUDGET', method));
    return new Promise((resolve, reject) => {
      const id = ++this.nextId;
      const timer = setTimeout(() => { this.pending.delete(id); reject(new Fault('CDP_COMMAND_DEADLINE', method)); }, remaining);
      this.pending.set(id, { resolve, reject, timer, method, sessionId });
      try { this.socket.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) })); }
      catch { clearTimeout(timer); this.pending.delete(id); reject(new Fault('CDP_SEND_FAILED', method)); }
    });
  }
  abort(error) {
    for (const pending of this.pending.values()) { clearTimeout(pending.timer); pending.reject(error); }
    this.pending.clear();
    if (!this.closing) this.failed(error);
  }
  async disconnect() {
    this.closing = true; this.abort(new Fault('CDP_DISCONNECT'));
    if (this.closed) return true;
    return new Promise(resolve => {
      const timer = setTimeout(() => resolve(false), 1000);
      this.socket.addEventListener('close', () => { clearTimeout(timer); resolve(true); }, { once: true });
      try { this.socket.close(); } catch { clearTimeout(timer); resolve(false); }
    });
  }
}


// Read-only geometry in the actual page. Neither this probe nor the caller
// invokes an element handler, changes CSS, or grants permission to a business API.
function pointerTarget() {
  const brief = e => e ? {
    tag: String(e.tagName ?? '').toLowerCase().slice(0, 24),
    id: String(e.id ?? '').replace(/[^A-Za-z0-9_-]/g, '').slice(0, 80),
    classes: String(typeof e.className === 'string' ? e.className : '').replace(/[^A-Za-z0-9_ -]/g, '').slice(0, 128),
  } : null;
  const viewport = {width: window.innerWidth, height: window.innerHeight};
  const r = this.getBoundingClientRect();
  const rect = {x: r.x, y: r.y, width: r.width, height: r.height};
  const base = {connected: this.isConnected, disabled: this.disabled === true, target: brief(this), rect, viewport};
  if (!base.connected || base.disabled || !this.getClientRects().length || r.width <= 0 || r.height <= 0)
    return {...base, point: null, hit: null, hitConfirmed: false};
  const left = Math.max(1, r.left + Math.min(2, r.width / 4));
  const right = Math.min(viewport.width - 1, r.right - Math.min(2, r.width / 4));
  const top = Math.max(1, r.top + Math.min(2, r.height / 4));
  const bottom = Math.min(viewport.height - 1, r.bottom - Math.min(2, r.height / 4));
  if (left >= right || top >= bottom) return {...base, point: null, hit: null, hitConfirmed: false};
  // A visible edge may be usable when a fixed toolbar covers the center. Only
  // a point whose real topmost hit is this element or its descendant is allowed.
  const points = [[.5,.5],[.5,.2],[.5,.8],[.2,.5],[.8,.5],[.2,.2],[.8,.2],[.2,.8],[.8,.8]];
  let lastHit = null;
  for (const [fx, fy] of points) {
    const point = {x: left + (right-left)*fx, y: top + (bottom-top)*fy};
    const hit = document.elementFromPoint(point.x, point.y); lastHit = hit;
    if (hit === this || this.contains(hit)) return {...base, point, hit: brief(hit), hitConfirmed: true};
  }
  return {...base, point: null, hit: brief(lastHit), hitConfirmed: false};
}

export class TeacherHarness {
 constructor(init,inbox){this.i=init;this.inbox=inbox;this.until=performance.now()+init.maxMillis;this.started=performance.now();this.requests=new Map();this.tasks=new Set();this.ledger=[];this.checks={};this.assets=[];this.phase='connect';this.fault=null;this.bundle='old';this.drop=null;this.dropped=null;this.dialog=null;this.eventCount=0;this.native=[];this.pairingCycles=[];this.download=null;this.actionDiagnostics=[];this.responseBodies=new Map();this.responseBodyBytes=0;this.captureResponses=false;this.context=null;this.session=null;this.cdp=new Cdp(init.cdpUrl,(m,p,s)=>this.event(m,p,s),e=>{this.fault=e;});}
 send(m,p={},session=this.session){check(performance.now()<this.until,'ORIGINAL_DEADLINE');return this.cdp.send(m,p,session??undefined,this.until);}
 task(p){check(this.tasks.size<32,'BODY_TASK_BOUND');this.tasks.add(p);p.catch(e=>{this.fault=e instanceof Fault?e:new Fault('EVENT_TASK_FAILED');}).finally(()=>this.tasks.delete(p));}
 event(m,p,s){if(m==='Browser.downloadWillBegin'){check(this.download===null&&p.suggestedFilename===this.i.materialName&&typeof p.url==='string'&&p.url.startsWith('blob:'+this.i.origin+'/')&&/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(p.guid),'OWNED_BLOB_DOWNLOAD');this.download={guid:p.guid,completed:false};return;}if(m==='Browser.downloadProgress'){check(this.download?.guid===p.guid&&Number.isSafeInteger(p.receivedBytes)&&p.receivedBytes<=this.i.materialBytes&&p.totalBytes<=this.i.materialBytes,'OWNED_DOWNLOAD_BOUND');check(p.state!=='canceled','DOWNLOAD_CANCELED');if(p.state==='completed'){check(p.receivedBytes===this.i.materialBytes&&p.totalBytes===this.i.materialBytes,'DOWNLOAD_COMPLETE_SIZE');this.download.completed=true;this.download.bytes=p.receivedBytes;}return;}if(s!==this.session)return;check(++this.eventCount<=65536,'EVENT_BOUND');
  if(m==='Page.javascriptDialogOpening'){check(this.dialog===null&&p.type==='confirm'&&typeof p.message==='string'&&((this.dialogPermit==='navigate'&&p.message.startsWith('本页有未保存草稿'))||(this.dialogPermit==='delete'&&p.message.includes(this.i.materialName))||(this.dialogPermit==='delete-batch'&&p.message==='确认一起删除所选的 20 个资料条目？同名的其他文件不受影响。')||(this.dialogPermit==='voice-stop'&&p.message==='停止这个任务，并取消它尚未播放的剩余遍数？')),'UNEXPECTED_DIALOG');this.dialog=p.type;this.task(this.send('Page.handleJavaScriptDialog',{accept:true}).then(()=>{this.dialog=null;this.dialogPermit=null;}));}
  if(m==='Network.requestWillBeSent'){const u=new URL(p.request.url);if(u.origin!==this.i.origin&&u.protocol!=='blob:')throw new Fault('CROSS_ORIGIN_REQUEST');if(u.protocol==='blob:')return;check(this.requests.size<128,'REQUEST_BOUND');check(!this.requests.has(p.requestId),'REDIRECT_OR_ID_REUSE');const r={ordinal:this.requests.size+1,path:u.pathname,query:u.search,method:p.request.method,bundle:this.bundle,startedAtEvent:this.eventCount,finished:false,status:null};if(u.pathname.startsWith('/fc/api/v1/')){check(['GET','POST'].includes(r.method),'API_METHOD');if(p.request.postData&&u.pathname!=='/fc/api/v1/pairing/requests'&&!u.pathname.endsWith('/poll')&&!u.pathname.startsWith('/fc/api/v1/voice/uploads')&&!/^\/fc\/api\/v1\/materials\/uploads\/[1-9][0-9]*\/bytes$/.test(u.pathname)&&!/^\/fc\/api\/v1\/materials\/batches\/[1-9][0-9]*\/files\/[1-9][0-9]*\/bytes$/.test(u.pathname)){check(Buffer.byteLength(p.request.postData)<=65536,'POST_BOUND');try{r.input=JSON.parse(p.request.postData);}catch{throw new Fault('POST_JSON');}}}this.requests.set(p.requestId,r);}
  if(m==='Network.responseReceived'){const r=this.requests.get(p.requestId);if(r){r.status=p.response.status;r.mime=p.response.mimeType;r.serviceWorker=p.response.fromServiceWorker===true;check(!r.serviceWorker,'SERVICE_WORKER_FORBIDDEN');if(r.path==='/fc/'||r.path==='/fc/index.html'){const entries=Object.entries(p.response.headers).filter(([k])=>k.toLowerCase()==='content-security-policy');check(entries.length===1&&entries[0][1]===this.i.expectedCsp,'ENTRY_CSP');}}}
  if(m==='Network.loadingFinished'){const r=this.requests.get(p.requestId);if(r){r.finished=true;r.finishedAtEvent=this.eventCount;this.task(this.body(p.requestId,r));}}
  if(m==='Network.loadingFailed'){const r=this.requests.get(p.requestId);if(r)r.failed=true;}
  if(m==='Fetch.requestPaused')this.task(this.pause(p));
 }
 responseSpec(path,method,query=''){
  if(method==='POST'&&/^\/fc\/api\/v1\/materials\/uploads\/[1-9][0-9]{0,19}\/bytes$/.test(path)&&query==='')return{status:200,limit:65536};
  if(method==='POST'&&/^\/fc\/api\/v1\/materials\/batches\/[1-9][0-9]{0,19}\/files\/[1-9][0-9]{0,19}\/bytes$/.test(path)&&query==='')return{status:200,limit:65536};
  if(query===''&&((method==='GET'&&/^\/fc\/api\/v1\/materials\/folders(?:\/[1-9][0-9]{0,19}){2}$/.test(path))||(method==='POST'&&/^\/fc\/api\/v1\/materials\/folders(?:\/[1-9][0-9]{0,19}){2}\/(?:append-batch|delete-batch)$/.test(path))||(method==='GET'&&/^\/fc\/api\/v1\/materials\/delete-batch-commands(?:\/[1-9][0-9]{0,19}){2}\/c2:[0-9a-f]{32}:[1-9][0-9]{0,18}\/[1-9][0-9]{0,19}$/.test(path))))return{status:200,limit:65536};
  if(method==='GET'&&/^\/fc\/api\/v1\/materials\/download(?:\/[1-9][0-9]{0,19}){4}$/.test(path)&&query==='')return{status:200,limit:this.i.materialBytes};
  if(method==='POST'&&path==='/fc/api/v1/voice/uploads'&&/^\?classId=[1-9][0-9]{0,19}$/.test(query))return{status:201,limit:65536};
  return null;
 }
 async startResponseCapture(){
  check(!this.captureResponses&&!this.drop&&this.responseBodies.size===0,'RESPONSE_CAPTURE_STATE');
  this.captureResponses=true;
  await this.send('Fetch.enable',{patterns:[
   {urlPattern:this.i.origin+'/fc/api/v1/materials/uploads/*/bytes',requestStage:'Response'},
   {urlPattern:this.i.origin+'/fc/api/v1/materials/batches/*/files/*/bytes',requestStage:'Response'},
   {urlPattern:this.i.origin+'/fc/api/v1/materials/folders/*',requestStage:'Response'},
   {urlPattern:this.i.origin+'/fc/api/v1/materials/delete-batch-commands/*',requestStage:'Response'},
   {urlPattern:this.i.origin+'/fc/api/v1/materials/download/*',requestStage:'Response'},
   {urlPattern:this.i.origin+'/fc/api/v1/voice/uploads?classId=*',requestStage:'Response'},
  ]});
 }
 async stopResponseCapture(){
  await this.wait(()=>this.tasks.size===0&&this.responseBodies.size===0&&this.responseBodyBytes===0,'RESPONSE_CAPTURE_NOT_DRAINED');
  await this.send('Fetch.disable');this.captureResponses=false;
 }
 async pause(p){
  const url=new URL(p.request.url),path=url.pathname;
  check(url.origin===this.i.origin,'FETCH_SAME_ORIGIN');
  if(this.drop&&p.responseStatusCode===200&&p.request.method==='POST'&&path===this.drop){const raw=await this.send('Fetch.getResponseBody',{requestId:p.requestId});const bytes=Buffer.from(raw.body,raw.base64Encoded?'base64':'utf8');check(bytes.length<=65536,'DROPPED_BODY_BOUND');const value=JSON.parse(bytes);check(value.schemaVersion===2&&value.status==='COMMITTED'&&value.content?.text===this.synthetic('HOMEWORK'),'DROP_ACTUAL_COMMIT');const original=pair(value);this.dropped={...original,path,status:200,contentSha256:hash(Buffer.from(value.content.text)),key:value.content.key};this.drop=null;await this.send('Fetch.failRequest',{requestId:p.requestId,errorReason:'Aborted'});}else{
   const spec=this.captureResponses?this.responseSpec(path,p.request.method,url.search):null;
   if(spec&&p.responseStatusCode===spec.status){
    const record=this.requests.get(p.networkId);
    check(typeof p.networkId==='string'&&record&&!record.finished&&!record.failed&&record.path===path&&record.query===url.search&&record.method===p.request.method&&(record.status===null||record.status===p.responseStatusCode),'FETCH_ORIGINAL_NETWORK_BINDING');
    check(!this.responseBodies.has(p.networkId)&&this.responseBodies.size<4,'FETCH_BODY_SLOT_BOUND');
    let raw;try{raw=await this.send('Fetch.getResponseBody',{requestId:p.requestId});}catch(error){error.responseObservation={ordinal:record.ordinal,path,status:p.responseStatusCode,finished:record.finished};throw error;}
    const bytes=Buffer.from(raw.body,raw.base64Encoded?'base64':'utf8');
    check(bytes.length>0&&bytes.length<=spec.limit&&this.responseBodyBytes+bytes.length<=8388608,'FETCH_ORIGINAL_BODY_BOUND');
    // Mark possession of this exact original reply before releasing it to
    // the renderer. Network.loadingFinished may be delivered after the next
    // renderer request, so its observer order cannot establish serial await.
    check(this.requests.get(p.networkId)===record&&!record.finished&&!record.failed&&record.originalReplyObservedAtEvent===undefined,'FETCH_ORIGINAL_REPLY_MARKER_BINDING');
    record.originalReplyObservedAtEvent=++this.eventCount;check(this.eventCount<=65536,'EVENT_BOUND');
    this.responseBodies.set(p.networkId,{record,path,query:url.search,method:p.request.method,status:p.responseStatusCode,bytes,originalReplyObservedAtEvent:record.originalReplyObservedAtEvent});this.responseBodyBytes+=bytes.length;
   }
   // Continue the very same paused response with no URL/header/status/body
   // override. No replay, synthetic result, stream takeover, or missing-body pass.
   await this.send('Fetch.continueRequest',{requestId:p.requestId});
  }
 }
 async body(id,r){if(r.status===null||r.status===204||r.status>=300)return;const path=r.path;let bytes;const observed=this.responseBodies.get(id);if(observed){check(observed.record===r&&r.finished&&!r.failed&&observed.path===path&&observed.query===r.query&&observed.method===r.method&&observed.status===r.status&&observed.originalReplyObservedAtEvent===r.originalReplyObservedAtEvent&&r.startedAtEvent<r.originalReplyObservedAtEvent&&r.originalReplyObservedAtEvent<r.finishedAtEvent,'FETCH_FINISHED_NETWORK_BINDING');bytes=observed.bytes;this.responseBodies.delete(id);this.responseBodyBytes-=bytes.length;}else{const required=this.captureResponses?this.responseSpec(path,r.method,r.query):null;check(!required||required.status!==r.status,'FETCH_REQUIRED_BODY_MISSING');let raw;try{raw=await this.send('Network.getResponseBody',{requestId:id});}catch(error){error.responseObservation={ordinal:r.ordinal,path:r.path,status:r.status,finished:r.finished};throw error;}bytes=Buffer.from(raw.body,raw.base64Encoded?'base64':'utf8');}check(bytes.length<=(path.startsWith('/fc/assets/')?5242880:65536),'BODY_BOUND');if(path.startsWith('/fc/api/v1/')){if(path.includes('/pairing/')){const v=JSON.parse(bytes);check(v.schemaVersion===1&&['PENDING','AUTHORIZED'].includes(v.status),'PAIR_RESPONSE');r.pairStatus=v.status;if(path==='/fc/api/v1/pairing/requests'){check(r.status===202&&/^[1-9][0-9]{0,19}$/.test(v.requestId),'PAIR_REQUEST_RECEIPT');r.pairRequestId=v.requestId;}return;}if(path.startsWith('/fc/api/v1/materials/download/')){r.downloadSha256=hash(bytes);r.downloadBytes=bytes.length;return;}r.value=JSON.parse(bytes);if(r.value.commandTicket){r.pair=pair(r.value);}const item={ordinal:r.ordinal,path:r.path,method:r.method,status:r.status};if(r.pair)Object.assign(item,r.pair);if(r.input?.commandTicket){item.input=pair(r.input);check(r.input.schemaVersion===2,'SUBMIT_SCHEMA');}if(r.value.status==='ISSUED'){check(r.status===201&&r.value.schemaVersion===2&&['content','materials','voice'].includes(r.value.family)&&r.value.expiresInMillis===86400000,'ISSUED_RESPONSE');item.family=r.value.family;}if(this.captureResponses&&this.responseSpec(path,r.method,r.query)&&((r.method==='GET'&&/^\/fc\/api\/v1\/materials\/folders(?:\/[1-9][0-9]{0,19}){2}$/.test(path))||path.endsWith('/append-batch')||path.endsWith('/delete-batch')||path.startsWith('/fc/api/v1/materials/delete-batch-commands/'))){check(observed&&observed.record===r,'MATERIAL_EVIDENCE_ORIGINAL_CAPTURE');item.materialEvidence={value:r.value,startedAtEvent:r.startedAtEvent,originalReplyObservedAtEvent:r.originalReplyObservedAtEvent,finishedAtEvent:r.finishedAtEvent,...(r.input?{input:r.input}:{})};}this.ledger.push(item);}else{const manifest=this.bundle==='old'?this.i.oldManifest:this.i.newManifest;const a=manifest.assets.find(a=>'/fc/'+a.path===path||(a.path==='index.html'&&path==='/fc/'));check(a&&a.sizeBytes===bytes.length&&a.sha256===hash(bytes),'ASSET_HASH');this.assets.push({bundle:r.bundle,path:a.path,sha256:a.sha256,sizeBytes:bytes.length});}}
 async eval(expression){const r=await this.send('Runtime.evaluate',{expression,returnByValue:true});check(!r.exceptionDetails,'DOM_READ_FAILED');return r.result.value;}
 async wait(fn,code){while(performance.now()<this.until){if(this.fault)throw this.fault;const v=await fn();if(v)return v;await sleep(25);}throw new Fault(code);}
 async dom(selector){return this.eval(`(()=>{const e=document.querySelector(${JSON.stringify(selector)});return e?{text:e.textContent,value:e.value,disabled:e.disabled===true}:null})()`);}
 actionDiagnostic(value){if(this.actionDiagnostics.length===12)this.actionDiagnostics.shift();this.actionDiagnostics.push(value);}
 async click(selector,text=null){
  const matches = text===null?'':`&&e.textContent.trim()===${JSON.stringify(text)}`;
  const state = await this.eval(`(()=>{const list=[...document.querySelectorAll(${JSON.stringify(selector)})].filter(e=>true${matches});const editor=document.querySelector('#content-text');return {visibilityState:document.visibilityState??'unknown',hasFocus:typeof document.hasFocus==='function'?document.hasFocus():false,matches:list.length,disabled:list.filter(e=>e.disabled===true).length,visible:list.filter(e=>e.getClientRects().length).length,editor:editor?{readOnly:editor.readOnly===true,invalid:editor.getAttribute('aria-invalid')==='true',focused:document.activeElement===editor,expired:!!document.querySelector('.editor-badge.expired'),receipt:!!document.querySelector('.command-receipt'),history:!!document.querySelector('.historical-result'),merge:!!document.querySelector('.merge-panel')}:null};})()`);
  this.actionDiagnostic({action:'target-state',selector,state});
  const expression=`(()=>{const list=[...document.querySelectorAll(${JSON.stringify(selector)})].filter(e=>!e.disabled&&e.getClientRects().length${matches});return list.length===1?list[0]:null})()`;
  const element=await this.wait(async()=>{const r=await this.send('Runtime.evaluate',{expression,returnByValue:false});check(!r.exceptionDetails,'DOM_ACTION_READ');return r.result.objectId;},'DOM_ACTION_MISSING');
  try{
   await this.send('DOM.scrollIntoViewIfNeeded',{objectId:element});
   for(let attempt=0;attempt<2;attempt++){
    if(attempt)await this.send('Runtime.callFunctionOn',{objectId:element,functionDeclaration:"function(){this.scrollIntoView({block:'center',inline:'center',behavior:'instant'});}",returnByValue:true});
    const result=await this.send('Runtime.callFunctionOn',{objectId:element,functionDeclaration:pointerTarget.toString(),returnByValue:true});
    check(!result.exceptionDetails&&result.result.value,'DOM_RECT');const target=result.result.value;
    this.actionDiagnostic({action:'mouse-hit-test',selector,attempt,...target});
    if(!target.hitConfirmed||!target.point)continue;
    const {x,y}=target.point;check(Number.isFinite(x)&&Number.isFinite(y)&&x>0&&y>0&&x<target.viewport.width&&y<target.viewport.height,'DOM_POINT_INVALID');
    await this.send('Input.dispatchMouseEvent',{type:'mouseMoved',buttons:0,x,y});
    const again=await this.send('Runtime.callFunctionOn',{objectId:element,functionDeclaration:'function(x,y){const hit=document.elementFromPoint(x,y);return this.isConnected&&!this.disabled&&(hit===this||this.contains(hit));}',arguments:[{value:x},{value:y}],returnByValue:true});
    check(!again.exceptionDetails,'DOM_HIT_RECHECK');
    if(again.result.value!==true){this.actionDiagnostic({action:'mouse-hit-changed',selector,attempt});continue;}
    await this.send('Input.dispatchMouseEvent',{type:'mousePressed',button:'left',buttons:1,clickCount:1,x,y});
    await this.send('Input.dispatchMouseEvent',{type:'mouseReleased',button:'left',buttons:0,clickCount:1,x,y});
    return;
   }
   throw new Fault('DOM_ACTION_OCCLUDED');
  }finally{await this.send('Runtime.releaseObject',{objectId:element});}
 }
 async key(key,code,modifiers=0){for(const type of ['keyDown','keyUp'])await this.send('Input.dispatchKeyEvent',{type,key,code,modifiers,windowsVirtualKeyCode:({KeyA:65,Home:36,Enter:13})[code]??0,nativeVirtualKeyCode:({KeyA:65,Home:36,Enter:13})[code]??0});}
 async input(selector,text){await this.click(selector);await this.key('a','KeyA',2);await this.send('Input.insertText',{text});const applied=await this.eval(`(()=>{const e=document.querySelector(${JSON.stringify(selector)});return !!e&&document.activeElement===e&&e.value===${JSON.stringify(text)};})()`);this.actionDiagnostic({action:'input-readback',selector,applied:applied===true});check(applied===true,'DOM_INPUT_NOT_APPLIED');}
 async settle(){await this.wait(async()=>this.tasks.size===0,'BODY_SETTLE');if(this.fault)throw this.fault;}
 mark(name){check(!this.checks[name],'DUPLICATE_CHECK');this.checks[name]=true;}
 async nativeAction(action){check(['MIDNIGHT','CACHE_UPGRADE','REVOKE','VOICE_SETTLED','COMPLETE'].includes(action),'NATIVE_ACTION');process.stdout.write(JSON.stringify({type:'NATIVE',action,marker:this.i.marker})+'\n');const {value}=await this.inbox.next(this.until);check(value?.type==='ACK'&&value.action===action&&value.marker===this.i.marker&&value.status==='PASS','NATIVE_ACK');this.native.push(action);return value;}
 synthetic(kind){return 'G03 '+kind+' '+this.i.marker.slice(-32);}
 api(predicate){return [...this.requests.values()].filter(r=>r.value&&predicate(r));}
 async pairUi(code){this.phase='pair-ui';const firstOrdinal=this.requests.size;await this.wait(()=>this.dom('#pair-code'),'PAIR_FORM');await this.input('#pair-code',code);await this.input('#login-name',this.i.loginName);await this.input('#device-name','G03 isolated Edge');await this.click('button','申请连接');await this.wait(async()=>await this.dom('.request-id')||await this.dom('.teacher-workspace'),'ACTUAL_PAIR_REQUEST');check([...this.requests.values()].filter(r=>r.path==='/fc/api/v1/pairing/requests'&&r.method==='POST').length===(this.bundle==='old'?1:2),'ACTUAL_PAIR_POST');await this.wait(()=>this.dom('.teacher-workspace'),'ACTUAL_NATIVE_APPROVAL');await this.settle();const session=this.api(r=>r.path==='/fc/api/v1/session'&&r.value.principal?.role==='TEACHER_ADMIN').at(-1);check(session&&session.value.principal?.displayName==='Synthetic HTTPS teacher'&&session.value.assignments?.length===1&&session.value.assignments[0].classId==='1'&&session.value.assignments[0].subjectId==='1','ORDINARY_TEACHER_REQUIRED');const request=[...this.requests.values()].find(r=>r.ordinal>firstOrdinal&&r.path==='/fc/api/v1/pairing/requests'&&r.pairRequestId);check(request,'ACTUAL_PAIR_RECEIPT');const polls=[...this.requests.values()].filter(r=>r.ordinal>request.ordinal&&r.path==='/fc/api/v1/pairing/requests/'+request.pairRequestId+'/poll');check(polls.length>0&&polls.some(r=>r.status===200&&r.pairStatus==='AUTHORIZED'),'ACTUAL_PAIR_AUTHORIZED_POLL');this.teacherId=session.value.principal.id;this.pairingCycles.push({cycle:this.pairingCycles.length+1,requestOrdinal:request.ordinal,requestId:request.pairRequestId,authorizedPollOrdinal:polls.find(r=>r.pairStatus==='AUTHORIZED').ordinal,sessionOrdinal:session.ordinal,principalId:this.teacherId,role:'TEACHER_ADMIN',assignments:session.value.assignments});}
 async home(){if(await this.dom('.workspace-top-actions')){const heading=await this.eval('document.querySelector(".workspace-heading h1")?.textContent');if(!heading?.startsWith('你好，')){this.dialogPermit='navigate';await this.click('button','返回工作台');}}}
 async content(kind,unknown=false){this.phase=kind.toLowerCase();await this.home();if(kind==='REMINDER')await this.click('aside.bottom-reminder-action button','管理今日提醒');else await this.click('button','管理今日作业');await this.click('.scope-options .scope-option');await this.wait(()=>this.dom('#content-text'),'CONTENT_READ');await this.settle();const scope=this.api(r=>r.value.status==='SCOPES'&&r.value.kind===kind).at(-1)?.value;check(scope?.total===1&&scope.items.length===1&&scope.items[0].classId==='1'&&(kind==='REMINDER'||scope.items[0].ownerTeacherId===this.teacherId&&scope.items[0].subjectId==='1'),'EXACT_TEACHER_SCOPE');await this.input('#content-text',this.synthetic(kind));if(unknown){const initial=this.api(r=>r.value.status==='CONTENT'&&r.value.key?.kind===kind).at(-1);check(initial,'CONTENT_BASELINE');this.drop=initial.path;await this.send('Fetch.enable',{patterns:[{urlPattern:this.i.origin+this.drop,requestStage:'Response'}]});}const before=this.requests.size;await this.click('button','保存当天内容');if(unknown){await this.wait(async()=>this.dropped&&await this.dom('.command-receipt'),'UNKNOWN_ORIGINAL_PAIR');const writes=[...this.requests.values()].filter(r=>r.ordinal>before&&r.method==='POST'&&r.path===this.dropped.path);check(writes.length===1&&JSON.stringify(pair(writes[0].input))===JSON.stringify(pair(this.dropped)),'ONE_ORIGINAL_WRITE');await this.click('.command-receipt button','手动查询原命令');await this.wait(()=>this.dom('.historical-result'),'HISTORY_RESULT');await this.settle();check(this.api(r=>r.path==='/fc/api/v1/content/commands/'+this.dropped.commandTicket+'/'+this.dropped.commandId&&r.value.content?.text===this.synthetic(kind)).length===1,'EXACT_HISTORY_PAIR');await this.click('.historical-result button','读取当前内容核对');await this.wait(()=>this.dom('.merge-panel'),'CURRENT_READ_MERGE');await this.click('.merge-panel button','采用服务器内容，放弃草稿');await this.send('Fetch.disable');this.mark('unknownOriginalPairReadbackNoReplay');}else await this.wait(async()=>{const n=await this.dom('.content-notice');return n?.text?.includes('白板已确认持久保存。');},'CONTENT_COMMITTED');await this.send('Page.reload',{});await this.wait(()=>this.dom('.teacher-workspace'),'RELOAD_AUTH');await this.home();if(kind==='REMINDER')await this.click('aside.bottom-reminder-action button','管理今日提醒');else await this.click('button','管理今日作业');await this.click('.scope-options .scope-option');await this.wait(async()=>{const d=await this.dom('#content-text');return d?.value===this.synthetic(kind);},'DURABLE_READBACK');await this.settle();const current=this.api(r=>r.value.status==='CONTENT'&&r.value.key?.kind===kind&&r.value.text===this.synthetic(kind)).at(-1);check(current&&current.value.revision!=='0','DURABLE_CURRENT_RESPONSE');this.mark(kind==='HOMEWORK'?'homeworkPersistentCurrent':'reminderPersistentBottomShortcut');}
 async offline(){this.phase='offline';await this.input('#content-text','G03 offline draft');const n=this.requests.size;const prior=(await this.dom('.content-notice'))?.text;await this.send('Network.emulateNetworkConditions',{offline:true,latency:0,downloadThroughput:0,uploadThroughput:0});try{await this.click('button','保存当天内容');await this.wait(async()=>{const d=await this.dom('.content-notice');return !!d?.text&&d.text!==prior;},'OFFLINE_NOTICE');}finally{await this.send('Network.emulateNetworkConditions',{offline:false,latency:0,downloadThroughput:-1,uploadThroughput:-1});}const count=()=>[...this.requests.values()].filter(r=>r.ordinal>n&&r.method==='POST'&&r.status===200).length;await sleep(300);await this.settle();check(count()===0&&(await this.dom('#content-text'))?.value==='G03 offline draft','OFFLINE_NO_AUTOREPLAY');this.mark('offlineDraftRetainedNoReplay');}
 async materials(){this.phase='materials';await this.home();await this.click('button','管理课堂资料');await this.click('.scope-options .scope-option');await this.wait(async()=>{const d=await this.dom('#material-file');return d&&!d.disabled;},'MATERIAL_SCOPE');await this.settle();const scope=this.api(r=>r.value.status==='MATERIAL_SCOPES').at(-1)?.value;check(scope?.total===1&&scope.items[0].ownerTeacherId===this.teacherId&&scope.items[0].subjectId==='1','OWN_TEACHER_MATERIAL_SCOPE');const {root}=await this.send('DOM.getDocument');const {nodeId}=await this.send('DOM.querySelector',{nodeId:root.nodeId,selector:'#material-file'});check(nodeId>0,'FILE_INPUT_NODE');await this.send('DOM.setFileInputFiles',{nodeId,files:[this.i.materialPath]});await this.wait(()=>this.dom('.material-picked'),'MATERIAL_PICK');const start=this.requests.size;await this.click('button','上传文件');await this.wait(async()=>this.api(r=>r.ordinal>start&&r.value.status==='UPLOADED').length===1,'FIRST_UPLOAD_COMPLETE_NOT_PUBLISHED');check(await this.eval('[...document.querySelectorAll("button")].find(e=>e.textContent.trim()==="校验并添加到课堂资料")?.disabled===true'),'NO_CLASS_NO_APPEND');await this.click('button','取消上传');await this.wait(async()=>this.api(r=>r.ordinal>start&&r.value.status==='UPLOAD_CANCELLED').length===1,'ORIGINAL_UPLOAD_CANCEL_CONFIRMED');await this.settle();check(!this.api(r=>r.ordinal>start&&(r.value.status==='ISSUED'||r.path.endsWith('/append'))).length,'CANCEL_NO_ISSUE_OR_APPEND');this.mark('materialsUnpublishedUploadCancelNoAppend');const second=this.requests.size;await this.click('button','上传文件');await this.wait(async()=>this.api(r=>r.ordinal>second&&r.value.status==='UPLOADED').length===1,'SECOND_UPLOAD_COMPLETE');await this.click('.material-classes input[type="checkbox"]');const append=this.requests.size;await this.click('button','校验并添加到课堂资料');await this.wait(async()=>this.api(r=>r.ordinal>append&&r.path.endsWith('/append')&&r.value.status==='COMMITTED').length===1,'MATERIAL_APPEND_COMMITTED');await this.wait(()=>this.dom('.historical-result'),'APPEND_HISTORY_UI');this.dialogPermit='navigate';await this.click('button','重新读取目录');await this.wait(()=>this.dom('.material-list .material-row'),'APPEND_CURRENT_READ');await this.settle();const rec=this.api(r=>r.value.status==='MATERIALS'&&r.value.items?.some(x=>x.displayName===this.i.materialName)).at(-1)?.value.items.find(x=>x.displayName===this.i.materialName);check(rec?.sha256===this.i.materialSha256&&rec.bytes===this.i.materialBytes&&rec.allowedClassIds?.length===1&&rec.allowedClassIds[0]==='1','MATERIAL_DURABLE_LEDGER');await this.click('button[aria-label='+JSON.stringify('下载 '+this.i.materialName)+']');await this.wait(async()=>this.download?.completed&&[...this.requests.values()].some(r=>r.path.startsWith('/fc/api/v1/materials/download/')&&r.downloadSha256===this.i.materialSha256&&r.downloadBytes===this.i.materialBytes),'MATERIAL_ACTUAL_BROWSER_DOWNLOAD');this.dialogPermit='delete';const remove=this.requests.size;await this.click('button[aria-label='+JSON.stringify('删除 '+this.i.materialName)+']');await this.wait(async()=>this.api(r=>r.ordinal>remove&&r.path.endsWith('/delete')&&r.value.status==='COMMITTED').length===1,'MATERIAL_DELETE_COMMITTED');const current=this.requests.size;this.dialogPermit='navigate';await this.click('button','重新读取目录');await this.wait(async()=>this.api(r=>r.ordinal>current&&r.value.status==='MATERIALS'&&r.value.items?.length===0).length===1,'MATERIAL_DELETE_DURABLE');check(!(await this.dom('.material-list .material-row')),'MATERIAL_DELETE_CURRENT_DOM');this.mark('materialsAppendDownloadDeleteReadback');}

 async materialsBatch(){
  this.phase='materials-batch';await this.home();await this.click('button','管理课堂资料');await this.click('.scope-options .scope-option');
  await this.wait(async()=>{const d=await this.dom('#material-file');return d&&!d.disabled;},'BATCH_SCOPE');await this.settle();
  const scope=this.api(r=>r.value.status==='MATERIAL_SCOPES').at(-1)?.value;
  check(scope?.total===1&&scope.items[0].ownerTeacherId===this.teacherId&&scope.items[0].subjectId==='1','BATCH_OWN_TEACHER_SCOPE');
  const initial=this.api(r=>r.value.status==='MATERIALS').at(-1)?.value;check(initial&&initial.total===0&&initial.items.length===0,'BATCH_INITIAL_EMPTY_CURRENT');
  const {root}=await this.send('DOM.getDocument');const {nodeId}=await this.send('DOM.querySelector',{nodeId:root.nodeId,selector:'#material-file'});
  check(nodeId>0&&await this.eval('document.querySelector("#material-file")?.multiple===true'),'REAL_MULTIPLE_INPUT');
  const names=()=>this.eval('[...document.querySelectorAll("section[aria-label=\\"本次文件顺序\\"] li strong")].map(e=>e.textContent)');
  const beforeReview=this.requests.size;await this.send('DOM.setFileInputFiles',{nodeId,files:this.i.batchFiles.map(f=>f.path)});
  await this.wait(async()=>(await names()).length===20,'BATCH_TWENTY_SELECTED');await this.click('button[aria-label="移除第20个文件"]');
  await this.wait(async()=>(await names()).length===19,'BATCH_REVIEW_REMOVAL');
  await this.send('DOM.setFileInputFiles',{nodeId,files:[]});await this.send('DOM.setFileInputFiles',{nodeId,files:this.i.batchFiles.map(f=>f.path)});
  await this.wait(async()=>(await names()).length===20,'BATCH_TWENTY_RESELECTED');
  await this.click('button[aria-label="下移第1个文件"]');await this.click('button[aria-label="上移第3个文件"]');
  const ordered=[this.i.batchFiles[1],this.i.batchFiles[2],this.i.batchFiles[0],...this.i.batchFiles.slice(3)];
  check(JSON.stringify(await names())===JSON.stringify(ordered.map(f=>f.displayName)),'BATCH_CONFIRMED_DOM_ORDER');
  await this.click('.material-classes input[type="checkbox"]');
  check(![...this.requests.values()].some(r=>r.ordinal>beforeReview&&r.method==='POST'),'REVIEW_NO_UPLOAD_OR_COMMAND');this.mark('batchTwentyReviewRemoveAndConfirmedOrder');
  const uploadStart=this.requests.size;await this.click('button','确认顺序并上传整批');
  await this.wait(async()=>this.api(r=>r.ordinal>uploadStart&&r.value.status==='MATERIAL_BATCH_MEMBER_UPLOADED').length===20,'BATCH_TWENTY_SERIAL_UPLOADS');await this.settle();
  const ready=this.api(r=>r.ordinal>uploadStart&&r.value.status==='MATERIAL_BATCH_READY');check(ready.length===1&&ready[0].status===201,'ONE_ORIGINAL_BATCH');
  const group=ready[0].value;shape(group,['schemaVersion','status','batchId','expiresInSeconds','maxChunkBytes','items'],'BATCH_READY_EXACT');
  check(group.schemaVersion===1&&group.expiresInSeconds===300&&group.maxChunkBytes===65536&&group.items.length===20&&/^[1-9][0-9]{0,19}$/.test(group.batchId),'BATCH_READY_METADATA');
  const sent=ready[0].input;shape(sent,['schemaVersion','files','allowedClassIds'],'BATCH_CREATE_EXACT');
  check(sent.schemaVersion===1&&JSON.stringify(sent.files)===JSON.stringify(ordered.map(f=>({displayName:f.displayName,bytes:f.bytes})))&&JSON.stringify(sent.allowedClassIds)==='["1"]','BATCH_CREATE_ORIGINAL_ORDER_CLASSES');
  const uploads=this.api(r=>r.ordinal>uploadStart&&r.value.status==='MATERIAL_BATCH_MEMBER_UPLOADED');const uploadIds=new Set();
  for(const [index,r]of uploads.entries()){
   const item=group.items[index],file=ordered[index];shape(item,['uploadId','displayName','bytes'],'BATCH_READY_MEMBER_EXACT');
   check(/^[1-9][0-9]{0,19}$/.test(item.uploadId)&&!uploadIds.has(item.uploadId)&&item.displayName===file.displayName&&item.bytes===file.bytes,'BATCH_READY_MEMBER_BINDING');uploadIds.add(item.uploadId);
   shape(r.value,['schemaVersion','status','batchId','uploadId','bytes'],'BATCH_UPLOAD_EXACT');
   check(r.status===200&&r.method==='POST'&&r.path==='/fc/api/v1/materials/batches/'+group.batchId+'/files/'+item.uploadId+'/bytes'&&r.value.schemaVersion===1&&r.value.batchId===group.batchId&&r.value.uploadId===item.uploadId&&r.value.bytes===file.bytes,'BATCH_ORIGINAL_UPLOAD_ACK');
   check(Number.isSafeInteger(r.originalReplyObservedAtEvent)&&r.startedAtEvent<r.originalReplyObservedAtEvent&&r.originalReplyObservedAtEvent<r.finishedAtEvent,'BATCH_ORIGINAL_REPLY_FINISHED');
   if(index)check(r.ordinal>uploads[index-1].ordinal&&uploads[index-1].originalReplyObservedAtEvent<r.startedAtEvent,'BATCH_SERIAL_ORIGINAL_REPLY_ORDER');
  }
  check(!this.api(r=>r.ordinal>uploadStart&&(r.value.status==='ISSUED'||r.path.endsWith('/append-batch')||r.path.endsWith('/append'))).length,'UPLOAD_NO_ISSUE_OR_COMMIT');
  check(await this.eval('document.querySelector("#material-file")?.disabled===true&&[...document.querySelectorAll(".material-classes input")].every(e=>e.disabled||e.closest("fieldset")?.disabled)'),'UPLOADED_ORDER_CLASSES_FROZEN');this.mark('batchTwentySerialPreparedNoEarlyCommit');
  const appendStart=this.requests.size;await this.click('button','校验并整批添加');await this.wait(async()=>this.api(r=>r.ordinal>appendStart&&r.value.status==='BATCH_COMMITTED').length===1,'BATCH_ONE_COMMIT');
  await this.wait(()=>this.dom('.historical-result'),'BATCH_HISTORY_DOM');await this.settle();
  const issued=this.api(r=>r.ordinal>appendStart&&r.value.status==='ISSUED'),commits=this.api(r=>r.ordinal>appendStart&&r.path.endsWith('/append-batch'));
  check(issued.length===1&&issued[0].value.family==='materials'&&commits.length===1,'ONE_BATCH_C2_ISSUE_AND_APPEND');const commit=commits[0],original=pair(issued[0].value);
  shape(commit.input,['schemaVersion','commandId','commandTicket','batchId','uploadIds','allowedClassIds'],'BATCH_APPEND_EXACT');
  check(commit.input.schemaVersion===2&&JSON.stringify(pair(commit.input))===JSON.stringify(original)&&commit.input.batchId===group.batchId&&JSON.stringify(commit.input.uploadIds)===JSON.stringify(group.items.map(f=>f.uploadId))&&JSON.stringify(commit.input.allowedClassIds)==='["1"]','BATCH_APPEND_ORIGINAL_METADATA');
  const records=commit.value.records;shape(commit.value,['schemaVersion','status','commandId','commandTicket','scope','records'],'BATCH_COMMIT_EXACT');
  check(commit.status===200&&commit.value.schemaVersion===2&&JSON.stringify(pair(commit.value))===JSON.stringify(original)&&commit.value.scope.ownerTeacherId===this.teacherId&&commit.value.scope.subjectId==='1'&&Array.isArray(records)&&records.length===20,'BATCH_COMMIT_PAIR_SCOPE');
  const materialIds=new Set(),resourceIds=new Set();for(const [index,r]of records.entries()){
   shape(r,['materialId','folderId','displayName','resourceVersionId','kind','bytes','sha256','allowedClassIds','addedAt','addedSequence','status','revision'],'BATCH_RECORD_EXACT');const f=ordered[index];
   check(r.displayName===f.displayName&&r.resourceVersionId===group.items[index].uploadId&&r.bytes===f.bytes&&r.sha256===f.sha256&&r.kind==='TXT'&&r.status==='PUBLISHED'&&r.revision==='1'&&r.folderId===initial.folderId&&r.addedAt===records[0].addedAt&&JSON.stringify(r.allowedClassIds)==='["1"]'&&/^[1-9][0-9]{0,19}$/.test(r.materialId)&&/^[1-9][0-9]{0,19}$/.test(r.addedSequence)&&BigInt(r.addedSequence)===BigInt(records[0].addedSequence)+BigInt(index)&&!materialIds.has(r.materialId)&&!resourceIds.has(r.resourceVersionId),'BATCH_ALL_TWENTY_BYTE_HASH_ORDER_LEDGER');materialIds.add(r.materialId);resourceIds.add(r.resourceVersionId);
  }
  check(!(await this.dom('.material-list .material-row')),'HISTORY_DOES_NOT_REPLACE_CURRENT_PAGE');
  const queryStart=this.requests.size;await this.click('.command-receipt button','手动查询这批结果');await this.wait(async()=>this.api(r=>r.ordinal>queryStart&&r.value.status==='COMMAND_BATCH_RESULT').length===1,'ORIGINAL_BATCH_HISTORY_QUERY');await this.settle();
  const history=this.api(r=>r.ordinal>queryStart&&r.value.status==='COMMAND_BATCH_RESULT')[0];shape(history.value,['schemaVersion','status','commandId','commandTicket','state','scope','records'],'BATCH_HISTORY_EXACT');
  check(history.path==='/fc/api/v1/materials/batch-commands/'+this.teacherId+'/1/'+original.commandTicket+'/'+original.commandId&&history.value.schemaVersion===2&&history.value.state==='COMMITTED'&&JSON.stringify(pair(history.value))===JSON.stringify(original)&&JSON.stringify(history.value.scope)===JSON.stringify(commit.value.scope)&&JSON.stringify(history.value.records)===JSON.stringify(records),'BATCH_HISTORY_ORIGINAL_PAIR_RECORDS');this.mark('batchOneAtomicCommitAndOriginalHistory');
  const currentStart=this.requests.size;this.dialogPermit='navigate';await this.click('button','重新读取目录');await this.wait(async()=>this.api(r=>r.ordinal>currentStart&&r.value.status==='MATERIALS'&&r.value.total===20).length===1,'BATCH_CURRENT_DIRECTORY_TWENTY');await this.settle();
  const current=this.api(r=>r.ordinal>currentStart&&r.value.status==='MATERIALS').at(-1).value;check(current.offset===0&&current.nextCursor===null&&BigInt(current.folderRevision)===BigInt(initial.folderRevision)+1n&&JSON.stringify(current.items)===JSON.stringify(records.slice().reverse()),'BATCH_ONE_FOLDER_REVISION_AND_DESCENDING_CURRENT');
  const visible=await this.eval('[...document.querySelectorAll(".material-list .material-row h3")].map(e=>e.textContent)');check(JSON.stringify(visible)===JSON.stringify(ordered.map(f=>f.displayName).reverse()),'BATCH_CURRENT_DOM_ALL_TWENTY');
  check(this.api(r=>r.ordinal>uploadStart&&r.value.status==='ISSUED').length===1&&this.api(r=>r.ordinal>uploadStart&&r.path.endsWith('/append-batch')).length===1&&!this.api(r=>r.ordinal>uploadStart&&r.path.endsWith('/append')).length,'BATCH_NO_SINGLE_OR_AUTOREPLAY');
  this.batchObservation={batchId:group.batchId,...original,confirmedNames:ordered.map(f=>f.displayName),uploadIds:group.items.map(f=>f.uploadId),materialIds:records.map(r=>r.materialId),sha256s:ordered.map(f=>f.sha256),memberCount:20,removedBeforeReselect:1,uploadOrder:uploads.map(r=>({requestOrdinal:r.ordinal,startedAtEvent:r.startedAtEvent,originalReplyObservedAtEvent:r.originalReplyObservedAtEvent,finishedAtEvent:r.finishedAtEvent})),firstAddedSequence:records[0].addedSequence,lastAddedSequence:records[19].addedSequence,folderRevisionBefore:initial.folderRevision,folderRevisionAfter:current.folderRevision,historyOrdinal:history.ordinal,commitOrdinal:commit.ordinal,currentOrdinal:this.api(r=>r.ordinal>currentStart&&r.value.status==='MATERIALS').at(-1).ordinal};this.mark('batchCurrentDirectoryTwentyNewestLastConfirmed');
  this.phase='materials-batch-delete';const reselectionStart=this.requests.size;this.dialogPermit='navigate';await this.click('.scope-options .scope-option');
  await this.wait(async()=>this.api(r=>r.ordinal>reselectionStart&&r.value.status==='MATERIALS'&&r.value.total===20).length===1,'DELETE_RESELECT_SAME_SCOPE');await this.settle();
  const reselected=this.api(r=>r.ordinal>reselectionStart&&r.value.status==='MATERIALS')[0];
  check(JSON.stringify(reselected.value)===JSON.stringify(current)&&!(await this.dom('.command-receipt'))&&!(await this.dom('.historical-result')),'DELETE_ORIGINAL_IMPORT_HISTORY_EXPLICITLY_LEFT');
  const selectedCount=()=>this.eval('[...document.querySelectorAll(".material-list .material-row input[type=checkbox]")].filter(e=>e.checked).length');
  const beforeSelection=this.requests.size;await this.click('button','全选本页');await this.wait(async()=>await selectedCount()===20,'DELETE_TWENTY_SELECTED');
  const selector='.material-list article.material-row:first-of-type input[type="checkbox"]';await this.click(selector);await this.wait(async()=>await selectedCount()===19,'DELETE_ONE_REMOVED');await this.click(selector);await this.wait(async()=>await selectedCount()===20,'DELETE_FIXED_ITEM_RESELECTED');
  const selected=[...current.items.slice(1),current.items[0]],expectedNames=selected.map(r=>r.displayName+' · '+r.addedAt.replace('T',' '));
  check(JSON.stringify(await this.eval(`[...document.querySelectorAll('section[aria-label="批量删除资料"] ol li')].map(e=>e.textContent)` ))===JSON.stringify(expectedNames),'DELETE_CONFIRMATION_ORIGINAL_NAMES_TIMES_ORDER');
  check(this.requests.size===beforeSelection&&!this.api(r=>r.ordinal>reselectionStart&&r.method==='POST').length,'DELETE_REVIEW_NO_WRITE');this.mark('batchDeleteTwentyReviewFixedSelection');
  const deleteStart=this.requests.size;this.dialogPermit='delete-batch';await this.click('button','删除所选资料');
  await this.wait(async()=>this.api(r=>r.ordinal>deleteStart&&r.value.status==='BATCH_DELETE_COMMITTED').length===1,'DELETE_ONE_COMMIT');await this.wait(()=>this.dom('.historical-result'),'DELETE_HISTORY_DOM');await this.settle();
  const deletionIssues=this.api(r=>r.ordinal>deleteStart&&r.value.status==='ISSUED'),deletions=this.api(r=>r.ordinal>deleteStart&&r.path.endsWith('/delete-batch'));
  check(deletionIssues.length===1&&deletionIssues[0].value.family==='materials'&&deletions.length===1,'DELETE_ONE_C2_AND_POST');const deletion=deletions[0],deletePair=pair(deletionIssues[0].value);
  shape(deletion.input,['schemaVersion','commandId','commandTicket','targets'],'DELETE_POST_EXACT');shape(deletion.value,['schemaVersion','status','commandId','commandTicket','scope','records'],'DELETE_REPLY_EXACT');
  const targets=selected.map(r=>({materialId:r.materialId,resourceVersionId:r.resourceVersionId,expectedMaterialRevision:r.revision})),deleted=selected.map(r=>({...r,status:'DELETED',revision:(BigInt(r.revision)+1n).toString()}));
  check(deletion.status===200&&deletion.input.schemaVersion===2&&deletion.value.schemaVersion===2&&JSON.stringify(pair(deletion.input))===JSON.stringify(deletePair)&&JSON.stringify(pair(deletion.value))===JSON.stringify(deletePair)&&JSON.stringify(deletion.value.scope)===JSON.stringify(current.scope)&&JSON.stringify(deletion.input.targets)===JSON.stringify(targets)&&JSON.stringify(deletion.value.records)===JSON.stringify(deleted),'DELETE_ALL_ORIGINAL_RECORDS_ONLY_STATUS_REVISION');
  const deleteQueryStart=this.requests.size;await this.click('.command-receipt button','手动查询这次删除结果');await this.wait(async()=>this.api(r=>r.ordinal>deleteQueryStart&&r.value.status==='COMMAND_BATCH_DELETE_RESULT').length===1,'DELETE_ORIGINAL_PAIR_HISTORY');await this.settle();
  const deleteHistory=this.api(r=>r.ordinal>deleteQueryStart&&r.value.status==='COMMAND_BATCH_DELETE_RESULT')[0];shape(deleteHistory.value,['schemaVersion','status','commandId','commandTicket','state','scope','records'],'DELETE_HISTORY_EXACT');
  check(deleteHistory.path==='/fc/api/v1/materials/delete-batch-commands/'+this.teacherId+'/1/'+deletePair.commandTicket+'/'+deletePair.commandId&&deleteHistory.value.schemaVersion===2&&deleteHistory.value.state==='COMMITTED'&&JSON.stringify(pair(deleteHistory.value))===JSON.stringify(deletePair)&&JSON.stringify(deleteHistory.value.scope)===JSON.stringify(current.scope)&&JSON.stringify(deleteHistory.value.records)===JSON.stringify(deleted),'DELETE_ORIGINAL_PAIR_IMMUTABLE_HISTORY');this.mark('batchDeleteOneAtomicCommitAndOriginalHistory');
  const deleteCurrentStart=this.requests.size;this.dialogPermit='navigate';await this.click('button','重新读取目录');await this.wait(async()=>this.api(r=>r.ordinal>deleteCurrentStart&&r.value.status==='MATERIALS'&&r.value.total===0).length===1,'DELETE_CURRENT_EMPTY');await this.settle();
  const deleteCurrent=this.api(r=>r.ordinal>deleteCurrentStart&&r.value.status==='MATERIALS')[0];check(deleteCurrent.value.folderId===current.folderId&&BigInt(deleteCurrent.value.folderRevision)===BigInt(current.folderRevision)+1n&&deleteCurrent.value.items.length===0&&deleteCurrent.value.offset===0&&deleteCurrent.value.nextCursor===null&&!(await this.dom('.material-list .material-row')),'DELETE_ONE_REVISION_NO_CURRENT_ROWS');
  check(this.api(r=>r.ordinal>deleteStart&&r.value.status==='ISSUED').length===1&&this.api(r=>r.ordinal>deleteStart&&r.path.endsWith('/delete-batch')).length===1&&!this.api(r=>r.ordinal>deleteStart&&r.path.endsWith('/delete')).length,'DELETE_NO_SINGLE_OR_REPLAY');
  this.batchDeleteObservation={...deletePair,memberCount:20,removedBeforeReselect:1,removedMaterialId:current.items[0].materialId,confirmedMaterialIds:selected.map(r=>r.materialId),sourceCurrentOrdinal:this.batchObservation.currentOrdinal,reselectionOrdinal:reselected.ordinal,issueOrdinal:deletionIssues[0].ordinal,commitOrdinal:deletion.ordinal,historyOrdinal:deleteHistory.ordinal,currentOrdinal:deleteCurrent.ordinal,folderRevisionBefore:current.folderRevision,folderRevisionAfter:deleteCurrent.value.folderRevision};this.mark('batchDeleteCurrentDirectoryEmptyOneRevision');
 }
 async voice(){this.phase='voice';await this.home();await this.click('button','打开语音播报');await this.wait(()=>this.dom('#voice-repeat'),'VOICE_CAPABILITIES');await this.click('#voice-repeat');await this.key('Home','Home');await this.key('Enter','Enter');await this.click('button','点击开始录音');await this.wait(async()=>{const d=await this.dom('.voice-hold');return d?.text?.includes('松开发送');},'ACTUAL_CAPTURE_STARTED');await this.click('button','取消录音');await this.wait(async()=>{const d=await this.dom('.voice-hold');return d?.text?.includes('按住说话');},'CAPTURE_CANCEL');check(this.api(r=>r.path==='/fc/api/v1/voice/broadcasts'&&r.method==='POST').length===0,'CANCEL_NO_JOB');await this.click('button','点击开始录音');await this.wait(async()=>{const d=await this.dom('.voice-hold');return d?.text?.includes('松开发送');},'SECOND_CAPTURE_STARTED');await sleep(1100);await this.click('button','结束并发送');await this.wait(async()=>this.api(r=>r.path==='/fc/api/v1/voice/broadcasts'&&r.method==='POST'&&r.value.status==='COMMITTED').length===1,'VOICE_SUBMIT');const submit=this.api(r=>r.path==='/fc/api/v1/voice/broadcasts'&&r.method==='POST').at(-1);check(submit.value.job?.commandTicket===submit.input.commandTicket&&submit.value.job?.jobId===submit.input.commandId,'VOICE_ORIGINAL_JOB_BINDING');const stop=await this.eval('[...document.querySelectorAll(".voice-job-list button")].filter(e=>!e.disabled).map(e=>e.textContent.trim())');if(stop.length){check(stop.length===1,'ONE_CURRENT_VOICE_JOB');this.dialogPermit='voice-stop';await this.click('.voice-job-list button',stop[0]);}await this.wait(async()=>{const d=await this.dom('.voice-job-list');return /驱动播放完成|已取消|播放失败|已过期|已中断/.test(d?.text??'')&&!/正在等待驱动归还/.test(d?.text??'');},'VOICE_DRIVER_TERMINAL');const prior=this.requests.size;await this.click('button','核对目标与任务');await this.wait(async()=>this.api(r=>r.ordinal>prior&&r.value.status==='VOICE_JOBS'&&r.value.jobs?.some(j=>j.jobId===submit.input.commandId&&j.commandTicket===submit.input.commandTicket&&['COMPLETED','CANCELED','FAILED','EXPIRED','INTERRUPTED'].includes(j.state))).length===1,'VOICE_DURABLE_CURRENT_ORIGINAL_JOB');const ack=await this.nativeAction('VOICE_SETTLED');check(ack.activeJobs===0&&ack.audioOwnerReturned===true,'VOICE_PHYSICAL_OWNER_REQUIRED');this.mark('voiceSyntheticCaptureSubmitCurrentAndPhysicalReturn');}
 async midnight(){this.phase='midnight';await this.home();await this.click('button','管理今日作业');await this.click('.scope-options .scope-option');await this.wait(()=>this.dom('#content-text'),'MIDNIGHT_OLD_CONTENT');await this.input('#content-text','G03 old day unsent');const old=this.api(r=>r.value.status==='CONTENT'&&r.value.key?.kind==='HOMEWORK').at(-1).value;const ack=await this.nativeAction('MIDNIGHT');check(ack.clockScope==='ISOLATED_CIVIL_ONLY'&&ack.oldDate===old.today&&ack.newDate!==old.today&&ack.osClockChanged===false,'MIDNIGHT_NATIVE_BOUND');await this.click('button','保存当天内容');await this.wait(async()=>{const d=await this.dom('.editor-badge');return d?.text==='旧草稿已过期';},'OLD_DATE_REJECT');const after=this.requests.size;await sleep(250);check(![...this.requests.values()].some(r=>r.ordinal>after&&r.method==='POST'&&r.path.startsWith('/fc/api/v1/content/')&&r.status===200),'MIDNIGHT_NO_AUTORETAG');this.dialogPermit='navigate';await this.click('button','重新加载范围');await this.click('.scope-options .scope-option');await this.wait(async()=>{const d=await this.dom('#content-text');return d?.value==='';},'NEW_DAY_CURRENT_EMPTY_DOM');await this.settle();const current=this.api(r=>r.value.status==='CONTENT'&&r.value.key?.kind==='HOMEWORK'&&r.value.today===ack.newDate).at(-1)?.value;check(current?.key.date===ack.newDate&&current.revision==='0'&&current.text==='','NEW_DAY_SERVER_CURRENT_READ');check(!this.api(r=>r.ordinal>after&&r.method==='POST').length,'NEW_DAY_NO_AUTOMATIC_WRITE');this.midnightObservation={oldDate:old.today,newDate:current.today,newContentEmpty:true,oldDraftRejected:true};this.mark('isolatedMidnightRejectsOldDraft');}
 async cacheUpgrade(){this.phase='cache-upgrade';const oldAssets=this.assets.filter(a=>a.bundle==='old'&&a.path.endsWith('.js'));check(oldAssets.length>0,'OLD_REAL_BUNDLE_LOADED');const ack=await this.nativeAction('CACHE_UPGRADE');check(ack.oldOwnerDrained===true&&ack.sameOrigin===this.i.origin&&/^[0-9]{8}$/.test(ack.pairingCode),'CACHE_NATIVE_SWITCH');const before=this.requests.size;await this.home();await this.click('button','打开语音播报');await this.wait(async()=>!(await this.dom('.teacher-workspace')),'OLD_SESSION_AFTER_REAL_RESTART');check([...this.requests.values()].some(r=>r.ordinal>before&&[401,403].includes(r.status)),'OLD_SESSION_AUTH_REJECT');this.bundle='new';await this.send('Page.reload');await this.pairUi(ack.pairingCode);await this.click('button','打开语音播报');await this.wait(()=>this.dom('#voice-repeat'),'NEW_LAZY_VOICE_CHUNK');await this.settle();check(this.assets.some(a=>a.bundle==='new'&&a.path==='index.html')&&this.assets.some(a=>a.bundle==='new'&&a.path.endsWith('.js'))&&oldAssets.some(a=>!this.assets.some(b=>b.bundle==='new'&&b.path===a.path&&b.sha256===a.sha256)),'REAL_CHANGED_BUNDLE_LOADED');this.mark('sameOriginDistinctBundleEntryAndLazyChunk');}
 async revoke(){this.phase='revocation';await this.home();await this.click('button','管理今日作业');await this.click('.scope-options .scope-option');await this.wait(()=>this.dom('#content-text'),'REVOKE_READY');await this.input('#content-text','G03 unsent revoked draft');const n=this.requests.size;const ack=await this.nativeAction('REVOKE');check(ack.actualIdentityWrite===true&&ack.teacherLogin===this.i.loginName,'REAL_NATIVE_REVOCATION');this.dialogPermit='navigate';await this.click('button','核对会话');await this.wait(async()=>!(await this.dom('.teacher-workspace')),'REVOKED_WORKSPACE_CLEARED');await this.settle();check([...this.requests.values()].some(r=>r.ordinal>n&&[401,403].includes(r.status))&&!this.api(r=>r.ordinal>n&&r.method==='POST'&&r.value.status==='ISSUED').length,'REVOKE_NO_NEW_TICKET');this.mark('revokedTeacherClearsWorkspaceNoReplay');}
 async run(){try{await this.cdp.open(this.until);const c=await this.send('Target.createBrowserContext',{disposeOnDetach:true},null);this.context=c.browserContextId;const t=await this.send('Target.createTarget',{url:'about:blank',browserContextId:this.context,background:true},null);const a=await this.send('Target.attachToTarget',{targetId:t.targetId,flatten:true},null);this.session=a.sessionId;await this.send('Network.enable',{maxTotalBufferSize:8388608,maxResourceBufferSize:5242880,maxPostDataSize:65536});await this.send('Page.enable');await this.send('Runtime.enable');await this.send('Page.bringToFront');await this.send('Browser.setDownloadBehavior',{behavior:'allowAndName',downloadPath:this.i.downloadPath,eventsEnabled:true,browserContextId:this.context},null);await this.send('Page.navigate',{url:this.i.origin+'/fc/'});await this.pairUi(this.i.pairingCode);this.mark('actualPairUiOrdinaryTeacher');await this.startResponseCapture();await this.materials();await this.stopResponseCapture();await this.cacheUpgrade();await this.content('HOMEWORK',true);await this.content('REMINDER');await this.offline();await this.startResponseCapture();await this.materialsBatch();await this.voice();await this.stopResponseCapture();await this.midnight();await this.revoke();const complete=await this.nativeAction('COMPLETE');check(complete.nativeHttpsDrained===true,'FINAL_NATIVE_DRAIN_REQUIRED');await this.settle();this.phase='complete';}catch(e){this.fault=e instanceof Fault?e:new Fault('HARNESS_INTERNAL');}finally{if(this.context){try{await this.send('Target.disposeBrowserContext',{browserContextId:this.context},null);this.contextDisposed=true;}catch{this.contextDisposed=false;}}this.cdpDisconnected=await this.cdp.disconnect();}return{schemaVersion:1,status:this.fault?'FAIL':'PASS_TEACHER_DOM_FINITE_WIN11_ONLY',marker:this.i.marker,scope:'SYNTHETIC_ORDINARY_TEACHER_SAME_HTTPS_DOM',checks:this.checks,ledger:this.ledger,assets:this.assets,nativeActions:this.native,pairingCycles:this.pairingCycles,download:this.download,batchObservation:this.batchObservation??null,batchDeleteObservation:this.batchDeleteObservation??null,midnightObservation:this.midnightObservation??null,droppedResponse:this.dropped,observedRequestCount:this.requests.size,elapsedMillis:performance.now()-this.started,maxMillis:this.i.maxMillis,contextDisposed:this.contextDisposed===true,cdpDisconnected:this.cdpDisconnected,physicalBrowserCleanup:'SEPARATE_ORIGINAL_EDGE_JOB_REQUIRED',voiceScope:'SYNTHETIC_ZERO_WAV_CAPTURE_ONLY_NO_REAL_MIC_OR_AUDIBILITY',windows7:'NOT_RUN',windows10:'NOT_RUN',phase:this.phase,...(this.fault?{error:{code:this.fault.code,...(this.fault.method?{cdpMethod:this.fault.method,cdpErrorCode:this.fault.nativeCode}:{}),...(this.fault.responseObservation?{responseObservation:this.fault.responseObservation}:{}),actionDiagnostics:this.actionDiagnostics}}:{})};}
}
async function main(){if(!process.argv.includes('--execute')){process.stdout.write(JSON.stringify({status:'NOT_RUN',mode:'DEFAULT_INERT',windows:'NOT_RUN'})+'\n');return;}const inbox=new Inbox(process.stdin);let h;try{const {value}=await inbox.next(performance.now()+5000);const init=validateInit(value);h=new TeacherHarness(init,inbox);const report=await h.run();process.stdout.write(JSON.stringify({type:'RESULT',report})+'\n');process.exitCode=report.status.startsWith('PASS_')?0:1;}catch(e){process.stdout.write(JSON.stringify({type:'FAIL',code:e instanceof Fault?e.code:'INPUT_FAILED'})+'\n');process.exitCode=1;}finally{inbox.close();}}
if(process.argv[1]&&fileURLToPath(import.meta.url)===resolve(process.argv[1]))await main();
