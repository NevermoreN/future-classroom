# Independent embedded-chainer experiment

This directory prepares a test fixture. No C# compilation, PE/MSI production,
Windows command, transaction or product install has run here. The C# program is
a .NET Framework x86 GUI executable for this experiment only. It is not a Rust
product binary, does not add a product dependency and cannot exercise the Rust
broker, Pending, gate, effect reconciliation or permissions implementation.

The first question is whether a genuine engine-launched embedded chainer runs
as the installing user, which actual process is its parent, which argv the
engine supplies, and whether the supplied transaction can really Join and End
with ROLLBACK. It does not prove Begin-to-guardian transaction binding, resistance
to arbitrary other MSIs/transforms, Win7/Win10 behavior or product acceptance.
The controlled candidate's mandatory Begin+gate remains a separate protection.

## Inputs and fixed constants

Root chooses a fresh 32-lowercase-hex nonce and an entirely absent ASCII report
directory, for example a child of its existing user TEMP, with exact basename
`FutureClassroom-r75-chainer-probe-<nonce>`. No operational nonce is supplied by
this preparation. The report root must have no reparse ancestors, no aliases,
ADS or MSI property format characters and fit the tool's length bounds.

Root obtains its actual current user's SID, rather than copying the service SID
from an earlier observation. The `prepare` command writes only a **new** owned
local output directory:

```text
python3 -B chainer-probe.py prepare --nonce <nonce> --windows-report-root <absoluteWindowsRoot> --owner-sid <actualUserSID> --output-dir <freshLocalOutput>
```

Outputs: `ChainerProbe.cs`, `probe.wxs`, `MsiEmbeddedChainer.idt`, `spec.json`,
`.owner`. The spec includes exact generated source/template/WXS/IDT identities
and independent nonce-derived ProductCode, UpgradeCode and PackageCode.

The constants in generated C# are nonce, root and expected **directory owner**.
ExpectedOwner is never compared with the helper's own process/token SID. A
SYSTEM helper can report that fact and attempt Join; actual MSI's result decides.
No helper user identity or privilege is manufactured by the fixture.

Root must create the new report directory with an explicit `DirectorySecurity`:
`SetOwner(new SecurityIdentifier(actualCurrentUserSID))`, protected DACL, and
FullControl allow rules for only that SID and `S-1-5-18`. Do not accept an
Administrators owner just because the directory was created from an admin SSH
session. Read back owner and DACL and record them. The `.owner` file must be
create-new ASCII containing exactly `future-classroom-owned-r75-chainer-probe/v1`
plus a single LF. Do not insert a UTF-8 BOM or literal backslash-n.

The program pins at most 16 path components and marker, rejects reparse objects,
verifies root owner/DACL/marker and preexisting report absence. It writes only
two fixed bounded reports under this root, via create-new temp + no-overwrite
rename: `pre-join.json` (identity facts before native Join) and `result.json`
(Join/End/checked-close results). Both are at most 8192 bytes. A crash may leave
its corresponding `.tmp`; preserve that as incomplete evidence, never overwrite.

## Compile-only phase, executed only by root

Use the existing 32-bit .NET Framework v4 compiler. Root/UI's separate
`chainer-compile-preparation-first` controller owns compilation and transfer.
The exact compiler argument array is:

```text
[<Windows>\Microsoft.NET\Framework\v4.0.30319\csc.exe,
 /nologo, /target:winexe, /platform:x86, /optimize+, /warnaserror+,
 /out:<exactOwnedOutputExe>, <exactOwnedInput>\ChainerProbe.cs]
```

No generated executable is run in this phase. Record actual compiler image SHA,
argv, stdout/stderr and actual exit; source SHA/bytes before and after; resulting
EXE SHA/bytes. `compile_receipt` accepted by the checker must include:

```json
{"status":"PASS_CSC_ACTUAL_EXIT0","actualExit":true,"exitCode":0,
 "sourceBefore":{"bytes":0,"sha256":"replaceWithActual"},
 "sourceAfter":{"bytes":0,"sha256":"replaceWithActual"},
 "sourceUnchanged":true,"exe":{"bytes":0,"sha256":"replaceWithActual"},
 "nonce":"replaceWithActual","commandArgs":[],"compilerSha256":"replaceWithActual"}
```

These are schema examples only, never an execution receipt. The explicit receipt
SHA is root-reviewed. Checker's PE validation requires I386 PE32 GUI subsystem2,
CLR header, ILONLY+32BITREQUIRED, no prefer32/AnyCPU fallback. A compilation error
or warning-as-error is retained; no blanket suppression is authorized.

## Isolated MSI authoring and actual final-table checks

Copy the exact compiled EXE as `chainer-probe.exe` beside generated authoring.
Run the already pinned wixl tool to create `probe.msi`, then pinned msibuild to
import the generated `MsiEmbeddedChainer.idt`. Record **each actual tool command**,
stdout/stderr, terminal exit and before/after MSI SHA. The complete final MSI is
separately SHA-pinned before any Windows transfer. This is an ordinary per-user
x86 MSI500 with empty Feature, one Binary and one embedded chainer. It contains
no File, Component, Registry, Upgrade, CustomAction or other installed payload.
Standard MSI product/cache registration itself is a real effect; it must be
rolled back or formally uninstalled, never classified as a read-only observer.

