---
name: fabio-warehouse-sql
description: >-
  Intent-scoped fabio skill for Fabric T-SQL surfaces: Warehouse (read-write analytics), SQL Database (OLTP), SQL analytics endpoint (read-only over a lakehouse), and warehouse snapshots. Use for running T-SQL, capturing execution plans, monitoring queries, and managing statistics. Triggers: "warehouse query", "run SQL", "T-SQL", "execution plan", "showplan", "queries running", "kill session", "statistics", "sql database", "sql endpoint".
license: MIT
---

# fabio-warehouse-sql — Warehouse & SQL — T-SQL query, execution plans, insights, statistics

> **Generated file — do not edit by hand.** This intent-scoped sub-skill of the `fabio` skill is generated from fabio's command schema plus authored judgment. Regenerate with `cargo test generate_subskills -- --ignored`. For install, auth, output envelope, global flags, and agent-safety rules, see the root `fabio` skill.

> **Prefer runtime introspection.** This index is a snapshot; the installed binary is always authoritative. Use `fabio context agent --group <group>` and `fabio context describe <group> <command>` for exact flags and output shapes.

## When to use
- Executing T-SQL against a Warehouse, SQL Database, or SQL analytics endpoint (also lakehouse plan/query).
- Discovering schema: list-tables (tables + views, optional --schema) and describe-table (columns, types, nullability for one table) over INFORMATION_SCHEMA.
- Bulk-loading files into a Warehouse table: copy-into (COPY INTO from Azure storage / OneLake; CSV or PARQUET).
- Capturing estimated execution plans (SHOWPLAN_XML) without running the query.
- Monitoring queries: running / frequent / long-running / history; killing a session.
- Managing user-defined statistics (list/show/create/update/delete).
- Creating/managing warehouse snapshots.
- Refreshing all or selected SQL analytics endpoint tables, optionally recreating their SQL metadata.

## When NOT to use (route elsewhere)
- Loading files into Delta tables -> use fabio-lakehouse (load-table).
- KQL / real-time queries -> use fabio-rti-kql.
- DAX over a semantic model -> use the bi-developer persona (semantic-model query).

## Command index

Generated from fabio's command schema. For full flag details use `fabio context agent --group <group>` or `fabio context describe <group> <command>`.

### fabio warehouse
Manage warehouses and run SQL queries

| Command | Mutates | Description |
|---|---|---|
| `fabio warehouse connection-string` | no | Get the connection string for a warehouse |
| `fabio warehouse copy-into` | yes | Bulk-load files from Azure storage / OneLake into a table with `COPY INTO` |
| `fabio warehouse create` | yes | Create a new warehouse |
| `fabio warehouse create-restore-point` | yes | Create a restore point for a warehouse |
| `fabio warehouse delete` | yes | Delete a warehouse |
| `fabio warehouse delete-restore-point` | yes | Delete a restore point |
| `fabio warehouse describe-table` | no | Describe the columns of a table (from `INFORMATION_SCHEMA.COLUMNS`) |
| `fabio warehouse get-audit-settings` | no | Get SQL audit settings for a warehouse |
| `fabio warehouse get-retention` | no | Report the configured data-retention (time-travel) period, in days |
| `fabio warehouse get-sql-pools-config` | no | Get SQL pools configuration for a workspace |
| `fabio warehouse list` | no | List warehouses in a workspace |
| `fabio warehouse list-restore-points` | no | List restore points for a warehouse |
| `fabio warehouse list-tables` | no | List tables and views in a warehouse (from `INFORMATION_SCHEMA.TABLES`) |
| `fabio warehouse mcp-url` | no | Print the remote Fabric Data Warehouse MCP server URLs (item-scoped + global) for agent consumption |
| `fabio warehouse plan` | no | Capture the estimated execution plan (`SHOWPLAN_XML`) without executing the query |
| `fabio warehouse pool-insights` | no | Report SQL pool state changes and sustained pressure events (from `queryinsights.sql_pool_insights`) |
| `fabio warehouse queries-frequent` | no | List frequently-run queries (from `queryinsights.frequently_run_queries`) |
| `fabio warehouse queries-history` | no | List completed query history (from `queryinsights.exec_requests_history`) |
| `fabio warehouse queries-kill` | yes | Kill a running query session by session ID |
| `fabio warehouse queries-long-running` | no | List long-running queries (from `queryinsights.long_running_queries`) |
| `fabio warehouse queries-running` | no | List currently running queries on a warehouse |
| `fabio warehouse query` | yes | Execute a SQL query against a warehouse or SQL endpoint |
| `fabio warehouse restore-to-point` | yes | Restore a warehouse to a restore point |
| `fabio warehouse set-audit-actions` | yes | Set audit actions and groups for a warehouse |
| `fabio warehouse set-retention` | yes | Configure the data-retention (time-travel) period, in days (1-120) |
| `fabio warehouse show` | no | Show details of a warehouse |
| `fabio warehouse show-restore-point` | no | Show details of a restore point |
| `fabio warehouse statistics-create` | yes | Create a user-defined statistic on a column |
| `fabio warehouse statistics-delete` | yes | Delete a user-defined statistic |
| `fabio warehouse statistics-list` | no | List user-defined statistics on a warehouse or SQL endpoint |
| `fabio warehouse statistics-show` | no | Show details of a statistic (header, density vector, histogram) |
| `fabio warehouse statistics-update` | yes | Update (refresh) an existing statistic |
| `fabio warehouse update` | yes | Update warehouse properties (name and/or description) |
| `fabio warehouse update-audit-settings` | yes | Update SQL audit settings for a warehouse |
| `fabio warehouse update-restore-point` | yes | Update a restore point |
| `fabio warehouse update-sql-pools-config` | yes | Update SQL pools configuration for a workspace |

