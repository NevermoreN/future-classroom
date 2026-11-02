# Build the controlled candidate

`package-msi-candidate.py --controlled` retains the existing formal Win7 MSVC ZIP,
normal GUI helper, CI result SHA, before/after source inventory, original Cargo
metadata and normal compiler-artifact admission. It additionally requires
`--guard-library` and `--guard-library-sha256` matching the one
`custom_action_library/fc-windows/fc_windows.dll` recorded by that same CI result.
The explicit library path/hash never replaces CI provenance. CI records the
normal `guard-library-compile` build, actual x86 DLL/import/export checks, and
keeps both existing normal GUI artifact roles unchanged. Modern CI remains a
separate compatibility gate; a modern or GNU result cannot satisfy formal Win7
admission.

The controlled mode has its own exact Binary/Type1/sequence contract and rejects
legacy Type2/chainer mixing or caller-conditioned hooks. It adds no files to the
fourteen installed payloads. MSI/installed payload bounds remain30 MiB, normal
inputs32 MiB, development ZIP15 MiB and Web5 MiB. The output bundle is
`Setup.exe` plus fixed `FutureClassroom.msi`. It is built without Windows
Installer execution. The final MSI bytes are sealed only into the external
Setup, with exact Binary/complete payload readback and sole zero32-hole binding.

## Explicit local GNU DEV fixture

`scripts/package-controlled-dev.py` defaults to NOT_RUN. Its `--execute` path is
for root's current real Rust joint test only; it never calls formal admission,
claims CI/MSVC provenance, or executes a Windows image. Supply the five explicit
packaging tools and reviewed msibuild SHA, `--source-root`, `--evidence` plus its
SHA, `--web-report` plus SHA, `--web-dist`, fresh `--input-dir` and `--output-dir`.
The first directory holds the newly made complete GNU development ZIP; the
second holds the controlled bundle and actual tool/contract records.

The evidence JSON schema is:

```json
{
  "status": "DEV_FROZEN_NORMAL_BUILD_ONLY",
  "formal_ci": false,
  "target": "i686-pc-windows-gnu",
  "source_root": "/actual/frozen/worktree",
  "source_head": "actual base Git HEAD; frozen raw inputs independently bound",
  "inputs_before": {"path":"inputs-before.json","bytes":0,"sha256":"actual"},
  "inputs_after": {"path":"inputs-after.json","bytes":0,"sha256":"actual"},
  "metadata": {"path":"metadata.stdout","bytes":0,"sha256":"actual"},
  "metadata_command": {
    "argv":["cargo","metadata","--offline","--locked","--no-deps","--format-version","1"],
    "actual_exit":0,"parent_reaped":true,"timed_out":false,
    "stdout":{"path":"metadata.stdout","bytes":0,"sha256":"actual"},
    "stderr":{"path":"metadata.stderr","bytes":0,"sha256":"actual"}
  },
  "commands":[
    {"name":"product-compile","argv":["actual complete argv"],
     "actual_exit":0,"parent_reaped":true,"timed_out":false,
     "stdout":{"path":"product.stdout","bytes":0,"sha256":"actual"},
     "stderr":{"path":"product.stderr","bytes":0,"sha256":"actual"},
     "artifact":{"path":"fc-bootstrap.exe","bytes":0,"sha256":"actual"}}
  ]
}
```

All facts above use actual size/SHA; illustrative zero/`actual` values are not
accepted. Commands must contain exactly `product-compile` (fc-bootstrap normal),
`lifecycle-compile` (fc-installer-lifecycle normal), and `guard-library-compile`
(fc-windows --lib). Each real Cargo command selects release/locked/default
features/GNU target/JSON and its exact package/normal target, without test
selection. Cargo metadata's actual target_directory must be the owned target;
selected executable/filename must lie in its `i686-pc-windows-gnu/release`
directory, and the copied artifact must equal the original bytes there. Metadata
and compiler messages must agree on package, normal target source and test=false.
The original raw logs and source inventories remain available, not just summaries.

Root can load this script with runpy (`__name__` stays non-main) and call
`source_inventory(Path(source_root))` before and after actual compiler commands.
Write the returned list as JSON twice; both original byte streams must equal,
and their records must equal the current tracked/untracked selected build inputs.
The source-head field labels the actual base commit; it does not claim a dirty
frozen working tree is that commit's blobs. The independently pinned raw source
inventory is the DEV compilation identity. Freeze all writers before capturing.

The Web report must be the successful separate owned build with source inputs
still equal, both npm commands reaped/actual0, every dist member equal and the
generated license equal the frozen source notice. A new complete ZIP is made
from those assets and the newly admitted normal GNU main image. Old ZIP main
images are not reused. Result scope stays DEV_ONLY_NOT_FORMAL_MSVC;
production joint MSI integration remains NOT_EXERCISED until actual tests.

Same-bundle source removal is the finite cleanup route being developed. This
slice does not claim standard ARP uninstall or bundle relocation/deletion repair.
Do not change guard conditions to obtain cleanup. Actual direct-inner rejection,
14-file missing/tampered repair, bad-preimage rollback, formal same-bundle removal
and all owned residual cleanup must be recorded separately from package PASS.

Controlled development candidates author the fixed Property `MsiLogging=voicewarmup!`
and verify it on readback. Legacy mode does not add this property. Root supplies
owned TEMP/TMP before launching Setup, so Installer-generated MSI*.LOG belongs to
the finite test directory. No policy or caller bypass property is authored. This
verbose diagnostic setting is for this development candidate; production logging
requires its own product decision. It does not prove external Begin API logging.

The present lifecycle/seal parser requires PE subsystem-version6.1 for both
normal images and the library. The finite GNU producer therefore records a
scoped `RUSTFLAGS=-C target-feature=+crt-static -C link-arg=-Wl,--major-subsystem-version,6,--minor-subsystem-version,1`
for all three actual normal builds. This is PE metadata, not Win7 certification.
The DLL may declare GUI2 or CUI3; its actual DLL characteristic is mandatory,
and the static import auditor checks the declared subsystem. Normal EXEs remain GUI2.

## ABI2 format foundation

`broker/carrier.rs` and `scripts/installer-carrier-format.py` define the new
immutable carrier descriptor. The full PE prefix, descriptor and footer remain
identical across packages; the complete final MSI fills exactly one 32-byte
SHA256 slot in the external Setup. `Binary.FcPayloadManifest` has a separate
version2 envelope binding family, PackageCode and the bounded payload ledger.
Its PackageCode must be fixed before MSI creation; the envelope contains no
whole-MSI hash. This avoids a package or carrier hash referring to itself.

Rust/Python synthetic fixtures match byte for byte. The published-host registry
is empty and these formats grant no install or upgrade authority. The existing
CLI above continues to build its legacy manifest-overlay candidate. Actual ABI2
package authoring, before-Begin source capture and package-bound guard tickets
remain separate implementation steps. A completed upgrade requires two different
product versions, one original transaction and verified old/new file effects.
