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


CONTRACT_SHA = "017da7e35aec46dbf522db43716ed8f316b44837b51255ddb894f84212596943"
FACTS = ("found", "currentTime", "privateKey", "serverAuthEku", "serverName", "sslChainPolicy", "cachedRevocation")
CHECKS = {"Satisfied", "Rejected", "Unverified", "NotChecked"}


def load_contract():
    data = (EVIDENCE / "r49-probe-contract-bound-third.json").read_bytes()
    require(sha(data) == CONTRACT_SHA, "exact R49 final contract SHA changed")
    value = json.loads(data)
    require(value["status"] == "FROZEN_SOURCE_CONTRACT_NOT_BUILT_NOT_RUN", "not frozen source contract")
    require(len(value["steps"]) == 22 and len(set(value["steps"])) == 22, "exact22 steps required")
    return value


def integer(value, minimum, maximum):
    return type(value) is int and minimum <= value <= maximum


def validate_observation(observation, contract, owned_reference=True):
    require(isinstance(observation, dict), "observation object missing")
    require(set(observation) == set(contract["observation"]["jsonKeys"]), "observation unknown/missing fields")
    require(observation["scope"] == "CurrentUserMyCacheOnly" and observation["readiness"] == "NOT_GRANTED", "cache-only/no-permit scope missing")
    for field in ("publicationRevision", "identityRevision", "settingsRevision"):
        require(integer(observation[field], 1, 2**64-1), "invalid revision")
    for field in ("chainTrustErrors", "sslPolicyError"):
        require(observation[field] is None or integer(observation[field], 0, 2**32-1), "invalid chain/policy status")
    for field in FACTS:
        item = observation[field]
        require(isinstance(item, dict) and set(item) == {"check", "nativeError"}, "fact shape invalid")
        require(type(item["check"]) is str and item["check"] in CHECKS, "unknown check outcome")
        require(item["nativeError"] is None or integer(item["nativeError"], -2**31, 2**31-1), "invalid native status")
        require(item["check"] not in {"Satisfied", "NotChecked"} or item["nativeError"] is None, "success/unchecked fact has error")
    if owned_reference:
        for field in contract["observation"]["ownedReferenceRequiredFacts"]:
            require(observation[field]["check"] == "Satisfied", "owned certificate required fact failed: " + field)
    else:
        require(observation["found"]["check"] == "Rejected" and all(observation[x]["check"] == "NotChecked" for x in FACTS[1:]), "missing-reference case is not an actual absent selection")
    if observation["sslChainPolicy"]["check"] == "Satisfied" or observation["serverName"]["check"] == "Satisfied":
        require(observation["sslPolicyError"] == 0 and observation["chainTrustErrors"] == 0, "positive policy/name contradicts actual status")
    return {"facts": {x: observation[x] for x in FACTS}, "scope": observation["scope"],
            "readiness": "NOT_GRANTED", "positiveProductionPermit": False,
            "actualTlsHandshake": "NOT_EXERCISED", "onlineRevocation": "NOT_EXERCISED", "otherClientTrust": "NOT_EXERCISED"}


def strict_json(text):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, "duplicate JSON key")
            result[key] = value
        return result
    return json.loads(text, object_pairs_hook=unique)


