#!/usr/bin/env python3
"""Run the explicit finite TLS/shared-worker synthetic business diagnostic.

The default is a raw API client through an owned SSH forward. Optional --browser
also observes actual isolated Windows Edge UI against the same native service.
This is not general browser acceptance,
production-listener enablement, native strict-5s evidence, audio playback, or
Windows 7/10 compatibility evidence. Project code runs only from the supplied
diagnostic EXE; --browser uses the separately owned installed Edge process.
The disposable public certificate is trusted in this Python context only; its
private key never leaves Windows and no system trust/network setting is changed.
"""
import argparse
from collections import deque
from email.utils import format_datetime, parsedate_to_datetime
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import queue
import re
import socket
import ssl
import struct
import subprocess
import sys
import tempfile
import time
import uuid
import zipfile

sys.dont_write_bytecode = True
_source = Path(__file__).with_name("windows-tls-probe.py")
_spec = importlib.util.spec_from_file_location("fc_tls_business_ownership", _source)
_tls = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_tls)

# Reuse the exact tested certificate/key/process ownership and full raw-artifact
# preservation templates, rather than creating a second cleanup implementation.
COMMON, SETUP, CAPTURE, CLEANUP = _tls.COMMON, _tls.SETUP, _tls.CAPTURE, _tls.CLEANUP
if _tls.LAUNCH.count("--tls-lifecycle-probe") != 1 or _tls.STOP.count("--tls-lifecycle-probe") != 1:
    raise RuntimeError("original diagnostic control template changed; review exact flag ownership")
LAUNCH = _tls.LAUNCH.replace("--tls-lifecycle-probe", "--tls-business-probe")
STOP = _tls.STOP.replace("--tls-lifecycle-probe", "--tls-business-probe")
Remote, Tunnel, Logs = _tls.Remote, _tls.Tunnel, _tls.Logs
digest, failure, script, ps_literal = _tls.digest, _tls.failure, _tls.script, _tls.ps_literal
READY, PASS = "TLS_BUSINESS_READY ", "TLS_BUSINESS_PASS "
PROGRESS = "TLS_BUSINESS_PROGRESS "
EXIT_PREFIXES = (_tls.EXIT, "TLS_BUSINESS_PROCESS_EXIT ")
HEAD_LIMIT, BODY_LIMIT, FRAGMENT = 16 * 1024, 64 * 1024, 16 * 1024
DOWNLOAD_LIMIT = 50 * 1024 * 1024
STATIC_LIMIT = 5 * 1024 * 1024
CSP = ("default-src 'none'; script-src 'self'; worker-src 'self'; style-src 'self'; connect-src 'self'; "
       "img-src 'self' blob: data:; media-src blob:; base-uri 'none'; object-src 'none'; "
       "frame-ancestors 'none'; form-action 'self'")
MATERIAL_BYTES, COMMAND_ID = 120000, "9001001"


class HttpResponse:
    """Bounded parser for this diagnostic server's response contract.

    This is not a general HTTP parser: our HEAD responses and 204/304 omit Content-Length and
    Transfer-Encoding, have no repeated headers and exactly zero wire-body bytes.
    Other responses
    require one canonical decimal Content-Length and no Transfer-Encoding.
    The caller separately requires actual clean TLS EOF, including HEAD/204/304.
    """
    def __init__(self, binary=False, body_limit=None, head_only=False):
        self.pending = bytearray()
        self.status = None
        self.headers = {}
        self.body = bytearray()
        self.length = None
        self.head_bytes = 0
        self.binary = binary
        self.head_only = head_only
        self.body_limit = body_limit if body_limit is not None else (DOWNLOAD_LIMIT if binary else BODY_LIMIT)
        self.body_bytes = 0
        self.body_hash = hashlib.sha256()

    def feed(self, data):
        if self.status is None:
            self.pending.extend(data)
            offset = self.pending.find(b"\r\n\r\n")
            if offset < 0:
                if len(self.pending) > HEAD_LIMIT:
                    raise RuntimeError("HTTP response head exceeded 16KiB")
                return
            self.head_bytes = offset + 4
            if self.head_bytes > HEAD_LIMIT:
                raise RuntimeError("HTTP response head exceeded 16KiB")
            head, remainder = bytes(self.pending[:offset]), bytes(self.pending[offset + 4:])
            self.pending.clear()
            lines = head.split(b"\r\n")
            if not re.fullmatch(rb"HTTP/1\.1 [1-5][0-9]{2}(?: [\x20-\x7e]*)?", lines[0]):
                raise RuntimeError("invalid HTTP/1.1 status line")
            if len(lines) > 65:
                raise RuntimeError("response header count exceeded bound")
            self.status = int(lines[0][9:12])
            for line in lines[1:]:
                if b":" not in line or line[:1] in (b" ", b"\t"):
                    raise RuntimeError("invalid or folded response header")
                name, value = line.split(b":", 1)
                if not re.fullmatch(rb"[!#$%&'*+.^_`|~0-9A-Za-z-]+", name):
                    raise RuntimeError("invalid response header name")
                if any(byte < 32 and byte != 9 or byte == 127 for byte in value):
                    raise RuntimeError("invalid response header value")
                self.headers.setdefault(name.decode("ascii").lower(), []).append(value.strip().decode("ascii"))
            lengths = self.headers.get("content-length", [])
            if "transfer-encoding" in self.headers:
                raise RuntimeError("diagnostic HTTP responses forbid Transfer-Encoding")
            if self.head_only or self.status in (204, 304):
                if lengths:
                    raise RuntimeError("diagnostic HEAD/204/304 contract omits Content-Length")
                if any(len(values) != 1 for values in self.headers.values()):
                    raise RuntimeError("diagnostic HEAD/204/304 contract forbids duplicate headers")
                self.length = 0
            else:
                if len(lengths) != 1 or not re.fullmatch(r"0|[1-9][0-9]*", lengths[0]):
                    raise RuntimeError("HTTP response needs one canonical Content-Length")
                if len(lengths[0]) > len(str(self.body_limit)):
                    raise RuntimeError("HTTP response body exceeded its bounded limit")
                self.length = int(lengths[0])
                if self.length > self.body_limit:
                    raise RuntimeError("HTTP response body exceeded its bounded limit")
            data = remainder
        self.body_bytes += len(data)
        if self.body_bytes > self.length:
            raise RuntimeError("HTTP response has bytes beyond its exact wire-body length")
        self.body_hash.update(data)
        if not self.binary:
            self.body.extend(data)

    def finish(self):
        if self.status is None or self.body_bytes != self.length:
            raise RuntimeError("HTTP response truncated before its exact wire-body length")
        return self

    def unique(self, name):
        values = self.headers.get(name.lower(), [])
        if len(values) != 1:
            raise RuntimeError("required unique response header missing/duplicated: " + name)
        return values[0]

    def json(self):
        if self.binary:
            raise RuntimeError("streamed binary body has no retained JSON representation")
        value = json.loads(bytes(self.body).decode("utf-8", "strict"))
        if not isinstance(value, dict) or value.get("schemaVersion") != 1:
            raise RuntimeError("response JSON object/schema contract failed")
        return value


VISUAL_FIXTURES = ("FILLED_DEFAULT", "FILLED_ZERO", "OUTLINED")


def validate_ready(item, marker, visual_fixture="FILLED_DEFAULT"):
    if (not isinstance(item, dict) or type(item.get("port")) is not int or not 1 <= item["port"] <= 65535
            or item.get("marker") != marker or type(item.get("generation")) is not int or item["generation"] <= 0
            or item.get("origin") != "https://localhost:" + str(item["port"])
            or item.get("visualFixture") != visual_fixture
            or item.get("basePath") != "/fc/" or item.get("classId") != "1"
            or item.get("subjectId") != "1" or item.get("loginName") != "probe"
            or type(item.get("staticAssets")) is not int or not 1 <= item["staticAssets"] <= 64
            or not isinstance(item.get("pairingCode"), str) or not re.fullmatch(r"[0-9]{8}", item["pairingCode"])):
        raise RuntimeError("native business READY identity/origin/synthetic-fixture contract failed")


def validate_native(value, events, browser_wire_responses=0):
    # CDP wire observations are independent of this raw HTTP client's events.
    # Cache hits do not prove native requests and must never inflate this floor.
    require(type(browser_wire_responses) is int and browser_wire_responses >= 0,
            "independent browser wire-response count invalid")
    http_count = sum("httpStatus" in event for event in events) + browser_wire_responses
    if (not isinstance(value, dict) or value.get("physicalNativeDrain") is not True
            or value.get("sharedWorkers") != 2 or type(value.get("sharedWorkers")) is not int
            or value.get("workerJoin") is not True or value.get("setupCleanup") is not True
            or type(value.get("hubPending")) is not int or value["hubPending"] != 0
            or type(value.get("downloadHubPending")) is not int or value["downloadHubPending"] != 0
            or type(value.get("quarantined")) is not int or value["quarantined"] != 0
            or type(value.get("approvals")) is not int or value["approvals"] < 1
            or type(value.get("heads")) is not int or value["heads"] < http_count
            or type(value.get("responses")) is not int or not http_count <= value["responses"] <= value["heads"]
            or type(value.get("peakRecords")) is not int or not 1 <= value["peakRecords"] <= 8):
        raise RuntimeError("native business actual-drain/shared-pool/Hub/cleanup ledger contract failed")


def json_bytes(value):
    return json.dumps(value, separators=(",", ":"), ensure_ascii=True).encode("ascii")


def request_fragments(head, body):
    if len(head) > HEAD_LIMIT or len(head) < 40 or not head.endswith(b"\r\n\r\n"):
        raise RuntimeError("invalid locally generated request head")
    # The final header byte and the first already-arrived body bytes share one
    # sendall. These are client send calls, not a claim about kernel packet/read
    # boundaries; every body fragment remains below the receiver's 64KiB ACK cap.
    first = min(4096, len(body))
    chunks = [head[:7], head[7:39], head[39:-1], head[-1:] + body[:first]]
    chunks.extend(body[offset:offset + FRAGMENT] for offset in range(first, len(body), FRAGMENT))
    if any(not chunk or len(chunk) > FRAGMENT for chunk in chunks):
        raise RuntimeError("local request fragmentation bound failed")
    return chunks, first


