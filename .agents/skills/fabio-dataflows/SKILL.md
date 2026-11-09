---
name: fabio-dataflows
description: >-
  Intent-scoped fabio skill for Fabric low-code data preparation: Dataflows Gen2 (Power Query / M mashup) and datamarts. Use for visual/low-code ETL that loads into a lakehouse or warehouse. Triggers: "dataflow", "dataflow gen2", "power query", "mashup", "low-code etl", "datamart", "transform without code".
license: MIT
---

# fabio-dataflows — Dataflows — low-code Power Query (Gen2) ETL and datamarts

> **Generated file — do not edit by hand.** This intent-scoped sub-skill of the `fabio` skill is generated from fabio's command schema plus authored judgment. Regenerate with `cargo test generate_subskills -- --ignored`. For install, auth, output envelope, global flags, and agent-safety rules, see the root `fabio` skill.

> **Prefer runtime introspection.** This index is a snapshot; the installed binary is always authoritative. Use `fabio context agent --group <group>` and `fabio context describe <group> <command>` for exact flags and output shapes.

## When to use
- Creating/updating/refreshing Dataflows Gen2 (Power Query mashup ETL).
- Assessing and upgrading legacy Gen1 dataflows to Gen2 in place.
- Inspecting or querying an existing dataflow definition.
- Managing datamarts.

## When NOT to use (route elsewhere)
- Code-first (PySpark/notebook) transformation -> use fabio-data-engineering.
- Orchestrating activities (copy, notebook, stored proc) -> that's a Data Pipeline (fabio-data-engineering).
- Loading files directly into Delta tables -> use fabio-lakehouse.

## Command index

Generated from fabio's command schema. For full flag details use `fabio context agent --group <group>` or `fabio context describe <group> <command>`.

### fabio dataflow
Manage dataflows (Power BI data transformation)

| Command | Mutates | Description |
|---|---|---|
| `fabio dataflow create` | yes | Create a new dataflow |
| `fabio dataflow delete` | yes | Delete a dataflow |
| `fabio dataflow discover-parameters` | no | Discover parameters of a dataflow |
| `fabio dataflow execute-query` | no | Execute a query against a dataflow (returns Apache Arrow IPC) |
| `fabio dataflow follow-upgrade` | no | Observe an existing Gen1 upgrade receipt with GET-only exact-scope result checks; never submit or cancel |
| `fabio dataflow get-definition` | no | Get the definition of a dataflow |
| `fabio dataflow list` | no | List dataflows in a workspace |
| `fabio dataflow list-upgrade-readiness` | no | List Gen1 dataflow upgrade readiness results for a workspace (Preview) |
| `fabio dataflow plan-upgrade` | no | Plan an explicitly selected Gen1 batch from bounded workspace assessment reads; never execute upgrades |
| `fabio dataflow run` | yes | Run a dataflow on demand |
| `fabio dataflow show` | no | Show details of a dataflow |
| `fabio dataflow update` | yes | Update dataflow properties (name and/or description) |
| `fabio dataflow update-definition` | yes | Update the definition of a dataflow |
| `fabio dataflow upgrade-gen1` | yes | Upgrade 1-50 Gen1 dataflows to Gen2 in place (Preview) |

### fabio datamart
Manage datamarts (Power BI)

| Command | Mutates | Description |
|---|---|---|
| `fabio datamart list` | no | List datamarts in a workspace |

## Must / Prefer / Avoid
### MUST
- Never use isRetriable:true as approval to retry a Gen1 upgrade or as an override of the published 24-hour failed-upgrade cooldown. Unknown/accepted work must be correlated, not resubmitted for an ID. No whole-batch retry or cancellation. The planner never executes; separate upgrade-gen1 requires explicit --allow-in-place-upgrade approval and complete all-ready assessment, then sends exactly ONE POST. Keep BOTH --readonly/--dry-run on previews.
- Use --readonly dataflow plan-upgrade --workspace <WS> --id <GEN1_ID> (repeatable) for bounded review before considering irreversible in-place migration. The service assesses ALL Gen1 in that workspace; IDs select output, not an upstream ID filter. --dry-run is offline. Respect page-size/page-limit/assessment-limit and the TOTAL timeout; --all/--force do not bypass bounds.
- Treat 'dataflow' as Dataflow Gen2 (the current Power Query item); see 'fabio context disambiguate dataflow'.
- Provide both the mashup (Power Query M) and queryMetadata parts when building a definition.
- Run list-upgrade-readiness before upgrade-gen1 and inspect every NeedsAttention or UpgradeUnavailable reason.

### PREFER
- Dataflow Gen2 for new low-code ETL; migrate legacy Gen1 rather than authoring new Gen1.
- A Data Pipeline (fabio-data-engineering) when the task is orchestration, not transformation.

### AVOID
- Confusing a Dataflow (transform) with a Data Pipeline (orchestrate) — see disambiguate dataflow.
- Authoring new Gen1 dataflows (Gen2 is the strategic path).

