---
name: fabio-mirroring
description: >-
  Intent-scoped fabio skill for Fabric database mirroring / real-time replication: mirrored databases, mirrored catalogs (Unity Catalog), mirrored Azure Databricks catalogs, mirrored warehouses, Snowflake databases, and Azure Databricks storage. Use to replicate external sources into OneLake Delta and query them with the SQL endpoint. Triggers: "mirror database", "mirroring", "replicate snowflake", "unity catalog mirror", "databricks catalog mirror", "real-time replication", "mirror status", "start mirroring".
license: MIT
---

# fabio-mirroring — Database Mirroring — replicate Snowflake, Databricks, Cosmos, SQL into OneLake

> **Generated file — do not edit by hand.** This intent-scoped sub-skill of the `fabio` skill is generated from fabio's command schema plus authored judgment. Regenerate with `cargo test generate_subskills -- --ignored`. For install, auth, output envelope, global flags, and agent-safety rules, see the root `fabio` skill.

> **Prefer runtime introspection.** This index is a snapshot; the installed binary is always authoritative. Use `fabio context agent --group <group>` and `fabio context describe <group> <command>` for exact flags and output shapes.

## When to use
- Creating a mirrored database and starting/stopping replication (start, stop, status, table-status).
- Mirroring a Unity Catalog (mirrored-catalog: list-scopes, list-tables, refresh-metadata, mirroring-status).
- Mirroring an Azure Databricks catalog (discover-catalogs/schemas/tables, refresh-metadata).
- Mirroring a Snowflake database into OneLake.
- Wiring Azure Databricks storage integration.

## When NOT to use (route elsewhere)
- You just need to reference external files without replication -> use a OneLake shortcut (fabio-lakehouse).
- One-time bulk copy between Fabric items -> use copy-job (fabio-data-engineering).
- Low-code transform of the data -> use fabio-dataflows.
- Querying the mirrored data with T-SQL -> use fabio-warehouse-sql (the SQL analytics endpoint).

## Command index

Generated from fabio's command schema. For full flag details use `fabio context agent --group <group>` or `fabio context describe <group> <command>`.

### fabio mirrored-database
Manage mirrored databases (real-time replication)

| Command | Mutates | Description |
|---|---|---|
| `fabio mirrored-database create` | yes | Create a new mirrored database |
| `fabio mirrored-database delete` | yes | Delete a mirrored database |
| `fabio mirrored-database get-definition` | no | Get the definition of a mirrored database |
| `fabio mirrored-database landing-zone` | no | Print the OneLake landing-zone URL of an OPEN mirrored database (push data files here) |
| `fabio mirrored-database list` | no | List mirrored databases in a workspace |
| `fabio mirrored-database show` | no | Show details of a mirrored database |
| `fabio mirrored-database start` | yes | Start mirroring |
| `fabio mirrored-database status` | no | Get mirroring status |
| `fabio mirrored-database stop` | yes | Stop mirroring |
| `fabio mirrored-database table-status` | no | Get tables mirroring status |
| `fabio mirrored-database update` | yes | Update mirrored database properties (name and/or description) |
| `fabio mirrored-database update-definition` | yes | Update the definition of a mirrored database |

### fabio mirrored-catalog
Manage mirrored catalogs (Unity Catalog mirroring)

| Command | Mutates | Description |
|---|---|---|
| `fabio mirrored-catalog create` | yes | Create a new mirrored catalog |
| `fabio mirrored-catalog delete` | yes | Delete a mirrored catalog |
| `fabio mirrored-catalog get-definition` | no | Get the definition of a mirrored catalog |
| `fabio mirrored-catalog list` | no | List mirrored catalogs in a workspace |
| `fabio mirrored-catalog list-scopes` | no | List catalog mirroring scopes (workspace-level) |
| `fabio mirrored-catalog list-tables` | no | List catalog mirroring tables (workspace-level) |
| `fabio mirrored-catalog mirroring-status` | no | Get mirroring status |
| `fabio mirrored-catalog refresh-metadata` | yes | Refresh catalog metadata |
| `fabio mirrored-catalog show` | no | Show details of a mirrored catalog |
| `fabio mirrored-catalog tables-mirroring-status` | no | Get tables mirroring status |
| `fabio mirrored-catalog update` | yes | Update mirrored catalog properties (name and/or description) |
| `fabio mirrored-catalog update-definition` | yes | Update the definition of a mirrored catalog |

### fabio mirrored-databricks-catalog
Manage mirrored Azure Databricks catalogs

| Command | Mutates | Description |
|---|---|---|
| `fabio mirrored-databricks-catalog create` | yes | Create a new mirrored Azure Databricks catalog |
| `fabio mirrored-databricks-catalog delete` | yes | Delete a mirrored Azure Databricks catalog |
| `fabio mirrored-databricks-catalog discover-catalogs` | no | Discover available Databricks catalogs (workspace-level) |
| `fabio mirrored-databricks-catalog discover-schemas` | no | Discover schemas in a Databricks catalog |
| `fabio mirrored-databricks-catalog discover-tables` | no | Discover tables in a Databricks catalog schema |
| `fabio mirrored-databricks-catalog get-definition` | no | Get the definition of a mirrored Databricks catalog |
| `fabio mirrored-databricks-catalog list` | no | List mirrored Azure Databricks catalogs in a workspace |
| `fabio mirrored-databricks-catalog refresh-metadata` | yes | Refresh catalog metadata |
| `fabio mirrored-databricks-catalog show` | no | Show details of a mirrored Azure Databricks catalog |
| `fabio mirrored-databricks-catalog update` | yes | Update mirrored Databricks catalog properties (name and/or description) |
| `fabio mirrored-databricks-catalog update-definition` | yes | Update the definition of a mirrored Databricks catalog |