class Client:
    def __init__(self, local_port, ready, tls_context, wrong_ca, expected_der, events):
        self.local_port, self.ready = local_port, ready
        self.context, self.wrong_ca, self.expected_der = tls_context, wrong_ca, expected_der
        self.events = events
        self.deadline = time.monotonic() + 30
        self.cookie = self.csrf = None

    def remaining(self):
        remaining = min(8, self.deadline - time.monotonic())
        if remaining <= 0:
            raise RuntimeError("finite client business observation budget expired")
        return remaining

    def request(self, case, method, route, body=b"", expected=(200,), content_type="application/json", origin=None,
                binary=False, authenticated=True, static=False, response_type=None, extra_headers=(), request_host=None):
        event = {"case": case, "status": "FAIL", "method": method, "route": route,
                 "requestBodyBytes": len(body), "requestBodySha256": digest(body)}
        self.events.append(event)
        started = time.monotonic_ns()
        connection = None
        try:
            authority = "localhost:" + str(self.ready["port"])
            wire_host = authority if request_host is None else request_host
            require(isinstance(wire_host, str) and len(wire_host) <= 253
                    and re.fullmatch(r"[A-Za-z0-9.:-]+", wire_host), "invalid diagnostic HTTP Host")
            event["requestHost"] = wire_host
            target = self.ready['basePath'] + ("" if static else "api/v1/") + route
            headers = [f"{method} {target} HTTP/1.1", f"Host: {wire_host}",
                       "Connection: close", "Origin: " + (origin if origin is not None else self.ready["origin"])]
            for name, value in extra_headers:
                require(re.fullmatch(r"[A-Za-z0-9-]+", name) and all(32 <= ord(c) < 127 for c in value),
                        "invalid locally generated extra header")
                headers.append(name + ": " + value)
            if self.cookie and authenticated:
                headers.append("Cookie: " + self.cookie)
            if method == "POST":
                headers += ["Content-Type: " + content_type, "Content-Length: " + str(len(body))]
                if self.csrf and authenticated:
                    headers.append("X-FC-CSRF: " + self.csrf)
            elif body:
                raise RuntimeError("local non-POST request unexpectedly contains body")
            head = ("\r\n".join(headers) + "\r\n\r\n").encode("ascii")
            chunks, first = request_fragments(head, body)
            connection = socket.create_connection(("127.0.0.1", self.local_port), timeout=self.remaining())
            connection.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
            connection.settimeout(self.remaining())
            connection = self.context.wrap_socket(connection, server_hostname="localhost", suppress_ragged_eofs=False)
            if connection.version() != "TLSv1.2" or connection.getpeercert(binary_form=True) != self.expected_der:
                raise RuntimeError("actual TLS version/exact disposable certificate mismatch")
            event.update({"tlsVersion": connection.version(), "peerDerSha256": digest(connection.getpeercert(binary_form=True)),
                          "serverHostname": "localhost", "hostAuthority": authority,
                          "clientSendCalls": len(chunks), "maxSendCallBytes": max(map(len, chunks)),
                          "coalescedHeaderTailBodyBytes": first})
            for index, chunk in enumerate(chunks):
                connection.settimeout(self.remaining())
                connection.sendall(chunk)
                if index < 3:
                    time.sleep(0.005)
            response = HttpResponse(binary=binary, body_limit=STATIC_LIMIT if static and binary else None,
                                    head_only=method == "HEAD")
            wire_hash = hashlib.sha256()
            total = 0
            while True:
                connection.settimeout(self.remaining())
                data = connection.recv(FRAGMENT)
                if not data:
                    # With suppress_ragged_eofs=False this is verified clean
                    # TLS close_notify; bare TCP EOF raises SSLEOFError instead.
                    break
                total += len(data)
                if total > HEAD_LIMIT + response.body_limit:
                    raise RuntimeError("HTTP response wire bound exceeded")
                wire_hash.update(data)
                response.feed(data)
            response.finish()
            date = response.unique("date")
            parsed_date = parsedate_to_datetime(date)
            require(format_datetime(parsed_date, usegmt=True) == date,
                    "response Date is not a unique canonical IMF-fixdate")
            event.update({"httpStatus": response.status, "responseWireBytes": total,
                          "responseWireSha256": wire_hash.hexdigest(), "responseBodyBytes": response.body_bytes,
                          "responseBodySha256": response.body_hash.hexdigest(), "responseDate": date,
                          "closeNotify": "PASS: actual clean TLS EOF"})
            if response.status not in expected:
                raise RuntimeError(f"{case}: expected HTTP {expected}, received {response.status}")
            if response.status == 405:
                event["responseAllow"] = response.unique("allow")
            if binary:
                require(response.unique("content-type") == (response_type or "application/octet-stream")
                        and response.unique("x-content-type-options") == "nosniff",
                        "stream response content/privacy headers mismatch")
                if static:
                    require(response.unique("content-security-policy") == CSP
                            and response.unique("referrer-policy") == "no-referrer"
                            and response.unique("permissions-policy") == "camera=(), geolocation=(), microphone=(self)",
                            "static response security headers mismatch")
                else:
                    require(response.unique("cache-control") == "no-store", "download cache privacy mismatch")
                event["binaryClientRetainedBodyBytes"] = len(response.body)
            elif method != "HEAD" and response.status not in (204, 304):
                response.json()
            event["responseFraming"] = ("HEAD response; CL/TE omitted and zero wire body"
                                        if method == "HEAD" else "status-defined empty; CL/TE omitted by diagnostic contract"
                                        if response.status in (204, 304) else "unique canonical Content-Length")
            event["status"] = "PASS"
            return response
        except Exception as error:
            event["error"] = failure(error, "client/" + case)
            raise
        finally:
            if connection:
                connection.close()
            event["forwardObservationNanos"] = time.monotonic_ns() - started

    def reject_certificate(self, case, wrong_hostname=False):
        event = {"case": case, "status": "FAIL", "serverHostname": "wrong.invalid" if wrong_hostname else "localhost",
                 "trustSource": "only owned public PEM" if wrong_hostname else "default client roots, fixture not added"}
        self.events.append(event)
        connection = None
        try:
            connection = socket.create_connection(("127.0.0.1", self.local_port), timeout=self.remaining())
            connection.settimeout(self.remaining())
            context = self.context if wrong_hostname else self.wrong_ca
            try:
                connection = context.wrap_socket(connection, server_hostname=event["serverHostname"], suppress_ragged_eofs=False)
            except ssl.SSLCertVerificationError as error:
                event.update({"status": "PASS", "rejectionType": "SSLCertVerificationError",
                              "verifyCode": error.verify_code, "verifyMessage": error.verify_message})
                return
            raise RuntimeError("invalid certificate/hostname unexpectedly passed actual TLS verification")
        except Exception as error:
            event["error"] = failure(error, "client/" + case)
            raise
        finally:
            if connection:
                connection.close()


class ProgressReader:
    """Consume only safe progress; preserve every unrelated lifecycle line.

    Uses the launcher's existing pipe-reader threads, never another thread.
    All waits share the original Client deadline and fixed per-wait limits.
    """
    COUNTERS = ("posts", "completions", "idle_cancellations", "idle_returns", "capacity_rejects", "transfers")

    def __init__(self, lines, marker, reports):
        self.lines, self.marker, self.reports = lines, marker, reports
        self.generation = None
        self.latest = None
        self.deferred = deque()
        self.deferred_bytes = 0

    def observe(self, line):
        if not line.startswith(PROGRESS):
            return False
        require(len(line) <= 8192 and len(self.reports) < 512, "bounded native progress exceeded")
        item = json.loads(line[len(PROGRESS):])
        integer = lambda value: type(value) is int and 0 <= value <= (1 << 64) - 1
        require(isinstance(item, dict) and set(item) == {"marker", "generation", "sequence", "acceptance",
                    "jsonBodyRecords", "retired", "rearms", "pendingJobs", "requests"}
                and item.get("marker") == self.marker
                and integer(item.get("generation")) and item["generation"] == self.generation
                and integer(item.get("sequence")) and item["sequence"] > 0, "native progress identity mismatch")
        a = item.get("acceptance")
        require(isinstance(a, dict) and set(a) == set(self.COUNTERS) | {"generation", "ready_slots", "bound_slots",
                    "native_pending", "owned_sockets"}
                and integer(a.get("generation")) and a.get("generation") == self.generation
                and all(integer(a.get(key)) for key in self.COUNTERS)
                and all(type(a.get(key)) is int and 0 <= a[key] <= limit for key, limit in
                        (("ready_slots", 4), ("bound_slots", 4), ("native_pending", 5), ("owned_sockets", 6))),
                "native acceptance counter/resource shape mismatch")
        require(all(integer(item.get(key)) for key in ("retired", "rearms", "pendingJobs", "jsonBodyRecords")),
                "native progress aggregate invalid")
        requests = item.get("requests")
        require(isinstance(requests, list) and len(requests) <= 4
                and len(requests) == item["jsonBodyRecords"], "native JSON-body observation bound mismatch")
        keys = set()
        for request in requests:
            require(isinstance(request, dict) and set(request) ==
                    {"generation", "slot", "serial", "admittedAtNs", "deadlineNs"}
                    and all(integer(value) for value in request.values())
                    and request["generation"] == self.generation and request["slot"] < 4
                    and request["serial"] > 0 and request["admittedAtNs"] < request["deadlineNs"]
                    and request["deadlineNs"] - request["admittedAtNs"] == 120_000_000_000,
                    "native original request interval mismatch")
            key = interval_key(request)
            require(key not in keys, "duplicate native original token")
            keys.add(key)
        if self.latest is not None:
            require(item["sequence"] > self.latest["sequence"]
                    and all(a[key] >= self.latest["acceptance"][key] for key in self.COUNTERS)
                    and item["retired"] >= self.latest["retired"] and item["rearms"] >= self.latest["rearms"],
                    "native progress went backwards")
        self.latest = item
        self.reports.append(item)
        return True

    def get(self, timeout):
        if self.deferred:
            line = self.deferred.popleft()
            self.deferred_bytes -= len(line.encode("utf-8"))
            return line
        until = time.monotonic() + timeout
        while True:
            line = self.lines.get(timeout=max(0, until - time.monotonic()))
            if not self.observe(line):
                return line

    def pump(self, timeout):
        try:
            line = self.lines.get(timeout=timeout)
        except queue.Empty:
            return
        if not self.observe(line):
            size = len(line.encode("utf-8"))
            require(len(self.deferred) < 256 and self.deferred_bytes + size <= 256 * 1024,
                    "bounded deferred native lifecycle lines exceeded")
            self.deferred.append(line)
            self.deferred_bytes += size

    def wait(self, client, predicate, description, budget=3):
        until = min(client.deadline, time.monotonic() + budget)
        while True:
            # Read already queued changes before consulting an older snapshot.
            while not self.lines.empty():
                self.pump(0)
            remaining = until - time.monotonic()
            require(remaining > 0, "native progress barrier expired: " + description)
            if self.latest is not None and predicate(self.latest):
                return self.latest
            self.pump(min(0.05, remaining))


def interval_key(interval):
    return interval["generation"], interval["slot"], interval["serial"]


def intervals(item):
    return {interval_key(entry): entry for entry in item["requests"]}


def unchanged_intervals(item, original):
    return intervals(item) == original


def capacity_transport_rejection(error):
    # No timeout, refusal, generic TLS/certificate failure or alert is evidence
    # that this native capacity owner accepted then closed this exact socket.
    return isinstance(error, (ConnectionResetError, ssl.SSLEOFError, ssl.SSLZeroReturnError))


