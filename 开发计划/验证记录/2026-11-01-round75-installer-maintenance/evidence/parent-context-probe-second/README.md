# R75 quiet MSI parent-context observer v2: GUI test-fixture ledger

Root approved a dedicated GUI2 test-fixture clone to avoid a CUI console flash. At this preparation point root has not generated that clone or executed it. This tool never creates/patches a PE or builds/runs an MSI. N has supplied the cfg(test)-only observer source with exact test name `installer_lifecycle::native::broker::context_probe::engine_parent_context_observer`; actual compilation/runtime remains separate. The original 22 CUI test artifacts, two GUI normal products, normal helper arguments and build targets remain unchanged. All first-version files are retained unchanged in sibling `parent-context-probe-first`.

`context-probe.py ... describe` encodes the proposed argv. The full actual test name is supplied explicitly; the example name in `check-context-probe.py` remains a nonexistent pure-model name. The only metadata carrier is a closed `--skip` token, not an inherited environment variable:

```text
--exact <actual-full-test-name> --test-threads=1 --nocapture --skip
fc-owned-parent-observer-v1:<32-lowercase-hex-nonce>:<absolute-Windows-owned-report-root>
```

Root basename must be `FutureClassroom-r75-parent-observer-<nonce>`. The future observer must independently verify the exact `future-classroom-owned-r75-parent-observer/v1` marker, owner, no reparse/alias paths, finite bounds and create-new output before writing. A token claiming ownership never substitutes for those checks. It must read the actual kernel parent/process creation/image, process TokenUser, optional thread token and console state using read-only OS handles, close every handle, and never call production Session::new/Begin/gate/Pending or registry/file mutation APIs outside its unique report. No parent-supplied PID/SID is authority. Report closed factual fields/OS error integers only; no command line/environment/secret dumps.

Before authoring, root must prove the actual same-SHA test harness accepts the exact argv plus `--list`, exits0 and lists only the intended test. The CLI-proof JSON has status `ACTUAL_HARNESS_CLI_PARSE_PASS_NOT_OBSERVER_RUNTIME`, `observerSha256`, `actualExit:true`, `exitCode:0`, `commandArgs:[...exact argv...,"--list"]`, and `selectedTests:["<actual full name>"]`; original actual command/raw output must be retained separately. `--list` proves parser/filter behavior only, not observer execution or engine identity. Rust documents repeated `--skip` and exact filters; no actual parser claim is made here.

The `author` and `check-exports` subcommands require `--fixture-ledger` and `--fixture-ledger-sha256`. Their closed validator reads the original x86 CUI3 fc_windows test artifact and the separately created x86 GUI2 fixture, checks both complete identities, and reconstructs the fixture from only the declared header fields. No code, entry point, imports, section layout or other bytes may differ. Subsystem changes exactly little-endian `0300` to `0200` at PE offset+92. If checksum is changed, it must be separately declared as exactly four bytes at PE offset+88 with both original/new hex strings; undeclared changes fail. The fixture remains `profileTest:true`, `normalProduct:false`, and original Cargo role tests/target fc_windows. It must never pass normal-helper admission or become a CI product. Root separately retains the actual original Cargo ledger/source binding. The actual --list CLI proof must bind the GUI clone SHA, not the CUI original SHA. Authoring only writes a fresh directory with the verified GUI test fixture and WXS; it does not construct MSI or execute a process.

The WXS defines one observer Binary/Type2 at1499, after costing/InstallValidate; unconditional Type19 abort1500; InstallInitialize1501. The isolated nonce-derived test Product/Upgrade IDs have no File/Component/Registry/Run/chainer effects. The deliberate abort is the expected failure (normally1603), not an installed-product success. After genuine compilation, export tables via original msiinfo; `check-exports` requires `--tables-list` with the complete original table listing and one IDT per non-system table, then checks exact Type2/19 rows, ordering/conditions, isolated product identity, empty side-effect tables and extracted Binary SHA. Unknown nonempty effects tables fail inspection. No actual MSI was built or table-exported by this preparation.

Root executes quiet first, preserving exact source/PE/MSI/argv/raw report identities. Before/after snapshots must show production target/markers/Run/gate/Pending and test Product/context registrations unchanged. Fully reap owned msiexec and observer processes before removing owned temporary artifacts. Other UI levels, elevation contexts and Win7/10/11 require separate actual evidence. If observed parent and helper SID differ, retain the original failure and redesign authentication from actual evidence; do not relax sameSID speculatively. An observer PASS does not prove production broker/guardian success.

Official references: [Type19 abort](https://learn.microsoft.com/en-us/windows/win32/msi/custom-action-type-19), [Type2](https://learn.microsoft.com/en-us/windows/win32/msi/custom-action-type-2), [custom-action sequencing](https://learn.microsoft.com/en-us/windows/win32/msi/sequencing-custom-actions), [Rust harness CLI](https://doc.rust-lang.org/rustc/tests/index.html).

Required fixture ledger schema (root creates it after the actual clone, never from these model values):

```json
{
  "status": "PASS_EXACT_CUI3_TO_GUI2_TEST_FIXTURE_CLONE",
  "profileTest": true, "normalProduct": false, "sourceGitSha": "<40hex actual source commit>",
  "sourceCui": {"path": "<local retained original CUI exe>", "bytes": 1, "sha256": "<actual SHA>"},
  "guiFixture": {"path": "<local separate GUI test clone>", "bytes": 1, "sha256": "<actual SHA>"},
  "cargoArtifact": {"package": "fc-windows", "target": "fc_windows", "role": "tests", "cargo_profile_test": true, "sha256": "<original CUI SHA>"},
  "changedFields": [{"name": "Subsystem", "offset": 0, "beforeHex": "0300", "afterHex": "0200"}]
}
```

The bytes/offset above are placeholders, not actual identities. Optional CheckSum is the second changedFields row with exactly name/offset/beforeHex/afterHex. The helper checks explicit ledger SHA plus full source/clone hashes and all bytes, so the placeholder values cannot be accepted. `describe` now returns the marker with a real LF character, matching N's exact marker bytes; the prior display used a literal backslash-n. This correction does not write or authorize a marker. N's observer writes bounded create-new `context.json` (8192-byte maximum) only after independently checking the marked, non-reparse owned root.
