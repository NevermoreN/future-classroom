R75 archive preparation — NOT RUN

Only root may execute after all R75 producers/controllers are terminal and source membership frozen.
Source: /tmp/fc-round75-installer-maintenance (exact root/evidence/controller owners checked).
Destination: /workspace/共享/Eliora/未来教室计划/开发计划/验证记录/2026-11-01-round75-installer-maintenance
No archive or main write has been performed by this preparation.

Invocation template (all paths below are relative to the R75 root):
python3 /tmp/fc-round75-installer-maintenance/evidence/archive-preparation-first/archive-evidence-first.py \
  --source-base-git <40hex-actual-candidate-worktree-base-only> \
  --candidate-producer evidence/n-release-third.json --expected-producer-sha256 <64hex> \
  --candidate-freeze evidence/root-n3-codegen-source-freeze-first.json --expected-freeze-sha256 <64hex> \
  --candidate-joint-freeze evidence/root-joint-source-freeze-fourth.json --expected-joint-freeze-sha256 <64hex> \
  --acceptance-report evidence/<actual-final-acceptance>.json --expected-acceptance-sha256 <64hex> \
  --expected-acceptance-status <exact-actual-PASS_R75_scope> \
  --readonly-audit evidence/<actual-final-readonly>.json --expected-readonly-sha256 <64hex>

Without --execute, this only prints PREPARED_ARCHIVE_ONLY_NOT_EXECUTED and reads no payload.
Root must review actual final producer/freeze/joint names and SHAs; if N4 supersedes N3 use those explicit inputs.
Only after freeze add --root-controllers-terminal --execute. Missing/nonterminal final acceptance/audit yields WAIT with no report.
Optional --public-certificate-allowlist requires exact root-relative paths, certificate bytes/SHA, provenance report SHA,
and canonical single public PEM certificate parse; no private-key suffix is read.

Whole-root layout is preserved. Worktree/target/tools are completely pruned before recursion.
Exactly 24 explicitly listed target PEs from the successful final producer are hashed for identity only; no target recursion.
All unpruned original source/spec/exports/logs/FAIL/RUNNING histories, B3 backup and N2 preserved source remain unchanged.
External fixture PE/MSI/ZIP/DEB and actual MZ/CFBF/ZIP/ELF/AR/compressed magic are excluded with exact identity.
Tool package hashes remain in their original evidence provenance reports; no tools subtree read or copied.
Huge objdump stdout (at least 1 MiB) is losslessly gzip-derived with original/stored size and SHA; other originals exact.
Every retained source/stored file is independently reopened, compared, UTF8/UTF16/JSON-base64 decoded credential-scanned.
Public SSH authorization text has no private-key classification; external SSH key paths are never read.
A scan finding must be reviewed precisely without modifying original bytes; preparation makes no promise of final scan PASS.
No deletion, Git mutation, Windows/SSH execution, Cargo, network or PE execution exists in this helper.
A failure may leave partial new destination evidence; retain it and qualify it rather than retry into/overwrite it.
