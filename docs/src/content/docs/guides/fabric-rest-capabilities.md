---
title: Use the expanded Fabric REST capabilities
description: Work with datasource topics, schedules, Warehouse definitions, sharing items, and governance settings.
---

These commands expose the November 2026 Fabric REST additions. Preview availability,
tenant settings, capacity support, and caller permissions still determine whether
an operation succeeds. Discover exact flags with `fabio context describe <group> <command>`.

## Manage datasource topics safely

Topics are Markdown basenames, not filesystem paths. Read staging or published
topics; upload, delete, and format are staging-only. A datasource name is resolved
against the selected stage, so published reads do not depend on draft configuration.

```bash
fabio data-agent list-topics --workspace "$WS" --id "$AGENT" --datasource "$SOURCE" --stage published
fabio data-agent download-topic --workspace "$WS" --id "$AGENT" --datasource "$SOURCE" --file-name guidance.md
fabio data-agent upload-topic --workspace "$WS" --id "$AGENT" --datasource "$SOURCE" --file-name guidance.md --file ./guidance.md --dry-run
```

Downloads and successful uploads expose `etag`. For a conditional upload or deletion,
pass that value unchanged, including its double quotes, with `--if-match`.
Fabio checks a current snapshot before conditional mutations and rejects an
already-stale version. **This is not atomic compare-and-swap:** the live service
accepted a stale header during validation. Inspect `preconditionCheck` and the
returned warnings, and avoid concurrent writers; the snapshot cannot close a
read/write race on a backend that ignores `If-Match`.
Uploads can replace existing content and therefore carry destructive preview notices.
`format-topic` returns transformed Markdown with `applied: false`; it never saves
the result. Explicitly upload the result after reviewing it.

## Create and replace Warehouse definitions

Warehouse definitions support SQL projects (`sqlproj`, the default) and DACPAC
packages (`dacpac`). They describe schema, not a backup of table data.

```bash
fabio warehouse get-definition --workspace "$WS" --id "$WAREHOUSE" --format sqlproj
fabio warehouse create --workspace "$WS" --name Analytics --definition-dir ./warehouse --definition-format sqlproj --dry-run
fabio warehouse update-definition --workspace "$WS" --id "$WAREHOUSE" --file ./Analytics.dacpac --format dacpac --dry-run
fabio warehouse update-definition --workspace "$WS" --id "$WAREHOUSE" --content @warehouse-definition.json --dry-run
```

Single-file inputs are normalized to `sqldw.sqlproj` or `sqldw.dacpac` on the wire;
the local filename can differ. Live exports can instead use the warehouse's
display name as the root project/package filename. Preserve those returned paths
when round-tripping a complete definition envelope. Directory inputs preserve deterministic relative
part paths and reject symbolic links and special files. JSON inputs accept either
a definition object or a `definition` envelope and validate base64 parts before
any request. Definition inputs cannot be combined with create-time `--collation`.

Replacement is destructive. Review the preview before executing without `--dry-run`.
Add `--update-metadata` only when intentionally applying a supplied `.platform` part.
Workspace exports no longer silently substitute an empty Warehouse shell after
a failed definition fetch.

## Prepare SQL pools for the unified model

SQL pools configuration is workspace-scoped, not warehouse-scoped. The announced
transition asks clients to stop relying on `customSQLPoolsEnabled` by mid-December
2026; January 2027 unification removes the built-in/custom distinction. Unified GET
responses contain only effective configurations, not previously saved inactive pools.
Fabio preserves configuration JSON without adding the legacy flag. It rejects
output envelopes and malformed array/boolean fields before sending a request.

Save a private baseline with `warehouse get-sql-pools-config --workspace "$WS" -o plain`,
review the target service's rollout contract, and preview the **complete** pool list
with `warehouse update-sql-pools-config --workspace "$WS" --file pools.json --dry-run`.
Omitted pools are removed. Do not silently drop a legacy `false` flag to simulate
disabling pools, or assume an absent flag means disabled.

