#!/usr/bin/env bash
# Supplemental Linux -> Win7 GNU cross-linking only. Preserve GCC's CRT/library
# selection while using the active Rust toolchain's LLD for PE/TLS relocations.
# No ASLR or NX flags are disabled. This is not the formal MSVC baseline.
set -euo pipefail

compiler="${RUSTC:-rustc}"
sysroot="$("$compiler" --print sysroot)"
host="$("$compiler" -vV | sed -n 's/^host: //p')"
rust_lld="$sysroot/lib/rustlib/$host/bin/rust-lld"
if [[ ! -x "$rust_lld" ]]; then
    printf 'Active toolchain rust-lld is unavailable: %s\n' "$rust_lld" >&2
    exit 1
fi

# Some cross-GCC distributions hardcode GNU ld in collect2, ignoring -B and
# -fuse-ld. Obtain GCC's exact link plan without executing it, then run LLD
# explicitly. Python shlex parses GCC's quoted diagnostic; no shell eval occurs.
exec python3 - "$rust_lld" "$@" <<'PY'
from pathlib import Path
import shlex
import subprocess
import sys

planned = subprocess.run(
    ["i686-w64-mingw32-gcc", "-###", *sys.argv[2:]],
    capture_output=True, text=True,
)
if planned.returncode:
    sys.stderr.write(planned.stderr)
    raise SystemExit(planned.returncode)
commands = []
for line in planned.stderr.splitlines():
    try:
        tokens = shlex.split(line)
    except ValueError:
        continue
    if tokens and Path(tokens[0]).name in {"collect2", "ld", "i686-w64-mingw32-ld"}:
        commands.append(tokens[1:])
if len(commands) != 1:
    sys.stderr.write("Expected exactly one GCC link command; refusing a GNU ld fallback.\n")
    raise SystemExit(1)
arguments = []
source = iter(commands[0])
for argument in source:
    # LLD consumes Rust-generated machine objects directly; GCC's LTO plugin is
    # neither needed nor accepted by LLD. Rust passes -fno-use-linker-plugin.
    if argument in {"-plugin", "--plugin"}:
        next(source, None)
    elif argument.startswith(("-plugin-opt=", "--plugin-opt=")):
        continue
    else:
        arguments.append(argument)
raise SystemExit(subprocess.call([sys.argv[1], "-flavor", "gnu", *arguments]))
PY