def exercise_idle_accept(client, progress):
    started = time.monotonic()
    require(client.deadline - started >= 6, "insufficient original client budget for idle acceptance stage")
    baseline = progress.wait(client, lambda p: p["acceptance"]["posts"] > 0,
                             "initial bare accept post", budget=1)
    idle_until = started + 5.05
    while time.monotonic() < idle_until:
        progress.pump(max(0, min(0.05, idle_until - time.monotonic())))
    observed = progress.wait(client, lambda p:
        p["acceptance"]["idle_returns"] > baseline["acceptance"]["idle_returns"]
        and p["acceptance"]["posts"] > baseline["acceptance"]["posts"]
        and p["acceptance"]["native_pending"] >= 1, "idle return and genuine repost", budget=2)
    elapsed = time.monotonic() - started
    require(elapsed >= 5.05, "idle client interval shorter than 5.05 seconds")
    status = client.request("continuous-idle-repost-status", "GET", "status").json()
    require(status.get("online") is True and status.get("apiVersion") == 1,
            "fresh request after idle accept return failed")
    client.events[-1]["idleNoClientSeconds"] = elapsed
    return {"noClientSeconds": elapsed, "baselineSequence": baseline["sequence"],
            "repostedSequence": observed["sequence"], "idleReturns": observed["acceptance"]["idle_returns"],
            "scope": "Application AcceptEx idle return/repost; not kernel ESTABLISHED or strict TCP5s evidence"}


class PendingReceipt:
    """One real authenticated exact-receipt POST with just its last byte held."""
    def __init__(self, client, index, body):
        self.client, self.connection, self.tail = client, None, body[-1:]
        self.started = time.monotonic_ns()
        self.event = {"case": f"continuous-original-{index}-exact-receipt", "status": "FAIL", "method": "POST",
                      "route": "materials/folders/1/1/append", "requestBodyBytes": len(body),
                      "requestBodySha256": digest(body)}
        client.events.append(self.event)
        try:
            require(body.endswith(b"}") and len(body) > 1 and client.cookie and client.csrf,
                    "partial receipt fixture not authenticated/canonical")
            authority = "localhost:" + str(client.ready["port"])
            target = client.ready["basePath"] + "api/v1/materials/folders/1/1/append"
            head = (f"POST {target} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n"
                    f"Origin: {client.ready['origin']}\r\nCookie: {client.cookie}\r\n"
                    f"Content-Type: application/json\r\nContent-Length: {len(body)}\r\n"
                    f"X-FC-CSRF: {client.csrf}\r\n\r\n").encode("ascii")
            connection = socket.create_connection(("127.0.0.1", client.local_port), timeout=client.remaining())
            self.connection = connection
            connection.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
            connection.settimeout(client.remaining())
            self.connection = client.context.wrap_socket(connection, server_hostname="localhost", suppress_ragged_eofs=False)
            connection = self.connection
            require(connection.version() == "TLSv1.2" and connection.getpeercert(binary_form=True) == client.expected_der,
                    "partial request TLS certificate/version mismatch")
            self.event.update({"tlsVersion": connection.version(), "peerDerSha256": digest(client.expected_der),
                               "serverHostname": "localhost", "hostAuthority": authority})
            connection.settimeout(client.remaining())
            connection.sendall(head + body[:-1])
        except Exception as error:
            self.event["error"] = failure(error, "client/" + self.event["case"])
            self.close()
            raise

    def finish(self, expected_receipt):
        try:
            connection = self.connection
            connection.settimeout(self.client.remaining())
            connection.sendall(self.tail)
            response, wire_hash, total = HttpResponse(), hashlib.sha256(), 0
            while True:
                connection.settimeout(self.client.remaining())
                data = connection.recv(FRAGMENT)
                if not data:
                    break  # suppress_ragged_eofs=False: actual TLS close_notify
                total += len(data)
                require(total <= HEAD_LIMIT + BODY_LIMIT, "pending receipt response wire bound exceeded")
                wire_hash.update(data)
                response.feed(data)
            response.finish()
            date = response.unique("date")
            require(format_datetime(parsedate_to_datetime(date), usegmt=True) == date,
                    "pending receipt Date is not canonical IMF-fixdate")
            response.json()
            require(response.status in (200, 201) and response.body == expected_receipt,
                    "original pending POST did not return the identical durable receipt")
            require(time.monotonic() < self.client.deadline, "original client deadline expired before receipt completion")
            self.event.update({"status": "PASS", "httpStatus": response.status, "responseWireBytes": total,
                               "responseWireSha256": wire_hash.hexdigest(), "responseBodyBytes": response.body_bytes,
                               "responseBodySha256": response.body_hash.hexdigest(), "responseDate": date,
                               "responseFraming": "unique canonical Content-Length",
                               "closeNotify": "PASS: actual clean TLS EOF", "exactOriginalReceipt": True})
        except Exception as error:
            self.event["error"] = failure(error, "client/" + self.event["case"])
            raise
        finally:
            self.event["forwardObservationNanos"] = time.monotonic_ns() - self.started
            self.close()

    def close(self):
        if self.connection is not None:
            self.connection.close()
            self.connection = None


def reject_full_capacity(client, index):
    event = {"case": f"continuous-capacity-reject-{index}", "status": "FAIL", "tcpConnected": False}
    client.events.append(event)
    connection = None
    started = time.monotonic_ns()
    try:
        connection = socket.create_connection(("127.0.0.1", client.local_port), timeout=client.remaining())
        event["tcpConnected"] = True
        connection.settimeout(min(2, client.remaining()))
        connection = client.context.wrap_socket(connection, server_hostname="localhost", suppress_ragged_eofs=False,
                                               do_handshake_on_connect=False)
        try:
            connection.do_handshake()
        except Exception as error:
            require(capacity_transport_rejection(error), "capacity did not produce actual EOF/RST/SSLEOF")
            event.update({"status": "PASS", "transportRejection": type(error).__name__})
            return
        raise RuntimeError("full business capacity unexpectedly completed TLS handshake")
    except Exception as error:
        event["error"] = failure(error, "client/" + event["case"])
        raise
    finally:
        if connection is not None:
            connection.close()
        event["forwardObservationNanos"] = time.monotonic_ns() - started


def exercise_capacity_accept(client, progress, body, receipt):
    require(client.deadline - time.monotonic() >= 5, "insufficient original client budget for four-body capacity stage")
    vacant = progress.wait(client, lambda p: p["jsonBodyRecords"] == 0
                           and p["acceptance"]["bound_slots"] == 0 and p["acceptance"]["ready_slots"] == 4,
                           "prior business requests physically retired/rearmed")
    pending, original = [], {}
    try:
        for index in range(1, 5):
            pending.append(PendingReceipt(client, index, body))
            observed = progress.wait(client, lambda p: p["jsonBodyRecords"] == index
                and all(intervals(p).get(key) == value for key, value in original.items()),
                f"original JSON body admission {index}")
            require(len(intervals(observed)) == index, "JSON-body admission token was reused")
            new_key = next(key for key in intervals(observed) if key not in original)
            original = intervals(observed)
            pending[-1].event["originalInterval"] = original[new_key]
        admitted = progress.wait(client, lambda p: p["acceptance"]["bound_slots"] == 4
                                 and p["acceptance"]["transfers"] == vacant["acceptance"]["transfers"] + 4
                                 and p["pendingJobs"] == 0 and unchanged_intervals(p, original),
                                 "four original admitted bodies, no queued worker work")
        for index in (1, 2):
            reject_full_capacity(client, index)
        rejected = progress.wait(client, lambda p:
            p["acceptance"]["capacity_rejects"] == admitted["acceptance"]["capacity_rejects"] + 2
            and p["acceptance"]["bound_slots"] == 4 and unchanged_intervals(p, original),
            "two native exact capacity closes without changing four intervals")
        pending[0].finish(receipt)
        first_key = interval_key(pending[0].event["originalInterval"])
        remaining = {key: value for key, value in original.items() if key != first_key}
        retired = progress.wait(client, lambda p: p["retired"] > rejected["retired"]
            and p["rearms"] > rejected["rearms"] and p["acceptance"]["ready_slots"] >= 1
            and p["acceptance"]["bound_slots"] == 3
            and unchanged_intervals(p, remaining), "first original physically Retired and rearmed")
        require(client.request("continuous-after-retire-status", "GET", "status").json().get("online") is True,
                "new request after exact original retirement failed")
        recovered = progress.wait(client, lambda p: p["retired"] > retired["retired"]
            and p["rearms"] > retired["rearms"] and p["acceptance"]["bound_slots"] == 3
            and unchanged_intervals(p, remaining), "other original intervals unchanged after recovery GET")
        for item in pending[1:]:
            item.finish(receipt)
        return {"admittedSequence": admitted["sequence"], "capacityClosedSequence": rejected["sequence"],
                "retiredRearmedSequence": retired["sequence"], "recoveredSequence": recovered["sequence"],
                "originalIntervals": list(original.values()), "capacityCloses": 2,
                "sameOriginalReceipts": 4, "binaryUploadPermitsUsedByStage": 0,
                "scope": "Real Schannel JSON-body admission/capacity/retirement; no kernel TCP timestamp claim"}
    finally:
        for item in pending:
            item.close()


def silent_wav():
    # Domain minimum: 8000 frames / 500ms, never a playback/broadcast command.
    pcm = bytes(8000 * 2)
    return (b"RIFF" + struct.pack("<I", 36 + len(pcm)) + b"WAVEfmt "
            + struct.pack("<IHHIIHH", 16, 1, 1, 16000, 32000, 2, 16)
            + b"data" + struct.pack("<I", len(pcm)) + pcm)


def require(value, message):
    if not value:
        raise RuntimeError(message)


def static_mime(path):
    if path == "index.html":
        return "text/html; charset=utf-8"
    return {"js": "text/javascript; charset=utf-8", "css": "text/css; charset=utf-8",
            "svg": "image/svg+xml", "png": "image/png", "jpg": "image/jpeg", "jpeg": "image/jpeg",
            "webp": "image/webp"}.get(path.rsplit(".", 1)[-1])


def read_web_package(source):
    """Read only bounded manifest entries; never extract request-controlled paths."""
    def unique_object(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, "duplicate package manifest field")
            result[key] = value
        return result
    with zipfile.ZipFile(source) as archive:
        names = archive.namelist()
        require(names.count("web/fc-assets.json") == 1, "missing/duplicate web manifest in supplied ZIP")
        info = archive.getinfo("web/fc-assets.json")
        require(0 < info.file_size <= BODY_LIMIT, "web manifest exceeded 64KiB")
        manifest_bytes = archive.read(info)
        manifest = json.loads(manifest_bytes.decode("utf-8", "strict"), object_pairs_hook=unique_object)
        require(isinstance(manifest, dict)
                and set(manifest) == {"schemaVersion", "apiVersion", "uiCompatibilityVersion", "assets"}
                and all(type(manifest[k]) is int and manifest[k] == 1
                        for k in ("schemaVersion", "apiVersion", "uiCompatibilityVersion")),
                "web manifest version/fields mismatch")
        assets = manifest["assets"]
        require(isinstance(assets, list) and 1 <= len(assets) <= 64, "web manifest entry count invalid")
        payloads, seen, total = [], set(), 0
        for asset in assets:
            require(isinstance(asset, dict) and set(asset) == {"path", "sizeBytes", "sha256"},
                    "web asset fields mismatch")
            path = asset["path"]
            require(isinstance(path, str) and (path == "index.html" or re.fullmatch(r"assets/[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}", path))
                    and static_mime(path) is not None and path.lower() not in seen,
                    "web asset path invalid/duplicated")
            if path.startswith("assets/"):
                stem = path[7:].split(".", 1)[0].upper()
                require(not re.fullmatch(r"CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9]", stem), "Windows reserved asset name")
            seen.add(path.lower())
            size, sha = asset["sizeBytes"], asset["sha256"]
            require(type(size) is int and 0 < size <= STATIC_LIMIT and isinstance(sha, str)
                    and re.fullmatch(r"[0-9a-f]{64}", sha), "web asset size/digest invalid")
            total += size
            require(total <= STATIC_LIMIT, "web bundle exceeded 5MiB")
            member = "web/" + path
            require(names.count(member) == 1, "missing/duplicate ZIP asset")
            info = archive.getinfo(member)
            require(info.file_size == size and not info.is_dir(), "ZIP asset size mismatch")
            data = archive.read(info)
            require(len(data) == size and digest(data) == sha, "ZIP asset digest mismatch")
            payloads.append((path, data))
        require("index.html" in seen, "web entry document missing")
    return manifest_bytes, payloads, {"manifestSha256": digest(manifest_bytes), "payloadBytes": total, "assets": assets}


