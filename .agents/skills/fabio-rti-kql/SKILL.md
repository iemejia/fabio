---
name: fabio-rti-kql
description: >-
  Intent-scoped fabio skill for Fabric Real-Time Intelligence: eventhouses, KQL databases (schema, ingest, query, monitoring), KQL querysets/dashboards, eventstreams (streaming topologies), reflex/activator triggers, operations agents (AI monitoring), and natural-language-to-KQL. Use for streaming ingestion, time-series analytics, and event-driven alerts. Triggers: "eventhouse", "kql", "kusto", "kql database", "ingest events", "eventstream", "streaming", "reflex", "activator", "operations agent", "nl to kql", "real-time".
license: MIT
---

# fabio-rti-kql — Real-Time Intelligence — Eventhouse, KQL, Eventstream, Activator

> **Generated file — do not edit by hand.** This intent-scoped sub-skill of the `fabio` skill is generated from fabio's command schema plus authored judgment. Regenerate with `cargo test generate_subskills -- --ignored`. For install, auth, output envelope, global flags, and agent-safety rules, see the root `fabio` skill.

> **Prefer runtime introspection.** This index is a snapshot; the installed binary is always authoritative. Use `fabio context agent --group <group>` and `fabio context describe <group> <command>` for exact flags and output shapes.

## When to use
- Creating an eventhouse and KQL databases inside it.
- KQL schema discovery (list-entities), ingestion, querying, and query monitoring (running/journal/completed).
- Building eventstream topologies (sources, destinations, connection strings).
- Creating reflex/activator triggers for event-driven alerts.
- Authoring AI operations agents that monitor an eventhouse/ontology and recommend actions (operations-agent create/update-definition/start/stop/status).
- Translating natural language to KQL (rti nl-to-kql).

## When NOT to use (route elsewhere)
- Batch Delta lakehouse ETL -> use fabio-lakehouse.
- T-SQL warehouse analytics -> use fabio-warehouse-sql.
- 'materialized view' in a lakehouse context -> that is a Materialized Lake View (fabio-lakehouse), not a KQL materialized view — see 'fabio context disambiguate materialized-view'.

## Command index

Generated from fabio's command schema. For full flag details use `fabio context agent --group <group>` or `fabio context describe <group> <command>`.

### fabio eventhouse
Manage eventhouses (real-time analytics)

| Command | Mutates | Description |
|---|---|---|
| `fabio eventhouse create` | yes | Create a new eventhouse |
| `fabio eventhouse delete` | yes | Delete an eventhouse |
| `fabio eventhouse get-definition` | no | Get the definition of an eventhouse |
| `fabio eventhouse list` | no | List eventhouses in a workspace |
| `fabio eventhouse show` | no | Show details of an eventhouse |
| `fabio eventhouse update` | yes | Update eventhouse properties (name and/or description) |
| `fabio eventhouse update-definition` | yes | Update the definition of an eventhouse |

### fabio kql-database
Manage KQL databases (within eventhouses)

