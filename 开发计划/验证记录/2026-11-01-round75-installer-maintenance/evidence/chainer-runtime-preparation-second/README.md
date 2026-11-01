# Second quiet chainer runtime controller

This is a new preparation, preserving first controller and its actual FAIL.
Only root executes Windows via `run-quiet-chainer-second.py --execute`.
No normal Rust, gate/Pending/Run, policy, service configuration or first evidence
is changed. Product/Upgrade/cache/root absence after first has been independently
proved by root; controller still repeats exact before checks.

CLI remains --spec/--spec-sha256, --msi/--msi-sha256,
--static-report/--static-report-sha256, --report and optional --execute.
All paths are explicit absolute R75 owned paths and report must be fresh.
Use the NEW generated second spec and NEW original static checker JSON stdout,
not first inputs and not a build-operation wrapper. Second static JSON must
contain registryFixture matching spec exactly. No new operational spec/MSI/hash
is guessed or produced by this runtime preparation. A no-execute run validates
only actual local input identities and prints PREPARED, never contacts Windows.

Second authoring has exactly one attached HKCU32 RegistryKeyPath4 component:
Software\FutureClassroom-Test-ChainerProbe-<nonce>, OwnedMarker DWORD1.
The runtime opens only that exact current-user Registry32 key read-only. It
records key/value/type/integer facts and requires the WHOLE key absent before
and after, separately from actual COM ProductsEx contexts7 (current-user per-user
plus machine) and RelatedProducts Count reflection. Other users/cache are not
queried or deleted here. If any exact product registration/state or nonce key
survives, try/finally performs one formal quiet /x for that exact ProductCode,
within remaining original time. The closed chainer REMOVE=ALL exclusion keeps
formal /x from launching the experiment again. No direct key/registration/cache
removal is implemented; unexpected residuals remain FAIL/UNKNOWN for root.

Same original120-second clock,20-second cleanup reserve, short UTF8 stdin SSH,
source-bound pre-join/result bytes and logs, exact user-owner/SYSTEM-only DACL,
original helper/client PID+creation and held-exit proof remain. C# source/EXE is
unchanged; no synchronization barrier added. Missing held helper exit code stays
UNKNOWN even with source JSON Join0/End0. Raw verbose chainer lines are kept for
root's independent explicit exit qualification, never generalized into a guessed
success code. MSI client's actual raw integer exit is a separate fact.

First controller wrongly treated every new msiexec PID as test-owned. Second
queries only Win32_Service Name='msiserver'. A running shared service exemption
requires exact SCM ProcessId, same actual creation FILETIME in the observed
msiexec inventory, OS WindowsDirectory System32/SysWOW64 msiexec image, SCM
StartName LocalSystem AND targeted Win32_Process.GetOwnerSid S-1-5-18, followed
by a fresh SCM PID/state recheck. It records this proof. This process is
SCM-owned shared service, not a per-MSI session identity or test worker. It is
NEVER waited for exit, killed or required absent, even when it first appears
after an empty baseline. Existing baseline instances likewise remain untouched.
A different or unknown new msiexec instance is labeled UNCLASSIFIED, never
assumed test-owned; it can retain cleanup UNKNOWN, but is not killed or waited.
No broad list of user processes or command lines is collected.

Only original test client/helper must actually be gone. Timeout recovery may
terminate solely the already held and independently verified exact test helper
object (creation/image plus compiled fixture SHA), never a service or msiexec
client or process-name match. Any negative termination attempt disqualifies
Join/End success. Unknown access, close, identity or registration retains FAIL.

Raw JSON/logs are downloaded byte-for-byte with lengths/SHA. The owned Add-Type
compiler runtime-temp has identity-only metadata and recorded source in stdin.
Only after known client/helper gone, product/related/HKCU32 key absent, no
unclassified NEW msiexec (SCM exemption verified), full snapshot exact hashes,
owner/marker and raw-download acknowledgment does cleanup delete this new root,
keeping its marker until last. Exhaustion or unknown objects retain it for
root's separately recorded recovery, with no global policy/Run/SSH changes.

A PASS is only independent .NET/MSI Join/EndRollback/actual helper-exit0 evidence.
It is not Rust guardian, Begin-session binding, Windows7/10, hostile external
MSI/transform or product install/repair/uninstall acceptance. Current root N3
Cargo/source work is independent and not hash-asserted by this fixture controller.