### fabio mirrored-warehouse
Manage mirrored warehouses

| Command | Mutates | Description |
|---|---|---|
| `fabio mirrored-warehouse list` | no | List mirrored warehouses in a workspace |

### fabio snowflake-database
Manage Snowflake databases (mirrored from Snowflake)

| Command | Mutates | Description |
|---|---|---|
| `fabio snowflake-database create` | yes | Create a new Snowflake database |
| `fabio snowflake-database delete` | yes | Delete a Snowflake database |
| `fabio snowflake-database get-definition` | no | Get the definition of a Snowflake database |
| `fabio snowflake-database list` | no | List Snowflake databases in a workspace |
| `fabio snowflake-database show` | no | Show details of a Snowflake database |
| `fabio snowflake-database update` | yes | Update Snowflake database properties |
| `fabio snowflake-database update-definition` | yes | Update the definition of a Snowflake database |

### fabio azure-databricks-storage
Manage Azure Databricks storage items (Fabric integration with Azure Databricks)

| Command | Mutates | Description |
|---|---|---|
| `fabio azure-databricks-storage create` | yes | Create a new Azure Databricks storage item |
| `fabio azure-databricks-storage delete` | yes | Delete an Azure Databricks storage item |
| `fabio azure-databricks-storage get-definition` | no | Get the definition of an Azure Databricks storage item |
| `fabio azure-databricks-storage list` | no | List Azure Databricks storage items in a workspace |
| `fabio azure-databricks-storage show` | no | Show details of an Azure Databricks storage item |
| `fabio azure-databricks-storage update` | yes | Update Azure Databricks storage item properties |
| `fabio azure-databricks-storage update-definition` | yes | Update the definition of an Azure Databricks storage item |

## Must / Prefer / Avoid
### MUST
- Start replication after create (mirrored-database start) and verify with status / table-status.
- Use discover-* (mirrored-databricks-catalog) or list-scopes/list-tables (mirrored-catalog) to scope what gets mirrored.
- Query mirrored data through the auto-provisioned SQL analytics endpoint (sql-endpoint), not the mirror item directly.

### PREFER
- Mirroring for continuously-replicated, queryable copies; a shortcut for in-place reference without a copy; copy-job for a one-off move (see disambiguate mirroring).
- refresh-metadata after upstream schema changes so new tables/columns appear.
- table-status / tables-mirroring-status to monitor per-table replication health.

### AVOID
- Confusing mirroring (continuous replication) with shortcuts (in-place reference) or copy-job (one-time copy).
- Assuming data is queryable immediately — initial snapshot + ongoing sync take time; check status.
- Deleting a mirrored database expecting the source to be affected (it only removes the Fabric replica).

## Key gotchas
- Mirroring is continuous replication into OneLake Delta; the mirror is read-only in Fabric and queried via its SQL analytics endpoint.
- mirrored-warehouse is list-only in fabio (it is provisioned/managed by Fabric, not independently created).
- Catalog mirrors expose discovery + refresh-metadata; the source schema must be re-discovered after upstream DDL.

## Troubleshooting
| Symptom | Fix |
|---|---|
| Mirrored tables are empty or missing | Confirm replication is started (mirrored-database start) and check status/table-status; initial sync may still be running. |
| New upstream tables/columns not showing | Run refresh-metadata (catalog mirrors) or re-check discovery; schema changes need a metadata refresh. |
| Can't write to the mirrored data | Mirrors are read-only in Fabric; write at the source. Query the replica via the SQL analytics endpoint. |
| Unsure whether to mirror, shortcut, or copy | Run 'fabio context disambiguate mirroring'. |

## Safety
- Stopping replication (mirrored-database stop) halts data freshness — confirm downstream consumers.
- Deleting a mirror removes the Fabric replica and its SQL endpoint — irreversible; the source is untouched but re-mirroring re-snapshots.

## Shared references
Cross-cutting operational guidance (the "common" layer) — consult the relevant topic before non-trivial work:

| Reference | Covers |
|---|---|
| `fabio context best-practices throttling` | fabio transparently handles 429 (Too Many Requests) and gateway errors. Agents do NOT need to implement retry logic. |
| `fabio context best-practices pagination` | fabio handles pagination via --all (auto-fetch all pages), --continuation-token (resume), and --limit (truncate). Agents rarely need to paginate manually. |
| `fabio context best-practices lro` | Many Fabric operations are async (return 202). fabio polls them automatically. Use --wait for job operations. |
| `fabio context best-practices shortcuts` | OneLake shortcuts virtualize external/other-OneLake storage into a lakehouse. Create with TYPED flags per target type (9 supported: OneLake, AdlsGen2, AmazonS3, AzureBlobStorage, GoogleCloudStorage, S3Compatible, Dataverse, ExternalDataShare, OneDriveSharePoint); cloud targets need a connection first. list-shortcuts enumerates them (DW-managed internal shortcuts hidden unless --include-managed). load-table resolves shortcut paths even when list-files does not show their contents. |

## See also
- fabio context persona data-engineer
- fabio context disambiguate mirroring
- fabio context disambiguate sql-endpoint
