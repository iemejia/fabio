---
title: Protect local job receipts
description: Understand receipt durability, corruption errors, safe pruning and recovery limits.
---

Fabio stores tracked async jobs and Gen1 upgrade receipts in `~/.fabio/jobs.jsonl`.
This is **local evidence**, not a tenant-wide job inventory, a backup of business
data, or proof that remote work completed. A missing receipt never authorizes
another submission.

Notebook, dataflow, dbt, scheduler and shared copy/pipeline/Spark runners use one
receipt tracker. Outputs retain legacy job fields and add `jobReceipt`,
`receiptPersisted` and `retryAllowed:false`. After acceptance, storage failures
emit `RECEIPT_PERSISTENCE_FAILED` warnings and keep the exact receipt in stdout;
they do not erase remote acknowledgement or turn it into a retry opportunity.
Keep this output privately. Existing conflicting receipts are never silently
replaced to resolve an initial append failure.

With `--wait`, read failures, unknown statuses and local deadlines preserve the
same job identity and return non-retriable guidance. Only observed terminal
statuses set `completed_at`. A cancel acknowledgement records `cancel_requested`,
not `cancelled`; a failed cancel retains its typed error and uncertain evidence.
Default pruning keeps these nonterminal states. Successful observation remains
successful even if its final receipt snapshot could not be persisted.

## Inspect and preview

```bash
fabio --wrap-untrusted --readonly jobs list
fabio --wrap-untrusted --readonly jobs get --id "$RECEIPT_ID"
fabio --wrap-untrusted --readonly --dry-run jobs prune
```

The prune preview reports the current candidate IDs, count and remaining count.
It is a point-in-time preview, not a reservation: another process can change the
ledger before a later separately approved prune. Normal pruning removes only
recognized terminal states (`completed`, `succeeded`, `failed`, `cancelled`,
`partial_success`). It keeps preparing, accepted, running, unknown, timed-out and
unrecognized future states. `--readonly` blocks actual pruning.

`--include-running` irreversibly deletes **all local receipts**, including evidence
of accepted or uncertain operations. It does not cancel remote work. Obtain
explicit approval before using it; a prune summary carries a destructive marker
and this mode emits a warning.

## How updates work

- All cooperating readers and writers use the stable `jobs.lock` sidecar, with
  shared reader locks and an exclusive lock spanning each read-modify-write.
  Pruning uses the same transaction. Lock waits are bounded to five seconds;
  contention returns `CONFLICT`, never an instruction to replay a remote action.
- Writers validate the entire existing snapshot, serialize a complete replacement
  into a private `.jobs-*.tmp` file in the same directory, flush and sync its data,
  then atomically replace `jobs.jsonl`. The data file is never truncated in place.
- The published file is synced again. On Unix, the containing directory and
  creation of the normal `.fabio` directory are synced as well. Windows uses
  atomic replacement and explicit file flushes; portable directory sync is not
  available. Filesystem, operating-system and hardware power-loss guarantees vary;
  this is not a guarantee against every storage failure, especially on network
  filesystems.
- An abrupt process exit before replacement leaves the old complete snapshot;
  after replacement the new snapshot is complete. A later sync error can occur
  **after publication**: failure does not prove that the local write was rolled back.
- Legacy JSONL, CRLF, blank lines, and a valid final record without a newline are
  supported. Unknown extension fields survive rewrites. Empty/duplicate IDs,
  malformed records, invalid UTF-8 and read errors fail the whole read; no partial
  history is returned or automatically repaired. Unix files use private modes;
  on Windows files inherit the account directory's ACLs.

Do not delete or replace `jobs.lock`, even when no process appears active. Lock
ownership is released by the OS on process exit; the file itself remains. Mixing
older binaries that lock the data file with binaries using the sidecar is **not**
coordinated. Stop old processes before upgrading or restoring receipt storage.

## If storage fails

1. Stop automation that could submit new work. Keep the error and any original
   submission stdout privately; they may contain resource details.
2. Preserve `jobs.jsonl` and any orphan `.jobs-*.tmp` files. Diagnostics report the
   failing line or I/O category without echoing receipt contents. Fabio never
   promotes an orphan file or silently removes corrupt rows.
3. After all Fabio processes exit, restore only a verified private backup with
   explicitly reviewed scope and identity. There is no automatic repair/import
   command in this increment. A truncated or old backup is not proof that missing
   work was never submitted.
4. Inspect existing receipts and observe the original operation when its identity
   is available. Never invent an operation ID, use latest-history guesses, or
   repeat an accepted/unknown submission to recreate its receipt.

For Gen1 upgrades, storage must succeed **before POST**. A later persistence error
preserves the acknowledgement in stdout and warns against resubmission; see
[Gen1 execution receipts](../gen1-upgrade-execution/).