The embedded row is Type2, Source FcIndependentChainer, fixed authored command
`probe <nonce> <root>` with the standard Windows argv quoting. MSI prepends the
transaction handle, so the executable requires exactly
`[nonzeroUint32Txn, probe, compiledNonce, compiledRoot]`.
The row runs only when not REMOVE=ALL and not ACTION=ADVERTISE. The cleanup /x
therefore never launches the probe again. These conditions are fixture wiring,
not production authorization. Only root's quiet `/i` route is permitted.

Export original complete `msiinfo tables` stdout, original `msiinfo suminfo`,
every non-system table including exact virtual `_ForceCodepage` and the exact
Binary.FcIndependentChainer stream. The checker takes these files, original
compiler receipt, final MSI and generated spec with explicit SHA arguments:

```text
python3 -B chainer-probe.py check-exports --spec <spec.json> --spec-sha256 <sha>
 --compile-receipt <actual.json> --compile-receipt-sha256 <sha>
 --tables-list <actualTablesStdout> --exports <allOriginalIdtDirectory>
 --extracted-probe <actualBinaryStream> --msi <actualFinalMsi> --msi-sha256 <sha>
 --summary <actualSuminfoStdout>
```

Root must bind export/extraction command receipts to this same final MSI hash;
the checker cannot independently infer where caller-provided exports came from.
It prints exact final MSI, EXE, source/receipt, table-list, all export and summary
identities. It does not execute the MSI. Unknown nonempty tables or action rows,
registry/file/components, extra Binary/chainer/CA, changed command/conditions,
wrong product identity, wrong codepage or auxiliary actions fail closed.
Only the exact standard AdminExecuteSequence InstallAdminPackage3900 exception
is admitted; it does not authorize `/a` execution. Neutral `_ForceCodepage`
must match actual raw `CRLF CRLF 0<TAB>_ForceCodepage CRLF NUL` and is metadata,
not a general ordinary-IDT NUL exception. Media/Error must be empty.

## Root-only quiet runtime and formal cleanup

Before actual runtime, independently record exact nonce Product/Upgrade absent
in current user and machine contexts; expected root is fresh; no test helper or
owned MSI client exists. Preserve existing user Run/SSH/school-data baseline.
No real product path, production lifecycle root, Run value or school file is
an input to this fixture. Do not turn its unrelated nonce registrations into
product MSI acceptance.

Start one **original 120-second wall-clock budget**, not a fresh budget per call,
for actual `/i`, helper return/engine rollback observations, any necessary formal
`/x`, and absence checks. Invoke existing msiexec using `UseShellExecute=false`,
`CreateNoWindow=true`, quiet `/qn /norestart`, `REBOOT=ReallySuppress`, fixed
DisableShutdown and a verbose log under this exact owned root. No full UI,
scheduled task, service changes, process-name kills or additional retry.

Use `try/finally` around installation regardless of exit/failure. Preserve the
actual client PID+creation, terminal exit and verbose log. Read the two fixed
reports without rewriting them. Join0/EndRollback0/checked-close plus original
probe process exit0 are independent facts from msiexec's exit code; preserve
the raw MSI exit rather than guessing that rollback implies install-success0.
A failure/missing report, unexpected token, ACCESS_DENIED or unknown Join/End
stays a failure/unknown; a cleanup pass never upgrades it to API success.

In `finally`, query the exact nonce ProductCode in all applicable registration
contexts. If the fixture registered, perform one **formal** quiet `/x` for that
exact ProductCode and validate terminal return plus original registration and
related-product absence. Check enough remaining original budget before each
bounded wait. If time is exhausted or context/ownership is unknown, record
cleanup FAILED/UNKNOWN and retain owned root/evidence for root's separately
authorized recovery. Do not renew 120 seconds, delete registrations, erase
Installer cache files or use a transform/changed CA to force cleanup.

Native Join/End are synchronous. The C# checks one monotonic120s clock before
Join and after native calls; it does not claim it can cancel a native call that
does not return. `pre-join.json` is available before that boundary. Root may
open the **exact** observed original helper process with synchronize/query
and, only for timeout recovery, the necessary terminate right. Before termination,
recheck held process object's PID+creation, image and exact expected fixture PE
SHA against its source-bound pre-join identity. A JSON PID alone is not authority.
Only that original test helper can be terminated, never msiexec/service/client
by process name. Record it as negative owner-death rollback, not Join/End success
or product Pending return. Reap held helper/client objects, then independently
verify engine rollback and fixture registration/cache/owned-root cleanup. If
the exact helper object cannot be established, retain failure for root recovery.

Only after all fixture registrations, related records, processes and temporary
cache images are proven absent may root remove this new marked scratch root.
Preserve original source, PE SHA/identity, tool/export receipts, pre-join/result,
MSI logs/exits and all cleanup reports in evidence first. Do not remove user's
SSH service, public-key authorization or test private keys. This preparation
does not implement the root Windows controller or execute any of these actions.
