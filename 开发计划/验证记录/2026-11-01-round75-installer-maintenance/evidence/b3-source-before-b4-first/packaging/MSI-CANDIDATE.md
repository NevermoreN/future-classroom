# MSI candidate — not a product release

This package is an unvalidated P5-03 build/inspection candidate. P5-01, P5-02,
P5-06 and the full P5-03/Q13 acceptance gates remain incomplete. A successful
build, static table inspection or extraction does not establish installation,
upgrade, rollback, uninstallation or operating-system compatibility.

The candidate currently accepts only the `i686-win7-windows-msvc` development
ZIP produced by `scripts/package-probe.py`, including the complete verified Web
manifest, assets and notices. Native-only ZIPs are rejected. This is an input-scope limit, not a
removal of the formal Windows 7 SP1 x86, Windows 10 and Windows 11 targets.
The two normal application/helper binaries require GUI subsystem 2; test
harnesses remain CUI subsystem 3. The lifecycle helper is a Binary-table stream,
not an installed application file. The original ZIP manifest, notices
and exact Web resource bytes are retained; no development server is installed.

## Deployment decision for this candidate

The installation context is **per-user**, at the fixed path
`LocalAppData/Programs/FutureClassroom`, for the selected deployment Windows
account. This candidate decision matches the existing SID-bound protected data,
current-user DPAPI and HKCU startup settings. It is not a new formal deployment
default or an all-users installation. Machine scope is rejected. The authored
installation directory uses the private mixed-case identifier `FcInstallDir`,
with no public installation-path property or path-selection UI. Standard Known
Folders are resolved by MSI rather than hard-coded to C:. This does not defend
against an installer/transform modified by the same user or an administrator.

Application data remains in the original Known Folder ProgramData project tree;
it is not moved into LocalAppData. No ProgramData directory, database,
deployment marker, administrator password, DPAPI key or ACL is created by MSI.
After installation the deployment account must explicitly launch the application
normally. Existing ACL/marker/instance-lock checks remain authoritative. A
different account or incomplete deployment must not silently initialize or
rebind school data. The installer never launches the application.

Only minimal MSI-owned HKCU component markers are authored under
`Software\FutureClassroom\Installer\Components`, one integer value per stable
component GUID. These registry KeyPaths follow the Windows Installer ICE38 rule
for components installed in a user profile; the application files are not their
KeyPaths. No actual registry change occurs during Linux package generation.
Standard component removal of these installer markers is not Run-value cleanup.

No Run value, service, scheduled task, firewall rule, certificate/trust store,
HTTP binding or listener is created or deleted. No application-data cleanup or
wildcard file deletion is authored. Uninstall success requires independent
absence of the fixed program tree, installer markers and product/component
registration; deleting program files alone is insufficient. The separate
installer coordination gate is not school data or an MSI payload, and its safe
retirement is not claimed by this slice.

## Lifecycle candidate boundary

The first synchronous execute action, `FcBeginGate`, runs the closed helper
`begin "[FcInstallDir]fc-bootstrap.exe"` role after `CostFinalize`/`InstallValidate` and
before `InstallInitialize` or any product mutation. This avoids the documented
full-UI client/server repeat of pre-InstallValidate actions. Its resolved main-file path
is an untrusted fact compared only against the native fixed Known Folder root
plus the fixed fc-bootstrap.exe filename;
it never selects or authorizes a caller path. The
helper composes the same current-SID/install-family gate used by ordinary,
autostart and application repair. Its broker retains the exclusive owner and
durable Pending through the original transaction and process/effects return.
This is direct MSI wiring; there is no outer setup wrapper or public approval
property. MSI launch conditions are additional early rejection, never identity
or permission authority.

The exact `DisableShutdown` property and pre-costing launch condition reject an
override enabling automatic Restart Manager shutdown. `InstallValidate` can
still detect occupied files; it does not replace the native gate. Full-UI,
basic-UI and quiet execution remain separate runtime checks.

This slice implements candidate paths for fresh per-user installation, healthy
same-manifest/version reapplication, formal MSI uninstall and known rollback.
These are implementation claims; actual MSI acceptance remains **NOT_RUN**.
The scope is the controlled, same-source original candidate without transforms.
Offline closed-table inspection is not runtime authentication of arbitrary
effective transformed sessions or control over unrelated external MSI packages.

