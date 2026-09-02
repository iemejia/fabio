---
name: fabio-app-dev
description: >-
  Intent-scoped fabio skill for building application and API workloads on Microsoft Fabric: AI data agents (natural-language Q&A over data), GraphQL data APIs, User Data Functions (serverless), app backends (Power Apps), Cosmos DB databases (NoSQL OLTP), and Organizational Apps for distribution. Use to expose, query, and package Fabric data as apps/APIs/agents. Triggers: "data agent", "graphql api", "user data function", "serverless function", "app backend", "power app", "cosmos db", "organizational app", "publish app", "nl q&a".
license: MIT
---

# fabio-app-dev — Application Development — data agents, data APIs, functions, org apps

> **Generated file — do not edit by hand.** This intent-scoped sub-skill of the `fabio` skill is generated from fabio's command schema plus authored judgment. Regenerate with `cargo test generate_subskills -- --ignored`. For install, auth, output envelope, global flags, and agent-safety rules, see the root `fabio` skill.

> **Prefer runtime introspection.** This index is a snapshot; the installed binary is always authoritative. Use `fabio context agent --group <group>` and `fabio context describe <group> <command>` for exact flags and output shapes.

## When to use
- Building an AI Data Agent for natural-language Q&A over lakehouse/warehouse data (add datasources, select tables, few-shots, publish, query).
- Exposing Fabric data over a GraphQL API item.
- Authoring/invoking serverless User Data Functions (invoke a published function via its public URL).
- Backing a Power App / data app (app-backend) or a transactional Cosmos DB (NoSQL).
- Packaging and distributing items as an Organizational App with audiences (org-app / org-app-audience).

## When NOT to use (route elsewhere)
- Building/transforming the underlying data (lakehouse/warehouse/pipelines) -> use fabio-data-engineering / fabio-lakehouse.
- Relational OLTP (SQL Database) or T-SQL analytics -> use fabio-warehouse-sql.
- Reports / semantic models -> use fabio-bi.
- Real-time operations agents (RTI monitoring) -> use fabio-rti-kql (operations-agent lives there).

## Command index

Generated from fabio's command schema. For full flag details use `fabio context agent --group <group>` or `fabio context describe <group> <command>`.

### fabio data-agent
Manage data agents (create, query, and interact with AI agents)

| Command | Mutates | Description |
|---|---|---|
| `fabio data-agent add-datasource` | yes | Add a data source to the agent (auto-discovers schema from artifact) |
| `fabio data-agent add-fewshot` | yes | Add a few-shot example (question/query pair) to a data source |
| `fabio data-agent clear-fewshots` | yes | Delete all few-shot examples for a data source |
| `fabio data-agent create` | yes | Create a new data agent |
| `fabio data-agent delete` | yes | Delete a data agent |
| `fabio data-agent delete-element` | yes | Delete a stale schema element (only elements no longer in the live schema) |
| `fabio data-agent describe-element` | yes | Set or clear a description on a table or column in a data source |
| `fabio data-agent evaluate` | no | Batch-run a set of questions against a published data agent (evaluation primitive) |
| `fabio data-agent get-config` | no | Get the configuration of a data agent (instructions, data sources, preview runtime) |
| `fabio data-agent get-definition` | no | Get the definition of a data agent (configuration, data sources, etc.) |
| `fabio data-agent list` | no | List data agents in a workspace |
| `fabio data-agent list-datasources` | no | List configured data sources for a data agent |
| `fabio data-agent list-elements` | no | List elements (tables, columns) in a data source with selection state and descriptions |
| `fabio data-agent list-fewshots` | no | List few-shot examples for a data source |
| `fabio data-agent mcp-url` | no | Print the Model Context Protocol (MCP) endpoint URL for consuming a published data agent |
| `fabio data-agent publish` | yes | Publish a data agent (promotes draft configuration to published state) |
| `fabio data-agent query` | no | Query (chat with) a published data agent using natural language |
| `fabio data-agent remove-datasource` | yes | Remove a data source from the agent |
| `fabio data-agent remove-fewshot` | yes | Remove a few-shot example by ID |
| `fabio data-agent reset` | yes | Reset staging (discard all draft changes, revert to published state) |
| `fabio data-agent select-tables` | yes | Select or unselect data-source elements (tables, or ontology/graph entity types) |
| `fabio data-agent show` | no | Show details of a data agent |
| `fabio data-agent show-datasource` | no | Show details of a configured data source |
| `fabio data-agent show-fewshot` | no | Show a specific few-shot example by ID |
| `fabio data-agent update` | yes | Update a data agent (name and/or description) |
| `fabio data-agent update-config` | yes | Update the configuration of a data agent (instructions, preview runtime) |
| `fabio data-agent update-datasource` | yes | Update a data source's metadata (instructions, description) |
| `fabio data-agent update-definition` | yes | Update the definition of a data agent (configure data sources, instructions, etc.) |
| `fabio data-agent update-fewshot` | yes | Update an existing few-shot example (question and/or query) |
| `fabio data-agent upload-fewshots` | yes | Bulk upload few-shot examples from a JSON or CSV file |
| `fabio data-agent validate-fewshots` | no | Validate a data source's few-shot examples with an LLM (duplicates, conflicts, quality) |

