---
name: fabio-migration
description: >-
  Intent-scoped fabio skill for migrating existing analytics workloads to Microsoft Fabric from Azure Synapse, Databricks, HDInsight, and Azure Data Factory. Covers assessment, API/path translation, target setup, deploy, and parity validation. Use when porting notebooks, jobs, or pipelines onto Fabric. Triggers: "migrate to fabric", "synapse to fabric", "databricks to fabric", "hdinsight to fabric", "port notebooks", "mssparkutils", "dbutils", "unity catalog", "dbfs", "linked service".
license: MIT
---

# fabio-migration — Migration — port Synapse, Databricks, HDInsight, and ADF to Fabric

> **Generated file — do not edit by hand.** This intent-scoped sub-skill of the `fabio` skill is generated from fabio's command schema plus authored judgment. Regenerate with `cargo test generate_subskills -- --ignored`. For install, auth, output envelope, global flags, and agent-safety rules, see the root `fabio` skill.

> **Prefer runtime introspection.** This index is a snapshot; the installed binary is always authoritative. Use `fabio context agent --group <group>` and `fabio context describe <group> <command>` for exact flags and output shapes.

## When to use
- Assessing a source workspace and mapping its artifacts to Fabric items.
- Translating utility APIs and storage paths (see best-practices migration-api-shims).
- Setting up the target environment (workspace, lakehouse, connections, variable libraries).
- Deploying migrated artifacts via stateless content-hash CI/CD and validating parity.
- Follow the workflow for the specific source: synapse-migration, databricks-migration, hdinsight-migration, pipeline-migration.

## When NOT to use (route elsewhere)
- Greenfield build with no source system -> use the data-engineer persona.
- Ongoing CI/CD of already-migrated items -> use fabio-deploy-cicd.

## Command index

Generated from fabio's command schema. For full flag details use `fabio context agent --group <group>` or `fabio context describe <group> <command>`.

### fabio connection
Manage connections (cloud, on-premises, virtual network)