| Command | Mutates | Description |
|---|---|---|
| `fabio kql-database bulk-create-shortcuts` | yes | Bulk-create multiple shortcuts (LRO) |
| `fabio kql-database create` | yes | Create a new KQL database |
| `fabio kql-database create-shortcut` | yes | Create a table shortcut in a KQL database (OneLake/S3/ADLS Gen2/GCS/S3-compatible/Azure Blob) |
| `fabio kql-database deeplink` | no | Generate a deeplink URL for a KQL query in Fabric portal or ADX Web Explorer |
| `fabio kql-database delete` | yes | Delete a KQL database |
| `fabio kql-database delete-shortcut` | yes | Delete a shortcut in a KQL database |
| `fabio kql-database describe` | no | Get schema for all entities in a database |
| `fabio kql-database describe-entity` | no | Get detailed schema for a specific entity (table, view, function) |
| `fabio kql-database diagnostics` | no | Run cluster diagnostics (capacity, health, ingestion failures) |
| `fabio kql-database examples` | no | Retrieve KQL example pairs relevant to a natural-language prompt (via the eventhouse remote MCP server) |
| `fabio kql-database get-definition` | no | Get the definition of a KQL database (KQL script) |
| `fabio kql-database get-shortcut` | no | Get a shortcut in a KQL database |
| `fabio kql-database ingest` | yes | Ingest data into a KQL table — inline (small) or from OneLake/blob storage (large files) |
| `fabio kql-database journal` | no | Show the operations journal (completed operations history) |
| `fabio kql-database list` | no | List KQL databases in a workspace |
| `fabio kql-database list-entities` | no | List entities (tables, materialized views, external tables, functions) in a database |
| `fabio kql-database list-shortcuts` | no | List shortcuts in a KQL database |
| `fabio kql-database mcp-url` | no | Print the remote MCP server URL for consuming this KQL database with AI agents |
| `fabio kql-database queries-completed` | no | Show recently completed queries |
| `fabio kql-database queries-running` | no | Show currently running queries on the KQL database |
| `fabio kql-database query` | no | Execute a KQL query against a KQL database |
| `fabio kql-database sample` | no | Sample rows from a table, materialized view, external table, or function |
| `fabio kql-database schema-context` | no | Retrieve relevant schema context (with column samples + stats) for a natural-language prompt (via the eventhouse remote MCP server) |
| `fabio kql-database show` | no | Show details of a KQL database |
| `fabio kql-database show-queryplan` | no | Show execution plan for a KQL query without running it |
| `fabio kql-database update` | yes | Update KQL database properties (name and/or description) |
| `fabio kql-database update-definition` | yes | Update the definition of a KQL database |

### fabio kql-queryset
Manage KQL querysets (saved KQL queries)

| Command | Mutates | Description |
|---|---|---|
| `fabio kql-queryset add-tab` | yes | Add a saved query tab bound to a KQL database (authors the `RealTimeQueryset.json` data source + tab) |
| `fabio kql-queryset create` | yes | Create a new KQL queryset |
| `fabio kql-queryset delete` | yes | Delete a KQL queryset |
| `fabio kql-queryset get-definition` | no | Get the definition of a KQL queryset |
| `fabio kql-queryset list` | no | List KQL querysets in a workspace |
| `fabio kql-queryset run` | no | Run a saved query tab from the queryset against its configured data source |
| `fabio kql-queryset show` | no | Show details of a KQL queryset |
| `fabio kql-queryset update` | yes | Update KQL queryset properties (name and/or description) |
| `fabio kql-queryset update-definition` | yes | Update the definition of a KQL queryset |

### fabio kql-dashboard
Manage KQL dashboards (real-time dashboards)

| Command | Mutates | Description |
|---|---|---|
| `fabio kql-dashboard create` | yes | Create a new KQL dashboard |
| `fabio kql-dashboard delete` | yes | Delete a KQL dashboard |
| `fabio kql-dashboard get-definition` | no | Get the definition of a KQL dashboard |
| `fabio kql-dashboard list` | no | List KQL dashboards in a workspace |
| `fabio kql-dashboard show` | no | Show details of a KQL dashboard |
| `fabio kql-dashboard update` | yes | Update KQL dashboard properties (name and/or description) |
| `fabio kql-dashboard update-definition` | yes | Update the definition of a KQL dashboard |

### fabio eventstream
Manage eventstreams (real-time data ingestion)

