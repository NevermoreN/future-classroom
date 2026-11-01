# MSI candidate — not a product release

This package is an unvalidated P5-03 build/inspection candidate. P5-01, P5-02,
P5-06 and the full P5-03/Q13 acceptance gates remain incomplete. A successful
build, static table inspection or extraction does not establish installation,
upgrade, rollback, uninstallation or operating-system compatibility.

The candidate currently accepts only the `i686-win7-windows-msvc` development
ZIP produced by `scripts/package-probe.py`, including the complete verified Web
manifest, assets and notices. Native-only ZIPs are rejected. This is an input-scope limit, not a
removal of the formal Windows 7 SP1 x86, Windows 10 and Windows 11 targets.
The package remains a CUI development build. The original ZIP manifest, notices
and exact Web resource bytes are retained; no development server is installed.

## Deployment decision for this candidate

The installation context is **per-user**, at the fixed path
`LocalAppData/Programs/FutureClassroom`, for the selected deployment Windows
account. This candidate decision matches the existing SID-bound protected data,
current-user DPAPI and HKCU startup settings. It is not a new formal deployment
default or an all-users installation. Machine scope is rejected.

Application data remains in the original Known Folder ProgramData project tree;
it is not moved into LocalAppData. No ProgramData directory, database,
deployment marker, administrator password, DPAPI key or ACL is created by MSI.
After installation the deployment account must explicitly launch the application
normally. Existing ACL/marker/instance-lock checks remain authoritative. A
different account or incomplete deployment must not silently initialize or
rebind school data. The installer never launches the application.

No Run value, service, scheduled task, firewall rule, certificate/trust store,
HTTP binding or listener is created or deleted. No application-data cleanup or
wildcard file deletion is authored. Removing installed program files does not
constitute complete product uninstall: ownership-aware cleanup remains pending.

## Incomplete lifecycle protections

* Active-process upgrade/uninstall refusal is not implemented. Do not install,
  upgrade, repair or uninstall this candidate on an active classroom computer.
  `MSIRESTARTMANAGERCONTROL=DisableShutdown` is not a fail-closed process guard;
  FilesInUse/reboot behavior is not a substitute for that guard.
* Exact type/value/ownership-checked cleanup of the deployment account's HKCU Run
  value is not implemented. Standard registry removal cannot safely replace it.
* The wixl 0.103 MajorUpgrade helper defaults to removal before InstallInitialize.
  This candidate explicitly sequences RemoveExistingProducts after
  InstallInitialize and verifies its exported sequence before InstallFinalize.
  Transactional authoring does not establish actual Windows upgrade rollback or
  failure preservation: those runtime checks remain NOT_RUN/NOT_VALIDATED.
* The first three MSI version fields determine upgrades. ProductCode changes
  deterministically with that version; UpgradeCode and unchanged file-component
  identities remain stable. Same-version upgrades and downgrades are not enabled.
* Actual Win7/Win10/Win11 install/repair/upgrade/downgrade/uninstall, ordinary
  standard-user startup, Run reconciliation, cross-account handover, Seewo and
  power-loss acceptance are NOT_RUN. Do not infer them from static PE/MSI checks.

Any future isolated installation experiment must explicitly acknowledge
`FC_CANDIDATE_ACK=I_UNDERSTAND_NOT_VALIDATED`; this is only an acknowledgement,
not a security boundary, lifecycle protection or permission to use school data.
No installation experiment is performed by the packaging script.

## Build and inspect

Use reviewed Ubuntu noble tools `wixl`, `wixl-data`, `msitools` and
`libmsi-1.0-0`, version `0.103+repack-1build3`, extracted into an owned directory.
Check official package hashes and dependencies first. Do not install tools
globally or replace system libraries. If required, set `LD_LIBRARY_PATH` only
for the single packaging subprocess. The script accepts only explicit absolute
tool paths and records their actual hashes, version outputs and command logs.

```sh
python3 scripts/package-msi-candidate.py \
  --input-zip /owned/input.zip --input-sha256 EXPECTED_SHA256 \
  --output-dir /owned/new-output \
  --wixl /owned/tools/usr/bin/wixl \
  --msiinfo /owned/tools/usr/bin/msiinfo \
  --msiextract /owned/tools/usr/bin/msiextract \
  --objdump /usr/bin/x86_64-linux-gnu-objdump \
  --acknowledge-unvalidated-candidate
```

Output is exclusive: existing destinations, directory aliases and reparse points
are refused. Inputs are bounded and admitted by a closed ZIP/manifest namespace;
all payload size/SHA/CRC, licenses, Web versions and exact membership are checked.
The source ZIP's claimed compiler provenance is not independently certified.
Generated `candidate.wxs`, original payloads, MSI, table exports, extraction and
complete command stdout/stderr remain in the owned output for review. Failures
retain their evidence. Tool commands have a 120-second deadline and 16-MiB
stdout/stderr budgets. Cleanup is the caller's explicit responsibility after
review; never delete school data, persistent SSH authorization or unrelated files.

Before any release, independently examine summary architecture/installer version,
Property, Directory, Component, File, Upgrade, LaunchCondition, both sequences,
and the absence of Registry/RemoveRegistry/CustomAction/service side effects.
The initial candidate does not perform Windows Installer ICE validation. The
Linux pure input/authoring tests are not a Windows Installer runtime simulator.

Sources: [GNOME v0.103 implementation](https://github.com/GNOME/msitools/tree/v0.103),
[Windows Installer 5.0/Win7](https://learn.microsoft.com/en-us/windows/win32/msi/released-versions-of-windows-installer),
[installation context](https://learn.microsoft.com/en-us/windows/win32/msi/installation-context),
[Restart Manager property](https://learn.microsoft.com/en-us/windows/win32/msi/msirestartmanagercontrol).
Tool licenses and dependencies must be recorded separately from the application's
notices; extracting a build tool does not make it an installed application payload.
