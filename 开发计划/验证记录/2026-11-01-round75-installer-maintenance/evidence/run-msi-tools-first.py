#!/usr/bin/env python3
"""R75 bounded local tool preparation, exact frozen source checks before/after."""
from pathlib import Path
import ast
import datetime
import hashlib
import json
import stat
import subprocess
import time

ROOT = Path('/tmp/fc-round75-installer-maintenance')
E = ROOT / 'evidence'
FREEZE_SHA = '0e3ee0df68cc6362c87860ec9f39bf37ca07f93fb050b29986a5950c7fd1f6a5'


def sha(data):
    return hashlib.sha256(data).hexdigest()


def frozen():
    assert (ROOT / '.owner').read_bytes() == b'future-classroom-owned-r75-installer-maintenance/v1\n'
    raw = (E / 'root-joint-source-freeze-first.json').read_bytes()
    assert sha(raw) == FREEZE_SHA
    listing = json.loads(raw)['sources']
    assert len(listing) == 8
    result = []
    for row in listing:
        path = ROOT / 'worktree' / row['path']
        assert stat.S_ISREG(path.lstat().st_mode)
        data = path.read_bytes()
        actual = {'path': row['path'], 'bytes': len(data), 'sha256': sha(data)}
        assert actual == row
        result.append(actual)
    return result


def fact(path):
    data = path.read_bytes()
    return {'path': str(path), 'bytes': len(data), 'sha256': sha(data)}


def main():
    report = {'schemaVersion': 1, 'status': 'RUNNING', 'startedUtc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'scope': 'Seven exact tool downloads and local extraction/load checks only. No MSI, Windows, Cargo, system install or main writes.',
              'sourceFreezeSha256': FREEZE_SHA, 'sourceBefore': frozen(), 'commands': []}
    config = json.loads((E / 'msi-tools-input-first.json').read_bytes())
    commands = [('download-' + p['name'], ['python3', '-B', str(E / 'download-msi-tool-first.py'), p['name']], 135)
                for p in config['packages']]
    commands.append(('prepare-local', ['python3', '-B', str(E / 'prepare-msi-tools-local-first.py')], 1200))
    try:
        for name, command, controller_deadline in commands:
            before = frozen()
            for script in (E / 'download-msi-tool-first.py', E / 'prepare-msi-tools-local-first.py'):
                ast.parse(script.read_bytes(), filename=str(script))
            stdout = E / ('msi-tools-controller-' + name + '-first.stdout.log')
            stderr = E / ('msi-tools-controller-' + name + '-first.stderr.log')
            began = time.monotonic()
            with stdout.open('xb') as out, stderr.open('xb') as err:
                process = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=out, stderr=err)
                try:
                    code = process.wait(timeout=controller_deadline)
                    timed_out = False
                except subprocess.TimeoutExpired:
                    # Kill only this original owned preparation child; original
                    # tools have their own unchanged bounded command budgets.
                    process.kill()
                    code = process.wait(timeout=5)
                    timed_out = True
            row = {'name': name, 'command': command, 'pid': process.pid, 'actualExit': True,
                   'parentReaped': True, 'exitCode': code, 'controllerTimedOut': timed_out,
                   'controllerDeadlineSeconds': controller_deadline, 'elapsedSeconds': time.monotonic() - began,
                   'stdout': fact(stdout), 'stderr': fact(stderr), 'sourceBefore': before}
            report['commands'].append(row)
            after = frozen()
            row.update(sourceAfter=after, sourceUnchanged=before == after)
            assert not timed_out and code == 0 and row['sourceUnchanged']
        local = json.loads((E / 'msi-tools-preparation-first.json').read_bytes())
        assert local['status'] == 'PASS_EXACT_PACKAGES_LOCAL_EXTRACTION_ELF_BINDING_AND_SCOPED_TOOL_LOADING_ONLY'
        assert len(local['packages']) == 7 and len(local['tools']) == 4 and len(local['libraries']) == 23
        report['sourceAfter'] = frozen()
        report['sourceUnchanged'] = report['sourceBefore'] == report['sourceAfter']
        assert report['sourceUnchanged']
        report['preparation'] = fact(E / 'msi-tools-preparation-first.json')
        report['status'] = 'PASS_EXACT_OWNED_TOOL_PREPARATION_AND_EIGHT_SOURCE_GUARDS'
    except BaseException as error:
        report['status'] = 'FAIL_RETAIN_ORIGINALS'
        report['failureType'] = type(error).__name__
        raise
    finally:
        report['completedUtc'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
        with (E / 'msi-tools-controller-first.json').open('x') as output:
            json.dump(report, output, ensure_ascii=False, indent=2)
            output.write('\n')
    print(json.dumps({'status': report['status'], 'actualReapedChildren': len(report['commands']),
                      'preparationSha256': report['preparation']['sha256'], 'sourceUnchanged': report['sourceUnchanged']}))


if __name__ == '__main__':
    main()