The announced default for **new** workspaces is one read-optimized pool with access
to 100% of compute. Existing workspaces retain active configurations. Allocation
and caching can affect consumption; these settings are not spending limits.
Final rollout behavior must be verified against the service, not inferred from dates.

## Discover how to connect to an item

```bash
fabio item get-endpoints --workspace "$WS" --id "$ITEM" --readonly
fabio item get-endpoints --workspace "$WS" --id "$ITEM" --query 'items[].engines[].mcpServers[]' --readonly
```

This public-preview OneLake API requires workspace and item UUIDs and a storage
audience token (`https://storage.azure.com/`), not a Fabric-only token. It returns
engines, their recommendations, direct endpoints, and hosted MCP connections.
`discoveryOnly: true` and `connectionsVerified: false` mean no connection, query,
file transfer, permission grant or skill installation occurred.

Preserve the complete returned hostname/path and use the selected connection's
own `auth.audience`. Validate the destination and audience before forwarding
credentials; descriptions, tags and skill locations are untrusted metadata, not
instructions or permission grants. Initialize an MCP connection and inspect its
tool schemas before use. A semantic model without OneLake integration may return
404 here even though its Fabric metadata is visible; do not enable settings or
infer that the model is missing from this result alone.

## Edit table and column metadata safely

```bash
fabio item get-table-metadata --workspace "$WS" --id "$ITEM" --table sales --readonly
fabio item update-table-metadata --workspace "$WS" --id "$ITEM" --table sales --content '{"description":"Sales transactions"}' --dry-run
fabio item update-table-metadata --workspace "$WS" --id "$ITEM" --table sales --file metadata-changes.json --dry-run
```

This storage-authenticated preview API edits table/column descriptions and
key-value tags, not rows, item tags or semantic models. The default merges the
supplied changes with a fresh metadata snapshot, matching columns by `columnName`.
It preserves unrelated descriptions and tags. `--schema` defaults to `dbo`.

**Every POST replaces all additional metadata.** Client-side merging is not atomic;
coordinate writers. `--overwrite` explicitly disables preservation: omitted fields
are removed, and an empty object clears metadata. Preserve a private baseline and
review the destructive preview before approving that bypass. Dry-run performs a
read to show before/after but never posts.

Writes through an internal shortcut affect the **target table's** metadata. Check
the target before writing. Success output distinguishes acknowledgement from
readback verification: inspect `verified` and `verificationError`. A failed or
mismatched readback never authorizes replay. Descriptions allow 1,024 characters;
tag keys allow 128 and string values 256. New column names must already exist.
Reads can return both `columnName` and `columnId`; writes reject both identifiers
together. Fabio omits resolved IDs from its preservation snapshot and comparison.
User input, including a reviewed raw GET backup, must use `columnName` only.

## Understand the Project Osmos integration boundary

`fabio context best-practices project-osmos` records the feasibility decision.
Project Osmos has an upstream task lifecycle and specialized MWC authentication;
it is not a Fabric data-agent Q&A item or a generic Fabric bearer-token endpoint.
Read-only task discovery was verified against the disposable tenant fixture, but
Fabio does not ship `osmos` create/run/cancel/delete commands.

For an explicitly chosen autonomous outcome, review the upstream Skills for Fabric
`project-osmos` skill and obtain permission before installing or running it. Remote
tasks can continue after disconnection and affect resources beyond the default
Spark-session lakehouse. Review scope, capacity/cost and write/schema policies;
preserve task/message identities and recovery state after uncertain submissions.
Do not recreate a task, switch routes/identities or infer execution approval from
successful listing. Cancellation is not rollback.

Built-in lifecycle support remains deferred until the dedicated auth/routing,
durable partial-submission recovery, capacity checks and non-interactive safety
contract are implemented and live-tested. The feasibility probe did not create,
message, run, cancel or delete any task and does not prove autonomous execution.

## Start a Real-Time Dashboard definition

