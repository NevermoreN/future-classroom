#!/usr/bin/env python3
"""Publish D14 metadata with python-tuf, never custom signature canonicalization.

Install scripts/update-publisher-requirements.txt into an isolated maintainer
venv. Private keys MUST live outside this repository in a mode-0700 directory.
`init-root` is an explicit maintainer operation; product machines never run it.
The three root keys must subsequently be separated into offline custody.
"""
from __future__ import annotations
import argparse
from datetime import datetime, timedelta, timezone
import json
import os
from pathlib import Path
import stat
import sys
from tuf.api.metadata import Metadata, Root, Targets, Snapshot, Timestamp, TargetFile, MetaFile
from securesystemslib.signer import CryptoSigner

REPOSITORY = Path(__file__).resolve().parent.parent

def private_directory(path, create=False):
    path = path.resolve()
    if path.is_relative_to(REPOSITORY):
        raise ValueError("private publishing keys must remain outside the repository")
    if create:
        path.mkdir(mode=0o700)
    info = path.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_mode & 0o077:
        raise ValueError("private key directory must have mode 0700")
    return path

def exclusive(path, data, mode=0o644):
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, mode)
    with os.fdopen(fd, "wb") as stream:
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())

def read_public_root(path):
    root = Metadata.from_file(str(path))
    if not isinstance(root.signed, Root):
        raise ValueError("installed anchor must be a TUF root")
    root.signed.verify_delegate("root", root.signed_bytes, root.signatures)
    return root

def init_root(args):
    keys = private_directory(args.key_dir, create=True)
    now = datetime.now(timezone.utc)
    root = Metadata(Root(version=1, spec_version="1.0.36", expires=now + timedelta(days=1825), consistent_snapshot=False))
    signers = []
    for name, role in [("root-1", "root"), ("root-2", "root"), ("root-3", "root"), ("targets", "targets"), ("snapshot", "snapshot"), ("timestamp", "timestamp")]:
        signer = CryptoSigner.generate_ed25519()
        exclusive(keys / f"{name}.pem", signer.private_bytes, 0o600)
        root.signed.add_key(signer.public_key, role)
        if role == "root":
            signers.append(signer)
    root.signed.roles["root"].threshold = 2
    for signer in signers[:2]:
        root.sign(signer, append=True)
    exclusive(args.output, root.to_bytes())
    print("Created public root; move the three root keys into separate offline custody.")

def signer_for(root, keys, role):
    role_keys = root.signed.roles[role].keyids
    if len(role_keys) != 1 or root.signed.roles[role].threshold != 1:
        raise ValueError("online publishing requires exactly one distinct key per role")
    path = keys / f"{role}.pem"
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or info.st_mode & 0o077:
        raise ValueError("private role key must be a mode-0600 regular file")
    return CryptoSigner.from_priv_key_uri("file2:" + str(path), root.signed.keys[role_keys[0]])

def target_path(channel, sequence, payload_name, multiple):
    """Keep the existing consumer contract ending in /payload.zip.

    Multiple targets use a safe basename-derived directory: legacy-payload.zip
    becomes legacy/payload.zip; another-name.zip becomes another-name/payload.zip.
    Single-target catalogs retain channel/sequence/payload.zip.
    """
    parts = [channel, str(sequence)]
    if multiple:
        if not payload_name.endswith(".zip"):
            raise ValueError("multi-target payloads need distinct safe .zip basenames")
        segment = payload_name[:-4].removesuffix("-payload")
        parts.append(segment)
    parts.append("payload.zip")
    path = "/".join(parts)
    if len(path) > 200 or any(not part or part in (".", "..") or any(
        c not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_.-"
        for c in part) for part in parts):
        raise ValueError("target path violates the installed consumer contract")
    return path

