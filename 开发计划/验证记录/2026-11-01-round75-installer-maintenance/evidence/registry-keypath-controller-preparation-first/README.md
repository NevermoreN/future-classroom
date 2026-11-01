Prepared serial registry-keypath fixture controller. Nothing here has run on Windows.

Review `controller.ps1`, `run-registry-fixture-first.py`, and the actual pure-contract output. The root agent alone may execute:

```sh
python3 -B /tmp/fc-round75-installer-maintenance/evidence/registry-keypath-controller-preparation-first/run-registry-fixture-first.py --execute --report /tmp/fc-round75-installer-maintenance/evidence/root-registry-keypath-windows-first.json
```

Omit `--execute` to check pinned local originals and print preparation metadata without SSH. The exact independent MSI SHA is d3c6d5e4937d5dd673e9835a7fbc8d5c7d03af8b0527e1a12d565da1a0e4e248; ProductCode is {60E68721-62F0-5656-B5BE-144E6B889AB6}. No production helper, gate, Pending, File, Run, service, or environment effect is authored in that MSI.

One original local 120-second deadline covers staging, child work and retention/cleanup. Run receives only the remaining milliseconds minus a 20-second capture reserve. Its one stopwatch never restarts; install has at most 35 seconds and leaves 55 seconds for genuine uninstall and the final query. Each child cap is additionally bounded by remaining time. Child/pipe-return waits consume the same deadline. Transport setup has no new allowance. If the deadline, SSH, original process exit, pipe return, or final absence becomes unknown, the controller fails and keeps facts/remote residuals rather than declaring cleanup.

The existing x86 query runs Before, Installed and After; it records the raw MsiGetComponentPathEx string and UTF16 units without asserting an unobserved 01/21 format. Before must prove this exact nonce fixture absent before `/i`. Finally, after that admission and an install attempt, the only engine cleanup is `/x` the exact ProductCode `/qn /norestart /L*v`. A failed measurement can still have a proven formal uninstall and directory cleanup; it remains a measurement FAIL. A failed/preexisting Before is never an authorization to uninstall.

Every original local command's stdout/stderr and actual exit/timeout are saved. After all admitted native children return and absence is established, Snapshot and Read retrieve and reopen all original remote files (including installer logs, phase reports and owned compiler temp files). Only then is a retention proof uploaded. Cleanup rechecks the exact root marker, input hashes, proof SHA, membership and file bytes, actual original process/pipe completion, formal uninstall and absence receipt, and rejects active owned processes/reparse points/unknown paths. It removes only the exact owned nonce temp directory. It never removes MSI registration/cache, edits production state, terminates unrelated processes or changes SSH/key authorization.

PowerShell 5.1 parsing and .NET/API runtime are NOT_RUN here; Python AST and 36 pure contracts are preparation evidence only. The MSI and static-check originals remain unchanged, including the first local static FAIL. No Windows success, keypath string, or lifecycle success is inferred from these checks.