| Command | Mutates | Description |
|---|---|---|
| `fabio eventstream add-derived-stream` | yes | Add a derived stream (filtered/transformed) between existing nodes |
| `fabio eventstream add-destination` | yes | Add a destination to an eventstream (fetches current definition, merges, and updates) |
| `fabio eventstream add-operator` | yes | Add an event-processor operator (Filter/ManageFields/Aggregate/GroupBy/Join/Union/Expand) node |
| `fabio eventstream add-sample-source` | yes | Add a sample data source to an eventstream (high-level helper) |
| `fabio eventstream add-source` | yes | Add a source to an eventstream (fetches current definition, merges, and updates) |
| `fabio eventstream create` | yes | Create a new eventstream |
| `fabio eventstream delete` | yes | Delete an eventstream |
| `fabio eventstream get-definition` | no | Get the definition of an eventstream |
| `fabio eventstream get-destination` | no | Get details of a destination |
| `fabio eventstream get-destination-connection` | no | Get a destination endpoint connection (`CustomEndpoint` or future endpoint shape) |
| `fabio eventstream get-source` | no | Get details of a source |
| `fabio eventstream get-source-connection` | no | Get a source endpoint connection (`CustomEndpoint` for custom sources; `KafkaEndpoint` for Cribl/SAP Datasphere). The response contains credentials (access keys or connection strings) and must not be logged |
| `fabio eventstream get-topology` | no | Get the topology of an eventstream |
| `fabio eventstream list` | no | List eventstreams in a workspace |
| `fabio eventstream list-components` | no | List available eventstream component types (sources, destinations, operators) |
| `fabio eventstream pause` | yes | Pause the entire eventstream |
| `fabio eventstream pause-destination` | yes | Pause a destination |
| `fabio eventstream pause-source` | yes | Pause a source |
| `fabio eventstream resume` | yes | Resume the entire eventstream |
| `fabio eventstream resume-destination` | yes | Resume a destination |
| `fabio eventstream resume-source` | yes | Resume a source |
| `fabio eventstream show` | no | Show details of an eventstream |
| `fabio eventstream update` | yes | Update eventstream properties (name and/or description) |
| `fabio eventstream update-definition` | yes | Update the definition of an eventstream |
| `fabio eventstream validate` | no | Validate an eventstream definition (client-side checks, no API call) |

### fabio reflex
Manage Reflex items (Data Activator triggers and alerts)

| Command | Mutates | Description |
|---|---|---|
| `fabio reflex configure-kql-source` | yes | Configure a KQL data source (portal-only operation) |
| `fabio reflex create` | yes | Create a new reflex |
| `fabio reflex create-rule` | yes | Create a monitoring rule in an existing reflex (via the Activator MCP server) |
| `fabio reflex create-trigger` | yes | Create a trigger with auto-generated Reflex definition (KQL source + email/Teams alert) |
| `fabio reflex delete` | yes | Delete a reflex |
| `fabio reflex delete-rule` | yes | Delete a monitoring rule (via the Activator MCP server) — irreversible |
| `fabio reflex get-definition` | no | Get the definition of a reflex |
| `fabio reflex list` | no | List reflexes in a workspace |
| `fabio reflex list-rules` | no | List monitoring rules in a reflex (via the Activator MCP server) |
| `fabio reflex mcp-url` | no | Print the Activator remote MCP server URL for this reflex |
| `fabio reflex rule-activations` | no | Show the activation (fired-alert) history for a rule (via the Activator MCP server) |
| `fabio reflex show` | no | Show details of a reflex |
| `fabio reflex start-rule` | yes | Start (enable) a monitoring rule (via the Activator MCP server) |
| `fabio reflex stop-rule` | yes | Stop (disable) a monitoring rule (via the Activator MCP server) |
| `fabio reflex update` | yes | Update reflex properties (name and/or description) |
| `fabio reflex update-definition` | yes | Update the definition of a reflex |

### fabio rti
Real-Time Intelligence copilot (NL-to-KQL)

| Command | Mutates | Description |
|---|---|---|
| `fabio rti nl-to-kql` | no | Convert natural language to a KQL query (beta) |

### fabio event-schema-set
Manage event schema sets (real-time intelligence)

| Command | Mutates | Description |
|---|---|---|
| `fabio event-schema-set create` | yes | Create a new event schema set |
| `fabio event-schema-set delete` | yes | Delete a event schema set |
| `fabio event-schema-set get-definition` | no | Get the definition of a event schema set |
| `fabio event-schema-set list` | no | List event schema sets in a workspace |
| `fabio event-schema-set show` | no | Show details of a event schema set |
| `fabio event-schema-set update` | yes | Update event schema set properties |
| `fabio event-schema-set update-definition` | yes | Update the definition of a event schema set |

### fabio operations-agent
Manage operations agents (AI-powered operations)