### fabio sql-database
Manage SQL databases (Fabric-native transactional databases)

| Command | Mutates | Description |
|---|---|---|
| `fabio sql-database connection-string` | no | Show the TDS connection string for a SQL database |
| `fabio sql-database create` | yes | Create a new SQL database |
| `fabio sql-database delete` | yes | Delete a SQL database |
| `fabio sql-database get-audit-settings` | no | Get SQL audit settings for the database |
| `fabio sql-database get-definition` | no | Get the definition of a SQL database (dacpac or sqlproj format) |
| `fabio sql-database import` | yes | Import data from a CSV or JSON file into a SQL database table |
| `fabio sql-database list` | no | List SQL databases in a workspace |
| `fabio sql-database list-deleted` | no | List restorable deleted SQL databases in a workspace |
| `fabio sql-database plan` | no | Capture the estimated execution plan (`SHOWPLAN_XML`) without executing the query |
| `fabio sql-database queries-history` | no | List completed query history |
| `fabio sql-database queries-kill` | yes | Kill a running query session by session ID |
| `fabio sql-database queries-running` | no | List currently running queries on a SQL database |
| `fabio sql-database query` | yes | Execute a SQL query against a SQL database via TDS |
| `fabio sql-database revalidate-cmk` | yes | Revalidate Customer-Managed Key (CMK) for the SQL database |
| `fabio sql-database show` | no | Show details of a SQL database |
| `fabio sql-database start-mirroring` | yes | Start mirroring for the SQL database |
| `fabio sql-database statistics-create` | yes | Create a user-defined statistic on a column |
| `fabio sql-database statistics-delete` | yes | Delete a user-defined statistic |
| `fabio sql-database statistics-list` | no | List statistics on a SQL database |
| `fabio sql-database statistics-show` | no | Show details of a statistic |
| `fabio sql-database statistics-update` | yes | Update (refresh) an existing statistic |
| `fabio sql-database stop-mirroring` | yes | Stop mirroring for the SQL database |
| `fabio sql-database update` | yes | Update SQL database properties |
| `fabio sql-database update-audit-settings` | yes | Update SQL audit settings for the database |
| `fabio sql-database update-definition` | yes | Update the definition of a SQL database |

### fabio sql-endpoint
Manage SQL endpoints (analytics endpoints for lakehouses)

| Command | Mutates | Description |
|---|---|---|
| `fabio sql-endpoint connection-string` | no | Get the SQL connection string for a SQL endpoint |
| `fabio sql-endpoint describe-table` | no | Describe the columns of a table (from `INFORMATION_SCHEMA.COLUMNS`) |
| `fabio sql-endpoint get-audit-settings` | no | Get SQL audit settings for the endpoint |
| `fabio sql-endpoint list` | no | List SQL endpoints in a workspace |
| `fabio sql-endpoint list-tables` | no | List tables and views in a SQL endpoint (from `INFORMATION_SCHEMA.TABLES`) |
| `fabio sql-endpoint mcp-url` | no | Print the remote Fabric Data Warehouse MCP server URLs (item-scoped + global) for agent consumption |
| `fabio sql-endpoint plan` | no | Capture the estimated execution plan (`SHOWPLAN_XML`) without executing the query |
| `fabio sql-endpoint pool-insights` | no | Report SQL pool state changes and sustained pressure events (from `queryinsights.sql_pool_insights`) |
| `fabio sql-endpoint queries-frequent` | no | List frequently-run queries (from `queryinsights.frequently_run_queries`) |
| `fabio sql-endpoint queries-history` | no | List completed query history (from `queryinsights.exec_requests_history`) |
| `fabio sql-endpoint queries-long-running` | no | List long-running queries (from `queryinsights.long_running_queries`) |
| `fabio sql-endpoint queries-running` | no | List currently running queries on a SQL endpoint |
| `fabio sql-endpoint query` | no | Execute a SQL query against a SQL endpoint |
| `fabio sql-endpoint refresh-metadata` | yes | Refresh metadata for all or selected tables in a SQL endpoint (LRO) |
| `fabio sql-endpoint set-audit-actions` | yes | Set audit actions and groups for the endpoint |
| `fabio sql-endpoint show` | no | Show details of a SQL endpoint |
| `fabio sql-endpoint update-audit-settings` | yes | Update SQL audit settings for the endpoint |

