---
name: fabio-lakehouse
description: >-
  Intent-scoped fabio skill for Microsoft Fabric Lakehouse work: create lakehouses, upload/load/sync files and Delta tables, OneLake shortcuts, Iceberg, and Materialized Lake Views. Use when the user wants to ingest, organize, or move data in a lakehouse. Triggers: "create lakehouse", "upload to lakehouse", "load table", "list tables", "sync lakehouse", "onelake", "shortcut", "materialized lake view".
license: MIT
---

# fabio-lakehouse — Lakehouse — files, tables, sync, Iceberg, Materialized Lake Views

> **Generated file — do not edit by hand.** This intent-scoped sub-skill of the `fabio` skill is generated from fabio's command schema plus authored judgment. Regenerate with `cargo test generate_subskills -- --ignored`. For install, auth, output envelope, global flags, and agent-safety rules, see the root `fabio` skill.

> **Prefer runtime introspection.** This index is a snapshot; the installed binary is always authoritative. Use `fabio context agent --group <group>` and `fabio context describe <group> <command>` for exact flags and output shapes.

## When to use
- Creating a lakehouse or listing/inspecting its files and Delta tables.
- Uploading files to Files/ and loading them into Delta tables.
- Syncing files between lakehouses or from a local directory (rsync-like).
- Creating OneLake shortcuts (typed flags for all 9 targets: OneLake, ADLS Gen2, S3, GCS, S3-compatible, Dataverse, Azure Blob, External Data Share, OneDrive/SharePoint), optionally with a CSV->Delta transform (--transform csvToDelta), listing them (list-shortcuts), or managing OneLake security.
- Deleting an arbitrary OneLake directory recursively (delete-directory), beyond just delete-table.
- Managing Materialized Lake Views (execution definitions + refresh schedules).

## When NOT to use (route elsewhere)
- Querying lakehouse tables with T-SQL -> use fabio-warehouse-sql (sql-endpoint / lakehouse plan/query).
- Real-time/streaming ingestion or KQL -> use fabio-rti-kql.
- Report/semantic model over the lakehouse -> use the bi-developer persona.

## Command index

Generated from fabio's command schema. For full flag details use `fabio context agent --group <group>` or `fabio context describe <group> <command>`.

### fabio lakehouse
Manage lakehouses (tables, files, shortcuts)

