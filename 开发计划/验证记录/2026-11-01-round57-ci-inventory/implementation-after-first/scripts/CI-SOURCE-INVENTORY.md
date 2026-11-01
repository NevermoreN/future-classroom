# CI source inventory capacity

The source-path inventory has a fixed limit of **20,000 paths**, including the
complete Git-bound historical evidence archive. The previous 10,000-path limit
had only 30 slots left at source `9ebe818bfc57c075e8b21419082dd44130b0e923`
(9,970 tracked files). This change provides bounded metadata capacity for
retained evidence; it does not increase a product or runtime resource limit.

All other limits remain: 64 MiB per file, 256 MiB total actual checkout content
hashed outside the exact historical archive prefix, and 16 MiB per metadata
output and stdout/stderr log stream. Original build/test/lint budgets, two build
jobs, full before/after comparison, reparse guards and clean-tree checks remain.
The larger path capacity does not promise that arbitrarily long path metadata
will fit within the unchanged byte limit.

The complete tracked set is still partitioned into ordinary SHA-256 inputs and
metadata-only `开发计划/验证记录/` entries. Historical paths are retained in the
full Git mode/type/blob-object/size inventory. They are not silently skipped or
claimed to have an actual checkout content SHA-256. Ordinary checkout content
must still match the exact Git blob or the exact full LF-to-CRLF transformation
when independently checked.

`scripts/check-ci-source-inventory.ps1` accepts the exact CI source via
`-TextSource` and its independently captured `-ExpectedSourceSha256`. It parses
the whole source but executes only four whitelisted helper definitions and the
two tracked-path Split expressions. Checks cover 20,000 and 20,001 entries, the
last entry, count/tree mismatch, complete partition summaries and UTF-8 metadata
byte boundaries. Hash-Inputs positive count admission is stopped by a sentinel
before any filesystem access; it is not a fake claim to have hashed 20,000 real
files. The check writes no file/environment, starts no process, and never runs
the CI entrypoint or hosted setup.

The check can be run as an isolated script block by the validation coordinator.
PowerShell 5.1 helper checks do not certify the hosted PowerShell >=7.2 CI body.
Historical artifact validators, including the archived `cf3780a6…` helper, keep
their original 10,000-path contract and source pin. New-source artifact audits
must explicitly acknowledge the new finite limit; do not edit an old validator
to turn preserved failures or old-source results green.
