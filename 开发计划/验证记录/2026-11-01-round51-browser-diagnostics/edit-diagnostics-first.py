from pathlib import Path
p=Path('/tmp/fc-round51-browser-diagnostics-worktree/scripts/windows-native-browser-harness.mjs')
s=p.read_text()
s=s.replace("const MAX_BUNDLE = 5 * 1024 * 1024;", """const MAX_BUNDLE = 5 * 1024 * 1024;
// Diagnostics are observations of local CDP delivery, not socket/native clocks.
// One fixed record per already bounded request; no frame/header/body storage.
const DIAGNOSTIC_MILLIS = 60000;
const DIAGNOSTIC_EVENTS = 65536;
const FAILURE_PHASES = new Set(['connect', 'await-run', 'inject-existing-approved-cookie', 'navigate-native',
  'read-original-content', 'visual-original', 'enter-synthetic-text', 'visual-refresh-with-draft',
  'save-once', 'visual-after-save', 'reload-and-read-durable-content', 'visual-after-reload',
  'settle-observed-responses', 'complete']);
const BLOCKED_REASONS = new Set(['other', 'csp', 'mixed-content', 'origin', 'inspector', 'subresource-filter',
  'content-type', 'coep-frame-resource-needs-coep-header', 'coop-sandboxed-iframe-cannot-navigate-to-coop-page',
  'corp-not-same-origin', 'corp-not-same-origin-after-defaulted-to-same-origin-by-coep', 'corp-not-same-site']);
function diagnosticCount(record, key, value, maximum) {
  if (!Number.isSafeInteger(value) || value < 0) { record.countsClamped = true; return; }
  if (value > maximum - record[key]) { record[key] = maximum; record.countsClamped = true; }
  else record[key] += value;
}
""")
s=s.replace("this.visualReads = 0; this.visualPayloads = []; this.visualObservations = [];", "this.visualReads = 0; this.visualPayloads = []; this.visualObservations = []; this.runStartedAt = null;")
needle="  event(method, params, session) {"
new="""  diagnosticMillis() {
    if (!Number.isFinite(this.runStartedAt)) return null;
    const elapsed = Math.floor(performance.now() - this.runStartedAt);
    // A missing/out-of-bound clock is explicit unknown, never a deadline claim.
    return Number.isSafeInteger(elapsed) && elapsed >= 0 && elapsed <= DIAGNOSTIC_MILLIS ? elapsed : null;
  }
  requestFailure(record, params) {
    const d = record.diagnostic;
    return Object.freeze({ requestOrdinal: d.requestOrdinal, kind: record.kind, path: record.path,
      phase: FAILURE_PHASES.has(this.phase) ? this.phase : 'OTHER',
      canceled: params.canceled === true,
      errorText: typeof params.errorText === 'string' && /^net::ERR_[A-Z0-9_]{1,80}$/.test(params.errorText) ? params.errorText : 'OTHER',
      blockedReason: params.blockedReason === undefined ? 'NONE' : BLOCKED_REASONS.has(params.blockedReason) ? params.blockedReason : 'OTHER',
      failureMillis: this.diagnosticMillis(), requestMillis: d.requestMillis, responseMillis: d.responseMillis,
      extraInfoMillis: d.extraInfoMillis, finishedMillis: d.finishedMillis, bodyVerifiedMillis: d.bodyVerifiedMillis,
      responseReceived: record.response, finished: record.finished, bodyVerified: record.analyzed,
      dataEvents: d.dataEvents, dataBytes: d.dataBytes, encodedBytes: d.encodedBytes, countsClamped: d.countsClamped });
  }
  event(method, params, session) {"""
assert needle in s;s=s.replace(needle,new)
needle="          response: false, finished: false, analyzed: false, extraStatus: null };"
new="""          response: false, finished: false, analyzed: false, extraStatus: null,
          diagnostic: { requestOrdinal: this.requests.size + 1, requestMillis: this.diagnosticMillis(), responseMillis: null,
            extraInfoMillis: null, finishedMillis: null, bodyVerifiedMillis: null,
            dataEvents: 0, dataBytes: 0, encodedBytes: 0, countsClamped: false } };"""
assert needle in s;s=s.replace(needle,new)
s=s.replace("        record.response = true;", "        record.response = true; record.diagnostic.responseMillis = this.diagnosticMillis();")
s=s.replace("if (record) { check(record.extraStatus === null, 'DUPLICATE_EXTRA_RESPONSE'); record.extraStatus = params.statusCode; }", "if (record) { check(record.extraStatus === null, 'DUPLICATE_EXTRA_RESPONSE'); record.extraStatus = params.statusCode; record.diagnostic.extraInfoMillis = this.diagnosticMillis(); }")
needle="""      } else if (method === 'Network.loadingFailed') {
        if (this.requests.has(params.requestId)) throw new Fault('NETWORK_LOAD_FAILED');"""
new="""      } else if (method === 'Network.dataReceived') {
        const record = this.requests.get(params.requestId);
        if (record) {
          diagnosticCount(record.diagnostic, 'dataEvents', 1, DIAGNOSTIC_EVENTS);
          diagnosticCount(record.diagnostic, 'dataBytes', params.dataLength, MAX_CDP);
          diagnosticCount(record.diagnostic, 'encodedBytes', params.encodedDataLength, MAX_CDP);
        }
      } else if (method === 'Network.loadingFailed') {
        const record = this.requests.get(params.requestId);
        if (record) {
          // Freeze the FIRST fault synchronously before original fail/dispose.
          // Unknown request/context/cleanup events retain the original filter.
          if (!this.fault) record.failure = this.requestFailure(record, params);
          throw new Fault('NETWORK_LOAD_FAILED');
        }"""
assert needle in s;s=s.replace(needle,new)
s=s.replace("check(!record.finished, 'DUPLICATE_FINISH'); record.finished = true;", "check(!record.finished, 'DUPLICATE_FINISH'); record.finished = true; record.diagnostic.finishedMillis = this.diagnosticMillis();")
s=s.replace("    record.analyzed = true;", "    record.analyzed = true; record.diagnostic.bodyVerifiedMillis = this.diagnosticMillis();")
s=s.replace("    this.run = run; this.until = receivedAt + run.maxMillis;", "    this.run = run; this.runStartedAt = receivedAt; this.until = receivedAt + run.maxMillis;")
s=s.replace("      wireStatus: r.extraStatus, finished: r.finished, bodyVerified: r.analyzed }));", "      wireStatus: r.extraStatus, finished: r.finished, bodyVerified: r.analyzed,\n      ...(r.failure ? { failure: r.failure } : {}) }));")
# Positive completion cannot coexist with a diagnostic failure, even if a
# synthetic model tries to change report.status after recording a failure.
s=s.replace("  checkApiLedger() {\n    const api", "  checkApiLedger() {\n    check(![...this.requests.values()].some(r => r.failure), 'NETWORK_FAILURE_NOT_SUCCESS');\n    const api")
p.write_text(s)