| Command | Mutates | Description |
|---|---|---|
| `fabio lakehouse bulk-create-shortcuts` | yes | Bulk-create multiple shortcuts (LRO) |
| `fabio lakehouse copy-file` | yes | Copy files between lakehouses (supports glob patterns for parallel copy) |
| `fabio lakehouse copy-table` | yes | Copy a table between lakehouses |
| `fabio lakehouse create` | yes | Create a new lakehouse |
| `fabio lakehouse create-directory` | yes | Create a directory in a lakehouse (DFS) |
| `fabio lakehouse create-execution-definition` | yes | Create a materialized lake view execution definition |
| `fabio lakehouse create-materialized-views-schedule` | yes | Create a schedule for materialized lake view refresh |
| `fabio lakehouse create-shortcut` | yes | Create a shortcut (typed target flags per target type, or raw --target JSON) |
| `fabio lakehouse delete` | yes | Delete a lakehouse |
| `fabio lakehouse delete-directory` | yes | Recursively delete a directory from a lakehouse (DFS; irreversible) |
| `fabio lakehouse delete-execution-definition` | yes | Delete a materialized lake view execution definition |
| `fabio lakehouse delete-file` | yes | Delete a file from a lakehouse |
| `fabio lakehouse delete-materialized-views-schedule` | yes | Delete a schedule for materialized lake view refresh |
| `fabio lakehouse delete-shortcut` | yes | Delete a shortcut |
| `fabio lakehouse delete-table` | yes | Delete a table from a lakehouse |
| `fabio lakehouse download` | no | Download a file from a lakehouse |
| `fabio lakehouse get-definition` | no | Get the definition of a lakehouse |
| `fabio lakehouse get-livy-session` | no | Get details of a Livy session for a lakehouse |
| `fabio lakehouse get-materialized-views-schedule` | no | Show a single materialized lake view refresh schedule by ID |
| `fabio lakehouse get-shortcut` | no | Get shortcut details |
| `fabio lakehouse iceberg-config` | no | Get Iceberg REST Catalog configuration for a lakehouse |
| `fabio lakehouse iceberg-credentials` | no | Load vended storage credentials scoped to a specific table |
| `fabio lakehouse iceberg-namespace` | no | Get metadata for a specific namespace via the Iceberg REST Catalog |
| `fabio lakehouse iceberg-namespace-exists` | no | Check if a namespace exists via the Iceberg REST Catalog (lightweight HEAD) |
| `fabio lakehouse iceberg-namespaces` | no | List table namespaces (schemas) via the Iceberg REST Catalog |
| `fabio lakehouse iceberg-snapshots` | no | Show snapshot history for a table via the Iceberg REST Catalog |
| `fabio lakehouse iceberg-stats` | no | Show table statistics from the latest Iceberg snapshot (record/file counts, size) |
| `fabio lakehouse iceberg-table` | no | Get table definition (schema, partitions, properties) via the Iceberg REST Catalog |
| `fabio lakehouse iceberg-table-exists` | no | Check if a table exists via the Iceberg REST Catalog (lightweight HEAD) |
| `fabio lakehouse iceberg-tables` | no | List tables in a namespace via the Iceberg REST Catalog |
| `fabio lakehouse list` | no | List lakehouses in a workspace |
| `fabio lakehouse list-execution-definitions` | no | List materialized lake view execution definitions for a lakehouse |
| `fabio lakehouse list-files` | no | List files in a lakehouse |
| `fabio lakehouse list-livy-sessions` | no | List Livy sessions for a lakehouse |
| `fabio lakehouse list-materialized-views-schedules` | no | List materialized lake view refresh schedules (named schedules for this lakehouse) |
| `fabio lakehouse list-shortcuts` | no | List shortcuts in an item (hides DW-managed shortcuts by default) |
| `fabio lakehouse list-tables` | no | List tables in a lakehouse |
| `fabio lakehouse load-table` | yes | Load a file (already in the lakehouse) into a Delta table |
| `fabio lakehouse move-file` | yes | Move files between lakehouses (supports glob patterns for parallel move) |
| `fabio lakehouse move-table` | yes | Move a table between lakehouses (copy + delete source) |
| `fabio lakehouse optimize-table` | yes | Optimize a Delta table (V-Order compaction + optional Z-Order) |
| `fabio lakehouse plan` | no | Capture the estimated execution plan (`SHOWPLAN_XML`) without executing the query |
| `fabio lakehouse pool-insights` | no | Report SQL pool state changes and sustained pressure events (from `queryinsights.sql_pool_insights`) |
| `fabio lakehouse queries-frequent` | no | List frequently-run queries (from `queryinsights.frequently_run_queries`) |
| `fabio lakehouse queries-history` | no | List completed query history (from `queryinsights.exec_requests_history`) |
| `fabio lakehouse queries-long-running` | no | List long-running queries (from `queryinsights.long_running_queries`) |
| `fabio lakehouse queries-running` | no | List currently running queries on the lakehouse SQL endpoint |
| `fabio lakehouse query` | no | Execute SQL against the lakehouse SQL endpoint |
| `fabio lakehouse refresh-materialized-views` | yes | Trigger a refresh of materialized lake views |
| `fabio lakehouse run-table-maintenance` | yes | Run table maintenance on a lakehouse |
| `fabio lakehouse show` | no | Show details of a lakehouse |
| `fabio lakehouse show-execution-definition` | no | Show a materialized lake view execution definition |
| `fabio lakehouse sync` | yes | Sync files between lakehouses (parallel, copies new/modified files) |
| `fabio lakehouse table-health` | no | Report file-level storage health metrics for a table (runs `sys.sp_get_table_health_metrics` on the SQL analytics endpoint) |
| `fabio lakehouse table-schema` | no | Show Delta table schema (reads from `OneLake` `_delta_log` without Spark/SQL) |
| `fabio lakehouse update` | yes | Update a lakehouse (rename/redescribe) |
| `fabio lakehouse update-definition` | yes | Update the definition of a lakehouse |
| `fabio lakehouse update-execution-definition` | yes | Update a materialized lake view execution definition |
| `fabio lakehouse update-materialized-views-schedule` | yes | Update a schedule for materialized lake view refresh |
| `fabio lakehouse upload` | yes | Upload files to a lakehouse (supports glob patterns for parallel upload) |
| `fabio lakehouse upload-table` | yes | Upload a local file and load it into a Delta table (upload + load-table in one step) |
| `fabio lakehouse vacuum-table` | yes | Vacuum a Delta table (remove old files beyond retention period) |

### fabio onelake-security
Manage `OneLake` data access roles (row/column-level security)

