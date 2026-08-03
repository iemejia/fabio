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
- Capturing estimated execution plans (SHOWPLAN_XML) without running the query.
- Monitoring queries: running / frequent / long-running / history; killing a session.
- Managing user-defined statistics (list/show/create/update/delete).
- Creating/managing warehouse snapshots.

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
| `fabio warehouse create` | yes | Create a new warehouse |
| `fabio warehouse create-restore-point` | yes | Create a restore point for a warehouse |
| `fabio warehouse delete` | yes | Delete a warehouse |
| `fabio warehouse delete-restore-point` | yes | Delete a restore point |
| `fabio warehouse get-audit-settings` | no | Get SQL audit settings for a warehouse |
| `fabio warehouse get-sql-pools-config` | no | Get SQL pools configuration for a workspace |
| `fabio warehouse list` | no | List warehouses in a workspace |
| `fabio warehouse list-restore-points` | no | List restore points for a warehouse |
| `fabio warehouse plan` | yes | Capture the estimated execution plan (`SHOWPLAN_XML`) without executing the query |
| `fabio warehouse pool-insights` | yes | Report SQL pool state changes and sustained pressure events (from `queryinsights.sql_pool_insights`) |
| `fabio warehouse queries-frequent` | yes | List frequently-run queries (from `queryinsights.frequently_run_queries`) |
| `fabio warehouse queries-history` | yes | List completed query history (from `queryinsights.exec_requests_history`) |
| `fabio warehouse queries-kill` | yes | Kill a running query session by session ID |
| `fabio warehouse queries-long-running` | yes | List long-running queries (from `queryinsights.long_running_queries`) |
| `fabio warehouse queries-running` | yes | List currently running queries on a warehouse |
| `fabio warehouse query` | yes | Execute a SQL query against a warehouse or SQL endpoint |
| `fabio warehouse restore-to-point` | yes | Restore a warehouse to a restore point |
| `fabio warehouse set-audit-actions` | yes | Set audit actions and groups for a warehouse |
| `fabio warehouse show` | no | Show details of a warehouse |
| `fabio warehouse show-restore-point` | no | Show details of a restore point |
| `fabio warehouse statistics-create` | yes | Create a user-defined statistic on a column |
| `fabio warehouse statistics-delete` | yes | Delete a user-defined statistic |
| `fabio warehouse statistics-list` | yes | List user-defined statistics on a warehouse or SQL endpoint |
| `fabio warehouse statistics-show` | yes | Show details of a statistic (header, density vector, histogram) |
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
| `fabio sql-database plan` | yes | Capture the estimated execution plan (`SHOWPLAN_XML`) without executing the query |
| `fabio sql-database queries-history` | yes | List completed query history |
| `fabio sql-database queries-kill` | yes | Kill a running query session by session ID |
| `fabio sql-database queries-running` | yes | List currently running queries on a SQL database |
| `fabio sql-database query` | yes | Execute a SQL query against a SQL database via TDS |
| `fabio sql-database revalidate-cmk` | yes | Revalidate Customer-Managed Key (CMK) for the SQL database |
| `fabio sql-database show` | no | Show details of a SQL database |
| `fabio sql-database start-mirroring` | yes | Start mirroring for the SQL database |
| `fabio sql-database statistics-create` | yes | Create a user-defined statistic on a column |
| `fabio sql-database statistics-delete` | yes | Delete a user-defined statistic |
| `fabio sql-database statistics-list` | yes | List statistics on a SQL database |
| `fabio sql-database statistics-show` | yes | Show details of a statistic |
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
| `fabio sql-endpoint get-audit-settings` | no | Get SQL audit settings for the endpoint |
| `fabio sql-endpoint list` | no | List SQL endpoints in a workspace |
| `fabio sql-endpoint plan` | yes | Capture the estimated execution plan (`SHOWPLAN_XML`) without executing the query |
| `fabio sql-endpoint pool-insights` | yes | Report SQL pool state changes and sustained pressure events (from `queryinsights.sql_pool_insights`) |
| `fabio sql-endpoint queries-frequent` | yes | List frequently-run queries (from `queryinsights.frequently_run_queries`) |
| `fabio sql-endpoint queries-history` | yes | List completed query history (from `queryinsights.exec_requests_history`) |
| `fabio sql-endpoint queries-long-running` | yes | List long-running queries (from `queryinsights.long_running_queries`) |
| `fabio sql-endpoint queries-running` | yes | List currently running queries on a SQL endpoint |
| `fabio sql-endpoint query` | no | Execute a SQL query against a SQL endpoint |
| `fabio sql-endpoint refresh-metadata` | yes | Refresh metadata for all tables in a SQL endpoint (LRO) |
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
- 'plan' (SHOWPLAN_XML) to inspect a query's cost before executing it.
- --sql @file.sql or stdin piping for large/multiline queries over inline strings.
- queries-history / queries-long-running for diagnostics instead of ad-hoc DMV queries.

### AVOID
- Using a Warehouse for OLTP or a SQL Database for heavy analytics — pick the surface that matches the workload.
- Running destructive DDL/DML (DROP/DELETE/TRUNCATE) without confirming with the user first.
- Assuming query monitoring DMVs behave identically to SQL Server (several columns/views differ on Fabric — see gotchas).

## Key gotchas
- sys.dm_exec_requests has no login_name column on Fabric (it lives in sys.dm_exec_sessions).
- sys.dm_db_stats_properties is NOT supported on Lakehouse SQL endpoints.

## Troubleshooting
| Symptom | Fix |
|---|---|
| SQL Database create fails with 18456 State 240 | The workspace capacity is too small; SQL Database requires F4 or higher. |
| Query against a lakehouse endpoint returns stale/missing tables | The SQL analytics endpoint syncs from Delta; ensure the lakehouse tables exist and metadata has refreshed. |
| 'invalid column name login_name' | Join sys.dm_exec_sessions for login_name; sys.dm_exec_requests does not have it on Fabric. |
| FORBIDDEN executing a query | You need appropriate workspace role / SQL permissions on the item. |

## Safety
- queries-kill terminates a running session (KILL) — confirm the session id and impact with the user.
- DDL/DML via query (DROP/DELETE/TRUNCATE) is executed for real — use 'plan' to inspect without executing, and confirm destructive statements.

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