def upload_web_package(remote, local_root, directory, marker, package):
    manifest, payloads, summary = package
    owned_root = Path(local_root) / "web-package"
    owned_root.mkdir()
    remote.powershell("$root=" + ps_literal(directory)
        + ";if(([IO.File]::ReadAllText((Join-Path $root '.tls-probe-root')))-ne " + ps_literal(marker)
        + "){throw 'static package owner mismatch'};$pkg=Join-Path $root 'web-package';"
        + "if(Test-Path -LiteralPath $pkg){throw 'static package already exists'};"
        + "New-Item -ItemType Directory -Path $pkg|Out-Null;"
        + "New-Item -ItemType Directory -Path (Join-Path $pkg 'assets')|Out-Null;'STATIC_PACKAGE_ROOT_CREATED'")
    expected = {}
    for path, data in [("fc-assets.json", manifest)] + payloads:
        source = owned_root / path
        source.parent.mkdir(parents=True, exist_ok=True)
        source.write_bytes(data)
        remote.upload(source, directory + "\\web-package" + ("\\assets" if path.startswith("assets/") else ""))
        expected[path] = {"bytes": len(data), "sha256": digest(data)}
    paths = ",".join(ps_literal(path) for path in expected)
    rows = json.loads(remote.powershell("$pkg=Join-Path " + ps_literal(directory) + " 'web-package';@(@(" + paths
        + ")|ForEach-Object{$f=Join-Path $pkg $_;@{path=$_;bytes=(Get-Item -LiteralPath $f).Length;"
        + "sha256=(Get-FileHash -LiteralPath $f -Algorithm SHA256).Hash.ToLowerInvariant()}})|ConvertTo-Json -Compress"))
    require(isinstance(rows, list) and len(rows) == len(expected), "uploaded static package inventory mismatch")
    actual = {row["path"]: {"bytes": row["bytes"], "sha256": row["sha256"]} for row in rows}
    require(actual == expected, "uploaded static package changed bytes/digest")
    return dict(summary, uploadedFiles=rows, uploadVerification="PASS")


def exercise_head(client):
    """Every current HEAD target is rejected; its error still has no wire body."""
    cases = [
        ("static-head-method", "index.html", True, None, 405, "GET"),
        ("static-head-host", "index.html", True, "wrong.invalid", 403, None),
        ("api-head-get", "status", False, None, 405, "GET"),
        ("api-head-post", "pairing/requests", False, None, 405, "POST"),
        ("api-head-shared", "voice/broadcasts", False, None, 405, "GET, POST"),
        ("api-head-unknown", "not-a-route", False, None, 404, None),
        ("api-head-host", "status", False, "wrong.invalid", 403, None),
    ]
    results = []
    for case, route, static, host, status, allowed in cases:
        rejected = client.request(case, "HEAD", route, static=static, expected=(status,),
                                  authenticated=False, request_host=host)
        require(rejected.length == rejected.body_bytes == 0 and not rejected.body
                and rejected.body_hash.hexdigest() == digest(b"")
                and "content-length" not in rejected.headers and "transfer-encoding" not in rejected.headers,
                "HEAD response contained a body/framing field: " + case)
        if allowed is not None:
            require(rejected.unique("allow") == allowed, "HEAD 405 Allow mismatch: " + case)
        else:
            require("allow" not in rejected.headers, "non-405 HEAD unexpectedly supplied Allow: " + case)
        results.append({"case": case, "httpStatus": rejected.status, "wireBodyBytes": rejected.body_bytes,
                        "allow": allowed, "status": "PASS"})
    return results


def exercise_static(client, package):
    require(client.cookie is None, "static public test must precede pairing")
    heads = exercise_head(client)
    tested = []
    for asset in package["assets"]:
        path = asset["path"]
        route = "" if path == "index.html" else path
        response = client.request("static-" + path, "GET", route, static=True, binary=True,
                                  authenticated=False, response_type=static_mime(path))
        cache = "no-cache" if path == "index.html" else "public, max-age=31536000, immutable"
        require(response.body_bytes == asset["sizeBytes"] and response.body_hash.hexdigest() == asset["sha256"]
                and response.unique("etag") == '"' + asset["sha256"] + '"'
                and response.unique("cache-control") == cache and not response.body,
                "static payload/cache/hash mismatch: " + path)
        tested.append({"path": path, "bytes": response.body_bytes, "sha256": response.body_hash.hexdigest()})
        if path == "index.html" or path.endswith(".css"):
            cached = client.request("static-304-" + path, "GET", path, expected=(304,), static=True,
                                    binary=True, authenticated=False, response_type=static_mime(path),
                                    extra_headers=(("If-None-Match", 'W/' + response.unique("etag") + ', "miss"'),))
            require(cached.body_bytes == 0 and cached.length == 0
                    and "content-length" not in cached.headers and "transfer-encoding" not in cached.headers
                    and cached.unique("etag") == response.unique("etag")
                    and cached.unique("cache-control") == cache, "static conditional response mismatch")
    for case, method, route, expected, headers, origin in [
        ("static-missing", "GET", "assets/missing.js", 404, (), None),
        ("static-traversal", "GET", "assets/../index.html", 404, (), None),
        ("static-query-not-fallback", "GET", "index.html?unknown=1", 404, (), None),
        ("static-bad-origin", "GET", "index.html", 403, (), "https://wrong.invalid"),
        ("static-cross-site", "GET", "index.html", 403, (("Sec-Fetch-Site", "cross-site"),), None),
        ("unknown-api-not-html", "GET", "api/v1/not-a-route", 404, (), None),
        ("static-wrong-method", "POST", "index.html", 405, (), None),
    ]:
        rejected = client.request(case, method, route, static=True, expected=(expected,), authenticated=False,
                                  extra_headers=headers, origin=origin)
        if expected == 405:
            require(rejected.unique("allow") == "GET", "static 405 Allow mismatch")
    return {"status": "PASS", "assets": tested, "conditionalResponses": 2, "negativePaths": 7, "headCases": heads,
            "source": "verified ZIP manifest; public resources before pairing", "browser": "NOT_RUN"}


def authenticate_business(client):
    for route, allowed in [("status", "GET"), ("pairing/requests", "POST"),
                           ("voice/broadcasts", "GET, POST")]:
        rejected = client.request("api-wrong-method-" + route, "DELETE", route, expected=(405,),
                                  authenticated=False)
        require(rejected.unique("allow") == allowed and rejected.json().get("code") == "METHOD_NOT_ALLOWED",
                "API 405 Allow mismatch: " + route)
    status = client.request("public-status", "GET", "status").json()
    require(status.get("online") is True and status.get("apiVersion") == 1, "public status payload mismatch")
    require(client.request("unauthorized-session", "GET", "session", expected=(401,)).json().get("code") == "UNAUTHORIZED",
            "private session was not rejected as unauthorized")
    pairing = json_bytes({"schemaVersion": 1, "code": client.ready["pairingCode"],
                          "loginName": client.ready["loginName"], "deviceName": "Synthetic TLS business probe"})
    require(client.request("bad-origin-pairing", "POST", "pairing/requests", pairing, expected=(403,),
                           origin="https://wrong.invalid").json().get("code") == "FORBIDDEN", "bad Origin not forbidden")
    requested = client.request("pairing-request", "POST", "pairing/requests", pairing, expected=(202,)).json()
    request_id, poll_secret = requested.get("requestId"), requested.get("pollSecret")
    require(requested.get("status") == "PENDING" and isinstance(request_id, str)
            and re.fullmatch(r"[1-9][0-9]{0,19}", request_id) and isinstance(poll_secret, str)
            and re.fullmatch(r"[0-9a-fA-F]{64}", poll_secret), "pairing receipt contract failed")
    poll_body = json_bytes({"schemaVersion": 1, "pollSecret": poll_secret})
    authorized = None
    for index in range(8):
        # Respect the API's 500ms minimum; never hammer/retry an upload/write.
        time.sleep(0.65)
        polled = client.request("pairing-poll-" + str(index + 1), "POST", f"pairing/requests/{request_id}/poll",
                                poll_body, expected=(200, 202))
        if polled.status == 200:
            authorized = polled
            break
        require(polled.json().get("status") == "PENDING", "unexpected pending pairing status")
    require(authorized is not None and authorized.json().get("status") == "AUTHORIZED", "synthetic local approval not observed")
    cookie = authorized.unique("set-cookie")
    require(re.fullmatch(r"__Host-FutureClassroom=[0-9a-fA-F]{64}; Path=/; Secure; HttpOnly; SameSite=Strict", cookie),
            "host-only secure session cookie contract failed")
    client.cookie = cookie.split(";", 1)[0]
    session = client.request("authenticated-session", "GET", "session").json()
    require(session.get("status") == "AUTHENTICATED" and isinstance(session.get("csrf"), str)
            and re.fullmatch(r"[0-9a-fA-F]{64}", session["csrf"]), "authenticated session/CSRF payload mismatch")
    client.csrf = session["csrf"]


def validate_visual(value, key, fixture):
    require(fixture in VISUAL_FIXTURES, "unknown visual fixture")
    zero = fixture == "FILLED_ZERO"
    expected = {"schemaVersion": 1, "status": "SUBJECT_VISUAL", "key": key,
                "subjectId": "1", "subjectName": "Synthetic TLS subject", "subjectRevision": "1",
                "displayRevision": "0" if fixture == "FILLED_DEFAULT" else "1", "rulesVersion": 1,
                "effectiveRgb": "#88A6C4", "mode": "OUTLINED" if fixture == "OUTLINED" else "FILLED",
                "homeworkFillPercent": 0 if zero else 3, "courseFillPercent": 0 if zero else 20,
                "homeworkComponentOpacityPercent": 70 if zero else 100,
                "courseComponentOpacityPercent": 80 if zero else 100,
                "homeworkVisible": True, "courseVisible": True}
    require(type(value) is dict and value == expected
            and all(type(value[k]) is type(v) for k, v in expected.items()),
            "single-subject visual exact derived RGB/mode/independent alpha/version/disclosure contract failed")
    return expected