| Command | Mutates | Description |
|---|---|---|
| `fabio connection add-role-assignment` | yes | Add a role assignment to a connection |
| `fabio connection create` | yes | Create a new connection |
| `fabio connection delete` | yes | Delete a connection |
| `fabio connection delete-role-assignment` | yes | Delete a role assignment from a connection |
| `fabio connection find-duplicates` | no | Find duplicate connections — multiple connections that reach the same target (same type, path, connectivity type, and gateway). Reports the redundant connections (keeping the most-recently-used one) as candidates for consolidation. Read-only |
| `fabio connection find-single-owner` | no | Find connections whose only Owner is a single individual user — an orphan risk if that user leaves the organization. Prefer adding a Microsoft Entra group as a second owner. Read-only |
| `fabio connection find-stale` | no | Find stale connections (never bound to an item, or whose credentials haven't been used recently) — a governance aid for reducing connection sprawl. Read-only: reports candidates for review, does not delete |
| `fabio connection list` | no | List all connections you have permission to access |
| `fabio connection list-role-assignments` | no | List role assignments for a connection |
| `fabio connection list-supported-types` | no | List supported connection types (gateway types catalog) |
| `fabio connection show` | no | Show details of a specific connection |
| `fabio connection show-role-assignment` | no | Show a specific role assignment for a connection |
| `fabio connection test-connection` | no | Test a connection (not supported for `StreamingVirtualNetworkGateway` connections) |
| `fabio connection update` | yes | Update a connection's name, credentials, or privacy level |
| `fabio connection update-role-assignment` | yes | Update a role assignment for a connection |

### fabio variable-library
Manage variable libraries (shared variables)

| Command | Mutates | Description |
|---|---|---|
| `fabio variable-library activate-value-set` | yes | Activate a value set for a variable library in a workspace |
| `fabio variable-library create` | yes | Create a new variable library |
| `fabio variable-library delete` | yes | Delete a variable library |
| `fabio variable-library get-definition` | no | Get the definition of a variable library |
| `fabio variable-library list` | no | List variable librarys in a workspace |
| `fabio variable-library list-value-sets` | no | List value sets defined in a variable library |
| `fabio variable-library show` | no | Show details of a variable library |
| `fabio variable-library update` | yes | Update variable library properties |
| `fabio variable-library update-definition` | yes | Update the definition of a variable library |

### fabio deploy
Deploy item definitions from a local directory to a workspace

| Command | Mutates | Description |
|---|---|---|
| `fabio deploy apply` | yes | Execute deployment (create/update/delete items) |
| `fabio deploy export` | no | Export workspace item definitions to a local directory |
| `fabio deploy init-params` | no | Generate a parameters.json scaffold by scanning or diffing exported definitions |
| `fabio deploy plan` | no | Preview what would be deployed (create/update/delete/skip) |
| `fabio deploy rebind` | yes | Rewrite environment-specific IDs in local definition files, in place (offline) |
| `fabio deploy validate` | no | Validate source directory locally (no API calls). Checks .platform files, item types, duplicate names/logical IDs, cross-references, and parameters |

## Must / Prefer / Avoid
### MUST
- Translate utility APIs (mssparkutils/dbutils -> notebookutils) and storage paths (DBFS/WASB/ADLS/S3 -> OneLake).
- Map Linked Services -> Connections and global parameters -> Variable Library value sets.
- Validate parity (row counts, schema, sample aggregates) before cutover.

### PREFER
- workspace clone / deploy export for bulk moves over recreating items by hand.
- Lakehouse shortcuts to keep large source data in place instead of re-copying everything.
- Variable libraries for environment-specific config instead of hardcoded IDs.

### AVOID
- Decommissioning the source before Fabric parity is validated.
- One-shot 'deploy apply --force-all' without a reviewed plan.
- Trying to migrate Spark pool/cluster sizing (Fabric manages compute per capacity).

## Key gotchas
- Delta tables are cross-compatible between platforms — you can often repoint to existing Parquet/Delta rather than re-writing data.
- %run magic and job-cluster configs have no direct equivalent; refactor shared code and let Fabric manage compute.

## Troubleshooting
| Symptom | Fix |
|---|---|
| Notebook fails: 'mssparkutils'/'dbutils' not defined | Replace with notebookutils.* (see context best-practices migration-api-shims). |
| Path not found (dbfs:/, wasb://, abfss://...core.windows.net) | Repoint to OneLake abfss paths or create a Lakehouse shortcut to the source. |
| Table exists in source but not in Fabric | Map Unity Catalog / Hive tables to lakehouse Delta tables; Delta is cross-compatible (often just repoint). |
| Pipeline references a missing connection | Recreate the Linked Service as a Fabric Connection and resolve it via deploy init-params --resolve-connections. |

## Safety
- STOP after assessment: present feature-parity gaps and get user sign-off on the target architecture before writing code.
- Do NOT decommission source systems until parity validation passes.
- Deploy migrated items with 'deploy plan' (dry-run) reviewed before 'deploy apply'.

## Shared references
Cross-cutting operational guidance (the "common" layer) — consult the relevant topic before non-trivial work:

| Reference | Covers |
|---|---|
| `fabio context best-practices migration-api-shims` | Translation table for porting Synapse, Databricks, and HDInsight code to Fabric: utility APIs (mssparkutils/dbutils -> notebookutils), storage paths (DBFS/WASB/ADLS -> OneLake), catalogs, and orchestration. |
| `fabio context best-practices fabric-cicd-migration` | Guide for teams migrating from Microsoft's fabric-cicd Python library to fabio's deploy commands. Shows the equivalent config mappings, parameter format translation, and additional capabilities available in fabio. |
| `fabio context best-practices variable-libraries` | Variable libraries are Microsoft's strategic Fabric capability for managing environment-specific settings across dev/test/prod. They store parameterized values (connection strings, paths, IDs) that items read at runtime, eliminating hardcoded environment references from item definitions. |

## See also
- fabio context persona migration-engineer
- fabio context best-practices migration-api-shims
- fabio context workflow synapse-migration
- fabio context workflow databricks-migration
- fabio context workflow hdinsight-migration
- fabio context workflow pipeline-migration
