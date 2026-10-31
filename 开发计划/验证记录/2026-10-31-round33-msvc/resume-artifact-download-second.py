#!/usr/bin/env python3
"""One explicitly gated resume attempt; prepare only until the parent authorizes.

The original failed report and ZIP are read-only. A signed HTTPS URL is accepted
only from stdin, never argv/environment/report. No retries or redirects. This
Linux-local coordinator uses one process alarm, not a new worker/thread/pool.
"""
from __future__ import annotations

import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import select
import signal
import stat
import sys
import time
import urllib.parse
import urllib.request

BASE = Path('/tmp/fc-round33-msvc-evidence')
EXPECTED_META = BASE / 'artifact-expected.json'
ORIGINAL_REPORT = BASE / 'artifact-download-first.json'
ORIGINAL = Path('/tmp/fc-round33-msvc-original.zip')
DESTINATION = Path('/tmp/fc-round33-msvc-resumed-second.zip')
REPORT = BASE / 'artifact-download-resume-second.json'
SOURCE = '9d42f7cc75c7d2545caa18366d4a6d047aedaa99'
RUN_ID = 41215512635
ARTIFACT_ID = 13048571502
EXPECTED_BYTES = 39_626_813
EXPECTED_SHA = '5dea0c53e6827d8fa5f1af2b8626259a5c6dd04ec81bb11b6393c609536b4b8c'
OVERALL_SECONDS = 600
SOCKET_SECONDS = 25
CHUNK = 256 * 1024
MAX_URL_BYTES = 16 * 1024


class PreconditionsFailed(Exception):
    pass


class OverallBudgetExpired(Exception):
    pass


class RangeResponseRejected(Exception):
    pass


class ArtifactIntegrityRejected(Exception):
    pass


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        # A redirect is a second HTTP request, not the one authorized Range.
        return None


def require(condition: bool) -> None:
    if not condition:
        raise PreconditionsFailed()


def integer(value: object) -> bool:
    return isinstance(value, int) and not isinstance(value, bool)


def snapshot(info: os.stat_result) -> tuple[int, int, int, int, int]:
    return (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns, info.st_ctime_ns)


def open_original(path: Path, expected_size: int | None = None):
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        info = os.fstat(descriptor)
        require(stat.S_ISREG(info.st_mode))
        if expected_size is not None:
            require(info.st_size == expected_size)
        return os.fdopen(descriptor, 'rb', buffering=0), snapshot(info)
    except BaseException:
        os.close(descriptor)
        raise


def read_json(path: Path) -> tuple[dict, str]:
    handle, before = open_original(path)
    with handle:
        require(0 < before[2] <= 64 * 1024)
        data = handle.read(64 * 1024 + 1)
        require(len(data) == before[2] and snapshot(os.fstat(handle.fileno())) == before)
    value = json.loads(data)
    require(isinstance(value, dict))
    return value, hashlib.sha256(data).hexdigest()


def checkpoint(deadline: float) -> float:
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        raise OverallBudgetExpired()
    return remaining


def alarm_expired(signum, frame) -> None:
    raise OverallBudgetExpired()


def read_url(deadline: float) -> str:
    pending = bytearray()
    descriptor = sys.stdin.fileno()
    while True:
        ready, _, _ = select.select([descriptor], [], [], checkpoint(deadline))
        if not ready:
            raise OverallBudgetExpired()
        block = os.read(descriptor, min(4096, MAX_URL_BYTES + 1 - len(pending)))
        if not block:
            break
        pending.extend(block)
        require(len(pending) <= MAX_URL_BYTES)
        if b'\n' in pending:
            line, trailing = bytes(pending).split(b'\n', 1)
            require(not trailing.strip())
            pending = bytearray(line)
            break
    url = bytes(pending).decode('ascii').strip()
    require(bool(url) and '\r' not in url and '\n' not in url)
    parsed = urllib.parse.urlsplit(url)
    require(parsed.scheme == 'https' and bool(parsed.hostname))
    require(parsed.username is None and parsed.password is None and not parsed.fragment)
    require(parsed.port in (None, 443))
    return url


def hash_original(handle, expected_size: int, deadline: float) -> str:
    handle.seek(0)
    digest = hashlib.sha256()
    count = 0
    while True:
        checkpoint(deadline)
        block = handle.read(min(CHUNK, expected_size - count + 1))
        if not block:
            break
        count += len(block)
        require(count <= expected_size)
        digest.update(block)
    require(count == expected_size)
    return digest.hexdigest()


def one_header(response, name: str) -> str:
    values = response.headers.get_all(name, [])
    if len(values) != 1:
        raise RangeResponseRejected()
    return values[0].strip()