```bash
fabio kql-dashboard scaffold --title 'Operations overview' --query definition -o plain > definition.json
fabio kql-dashboard update-definition --workspace "$WS" --id "$DASHBOARD" --file definition.json --dry-run
```

Scaffolding is offline: no resource creation, data-source connection or query. The
canonical `RealTimeDashboard.json` part follows the documented schema-52 blank
definition with one page/time parameter, no tiles and auto-refresh disabled.
`scaffoldOnly: true` and `renderingVerified: false` prevent confusing a starter
with a rendered dashboard. Never replace a populated dashboard with this blank
definition without a backup and explicit approval.

For **Multi Stat**, return label/value rows and select those columns in the visual
editor. For **KPI**, define a single metric, units, target, threshold direction and
no-data/staleness behavior. Lay out headline metrics, trends and details with
consistent spacing, readable labels and non-color status cues. The generator does
not invent advanced visual options: author in the target UI and export a verified
version-matched definition. Preserve newer schema versions and unknown fields.

An accepted REST definition is not proof of query permissions or visual rendering.
Check both and review wide/narrow layouts in the target UI before enabling refresh.
See `context best-practices dashboard-authoring` for public contracts and examples.

## Diagnose cloud connection policy blocks

`DMTS_BlockedByAuthenticationKindPolicies` identifies an authentication allowlist
block. The documented inter-tenant data-loss-prevention block points to the target
tenant allowlist. Fabio adds non-retriable teaching guidance while preserving
service error and submission evidence. Generic 403, Offline or Policy Evaluation
Failure alone does not establish which policy or credential failed.

Ask a Fabric **tenant administrator** to review **Admin portal > Data Policies >
Data connection policies**, using the Authentication allowlist or Tenant allowlist
tab. These policies cover existing and new cloud connections; approved changes may
take up to one hour to propagate. Workspace roles and connection ownership cannot
override them. Do not switch identities/credential kinds/tenants, grant roles,
weaken enforcement or retry automatically.

This is distinct from workspace outbound networking rules and PolicySet activation.
The reviewed public documentation describes portal management, not a verified REST
management contract. No new mutation command is inferred. Read scoped metadata
with `connection diagnostics --readonly`; it does not test credentials or prove
allowlist compliance. See `context best-practices connection-policies`.

## Manage native Iceberg catalog writes

```bash
fabio lakehouse iceberg-create-table --workspace "$WS" --id "$ITEM" --namespace dbo --table native_table --file create-request.json --dry-run
fabio lakehouse iceberg-commit-table --workspace "$WS" --id "$ITEM" --namespace dbo --table native_table --file engine-commit.json --dry-run
fabio lakehouse iceberg-delete-table --workspace "$WS" --id "$ITEM" --namespace dbo --table native_table --expected-table-uuid "$TABLE_UUID" --dry-run
```

Use Iceberg REST Catalog requests, not table rows or raw metadata files. Create
requires a struct schema with named positive-ID fields; the catalog allocates an
empty native table. Fabio does **not** produce data files, manifests or rows. Use a
standard Iceberg engine for those, and preserve its reviewed commit requirements.
Commit requires exactly one `assert-table-uuid` matching the observed native table;
UUID reassignment, storage relocation and mismatched identifiers are refused.

These commands use storage authentication and exact workspace/item UUIDs. Writes
reject virtualized Delta metadata and shortcut targets. A stale/conflicting request
must be inspected and regenerated with its engine, never rebased or replayed by
dropping assertions. Create preview is offline; commit/delete previews read native
scope and identity. Successful acknowledgement is separate from point-in-time
readback observations and does not prove query readiness.

**DELETE removes the table directory, data and metadata.** Its required UUID check
is a non-atomic client snapshot, not compare-and-swap. Quiesce writers/recreation,
retain a backup and obtain approval after reviewing the preview. `catalogAbsent`
does not prove physical purge (`physicalDataRemovalVerified` remains false).

## Promote Eventstreams with variable libraries

