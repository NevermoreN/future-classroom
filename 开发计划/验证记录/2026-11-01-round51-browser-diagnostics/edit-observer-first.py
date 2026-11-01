from pathlib import Path
p=Path('/tmp/fc-round51-browser-diagnostics-worktree/scripts/windows-native-browser-observer.py')
s=p.read_text().replace('"fromServiceWorker", "fromPrefetchCache", "wireStatus", "finished", "bodyVerified",','"fromServiceWorker", "fromPrefetchCache", "wireStatus", "finished", "bodyVerified", "failure",')
needle='\n\ndef require(condition, code):'
new='''
FAILURE_KEYS = frozenset((
    "requestOrdinal", "kind", "path", "phase", "canceled", "errorText", "blockedReason",
    "failureMillis", "requestMillis", "responseMillis", "extraInfoMillis", "finishedMillis", "bodyVerifiedMillis",
    "responseReceived", "finished", "bodyVerified", "dataEvents", "dataBytes", "encodedBytes", "countsClamped",
))
FAILURE_PHASES = frozenset((
    "connect", "await-run", "inject-existing-approved-cookie", "navigate-native", "read-original-content",
    "visual-original", "enter-synthetic-text", "visual-refresh-with-draft", "save-once", "visual-after-save",
    "reload-and-read-durable-content", "visual-after-reload", "settle-observed-responses", "complete", "OTHER",
))
BLOCKED_REASONS = frozenset((
    "NONE", "OTHER", "other", "csp", "mixed-content", "origin", "inspector", "subresource-filter", "content-type",
    "coep-frame-resource-needs-coep-header", "coop-sandboxed-iframe-cannot-navigate-to-coop-page",
    "corp-not-same-origin", "corp-not-same-origin-after-defaulted-to-same-origin-by-coep", "corp-not-same-site",
))


def require(condition, code):'''
assert needle in s;s=s.replace(needle,new)
needle='\n\nAPI_OPERATIONS ='
new='''

def validate_network_failures(value, run, manifest, content_kind):
    """Validate only finite diagnostic facts, never relax the PASS contract.

    Milliseconds describe local Node CDP event receipt relative to RUN receipt;
    null means unavailable/outside the 60s observation bound. Counters are
    capped with an explicit countsClamped flag. No socket/native clock claim.
    """
    records = value.get("network", [])
    require(isinstance(records, list) and len(records) <= 128, "NODE_FAILURE_NETWORK_BOUND")
    failures = []
    for index, record in enumerate(records):
        require(isinstance(record, dict), "NODE_FAILURE_RECORD_SHAPE")
        if "failure" not in record:
            continue
        require(set(record) <= RECORD_KEYS and value.get("status") == "FAIL"
                and isinstance(value.get("error"), dict) and value["error"].get("code") == "NETWORK_LOAD_FAILED",
                "NODE_FAILURE_OUTCOME")
        observation = record["failure"]
        require(isinstance(observation, dict) and set(observation) == FAILURE_KEYS, "NODE_FAILURE_SHAPE")
        path = observation["path"]
        require(isinstance(path, str) and len(path) <= 512 and record.get("url") == run["origin"] + path
                and record.get("method") in ("GET", "POST"), "NODE_FAILURE_PATH")
        prefix = "homework" if content_kind == "HOMEWORK" else "reminders"
        fixed_api = ("/fc/api/v1/status", "/fc/api/v1/session", f"/fc/api/v1/content/{prefix}/scopes/0")
        ids = r"[1-9][0-9]{0,19}/" * (3 if content_kind == "HOMEWORK" else 1)
        content = f"/fc/api/v1/content/{prefix}/" + ids + r"[0-9]{4}-[0-9]{2}-[0-9]{2}"
        assets = {"/fc/" + asset["path"] for asset in manifest["assets"]} | {"/fc/"}
        kind = ("content" if re.fullmatch(content, path) else
                "visual" if content_kind == "HOMEWORK" and re.fullmatch(content + r"/visual", path) else
                "api" if path in fixed_api else "asset" if path in assets else None)
        require(kind is not None and observation["kind"] == kind
                and (kind == "content" or record["method"] == "GET"), "NODE_FAILURE_UNEXPECTED_TARGET")
        require(type(observation["requestOrdinal"]) is int and observation["requestOrdinal"] == index + 1
                and observation["phase"] in FAILURE_PHASES
                and type(observation["canceled"]) is bool
                and isinstance(observation["errorText"], str)
                and (observation["errorText"] == "OTHER" or re.fullmatch(r"net::ERR_[A-Z0-9_]{1,80}", observation["errorText"]))
                and observation["blockedReason"] in BLOCKED_REASONS,
                "NODE_FAILURE_CLASSIFICATION")
        for key in ("failureMillis", "requestMillis", "responseMillis", "extraInfoMillis", "finishedMillis", "bodyVerifiedMillis"):
            number = observation[key]
            require(number is None or type(number) is int and 0 <= number <= 60000, "NODE_FAILURE_TIME_BOUND")
            require(number is None or observation["failureMillis"] is None or number <= observation["failureMillis"],
                    "NODE_FAILURE_TIME_ORDER")
        for key in ("responseReceived", "finished", "bodyVerified", "countsClamped"):
            require(type(observation[key]) is bool, "NODE_FAILURE_FLAG")
        require(observation["finished"] is record.get("finished") and observation["bodyVerified"] is record.get("bodyVerified")
                and observation["responseReceived"] == (record.get("status") is not None), "NODE_FAILURE_LIFECYCLE")
        for key, bound in (("dataEvents", 65536), ("dataBytes", 8 * 1024 * 1024), ("encodedBytes", 8 * 1024 * 1024)):
            require(type(observation[key]) is int and 0 <= observation[key] <= bound, "NODE_FAILURE_COUNTER")
        failures.append(dict(observation))
    require(len(failures) <= 1, "NODE_MULTIPLE_FIRST_FAILURES")
    # Summary's wire subset may repeat that same record, never add a hidden
    # second diagnostic or change its snapshot. It remains partial evidence.
    for record in value.get("wireResponses", []):
        if isinstance(record, dict) and "failure" in record:
            require(record in records, "NODE_FAILURE_WIRE_SUBSET")
    return failures


API_OPERATIONS ='''
assert needle in s;s=s.replace(needle,new)
s=s.replace('''    def _validate_result(self, value, message, run):
        expected_operations''','''    def _validate_result(self, value, message, run):
        validate_network_failures(value, run, self.manifest, self.content_kind)
        expected_operations''')
needle='''            self.report["nodeReport"] = identity
            require(self.node.returncode == 0, "NODE_ACTUAL_EXIT_FAILED")'''
new='''            self.report["nodeReport"] = identity
            diagnostics = validate_network_failures(value, run, self.manifest, self.content_kind)
            if diagnostics:
                require(value.get("marker") == self.marker and value.get("generation") == run["generation"]
                        and value.get("origin") == run["origin"] and value.get("maxMillis") == run["maxMillis"]
                        and value.get("sourceSha256") == self._node_source_sha and value.get("contentKind") == self.content_kind,
                        "NODE_FAILURE_REPORT_IDENTITY")
                self.report["networkFailureDiagnostics"] = diagnostics
            require(self.node.returncode == 0, "NODE_ACTUAL_EXIT_FAILED")'''
assert needle in s;s=s.replace(needle,new)
p.write_text(s)