### fabio graphql-api
Manage GraphQL APIs

| Command | Mutates | Description |
|---|---|---|
| `fabio graphql-api create` | yes | Create a new GraphQL API |
| `fabio graphql-api delete` | yes | Delete a GraphQL API |
| `fabio graphql-api get-definition` | no | Get the definition of a GraphQL API |
| `fabio graphql-api list` | no | List GraphQL APIs in a workspace |
| `fabio graphql-api query` | no | Execute a GraphQL query against a GraphQL API |
| `fabio graphql-api show` | no | Show details of a GraphQL API |
| `fabio graphql-api update` | yes | Update GraphQL API properties (name and/or description) |
| `fabio graphql-api update-definition` | yes | Update the definition of a GraphQL API |

### fabio user-data-function
Manage user data functions

| Command | Mutates | Description |
|---|---|---|
| `fabio user-data-function create` | yes | Create a new user data function |
| `fabio user-data-function delete` | yes | Delete a user data function |
| `fabio user-data-function get-definition` | no | Get the definition of a user data function |
| `fabio user-data-function invoke` | yes | Invoke a published function via its public REST endpoint |
| `fabio user-data-function list` | no | List user data functions in a workspace |
| `fabio user-data-function show` | no | Show details of a user data function |
| `fabio user-data-function update` | yes | Update user data function properties |
| `fabio user-data-function update-definition` | yes | Update the definition of a user data function |

### fabio app-backend
Manage app backends (Power Apps backend services) [preview]

| Command | Mutates | Description |
|---|---|---|
| `fabio app-backend create` | yes | Create a new app backend |
| `fabio app-backend delete` | yes | Delete an app backend |
| `fabio app-backend list` | no | List app backends in a workspace |
| `fabio app-backend show` | no | Show details of an app backend |
| `fabio app-backend update` | yes | Update app backend properties (name and/or description) |

### fabio cosmos-db-database
Manage Cosmos DB databases (mirrored from Azure Cosmos DB)

| Command | Mutates | Description |
|---|---|---|
| `fabio cosmos-db-database create` | yes | Create a new Cosmos DB database |
| `fabio cosmos-db-database create-container` | yes | Create a container in a Cosmos DB database (data-plane) |
| `fabio cosmos-db-database delete` | yes | Delete a Cosmos DB database |
| `fabio cosmos-db-database delete-container` | yes | Delete a container and all its documents (data-plane, irreversible) |
| `fabio cosmos-db-database get-definition` | no | Get the definition of a Cosmos DB database |
| `fabio cosmos-db-database import` | yes | Bulk import documents from JSONL/JSON into a container (data-plane, upsert by default) |
| `fabio cosmos-db-database list` | no | List Cosmos DB databases in a workspace |
| `fabio cosmos-db-database list-containers` | no | List containers in a Cosmos DB database (data-plane) |
| `fabio cosmos-db-database query` | no | Run a query against a container (Cosmos DB data-plane) |
| `fabio cosmos-db-database show` | no | Show details of a Cosmos DB database |
| `fabio cosmos-db-database update` | yes | Update Cosmos DB database properties |
| `fabio cosmos-db-database update-definition` | yes | Update the definition of a Cosmos DB database |

### fabio org-app
Manage org apps (organizational Power Apps)

| Command | Mutates | Description |
|---|---|---|
| `fabio org-app create` | yes | Create a new org app |
| `fabio org-app delete` | yes | Delete an org app |
| `fabio org-app get-definition` | no | Get the definition of an org app |
| `fabio org-app list` | no | List org apps in a workspace |
| `fabio org-app show` | no | Show details of an org app |
| `fabio org-app update` | yes | Update org app properties (name and/or description) |
| `fabio org-app update-definition` | yes | Update the definition of an org app |

### fabio org-app-audience
Manage org app audiences (audience definitions for org apps)

| Command | Mutates | Description |
|---|---|---|
| `fabio org-app-audience create` | yes | Create a new org app audience |
| `fabio org-app-audience delete` | yes | Delete an org app audience |
| `fabio org-app-audience get-definition` | no | Get the definition of an org app audience |
| `fabio org-app-audience list` | no | List org app audiences in a workspace |
| `fabio org-app-audience show` | no | Show details of an org app audience |
| `fabio org-app-audience update` | yes | Update org app audience properties (name and/or description) |
| `fabio org-app-audience update-definition` | yes | Update the definition of an org app audience |

