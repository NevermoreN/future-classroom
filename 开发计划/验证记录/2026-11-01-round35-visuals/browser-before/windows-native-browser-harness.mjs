#!/usr/bin/env node
/** Isolated Edge CDP QA only. No browser launch, system trust change or browser kill.
 * Node v22 builtin WebSocket; no Playwright/ws/project dependencies.
 * stdin: INIT -> stdout READY -> stdin RUN -> stdout RESULT. Credentials stay
 * in memory; the exclusive Windows Job owner remains responsible for actual
 * process exit, profile deletion and its separate original-log evidence.
 */
import { createHash } from 'node:crypto';
import { openSync, closeSync, readFileSync, writeSync, fsyncSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { performance } from 'node:perf_hooks';
import { createServer } from 'node:http';
import { resolve } from 'node:path';
import assert from 'node:assert/strict';

const MAX_LINE = 64 * 1024;
const MAX_CDP = 8 * 1024 * 1024;
const MAX_REQUESTS = 128;
const MAX_BUNDLE = 5 * 1024 * 1024;
const U64_MAX = (1n << 64n) - 1n;
const SHA = /^[0-9a-f]{64}$/;
const HEX_SECRET = /^[0-9a-fA-F]{64}$/;
const COOKIE = /^__Host-FutureClassroom=([0-9a-fA-F]{64})$/;
const MARKER = /^FutureClassroom-Test-TLS-[0-9a-f]{32}$/;
const SAVED = '白板已确认持久保存。此结果不代表内容此刻正在屏幕上显示。';
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
class Fault extends Error {
  constructor(code, method, nativeCode) {
    super(code); this.code = code;
    if (method) this.method = method;
    if (Number.isInteger(nativeCode)) this.nativeCode = nativeCode;
  }
}
function check(value, code) { if (!value) throw new Fault(code); }
function shape(value, keys, code) {
  check(value !== null && typeof value === 'object' && !Array.isArray(value), code);
  check(Object.keys(value).sort().join(',') === [...keys].sort().join(','), code);
}
function decimal(value, zero = false) {
  return typeof value === 'string' && (zero ? /^(0|[1-9][0-9]{0,19})$/ : /^[1-9][0-9]{0,19}$/).test(value)
    && BigInt(value) <= U64_MAX;
}
function civilDate(value) {
  if (typeof value !== 'string' || !/^[0-9]{4}-[0-9]{2}-[0-9]{2}$/.test(value)) return false;
  const date = new Date(value + 'T00:00:00.000Z');
  return Number.isFinite(date.getTime()) && date.toISOString().slice(0, 10) === value;
}
function header(headers, name) {
  const entries = Object.entries(headers ?? {}).filter(([key]) => key.toLowerCase() === name);
  check(entries.length <= 1, 'DUPLICATE_OBSERVED_HEADER');
  return entries.length ? entries[0][1] : undefined;
}
function validateInit(value) {
  shape(value, ['type', 'cdpUrl', 'expectedCsp', 'assetManifest', 'marker'], 'INVALID_INIT');
  check(value.type === 'INIT' && MARKER.test(value.marker), 'INVALID_INIT');
  let url; try { url = new URL(value.cdpUrl); } catch { throw new Fault('INVALID_CDP_ENDPOINT'); }
  check(url.protocol === 'ws:' && ['127.0.0.1', '[::1]'].includes(url.hostname) && url.port
    && !url.username && !url.password && !url.search && !url.hash
    && /^\/devtools\/browser\/[0-9a-fA-F-]{16,80}$/.test(url.pathname), 'INVALID_CDP_ENDPOINT');
  check(typeof value.expectedCsp === 'string' && value.expectedCsp.length <= 2048
    && value.expectedCsp.includes("script-src 'self'") && value.expectedCsp.includes("connect-src 'self'")
    && !value.expectedCsp.includes('unsafe-inline') && !value.expectedCsp.includes('unsafe-eval')
    && !/[\r\n\0]/.test(value.expectedCsp), 'INVALID_EXPECTED_CSP');
  const manifest = value.assetManifest;
  shape(manifest, ['schemaVersion', 'apiVersion', 'uiCompatibilityVersion', 'assets'], 'INVALID_MANIFEST');
  check(manifest.schemaVersion === 1 && manifest.apiVersion === 1 && manifest.uiCompatibilityVersion === 1
    && Array.isArray(manifest.assets) && manifest.assets.length >= 1 && manifest.assets.length <= 64, 'INVALID_MANIFEST');
  const paths = new Set(); let bytes = 0;
  for (const asset of manifest.assets) {
    shape(asset, ['path', 'sizeBytes', 'sha256'], 'INVALID_ASSET');
    check(typeof asset.path === 'string' && (asset.path === 'index.html'
      || /^assets\/[A-Za-z0-9_][A-Za-z0-9._-]{0,127}\.(js|css|svg|png|jpg|jpeg|webp)$/.test(asset.path))
      && !paths.has(asset.path.toLowerCase()) && Number.isSafeInteger(asset.sizeBytes) && asset.sizeBytes > 0
      && SHA.test(asset.sha256), 'INVALID_ASSET');
    paths.add(asset.path.toLowerCase()); bytes += asset.sizeBytes;
    check(bytes <= MAX_BUNDLE, 'MANIFEST_BUDGET');
  }
  check(paths.has('index.html') && manifest.assets.some(a => a.path.endsWith('.js'))
    && manifest.assets.some(a => a.path.endsWith('.css')), 'MANIFEST_ENTRY_MISSING');
  return value;
}
function validateRun(value, init) {
  shape(value, ['type', 'origin', 'cookie', 'expectedCsrf', 'maxMillis', 'marker', 'generation'], 'INVALID_RUN');
  check(value.type === 'RUN' && value.marker === init.marker && Number.isSafeInteger(value.generation)
    && value.generation > 0 && typeof value.origin === 'string'
    && /^https:\/\/localhost:([1-9][0-9]{0,4})$/.test(value.origin)
    && Number(new URL(value.origin).port) >= 1 && Number(new URL(value.origin).port) <= 65535
    && typeof value.cookie === 'string' && COOKIE.test(value.cookie)
    && typeof value.expectedCsrf === 'string' && HEX_SECRET.test(value.expectedCsrf)
    && Number.isSafeInteger(value.maxMillis) && value.maxMillis >= 1 && value.maxMillis <= 12000, 'INVALID_RUN');
  return value;
}

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
          check(++this.received <= 2 && Buffer.byteLength(line) <= MAX_LINE && line.length > 0, 'INPUT_SEQUENCE');
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

class Harness {
  constructor(init) {
    this.init = init; this.until = performance.now() + 10000; this.phase = 'connect'; this.fault = null;
    this.contextId = null; this.sessionId = null; this.disposePromise = null; this.run = null;
    this.requests = new Map(); this.earlyExtra = new Map(); this.bodies = new Set(); this.scope = null; this.initial = null; this.committed = null;
    this.reloaded = null; this.post = null; this.saveClicks = 0; this.sessionCount = 0; this.statusCount = 0;
    this.pageErrors = 0; this.consoleErrors = 0; this.scopeReads = 0; this.contentReads = 0;
    this.report = { schemaVersion: 1, status: 'FAIL', marker: init.marker,
      scope: 'Actual isolated Win11 Edge to Rust native loopback HTTPS; approved session reused; one synthetic content save and reload. No pairing, microphone, playback, foreground or OS trust changes.',
      assetManifestCanonicalSha256: hash(JSON.stringify(init.assetManifest)), expectedCspSha256: hash(init.expectedCsp),
      credentialScope: 'stdin memory only; Cookie/CSRF/Set-Cookie/response bodies and raw CDP frames omitted',
      browserPhysicalCleanup: 'SEPARATE_ORIGINAL_WINDOWS_JOB_OWNER_REQUIRED', assets: [], checks: [] };
  }
  fail(error) {
    if (!this.fault) this.fault = error instanceof Fault ? error : new Fault('INTERNAL_FAILURE');
    if (this.contextId && !this.disposePromise) void this.dispose();
  }
  active() { if (this.fault) throw this.fault; check(performance.now() < this.until, 'RUN_DEADLINE'); }
  async command(method, params = {}, browser = false) { this.active(); return this.cdp.send(method, params, browser ? undefined : this.sessionId, this.until); }
  async initBrowser() {
    this.cdp = new Cdp(this.init.cdpUrl, (method, params, session) => this.event(method, params, session), error => this.fail(error));
    await this.cdp.open(this.until);
    const version = await this.command('Browser.getVersion', {}, true);
    check(typeof version.product === 'string' && /^[A-Za-z0-9 ._/-]{1,100}$/.test(version.product), 'BROWSER_VERSION');
    this.report.browserVersion = version.product;
    this.contextId = (await this.command('Target.createBrowserContext', { disposeOnDetach: true }, true)).browserContextId;
    check(typeof this.contextId === 'string' && this.contextId.length <= 128, 'CDP_CONTEXT_ID');
    const { targetId } = await this.command('Target.createTarget', { url: 'about:blank', browserContextId: this.contextId, background: true }, true);
    this.sessionId = (await this.command('Target.attachToTarget', { targetId, flatten: true }, true)).sessionId;
    check(typeof this.sessionId === 'string' && this.sessionId.length <= 128, 'CDP_TARGET_SESSION');
    await this.command('Network.enable', { maxTotalBufferSize: MAX_CDP, maxResourceBufferSize: MAX_BUNDLE, maxPostDataSize: 65536 });
    await this.command('Page.enable'); await this.command('Runtime.enable');
    this.phase = 'await-run';
  }
  classify(raw, method) {
    let url; try { url = new URL(raw); } catch { return null; }
    if (!this.run || url.origin !== this.run.origin || url.username || url.password || url.search || url.hash) return null;
    const path = url.pathname;
    if (method === 'GET' && (path === '/fc/' || this.init.assetManifest.assets.some(a => '/fc/' + a.path === path))) return { kind: 'asset', path };
    if (method === 'GET' && ['/fc/api/v1/status', '/fc/api/v1/session', '/fc/api/v1/content/homework/scopes/0'].includes(path)) return { kind: 'api', path };
    if (['GET', 'POST'].includes(method) && /^\/fc\/api\/v1\/content\/homework\/[1-9][0-9]{0,19}\/[1-9][0-9]{0,19}\/[1-9][0-9]{0,19}\/[0-9]{4}-[0-9]{2}-[0-9]{2}$/.test(path)) return { kind: 'content', path };
    return null;
  }
  event(method, params, session) {
    if (session !== this.sessionId || !this.run || this.disposePromise) return;
    try {
      if (method === 'Runtime.exceptionThrown') { this.pageErrors++; throw new Fault('PAGE_EXCEPTION'); }
      if (method === 'Runtime.consoleAPICalled' && params.type === 'error') { this.consoleErrors++; throw new Fault('PAGE_CONSOLE_ERROR'); }
      if (method === 'Network.requestWillBeSent') {
        check(!params.redirectResponse && !this.requests.has(params.requestId), 'REDIRECT_OR_REUSED_REQUEST_ID');
        check(this.requests.size < MAX_REQUESTS, 'NETWORK_RECORD_BUDGET');
        const request = params.request; const classified = this.classify(request?.url, request?.method);
        check(classified, 'UNEXPECTED_NETWORK_TARGET_OR_METHOD');
        const record = { method: request.method, url: request.url, kind: classified.kind, path: classified.path,
          servedFromCache: false, fromDiskCache: false, fromServiceWorker: false, fromPrefetchCache: false,
          response: false, finished: false, analyzed: false, extraStatus: null };
        this.requests.set(params.requestId, record);
        if (this.earlyExtra.has(params.requestId)) { record.extraStatus = this.earlyExtra.get(params.requestId); this.earlyExtra.delete(params.requestId); }
        if (record.method === 'POST') {
          check(this.saveClicks === 1 && !this.post && this.scope && this.initial, 'UNEXPECTED_OR_DUPLICATE_CONTENT_POST');
          check(record.path === this.contentPath(), 'POST_SCOPE_CHANGED');
          check(header(request.headers, 'x-fc-csrf') === this.run.expectedCsrf, 'POST_CSRF_MISMATCH');
          check(typeof request.postData === 'string' && Buffer.byteLength(request.postData) <= 65536, 'POST_BODY_UNAVAILABLE');
          let value; try { value = JSON.parse(request.postData); } catch { throw new Fault('POST_JSON'); }
          shape(value, ['schemaVersion', 'commandId', 'expectedRevision', 'text'], 'POST_SHAPE');
          check(value.schemaVersion === 1 && decimal(value.commandId) && value.expectedRevision === this.initial.revision
            && value.text === this.text, 'POST_ORIGINAL_COMMAND_MISMATCH');
          this.post = { commandId: value.commandId, expectedRevision: value.expectedRevision };
        }
      } else if (method === 'Network.requestServedFromCache') {
        const record = this.requests.get(params.requestId); if (record) record.servedFromCache = true;
      } else if (method === 'Network.responseReceived') {
        const record = this.requests.get(params.requestId); check(record && !record.response, 'UNMATCHED_RESPONSE');
        const response = params.response;
        check(response.url === record.url && Number.isInteger(response.status), 'RESPONSE_TARGET_CHANGED');
        record.status = response.status; record.mime = response.mimeType;
        record.fromDiskCache = response.fromDiskCache === true;
        record.fromServiceWorker = response.fromServiceWorker === true;
        record.fromPrefetchCache = response.fromPrefetchCache === true;
        record.hasExtraInfo = params.hasExtraInfo === true;
        check(!record.fromServiceWorker && !record.fromPrefetchCache, 'UNEXPECTED_RESPONSE_SOURCE');
        if (record.kind === 'asset') {
          check(header(response.headers, 'content-security-policy') === this.init.expectedCsp, 'STATIC_CSP_MISMATCH');
          record.csp = this.init.expectedCsp;
          check(record.status === 200, 'STATIC_HTTP_STATUS');
        } else check(record.status === 200 && record.mime === 'application/json', 'API_HTTP_STATUS_OR_MIME');
        record.response = true;
      } else if (method === 'Network.responseReceivedExtraInfo') {
        check(Number.isInteger(params.statusCode) && [200, 304].includes(params.statusCode), 'WIRE_HTTP_STATUS');
        const record = this.requests.get(params.requestId);
        if (record) { check(record.extraStatus === null, 'DUPLICATE_EXTRA_RESPONSE'); record.extraStatus = params.statusCode; }
        else { check(this.earlyExtra.size < MAX_REQUESTS && !this.earlyExtra.has(params.requestId), 'EXTRA_RESPONSE_BUDGET'); this.earlyExtra.set(params.requestId, params.statusCode); }
      } else if (method === 'Network.loadingFailed') {
        if (this.requests.has(params.requestId)) throw new Fault('NETWORK_LOAD_FAILED');
      } else if (method === 'Network.loadingFinished') {
        const record = this.requests.get(params.requestId); check(record && record.response, 'UNMATCHED_FINISH');
        check(!record.finished, 'DUPLICATE_FINISH'); record.finished = true;
        const task = this.analyze(params.requestId, record).catch(error => this.fail(error));
        this.bodies.add(task); void task.finally(() => this.bodies.delete(task));
      }
    } catch (error) { this.fail(error); }
  }
  contentPath() {
    const s = this.scope;
    return `/fc/api/v1/content/homework/${s.classId}/${s.subjectId}/${s.ownerTeacherId}/${s.today}`;
  }
  checkDocument(value) {
    shape(value, ['schemaVersion', 'status', 'today', 'serverNow', 'expiresAt', 'key', 'revision', 'text', 'lastActorId', 'updatedAt'], 'CONTENT_SHAPE');
    check(value.schemaVersion === 1 && value.status === 'CONTENT' && this.scope && value.today === this.scope.today
      && typeof value.serverNow === 'string' && value.serverNow.startsWith(value.today + 'T')
      && typeof value.expiresAt === 'string' && decimal(value.revision, true)
      && typeof value.text === 'string' && Buffer.byteLength(value.text) <= 16384, 'CONTENT_FIELDS');
    shape(value.key, ['kind', 'classId', 'subjectId', 'ownerTeacherId', 'date'], 'CONTENT_KEY_SHAPE');
    const expected = { kind: 'HOMEWORK', classId: this.scope.classId, subjectId: this.scope.subjectId,
      ownerTeacherId: this.scope.ownerTeacherId, date: this.scope.today };
    check(Object.keys(expected).every(k => value.key[k] === expected[k]), 'CONTENT_KEY_CHANGED');
    if (value.revision === '0') check(value.text === '' && value.lastActorId === null && value.updatedAt === null, 'INITIAL_CONTENT_NOT_EMPTY');
    else check(decimal(value.lastActorId) && typeof value.updatedAt === 'string' && value.updatedAt.startsWith(value.today + 'T'), 'CONTENT_AUDIT_FIELDS');
    return { revision: value.revision, textMatches: value.text === this.text };
  }
  async analyze(id, record) {
    const result = await this.command('Network.getResponseBody', { requestId: id });
    check(typeof result.body === 'string', 'RESPONSE_BODY_UNAVAILABLE');
    const bytes = Buffer.from(result.body, result.base64Encoded ? 'base64' : 'utf8');
    if (record.kind === 'asset') {
      const path = record.path === '/fc/' ? 'index.html' : record.path.slice('/fc/'.length);
      const asset = this.init.assetManifest.assets.find(a => a.path === path);
      check(asset && bytes.length === asset.sizeBytes && hash(bytes) === asset.sha256, 'STATIC_BODY_IDENTITY');
      const expectedMime = path === 'index.html' ? 'text/html' : path.endsWith('.js') ? 'text/javascript'
        : path.endsWith('.css') ? 'text/css' : path.endsWith('.svg') ? 'image/svg+xml' : null;
      if (expectedMime) check(record.mime === expectedMime, 'STATIC_MIME_MISMATCH');
      this.report.assets.push({ path, bytes: bytes.length, sha256: asset.sha256, verified: true });
    } else {
      check(bytes.length <= 65536, 'API_BODY_BUDGET');
      let value; try { value = JSON.parse(new TextDecoder('utf8', { fatal: true }).decode(bytes)); }
      catch { throw new Fault('API_JSON'); }
      if (record.path.endsWith('/status')) {
        check(value.schemaVersion === 1 && value.apiVersion === 1 && value.uiCompatibilityVersion === 1 && value.online === true, 'STATUS_PAYLOAD');
        this.statusCount++;
      } else if (record.path.endsWith('/session')) {
        check(value.schemaVersion === 1 && value.status === 'AUTHENTICATED' && value.csrf === this.run.expectedCsrf, 'SESSION_CSRF_MISMATCH');
        this.sessionCount++;
      } else if (record.path.endsWith('/scopes/0')) {
        shape(value, ['schemaVersion', 'status', 'kind', 'today', 'identityRevision', 'offset', 'total', 'nextOffset', 'items'], 'SCOPE_SHAPE');
        check(value.schemaVersion === 1 && value.status === 'SCOPES' && value.kind === 'HOMEWORK' && civilDate(value.today)
          && decimal(value.identityRevision, true) && value.offset === 0 && value.total === 1 && value.nextOffset === null
          && Array.isArray(value.items) && value.items.length === 1, 'SCOPE_FIELDS');
        const item = value.items[0];
        shape(item, ['classId', 'className', 'subjectId', 'subjectName', 'ownerTeacherId', 'ownerDisplayName'], 'SCOPE_ITEM_SHAPE');
        check(item.classId === '1' && item.subjectId === '1' && decimal(item.ownerTeacherId)
          && item.className === 'Synthetic TLS class' && item.subjectName === 'Synthetic TLS subject'
          && item.ownerDisplayName === 'Synthetic TLS administrator', 'SYNTHETIC_SCOPE_MISMATCH');
        const scope = { classId: item.classId, subjectId: item.subjectId, ownerTeacherId: item.ownerTeacherId, today: value.today };
        if (this.scope) check(Object.keys(scope).every(k => scope[k] === this.scope[k]), 'RELOAD_SCOPE_CHANGED');
        this.scope = scope; this.scopeReads++;
      } else if (record.method === 'POST') {
        shape(value, ['schemaVersion', 'status', 'commandId', 'content'], 'COMMITTED_SHAPE');
        check(value.schemaVersion === 1 && value.status === 'COMMITTED' && this.post
          && value.commandId === this.post.commandId, 'COMMITTED_COMMAND_MISMATCH');
        const document = this.checkDocument(value.content);
        check(document.textMatches && document.revision === (BigInt(this.post.expectedRevision) + 1n).toString(), 'COMMITTED_CONTENT_MISMATCH');
        this.committed = document;
      } else {
        check(record.path === this.contentPath(), 'GET_SCOPE_CHANGED');
        const document = this.checkDocument(value);
        if (!this.initial) { check(document.revision === '0', 'UNEXPECTED_EXISTING_CONTENT'); this.initial = document; }
        else { check(this.committed && document.revision === this.committed.revision && document.textMatches, 'RELOADED_CONTENT_MISMATCH'); this.reloaded = document; }
        this.contentReads++;
      }
    }
    record.analyzed = true;
  }
  async evaluate(expression) {
    const result = await this.command('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: false });
    check(!result.exceptionDetails && result.result && !result.result.unserializableValue, 'DOM_EVALUATION');
    return result.result.value;
  }
  async untilTrue(condition) {
    while (true) { this.active(); if (await condition()) return; await sleep(Math.min(20, Math.max(1, this.until - performance.now()))); }
  }
  selectorExpression(kind) {
    return `(() => { const kind=${JSON.stringify(kind)}; let all;
      if(kind==='home') all=[...document.querySelectorAll('button')].filter(e=>e.textContent.trim()==='管理今日作业');
      else if(kind==='scope') all=[...document.querySelectorAll('.scope-options button.scope-option')];
      else if(kind==='save') all=[...document.querySelectorAll('.editor-actions button')].filter(e=>e.textContent.trim()==='保存当天内容');
      else all=[...document.querySelectorAll('textarea#content-text')];
      if(all.length!==1) return {ready:false,count:all.length}; const e=all[0];
      if(e.disabled || e.readOnly) return {ready:false,count:1};
      if(kind==='scope' && !['Synthetic TLS class','Synthetic TLS subject','Synthetic TLS administrator'].every(s=>e.textContent.includes(s))) return {ready:false,count:1};
      e.scrollIntoView({block:'center',inline:'center'}); const r=e.getBoundingClientRect(),x=r.x+r.width/2,y=r.y+r.height/2;
      const top=document.elementFromPoint(x,y);return {ready:r.width>0&&r.height>0&&x>=0&&y>=0&&x<innerWidth&&y<innerHeight&&(top===e||e.contains(top)),count:1,x,y}; })()`;
  }
  async click(kind) {
    let point;
    await this.untilTrue(async () => { point = await this.evaluate(this.selectorExpression(kind)); return point?.ready === true; });
    await this.command('Input.dispatchMouseEvent', { type: 'mouseMoved', x: point.x, y: point.y });
    await this.command('Input.dispatchMouseEvent', { type: 'mousePressed', x: point.x, y: point.y, button: 'left', clickCount: 1 });
    await this.command('Input.dispatchMouseEvent', { type: 'mouseReleased', x: point.x, y: point.y, button: 'left', clickCount: 1 });
  }
  async openContent() {
    await this.click('home');
    await this.untilTrue(async () => this.scopeReads >= (this.committed ? 2 : 1));
    await this.click('scope');
    await this.untilTrue(async () => this.contentReads >= (this.committed ? 2 : 1));
    await this.untilTrue(async () => await this.evaluate(`(() => {const e=document.querySelector('#content-text');return !!e&&!e.disabled&&!e.readOnly&&e.getAttribute('aria-invalid')==='false';})()`));
  }
  async execute(run, receivedAt) {
    this.run = run; this.until = receivedAt + run.maxMillis; this.text = `Synthetic Native Edge content ${run.marker}`;
    this.report.generation = run.generation; this.report.origin = run.origin; this.report.maxMillis = run.maxMillis;
    this.report.syntheticTextSha256 = hash(this.text); const began = performance.now();
    this.phase = 'inject-existing-approved-cookie';
    await this.command('Storage.setCookies', { browserContextId: this.contextId, cookies: [{
      name: '__Host-FutureClassroom', value: COOKIE.exec(run.cookie)[1], url: run.origin + '/', path: '/',
      httpOnly: true, secure: true, sameSite: 'Strict',
    }] }, true);
    run.cookie = '';
    this.phase = 'navigate-native';
    const navigation = await this.command('Page.navigate', { url: run.origin + '/fc/' });
    check(!navigation.errorText, 'NATIVE_NAVIGATION_FAILED');
    await this.untilTrue(async () => this.statusCount === 1 && this.sessionCount === 1);
    this.phase = 'read-original-content'; await this.openContent();
    this.phase = 'enter-synthetic-text'; await this.click('text');
    await this.command('Input.insertText', { text: this.text });
    await this.untilTrue(async () => await this.evaluate(`document.querySelector('#content-text')?.value===${JSON.stringify(this.text)}`));
    this.phase = 'save-once'; this.saveClicks++;
    await this.click('save');
    await this.untilTrue(async () => this.committed !== null);
    await this.untilTrue(async () => await this.evaluate(`(() => {const n=document.querySelector('.content-editor .content-notice');return n?.textContent.trim()===${JSON.stringify(SAVED)}&&!document.querySelector('.command-receipt');})()`));
    this.phase = 'reload-and-read-durable-content'; await this.command('Page.reload');
    await this.untilTrue(async () => this.statusCount === 2 && this.sessionCount === 2);
    await this.openContent();
    await this.untilTrue(async () => this.reloaded !== null && await this.evaluate(`document.querySelector('#content-text')?.value===${JSON.stringify(this.text)}`));
    this.phase = 'settle-observed-responses';
    await this.untilTrue(async () => this.responsesSettled());
    const api = [...this.requests.values()].filter(r => r.kind !== 'asset');
    check(api.length === 9 && this.sessionCount === 2 && this.statusCount === 2 && this.scopeReads === 2
      && this.contentReads === 2 && api.filter(r => r.method === 'POST').length === 1, 'API_OPERATION_COUNT');
    check(api.every(r => r.extraStatus === 200 && !r.servedFromCache && !r.fromDiskCache
      && !r.fromServiceWorker && !r.fromPrefetchCache), 'API_NOT_PROVEN_NATIVE_NO_STORE');
    check(this.report.assets.some(a => a.path === 'index.html') && this.report.assets.some(a => a.path.endsWith('.css'))
      && this.report.assets.some(a => a.path.endsWith('.js')), 'MISSING_NATIVE_BROWSER_ASSETS');
    check(this.pageErrors === 0 && this.consoleErrors === 0, 'BROWSER_ERROR_COUNT');
    this.report.checks = ['Native HTML/JS/CSS exact manifest identities and CSP', 'Original approved HttpOnly cookie; actual session CSRF matched twice',
      'One synthetic authorized scope and original empty content', 'Exactly one UI content POST with original CSRF/date/revision',
      'Committed receipt and post-reload durable content match', 'Nine real no-store API responses; cache hits counted separately'];
    this.report.elapsedMillis = Math.round(performance.now() - began); this.report.status = 'PASS';
    this.phase = 'complete';
  }
  responsesSettled() {
    return this.bodies.size === 0 && this.earlyExtra.size === 0
      && [...this.requests.values()].every(r => {
        // A verified memory/disk cached asset may advertise hasExtraInfo but
        // never deliver that event. It needs no new wire-response proof. API
        // responses still pass the separate exact200/no-cache proof gate.
        const cachedAsset = r.kind === 'asset' && (r.servedFromCache || r.fromDiskCache)
          && !r.fromServiceWorker && !r.fromPrefetchCache;
        return r.finished && r.analyzed && (cachedAsset || !r.hasExtraInfo || r.extraStatus !== null);
      });
  }
  dispose() {
    if (this.disposePromise) return this.disposePromise;
    this.disposePromise = (async () => {
      if (!this.contextId) return false;
      try {
        await this.cdp.send('Target.disposeBrowserContext', { browserContextId: this.contextId }, undefined, performance.now() + 2000);
        this.report.contextDisposed = true; return true;
      } catch { this.report.contextDisposed = false; return false; }
    })();
    return this.disposePromise;
  }
  summary() {
    const records = [...this.requests.values()].map(r => ({ method: r.method, url: r.url, status: r.status ?? null,
      mime: typeof r.mime === 'string' && /^[a-zA-Z0-9.+/-]{1,100}$/.test(r.mime) ? r.mime : null,
      ...(r.csp ? { csp: r.csp } : {}), servedFromCache: r.servedFromCache, fromDiskCache: r.fromDiskCache,
      fromServiceWorker: r.fromServiceWorker, fromPrefetchCache: r.fromPrefetchCache,
      wireStatus: r.extraStatus, finished: r.finished, bodyVerified: r.analyzed }));
    this.report.network = records;
    // Cached 200 ExtraInfo alone does not prove a new wire response; a raw
    // 304 proves revalidation even when the effective page response is 200.
    const wire = r => r.wireStatus !== null && (r.wireStatus === 304
      || !(r.servedFromCache || r.fromDiskCache || r.fromServiceWorker || r.fromPrefetchCache));
    this.report.wireResponses = records.filter(wire);
    this.report.wireResponseCount = this.report.wireResponses.length;
    this.report.observedRequestCount = records.length;
    this.report.observedResponseCount = records.filter(r => r.status !== null).length;
    this.report.cacheOnlyResponseCount = records.filter(r => !wire(r) && (r.servedFromCache || r.fromDiskCache)).length;
    this.report.apiResponses = records.filter(r => r.url.includes('/api/v1/')).length;
    this.report.contentPosts = records.filter(r => r.method === 'POST').length;
    this.report.sessionCsrfMatched = this.sessionCount === 2;
    this.report.writeOutcome = this.committed ? 'COMMITTED' : this.post ? 'UNKNOWN' : 'NOT_OBSERVED';
    if (this.committed) this.report.contentRevision = this.committed.revision;
    if (this.post) this.report.commandId = this.post.commandId;
    this.report.pageErrorCount = this.pageErrors; this.report.consoleErrorCount = this.consoleErrors;
    return this.report;
  }
}

function safeError(error, phase) {
  return { phase, code: error instanceof Fault ? error.code : 'INTERNAL_FAILURE',
    ...(error instanceof Fault && error.method ? { method: error.method } : {}),
    ...(error instanceof Fault && Number.isInteger(error.nativeCode) ? { nativeCode: error.nativeCode } : {}) };
}
async function main() {
  const args = process.argv.slice(2);
  if (args.length === 1 && args[0] === '--self-test') { await selfTest(); return; }
  if (args.length !== 2 || args[0] !== '--report') throw new Fault('USAGE_REPORT_REQUIRED');
  const descriptor = openSync(args[1], 'wx', 0o600); // reserve before accepting any credential
  const inbox = new Inbox(process.stdin); let harness = null; let result;
  try {
    const init = validateInit((await inbox.next(performance.now() + 10000)).value);
    harness = new Harness(init); inbox.onFault = error => harness.fail(error);
    if (inbox.fault) throw inbox.fault;
    await harness.initBrowser(); const readyAt = performance.now();
    writeSync(1, JSON.stringify({ type: 'READY', marker: init.marker, browserVersion: harness.report.browserVersion,
      scope: 'Exclusive CDP context prepared; no Native navigation or new pairing yet' }) + '\n');
    const input = await inbox.next(performance.now() + 60000);
    check(input.at >= readyAt, 'RUN_BEFORE_READY');
    await harness.execute(validateRun(input.value, init), input.at);
  } catch (error) {
    if (harness) { harness.report.status = 'FAIL'; harness.report.error = safeError(error, harness.phase); }
    else result = { schemaVersion: 1, status: 'FAIL', error: safeError(error, 'init-input'), credentialScope: 'stdin not logged' };
  } finally {
    inbox.close();
    if (harness) {
      await harness.dispose();
      if (harness.cdp) harness.report.cdpDisconnected = await harness.cdp.disconnect();
      if (harness.fault && !harness.report.error) harness.report.error = safeError(harness.fault, harness.phase);
      if (!harness.report.contextDisposed || !harness.report.cdpDisconnected || harness.report.error) harness.report.status = 'FAIL';
      result = harness.summary(); if (harness.run) { harness.run.cookie = ''; harness.run.expectedCsrf = ''; }
    }
    result.sourceSha256 = hash(readFileSync(fileURLToPath(import.meta.url)));
    writeSync(descriptor, JSON.stringify(result, null, 2) + '\n'); fsyncSync(descriptor); closeSync(descriptor);
  }
  writeSync(1, JSON.stringify({ type: 'RESULT', status: result.status, marker: result.marker ?? null,
    wireResponseCount: result.wireResponseCount ?? 0, contentPosts: result.contentPosts ?? 0,
    contextDisposed: result.contextDisposed === true, report: args[1] }) + '\n');
  process.exitCode = result.status === 'PASS' ? 0 : 1;
  // Do not let a failed WebSocket close handshake keep the helper alive. This
  // exits only this Node helper; original Edge Job teardown is independent.
  setTimeout(() => process.exit(process.exitCode), 1000).unref();
}

async function selfTest() {
  const manifest = { schemaVersion: 1, apiVersion: 1, uiCompatibilityVersion: 1, assets: [
    { path: 'index.html', sizeBytes: 1, sha256: hash('a') },
    { path: 'assets/index-AbCd1234.js', sizeBytes: 1, sha256: hash('b') },
    { path: 'assets/index-DFFVdc8-.css', sizeBytes: 1, sha256: hash('c') },
  ] };
  const init = { type: 'INIT', cdpUrl: 'ws://127.0.0.1:12345/devtools/browser/00000000-0000-0000-0000-000000000000',
    expectedCsp: "script-src 'self'; connect-src 'self'", assetManifest: manifest, marker: 'FutureClassroom-Test-TLS-' + 'a'.repeat(32) };
  assert.equal(validateInit(init), init);
  assert.throws(() => validateInit({ ...init, cdpUrl: init.cdpUrl.replace('127.0.0.1', 'example.com') }), Fault);
  const run = { type: 'RUN', origin: 'https://localhost:12345', cookie: '__Host-FutureClassroom=' + 'b'.repeat(64),
    expectedCsrf: 'c'.repeat(64), maxMillis: 12000, marker: init.marker, generation: 1 };
  assert.equal(validateRun(run, init), run);
  assert.throws(() => validateRun({ ...run, maxMillis: 12001 }, init), Fault);
  assert.throws(() => validateRun({ ...run, cookie: 'bad' }, init), Fault);
  const h = new Harness(init); h.sessionId = 'owned'; h.run = { ...run }; h.report.status = 'PASS';
  assert.equal(h.classify(run.origin + '/fc/api/v1/session', 'GET').kind, 'api');
  assert.equal(h.classify(run.origin + '/fc/api/v1/pairing/requests', 'POST'), null);
  assert.equal(h.classify(run.origin + '/fc/api/v1/session/logout', 'POST'), null);
  assert.equal(h.classify('https://wrong.invalid/fc/', 'GET'), null);
  h.event('Network.requestWillBeSent', { requestId: 'cached', request: { method: 'GET', url: run.origin + '/fc/assets/index-AbCd1234.js' } }, 'owned');
  h.event('Network.requestServedFromCache', { requestId: 'cached' }, 'owned');
  h.event('Network.responseReceived', { requestId: 'cached', response: { url: run.origin + '/fc/assets/index-AbCd1234.js', status: 200,
    mimeType: 'text/javascript', fromDiskCache: true, headers: { 'Content-Security-Policy': init.expectedCsp } } }, 'owned');
  h.event('Network.responseReceivedExtraInfo', { requestId: 'cached', statusCode: 200 }, 'owned');
  h.event('Network.requestWillBeSent', { requestId: 'revalidated', request: { method: 'GET', url: run.origin + '/fc/' } }, 'owned');
  h.event('Network.responseReceivedExtraInfo', { requestId: 'revalidated', statusCode: 304, headers: { 'Set-Cookie': run.cookie } }, 'owned');
  const summary = h.summary();
  assert.equal(summary.wireResponseCount, 1); assert.equal(summary.wireResponses[0].wireStatus, 304); assert.equal(summary.cacheOnlyResponseCount, 1);
  const serialized = JSON.stringify(summary);
  assert(!serialized.includes(run.cookie)); assert(!serialized.includes(run.expectedCsrf)); assert(!serialized.includes('"Set-Cookie":'));
  // Regression state from the preserved first Win11 run: nine actual API
  // responses, five actual static responses (including raw304), and three
  // verified cached assets with hasExtraInfo but no ExtraInfo event delivered.
  const settled = new Harness(init);
  const completed = { finished: true, analyzed: true, hasExtraInfo: true,
    servedFromCache: false, fromDiskCache: false, fromServiceWorker: false, fromPrefetchCache: false };
  for (let i = 0; i < 9; i++) settled.requests.set('api-' + i, { ...completed, kind: 'api', extraStatus: 200 });
  for (let i = 0; i < 5; i++) settled.requests.set('wire-' + i, { ...completed, kind: 'asset', extraStatus: i === 4 ? 304 : 200 });
  for (let i = 0; i < 3; i++) settled.requests.set('cache-' + i, { ...completed, kind: 'asset', extraStatus: null,
    servedFromCache: i < 2, fromDiskCache: i === 2 });
  check(settled.responsesSettled(), 'CACHED_EXTRAINFO_MUST_NOT_BLOCK_SETTLE');
  const cached = settled.requests.get('cache-0');
  cached.finished = false; assert.equal(settled.responsesSettled(), false); cached.finished = true;
  cached.analyzed = false; assert.equal(settled.responsesSettled(), false); cached.analyzed = true;
  const api = settled.requests.get('api-0');
  api.extraStatus = null; assert.equal(settled.responsesSettled(), false);
  api.servedFromCache = true; assert.equal(settled.responsesSettled(), false); api.servedFromCache = false; api.extraStatus = 200;
  const wire = settled.requests.get('wire-0');
  wire.extraStatus = null; assert.equal(settled.responsesSettled(), false); wire.extraStatus = 200;
  settled.earlyExtra.set('unknown', 200); assert.equal(settled.responsesSettled(), false); settled.earlyExtra.clear();
  const pendingBody = Promise.resolve(); settled.bodies.add(pendingBody); assert.equal(settled.responsesSettled(), false); settled.bodies.clear();
  assert.equal(settled.responsesSettled(), true);
  // Unknown context ownership cannot become a successful disposal claim.
  let calls = 0;
  const absent = new Harness(init); absent.cdp = { send: async () => { calls++; } };
  assert.equal(await absent.dispose(), false); assert.equal(calls, 0); assert.notEqual(absent.report.contextDisposed, true);
  const rejected = new Harness(init); rejected.contextId = 'own'; rejected.cdp = { send: async () => { throw new Fault('TEST_REJECT'); } };
  assert.equal(await rejected.dispose(), false); assert.equal(rejected.report.contextDisposed, false);
  const owned = new Harness(init); owned.contextId = 'own'; owned.cdp = { send: async (method, params, session, until) => {
    assert.equal(method, 'Target.disposeBrowserContext'); assert.deepEqual(params, { browserContextId: 'own' });
    assert.equal(session, undefined); assert(until > performance.now()); calls++;
  } };
  owned.until = performance.now() - 1;
  await assert.rejects(owned.command('Runtime.evaluate'), error => error.code === 'RUN_DEADLINE');
  assert.equal(await owned.dispose(), true); assert.equal(calls, 1); assert.equal(owned.report.contextDisposed, true);
  await socketSelfTest();
  writeSync(1, JSON.stringify({ type: 'SELF_TEST', status: 'PASS', builtinWebSocket: typeof WebSocket,
    checks: ['strict INIT/RUN and finite budget', 'pairing/logout/foreign target rejection', 'cached response excluded; raw304 wire status counted',
      'Cookie/CSRF/Set-Cookie omitted from summaries', 'actual loopback WebSocket CDP correlation and deadline', 'unknown/failed/owned context disposal and expired positive command rejection', 'verified cached assets do not wait for absent ExtraInfo; incomplete bodies/API/wire proof still block'],
    windowsBrowser: 'NOT_RUN', sourceSha256: hash(readFileSync(fileURLToPath(import.meta.url))) }) + '\n');
}

// A tiny owned loopback RFC6455 peer tests Node's REAL builtin WebSocket; it
// implements only masked final text frames needed by this isolated self-test.
async function socketSelfTest() {
  const sockets = new Set(); const server = createServer();
  server.on('upgrade', (request, socket) => {
    sockets.add(socket); socket.on('close', () => sockets.delete(socket));
    const accept = createHash('sha1').update(request.headers['sec-websocket-key'] + '258EAFA5-E914-47DA-95CA-C5AB0DC85B11').digest('base64');
    socket.write('HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ' + accept + '\r\n\r\n');
    let input = Buffer.alloc(0);
    const send = value => { const body = Buffer.from(JSON.stringify(value)); assert(body.length < 126); socket.write(Buffer.concat([Buffer.from([0x81, body.length]), body])); };
    socket.on('data', chunk => {
      input = Buffer.concat([input, chunk]);
      while (input.length >= 6) {
        const opcode = input[0] & 15; const length = input[1] & 127;
        if (opcode === 8) { socket.end(Buffer.from([0x88, 0])); return; }
        assert.equal(opcode, 1); assert(input[1] & 128); assert(length < 126);
        if (input.length < length + 6) return;
        const mask = input.subarray(2, 6), body = Buffer.from(input.subarray(6, 6 + length)); input = input.subarray(6 + length);
        for (let i = 0; i < body.length; i++) body[i] ^= mask[i % 4];
        const command = JSON.parse(body.toString('utf8'));
        if (command.method === 'Test.hold') continue;
        send({ method: 'Test.event', params: { safe: true }, sessionId: 'owned' });
        setTimeout(() => send({ id: command.id, result: { echoed: true }, ...(command.sessionId ? { sessionId: command.sessionId } : {}) }), command.method === 'Test.slow' ? 10 : 0);
      }
    });
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const events = []; const client = new Cdp('ws://127.0.0.1:' + server.address().port + '/devtools/browser/test', (m, p, s) => events.push([m, p.safe, s]), () => {});
  try {
    await client.open(performance.now() + 1000);
    const pending = Promise.all([client.send('Test.slow', {}, 'owned', performance.now() + 1000), client.send('Test.fast', {}, 'owned', performance.now() + 1000)]);
    assert((await pending).every(r => r.echoed === true));
    assert.equal(events.length, 2); assert(events.every(e => e[2] === 'owned'));
    await assert.rejects(client.send('Test.hold', {}, undefined, performance.now() + 20), error => error.code === 'CDP_COMMAND_DEADLINE');
    assert.equal(client.pending.size, 0); assert.equal(await client.disconnect(), true);
  } finally { for (const socket of sockets) socket.destroy(); await new Promise(resolve => server.close(resolve)); }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  main().catch(error => { writeSync(2, JSON.stringify({ type: 'HARNESS_ERROR', ...safeError(error, 'entry') }) + '\n'); process.exitCode = 1; });
}
