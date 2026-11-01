# R73 cleanup preparation only

Nothing has been removed. `cleanup-r73-third.py` was parsed with `ast.parse` only; its imports, preflight and removal path were not run. Output from this preparation is confined to this directory.

After every original controller is physically terminal and root has finished archive/Git byte proofs, root first commits source+archive, then generates the first-commit proof. A second closure commit includes every finalized controller file, the cleanup plan and its three final inventories. Root produces an independent final proof at a main repository path covering all files from those two commits; that final proof itself may remain uncommitted and is pinned by an explicit SHA256. It does not live in either temporary controller, so it creates no self-copy/inventory cycle. This helper only verifies this outcome; it does not implement the commits or generate approval. Do not generate a running inventory and immediately trust it.

Required plan schema:

```json
{
  "status": "ROOT_REVIEWED_TERMINAL_R73_CLEANUP_PLAN",
  "rootConfirmedAllControllersTerminal": true,
  "archiveIndexSha256": "FINAL_ARCHIVE_INDEX_SHA256",
  "hostFailureAttribution": {"path": "n-host-qualification-first.json", "sha256": "c2883954cb93bea75730c95a42b56430fe1759b162b078ab1648ce60089ad6d1"},
  "terminalReports": [
    {"path": "RELATIVE_TO_R73_EVIDENCE", "sha256": "ACTUAL_FINAL_SHA256", "status": "EXACT_ACTUAL_PASS_STATUS"}
  ],
  "roots": [
    {"path": "/tmp/fc-round73-installer-broker", "entries": []},
    {"path": "/tmp/fc-round73-archive-controller", "entries": []},
    {"path": "/tmp/fc-round73-cleanup-controller", "entries": []}
  ],
  "controllerCopies": [
    {"source": "/tmp/fc-round73-cleanup-controller/cleanup-r73-third.py", "archived": "开发计划/验证记录/EXACT_COMMITTED_COPY_PATH"}
  ]
}
```

`terminalReports` must include the actual final native/full runtime acceptance, codegen and host controllers (at least three distinct root-reviewed records); include every other original controller whose terminal state is material. The original `n-host-first.json` is honestly listed with status `FAIL` and SHA `da0715d485985841af18f274ba28a6db9425f0edcc596877c392d206f89db036`. This single pinned report has a narrow terminal exception: 22 harnesses/989 passes; exactly tests PASS0 and strict-clippy FAIL101, both actual exits without timeout; identical sourceBefore/sourceAfter; original scratch cleanup PASS. Its failure remains a failure. Every other final report in terminalReports must have an actual PASS status. `hostFailureAttribution` separately pins N's original qualification (its original non-PASS-prefixed status is preserved), including exact original-report binding, six diagnostics/four unchanged files, and the independently pinned actual-exit1/reaped producer record. Do not list those attribution/producer records as though their statuses were PASS, or add a generic FAIL exception. Other historical failed runs are retained by archive replay, not recast as final PASS. Compile-only report status remains compile-only; it does not substitute for actual runtime acceptance.

Each `entries` array is the final complete sorted non-following inventory of that exact root. Row forms are `{"path":"relative","kind":"directory"}`, `{"path":"relative","kind":"file","bytes":123,"sha256":"..."}`, or `{"path":"relative","kind":"symlink","target":"exact relative target"}`. Root can use the helper's `inventory` function in a separately authorized final controller after all producers stop. Symlinks are not traversed or read as files; only reviewed relative links that terminate within the same owned root are accepted. Unknown files, special objects, dangling/absolute/outside links or any membership/byte change abort cleanup. No tar-member provenance expansion or per-object compiler provenance is required for the owned target tree.

`controllerCopies` must map **every regular file including `.owner`** from both archive and cleanup controllers to exact committed repository copies. Copy the helper, this README, preparation receipt and any subsequent controller/proof files first. The plan itself belongs in repository records, so it does not create a self-hashing inventory cycle. The committed proof supports the existing R72 `commit/gitBlobOid` or R70 `verifiedCommit/gitBlob` row schema; every listed blob is reopened and checked at the exact commit path and against main bytes.

Execution command, only after root review (placeholders are intentionally not executable values):

```text
python3 -B /tmp/fc-round73-cleanup-controller/cleanup-r73-third.py 
  --plan <absolute repository plan> --expected-plan-sha256 <sha256>
  --committed-proof <absolute repository final proof> --expected-proof-sha256 <sha256>
  --commit <full final commit> --execute
```

The helper requires all 15 integrated sources (B7 + N3 + root's web_editor fix), all 548 original build inputs, exact source worktree delta/base/branch, original evidence and decoded optional-gzip archive bytes, 24 excluded N2 PE identities, seven exact downloaded MSI packages and successful extraction reports, tools ownership, and no self-owned process retaining any scratch root via argv/cwd/open FD. It makes no network/Windows calls and never terminates a process.

All checks precede `git worktree remove --force` on the **proved exact** worktree, deletion of branch `codex/installer-broker`, then removal of the three exact owned roots. Failures retain the remainder and record partial actions; no unknown delta is force-deleted. The removal receipt is created at `开发计划/验证记录/2026-11-01-round73-cleanup-first.json`; root must subsequently preserve/sync that receipt normally.

Preserved: main target, global caches, three development ZIPs, Windows SSH service/public-key authorization and external test private key. No key contents are read or hashed. A permission denial inspecting an own process is fail-closed. An unexpected root child, extra archived/excluded payload, changed fixed report, new ignored worktree file or mismatched archive-controller owner needs root review, not automatic weakening.

Second-version correction: terminal root membership excludes `host-scratch-first`. The helper checks the original `n-host-first.json` exact scratchRoot and `scratchCleanup=PASS_ONLY_EXACT_OWNED_MARKER_AND_EMPTY_ROOT_REMOVED`, then requires the path to be absent, including absence of dangling symlinks. This validates scratch retirement only; the original Host report overall FAIL remains unchanged and is not promoted to PASS. The separate root-reviewed final acceptance/controller reports carry their own actual outcomes. First helper/README/preparation are retained byte-for-byte and must also be mapped to committed copies before controller removal.

Third-version correction: only the precise historical Host report above is admitted as terminal FAIL; it is accompanied by exact read-only attribution and physical-parent-terminal records. This does not claim Host strict Clippy passed or introduce broader Linux product support. First and second helpers/receipts remain immutable originals; all are included in the controller copy map before cleanup.
