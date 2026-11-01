"""Exact staged/committed proof; no mutation of original evidence or test roots."""
from pathlib import Path
import argparse
import datetime
import hashlib
import json
import subprocess

ROOT = Path(__file__).resolve().parents[2]
HISTORY = ROOT / "开发计划/验证记录"
NAMES = ["round46-msvc", "round47-https-settings", "round48-certificate-query",
         "round49-https-ui", "round50-https-integration", "round51-browser-diagnostics"]


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=["staged", "committed"])
    parser.add_argument("--commit")
    args = parser.parse_args()
    required = set()
    indexes = []
    for name in NAMES:
        base = HISTORY / ("2026-11-01-" + name)
        index = json.loads((base / "INDEX.json").read_text())
        expected = {row["path"] for row in index["files"]} | {"INDEX.json", "INDEX.md"}
        actual = {p.relative_to(base).as_posix() for p in base.rglob("*") if p.is_file()}
        assert actual == expected, (name, actual ^ expected)
        for row in index["files"]:
            path = base / row["path"]
            assert not path.is_symlink()
            data = path.read_bytes()
            assert len(data) == row["bytes"] and digest(data) == row["sha256"], str(path)
        required.update((base / p).relative_to(ROOT).as_posix() for p in expected)
        indexes.append({"path": (base / "INDEX.json").relative_to(ROOT).as_posix(),
                        "originals": len(index["files"]),
                        "sha256": digest((base / "INDEX.json").read_bytes())})
    if args.mode == "staged":
        assert args.commit is None
        paths = git("diff", "--cached", "--name-only", "-z", "--diff-filter=ACMRT").decode().rstrip("\0").split("\0")
        revision = ""
    else:
        assert args.commit and len(args.commit) == 40
        prior = json.loads((HISTORY / "2026-11-01-round46-51-staged-proof.json").read_text())
        paths = [row["path"] for row in prior["files"]]
        paths.append("开发计划/验证记录/2026-11-01-round46-51-staged-proof.json")
        revision = args.commit
    assert required <= set(paths), required - set(paths)
    process = subprocess.Popen(["git", "cat-file", "--batch"], cwd=ROOT,
                               stdin=subprocess.PIPE, stdout=subprocess.PIPE)
    rows = []
    try:
        for name in paths:
            assert "\n" not in name and "\r" not in name
            process.stdin.write((revision + ":" + name + "\n").encode())
            process.stdin.flush()
            header = process.stdout.readline().decode().strip().split()
            assert len(header) == 3 and header[1] == "blob", (name, header)
            length = int(header[2])
            data = process.stdout.read(length)
            assert len(data) == length and process.stdout.read(1) == b"\n"
            local = ROOT / name
            assert local.is_file() and not local.is_symlink() and local.read_bytes() == data, name
            rows.append({"path": name, "bytes": length, "sha256": digest(data), "gitBlob": header[0]})
    finally:
        process.stdin.close()
        assert process.wait(timeout=30) == 0
    out = HISTORY / ("2026-11-01-round46-51-" + args.mode + "-proof.json")
    with out.open("x") as stream:
        json.dump({"status": "PASS_EXACT_GIT_BLOBS", "mode": args.mode, "commit": args.commit,
                   "recordedUtc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
                   "requiredArchiveFiles": len(required), "indexes": indexes, "files": rows,
                   "scope": "All six original archives and INDEX members, including ignored raw logs, are exact Git blobs before owned working-copy cleanup. No OS compatibility or production admission claim."}, stream, indent=2)
        stream.write("\n")
    print(json.dumps({"status": "PASS_EXACT_GIT_BLOBS", "mode": args.mode,
                      "files": len(rows), "requiredArchiveFiles": len(required)}))


if __name__ == "__main__":
    main()