### fabio warehouse-snapshot
Manage warehouse snapshots

| Command | Mutates | Description |
|---|---|---|
| `fabio warehouse-snapshot create` | yes | Create a new warehouse snapshot |
| `fabio warehouse-snapshot delete` | yes | Delete a warehouse snapshot |
| `fabio warehouse-snapshot list` | no | List warehouse snapshots in a workspace |
| `fabio warehouse-snapshot show` | no | Show details of a warehouse snapshot |
| `fabio warehouse-snapshot update` | yes | Update warehouse snapshot properties (name and/or description) |

## Must / Prefer / Avoid
### MUST
- Pick the right surface for the intent: sql-endpoint (read-only over a lakehouse), warehouse (read-write analytics), sql-database (OLTP).
- Provision SQL Database on F4+ capacity (F2 fails with error 18456 State 240).

### PREFER
- 'list-tables' / 'describe-table' for schema discovery instead of hand-written INFORMATION_SCHEMA queries via 'query --sql'.
- 'plan' (SHOWPLAN_XML) to inspect a query's cost before executing it.
- --sql @file.sql or stdin piping for large/multiline queries over inline strings.
- queries-history / queries-long-running for diagnostics instead of ad-hoc DMV queries.
- 'queries-history --label <name>' (with the allocated-CPU + data-scanned-remote/memory/disk columns) to compare labeled executions — e.g. tag queries with OPTION (LABEL='Regular'|'Clustered') to assess clustering effectiveness.
- 'warehouse mcp-url' / 'sql-endpoint mcp-url' when the user wants an external MCP client (VS Code agent mode, Copilot) to connect to the remote Fabric Data Warehouse MCP server; otherwise use fabio's native query/plan/insights for scripted execution.

### AVOID
- Using a Warehouse for OLTP or a SQL Database for heavy analytics — pick the surface that matches the workload.
- Running destructive DDL/DML (DROP/DELETE/TRUNCATE) without confirming with the user first.
- Assuming query monitoring DMVs behave identically to SQL Server (several columns/views differ on Fabric — see gotchas).

