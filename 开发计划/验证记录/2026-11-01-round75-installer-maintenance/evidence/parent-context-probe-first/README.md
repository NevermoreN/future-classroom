# R75 quiet MSI parent-context observer: preparation only

No observer test implementation, observer EXE or actual MSI has been produced by this preparation. Normal helper arguments, products and build targets remain unchanged. N/root must separately authorize and implement a future cfg(test)-only exact observer test in the existing fc_windows test harness.

`context-probe.py ... describe` encodes the proposed argv. The full future test name is supplied explicitly; the example name in `check-context-probe.py` is deliberately a nonexistent model. The only metadata carrier is a closed `--skip` token, not an inherited environment variable:

```text
--exact <actual-full-test-name> --test-threads=1 --nocapture --skip
fc-owned-parent-observer-v1:<32-lowercase-hex-nonce>:<absolute-Windows-owned-report-root>
```

Root basename must be `FutureClassroom-r75-parent-observer-<nonce>`. The future observer must independently verify the exact `future-classroom-owned-r75-parent-observer/v1` marker, owner, no reparse/alias paths, finite bounds and create-new output before writing. A token claiming ownership never substitutes for those checks. It must read the actual kernel parent/process creation/image, process TokenUser, optional thread token and console state using read-only OS handles, close every handle, and never call production Session::new/Begin/gate/Pending or registry/file mutation APIs outside its unique report. No parent-supplied PID/SID is authority. Report closed factual fields/OS error integers only; no command line/environment/secret dumps.

Before authoring, root must prove the actual same-SHA test harness accepts the exact argv plus `--list`, exits0 and lists only the intended test. The CLI-proof JSON has status `ACTUAL_HARNESS_CLI_PARSE_PASS_NOT_OBSERVER_RUNTIME`, `observerSha256`, `actualExit:true`, `exitCode:0`, `commandArgs:[...exact argv...,"--list"]`, and `selectedTests:["<actual full name>"]`; original actual command/raw output must be retained separately. `--list` proves parser/filter behavior only, not observer execution or engine identity. Rust documents repeated `--skip` and exact filters; no actual parser claim is made here.

The `author` subcommand requires that explicit proof SHA and an actual x86 CUI test harness SHA. It only writes a fresh authoring directory containing those test EXE bytes and WXS; it never runs wixl, msiexec or any process. No synthetic executable is accepted as compiler provenance. Root separately verifies the harness's original Cargo test-artifact ledger/source identity, using its unique Windows slot and exact reviewed MSI tools.

The WXS defines one observer Binary/Type2 at1499, after costing/InstallValidate; unconditional Type19 abort1500; InstallInitialize1501. The isolated nonce-derived test Product/Upgrade IDs have no File/Component/Registry/Run/chainer effects. The deliberate abort is the expected failure (normally1603), not an installed-product success. After genuine compilation, export tables via original msiinfo; `check-exports` requires `--tables-list` with the complete original table listing and one IDT per non-system table, then checks exact Type2/19 rows, ordering/conditions, isolated product identity, empty side-effect tables and extracted Binary SHA. Unknown nonempty effects tables fail inspection. No actual MSI was built or table-exported by this preparation.

Root executes quiet first, preserving exact source/PE/MSI/argv/raw report identities. Before/after snapshots must show production target/markers/Run/gate/Pending and test Product/context registrations unchanged. Fully reap owned msiexec and observer processes before removing owned temporary artifacts. Other UI levels, elevation contexts and Win7/10/11 require separate actual evidence. If observed parent and helper SID differ, retain the original failure and redesign authentication from actual evidence; do not relax sameSID speculatively. An observer PASS does not prove production broker/guardian success.

Official references: [Type19 abort](https://learn.microsoft.com/en-us/windows/win32/msi/custom-action-type-19), [Type2](https://learn.microsoft.com/en-us/windows/win32/msi/custom-action-type-2), [custom-action sequencing](https://learn.microsoft.com/en-us/windows/win32/msi/sequencing-custom-actions), [Rust harness CLI](https://doc.rust-lang.org/rustc/tests/index.html).
