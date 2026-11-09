---
title: Plan a Gen1 dataflow upgrade without executing it
description: Bounded service assessments and review-only plans before irreversible in-place migration.
---

`dataflow plan-upgrade` is **GET-only**. It reads bounded assessment metadata and
produces review guidance, not an upgrade executor or consumer/data readiness test.

```bash
# Offline read-scope preview
fabio --wrap-untrusted --readonly dataflow plan-upgrade \
  --workspace "$WORKSPACE_ID" --id "$GEN1_ID" --dry-run

# Explicit selected IDs; no mutations
fabio --wrap-untrusted --readonly dataflow plan-upgrade \
  --workspace "$WORKSPACE_ID" --id "$GEN1_ID" \
  --page-size 30 --page-limit 3 --assessment-limit 90 --timeout 30
```

Repeat `--id` for **1–50 distinct non-zero UUIDs**. The API assesses **all Gen1
dataflows in this workspace**, without individual-ID filtering; local selection
controls only the output. Gen2 items are excluded. The planner never scans the
tenant, reads definitions, imports/creates items, refreshes, takes over or grants
permissions. Workspace Viewer or higher is required for the assessment read;
read access does not establish permission to upgrade every selected item.

## Bound the assessment

Defaults are 30 rows per page (maximum 30), 3 pages (maximum 20), 90 workspace
assessment rows (maximum 600), and a **total 30-second deadline** (maximum 300)
covering authentication, retries and in-flight reads. Unselected rows count
toward the limits. `--all` and `--force` do not override bounds or apply anything.

Opaque continuation tokens stay on the same workspace assessment route; returned
URLs are not followed. A plan cannot resume one cursor—use
`dataflow list-upgrade-readiness` for raw page resumption.

## Read evidence conservatively

- **ReadyToMigrate** assesses the dataflow itself, not every consumer or limitation.
- **NeedsAttention** can be migratable under the service contract, but still needs
  review. This conservative planner offers no unresolved-attention batch preview.
- **UpgradeUnavailable** is blocked; **Unknown** is incomplete evidence.
- Missing/malformed/error-bearing/wrong-scope rows, duplicate identities, unknown
  reasons, conflicting severities and incomplete pagination stay unknown. A
  partial scan does not certify a visible ready row.
- Missing selected IDs are not proof of deletion: they may be Gen2, excluded or
  unavailable. Names, messages, credentials, URLs and unselected IDs are omitted.

`--strict` prints the plan and fails unless all selected assessments are ready.
Even a successful strict report has `readyToApply: false` and `applied: false`.

## In-place migration is irreversible

The proposed plan is `destructive: true`, but the planner itself never mutates.
It always retains approval and review blockers. An all-ready complete scan can
offer a **preview** using both `--readonly` and `--dry-run`; do not remove them.
The planner never executes. [Separate execution and receipt tracking](../gen1-upgrade-execution/)
requires explicit approval and fresh all-ready assessment; a plan is not
permission to invoke or blindly retry it. Verification snippets inspect item
metadata only, not upgrade completion or loaded data.

Before any separately approved upgrade, review:

- **Historical data:** cached Gen1 data is not carried over. Confirm source
  reloadability or backup. The original Gen1 cannot be recovered after upgrade.
- **Consumers:** legacy `PowerBI.Dataflows`, external Desktop/Excel files and actual
  DirectQuery dependencies are not exhaustively detected. Do not silently switch
  consumers to Import/Direct Lake or change their identity/security semantics.
- **Storage and orchestration:** BYOL destinations, incremental-refresh settings
  and linked-entity cascading refresh are not preserved automatically. Review
  both sides of linked relationships and all before/after actions.
- **Names, access and costs:** the service may sanitize/prefix names; Viewer table
  consumption, Power BI REST automation and capacity consumption differ in Gen2.
  Never rename, grant roles, rewrite clients or start workloads automatically.
- **After migration:** the new Gen2 holds no data until its first separately
  approved refresh completes. Consumer saving/rebinding and connector credentials
  may need changes; preserved IDs or currently displayed imported data do not
  prove a successful refresh or compatible consumers.

Published wizard guidance also describes a **24-hour failed-upgrade retry wait**
and a retained Gen1 **30-day window that can block OAP enablement**. Unknown or
accepted work must not be resubmitted to obtain an operation ID. Cancellation is
unsupported. Save As is a different, explicitly chosen copy migration—not an
automatic fallback.

See [Microsoft's upgrade guidance](https://learn.microsoft.com/en-us/fabric/data-factory/migrate-to-dataflow-gen2-using-upgrade-wizard)
and `fabio context best-practices gen1-upgrade-planning` for review details.