## Key gotchas
- sys.dm_exec_requests has no login_name column on Fabric (it lives in sys.dm_exec_sessions).
- sys.dm_db_stats_properties is NOT supported on Lakehouse SQL endpoints.
- 'warehouse query --id <X>' resolves the SQL analytics endpoint of a Warehouse, a WarehouseSnapshot, a MirroredDatabase (open/CDC mirror), a MirroredAzureDatabricksCatalog, AND a Lakehouse — use it to T-SQL query mirrored/snapshot data, not just warehouses.
- TDS date/time columns (DATE/DATETIME2/DATETIMEOFFSET/TIME) render as ISO-8601 strings across warehouse/sql-database/sql-endpoint/lakehouse queries; parse them as ISO-8601, not raw internals.
- A zero-row SELECT returns the LIST envelope {"data":[],"count":0} (a result set with 0 rows), NOT a scalar 'no result set' message; DDL/DML with no result set returns the scalar rows_affected message. Iterate/filter 'data' safely.
- queryinsights.* views populate asynchronously (up to ~15 min after the first queries on a fresh warehouse) — 'Invalid object name queryinsights.*' is initialization lag, not a bug.
- 'warehouse set-retention' sets the time-travel window (1-120 days); DECREASING it is irreversible (background GC permanently drops older history) — it is destructive and dry-run guarded. 'warehouse create --collation' is create-time only (case-sensitive default vs Latin1_General_100_CI_AS_KS_WS_SC_UTF8).
- 'sql-endpoint refresh-metadata --tables' accepts a JSON array of {schema,tableNames} selectors (inline or @file), with at most 25 total tables. Its optional --timeout is a Duration JSON object with numeric value and a PascalCase timeUnit (Seconds, Minutes, Hours, or Days). Schema-enabled endpoints return schema-qualified tableName values; non-schema-enabled endpoints resolve under the default schema.
- 'warehouse mcp-url' / 'sql-endpoint mcp-url' emit the deterministic remote Fabric Data Warehouse MCP server URLs (preview) for external MCP clients (VS Code agent mode, Copilot, Copilot Studio, Azure AI Foundry) — an item-scoped mcpUrl (.../items/{id}/sqlEndpoint, binds to one item) plus a global globalMcpUrl (.../mcp/dataPlane/sqlEndpoint, agent supplies workspace+item per-prompt). That remote server exposes ONE T-SQL execution tool (live tool name 'execute_query'; docs call it 'executeSQL') and NO schema/metadata tools — it's for interactive Copilot SQL authoring. For scripted/composable execution, execution plans, query insights, and statistics, use fabio's native 'warehouse query'/'plan'/'queries-*'/'statistics-*' instead of emitting an MCP URL.
- Because the remote DW MCP server has NO schema tools, prefer fabio's typed 'list-tables' (tables+views, optional --schema) and 'describe-table --table [schema.]table' (columns/types/nullability) over hand-writing INFORMATION_SCHEMA.TABLES/COLUMNS queries via 'query --sql'. Both accept a Warehouse OR Lakehouse/SQL-endpoint id (schema discovery works on any TDS surface). describe-table returns the LIST envelope (one row per column); a count of 0 means the table/schema does not exist. --table accepts 'schema.table', '[schema].[table]', or a bare 'table' (unqualified matches the name across schemas).
- 'warehouse copy-into' bulk-loads files into an EXISTING warehouse table via COPY INTO (the authoring loop: create table with 'query' -> copy-into -> validate with describe-table/COUNT(*)). It is append-only (never deletes/overwrites), so mutates:true but NOT destructive. --source must be an HTTPS Azure storage (*.dfs/blob.core.windows.net) or OneLake (onelake.dfs.fabric.microsoft.com) URL; --file-type is CSV or PARQUET; CSV-only flags (--field-terminator/--row-terminator/--first-row/--encoding) are rejected with PARQUET. Omit --sas-token to use the caller's Entra identity (works for OneLake and storage the signed-in user can read). --dry-run prints the generated COPY INTO SQL with the SAS secret REDACTED; --readonly blocks execution but still allows the dry-run preview. Create the table first — copy-into does not create it.

## Troubleshooting
| Symptom | Fix |
|---|---|
| SQL Database create fails with 18456 State 240 | The workspace capacity is too small; SQL Database requires F4 or higher. |
| Query against a lakehouse endpoint returns stale/missing tables | The SQL analytics endpoint syncs from Delta; ensure the lakehouse tables exist and metadata has refreshed. |
| 'invalid column name login_name' | Join sys.dm_exec_sessions for login_name; sys.dm_exec_requests does not have it on Fabric. |
| FORBIDDEN executing a query | You need appropriate workspace role / SQL permissions on the item. |
| Selective refresh reports DeltaTableNotFound | For schema-enabled parent items, use the table's actual schema. Non-schema-enabled items resolve under the default schema and cannot resolve tables under a non-default schema. |

## Safety
- copy-into loads data into an existing table (append-only). It is a mutation (dry-run guarded, --readonly blocks it); confirm the target table and --source before running. --dry-run prints the COPY INTO SQL with the SAS secret redacted.
- queries-kill terminates a running session (KILL) — confirm the session id and impact with the user.
- DDL/DML via query (DROP/DELETE/TRUNCATE) is executed for real — use 'plan' to inspect without executing, and confirm destructive statements.
- 'set-retention' DECREASING the retention window is irreversible (permanently drops time-travel history older than the new window) — it is destructive and dry-run guarded; confirm with the user.
- 'sql-endpoint refresh-metadata --recreate-tables' drops and recreates the selected SQL metadata tables; inspect the dry-run body and confirm the scope first.

## Shared references
Cross-cutting operational guidance (the "common" layer) — consult the relevant topic before non-trivial work:

| Reference | Covers |
|---|---|
| `fabio context best-practices throttling` | fabio transparently handles 429 (Too Many Requests) and gateway errors. Agents do NOT need to implement retry logic. |
| `fabio context best-practices pagination` | fabio handles pagination via --all (auto-fetch all pages), --continuation-token (resume), and --limit (truncate). Agents rarely need to paginate manually. |
| `fabio context best-practices lro` | Many Fabric operations are async (return 202). fabio polls them automatically. Use --wait for job operations. |

## See also
- fabio context persona data-engineer
- fabio context disambiguate sql-endpoint
- fabio context disambiguate semantic-model
- fabio context blueprint basic-data-analytics
- fabio context persona data-solution-architect
- fabio context blueprint data-analytics-sql-endpoint
