#!/usr/bin/env python3
"""Prepare owned Windows certificate controls; no SSH, process or native query.

This preparer deliberately has no execution mode. The root coordinator owns
the Windows serial slot and runs the emitted controls only after the R49 exact
transcript contract is frozen and the real diagnostic EXE has been built.
"""
import argparse
import ast
import base64
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import sys

sys.dont_write_bytecode = True
EVIDENCE = Path(__file__).resolve().parent


def sha(data):
    return hashlib.sha256(data).hexdigest()


def require(value, message):
    if not value:
        raise ValueError(message)


def load_ownership():
    binding = json.loads((EVIDENCE / "certificate-fixture-ownership-binding-first.json").read_text())
    for item in binding["sources"].values():
        require(sha(Path(item["path"]).read_bytes()) == item["sha256"], "ownership source SHA changed")
    source = Path(binding["sources"]["windows-tls-probe.py"]["path"])
    spec = importlib.util.spec_from_file_location("fc_r48_owned_certificate", source)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module, binding


def prepared_templates(ownership):
    original = "@('--tls-lifecycle-probe',$thumbprint,$marker)"
    require(ownership.LAUNCH.count(original) == 1, "launch argument contract changed")
    launch = ownership.LAUNCH.replace(original,
        "@('--https-settings-probe','--certificate-thumbprint',$thumbprint)")
    launch = launch.replace("TLS_FRONTDOOR_PROCESS_EXIT ", "HTTPS_CERTIFICATE_PROCESS_EXIT ")
    require(ownership.STOP.count("--tls-lifecycle-probe") == 1, "stop identity contract changed")
    stop = ownership.STOP.replace("--tls-lifecycle-probe", "--https-settings-probe")
    # The exact uploaded path already contains the marker. Both path and new
    # diagnostic flag remain mandatory in the unjournaled-process recovery.
    return {"setup": ownership.SETUP, "launch": launch, "stop": stop,
            "capture": ownership.CAPTURE, "cleanup": ownership.CLEANUP}


FACTS = ("found", "current_time", "private_key", "server_auth_eku",
         "server_name", "ssl_chain_policy", "cached_revocation")
CHECKS = {"Satisfied", "Rejected", "Unverified", "NotChecked"}


def validate_observation(observation, contract, owned_reference):
    """Strict semantic model; final wire-key spellings come from R49 freeze."""
    require(isinstance(observation, dict), "observation object missing")
    required = {"scope", "facts", "publication_revision", "identity_revision", "settings_revision"}
    require(set(observation) == required, "observation unknown/missing fields")
    require(observation["scope"] == "CurrentUserMyCacheOnly", "cache-only scope missing")
    for field in ("publication_revision", "identity_revision", "settings_revision"):
        require(type(observation[field]) is int and observation[field] > 0, "invalid revision")
    require(isinstance(observation["facts"], dict) and set(observation["facts"]) == set(FACTS), "seven exact native facts required")
    for item in observation["facts"].values():
        require(isinstance(item, str) and item in CHECKS, "unknown native check outcome")
    if owned_reference:
        for field in FACTS[:4]:
            require(observation["facts"][field] == "Satisfied", "owned certificate required fact failed: " + field)
    require(contract["listener"] == "NOT_ENABLED", "query must not enable listener")
    return {"facts": observation["facts"], "scope": observation["scope"],
            "positiveProductionPermit": False, "actualTlsHandshake": "NOT_EXERCISED",
            "onlineRevocation": "NOT_EXERCISED", "otherClientTrust": "NOT_EXERCISED"}