| Command | Mutates | Description |
|---|---|---|
| `fabio operations-agent create` | yes | Create a new operations agent |
| `fabio operations-agent delete` | yes | Delete a operations agent |
| `fabio operations-agent get-definition` | no | Get the definition of a operations agent |
| `fabio operations-agent list` | no | List operations agents in a workspace |
| `fabio operations-agent show` | no | Show details of a operations agent |
| `fabio operations-agent start` | yes | Start the operations agent so it evaluates its rules (sets shouldRun=true) |
| `fabio operations-agent status` | no | Show whether the operations agent is running (reads shouldRun from its definition) |
| `fabio operations-agent stop` | yes | Stop the operations agent so it stops evaluating its rules (sets shouldRun=false) |
| `fabio operations-agent update` | yes | Update operations agent properties |
| `fabio operations-agent update-definition` | yes | Update the definition of a operations agent |

## Must / Prefer / Avoid
### MUST
- Create the eventhouse first; kql-database create requires --eventhouse-id.
- Discover schema with kql-database list-entities before querying unknown tables.
- Use kql-database manage for .create/.create-or-alter management commands (not query).

### PREFER
- rti nl-to-kql to draft a query, then verify the generated KQL before running it.
- eventstream for continuous ingestion over repeated manual batch ingest.
- list-entities / describe for schema discovery over guessing table/column names.

### AVOID
- Confusing a KQL materialized view with a lakehouse Materialized Lake View (see context disambiguate materialized-view).
- Creating reflex actions that alert/mutate without human confirmation of the rule.
- Assuming KQL uses the standard Fabric scope (it uses {kusto_uri}/.default; fabio handles it).

## Key gotchas
- KQL Queryset definitions use RealTimeQueryset.json (NOT RawQueryset.kql).
- KQL queries use a separate auth scope ({kusto_uri}/.default), not the standard Fabric scope (fabio handles this).
- Operations agents have no dedicated start/stop endpoint: activation is the shouldRun flag inside Configurations.json. Use operations-agent start/stop/status (fabio flips the flag for you) instead of hand-editing the definition.
- Reflex/Activator: `reflex create` makes the item; individual monitoring rules live on the Activator MCP server (no REST API). Create rules RELIABLY with `reflex create-rule` (typed flags for the common KQL+email/Teams case, or `--rule @file.json` for full control incl. Ontology sources) — prefer it over `reflex create-trigger` (REST, fragile). Manage rules with `reflex list-rules` / `start-rule` / `stop-rule` / `delete-rule` / `rule-activations` (fabio drives the MCP server), or print `reflex mcp-url` for an agent to author rules via natural language. CHANGE conditions (increasesAbove/decreasesBelow) require --split-column; STATE conditions (isGreaterThan) do not. Rules support KQL sources (ADX cluster or eventhouse) and email/Teams actions only.
- eventstream add-operator: --input-node must be the source's DEFAULT STREAM (named `<source>-stream`, see get-topology), NOT the source name. A Filter condition uses `operatorType` (not `operator`) and its value.dataType enum is BigInt|Float|Nvarchar(max)|DateTime|Bit|Record|Array|Any (use BigInt for integers, not `Number`/`Int`).
- ReferenceLakehouse eventstream sources are point-in-time Delta-table snapshots for enrichment. Set workspaceId, itemId, absoluteOneLakePath (the IDs must match its first two path segments), optional referencedColumns, and optional refreshRate in hh:mm:ss (<24h; omitted/00:00:00 is static). They can feed only Join (or SQL when reference input is enabled): repeat --input-node for the streaming input first and reference input second, provide joinOn keys, omit duration, and use LeftOuter to preserve the streaming side.
- eventstream get-source-connection is type-discriminated: CustomEndpoint returns Event Hub namespace/name/access keys; Cribl and SAPDatasphere return KafkaEndpoint with bootstrapServer, topicName, securityProtocol (SASL_PLAINTEXT|PLAINTEXT|SASL_SSL|SSL), and OAuthBearer/Plain authentications. The response contains credentials and must not be logged.
- eventstream add-destination --destination-type Eventhouse needs per-ingestion-mode --properties: ProcessedIngestion (pure-REST, self-contained) needs {workspaceId,itemId,databaseName,tableName,inputSerialization} -> status Running; DirectIngestion needs {workspaceId,itemId,connectionName,mappingRuleName} referencing a PRE-EXISTING Kusto data connection -> status External. The WRONG shape is accepted then silently sits in status:Warning and ingests 0 rows — fabio validates offline and teaches the correct shape.
- kql-database create-shortcut REQUIRES --enable-query-acceleration and supports only the 6 STORAGE targets (OneLake, AmazonS3, AdlsGen2, GoogleCloudStorage, S3Compatible, AzureBlobStorage) — Dataverse/ExternalDataShare/OneDriveSharePoint are rejected offline. Use the typed flags (--target-type/--connection-id/--location/...) instead of hand-writing the target JSON.
- kql-queryset add-tab --kql-database <kqlDbId> authors a tab bound to a Fabric KQL DB without hand-crafting RealTimeQueryset.json (it resolves queryServiceUri + display name and uses a type:'Fabric' data source with databaseItemName). A Fabric data source that omits databaseItemName makes 'run' fall back to NetDefaultDB and fail with EntityNotFound.
- 'kql-database query' auto-routes a leading-SELECT (T-SQL) to the Kusto SQL endpoint; '.'-prefixed commands go to /mgmt. eventhouse create --min-consumption-units sets an always-on minimum capacity; a KQL data source in a queryset/reflex uses the KQL DATABASE item id (queryServiceUri), not the eventhouse id.

