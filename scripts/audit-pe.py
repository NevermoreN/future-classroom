#!/usr/bin/env python3
"""Inspect a PE probe without executing it or claiming operating-system support."""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import subprocess
import sys


# These are review triggers, not a substitute for checking the exact target OS
# exports, prerequisite updates, dynamic loads, and real machine behavior.
MODERN_APIS = {
    "WaitOnAddress", "WakeByAddressSingle", "WakeByAddressAll",
    "GetSystemTimePreciseAsFileTime", "SetThreadDescription", "GetThreadDescription",
    "GetTempPath2W", "GetTempPath2A", "CreateFile2", "GetOverlappedResultEx",
    "GetCurrentThreadStackLimits", "GetSystemTimeAdjustmentPrecise",
    "GetProcessMitigationPolicy", "SetProcessMitigationPolicy",
    "RoInitialize", "RoUninitialize", "RoGetActivationFactory", "RoActivateInstance",
    "WindowsCreateString", "WindowsDeleteString", "WindowsGetStringRawBuffer",
}
UPDATE_SENSITIVE_APIS = {
    "AddDllDirectory", "RemoveDllDirectory", "SetDefaultDllDirectories",
    "GetSystemCpuSetInformation", "SetThreadSelectedCpuSets", "GetThreadSelectedCpuSets",
}


def parse_objdump(output: str) -> dict:
    errors = []
    if not re.search(r"file format\s+pei-i386\b", output):
        errors.append("Expected an x86 PE image (objdump file format pei-i386).")
    if not re.search(r"^\s*Magic\s+010b\s+\(PE32\)", output, re.MULTILINE):
        errors.append("Expected PE32 optional header; PE32+ is not accepted.")
    if not re.search(r"^\s*Subsystem\s+0*3\s+\(Windows CUI\)", output, re.MULTILINE):
        errors.append("Expected Windows console subsystem (CUI = 3).")
    imports: dict[str, set[str]] = {}
    dll = None
    for line in output.splitlines():
        match = re.search(r"DLL Name:\s*(\S+)", line)
        if match:
            dll = match.group(1).lower()
            imports.setdefault(dll, set())
            continue
        if re.match(r"\s*(The Function Table|PE File Base Relocations|The Debug Directory|Dump of)", line):
            dll = None
        symbol = re.match(r"^\s*[0-9a-fA-F]+\s+\d+\s+([A-Za-z_?@][A-Za-z0-9_?@$]*)\s*$", line)
        if dll and symbol:
            imports[dll].add(symbol.group(1))

    risks = []
    for dll, symbols in sorted(imports.items()):
        if dll == "combase.dll" or dll.startswith("api-ms-win-core-winrt"):
            risks.append({"kind": "WINRT_DLL", "dll": dll, "reason": "WinRT/COM import requires baseline OS export validation; Win7 is not established."})
        elif dll.startswith(("api-ms-", "ext-ms-")):
            risks.append({"kind": "API_SET_DLL", "dll": dll, "reason": "API-set contract availability must be verified on the exact OS and update baseline."})
        if dll.startswith(("vcruntime", "msvcp", "libgcc_s_", "libstdc++", "libwinpthread")) or dll == "ucrtbase.dll":
            risks.append({"kind": "RUNTIME_DEPLOYMENT", "dll": dll, "reason": "Runtime prerequisite is not bundled by this probe package; deployment must be verified."})
        for symbol in sorted(symbols):
            if symbol in MODERN_APIS:
                risks.append({"kind": "MODERN_API", "dll": dll, "api": symbol, "reason": "Post-baseline API review trigger; verify target exports and load behavior."})
            elif symbol in UPDATE_SENSITIVE_APIS:
                risks.append({"kind": "OS_UPDATE_OR_API_REVIEW", "dll": dll, "api": symbol, "reason": "Availability varies by OS/update baseline; verify exact target exports."})
    return {
        "status": "NOT_VALIDATED",
        "format": "PE32-x86" if not errors else "INVALID_PROBE_FORMAT",
        "subsystem": "Windows CUI" if not errors else "UNCONFIRMED",
        "structural_errors": errors,
        "imports": {name: sorted(symbols) for name, symbols in sorted(imports.items())},
        "risks": risks,
        "limitations": [
            "Static import inspection cannot establish Win7 compatibility.",
            "Dynamic loads, delayed paths, prerequisite updates and actual OS behavior require separate validation.",
            "Formal MSVC baseline and real Win7 SP1 x86 machine gates remain NOT_VALIDATED.",
        ],
    }


def audit_pe(executable: Path, objdump: str = "i686-w64-mingw32-objdump") -> dict:
    if not executable.is_file():
        raise ValueError(f"Executable does not exist: {executable}")
    inspected = subprocess.run([objdump, "-p", str(executable)], check=True, capture_output=True, text=True, timeout=60)
    result = parse_objdump(inspected.stdout)
    version = subprocess.run([objdump, "--version"], check=True, capture_output=True, text=True, timeout=10)
    result["inspector"] = version.stdout.splitlines()[0]
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path, required=True, help="Built probe executable to inspect; never executed.")
    parser.add_argument("--objdump", default="i686-w64-mingw32-objdump")
    args = parser.parse_args()
    try:
        report = audit_pe(args.exe.resolve(), args.objdump)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"PE audit failed: {error}", file=sys.stderr)
        return 2
    print(json.dumps(report, ensure_ascii=False, indent=2))
    return 1 if report["structural_errors"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