| Command | Mutates | Description |
|---|---|---|
| `fabio onelake-security create` | yes | Create or update a single data access role |
| `fabio onelake-security delete` | yes | Delete a data access role |
| `fabio onelake-security list` | no | List data access roles for an item |
| `fabio onelake-security show` | no | Show details of a data access role |
| `fabio onelake-security upsert` | yes | Replace all data access roles for an item (atomic PUT) |

## Must / Prefer / Avoid
### MUST
- Use PascalCase for --mode (Overwrite/Append) and --format (Csv/Parquet).
- Upload to Files/ before load-table, or use upload-table for the combined one-step path.
- Convert JSON to CSV/Parquet before load-table (JSON is not a supported load format).

### PREFER
- lakehouse sync (ETag-based, rename-aware) over manual re-upload loops.
- move-file within a lakehouse (atomic O(1) rename) over copy+delete.
- Runtime introspection (context agent --group lakehouse, context describe) over guessing flags.

### AVOID
- load-table with JSON format (unsupported).
- Lowercase enum values like --mode overwrite or --format csv (silently rejected by the API).
- Deleting Lakehouse/Warehouse/other data-bearing items without explicit user approval.

## Key gotchas
- Table file listing lists from the root to get real item-id-prefixed paths.
- move-file within a lakehouse is an atomic O(1) rename; cross-item falls back to copy+delete automatically.
- list-shortcuts hides DW-managed internal shortcuts (OneLake->OneLake under Tables/) by default — pass --include-managed to see them.
- create-shortcut takes typed flags per target type (--connection-id/--location/--subpath for cloud; --target-workspace/--target-item/--target-path for OneLake; --bucket for S3); --target JSON remains a raw escape hatch.
- create-shortcut can convert source files into a queryable Delta table via --transform: only csvToDelta is exposed by the REST API (with --csv-delimiter/--csv-no-header/--csv-keep-error-files/--transform-include-subfolders). Parquet/JSON/Excel and the AI-powered transforms are portal-only. Create the shortcut under Tables; --transform-json is a raw escape hatch. Materialization is an async Fabric Spark job (~2-min polling) and the documented sources are external (ADLS/S3/...).

## Troubleshooting
| Symptom | Fix |
|---|---|
| load-table fails or ignores the file | Check --mode/--format are PascalCase (Overwrite, Csv) and the format is Csv or Parquet, not JSON. |
| FORBIDDEN when creating/deleting | You need at least Member role on the workspace; Delete requires Member+. |
| list-tables shows no rows after an ETL run | Confirm the notebook/job actually wrote Delta tables and completed (use notebook run --wait). |
| move-file returns 403 across items | Atomic rename only works within the same lakehouse; across items fabio falls back to copy+delete automatically. |

## Safety
- load-table --mode Overwrite replaces the whole table — confirm with the user.
- sync --delete removes destination files not present in source — preview without --delete first.
- delete-directory is a recursive OneLake delete — fabio refuses an empty/root path (that would erase the entire lakehouse); confirm the exact subpath, and it is --dry-run-guarded.
- Deleting a lakehouse is a protected, data-bearing operation — never do it without explicit user approval.

## Shared references
Cross-cutting operational guidance (the "common" layer) — consult the relevant topic before non-trivial work:

| Reference | Covers |
|---|---|
| `fabio context best-practices throttling` | fabio transparently handles 429 (Too Many Requests) and gateway errors. Agents do NOT need to implement retry logic. |
| `fabio context best-practices pagination` | fabio handles pagination via --all (auto-fetch all pages), --continuation-token (resume), and --limit (truncate). Agents rarely need to paginate manually. |
| `fabio context best-practices lro` | Many Fabric operations are async (return 202). fabio polls them automatically. Use --wait for job operations. |
| `fabio context best-practices shortcuts` | OneLake shortcuts virtualize external/other-OneLake storage into a lakehouse. Create with TYPED flags per target type (9 supported: OneLake, AdlsGen2, AmazonS3, AzureBlobStorage, GoogleCloudStorage, S3Compatible, Dataverse, ExternalDataShare, OneDriveSharePoint); cloud targets need a connection first. list-shortcuts enumerates them (DW-managed internal shortcuts hidden unless --include-managed). load-table resolves shortcut paths even when list-files does not show their contents. |
| `fabio context best-practices lakehouse-mlv` | Materialized Lake Views are pre-computed Delta table snapshots inside a Fabric Lakehouse that accelerate repeated query patterns. They are refreshed on demand or on a schedule, and their refresh behavior is controlled by MLV execution definitions. |

## See also
- fabio context persona data-engineer
- fabio context workflow lakehouse-etl
- fabio context workflow lakehouse-mlv
- fabio context disambiguate materialized-view
- fabio context disambiguate sql-endpoint