def validate_transcript(text, contract):
    require(contract.get("status") == "FROZEN_R49_PROBE_CONTRACT", "R49 contract is not frozen")
    expected = contract.get("steps")
    require(isinstance(expected, list) and expected and all(isinstance(x, str) and x for x in expected)
            and len(set(expected)) == len(expected), "exact unique step contract missing")
    steps, observations, finals, exits = [], [], [], []
    step_positions, final_positions, exit_positions = [], [], []
    for position, line in enumerate(text.splitlines()):
        if line.startswith("HTTPS_SETTINGS_NATIVE_STEP "):
            entry = json.loads(line[len("HTTPS_SETTINGS_NATIVE_STEP "):])
            require(set(entry) == {"index", "name"} and type(entry["index"]) is int, "step shape invalid")
            steps.append(entry)
            step_positions.append(position)
        elif line.startswith("HTTPS_SETTINGS_CERTIFICATE_OBSERVATION "):
            observations.append(json.loads(line[len("HTTPS_SETTINGS_CERTIFICATE_OBSERVATION "):]))
        elif line.startswith("HTTPS_SETTINGS_NATIVE_PASS "):
            finals.append(line)
            final_positions.append(position)
        elif line.startswith("HTTPS_CERTIFICATE_PROCESS_EXIT "):
            exits.append(json.loads(line[len("HTTPS_CERTIFICATE_PROCESS_EXIT "):]))
            exit_positions.append(position)
    require(steps == [{"index": i, "name": name} for i, name in enumerate(expected)], "ordered exact native steps mismatch")
    require(len(observations) == 1, "exactly one actual certificate observation required")
    require(len(finals) == 1 and finals[0] == contract["finalLine"], "exact native final footer mismatch")
    require(exits == [{"code": 0}], "actual owned EXE exit0 missing")
    require(max(step_positions) < final_positions[0] < exit_positions[0], "final/actual exit order invalid")
    # Normalize only through an explicit frozen field map; never infer success
    # from a PASS substring or silently drop unknown observation fields.
    require(contract.get("observationWireFormat") == "R48_MODEL_V1", "await exact R49 wire normalization")
    return validate_observation(observations[0], contract, True)


def prepare(root, marker, exe, thumbprint, output):
    require(re.fullmatch(r"FutureClassroom-Test-TLS-[0-9a-f]{32}", marker), "unique ownership marker required")
    require(root.endswith("\\" + marker) and "\x00" not in root and "\r" not in root and "\n" not in root, "owned root basename mismatch")
    require(re.fullmatch(r"[A-Za-z0-9_.-]+\.exe", exe), "plain diagnostic EXE basename required")
    require(thumbprint is None or re.fullmatch(r"[0-9a-fA-F]{40}", thumbprint), "public 40hex fixture reference required")
    ownership, binding = load_ownership()
    output.mkdir(parents=True, exist_ok=False)
    facts = {}
    for name, template in prepared_templates(ownership).items():
        if name == "launch" and thumbprint is None:
            continue
        data = (ownership.COMMON + ownership.script(template, root, marker, exe, thumbprint or "")).encode("utf-8-sig")
        path = output / ("tls-" + name + ".ps1")
        path.write_bytes(data)
        facts[name] = {"path": str(path), "bytes": len(data), "sha256": sha(data),
                       "sourceBase64": base64.b64encode(data).decode("ascii")}
    ast_check = """param([Parameter(Mandatory=$true)][string]$OwnedScriptRoot)
$ErrorActionPreference='Stop'
$result=@()
foreach($name in @('setup','launch','stop','capture','cleanup')){
 $path=Join-Path $OwnedScriptRoot ('tls-'+$name+'.ps1')
 $tokens=$null;$errors=$null
 [Management.Automation.Language.Parser]::ParseFile($path,[ref]$tokens,[ref]$errors)|Out-Null
 $result+=@{name=$name;errorCount=$errors.Count;sha256=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()}
 if($errors.Count-ne 0){throw ('Owned control AST failed: '+$name)}
}
@{status='PASS';scope='AST_ONLY_NO_FIXTURE_OR_EXE_EXECUTION';scripts=$result}|ConvertTo-Json -Depth 5 -Compress
"""
    ast_names = ",".join(ownership.ps_literal(name) for name in facts)
    ast_check = ast_check.replace("@('setup','launch','stop','capture','cleanup')", "@(" + ast_names + ")")
    (output / "parse-owned-controls.ps1").write_text(ast_check, encoding="utf-8-sig")
    report = {"status": "PREPARED_NOT_RUN", "windows": "NOT_RUN", "powershellAst": "NOT_RUN",
              "r49Contract": "AWAITING_FINAL_FREEZE", "root": root, "marker": marker,
              "launchPrepared": thumbprint is not None, "publicReferenceOnly": True, "sources": binding["sources"], "scripts": facts,
              "launchBudgetMillis": 90000, "ownershipObservationBudgetScope": "Original TLS supervisor bound unchanged; may honestly fail before the App's own120s deadline",
              "scriptChanges": "Only LAUNCH argument list/process-exit marker and STOP diagnostic flag differ. COMMON/SETUP/CAPTURE/CLEANUP exact original templates.",
              "forbiddenEffects": ["root-trust write", "PFX/private-key export", "HTTP.sys", "firewall", "Run", "listener", "foreground focus", "new business worker pool"],
              "unexercised": ["missing thumbprint", "wrong host", "mid-native cancellation", "Win7", "Win10", "Seewo"],
              "cleanupOrder": ["original owned supervisor+child actual stop", "stable full raw metadata/download/SHA verification", "exact owned cert+CNG deletion", "MY digest restored", "owned root absent"],
              "failurePolicy": "No blind retry. Keep originals and exact ownership journal; if stop/raw/key/cert proof fails retain root/resources and mark FAIL."}
    (output / "preparation.json").write_text(json.dumps(report, indent=2) + "\n")
    return report


