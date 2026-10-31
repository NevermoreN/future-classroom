#!/usr/bin/env python3
"""Finite browser QA coordinator; never a listener or browser-ownership proof.

The unchanged Edge Job owner proves physical browser/profile cleanup. Node owns
only its CDP context. Cookie/CSRF travel through private stdin, never argv,
environment, reports or raw CDP frames. Construction performs no I/O.
"""
import base64
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import queue
import re
import selectors
import stat
import subprocess
import sys
import time
import uuid

sys.dont_write_bytecode = True
MAX_BYTES = 512 * 1024
MAX_INPUT = 64 * 1024
REPORT_KEYS = frozenset((
    "schemaVersion", "status", "marker", "scope", "assetManifestCanonicalSha256",
    "expectedCspSha256", "credentialScope", "browserPhysicalCleanup", "assets",
    "checks", "browserVersion", "generation", "origin", "maxMillis",
    "syntheticTextSha256", "elapsedMillis", "contextDisposed", "cdpDisconnected",
    "network", "wireResponses", "wireResponseCount", "observedRequestCount",
    "observedResponseCount", "cacheOnlyResponseCount", "apiResponses", "contentPosts",
    "sessionCsrfMatched", "writeOutcome", "contentRevision", "commandId",
    "pageErrorCount", "consoleErrorCount", "sourceSha256", "error",
))
RECORD_KEYS = frozenset((
    "method", "url", "status", "mime", "csp", "servedFromCache", "fromDiskCache",
    "fromServiceWorker", "fromPrefetchCache", "wireStatus", "finished", "bodyVerified",
))


def require(condition, code):
    if not condition:
        raise RuntimeError(code)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def encoded(value):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode("utf-8")


def failure(error, phase):
    # Exception messages can contain private stdin, a CDP capability or a remote
    # response body. Only static phase/type metadata belongs in the QA report.
    return {"phase": phase, "kind": type(error).__name__}


def load_helpers():
    source = Path(__file__).resolve().with_name("windows-tls-probe.py")
    spec = importlib.util.spec_from_file_location("fc_native_browser_tls_helpers", source)
    require(spec is not None and spec.loader is not None, "TLS_HELPER_UNAVAILABLE")
    helper = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helper)
    return helper


