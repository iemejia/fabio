---
title: Execute one approved Gen1 upgrade and retain its receipt
description: Single-attempt submission, exact-scope completion tracking and safe partial/unknown handling.
---

Gen1-to-Gen2 replacement is **irreversible**. Cached history is not carried over.
First complete [assessment and review](../gen1-upgrade-planning/), including
backup/reload, every consumer and storage/security semantics. Readiness and a
preview are **not** human approval.

## Preview, approve, submit once

```bash
# Offline preview: keep both guards
fabio --wrap-untrusted --readonly dataflow upgrade-gen1 \
  --workspace "$WORKSPACE_ID" --id "$GEN1_ID" --dry-run

# ONLY after explicit approval of this exact scope
fabio --wrap-untrusted dataflow upgrade-gen1 \
  --workspace "$WORKSPACE_ID" --id "$GEN1_ID" \
  --allow-in-place-upgrade --timeout 120
```

`--allow-in-place-upgrade` is a safety-bypass flag, not an auto-approve mode or
readiness override. The command refuses execution under `--readonly` and requires
fresh, complete, all-ready assessment of the selected IDs. Missing, attention,
unknown or partial results block **before POST**. `--all`/`--force` cannot bypass
the gate. Repeat `--id` for 1–50 distinct non-zero UUIDs, all in one workspace.

Examples in `fabio context agent` and `fabio context describe dataflow upgrade-gen1`
reuse the execution workflow's guarded preview and explicitly authorized submission
steps. Checks validate both CLI syntax and safety flags; a published example is
still not approval to execute it.

The service assesses all workspace Gen1 items. Assessment defaults are 3 pages
and 90 rows, including unselected rows, at page size 30. You can explicitly set
`--assessment-page-limit` (maximum 20) and `--assessment-row-limit` (maximum 600).
`--timeout` is a **total** budget for assessment, authentication, the single
submission, body decoding and optional same-operation waiting (default 120,
maximum 600 seconds).

The sender makes **exactly one POST**, even on 401/429/5xx or network failure.
There is no redirect following, automatic replay or cancellation. Acceptance
headers survive malformed or timed-out success bodies. Keep the receipt instead
of submitting again to get an ID.

## Observe the same receipt

```bash
fabio --wrap-untrusted --readonly dataflow follow-upgrade \
  --receipt-id "$RECEIPT_ID" --wait --timeout 120

fabio --wrap-untrusted --readonly jobs get --id "$RECEIPT_ID"
```

The local receipt ID is generated before POST and persisted with workspace and
target IDs. It is **not** `x-ms-request-id` or an invented cloud operation ID.
Without `--wait`, asynchronous submission returns accepted/not-checked evidence.
`upgrade-gen1 --wait` can observe the same submission immediately; neither wait
nor follow resubmits.

Operation identity comes from a unique non-zero operation header or an exact
configured-origin operation Location. The header is independently authoritative:
a different-origin Location is ignored, never used for transport. Same-origin
conflicts and malformed/duplicate identity headers remain invalid. Returned URLs
are never followed. Follow validates
the saved endpoint and scope, then uses GET-only operation/result reads. Waiting
honors acknowledgement retry delay, a total deadline and an attempt cap.

Result validation checks every requested ID/workspace, uniqueness, statuses and
summary counts. HTTP success or an aggregate operation success is not enough.
Per-item partial success and unknown outcomes retain the receipt and return
non-zero; never replay the whole batch. Reported `isRetriable:true` does not
authorize retry or override the published 24-hour failed-upgrade cooldown.

Local persistence failure before POST fails closed. If persistence fails after
submission, stdout remains the receipt: retain it securely for manual recovery,
and **do not resubmit**. Earlier verified results stay cached if later reads fail.
Follow updates local bookkeeping only. Preparing, accepted and unknown receipts
are not dropped by ordinary `jobs prune`.

The [local receipt store](../local-job-receipts/) uses a stable sidecar lock and
synced, atomically replaced snapshots. Corruption blocks observation and updates;
do not delete damaged evidence or replay accepted work to obtain another receipt.

Bounded private header diagnostics also permit GET-only recovery of an original
operation header that an older validator rejected solely for Location origin.
That is not a trace-ID/latest-history guess or another submission; the stored
workspace and every target/count must still match the operation result.

## Completion is not data readiness

No refresh, takeover, grant, consumer rewrite, destination configuration,
storage/security-mode change or Save As fallback runs. The new Gen2 still needs
a separately approved first refresh and consumer validation. Preserved IDs and a
completed migration do not prove historic data, fresh tables, compatible Viewer
or legacy-connector consumers, or rendered reports.