def publish(args):
    # Reuse the canonical bounded transport inspector, not arbitrary ZIP extraction.
    import runpy
    transport = runpy.run_path(str(Path(__file__).with_name("package-update-envelope.py")))
    inputs = [(args.payload, args.claims)] + list(getattr(args, "additional_target", None) or [])
    if len(inputs) > 16:
        raise ValueError("at most sixteen targets per signed catalog")
    inspected = []
    for payload, claims in inputs:
        size = transport["regular"](payload).st_size
        with payload.open("rb") as source:
            transport["inspect_payload"](source, size)
        custom = json.loads(claims.read_text(encoding="utf-8"))
        current_sequence = custom.get("releaseSequence")
        if type(current_sequence) is not int or not 1 <= current_sequence < 2**63:
            raise ValueError("releaseSequence must be a positive monotonic integer")
        if custom.get("product") != "FutureClassroom" or custom.get("installerAbi") != 2 or custom.get("guardCapability") != 3 or not custom.get("allowedPredecessors"):
            raise ValueError("formal ABI2/capability3 release and exact predecessors required")
        inspected.append((payload, custom, size))
    custom = inspected[0][1]
    sequence = custom.get("releaseSequence")
    if not isinstance(sequence, int) or isinstance(sequence, bool) or not 1 <= sequence < 2**63:
        raise ValueError("releaseSequence must be a positive monotonic integer")
    for _, other, _ in inspected[1:]:
        if any(other.get(key) != custom.get(key) for key in (
            "releaseSequence", "channel", "version", "product", "architecture",
            "minimumWindows", "installerAbi", "guardCapability", "sourceSchema", "sourceProtocol")):
            raise ValueError("all targets must describe the same release, platform and protocol")
    root = read_public_root(args.root)
    keys = private_directory(args.key_dir)
    state = keys / "last-release-sequence"
    if state.exists() and sequence <= int(state.read_text(encoding="ascii")):
        raise ValueError("release sequence must exceed the maintainer's durable high-water mark")
    now = datetime.now(timezone.utc)
    if root.signed.expires <= now:
        raise ValueError("root expired; perform an offline root rotation first")
    channel = custom.get("channel", "")
    if not channel or any(c not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_.-" for c in channel):
        raise ValueError("invalid release channel")
    target_files, results = {}, []
    for payload, claims, size in inspected:
        path = target_path(channel, sequence, payload.name, len(inspected) > 1)
        if path in target_files:
            raise ValueError("duplicate target path")
        target = TargetFile.from_file(path, str(payload), ["sha256"])
        target.unrecognized_fields["custom"] = claims
        target_files[path] = target
        results.append({"target": path, "payloadBytes": size, "sha256": target.hashes["sha256"]})
    targets = Metadata(Targets(version=sequence, spec_version="1.0.36", expires=now + timedelta(days=365), targets=target_files))
    targets.sign(signer_for(root, keys, "targets"))
    targets_bytes = targets.to_bytes()
    snapshot = Metadata(Snapshot(version=sequence, spec_version="1.0.36", expires=now + timedelta(days=365), meta={"targets.json": MetaFile.from_data(sequence, targets_bytes, ["sha256"])}))
    snapshot.sign(signer_for(root, keys, "snapshot"))
    snapshot_bytes = snapshot.to_bytes()
    timestamp = Metadata(Timestamp(version=sequence, spec_version="1.0.36", expires=now + timedelta(days=90), snapshot_meta=MetaFile.from_data(sequence, snapshot_bytes, ["sha256"])))
    timestamp.sign(signer_for(root, keys, "timestamp"))
    args.output_dir.mkdir(mode=0o755)
    for name, data in [("timestamp.json", timestamp.to_bytes()), ("snapshot.json", snapshot_bytes), ("targets.json", targets_bytes)]:
        exclusive(args.output_dir / name, data)
    # Only advance after all public roles are durable. A failed retry with an
    # existing output directory requires a new sequence, never silent overwrite.
    pending = keys / "last-release-sequence.pending"
    exclusive(pending, str(sequence).encode("ascii"), 0o600)
    os.replace(pending, state)
    result = dict(results[0]) if len(results) == 1 else {"targets": results}
    result["rootVersion"] = root.signed.version
    print(json.dumps(result))

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    init = commands.add_parser("init-root")
    init.add_argument("--key-dir", type=Path, required=True)
    init.add_argument("--output", type=Path, required=True)
    command = commands.add_parser("publish")
    for name in ("root", "key-dir", "payload", "claims", "output-dir"):
        command.add_argument("--" + name, type=Path, required=True)
    command.add_argument("--additional-target", type=Path, nargs=2, action="append",
        metavar=("PAYLOAD_ZIP", "CLAIMS_JSON"),
        help="Sign another pair once; legacy-payload.zip maps to legacy/payload.zip under channel/sequence")
    args = parser.parse_args()
    try:
        init_root(args) if args.command == "init-root" else publish(args)
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f"update publication refused: {error}", file=sys.stderr)
        return 1
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
