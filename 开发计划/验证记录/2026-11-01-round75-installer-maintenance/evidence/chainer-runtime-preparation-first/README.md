# Prepared independent chainer runtime controller

Root alone may execute `run-quiet-chainer-first.py --execute`. No Windows or
network operation has been performed by this preparation. It does not modify
Rust/Cargo, production Pending/gate/Run, system policies or original evidence.

Required explicit inputs are generated spec+SHA, actual final MSI+SHA, actual
static check JSON+SHA, and a new evidence report path. `--execute` is the only
mutation switch. Without it the same exact local identities/static qualifications
are checked and only PREPARED is printed. It does not create the report or Windows
root. Do not pass `build-operation.json` as the static report: use the original
`07-static-check.stdout.log` emitted by the closed chainer checker.

```text
python3 -B E/chainer-runtime-preparation-first/run-quiet-chainer-first.py
 --spec ROOT/chainer-probe-input-bd0f287d596c4a01bc2a120c3670afb8/spec.json
 --spec-sha256 da135548b895d0647e001f4e75b7107b4ec08c40832c8dbc37841b638fa115a0
 --msi E/chainer-build-first/chainer-probe-final.msi
 --msi-sha256 8b3bbf3c45e14751268a2963c80ef2c13b48688d84052f3e04ffebaca8670590
 --static-report E/chainer-build-first/07-static-check.stdout.log
 --static-report-sha256 <actualSHA>
 --report E/quiet-chainer-first.json
 [--execute]
```

ROOT/E above denote the actual absolute R75 owned root/evidence paths, not literal
shell variables. Compiler/helper identity is derived from the exact pinned static
report, not an assumed current Rust/N2/N3 hash. Independent C# fixture is unaffected
by separately authorized Rust engine-policy edits. All original inputs are hashed
again after the run.

The SSH bootstrap is short UTF16 encoded PowerShell which consumes the entire
actual script as UTF8 stdin. No long-script command line or accidental echoing of
stdin is used. The controller never reads the SSH private key: it supplies the
already authorized external key pathname to OpenSSH only. Connected user and
report-root owner must match the spec. The fresh root is under exact caller TEMP,
with explicit SetOwner(user SID), protected user+SYSTEM-only DACL, exact LF marker
and reparse rejection. Helper own SID may be SYSTEM and is never used to change
root ownership or production permissions.

Before/after ProductsEx uses actual COM Count reflection and contexts7 with empty
SID argument (current user's per-user contexts plus machine). RelatedProducts
Count must also be zero. Other users and installer cache are not enumerated or
deleted by this controller; root's separate final cache/read-only audit remains
required. Any preexisting nonce product/related registration rejects before root
creation/MSI execution.

One monotonic120-second controller budget includes a reserved20 seconds for
cleanup. Calls receive remaining original time; no retry or renewal. Hidden quiet
msiexec has no expected exit code: actual integer client exit is recorded. A
scoped Add-Type helper supplies only process query/sync/exit and bounded negative
termination APIs; its compiler TEMP/TMP is `runtime-temp` inside this owned root,
then environment is restored. No product dependency or new normal binary is added.

During MSI execution, the controller polls create-new `pre-join.json`. It attempts
to hold the exact helper object with limited query+synchronize, verifies actual
creation FILETIME/image against source-bound pre-join and actual image SHA against
the compiled fixture, then reads actual exit code from that held object. It never
treats JSON releaseProof or later process disappearance as an exit-code API result.
Because the unmodified helper can finish before polling catches it, this may yield
UNKNOWN even if Join/End JSON shows0. The complete original verbose log and bounded
chainer-related lines are preserved for independent root review; this version
does not invent a generalized chainer-exit regex. Unknown is not silently PASS.

For a stuck helper, only an already verified exact original object may trigger
negative owner-death cleanup: reopen with terminate/query/sync, recheck creation,
image and compiled image SHA, terminate that object once, close checked once, and
give original MSI client a bounded chance to return. No msiexec/service/client
kill or process-name kill exists. Any such attempt disqualifies API success even
when rollback/cleanup later succeeds. Unknown identity, access denied, unknown
close or still-live objects retain failure. No synchronization barrier or source
change is added to the frozen C# helper.

The Python `finally` path independently queries registration; if necessary it
performs one exact ProductCode formal quiet `/x` within remaining original time.
The statically checked REMOVE=ALL exclusion prevents that `/x` from rerunning the
chainer. It preserves formal uninstall exit plus after product+related absence,
original helper/client disappearance and no new msiexec instances beyond baseline.
No direct registration/cache deletion or bypass transform is available.

Original pre-join/result JSON, incomplete `.tmp` reports and MSI logs are downloaded
as actual bytes with base64, length and SHA verified. Only the owned runtime-temp
compiler cache has identity-only retention; its original controller C# text stays
in the recorded stdin. Cleanup rechecks owner/marker, registration, process instances,
complete exact member hashes and download acknowledgment before removing only
this new marked scratch root. Unknown live state/membership or exhausted budget
retains it for root's separately recorded recovery; no blind deletion.

PASS requires all of Join0, EndRollback0, one successful returned-event close and
all controls closed, actual **held** helper exit0 with exact instance+image SHA,
no owner-death attempt, original client actual terminal integer exit, both instances
gone, product+related absence and exact scratch cleanup. This is independent API
fixture evidence only, not Rust product guardian, original Begin session binding,
Windows7/10 or normal product install/repair/uninstall acceptance.