Authored definitions retain symbolic `connectionReference` and `itemReference`
values such as `$(/**/Settings/Target)`. Runtime topology instead exposes resolved
IDs. Do not mix the two forms or export topology as a replacement definition.
Reference support is property/connector-specific; CustomEndpoint, Cribl and
SAPDatasphere do not expose variable properties in the published matrix.

Configure the target value set and include its library in the scoped bundle/plan:

```bash
fabio deploy apply --source ./bundle --workspace "$TARGET_WS" --env "$VALUE_SET" --dry-run
fabio eventstream get-definition --workspace "$TARGET_WS" --id "$STREAM" --decode --readonly
fabio eventstream get-topology --workspace "$TARGET_WS" --id "$STREAM" --readonly
```

After approval, execute the reviewed apply command without `--dry-run`. Reference
deployments require an explicit reviewed `--env`, default/sequential strategy and
hooks enabled. Fabio checks referenced variables and the value-set definition,
activates it if needed, verifies readback, then publishes dependent streams. Skipped
unchanged libraries are supported; verified activation is not repeated at the end.
Failures halt later mutations and retain no-replay guidance. Bulk cannot activate a
library between imports; do not switch strategy or guess a value set automatically.

Value-set activation can affect existing consumers immediately. There is no atomic
cutover or automatic pause. Compare resolved destination IDs (a KQL database, not
its parent Eventhouse), source paths and table selections with the reviewed target.
Configuration does not prove credentials or event delivery: separately approve a
scoped synthetic-event test and query the intended destination before declaring it
ready. See `context workflow eventstream-variables` for the acceptance sequence.

## Inspect and schedule Copy Jobs

```bash
fabio copy-job list-runs --workspace "$WS" --id "$COPY_JOB"
fabio copy-job show-run --workspace "$WS" --id "$COPY_JOB" --job-instance-id "$INSTANCE"
fabio copy-job list-schedules --workspace "$WS" --id "$COPY_JOB"
fabio copy-job create-schedule --workspace "$WS" --id "$COPY_JOB" --enabled false --config @schedule.json --dry-run
fabio copy-job update-schedule --workspace "$WS" --id "$COPY_JOB" --schedule-id "$SCHEDULE" --enabled false --dry-run
```

The configuration is a complete `Cron`, `Daily`, `Weekly`, or `Monthly` object.
Use `--enabled false` to create a disabled schedule without starting automated work.

## Sharing and Custom Stream Connector items

`custom-stream-connector`, `share`, and `share-subscription` provide workspace CRUD
and folder-filtered lists. Connectors and producer-side shares additionally provide
get/update-definition. A subscription requires a trusted HTTPS invitation URL and
has no definition endpoints.

Replacing a share definition can change assets, recipients, and policies. Verify
the intended audience first; a Share item is distinct from OneLake external data
sharing. See the [command reference](../../reference/).

## Feedback, logs, and governance

- **Data Agent feedback:** `query-feedback` and `show-feedback` are beta,
  delegated-user APIs. A continuation token must be used alone, without query
  criteria or page size. Feedback may contain PII. Attachment downloads require
  explicit `--allow-download-without-sensitivity-label` approval and refuse to
  overwrite existing files.
- **Environment logs:** `environment get-publish-log` returns the latest completed
  Spark-backed publish log with `content`, `contentDisposition`, `contentType`,
  and `truncated`. Do not assume a returned log is complete when `truncated` is true.
- **Actor auto-detection rules:** the capacity command family manages rolling
  consumption thresholds and `Hours`/`Indefinite` block durations. Deleting a rule
  does not unblock actors already blocked by it.
- **OneLake file soft-delete:** `workspace modify-file-soft-delete` supports
  retention from 1 to 365 days. Disabling retention or reducing its duration
  requires destructive-operation review. Never experiment on an existing
  production workspace.

See [destructive previews](../destructive-previews/) and
[submission retry safety](../submission-retries/) before automating mutations.