class BrowserObserver:
    def __init__(self, remote, local_root, report_path, marker, asset_manifest,
                 expected_csp, identity, ssh_target):
        self.remote, self.local_root = remote, Path(local_root)
        self.report_path, self.marker = Path(report_path), marker
        self.manifest = json.loads(encoded(asset_manifest))
        self.expected_csp, self.identity, self.ssh_target = expected_csp, identity, ssh_target
        self.report = {"status": "NOT_RUN", "cleanup": "NOT_RUN", "marker": marker,
                       "credentialScope": "Cookie/CSRF private stdin only; no raw CDP frames",
                       "edgeSessionCapSeconds": 180, "ownerExitObservationSeconds": 215,
                       "errors": []}
        self.result = {}
        self.helper = self.owner = self.owner_logs = self.node = self.node_logs = None
        self.tunnel = self.control_file = self.control_hash = self.owner_marker = None
        self.edge_report = self.node_report = self.cdp_root = None
        self.prepared = self.started = self.closed = False
        self._secrets = []
        self._node_source_sha = None

    def _remaining(self, deadline):
        value = deadline - time.monotonic()
        require(value > 0, "BROWSER_DEADLINE")
        return value

    def _safe(self, raw):
        lowered = raw.lower()
        require(not any(secret.lower() in lowered for secret in self._secrets if secret),
                "PRIVATE_VALUE_IN_BROWSER_OUTPUT")

    def _file(self, path, limit=MAX_BYTES):
        require(stat.S_ISREG(path.lstat().st_mode), "REPORT_NOT_REGULAR")
        require(path.stat().st_size <= limit, "REPORT_BYTE_LIMIT")
        raw = path.read_bytes()
        require(len(raw) <= limit, "REPORT_BYTE_LIMIT")
        self._safe(raw)
        value = json.loads(raw)
        require(isinstance(value, dict), "REPORT_SHAPE")
        return value, {"path": str(path), "bytes": len(raw), "sha256": digest(raw)}

    def _line(self, process, logs, deadline):
        while True:
            remaining = self._remaining(deadline)
            require(not logs.overflow.is_set(), "BROWSER_OUTPUT_BOUND")
            try:
                return logs.lines.get(timeout=min(0.1, remaining))
            except queue.Empty:
                require(process.poll() is None or any(t.is_alive() for t in logs.threads),
                        "BROWSER_HELPER_EXITED_BEFORE_REPLY")

    def _send(self, message, deadline):
        data = encoded(message) + b"\n"
        require(len(data) <= MAX_INPUT and self.node.poll() is None, "NODE_INPUT_BOUND_OR_EXIT")
        # No writer thread and no unbounded buffered-pipe flush. A stalled Node
        # input consumes the same absolute prepare/run deadline.
        descriptor = self.node.stdin.fileno()
        os.set_blocking(descriptor, False)
        with selectors.DefaultSelector() as ready:
            ready.register(descriptor, selectors.EVENT_WRITE)
            offset = 0
            while offset < len(data):
                require(self.node.poll() is None, "NODE_EXITED_DURING_INPUT")
                require(ready.select(min(0.1, self._remaining(deadline))) or
                        time.monotonic() < deadline, "NODE_INPUT_DEADLINE")
                try:
                    offset += os.write(descriptor, data[offset:offset + 4096])
                except BlockingIOError:
                    pass

    def _read_control(self):
        if self.control_file is None or not self.control_file.exists():
            return False
        require(stat.S_ISREG(self.control_file.lstat().st_mode)
                and self.control_file.stat().st_mode & 0o077 == 0
                and self.control_file.stat().st_size <= 16 * 1024, "OWNER_CONTROL_NOT_PRIVATE")
        raw = self.control_file.read_bytes()
        require(len(raw) <= 16 * 1024, "OWNER_CONTROL_BOUND")
        state = json.loads(raw)
        require(state.get("schemaVersion") == 1 and state.get("sshTarget") == self.ssh_target
                and re.fullmatch(r"FutureClassroom-Test-Browser-[0-9a-f]{32}", state.get("marker", ""))
                and re.fullmatch(r"[0-9a-f]{64}", state.get("stopCapability", "")), "OWNER_CONTROL_IDENTITY")
        require(self.control_hash is None or self.control_hash == digest(raw), "OWNER_CONTROL_CHANGED")
        self.control_hash, self.owner_marker = digest(raw), state["marker"]
        # Retain only its hash; the stop capability remains in its mode-0600 file.
        return True

    def prepare(self, public_pem):
        require(not self.started and not self.closed, "OBSERVER_ALREADY_STARTED")
        self.started = True
        deadline = time.monotonic() + 60
        phase = "prepare-public-spki"
        try:
            require(re.fullmatch(r"FutureClassroom-Test-TLS-[0-9a-f]{32}", self.marker)
                    and self.remote.host == self.ssh_target, "BROWSER_COMPOSITION")
            pem = public_pem.encode("ascii") if isinstance(public_pem, str) else bytes(public_pem)
            require(len(pem) <= MAX_INPUT and pem.startswith(b"-----BEGIN CERTIFICATE-----"), "PUBLIC_PEM_BOUND")
            public = subprocess.run(["openssl", "x509", "-pubkey", "-noout"], input=pem,
                                    capture_output=True, timeout=min(5, self._remaining(deadline)))
            require(public.returncode == 0 and len(public.stdout) <= MAX_INPUT, "PUBLIC_KEY_EXTRACTION")
            der = subprocess.run(["openssl", "pkey", "-pubin", "-outform", "DER"], input=public.stdout,
                                 capture_output=True, timeout=min(5, self._remaining(deadline)))
            require(der.returncode == 0 and 0 < len(der.stdout) <= MAX_INPUT, "PUBLIC_SPKI_EXTRACTION")
            spki = base64.b64encode(hashlib.sha256(der.stdout).digest()).decode("ascii")
            self.helper = load_helpers()
            directory = Path(__file__).resolve().parent
            owner_source = directory / "windows-test-edge-owner.py"
            node_source = directory / "windows-native-browser-harness.mjs"
            self._node_source_sha = digest(node_source.read_bytes())
            self.report["sourceSha256"] = {
                "observer": digest(Path(__file__).read_bytes()), "edgeOwner": digest(owner_source.read_bytes()),
                "nodeHarness": self._node_source_sha, "tlsOwnershipHelpers": digest((directory / "windows-tls-probe.py").read_bytes()),
            }
            self.report["spkiSha256Base64"] = spki
            prefix = self.report_path.resolve()
            prefix.parent.mkdir(parents=True, exist_ok=True)
            self.edge_report = prefix.with_name(prefix.name + ".browser-edge-owner.json")
            self.node_report = prefix.with_name(prefix.name + ".browser-node.json")
            self.control_file = self.edge_report.with_name(self.edge_report.name + ".control.json")
            require(not any(p.exists() for p in (self.edge_report, self.node_report, self.control_file)),
                    "BROWSER_ARTIFACT_ALREADY_EXISTS")
            self.report["ownerReportPath"], self.report["nodeReportPath"] = str(self.edge_report), str(self.node_report)
            phase = "prepare-original-edge-owner"
            command = [sys.executable, "-B", str(owner_source), "run", "--spki", spki,
                       "--report", str(self.edge_report), "--ssh-target", self.ssh_target]
            if self.identity:
                command += ["--identity", str(self.identity)]
            self.owner = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            self.owner_logs = self.helper.Logs(self.owner)
            controlled = False
            while True:
                line = self._line(self.owner, self.owner_logs, deadline)
                if line.startswith("EDGE_CACHE_CONTROL "):
                    value = json.loads(line[len("EDGE_CACHE_CONTROL "):])
                    require(not controlled and Path(value["controlFile"]) == self.control_file
                            and self._read_control() and value.get("marker") == self.owner_marker,
                            "OWNER_CONTROL_PROTOCOL")
                    controlled = True
                elif line.startswith("EDGE_CACHE_READY "):
                    value = json.loads(line[len("EDGE_CACHE_READY "):])
                    require(controlled and value.get("marker") == self.owner_marker
                            and type(value.get("port")) is int and 1 <= value["port"] <= 65535
                            and re.fullmatch(r"/devtools/browser/[0-9a-fA-F-]{16,80}", value.get("webSocketPath", ""))
                            and value.get("boundLoopback") is True, "OWNER_READY_PROTOCOL")
                    break
            self.report["edgeReady"] = value
            phase = "prepare-independent-cdp-tunnel"
            self.cdp_root = self.local_root / ("browser-cdp-" + uuid.uuid4().hex[:12])
            self.cdp_root.mkdir(mode=0o700, parents=False, exist_ok=False)
            require(self._remaining(deadline) > 12, "CDP_PREPARE_BUDGET")
            self.tunnel = self.helper.Tunnel(self.remote, self.cdp_root)
            port = self.tunnel.open(value["port"])
            cdp_url = "ws://127.0.0.1:" + str(port) + value["webSocketPath"]
            self._secrets.append(cdp_url.encode())
            phase = "prepare-node-cdp-context"
            self.node = subprocess.Popen(["node", str(node_source), "--report", str(self.node_report)],
                                         stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            self.node_logs = self.helper.Logs(self.node)
            self._send({"type": "INIT", "cdpUrl": cdp_url, "expectedCsp": self.expected_csp,
                        "assetManifest": self.manifest, "marker": self.marker}, deadline)
            ready = json.loads(self._line(self.node, self.node_logs, deadline))
            require(set(ready) == {"type", "marker", "browserVersion", "scope"}
                    and ready.get("type") == "READY" and ready.get("marker") == self.marker,
                    "NODE_READY_PROTOCOL")
            self._safe(encoded(ready))
            self.report["nodeReady"] = ready
            self.prepared = True
            self.report["status"] = "PREPARED"
        except Exception as error:
            self.report["status"] = "FAIL"
            self.report["errors"].append(failure(error, phase))
            raise RuntimeError("NATIVE_BROWSER_PREPARE_FAILED") from None

    def _validate_result(self, value, message, run):
        require(set(value) <= REPORT_KEYS and value.get("schemaVersion") == 1
                and value.get("status") == "PASS" and value.get("marker") == self.marker
                and value.get("generation") == run["generation"] and value.get("origin") == run["origin"]
                and value.get("maxMillis") == run["maxMillis"]
                and value.get("sourceSha256") == self._node_source_sha
                and value.get("assetManifestCanonicalSha256") == digest(encoded(self.manifest))
                and value.get("expectedCspSha256") == digest(self.expected_csp.encode())
                and value.get("contextDisposed") is True and value.get("cdpDisconnected") is True
                and value.get("sessionCsrfMatched") is True and value.get("writeOutcome") == "COMMITTED"
                and value.get("contentPosts") == 1, "NODE_REPORT_IDENTITY_OR_OUTCOME")
        records = value.get("network")
        require(isinstance(records, list) and len(records) <= 128, "NODE_NETWORK_BOUND")
        assets = {asset["path"]: asset for asset in self.manifest["assets"]}
        api = ("/fc/api/v1/status", "/fc/api/v1/session", "/fc/api/v1/content/homework/scopes/0")
        content = r"/fc/api/v1/content/homework/[1-9][0-9]{0,19}/[1-9][0-9]{0,19}/[1-9][0-9]{0,19}/[0-9]{4}-[0-9]{2}-[0-9]{2}"
        for record in records:
            require(isinstance(record, dict) and set(record) <= RECORD_KEYS
                    and record.get("method") in ("GET", "POST") and isinstance(record.get("url"), str),
                    "NODE_NETWORK_SHAPE")
            path = record["url"].removeprefix(run["origin"])
            is_content = re.fullmatch(content, path) is not None
            require(record["url"] == run["origin"] + path
                    and (is_content or record["method"] == "GET" and
                         (path in api or path == "/fc/" or path.startswith("/fc/") and path[4:] in assets)),
                    "NODE_UNEXPECTED_TARGET")
            if "csp" in record:
                require(record["csp"] == self.expected_csp, "NODE_CSP_MISMATCH")
            require(record.get("status") in (200, 304, None) and record.get("wireStatus") in (200, 304, None),
                    "NODE_UNEXPECTED_HTTP_STATUS")
        wires = [record for record in records if record.get("wireStatus") == 304 or
                 record.get("wireStatus") is not None and not
                 (record.get("servedFromCache") or record.get("fromDiskCache") or
                  record.get("fromServiceWorker") or record.get("fromPrefetchCache"))]
        require(type(value.get("wireResponseCount")) is int and value["wireResponseCount"] == len(wires)
                and value.get("wireResponses") == wires and value.get("observedRequestCount") == len(records)
                and value["contentPosts"] == sum(record["method"] == "POST" for record in records)
                and value.get("apiResponses") == 9 and value.get("pageErrorCount") == 0
                and value.get("consoleErrorCount") == 0,
                "NODE_WIRE_LEDGER_MISMATCH")
        api_records = [record for record in records if "/api/v1/" in record["url"]]
        require(len(api_records) == 9 and all(record.get("wireStatus") == 200
                and record.get("status") == 200 and not any(record.get(key) for key in
                ("servedFromCache", "fromDiskCache", "fromServiceWorker", "fromPrefetchCache"))
                for record in api_records), "NODE_API_WIRE_IDENTITY")
        require(value.get("observedResponseCount") == sum(record.get("status") is not None for record in records)
                and value.get("cacheOnlyResponseCount") == sum(record not in wires and
                    bool(record.get("servedFromCache") or record.get("fromDiskCache")) for record in records),
                "NODE_CACHE_LEDGER_MISMATCH")
        for asset in value.get("assets", []):
            require(isinstance(asset, dict) and set(asset) == {"path", "bytes", "sha256", "verified"}
                    and asset.get("path") in assets and asset.get("verified") is True
                    and asset.get("bytes") == assets[asset["path"]]["sizeBytes"]
                    and asset.get("sha256") == assets[asset["path"]]["sha256"], "NODE_ASSET_IDENTITY")
        require(set(message) == {"type", "status", "marker", "wireResponseCount", "contentPosts", "contextDisposed", "report"}
                and message.get("type") == "RESULT" and message.get("status") == "PASS"
                and message.get("marker") == self.marker and message.get("report") == str(self.node_report)
                and message.get("wireResponseCount") == len(wires) and message.get("contentPosts") == 1
                and message.get("contextDisposed") is True, "NODE_RESULT_PROTOCOL")

    def _node_raw(self):
        drain_error = None
        try:
            self.node_logs.join()
        except Exception as error:
            drain_error = error
        raw = self.node_logs.report()
        private = False
        for item in raw.values():
            try:
                self._safe(base64.b64decode(item["base64"], validate=True))
            except Exception:
                # Keep non-reversible identity evidence of the failure, without
                # copying a leaked credential into the encompassing TLS report.
                item.pop("base64", None)
                item["privateOutputOmitted"] = True
                private = True
        self.report["nodeRaw"] = raw
        require(not private, "PRIVATE_VALUE_IN_BROWSER_OUTPUT")
        require(drain_error is None and all(item["captureScope"] == "COMPLETE" for item in raw.values()),
                "NODE_RAW_INCOMPLETE")

    def run(self, client):
        require(self.prepared and not self.closed and not self.result, "OBSERVER_RUN_STATE")
        phase = "run-private-input"
        try:
            maximum = min(12000, int((client.deadline - time.monotonic() - 5) * 1000))
            require(maximum > 0 and isinstance(client.cookie, str)
                    and re.fullmatch(r"__Host-FutureClassroom=[0-9a-fA-F]{64}", client.cookie)
                    and isinstance(client.csrf, str) and re.fullmatch(r"[0-9a-fA-F]{64}", client.csrf),
                    "BROWSER_RUN_AUTH_OR_BUDGET")
            self._secrets.extend((client.cookie.encode(), client.cookie.split("=", 1)[1].encode(), client.csrf.encode()))
            generation = client.ready.get("generation")
            require(type(generation) is int and 0 < generation <= 9007199254740991, "BROWSER_GENERATION")
            run = {"type": "RUN", "origin": client.ready["origin"], "cookie": client.cookie,
                   "expectedCsrf": client.csrf, "maxMillis": maximum, "marker": self.marker, "generation": generation}
            deadline = min(client.deadline - 2, time.monotonic() + maximum / 1000 + 3)
            self._send(run, deadline)
            self.node.stdin.close()
            phase = "run-node-result-and-actual-exit"
            line = self._line(self.node, self.node_logs, deadline)
            self._safe(line.encode())
            message = json.loads(line)
            self.node.wait(timeout=self._remaining(deadline))
            self.report["nodeActualExitCode"] = self.node.returncode
            self._node_raw()
            value, identity = self._file(self.node_report)
            self.report["nodeReport"] = identity
            require(self.node.returncode == 0, "NODE_ACTUAL_EXIT_FAILED")
            stdout_lines = bytes(self.node_logs.stdout).decode("utf-8", "strict").splitlines()
            require(len(stdout_lines) == 2 and json.loads(stdout_lines[0]).get("type") == "READY"
                    and json.loads(stdout_lines[1]) == message, "NODE_STDOUT_SEQUENCE")
            self._validate_result(value, message, run)
            self.result = value
            self.report.update({"status": "PASS", "result": value, "wireResponseCount": value["wireResponseCount"]})
            return self.result
        except Exception as error:
            self.report["status"] = "FAIL"
            self.report["errors"].append(failure(error, phase))
            raise RuntimeError("NATIVE_BROWSER_RUN_FAILED") from None

    def close(self):
        if self.closed and self.report["cleanup"] == "PASS":
            return self.report
        # Failed cleanup is resumable only through these original Popen/owner
        # capabilities; never allocate a replacement browser or tunnel.
        self.closed = True
        errors = []

        def attempt(phase, action):
            try:
                action()
            except Exception as error:
                errors.append(failure(error, phase))

        def stop_node():
            if self.node is None:
                return
            if self.node.poll() is None:
                self.node.terminate()
                try:
                    self.node.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    self.node.kill()
                    self.node.wait(timeout=3)
            self.report["nodeActualExitCode"] = self.node.returncode

        if self.node is not None and self.node.stdin and not self.node.stdin.closed:
            attempt("close-original-node-stdin", self.node.stdin.close)
        attempt("close-original-node", stop_node)
        if self.node_logs:
            attempt("preserve-safe-node-output", self._node_raw)
        if self.tunnel:
            attempt("close-independent-cdp-tunnel", self.tunnel.close)
            attempt("preserve-cdp-tunnel-output", lambda: self.report.update({"cdpTunnelRaw": self.tunnel.logs.report()}))
        if self.cdp_root:
            attempt("remove-empty-cdp-control-directory", lambda: self.cdp_root.rmdir() if self.cdp_root.exists() else None)

        def request_owner_stop():
            if self.owner is None:
                return
            # If prepare failed before CONTROL was consumed, use only the same
            # deterministic fresh control path, never a PID/name fallback.
            if self.owner.poll() is None:
                require(self._read_control(), "OWNER_CONTROL_UNAVAILABLE")
                command = [sys.executable, "-B", str(Path(__file__).resolve().with_name("windows-test-edge-owner.py")),
                           "stop", "--state", str(self.control_file)]
                stopped = subprocess.run(command, capture_output=True, timeout=25)
                self._safe(stopped.stdout + stopped.stderr)
                self.report["ownerStopRequest"] = {"exitCode": stopped.returncode,
                    **{name: {"bytes": len(raw), "sha256": digest(raw), "base64": base64.b64encode(raw).decode()}
                       for name, raw in (("stdout", stopped.stdout), ("stderr", stopped.stderr))}}
                require(stopped.returncode == 0, "OWNER_STOP_REQUEST_FAILED")

        def observe_owner_exit():
            if self.owner is None:
                return
            # This is the ORIGINAL run Popen, not the stop request process.
            # On timeout retain the capability/profile; its original 180s cap
            # remains active. Never turn an unobserved exit into cleanup PASS.
            self.owner.wait(timeout=215)
            self.report["ownerActualExitCode"] = self.owner.returncode
            value, identity = self._file(self.edge_report, limit=16 * 1024 * 1024)
            self.report["ownerReport"] = identity
            outcome = value.get("nativeOutcome", {})
            require(self.owner.returncode == 0 and value.get("status") == "PASS"
                    and value.get("cleanup") == "PASS" and value.get("marker") == self.owner_marker
                    and value.get("rawPreservation", {}).get("status") == "PASS"
                    and outcome.get("actualJobProcessesZero") is True
                    and outcome.get("originalPrimaryExited") is True
                    and outcome.get("originalHandlesClosed") is True
                    and value.get("cleanupDetails", {}).get("rootAbsent") is True,
                    "OWNER_ACTUAL_PHYSICAL_CLEANUP_UNPROVEN")
            self.report["ownerPhysicalCleanup"] = "PASS"

        attempt("request-original-edge-owner-stop", request_owner_stop)
        attempt("observe-original-edge-owner-exit", observe_owner_exit)
        if self.owner_logs:
            attempt("drain-original-edge-owner-output", self.owner_logs.join)
            # Preserve the exact bounded stream even when EOF/overflow checking
            # failed; Logs.report labels any prefix instead of claiming full SHA.
            self.report["ownerSupervisorRaw"] = self.owner_logs.report()
        if not errors and self.control_file and self.control_file.exists():
            attempt("remove-private-owner-control-after-proof", lambda: (self._read_control(), self.control_file.unlink()))
        self.report["cleanup"] = "FAIL" if errors else "PASS"
        self.report["errors"].extend(errors)
        if errors:
            self.report["status"] = "FAIL"
            self.report["retainedControlFile"] = str(self.control_file) if self.control_file else None
            self.report["ownerProcessRetained"] = self.owner is not None and self.owner.poll() is None
            raise RuntimeError("NATIVE_BROWSER_CLEANUP_FAILED")
        self.report.pop("retainedControlFile", None)
        self.report["ownerProcessRetained"] = False
        return self.report