| Path | Admission and required final effect | Runtime evidence |
| --- | --- | --- |
| Fresh install | Product/family, fixed target and markers absent; final exact manifest present | NOT_RUN |
| Healthy same-version reapplication | One healthy same-manifest current-user installation; final exact manifest present | NOT_RUN |
| Formal uninstall | Same healthy baseline; final product/family, fixed target and markers absent | NOT_RUN |
| Known rollback | Actual transaction rollback; fresh absence or original healthy manifest restored | NOT_RUN |
| Damaged repair, different manifest/version or major upgrade | Unsupported; reject before product mutation | NOT_VALIDATED |
| Run cleanup | Not implemented; no Run removal authored | NOT_VALIDATED |

Admission is based on actual product/context/related registrations, version,
component keypaths, bounded exact payload hashes/membership and marker state.
It is checked again under the original exclusive owner. The internal
`SameManifest` Pending label is neutral: `Installed`, `REMOVE`, `REINSTALL`,
command-line intent and advisory notices do not grant an action. A full staged
manifest permits the expected-present path; complete target/marker absence from
a healthy admitted baseline permits expected-absent. Partial or unknown effects
require rollback and verification against the admitted baseline. Final product
registration is independently checked after the actual transaction ends and
the original guardian returns; it is not inferred from staged registration
visibility. File-proof handles must actually return before MSI needs delete
access. The original 120-second deadline and resource/worker budgets do not grow.

An intact installation being reapplied is not evidence of repairing missing or
corrupt files. A damaged or foreign preimage is refused rather than silently
adopted. The fixed project HKCU Run value must be absent: any existing value,
including an apparently matching command or a different value type, is refused
without changing it. No application startup, Run write/delete, school database
or credential change is part of this candidate. Ordinary marker removal is not
an ownership-safe Run cleanup substitute.

The exact Binary table contains one separately compiled normal
`fc-installer-lifecycle` image. The packager first binds it and the ordinary
application to one accepted same-source CI result, original non-test Cargo
messages and source/byte ledgers. It appends a bounded immutable payload/effects
manifest to that original helper PE, preserving the normal PE prefix byte for
byte. The embedded guardian receives the engine transaction ID followed by the
fixed `guardian` argument; it is not a broker-created imitation of an engine
child. The native implementation authenticates actual peer/process capabilities
and reads the manifest from its pinned image. A caller cannot send an
`effects_reconciled` boolean as proof.

Only three exact CustomAction rows are admitted: synchronous immediate begin
(type 2), impersonated deferred rollback notification (1282), and impersonated
deferred commit notification (1538). Notifications are advisory. Successful
Join/End, an ACK or pipe EOF alone cannot release Pending: the broker also needs
the original guardian's actual return and independently reconciled closed
payload/registration effects or the admitted fresh/healthy rollback baseline. Failure,
unknown effects or cleanup retains negative state. No ignore/async or
NoImpersonate variant is admitted.

`RemoveExistingProducts` follows `InstallExecute` and precedes
`InstallFinalize`. Native admission still refuses different-version/related
products before this candidate mutates files; its transactional location is not evidence
of implemented upgrade handoff or rollback. ProductCode remains deterministic
per version, and the existing family/component identities stay stable.

Actual fresh-install, healthy reapplication, formal uninstall, known rollback,
active-app refusal, process return and residual cleanup must still be
demonstrated on an owned Windows fixture. Only genuine MSI removal counts;
manual cache/registration deletion, skipped hooks or transforms cannot create
an uninstall PASS. Existing MSI File/Directory authoring is retained: installer-
created directories not explicitly listed in CreateFolder are documented to be
removed when empty; actual nested-root absence is still measured. No speculative
CreateFolder/RemoveFile or wildcard cleanup is added. Real
Win7/Win10/Win11, classroom hardware, damaged repair/upgrade, strict Run cleanup,
standard-user handover, reboot/power-loss, signing and full ICE/release acceptance
remain separate and unvalidated. `DisableShutdown` alone is not a process guard.
The current parent-process SID/context assumption also needs a separate
test-only observer before invoking production Begin. An observer's success does
not prove broker/guardian success, and must not create production Pending.

