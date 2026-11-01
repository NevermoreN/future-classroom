"""Read-only exact GitHub run observation; never dispatch, retry or cancel."""
from pathlib import Path
import argparse
import base64
import datetime
import hashlib
import json
import subprocess
import urllib.request

ROOT = Path(__file__).resolve().parent
WORKFLOWS = {'modern': 'windows-msvc.yml', 'win7': 'windows-win7-msvc.yml'}

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--baseline', choices=WORKFLOWS, required=True)
    parser.add_argument('--label', required=True)
    parser.add_argument('--source-sha', required=True)
    parser.add_argument('--run-id', type=int, required=True)
    args = parser.parse_args()
    assert len(args.source_sha)==40 and all(c in '0123456789abcdef' for c in args.source_sha)
    assert args.run_id > 0
    assert (ROOT/'.owner').read_bytes()==b'future-classroom-owned-r74-ci-evidence/v1\n'
    workflow=WORKFLOWS[args.baseline]
    dispatch=json.loads((ROOT/('FutureClassroom-round74-'+workflow.removesuffix('.yml')+'-dispatch.json')).read_text())
    assert dispatch.get('status')=='RUN_IDENTIFIED' and dispatch.get('sourceSha')==args.source_sha and dispatch.get('runId')==args.run_id and dispatch.get('workflowPath')=='.github/workflows/'+workflow
    assert args.label.isalnum() and args.label.isascii()
    output = ROOT / (args.baseline + '-run-' + args.label + '.json')
    receipt = ROOT / (args.baseline + '-run-observation-' + args.label + '.json')
    assert not output.exists() and not receipt.exists()
    credentials = subprocess.run(['git', 'credential', 'fill'], input=b'protocol=https\nhost=github.com\n\n', capture_output=True, timeout=15, check=True)
    values = dict(line.split('=', 1) for line in credentials.stdout.decode().splitlines() if '=' in line)
    token = values.get('password')
    assert token
    route = '/repos/NevermoreN/future-classroom/actions/runs/' + str(args.run_id)
    request = urllib.request.Request('https://api.github.com' + route, headers={'Authorization': 'Bearer ' + token, 'Accept': 'application/vnd.github+json', 'X-GitHub-Api-Version': '2026-03-10', 'User-Agent': 'FutureClassroom-readonly-validation'})
    with urllib.request.urlopen(request, timeout=25) as response:
        data = response.read(2 * 1024 * 1024 + 1)
        assert response.status == 200 and len(data) <= 2 * 1024 * 1024 and token.encode() not in data
    run = json.loads(data)
    assert run['id'] == args.run_id and run['head_sha'] == args.source_sha and run['event'] == 'workflow_dispatch' and run['head_branch']=='main' and run['path']=='.github/workflows/'+workflow
    with output.open('xb') as stream:
        stream.write(data)
    observed = {'status': 'PASS_READ_ONLY_OBSERVATION', 'recordedUtc': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'scope': 'Only GET of exact dispatched run; connector has no exact workflow_dispatch run lookup. No dispatch/retry/cancel/download.', 'method': 'GET', 'route': route, 'runStatus': run['status'], 'conclusion': run['conclusion'], 'sourceSha': args.source_sha, 'response': {'path': output.name, 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest(), 'base64': base64.b64encode(data).decode('ascii')}}
    with receipt.open('x') as stream:
        json.dump(observed, stream, indent=2)
        stream.write('\n')
    print(json.dumps({k: observed[k] for k in ('status', 'runStatus', 'conclusion', 'sourceSha')}))

if __name__ == '__main__':
    main()