def main() -> int:
    started = time.monotonic()
    deadline = started + OVERALL_SECONDS
    result = {
        'scope': 'One gated HTTP Range resume of the original failed modern MSVC source9d42 artifact; no CI or executable execution. Original FAIL/report/partial remain unchanged. Signed URL excluded.',
        'status': 'FAIL', 'runId': RUN_ID, 'sourceSha': SOURCE, 'artifactId': ARTIFACT_ID,
        'path': str(DESTINATION), 'bytes': 0, 'sha256': hashlib.sha256(b'').hexdigest(),
        'expectedBytes': EXPECTED_BYTES, 'expectedSha256': EXPECTED_SHA,
        'originalReport': str(ORIGINAL_REPORT), 'originalPath': str(ORIGINAL),
        'overallBudgetSeconds': OVERALL_SECONDS, 'socketTimeoutSeconds': SOCKET_SECONDS,
        'chunkBytes': CHUNK, 'rangeRequestsAttempted': 0, 'response206Observed': False,
        'resumedFileCreated': False, 'originalPreservedVerified': False,
        'startedUtc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
    }
    report_handle = None
    destination = None
    original = None
    alarm_installed = False
    old_alarm_handler = None
    count = 0
    digest = hashlib.sha256()
    phase = 'fresh-report-and-budget'
    try:
        # Exclusive creation before any mutation. A repeated invocation must
        # preserve both the old resumed file and its first-attempt report.
        report_handle = REPORT.open('x', encoding='utf-8')
        require(os.name == 'posix' and hasattr(signal, 'setitimer') and hasattr(os, 'O_NOFOLLOW'))
        require(signal.getitimer(signal.ITIMER_REAL) == (0.0, 0.0))
        old_alarm_handler = signal.signal(signal.SIGALRM, alarm_expired)
        signal.setitimer(signal.ITIMER_REAL, checkpoint(deadline))
        alarm_installed = True

        phase = 'expected-metadata'
        expected, expected_meta_sha = read_json(EXPECTED_META)
        require(integer(expected.get('runId')) and expected['runId'] == RUN_ID)
        require(integer(expected.get('artifactId')) and expected['artifactId'] == ARTIFACT_ID)
        require(expected.get('sourceSha') == SOURCE)
        require(integer(expected.get('bytes')) and expected['bytes'] == EXPECTED_BYTES)
        require(expected.get('sha256') == EXPECTED_SHA)
        result['expectedMetadataSha256'] = expected_meta_sha

        phase = 'original-terminal-failure'
        failed, original_report_sha = read_json(ORIGINAL_REPORT)
        require(failed.get('status') == 'FAIL')
        require(integer(failed.get('runId')) and failed['runId'] == RUN_ID)
        require(integer(failed.get('artifactId')) and failed['artifactId'] == ARTIFACT_ID)
        require(failed.get('sourceSha') == SOURCE and failed.get('path') == str(ORIGINAL))
        require(failed.get('expectedBytes') == EXPECTED_BYTES and failed.get('expectedSha256') == EXPECTED_SHA)
        offset = failed.get('bytes')
        prefix_sha = failed.get('sha256')
        require(integer(offset) and 0 < offset < EXPECTED_BYTES)
        require(isinstance(prefix_sha, str) and re.fullmatch('[0-9a-f]{64}', prefix_sha) is not None)
        result.update(originalReportSha256=original_report_sha, originalBytes=offset, originalSha256=prefix_sha, rangeOffset=offset)

        phase = 'verify-original-partial'
        original, original_stat = open_original(ORIGINAL, offset)
        require(hash_original(original, offset, deadline) == prefix_sha)
        require(snapshot(os.fstat(original.fileno())) == original_stat)
        require(snapshot(os.stat(ORIGINAL, follow_symlinks=False)) == original_stat)

        phase = 'copy-original-to-exclusive-resume-file'
        descriptor = os.open(DESTINATION, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        destination = os.fdopen(descriptor, 'wb', buffering=0)
        result['resumedFileCreated'] = True

        def write_chunk(block: bytes) -> None:
            nonlocal count
            view = memoryview(block)
            while view:
                checkpoint(deadline)
                written = destination.write(view)
                if written is None or written <= 0:
                    raise ArtifactIntegrityRejected()
                digest.update(view[:written])
                count += written
                view = view[written:]

        original.seek(0)
        while count < offset:
            checkpoint(deadline)
            block = original.read(min(CHUNK, offset - count))
            require(bool(block))
            write_chunk(block)
        require(digest.hexdigest() == prefix_sha)
        require(not original.read(1))
        require(snapshot(os.fstat(original.fileno())) == original_stat)

        phase = 'private-stdin-url'
        url = read_url(deadline)
        request = urllib.request.Request(url, headers={
            'Range': f'bytes={offset}-', 'Accept-Encoding': 'identity',
        })
        opener = urllib.request.build_opener(NoRedirect())
        phase = 'single-range-request'
        result['rangeRequestsAttempted'] = 1
        with opener.open(request, timeout=min(SOCKET_SECONDS, checkpoint(deadline))) as response:
            if response.status != 206:
                raise RangeResponseRejected()
            result['response206Observed'] = True
            if one_header(response, 'Content-Range') != f'bytes {offset}-{EXPECTED_BYTES - 1}/{EXPECTED_BYTES}':
                raise RangeResponseRejected()
            length = one_header(response, 'Content-Length')
            if re.fullmatch('[0-9]+', length) is None or int(length) != EXPECTED_BYTES - offset:
                raise RangeResponseRejected()
            if response.headers.get_all('Transfer-Encoding', []):
                raise RangeResponseRejected()
            encoding = response.headers.get_all('Content-Encoding', [])
            if len(encoding) > 1 or (encoding and encoding[0].strip().lower() != 'identity'):
                raise RangeResponseRejected()
            phase = 'receive-original-range-once'
            next_progress = count + 1024 * 1024
            while count < EXPECTED_BYTES:
                checkpoint(deadline)
                block = response.read(min(CHUNK, EXPECTED_BYTES - count))
                if not block:
                    raise ArtifactIntegrityRejected()
                write_chunk(block)
                if count >= next_progress:
                    print(json.dumps({'status': 'RUNNING', 'bytes': count, 'expectedBytes': EXPECTED_BYTES, 'elapsedSeconds': time.monotonic() - started}), flush=True)
                    next_progress = count + 1024 * 1024
            checkpoint(deadline)
            if response.read(1):
                raise ArtifactIntegrityRejected()
        url = None
        request = None
        phase = 'verify-full-artifact-and-original-preservation'
        checkpoint(deadline)
        if count != EXPECTED_BYTES or digest.hexdigest() != EXPECTED_SHA:
            raise ArtifactIntegrityRejected()
        if os.fstat(destination.fileno()).st_size != EXPECTED_BYTES:
            raise ArtifactIntegrityRejected()
        require(hash_original(original, offset, deadline) == prefix_sha)
        require(snapshot(os.fstat(original.fileno())) == original_stat)
        require(snapshot(os.stat(ORIGINAL, follow_symlinks=False)) == original_stat)
        _, report_sha_after = read_json(ORIGINAL_REPORT)
        _, meta_sha_after = read_json(EXPECTED_META)
        require(report_sha_after == original_report_sha and meta_sha_after == expected_meta_sha)
        result['originalPreservedVerified'] = True
        checkpoint(deadline)
        result['status'] = 'PASS'
    except Exception as error:
        # Never stringify HTTP/network exceptions: they can contain signed URLs.
        result.update(status='FAIL', errorKind=type(error).__name__, errorMessage='Artifact resume failed; original and resumed evidence are retained.', failedPhase=phase)
    finally:
        # An expired budget stops positive work. Only close existing handles and
        # record this same attempt; no retry, delete, truncate or replacement.
        if alarm_installed:
            signal.setitimer(signal.ITIMER_REAL, 0)
            signal.signal(signal.SIGALRM, old_alarm_handler)
        for handle in (destination, original):
            if handle is not None:
                try:
                    handle.close()
                except Exception as error:
                    result.update(status='FAIL', errorKind=type(error).__name__, errorMessage='Artifact resume handle cleanup failed; evidence retained.', failedPhase='close-owned-handles')
        result.update(bytes=count, sha256=digest.hexdigest(), elapsedSeconds=time.monotonic() - started)
        if report_handle is not None:
            try:
                report_handle.write(json.dumps(result, indent=2) + '\n')
            finally:
                report_handle.close()
    print(json.dumps({key: result.get(key) for key in ['status','bytes','sha256','path','sourceSha','runId','artifactId','elapsedSeconds','errorKind','failedPhase']}), flush=True)
    return 0 if result['status'] == 'PASS' else 1


if __name__ == '__main__':
    # Suppress traceback/context even for a failure in negative report cleanup:
    # an exception chain must never stringify an earlier signed-URL HTTPError.
    try:
        exit_code = main()
    except BaseException as error:
        print(json.dumps({'status': 'FAIL', 'errorKind': type(error).__name__,
                          'errorMessage': 'Resume coordinator failed; existing evidence retained.'}),
              flush=True)
        exit_code = 1
    raise SystemExit(exit_code)