## Key gotchas
- MightBeAccessedByDirectQuery is only a potential-consumer signal, not proof of an actual DirectQuery consumer. HasLinkedEntities concerns upstream links; IsReferencedByLinkedEntities concerns downstream links. If both occur, review BOTH sides and actual consumers. Gen2 does not preserve linked cascading refresh. Do not automatically change consumer storage/security modes or rewrite schedules.
- HasByolAsStorage means Gen1 BYOL is not preserved as a Gen2 data destination. HasIncrementalRefresh means settings/history need explicit destination and reload/backup review. Do not repoint storage or enable a new destination/incremental-refresh configuration automatically, even when NeedsAttention permits migration under the service contract.
- Preserved IDs do NOT preserve cached history, compatible consumers or fresh data. Gen2 needs its first separately approved refresh to COMPLETE before data is available. Legacy PowerBI.Dataflows cannot consume the upgraded Gen2; PowerPlatform.Dataflows has different connector credentials. Workspace Viewer users cannot consume its tables through that connector. Review changes explicitly; do not grant Contributor, refresh, rewrite connectors or silently choose Save As automatically.
- plan-upgrade keeps missing/malformed/partial/conflicting/unknown assessments unknown and offers no unresolved-attention batch preview. ReadyToMigrate does not inventory consumers or detect every legacy connector/DirectQuery dependency; --strict checks selected assessments only, not apply readiness.
- Gen1 in-place upgrade is irreversible and cached historic data is not carried over. The new Gen2 has no data until a separate first refresh completes; preserved IDs are not working consumers or fresh data. Review backup/reload, BYOL/incremental refresh, linked entities, names, Viewer consumption, automation APIs and costs; never silently change storage/security modes, rewrite consumers, take over or grant access.
- plan-upgrade reviewPlan ALWAYS remains readyToApply:false/applied:false with approval blockers; it does not execute. Separately reviewed upgrade-gen1 sends once, retains a local receipt before POST and preserves acknowledgement on success-body failure. follow-upgrade --receipt-id <LOCAL_RECEIPT_ID> uses GET-only exact-scope observation, never latest history or a trace ID. Per-item partial/unknown results return non-zero without authorizing retries. See gen1-upgrade-execution.
- A Dataflow Gen2 definition combines a Power Query mashup with queryMetadata; both are required.
- Refresh is asynchronous — poll or use the run/wait semantics.
- Gen1 upgrade is Preview and in place: successful rows preserve item IDs, a 1-50 item batch may partially succeed, only the owner or a workspace Admin can upgrade each row, and cancellation is unsupported.

## Troubleshooting
| Symptom | Fix |
|---|---|
| Unsure whether to use a dataflow or a pipeline | Transform data -> dataflow; sequence activities -> data-pipeline. Run 'fabio context disambiguate dataflow'. |
| Definition rejected | Ensure both the mashup and queryMetadata parts are present and valid. |
| 'dataflow run' fails with 'Invalid QueriesMetadata ... must not be empty' | A REST-authored Gen2 dataflow needs a queriesMetadata map in queryMetadata.json: add "queriesMetadata": {"<QueryName>": {"queryId", "queryName", "loadEnabled"}}. NOTE the Gen2 lakehouse data destination is portal-generated (a hand-authored dataDestinations binding runs but persists no table). |

## Safety
- A service-ready Gen1 plan is NOT approval or an execution workflow. Before any irreversible upgrade, require separately approved scope, historical-data backup/reload and consumer/security review plus receipt-safe submission/completion handling. No automatic upgrade, credential/role change, storage-mode conversion or Save As fallback is permitted from a plan.
- Deleting or overwriting a dataflow definition replaces its transformation logic — confirm with the user.
- Gen1-to-Gen2 upgrade changes the item in place. Preview with --dry-run and review readiness results before executing.

## Shared references
Cross-cutting operational guidance (the "common" layer) — consult the relevant topic before non-trivial work:

| Reference | Covers |
|---|---|
| `fabio context best-practices gen1-upgrade-execution` | upgrade-gen1 requires explicit in-place approval and complete all-ready assessment, sends exactly one guarded POST, preserves acknowledgement/body uncertainty and persists a local scoped receipt. follow-upgrade uses GET-only observation and validates every migration ID/workspace/count; partial/unknown outcomes never authorize retry, grants, mode changes or refresh. |
| `fabio context best-practices gen1-upgrade-planning` | dataflow plan-upgrade reads bounded workspace Gen1 assessments for explicitly selected IDs, preserves unknown/partial evidence, and reports approval-blocked in-place upgrade previews. Service readiness is not consumer compatibility, historic-data preservation, permissions, approval, submission or refreshed Gen2 readiness. No upgrade or repair runs. |
| `fabio context best-practices receipt-storage` | The local JSONL job ledger uses a stable sidecar lock, bounded waits and synced atomic snapshots. Corrupt or unreadable records fail closed, never become empty history. Preserve evidence and never replay accepted/unknown work to recreate a missing receipt. |
| `fabio context best-practices throttling` | fabio transparently handles 429 (Too Many Requests) and gateway errors. Agents do NOT need to implement retry logic. |
| `fabio context best-practices pagination` | fabio handles pagination via --all (auto-fetch all pages), --continuation-token (resume), and --limit (truncate). Agents rarely need to paginate manually. |
| `fabio context best-practices lro` | Many Fabric operations are async (return 202). fabio polls them automatically. Use --wait for job operations. |

## See also
- fabio context persona data-engineer
- fabio context disambiguate dataflow
- fabio context blueprint basic-data-analytics
- fabio context persona data-solution-architect
