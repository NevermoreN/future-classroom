# Controlled Setup candidate

This bundle contains Setup.exe and the fixed sibling FutureClassroom.msi. Keep
both complete files together. It is not a validated release or a general MSI
wrapper. The normal product, helper and guard DLL must be non-test artifacts
from the admitted same-source compiler inputs. The complete payload ledger
also binds the Web build, license texts and reviewed instructions. GNU DEV
fixtures remain separate
from the original Win7-target MSVC CI admission; modern MSVC compilation is an
additional check. Compiling a target does not establish runtime compatibility.

Version 0.1.5 implements unified Web management and the classroom desktop corrections. Its exact compiler source,
CI run, package hashes and actual installation results are recorded separately
in the delivered receipt. This document does not certify every deployment
environment or change the behavior of previously delivered bundles.

For D12, start the application from the desktop/Start menu shortcut, or keep the
complete direct-run directory and its adjacent web folder. Tray View Status and
Settings open the actual loopback Web address. Login uses username/password;
refresh, reconnect and ordinary idle retain the open page's session. Normal tab
close/new open uses standard sessionStorage semantics and requires login again.
Closing the browser does not stop the resident desktop. Its nonactivating information windows remain below ordinary application windows and above the Shell desktop. Taskbar focus does not hide them; maximized and full-screen applications cover them naturally.
Normal application exit is the tray Exit action.

Layout editing uses plain-language item/group labels. Timetable columns align,
each lesson has a visible left-side delete button, and time fields keep the
HH:MM shape while accepting digits only.

Application startup separately prepares its installation-owned IP-SAN certificate
and CurrentUser trust for the local control endpoint. The teacher endpoint
automatically prepares its eligible private-IPv4 HTTPS configuration at startup;
teacher devices must trust the selected certificate or chain. Saved custom
source restrictions remain in force. LAN may use another eligible CurrentUser
MY server certificate without replacing the local certificate. Private keys are
not distributed. D14 separately prepares only this program's restricted update
ingress rule where Windows policy and the current token allow it; otherwise
sharing stays visibly restricted until an administrator prepares the LAN rules.

Setup expresses only fixed install/repair/remove intent. No command line path,
caller hash, PID, transaction number or success claim authorizes installation.
The original B/G association, actual MSI Type1 session and checked native return
path remain required. Direct inner install/repair/remove has no such grant.

The ABI2 inner Binary table has three fixed streams: the admitted Rust guard
DLL, the canonical zero32 client template and FcPayloadManifest. A qualified
upgrade adds only the fixed FcPublishedPredecessor fourth stream. Capability3
binds each version's own normal helper/template/DLL, including a four-stream
predecessor, without recursively executing its older publication. Earlier
capability0 internal bundles need a compatible installer; their data is retained.
The manifest
binds the actual compiler-generated PackageCode, product/version and complete
payload identities. The final MSI is completed first; only the external
Setup's fixed32 digest slot is filled afterwards. Its normal compiler prefix
stays byte-for-byte unchanged. Neither the embedded template nor the DLL hashes
its enclosing MSI. Legacy FCLM73 overlay candidates use a different format and
must not be mixed with ABI2 products.

Do not install over an old version unless the successor explicitly carries an
accepted predecessor record matching that deployment. Delivered 0.1.1/0.1.2
FCLM73 bundles and the three-stream 0.1.3 bundle do not establish that permission
for the 0.1.5 package. The delivered three-stream 0.1.4 package also does not
establish a qualified predecessor record for this package. Without it, disable the old login startup, exit
the old app, and uninstall using that version's complete matching Setup/MSI pair.
Default uninstall preserves school data. Then install the newly delivered full
pair and verify the original accounts, timetable, layouts and materials. Do not
mix versions' Setup/MSI files, overwrite an installed directory, delete the
database, or use data reset as an upgrade step.

Setup Repair restores installer-owned files. Application data reset is a separate
destructive operation: use authenticated loopback Web maintenance, or first exit
the app and explicitly run fc-bootstrap.exe --repair for the OS-authorized Web
repair process. Review the deletion scope and new-identity impact before
confirmation. This clears deployment data, including internal exports, while
preserving externally saved copies and Windows settings; restart then initializes
a new school/admin identity. Missing old credentials do not open registration or
grant ordinary unauthenticated pages administrator access.