def validate_transcript(text, contract=None, owned_reference=True):
    contract = load_contract() if contract is None else contract
    # An explicit caller cannot silently substitute a different step/shape.
    require(contract == load_contract(), "validator contract differs from bound final source")
    steps, observations, finals, exits = [], [], [], []
    step_positions, observation_positions, final_positions, exit_positions = [], [], [], []
    for position, line in enumerate(text.splitlines()):
        if line.startswith("HTTPS_SETTINGS_NATIVE_STEP"):
            match = re.fullmatch(r"HTTPS_SETTINGS_NATIVE_STEP index=(0|[1-9][0-9]*) name=([a-z0-9-]+)", line)
            require(match is not None, "step plain-wire shape invalid")
            steps.append({"index": int(match[1]), "name": match[2]}); step_positions.append(position)
        elif line.startswith("HTTPS_SETTINGS_CERTIFICATE_OBSERVATION"):
            require(line.startswith("HTTPS_SETTINGS_CERTIFICATE_OBSERVATION "), "observation prefix invalid")
            observations.append(strict_json(line[len("HTTPS_SETTINGS_CERTIFICATE_OBSERVATION "):])); observation_positions.append(position)
        elif line.startswith("HTTPS_SETTINGS_NATIVE_PASS"):
            finals.append(line); final_positions.append(position)
        elif line.startswith("HTTPS_CERTIFICATE_PROCESS_EXIT"):
            require(line.startswith("HTTPS_CERTIFICATE_PROCESS_EXIT "), "exit prefix invalid")
            exits.append(strict_json(line[len("HTTPS_CERTIFICATE_PROCESS_EXIT "):])); exit_positions.append(position)
    require(steps == [{"index": i, "name": name} for i, name in enumerate(contract["steps"])], "ordered exact22 native steps mismatch")
    require(len(observations) == 1 and len(finals) == 1 and exits == [{"code": 0}] and type(exits[0]["code"]) is int, "observation/footer/actual exit count or status failed")
    prefix = contract["footer"].removesuffix("<actual int>")
    require(finals[0].startswith(prefix), "exact final footer mismatch")
    elapsed = finals[0][len(prefix):]
    require(re.fullmatch(r"0|[1-9][0-9]*", elapsed) is not None and len(elapsed) <= 6 and int(elapsed) <= contract["budgets"]["finiteNativeWorkflowSeconds"]*1000, "actual native elapsed missing/out-of-budget")
    require(max(step_positions) < final_positions[0] < exit_positions[0], "final/actual exit order invalid")
    check_index = contract["steps"].index("explicit-native-certificate-check")
    require(step_positions[check_index-1] < observation_positions[0] < step_positions[check_index], "observation not in actual native check interval")
    result = validate_observation(observations[0], contract, owned_reference)
    result.update(exactSteps=22, actualProcessExit=0, nativeElapsedMillis=int(elapsed))
    return result


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
              "r49Contract": CONTRACT_SHA, "root": root, "marker": marker,
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
    contract = load_contract()
    observation = {"scope": "CurrentUserMyCacheOnly", "publicationRevision": 4, "identityRevision": 1, "settingsRevision": 2,
                   "chainTrustErrors": 32, "sslPolicyError": 0x800b0109, "readiness": "NOT_GRANTED"}
    observation.update({x: {"check": "Satisfied" if x in FACTS[:4] else "Unverified", "nativeError": None} for x in FACTS})
    def transcript(obs=observation):
        lines = ["HTTPS_SETTINGS_NATIVE_STEP index=" + str(i) + " name=" + name for i,name in enumerate(contract["steps"])]
        lines.insert(8, "HTTPS_SETTINGS_CERTIFICATE_OBSERVATION " + json.dumps(obs))
        lines += [contract["footer"].replace("<actual int>", "1234"), "HTTPS_CERTIFICATE_PROCESS_EXIT {\"code\":0}"]
        return lines
    positive = [transcript()]
    clean = json.loads(json.dumps(observation)); clean.update(chainTrustErrors=0, sslPolicyError=0)
    for field in FACTS: clean[field] = {"check":"Satisfied", "nativeError":None}
    positive.append(transcript(clean))
    for lines in positive: validate_transcript("\n".join(lines))
    negatives = []
    for field in FACTS[:4]:
        bad = json.loads(json.dumps(observation)); bad[field]["check"]="Unverified"; negatives.append(transcript(bad))
    for mutation in (lambda x:x.update(scope="Online"), lambda x:x.update(readiness="GRANTED"), lambda x:x.update(extra=True),
        lambda x:x.pop("cachedRevocation"), lambda x:x["serverName"].update(check="READY"), lambda x:x.update(publicationRevision=True),
        lambda x:x.update(settingsRevision=0), lambda x:x.update(identityRevision=2**64), lambda x:x.update(chainTrustErrors=-1),
        lambda x:x.update(sslPolicyError=2**32), lambda x:x["privateKey"].update(nativeError=True),
        lambda x:x["privateKey"].update(nativeError=-2**31-1), lambda x:x["found"].update(nativeError=5),
        lambda x:x["privateKey"].update(extra=1), lambda x:x["sslChainPolicy"].update(check="Satisfied")):
        bad=json.loads(json.dumps(observation)); mutation(bad); negatives.append(transcript(bad))
    base=transcript()
    negatives += [base[:-1], base+[base[-2]], [base[1],base[0],*base[2:]],
                  [base[-2],*base[:-2],base[-1]], base[:8]+[base[8],base[8]]+base[9:],
                  base[:-1]+['HTTPS_CERTIFICATE_PROCESS_EXIT {"code":1}'], base[:-1]+['HTTPS_CERTIFICATE_PROCESS_EXIT {"code":false}'],
                  base[:-2]+[base[-2].replace("elapsed_ms=1234","elapsed_ms=120001"),base[-1]],
                  base[:-2]+[base[-2].replace("elapsed_ms=1234","elapsed_ms=01"),base[-1]],
                  base[:8]+base[9:]+[base[8]], base[:8]+[base[8].replace('"readiness": "NOT_GRANTED"','"readiness": "NOT_GRANTED", "readiness":"GRANTED"')]+base[9:],
                  [base[0]+" extra=1",*base[1:]], base[:-2]+[base[-2].replace("shared_workers=2","shared_workers=3"),base[-1]]]
    for lines in negatives:
        try: validate_transcript("\n".join(lines))
        except (ValueError, TypeError): pass
        else: raise AssertionError("bad real-wire model accepted")
    return {"status":"PURE_MODEL_PASS", "positiveCases":len(positive), "negativeCases":len(negatives),
            "contractSha256":CONTRACT_SHA, "wire":"Actual22 plainsteps +exact14key nativeobservation +footer +ownedprocess exit",
            "scope":"Synthetic transcript models only; no PowerShell AST, SSH, Windows, CryptoAPI or EXE execution"}


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