def exercise_visuals(client):
    scopes = client.request("visual-owned-scopes", "GET", "content/homework/scopes/0").json()
    require(scopes.get("status") == "SCOPES" and scopes.get("total") == 1
            and scopes.get("nextOffset") is None and len(scopes.get("items", [])) == 1,
            "visual fixture owned scope missing")
    scope = scopes["items"][0]
    require(scope.get("classId") == "1" and scope.get("subjectId") == "1"
            and scope.get("ownerTeacherId") == "1" and re.fullmatch(r"[0-9]{4}-[0-9]{2}-[0-9]{2}", scopes.get("today", "")),
            "visual fixture scope identity mismatch")
    key = {"kind": "HOMEWORK", "classId": "1", "subjectId": "1", "ownerTeacherId": "1", "date": scopes["today"]}
    content_route = "content/homework/1/1/1/" + scopes["today"]
    route = content_route + "/visual"
    before = client.request("visual-content-before", "GET", content_route).json()
    response = client.request("visual-read", "GET", route)
    value = response.json()
    validate_visual(value, key, client.ready["visualFixture"])
    require(client.request("visual-no-session", "GET", route, expected=(401,), authenticated=False).json().get("code") == "UNAUTHORIZED",
            "visual preview disclosed without exact authenticated session")
    outside = "content/homework/2/1/1/" + scopes["today"] + "/visual"
    require(client.request("visual-other-class", "GET", outside, expected=(403,)).json().get("code") == "FORBIDDEN",
            "visual preview disclosed outside exact teaching class")
    for method in ("POST", "DELETE"):
        rejected = client.request("visual-write-method-" + method, method, route, expected=(405,))
        require(rejected.unique("allow") == "GET" and rejected.json().get("code") == "METHOD_NOT_ALLOWED",
                "visual preview accepted a write method")
    mixed = json_bytes({"schemaVersion": 1, "commandId": "9001099", "expectedRevision": before["revision"],
                        "text": "Synthetic rejected visual write", "effectiveRgb": "#FFFFFF"})
    require(client.request("visual-fields-cannot-write-content", "POST", content_route, mixed, expected=(400,)).json().get("code") == "INVALID_REQUEST",
            "content write schema accepted visual management input")
    after = client.request("visual-content-after", "GET", content_route).json()
    require(all(before[k] == after[k] for k in ("key", "revision", "text")),
            "readonly preview/rejected visual writes mutated teacher content")
    reread = client.request("visual-explicit-reread", "GET", route)
    require(reread.body == response.body, "readonly refresh changed palette/display without a management commit")
    return {"status": "PASS", "fixture": client.ready["visualFixture"], "preview": value,
            "previewSha256": digest(response.body), "contentRevisionBefore": before["revision"],
            "contentRevisionAfter": after["revision"], "readOnly": True,
            "nativeDesktopPixels": "NOT_OBSERVED", "wallpaperContrast": "NOT_CERTIFIED"}


def exercise(client, browser_observer=None, continuous=None, visual_only=False):
    authenticate_business(client)
    visuals = exercise_visuals(client)
    if browser_observer is not None:
        # Keep the original one approved session and the original client
        # deadline. The page itself must read session/CSRF through the API.
        browser_observer.run(client)
    if visual_only:
        client.reject_certificate("wrong-ca-rejected")
        client.reject_certificate("wrong-hostname-rejected", wrong_hostname=True)
        return {"subjectVisuals": visuals, "scope": "TARGETED_VISUAL_MATRIX_NO_MATERIAL_OR_AUDIO_OPERATIONS"}
    material = (b"Synthetic TLS business probe material.\n" * 4000)[:MATERIAL_BYTES]
    require(len(material) == MATERIAL_BYTES, "synthetic material length changed")
    created = client.request("material-create", "POST", "materials/uploads/1/1",
                             json_bytes({"schemaVersion": 1, "displayName": "tls-business.txt", "bytes": len(material)}),
                             expected=(201,)).json()
    upload_id = created.get("uploadId")
    require(created.get("status") == "UPLOAD_READY" and isinstance(upload_id, str)
            and re.fullmatch(r"[1-9][0-9]{0,19}", upload_id), "material reservation payload mismatch")
    uploaded = client.request("material-binary-stream", "POST", f"materials/uploads/{upload_id}/bytes", material,
                              content_type="application/octet-stream").json()
    require(uploaded.get("status") == "UPLOADED" and uploaded.get("uploadId") == upload_id
            and uploaded.get("bytes") == len(material), "actual binary material completion mismatch")
    cancelled_ids = []
    # Keep A Ready while B fills the second slot. JSON cancel removes B from
    # the application; C must reap B's absent Hub entry, keep A live, and reuse
    # the second slot. The later successful append/download still proves A.
    for cycle in (1, 2):
        created = client.request(f"cancel-cycle-{cycle}-create", "POST", "materials/uploads/1/1",
                                 json_bytes({"schemaVersion": 1, "displayName": f"cancel-{cycle}.txt",
                                             "bytes": len(material)}), expected=(201,)).json()
        cancelled_id = created.get("uploadId")
        require(isinstance(cancelled_id, str) and re.fullmatch(r"[1-9][0-9]{0,19}", cancelled_id),
                "cancel cycle upload ID missing")
        require(cancelled_id != upload_id and cancelled_id not in cancelled_ids,
                "cancel cycle unexpectedly aliased an original upload")
        uploaded = client.request(f"cancel-cycle-{cycle}-binary", "POST",
                                  f"materials/uploads/{cancelled_id}/bytes", material,
                                  content_type="application/octet-stream").json()
        require(uploaded.get("status") == "UPLOADED" and uploaded.get("uploadId") == cancelled_id,
                "cancel cycle failed to reuse a physical/Hub upload slot")
        cancelled = client.request(f"cancel-cycle-{cycle}-json-cancel", "POST",
                                   f"materials/uploads/{cancelled_id}/cancel",
                                   json_bytes({"schemaVersion": 1})).json()
        require(cancelled.get("status") == "UPLOAD_CANCELLED" and cancelled.get("uploadId") == cancelled_id,
                "actual JSON upload cancellation response mismatch")
        cancelled_ids.append(cancelled_id)
    append_body = json_bytes({"schemaVersion": 1, "commandId": COMMAND_ID, "uploadId": upload_id, "allowedClassIds": ["1"]})
    first = client.request("material-append", "POST", "materials/folders/1/1/append", append_body, expected=(200, 201))
    committed = first.json()
    record = committed.get("record", {})
    require(committed.get("status") == "COMMITTED" and committed.get("commandId") == COMMAND_ID
            and committed.get("scope") == {"ownerTeacherId": "1", "subjectId": "1"}
            and record.get("bytes") == len(material) and record.get("sha256") == digest(material)
            and record.get("allowedClassIds") == ["1"] and record.get("kind") == "TXT"
            and record.get("resourceVersionId") == upload_id and record.get("status") == "PUBLISHED",
            "actual durable material receipt/content hash mismatch")
    retry = client.request("material-append-exact-retry", "POST", "materials/folders/1/1/append", append_body, expected=(200, 201))
    require(retry.body == first.body, "exact append retry did not return the identical committed receipt")
    changed = json_bytes({"schemaVersion": 1, "commandId": COMMAND_ID, "uploadId": upload_id, "allowedClassIds": ["2"]})
    conflict = client.request("material-append-changed-classes", "POST", "materials/folders/1/1/append", changed, expected=(409,)).json()
    require(conflict.get("code") == "IDEMPOTENCY_CONFLICT", "changed append did not return idempotency conflict")
    capacity = exercise_capacity_accept(client, continuous, append_body, first.body) if continuous else None
    material_id = record.get("materialId")
    require(isinstance(material_id, str) and re.fullmatch(r"[1-9][0-9]{0,19}", material_id),
            "committed download material identifier missing")
    download_route = f"materials/download/1/1/{material_id}/{upload_id}"
    require(client.request("unauthorized-material-download", "GET", download_route, expected=(401,),
                           authenticated=False).json().get("code") == "UNAUTHORIZED",
            "private binary download was not rejected without its exact session")
    downloaded = client.request("material-binary-download", "GET", download_route, binary=True)
    require(downloaded.body_bytes == len(material) and downloaded.body_hash.hexdigest() == digest(material)
            and downloaded.unique("x-fc-sha256") == digest(material)
            and downloaded.unique("content-disposition") == "attachment; filename*=UTF-8''tls-business.txt"
            and not downloaded.body,
            "actual bounded binary download content/hash/disposition mismatch")
    wav = silent_wav()
    voice = client.request("silent-voice-upload-only", "POST", "voice/uploads?classId=1", wav, expected=(201,),
                           content_type="audio/wav").json()
    require(voice.get("status") == "VOICE_UPLOADED" and voice.get("classId") == "1"
            and voice.get("bytes") == len(wav) and voice.get("durationMillis") == 500
            and isinstance(voice.get("assetId"), str) and re.fullmatch(r"[1-9][0-9]{0,19}", voice["assetId"]),
            "actual silent PCM WAV upload response mismatch")
    client.reject_certificate("wrong-ca-rejected")
    client.reject_certificate("wrong-hostname-rejected", wrong_hostname=True)
    return {"subjectVisuals": visuals, "continuousAcceptance": capacity, "materialBytes": len(material), "materialSha256": digest(material), "uploadId": upload_id,
            "commandId": COMMAND_ID, "receiptSha256": digest(first.body), "exactRetryReceipt": True,
            "downloadBytes": downloaded.body_bytes, "downloadSha256": downloaded.body_hash.hexdigest(),
            "downloadClientRetainedBodyBytes": len(downloaded.body),
            "jsonCancelledReadyIds": cancelled_ids, "subsequentBinaryBeginReusedHubSlot": True,
            "voiceWavBytes": len(wav), "voiceSamples": 8000, "voiceDurationMillis": 500,
            "voicePcmAllZero": True, "voiceOperation": "UPLOAD_ONLY_NO_BROADCAST_OR_PLAYBACK"}