def self_test():
    observation = {"scope": "CurrentUserMyCacheOnly", "facts": {x: "Satisfied" for x in FACTS},
                   "publication_revision": 4, "identity_revision": 1, "settings_revision": 2}
    contract = {"listener": "NOT_ENABLED"}
    validate_observation(observation, contract, True)
    cases = []
    for field in FACTS[:4]:
        bad = json.loads(json.dumps(observation)); bad["facts"][field] = "Unverified"; cases.append(bad)
    for mutation in (lambda x: x.update(scope="Online"), lambda x: x.update(ready=True),
                     lambda x: x["facts"].update(server_name="READY"), lambda x: x.update(publication_revision=True),
                     lambda x: x["facts"].pop("cached_revocation")):
        bad = json.loads(json.dumps(observation)); mutation(bad); cases.append(bad)
    for bad in cases:
        try: validate_observation(bad, contract, True)
        except ValueError: pass
        else: raise AssertionError("bad certificate model accepted")
    contract.update(status="FROZEN_R49_PROBE_CONTRACT", steps=["saved", "queried"],
                    finalLine="HTTPS_SETTINGS_NATIVE_PASS model-only", observationWireFormat="R48_MODEL_V1")
    lines = ["HTTPS_SETTINGS_NATIVE_STEP " + json.dumps({"index": i, "name": name}) for i, name in enumerate(contract["steps"])]
    lines += ["HTTPS_SETTINGS_CERTIFICATE_OBSERVATION " + json.dumps(observation), contract["finalLine"], "HTTPS_CERTIFICATE_PROCESS_EXIT " + json.dumps({"code": 0})]
    validate_transcript("\n".join(lines), contract)
    transcripts = [lines[:-1], lines+[lines[-2]], [lines[1],lines[0],*lines[2:]],
                   [lines[-2],*lines[:-2],lines[-1]], lines[:2]+[lines[2],lines[2]]+lines[3:],
                   lines[:-1]+["HTTPS_CERTIFICATE_PROCESS_EXIT " + json.dumps({"code": 1})]]
    for bad in transcripts:
        try: validate_transcript("\n".join(bad), contract)
        except ValueError: pass
        else: raise AssertionError("bad transcript model accepted")
    return {"status": "PURE_MODEL_PASS", "positiveCases": 2, "negativeCases": len(cases)+len(transcripts),
            "scope": "Model only; no PowerShell AST, SSH, Windows, CryptoAPI or EXE execution"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--root")
    parser.add_argument("--marker")
    parser.add_argument("--exe-name")
    parser.add_argument("--thumbprint")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.self_test:
        require(not any((args.root, args.marker, args.exe_name, args.thumbprint, args.output)), "self-test cannot prepare controls")
        print(json.dumps(self_test(), indent=2)); return
    require(all((args.root, args.marker, args.exe_name, args.output)), "root/marker/exe-name/output required; publicthumb only after real setup")
    result = prepare(args.root, args.marker, args.exe_name, args.thumbprint, args.output)
    print(json.dumps({"status": result["status"], "scripts": len(result["scripts"]), "windows": "NOT_RUN"}, indent=2))


if __name__ == "__main__":
    main()