Controlled joint install, damaged repair, failure rollback and same-bundle
source REMOVE=ALL still require actual evidence. Static build/inspection PASS
is not runtime PASS. Cache/source identity remains independently bounded; no
property substitutes for it. Standard Control Panel/ARP uninstall and recovery
when this external bundle is moved/deleted are not supported by this slice.
Removing or disabling the guard is not an uninstall solution.

School business data, certificates, URLACL and firewall are outside this MSI
payload's write/remove scope. Default uninstall preserves school data. Upgrade
and repair preserve an exact owned installed Run value. Closed Setup removal
deletes only that installation's exact value. Its original maintenance owner
retains the preimage; a known rollback restores the original bytes only into an
absent value, after the original transaction and guardian return. Observed
external values are preserved. Adjacent registry reads/writes are not CAS against
a Windows user simultaneously editing the same value. Standard-user deployment
and complete failure acceptance remain independent checks.

Windows 7 and Windows 10 physical testing is not scheduled, at the user's
request. Their compatibility design and build checks remain; runtime status is
NOT_RUN, and neither physical test is a current release prerequisite. Win11
product behavior, Seewo, full P5/Q13 and release readiness still require their
own evidence. Persistent test SSH authorization is preserved.

The formal ABI2 producer is scripts/package-abi2-msvc-candidate.py. It is inert
unless --execute is supplied and requires the original input ZIP, normal GUI,
helper and guard CI evidence, accepted SHA256 values, exact source checkout and
explicit packaging tools. It admits only successful original Win7-target MSVC
CI artifacts and records reviewed builder/instructions separately from those
compiled sources. Static packaging does not run the product or publish a
release. The GNU DEV producers retain their own provenance rules.

## D14 coordinator and current formal build

Starting with version 0.1.5, the formal Win7-target MSVC build must contain three
normal binaries: `fc-bootstrap`, `fc-installer-lifecycle`, and
`fc-update-coordinator`. `scripts/windows-msvc-ci.ps1` captures each original
Cargo JSON stream and all three exact EXEs in the same immutable artifact
ledger. `package-probe.py --target i686-win7-windows-msvc` requires an explicit
`--update-coordinator` from that run. Its complete `manifest.json` file list
binds the coordinator; the MSI payload ledger and exact extraction check include
it too. A copied GNU Showcase EXE or a separately rebuilt coordinator fails
formal source admission. Older version <=0.1.4 sources retain their original
two-binary admission so an old qualified predecessor can still be replayed.

The complete D14 MSI without tutorial images has twenty payload files: the coordinator plus
the original payload set and all five fixed PWA files (`manifest.webmanifest`,
`sw.js`, `offline.html`, `pwa-icon-192.png`, `pwa-icon-512.png` under `web/`).
The five PWA files are admitted together; a partial set is rejected. Historical
fourteen-file published sources and the earlier unpublished fifteen-file D14
contract retain their original exact replay grammar. The source ZIP contains
nineteen files before the MSI builder adds its reviewed instructions. Pass
`package-probe.py --source-root` for the frozen source checkout so its Cargo
version and license/notices cannot come from a different builder checkout.

Sources at version 0.1.8 or newer can additionally contain the complete eleven
reviewed tutorial screenshots under hashed `web/assets/*.png` paths. This makes
thirty-one MSI payload files (thirty before the builder adds its instructions).
Partial or duplicate screenshot sets are rejected. The historical fourteen,
fifteen and twenty-file grammars remain available for exact predecessor replay;
the new screenshot assets do not replace any existing member or source seal.

A real D14 baseline requires the committed source SHA to complete the existing
`Windows 7 MSVC candidate COMPILE_ONLY` GitHub workflow, followed by the existing
ABI2 source/package validation. This is a compatibility build, not a Win7
hardware test. For a real upgrade test, first qualify that baseline on the
Windows 11 test machine and capture its actual installed MSI/predecessor record.
Build the successor from a separate commit with a genuinely higher Cargo/product
version, then embed that exact predecessor record through
`package-abi2-msvc-candidate.py`'s qualified predecessor input. Reusing a new
version label around unchanged lower-version product bytes does not establish
that the installed program was upgraded. The existing source/CI inventory,
Setup/MSI binding, current-user lease and native guard checks remain required.
The D14 public root in `packaging/update-root.json` must be part of that frozen
source build; publishing private keys are never payload members.