def cleanup_owned(remote, directory, marker, uploaded, report_path, token, report):
    """Same stop -> capture/hash/download -> certificate/key/root order as old runner."""
    stopped = False
    try:
        if "stop" in uploaded:
            report["ownedProcessCleanup"] = json.loads(remote.run_file(directory + "\\tls-stop.ps1", timeout=25))
            stopped = report["ownedProcessCleanup"].get("status") == "PASS"
        else:
            stopped = "setup" not in uploaded
    except Exception as error:
        report["processCleanupError"] = failure(error, "stop-owned-native-and-supervisor-processes")
        report["status"] = "FAIL"
    raw_preserved = "setup" not in uploaded
    try:
        if "capture" in uploaded:
            require(stopped, "physical process stop unproven; original logs may still change")
            metadata = json.loads(remote.run_file(directory + "\\tls-capture.ps1", timeout=30))
            report["remoteRawEvidence"] = metadata
            _tls.preserve_raw(remote, directory, report_path, token, metadata, report)
            raw_preserved = True
    except Exception as error:
        report["remoteCaptureError"] = failure(error, "preserve-complete-original-artifacts")
        report["status"] = "FAIL"
    try:
        require(stopped, "owned process stop unproven; preserving certificate/key/root")
        require(raw_preserved, "original evidence preservation unproven; retaining remote root")
        if "cleanup" in uploaded:
            report["cleanupDetails"] = json.loads(remote.run_file(directory + "\\tls-cleanup.ps1", timeout=30))
        else:
            outcome = remote.powershell("$root=" + ps_literal(directory)
                + ";if(([IO.File]::ReadAllText((Join-Path $root '.tls-probe-root')))-ne " + ps_literal(marker)
                + "){throw 'TLS early cleanup marker mismatch'};Remove-Item -LiteralPath $root -Recurse -Force;"
                + "if(Test-Path -LiteralPath $root){throw 'TLS early root remains'};'TLS_EARLY_CLEANUP_PASS'", timeout=15)
            report["cleanupDetails"] = {"status": "PASS" if outcome == "TLS_EARLY_CLEANUP_PASS" else "FAIL",
                                        "scope": "pre-fixture upload failure; no key/process created"}
        report["cleanup"] = report["cleanupDetails"]["status"]
    except Exception as error:
        report.update({"cleanup": "FAIL", "status": "FAIL", "cleanupError": failure(error, "cleanup-owned-fixture"),
                       "retainedRemoteEvidence": {"root": directory, "processStopProven": stopped,
                       "completeLocalArtifactsProven": raw_preserved, "cleanupIncomplete": True,
                       "nextAction": "recover complete originals then use this root's exact tls-cleanup.ps1; retain SSH authorization"}})