## Troubleshooting
| Symptom | Fix |
|---|---|
| kql-database create fails | Pass --eventhouse-id of an existing eventhouse; the KQL DB must live inside one. |
| Query returns 'table not found' | Run kql-database list-entities to confirm the exact table name and casing. |
| A .create command has no effect via query | Management commands must go through kql-database manage, which routes to the mgmt endpoint. |
| AUTH errors on KQL queries | KQL uses a separate token scope ({kusto_uri}/.default); re-run fabio auth login if the cached token lacks it. |
| Operations agent isn't evaluating rules / not sending alerts | Run operations-agent status to check shouldRun; start it with operations-agent start (Fabric evaluates rules every 5 minutes only while running). |

## Safety
- Reflex/activator triggers can take automated actions — confirm the rule, threshold, and action with the user before creating.
- KQL management commands that alter retention/caching/policies change data lifecycle — confirm before running.

## Shared references
Cross-cutting operational guidance (the "common" layer) — consult the relevant topic before non-trivial work:

| Reference | Covers |
|---|---|
| `fabio context best-practices throttling` | fabio transparently handles 429 (Too Many Requests) and gateway errors. Agents do NOT need to implement retry logic. |
| `fabio context best-practices pagination` | fabio handles pagination via --all (auto-fetch all pages), --continuation-token (resume), and --limit (truncate). Agents rarely need to paginate manually. |
| `fabio context best-practices lro` | Many Fabric operations are async (return 202). fabio polls them automatically. Use --wait for job operations. |
| `fabio context best-practices workspace-monitoring` | Workspace Monitoring provisions a read-only monitoring Eventhouse/KQL database that collects diagnostic logs + metrics from Fabric items (eventstreams, pipelines, copy jobs, GraphQL APIs, semantic models, eventhouses, mirrored databases, Real-Time hub job events). There is NO public REST API to enable/configure it or the per-eventstream 'Log Eventstream activity' toggle — both are PORTAL-ONLY. Enablement is gated by the tenant setting PlatformMonitoringTenantSetting (fabio can toggle THAT). Once enabled, query the monitoring tables with fabio's existing 'kql-database query' — there is no 'fabio workspace-monitoring' command and there does not need to be. |

## See also
- fabio context persona rti-engineer
- fabio context workflow rti-pipeline
- fabio context workflow eventstream-workspace-identity
- fabio context disambiguate materialized-view
- fabio context blueprint event-analytics
- fabio context blueprint lambda
- fabio context persona data-solution-architect
- fabio context blueprint event-medallion