## Build and inspect

Use reviewed Ubuntu noble tools `wixl`, `wixl-data`, `msitools` and
`libmsi-1.0-0`, version `0.103+repack-1build3`, extracted into an owned directory.
Check official package hashes and dependencies first. Do not install tools
globally or replace system libraries. If required, set `LD_LIBRARY_PATH` only
for the single packaging subprocess. The script accepts only explicit absolute
tool paths and records their actual hashes, version outputs and command logs.
The explicit `msibuild` SHA is required and is bound to the reviewed
0.103+repack-1build3 tool package (upstream msibuild has no `--version` option).
Its reviewed importer authors the
unsupported-in-wixl `MsiEmbeddedChainer` table, which is then exported and checked.

```sh
python3 scripts/package-msi-candidate.py \
  --input-zip /owned/input.zip --input-sha256 EXPECTED_SHA256 \
  --lifecycle-helper /owned/ci/product/fc-bootstrap/fc-installer-lifecycle.exe \
  --lifecycle-helper-sha256 EXPECTED_NORMAL_HELPER_SHA256 \
  --build-evidence /owned/ci --build-result-sha256 EXPECTED_CI_RESULT_SHA256 \
  --output-dir /owned/new-output \
  --wixl /owned/tools/usr/bin/wixl \
  --msibuild /owned/tools/usr/bin/msibuild --msibuild-sha256 EXPECTED_TOOL_SHA256 \
  --msiinfo /owned/tools/usr/bin/msiinfo \
  --msiextract /owned/tools/usr/bin/msiextract \
  --objdump /usr/bin/x86_64-linux-gnu-objdump
```

Output is exclusive: existing destinations, directory aliases and reparse points
are refused. Inputs are bounded and admitted by a closed ZIP/manifest namespace;
all payload size/SHA/CRC, licenses, Web versions and exact membership are checked.
The ZIP claim alone is insufficient: both normal executables must match the
accepted original CI artifacts and current source input ledger. A helper test
EXE, other source/version, third normal target or changed compiler log is rejected.
Generated `candidate.wxs`, original payloads, MSI, table exports, extraction and
complete command stdout/stderr remain in the owned output for review. Failures
retain their evidence. Tool commands have a 120-second deadline and 16-MiB
stdout/stderr budgets. Cleanup is the caller's explicit responsibility after
review; never delete school data, persistent SSH authorization or unrelated files.

Before any release, independently examine summary architecture/installer version,
Property, Directory, Component, File, Upgrade, LaunchCondition, both sequences,
the exact installer-only Registry marker set, the single helper Binary stream,
the exact three CustomAction rows and one embedded chainer. Extra roles, binary
streams, bypass conditions and altered sequencing fail inspection.
RemoveRegistry/service/shortcut/environment/ACL side effects remain forbidden.
The initial candidate does not perform Windows Installer ICE validation. The
Linux pure input/authoring tests are not a Windows Installer runtime simulator.

Sources: [GNOME v0.103 implementation](https://github.com/GNOME/msitools/tree/v0.103),
[Windows Installer 5.0/Win7](https://learn.microsoft.com/en-us/windows/win32/msi/released-versions-of-windows-installer),
[installation context](https://learn.microsoft.com/en-us/windows/win32/msi/installation-context),
[directory costing](https://learn.microsoft.com/en-us/windows/win32/msi/using-a-directory-property-in-a-path),
[client/server custom-action sequencing](https://learn.microsoft.com/en-us/windows/win32/msi/sequencing-custom-actions),
[Restart Manager property](https://learn.microsoft.com/en-us/windows/win32/msi/msirestartmanagercontrol),
[installer-created directory removal](https://learn.microsoft.com/en-us/windows/win32/msi/createfolder-table),
[Type2 command-line context](https://learn.microsoft.com/en-us/windows/win32/msi/custom-action-type-2).
Tool licenses and dependencies must be recorded separately from the application's
notices; extracting a build tool does not make it an installed application payload.