## Must / Prefer / Avoid
### MUST
- Wire an API/function/agent to a data store that ALREADY exists and is populated.
- For a Data Agent, configure datasources + select-tables (+ few-shots) BEFORE publish — only a published agent is queryable.
- Invoke a User Data Function only after it is published; pass its portal-copied public URL to 'user-data-function invoke --url'.

### PREFER
- A Data Agent for NL Q&A over Fabric data instead of hand-building query logic; ground it on a lakehouse/warehouse/ontology via add-datasource.
- Evaluate Data Agent answers with 'data-agent evaluate' (add --llm-* for a judge model) before relying on the agent; query a published agent with 'data-agent query'.
- GraphQL API / User Data Functions over bespoke external services when the data lives in Fabric.
- Runtime introspection (context agent --group data-agent|graphql-api|user-data-function) for exact flags.

### AVOID
- Exposing an API/agent over a store that is not yet populated.
- Publishing a Data Agent before validating its answers (data-agent evaluate / validate-fewshots).
- Assuming a User Data Function is REST-discoverable — Fabric exposes no API to list/invoke it; the public URL is copied from the portal.

## Key gotchas
- Only a PUBLISHED Data Agent is queryable. Runtime consumption goes through the agent's Model Context Protocol (MCP) endpoint ({fabricBase}/mcp/workspaces/{ws}/dataagents/{id}/agent) — 'data-agent query' initializes the MCP session, discovers the single query tool, calls it, and returns the answer (pass --published-url only to target a specific MCP URL; 'data-agent mcp-url' prints it). The OpenAI Assistants API that previously backed this path was retired by OpenAI on 2026-08-26.
- Data Agent preview runtime is toggled via 'update-config --enable-preview-runtime'. It selects the multi-step-reasoning variant of EVERY built-in query tool: Advanced NL2SQL for SQL sources AND Advanced DAX generation (preview) for semantic-model sources (iterative reasoning, ambiguity resolution, instance-value indexing for accurate filters). A published agent's runtime is fixed at publish time (republish to change). Advanced DAX generation's instance-value indexing needs the semantic model's Q&A setting enabled (default on for Import/Direct Lake). For semantic models the DAX tool IGNORES data-agent-level instructions — model-specific guidance must live in Power BI 'Prep for AI' (portal/Desktop-only); see context best-practices semantic-model-optimization.
- The MCP consumption surface returns the answer text only. Per-step SQL/DAX/KQL introspection, multi-turn threads, answer-file downloads, and chart-spec extraction are NOT available — those were artifacts of the retired Assistants API. Use 'data-agent query --raw' to inspect the full MCP tool result.
- 'user-data-function invoke' needs a PUBLISHED function with public access enabled in the portal; fabio SSRF-guards the URL (HTTPS + trusted Microsoft host) and attaches the Fabric bearer token.
- Cosmos DB and SQL Database backends require F4+ capacity.
- Cosmos DB has a DATA-PLANE surface beyond item CRUD: 'list-containers', 'create-container' (autoscale-only — Fabric rejects manual/no throughput; fabio always sends the autoscale max, default 1000 RU/s), 'delete-container' (irreversible — drops all documents), 'query' (Cosmos NoSQL via --query-text/@file/stdin; cross-partition is automatic unless --partition-key is given; --parameter name=value binds @params; RU cost in verbose), and 'import' (bulk JSONL/JSON-array, UPSERT by default = idempotent, partition key auto-derived from the container's partitionKey path — pass --continue-on-error to skip bad rows). The Cosmos database name == the item display name; the endpoint is resolved from the item's properties.serverFqdn (override with --endpoint). Auth uses the https://cosmos.azure.com/.default scope.
- app-backend has aliases (rayfin-app, data-app); org-app distribution pairs org-app with org-app-audience.
- Data agent query languages are chosen by the DATA SOURCE type, and the agent generates the query itself: Lakehouse/Warehouse/SQLDatabase/MirroredDatabase -> NL2SQL, KQLDatabase -> NL2KQL, SemanticModel -> NL2DAX (Power BI datasets), Ontology/GraphModel -> graph. To do NL2DAX, `add-datasource --artifact-type SemanticModel`. CRITICAL: `add-datasource` does NOT auto-select a semantic model's TABLES (only columns) — you MUST `select-tables` for the source or the agent hallucinates instead of generating DAX. Re-`publish` after changing sources/selection (published config is a snapshot).

## Troubleshooting
| Symptom | Fix |
|---|---|
| data-agent add-datasource fails 'BadRequest: Failed to fetch schema for the data source' for a Lakehouse/Warehouse/SQLDatabase/MirroredDatabase | The data-agent server fetches a SQL source's schema on-behalf-of YOUR token, and fabio's own device-code login token is not authorized for that SQL-endpoint exchange. Run with an Azure-CLI Fabric token: FABIO_ACCESS_TOKEN=$(az account get-access-token --resource https://api.fabric.microsoft.com --query accessToken -o tsv) fabio data-agent add-datasource ... . Non-SQL sources (KQLDatabase/SemanticModel/GraphModel) are unaffected. |
| data-agent query returns an error / not published | Publish first (data-agent publish); only published agents are queryable. Confirm datasources + select-tables were configured before publishing. |
| Data Agent gives wrong answers | Add few-shots (add-fewshot), tighten instructions (update-config), and validate with 'data-agent evaluate' / 'validate-fewshots' (add --llm-* for a judge model). |
| user-data-function invoke fails | Ensure the function is published with public access; pass the exact portal-copied --url (HTTPS *.fabric.microsoft.com). fabio rejects non-trusted/non-HTTPS URLs. |
| SQL Database / Cosmos DB create fails on small capacity | These need F4+ capacity; resume/scale the capacity first. |
| cosmos-db-database create-container fails 'Offer Type is restricted to Autoscale for your account.' | Fabric Cosmos DB is autoscale-only. fabio always sends the autoscale throughput header, so use 'create-container --autoscale-max <RU>' (default 1000, the autoscale minimum); do not expect manual/fixed throughput. |
| cosmos-db-database query fails 'Cross partition query is required but disabled' | The query spans partitions. Omit --partition-key (fabio then enables cross-partition automatically), or pass --partition-key <value> to scope the query to one partition. |
| cosmos-db-database import reports documents skipped/failed with 'missing partition-key path' | Every document must contain the container's partition-key field (the container's partitionKey path). Fix the source rows, or pass --continue-on-error to skip invalid rows and import the rest. |
| data-agent add-datasource fails 'Failed to fetch schema' for an OPEN (push/GenericMirror) MirroredDatabase, even though 'warehouse query' reads it fine | This is a Fabric data-agent server limitation, not a fabio bug: an open mirror deterministically fails NL2SQL schema fetch (a Warehouse added to the same agent/session/token succeeds). Use a connection-based mirror (Snowflake/CosmosDB/Azure SQL) or a Warehouse/Lakehouse/SQLDatabase source for the data-agent NL2SQL case. |
| data-agent query/evaluate fails with 'The Data Agent run failed before producing a result.' | The orchestrator never ran. This is a LIKELY symptom of the 'AllowStoreAOAIDataInOtherRegions' tenant setting being disabled (the conversational runtime must store history via Azure OpenAI, gated for capacities outside the EU data boundary and the US) — ask an admin to enable it (fabio admin update-tenant-setting). It can also be a transient failure or a paused capacity, so retry once and confirm the capacity is running before escalating. |

## Safety
- Publishing a Data Agent makes it available to consumers — confirm datasources/table scope and validate answers first.
- Deleting a data-agent with --hard-delete is irreversible; confirm with the user.
- Exposing a GraphQL API / org-app shares underlying data — confirm the audience and least-privilege scope.

## Shared references
Cross-cutting operational guidance (the "common" layer) — consult the relevant topic before non-trivial work:

| Reference | Covers |
|---|---|
| `fabio context best-practices throttling` | fabio transparently handles 429 (Too Many Requests) and gateway errors. Agents do NOT need to implement retry logic. |
| `fabio context best-practices pagination` | fabio handles pagination via --all (auto-fetch all pages), --continuation-token (resume), and --limit (truncate). Agents rarely need to paginate manually. |
| `fabio context best-practices lro` | Many Fabric operations are async (return 202). fabio polls them automatically. Use --wait for job operations. |
| `fabio context best-practices translytical-writeback` | How to build and operate a Power BI translytical task flow (GA March 2026): a report button or input slicer invokes a Fabric User Data Function that writes back to a Fabric data source. Covers writeback-target choice (SQL Database vs Warehouse vs Lakehouse), optional parameters + defaults (May 2026), the input-slicer-as-input pattern (Feb 2026), testing with user-data-function invoke, safety/idempotency, and which parts are portal-authored. |

## See also
- fabio context persona app-developer
- fabio context workflow data-agent-setup
- fabio context examples data_agent query
- fabio context disambiguate sql-endpoint
- fabio context blueprint app-backend
- fabio context blueprint conversational-analytics
- fabio context persona data-solution-architect
- fabio context blueprint translytical