def self_test():
    """Pure local framing/fixture checks; no sockets, subprocesses or Windows IO."""
    checks = []
    progress_queue, progress_log = queue.Queue(), []
    reader = ProgressReader(progress_queue, "synthetic", progress_log)
    reader.generation = 7
    original = {"generation": 7, "slot": 0, "serial": 3, "admittedAtNs": 123,
                "deadlineNs": 120_000_000_123}
    sample = {"marker": "synthetic", "generation": 7, "sequence": 1,
              "acceptance": {"generation": 7, "posts": 2, "completions": 1,
                  "idle_cancellations": 0, "idle_returns": 0, "capacity_rejects": 0, "transfers": 1,
                  "ready_slots": 3, "bound_slots": 1, "native_pending": 2, "owned_sockets": 3},
              "jsonBodyRecords": 1, "retired": 0, "rearms": 0, "pendingJobs": 0, "requests": [original]}
    progress_queue.put(PROGRESS + json.dumps(sample))
    lifecycle = [PASS + "{}", EXIT_PREFIXES[0] + "{}", READY + "{}"]
    for line in lifecycle:
        progress_queue.put(line)
    for _ in range(4):
        reader.pump(0)
    require(reader.latest == sample and [reader.get(0) for _ in lifecycle] == lifecycle
            and len(progress_log) == 1 and reader.deferred_bytes == 0,
            "progress reader lost/reordered unrelated lifecycle lines")
    for changed in (dict(original, serial=4), dict(original, admittedAtNs=124), dict(original, deadlineNs=120_000_000_124)):
        require(not unchanged_intervals(dict(sample, requests=[changed]), intervals(sample)),
                "original request binding accepted changed token/admission/deadline")
    for changed in (dict(sample, sequence=1), dict(sample, marker="foreign"),
                    dict(sample, generation=8), dict(sample, sequence=2, requests=[dict(original, deadlineNs=123)]),
                    dict(sample, sequence=2, acceptance=dict(sample["acceptance"], owned_sockets=7))):
        try:
            reader.observe(PROGRESS + json.dumps(changed))
        except RuntimeError:
            continue
        raise RuntimeError("progress reader accepted stale/foreign/unbounded data")
    # Busy timing, refused TCP and certificate failures cannot masquerade as capacity rejection.
    for error in (TimeoutError(), ConnectionRefusedError(), ssl.SSLCertVerificationError(1, "synthetic"),
                  ssl.SSLError(1, "synthetic"), BrokenPipeError()):
        require(not capacity_transport_rejection(error), "unproven transport failure accepted as capacity close")
    for error in (ConnectionResetError(), ssl.SSLEOFError(8, "synthetic"), ssl.SSLZeroReturnError(6, "synthetic")):
        require(capacity_transport_rejection(error), "exact EOF/RST transport class not recognized")
    checks.append("synthetic progress identity/interval bounds; unrelated lifecycle lines preserved; only EOF/RST capacity classes accepted (no real IO)")
    wire = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Type: application/json\r\n\r\n{}"
    for step in (1, 7, len(wire)):
        parsed = HttpResponse()
        for offset in range(0, len(wire), step):
            parsed.feed(wire[offset:offset + step])
        require(parsed.finish().status == 200 and parsed.body == b"{}", "pure fragmented framing check failed")
    checks.append("fragmented/exact-CL response framing")
    rejected = [wire + b"extra", wire[:-1], wire.replace(b"Content-Length: 2", b"Content-Length: 2\r\nContent-Length: 2"),
                wire.replace(b"Content-Length: 2\r\n", b""),
                wire.replace(b"Content-Length: 2", b"Content-Length: 02"),
                wire.replace(b"Content-Length: 2", b"Content-Length: +2"),
                wire.replace(b"Content-Length: 2", b"Content-Length: 2, 2"),
                wire.replace(b"Content-Length: 2", b"Transfer-Encoding: chunked\r\nContent-Length: 2"),
                wire.replace(b"Content-Length: 2", b"Content-Length: 65537"),
                b"HTTP/1.1 200 OK\r\n" + b"X" * HEAD_LIMIT, wire.replace(b"Content-Type:", b" Content-Type:")]
    for invalid in rejected:
        try:
            parsed = HttpResponse()
            parsed.feed(invalid)
            parsed.finish()
        except (RuntimeError, UnicodeDecodeError):
            continue
        raise RuntimeError("pure parser accepted invalid/oversize/truncated framing")
    checks.append("surplus/truncation/duplicate-CL/TE/head-body limits/folding rejected")
    for status in (204, 304):
        empty_wire = (f"HTTP/1.1 {status} Empty\r\nETag: \"synthetic\"\r\n\r\n").encode("ascii")
        for step in (1, 7, len(empty_wire)):
            parsed = HttpResponse(binary=True)
            for offset in range(0, len(empty_wire), step):
                parsed.feed(empty_wire[offset:offset + step])
            parsed.finish()
            require(parsed.status == status and parsed.length == 0 and parsed.body_bytes == 0
                    and not parsed.body and parsed.body_hash.hexdigest() == digest(b"")
                    and "content-length" not in parsed.headers and "transfer-encoding" not in parsed.headers,
                    "diagnostic empty-status omitted-CL/wire-zero contract failed")
        invalid_empty = [
            empty_wire + b"x",
            empty_wire.replace(b"ETag:", b"ETag: \"duplicate\"\r\nETag:"),
            empty_wire.replace(b"ETag:", b"Content-Length: 0\r\nETag:"),
            empty_wire.replace(b"ETag:", b"Content-Length: 1\r\nETag:") + b"x",
            empty_wire.replace(b"ETag:", b"Content-Length: 0\r\ncOnTeNt-LeNgTh: 0\r\nETag:"),
            empty_wire.replace(b"ETag:", b"Transfer-Encoding: chunked\r\nETag:"),
            empty_wire.replace(b"ETag:", b"Transfer-Encoding: identity\r\nContent-Length: 0\r\nETag:"),
        ]
        for invalid in invalid_empty:
            for step in (1, len(invalid)):
                try:
                    parsed = HttpResponse(binary=True)
                    for offset in range(0, len(invalid), step):
                        parsed.feed(invalid[offset:offset + step])
                    parsed.finish()
                except (RuntimeError, UnicodeDecodeError):
                    continue
                raise RuntimeError("diagnostic empty-status parser accepted CL/body/duplicate-header/TE")
    checks.append("204/304 omit CL/TE and count/hash zero wire body; CL0/body/duplicate-header/TE rejected")
    for status in (200, 403, 404, 405):
        head_wire = (f"HTTP/1.1 {status} Head\r\nDate: Sat, 31 Oct 2026 00:00:00 GMT\r\n\r\n").encode("ascii")
        for step in (1, 7, len(head_wire)):
            parsed = HttpResponse(head_only=True)
            for offset in range(0, len(head_wire), step):
                parsed.feed(head_wire[offset:offset + step])
            parsed.finish()
            require(parsed.status == status and parsed.length == parsed.body_bytes == 0
                    and not parsed.body and parsed.body_hash.hexdigest() == digest(b""),
                    "HEAD diagnostic parser failed status-independent zero wire body")
        invalid_heads = [head_wire + b"x", head_wire.replace(b"Date:", b"Content-Length: 0\r\nDate:"),
                         head_wire.replace(b"Date:", b"Content-Length: 2\r\nDate:"),
                         head_wire.replace(b"Date:", b"Transfer-Encoding: chunked\r\nDate:"),
                         head_wire.replace(b"Date:", b"Date: duplicate\r\nDate:")]
        for invalid in invalid_heads:
            for step in (1, len(invalid)):
                try:
                    parsed = HttpResponse(head_only=True)
                    for offset in range(0, len(invalid), step):
                        parsed.feed(invalid[offset:offset + step])
                    parsed.finish()
                except RuntimeError:
                    continue
                raise RuntimeError("HEAD diagnostic parser accepted CL/TE/duplicate fields/body")
        try:
            parsed = HttpResponse()
            parsed.feed(head_wire)
            parsed.finish()
        except RuntimeError:
            pass
        else:
            raise RuntimeError("HEAD parser mode leaked into ordinary GET response framing")
    checks.append("HEAD 200/403/404/405 require omitted CL/TE and zero wire body; ordinary GET remains strict")
    # The zero-body rule never bypasses the original static 5MiB bound for 200.
    try:
        parsed = HttpResponse(binary=True, body_limit=STATIC_LIMIT)
        parsed.feed(b"HTTP/1.1 200 OK\r\nContent-Length: " + str(STATIC_LIMIT + 1).encode("ascii") + b"\r\n\r\n")
    except RuntimeError:
        pass
    else:
        raise RuntimeError("static 200 body bound exceeded without rejection")
    checks.append("200 still requires canonical unique CL, JSON64KiB/static5MiB/download50MiB caps unchanged")
    binary_body = b"synthetic streamed download\n" * 5000
    binary_head = (b"HTTP/1.1 200 OK\r\nContent-Length: " + str(len(binary_body)).encode("ascii")
                   + b"\r\nContent-Type: application/octet-stream\r\n\r\n")
    parsed = HttpResponse(binary=True)
    parsed.feed(binary_head)
    for offset in range(0, len(binary_body), FRAGMENT):
        parsed.feed(binary_body[offset:offset + FRAGMENT])
        require(not parsed.body and not parsed.pending, "binary client retained a full response body")
    parsed.finish()
    require(parsed.body_bytes == len(binary_body) and parsed.body_hash.hexdigest() == digest(binary_body),
            "streamed binary hash/count mismatch")
    for invalid in (binary_head + binary_body[:-1], binary_head + binary_body + b"x",
                    binary_head.replace(str(len(binary_body)).encode("ascii"), str(DOWNLOAD_LIMIT + 1).encode("ascii"))):
        try:
            parsed = HttpResponse(binary=True)
            parsed.feed(invalid)
            parsed.finish()
        except RuntimeError:
            continue
        raise RuntimeError("binary stream accepted truncation/surplus/oversize")
    checks.append("binary streaming hash/count without retained body; truncation/surplus/50MiB cap rejected")
    fixture_assets = [{"path": path, "sizeBytes": 3, "sha256": digest(b"abc")}
                      for path in ("index.html", "assets/app-abcdeF12.js")]
    def fixture_zip(assets, payload=b"abc", version=1):
        source = io.BytesIO()
        manifest = {"schemaVersion": version, "apiVersion": 1, "uiCompatibilityVersion": 1, "assets": assets}
        with zipfile.ZipFile(source, "w") as archive:
            archive.writestr("web/fc-assets.json", json_bytes(manifest))
            archive.writestr("web/index.html", payload)
            archive.writestr("web/assets/app-abcdeF12.js", payload)
        source.seek(0)
        return source
    _, loaded, summary = read_web_package(fixture_zip(fixture_assets))
    require(len(loaded) == 2 and summary["payloadBytes"] == 6, "pure manifest/payload verification failed")
    invalid_packages = [fixture_zip(fixture_assets, payload=b"bad"), fixture_zip(fixture_assets, version=2),
                        fixture_zip(fixture_assets[1:]), fixture_zip(fixture_assets + [fixture_assets[0]]),
                        fixture_zip([dict(fixture_assets[0], path="../index.html")]),
                        fixture_zip([dict(fixture_assets[0], sizeBytes=STATIC_LIMIT + 1)]),
                        fixture_zip([dict(fixture_assets[0], sizeBytes=True)]),
                        fixture_zip([dict(fixture_assets[0], path="assets/CON.js")])]
    for source in invalid_packages:
        try:
            read_web_package(source)
        except RuntimeError:
            continue
        raise RuntimeError("pure package accepted corrupt/version/missing/duplicate/traversal/size/reserved-name input")
    checks.append("ZIP manifest and all payload hashes verified; corrupt/version/path/count/size inputs rejected")
    wav = silent_wav()
    require(len(wav) == 16044 and wav[:4] == b"RIFF" and wav[8:16] == b"WAVEfmt "
            and struct.unpack("<IHHIIHH", wav[16:36]) == (16, 1, 1, 16000, 32000, 2, 16)
            and wav[36:40] == b"data" and struct.unpack("<I", wav[40:44])[0] == 16000 and not any(wav[44:]),
            "pure silent WAV shape check failed")
    frames = (len(wav) - 44) // 2
    require(8000 <= frames <= 960000 and len(wav) == 44 + frames * 2 and frames * 1000 // 16000 == 500,
            "pure fixture failed VoicePcmMetadata domain duration/length bounds")
    checks.append("8000-sample/500ms silent PCM fixture meets domain minimum/byte-length bounds")
    body = bytes(MATERIAL_BYTES)
    head = b"POST /fc/api/v1/materials/uploads/1/bytes HTTP/1.1\r\nHost: localhost:12345\r\n\r\n"
    fragments, first = request_fragments(head, body)
    require(b"".join(fragments) == head + body and first == 4096 and max(map(len, fragments)) <= FRAGMENT,
            "pure fragment/coalescing reconstruction failed")
    checks.append("120000-byte request fragmentation and head/body coalescing")
    ready = {"port": 12345, "generation": 1, "marker": "synthetic", "origin": "https://localhost:12345",
             "basePath": "/fc/", "classId": "1", "subjectId": "1", "loginName": "probe", "pairingCode": "12345678",
             "staticAssets": 6, "visualFixture": "FILLED_DEFAULT"}
    validate_ready(ready, "synthetic")
    final = {"physicalNativeDrain": True, "sharedWorkers": 2, "workerJoin": True, "hubPending": 0,
             "downloadHubPending": 0,
             "quarantined": 0, "setupCleanup": True, "approvals": 1, "heads": 12, "responses": 12, "peakRecords": 2}
    validate_native(final, [{"httpStatus": 200}] * 12)
    validate_native(final, [{"httpStatus": 200}] * 3, browser_wire_responses=9)
    for invalid_count in (True, -1, "9", 13):
        try:
            validate_native(final, [], browser_wire_responses=invalid_count)
        except RuntimeError:
            continue
        raise RuntimeError("native ledger accepted invalid/uncovered browser wire count")
    for key in ("physicalNativeDrain", "workerJoin", "setupCleanup"):
        try:
            validate_native(dict(final, **{key: False}), [{"httpStatus": 200}] * 12)
        except RuntimeError:
            continue
        raise RuntimeError("pure final ledger accepted missing actual cleanup/join")
    checks.append("READY identity and actual-drain final ledger required; independent browser wire-count floor, excluding cache hits")
    key = {"kind": "HOMEWORK", "classId": "1", "subjectId": "1", "ownerTeacherId": "1", "date": "2026-11-01"}
    for fixture in VISUAL_FIXTURES:
        zero = fixture == "FILLED_ZERO"
        visual = {"schemaVersion": 1, "status": "SUBJECT_VISUAL", "key": key,
                  "subjectId": "1", "subjectName": "Synthetic TLS subject", "subjectRevision": "1",
                  "displayRevision": "0" if fixture == "FILLED_DEFAULT" else "1", "rulesVersion": 1,
                  "effectiveRgb": "#88A6C4", "mode": "OUTLINED" if fixture == "OUTLINED" else "FILLED",
                  "homeworkFillPercent": 0 if zero else 3, "courseFillPercent": 0 if zero else 20,
                  "homeworkComponentOpacityPercent": 70 if zero else 100,
                  "courseComponentOpacityPercent": 80 if zero else 100,
                  "homeworkVisible": True, "courseVisible": True}
        validate_visual(visual, key, fixture)
        for invalid in (dict(visual, palette=[]), dict(visual, effectiveRgb="#020304"),
                        dict(visual, homeworkVisible=1), dict(visual, subjectRevision="2"),
                        dict(visual, homeworkComponentOpacityPercent=0), dict(visual, key=dict(key, classId="2"))):
            try:
                validate_visual(invalid, key, fixture)
            except RuntimeError:
                continue
            raise RuntimeError("visual disclosure validator accepted extra palette/raw RGB/wrong version/scope/alpha/type")
        for wrong in VISUAL_FIXTURES:
            if wrong == fixture:
                continue
            try:
                validate_ready(dict(ready, visualFixture=wrong), "synthetic", fixture)
            except RuntimeError:
                continue
            raise RuntimeError("READY silently ignored requested visual fixture")
    checks.append("visual exact single-scope disclosure: default3/20, independent0fill+70/80component, outline retains stored3/20; extra/raw/version/scope/type and wrong fixture rejected")
    print(json.dumps({"status": "PURE_LOCAL_CHECKS_PASS", "checks": checks,
                      "tls": "NOT_RUN", "windows": "NOT_RUN", "business": "NOT_RUN"}, indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ssh-target", default="future-classroom-win11")
    parser.add_argument("--identity", type=Path)
    parser.add_argument("--exe", type=Path)
    parser.add_argument("--report", type=Path)
    parser.add_argument("--web-bundle", type=Path, help="Existing development ZIP; only verified web manifest payloads are uploaded")
    parser.add_argument("--browser", action="store_true", help="Also verify isolated Windows Edge UI against this same finite native service")
    parser.add_argument("--continuous-accept", action="store_true", help="Also verify real idle AcceptEx repost and four admitted JSON bodies within the original client/native deadlines; not strict TCP5s evidence")
    parser.add_argument("--visual-fixture", choices=VISUAL_FIXTURES, default="FILLED_DEFAULT", help="Exact finite isolated synthetic display fixture; no production settings")
    parser.add_argument("--visual-only", action="store_true", help="Target only visual/scope/read-only contract within the original client/native budgets")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        if args.exe or args.report or args.identity or args.web_bundle or args.browser or args.continuous_accept or args.visual_only or args.visual_fixture != "FILLED_DEFAULT":
            parser.error("self-test is pure local; do not combine with remote inputs")
        self_test()
        return
    if args.visual_only and args.continuous_accept:
        parser.error("visual-only is separate from full material/capacity acceptance")
    if not args.exe or not args.report or not args.web_bundle:
        parser.error("--exe, --report and --web-bundle required for actual diagnostic")
    if not re.fullmatch(r"[A-Za-z0-9_.@:-]+", args.ssh_target) or args.ssh_target.startswith("-"):
        parser.error("ssh-target must be an SSH alias/user@host without whitespace/options")
    if not args.exe.is_file() or args.exe.suffix.lower() != ".exe" or (args.identity and not args.identity.is_file()):
        parser.error("exe and optional private identity must exist")
    if args.exe.stat().st_size > 128 * 1024 * 1024:
        parser.error("supplied executable exceeded 128MiB bound")
    executable = args.exe.read_bytes()
    pe = int.from_bytes(executable[60:64], "little") if len(executable) >= 64 else 0
    if executable[:2] != b"MZ" or executable[pe:pe + 6] != b"PE\0\0\x4c\x01":
        parser.error("supplied executable must be a real Windows x86 PE")
    if args.report.exists():
        parser.error("report already exists; preserve original first/final evidence separately")
    if not args.web_bundle.is_file() or args.web_bundle.stat().st_size > 128 * 1024 * 1024:
        parser.error("web-bundle must be an existing ZIP no larger than 128MiB")
    package = read_web_package(args.web_bundle)
    token = uuid.uuid4().hex
    # The shared native capability accepts exactly this prefix plus 32 hex
    # digits. Business scope belongs in the report, not the capability grammar.
    marker = "FutureClassroom-Test-TLS-" + token
    report = {"schemaVersion": 1, "status": "FAIL", "scope": "Finite raw HTTPS API diagnostic through owned SSH loopback forward",
              "strictP0": "NOT_VALIDATED", "productionListener": "NOT_ENABLED", "browserAcceptance": "NOT_TESTED",
              "win7": "NOT_VALIDATED", "win10": "NOT_VALIDATED", "physicalPlayback": "NOT_EXERCISED",
              "forwardTimingIsNativeDeadlineEvidence": False, "marker": marker, "exeSha256": digest(executable),
              "runnerSourceSha256": digest(Path(__file__).read_bytes()), "ownershipHelperSourceSha256": digest(_source.read_bytes()),
              "webBundleSha256": digest(args.web_bundle.read_bytes()),
              "remoteScriptsSha256": digest((COMMON + SETUP + LAUNCH + STOP + CAPTURE + CLEANUP).encode("utf-8")),
              "credentialScope": "Disposable synthetic probe account only; cookie/CSRF/poll secret omitted from client events; READY original contains synthetic pairing code",
              "visualFixture": args.visual_fixture, "visualOnly": args.visual_only,
              "events": [], "cleanup": "NOT_RUN"}
    remote = Remote(args.ssh_target, args.identity)
    directory = launcher = launcher_logs = tunnel = local_owner = browser_observer = None
    uploaded = set()
    phase = "create-owned-root"
    try:
        directory = remote.powershell("$root=Join-Path $env:TEMP " + ps_literal(marker)
            + ";New-Item -ItemType Directory -Path $root|Out-Null;[IO.File]::WriteAllText((Join-Path $root '.tls-probe-root'),"
            + ps_literal(marker) + ");$root")
        report["remoteRoot"] = directory
        local_owner = tempfile.TemporaryDirectory(prefix="fc-tls-business-probe-")
        phase = "upload-verified-public-web-package"
        report["staticPackage"] = upload_web_package(remote, local_owner.name, directory, marker, package)
        phase = "upload-owned-control-scripts"
        report["uploadedScriptSha256"] = {}
        for name, template in (("cleanup", CLEANUP), ("stop", STOP), ("capture", CAPTURE), ("setup", SETUP)):
            source = Path(local_owner.name) / ("tls-" + name + ".ps1")
            source.write_text(COMMON + script(template, directory, marker, args.exe.name), encoding="utf-8-sig")
            report["uploadedScriptSha256"][name] = digest(source.read_bytes())
            remote.upload(source, directory)
            uploaded.add(name)
        parsed = remote.powershell("$files=Get-ChildItem -LiteralPath " + ps_literal(directory)
            + " -Filter 'tls-*.ps1';foreach($file in $files){$tokens=$null;$errors=$null;"
            + "[Management.Automation.Language.Parser]::ParseFile($file.FullName,[ref]$tokens,[ref]$errors)|Out-Null;"
            + "if($errors.Count-ne 0){throw (($errors|ForEach-Object{$_.Message})-join '; ')}};'TLS_CONTROL_SCRIPTS_PARSE_PASS'", timeout=15)
        require(parsed == "TLS_CONTROL_SCRIPTS_PARSE_PASS", "control script parse result missing")
        report["controlScriptsParse"] = "PASS"
        phase = "upload-owned-bounded-visual-fixture"
        applied = remote.powershell("$root=" + ps_literal(directory)
            + ";if(([IO.File]::ReadAllText((Join-Path $root '.tls-probe-root')))-ne " + ps_literal(marker)
            + "){throw 'visual fixture root mismatch'};[IO.File]::WriteAllText((Join-Path $root '.subject-visual-fixture'),"
            + ps_literal(args.visual_fixture) + ",[Text.ASCIIEncoding]::new());"
            + "[IO.File]::ReadAllText((Join-Path $root '.subject-visual-fixture'))")
        require(applied == args.visual_fixture, "owned visual fixture upload/readback mismatch")
        report["visualFixtureInput"] = {"bytes": len(applied), "sha256": digest(applied.encode("ascii")), "readBack": "PASS"}
        phase = "create-owned-certificate-and-cng-key"
        fixture = json.loads(remote.run_file(directory + "\\tls-setup.ps1", timeout=45))
        report["fixture"] = {key: value for key, value in fixture.items() if key != "publicPem"}
        source = Path(local_owner.name) / "tls-launch.ps1"
        source.write_text(COMMON + script(LAUNCH, directory, marker, args.exe.name, fixture["thumbprint"]), encoding="utf-8-sig")
        report["uploadedScriptSha256"]["launch"] = digest(source.read_bytes())
        remote.upload(source, directory)
        phase = "upload-and-verify-owned-executable"
        remote.upload(args.exe, directory)
        uploaded_digest = remote.powershell("(Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path "
            + ps_literal(directory) + " " + ps_literal(args.exe.name) + ")).Hash.ToLowerInvariant()")
        require(uploaded_digest == report["exeSha256"] == digest(args.exe.read_bytes()), "executable changed after identity freeze")
        pem = Path(local_owner.name) / "tls-public.pem"
        pem.write_text(fixture["publicPem"], encoding="ascii")
        expected_der = ssl.PEM_cert_to_DER_cert(fixture["publicPem"])
        tls_context, wrong_ca = _tls.context(pem), _tls.untrusted_context(expected_der)
        report["clientVerification"] = {"checkHostname": True, "verifyMode": "CERT_REQUIRED", "tls": "TLSv1.2",
            "publicCertificateSha256": digest(expected_der), "trustSource": "only owned public PEM; no system trust mutation",
            "closeNotify": "required actual clean TLS EOF with suppress_ragged_eofs=False", "openssl": ssl.OPENSSL_VERSION}
        if args.browser:
            phase = "prepare-isolated-windows-edge-before-native-deadline"
            observer_source = Path(__file__).with_name("windows-native-browser-observer.py")
            observer_spec = importlib.util.spec_from_file_location("fc_native_browser_observer", observer_source)
            observer_module = importlib.util.module_from_spec(observer_spec)
            observer_spec.loader.exec_module(observer_module)
            browser_observer = observer_module.BrowserObserver(
                remote, local_owner.name, args.report, marker, json.loads(package[0]), CSP,
                args.identity, args.ssh_target)
            browser_observer.prepare(fixture["publicPem"])
            report["scope"] = "Finite raw HTTPS API and isolated Windows Edge UI diagnostic against one native service"
        phase = "start-owned-ssh-master"
        tunnel = Tunnel(remote, local_owner.name)
        phase = "start-native-business-diagnostic"
        launcher = subprocess.Popen(remote.command_file(directory + "\\tls-launch.ps1"), stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        launcher_logs = Logs(launcher)
        until = time.monotonic() + 100
        ready, native, exits = [], [], []
        progress_reports = []
        progress_reader = ProgressReader(launcher_logs.lines, marker, progress_reports)
        report["continuousAcceptEnabled"] = args.continuous_accept
        report["nativeProgress"] = progress_reports
        report.update({"ready": ready, "nativeReports": native, "nativeProcessExits": exits})
        while time.monotonic() < until:
            try:
                line = progress_reader.get(timeout=0.1)
            except queue.Empty:
                if launcher.poll() is not None and not any(thread.is_alive() for thread in launcher_logs.threads):
                    break
                continue
            if line.startswith(READY):
                require(not ready, "duplicate business READY domain; no implicit new generation")
                item = json.loads(line[len(READY):])
                validate_ready(item, marker, args.visual_fixture)
                require(item["staticAssets"] == len(package[2]["assets"]), "native/package static asset count mismatch")
                progress_reader.generation = item["generation"]
                ready.append({key: value for key, value in item.items() if key != "pairingCode"})
                phase = "actual-tls-business-clients"
                port = tunnel.open(item["port"])
                client = Client(port, item, tls_context, wrong_ca, expected_der, report["events"])
                if args.continuous_accept:
                    report["continuousIdle"] = exercise_idle_accept(client, progress_reader)
                report["static"] = {"status": "NOT_RUN_TARGETED_VISUAL_MATRIX"} if args.visual_only else exercise_static(client, package[2])
                report["business"] = exercise(client, browser_observer, progress_reader if args.continuous_accept else None, visual_only=args.visual_only)
                tunnel.cancel()
            elif line.startswith(PASS):
                native.append(json.loads(line[len(PASS):]))
            elif any(line.startswith(prefix) for prefix in EXIT_PREFIXES):
                prefix = next(prefix for prefix in EXIT_PREFIXES if line.startswith(prefix))
                exits.append(json.loads(line[len(prefix):]))
            elif line.startswith("TLS_BUSINESS_REPORT "):
                report.setdefault("nativeDebugLines", []).append(line)
            else:
                report.setdefault("otherNativeLines", []).append(line)
        phase = "actual-native-and-ssh-exit"
        launcher.wait(timeout=max(0.01, until - time.monotonic()))
        launcher_logs.join()
        require(launcher.returncode == 0 and len(exits) == 1 and exits[0].get("code") == 0, "native child/supervisor actual successful exit missing")
        require(len(ready) == 1 and len(native) == 1 and report.get("business") is not None
                and all(event["status"] == "PASS" for event in report["events"]), "business client/native summary incomplete")
        for case in ("wrong-ca-rejected", "wrong-hostname-rejected"):
            matching = [event for event in report["events"] if event["case"] == case]
            require(len(matching) == 1 and matching[0].get("rejectionType") == "SSLCertVerificationError", "actual TLS negative rejection evidence missing")
        browser_count = browser_observer.result["wireResponseCount"] if browser_observer else 0
        validate_native(native[0], report["events"], browser_count)
        if browser_observer:
            report["browserAcceptance"] = "FINITE_NATIVE_CONTENT_UI_PASS"
        report.update({"native": native[0], "nativeProcessExit": exits[0], "status": "PASS"})
    except Exception as error:
        report["error"] = failure(error, phase)
    finally:
        if launcher:
            try:
                if launcher.poll() is None:
                    launcher.terminate()
                    try:
                        launcher.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        launcher.kill()
                        launcher.wait(timeout=5)
            except Exception as error:
                # Continue the independent exact remote STOP/capture/cleanup
                # path even if the local SSH launcher could not be reaped here.
                report["localLauncherCleanupError"] = failure(error, "stop-owned-local-ssh-launcher")
                report["status"] = "FAIL"
            if launcher_logs:
                try:
                    launcher_logs.join()
                except Exception as error:
                    report["launcherDrainError"] = failure(error, "drain-owned-ssh-pipes")
                    report["status"] = "FAIL"
                report["sshLauncherRaw"] = launcher_logs.report()
        if tunnel:
            try:
                tunnel.close()
                report["ownedTunnelCleanup"] = "PASS"
            except Exception as error:
                report["tunnelCleanupError"] = failure(error, "stop-owned-ssh-tunnel")
                report["status"] = "FAIL"
            report["sshTunnelRaw"] = tunnel.logs.report()
        if browser_observer:
            try:
                browser_observer.close()
            except Exception as error:
                report["browserCleanupError"] = failure(error, "close-original-browser-context-forward-job")
                report["status"] = "FAIL"
            report["browser"] = browser_observer.report
        if local_owner:
            try:
                local_owner.cleanup()
            except Exception as error:
                report["localCleanupError"] = failure(error, "cleanup-owned-local-public-cert/control-socket")
                report["status"] = "FAIL"
        if directory:
            cleanup_owned(remote, directory, marker, uploaded, args.report, token, report)
        report["clientEventsSha256"] = digest(json.dumps(report["events"], sort_keys=True, separators=(",", ":")).encode("utf-8"))
        report["remoteControlRunsRaw"] = remote.script_runs
        args.report.parent.mkdir(parents=True, exist_ok=True)
        with args.report.open("x", encoding="utf-8") as output:
            output.write(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    if report["status"] != "PASS" or report["cleanup"] != "PASS":
        raise SystemExit("TLS business diagnostic failed; inspect " + str(args.report))
    print("TLS_BUSINESS_RUNNER_PASS " + json.dumps({"report": str(args.report), "exeSha256": report["exeSha256"],
          "cleanup": report["cleanup"], "strictP0": "NOT_VALIDATED", "physicalPlayback": "NOT_EXERCISED"}, separators=(",", ":")))


if __name__ == "__main__":
    main()
