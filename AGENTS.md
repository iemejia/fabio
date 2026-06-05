# Fabio CLI - Session Context

## Goal
- Design and implement an agent-first CLI (`fabio`) to manage Microsoft Fabric artifacts and data, inspired by AWS/gcloud/Azure CLI principles, with structured JSON output, composability via stdin/stdout, and machine-readable errors.

## Agent-Native CLI Principles

Fabio must always respect these 10 principles for agent-native CLIs:
https://trevinsays.com/p/10-principles-for-agent-native-clis

1. **Non-interactive by default** — No prompts; all inputs via flags/env/files. Non-TTY must fail fast.
2. **Structured, parseable output** — `--json` on every command; stdout = data, stderr = diagnostics; stable exit codes.
3. **Errors that teach and enumerate** — Errors include valid enum values, corrected command examples, and machine-readable codes with hints.
4. **Safe retries and explicit mutation boundaries** — `--dry-run` for mutations; idempotency-safe; stable returned IDs.
5. **Bounded responses** — `--limit` for list commands; default to concise output; truncation metadata in envelope.
6. **Cross-CLI vocabulary consistency** — Canonical agent verbs: `list`, `show`, `create`, `delete`, `copy`, `move`.
7. **Three-layer introspection** — `fabio agent-context` provides machine-readable command schema (flags, types, mutability, examples).
8. **Async-aware execution** — `--wait` for async jobs; local job ledger (`fabio jobs list/get/prune`); status polling.
9. **Persistent identity through profiles** — Named profiles (`fabio profile save/use/list/show/delete`); `--profile` flag.
10. **Two-way I/O** — Feedback channel (`fabio feedback send/list`); artifact delivery via stdout/file.

## Constraints & Preferences
- **Windows-first compatibility** — All code must work on Windows. Use `Path::new().join()` (never hardcoded `/` for filesystem paths), `dirs::home_dir()` (never manual `HOME`/`USERPROFILE`), `.lines()` for text parsing (handles CRLF), no Unix-specific APIs. `.gitattributes` enforces LF line endings.
- **Throttling reduction** — Reduce the likelihood of API throttling by:
  - Use bulk and batch operations when available (e.g., `item bulk-create`, `item bulk-delete`, workspace role batch-assign, domain batch-assign).
  - Prefer list APIs over repeated single-resource requests (e.g., use a single list call + client-side filter rather than N individual show calls).
- CLI designed for AI agents first (structured output, no interactive prompts, explicit params)
- JSON output by default with `--output json|table|plain` flag
- Composable: manage inputs and produce outputs for piping
- Machine-readable error codes in structured JSON envelope
- Rust (edition 2024, rust-version 1.85), uses clap derive, tokio, reqwest, azure_identity, serde, comfy-table, thiserror/anyhow
- Linting: clippy pedantic+nursery (zero warnings), rustfmt
- CI: GitHub Actions (cargo fmt, clippy, test, build release) on ubuntu/macos/windows
- Installable via `cargo install --git https://github.com/iemejia/fabio.git`

## Progress
### Done
- **Full Rust implementation** (broad command surface): auth, workspace, item, lakehouse, capacity, catalog, notebook, warehouse, data-agent, sql-database, sql-endpoint, ontology, environment, data-pipeline, copy-job, dataflow, report, semantic-model, eventhouse, eventstream, kql-database, kql-queryset, kql-dashboard, mirrored-database, mirrored-catalog, mirrored-databricks-catalog, mirrored-warehouse, reflex, ml-model, ml-experiment, spark, spark-job-definition, graphql-api, cosmos-db-database, snowflake-database, digital-twin-builder, digital-twin-builder-flow, event-schema-set, operations-agent, mounted-data-factory, user-data-function, git, connection, deployment-pipeline, domain, deploy, gateway, job-scheduler, variable-library, map, graph-query-set, graph-model, onelake-security, managed-private-endpoint, warehouse-snapshot, admin, paginated-report, dashboard, datamart, anomaly-detector, apache-airflow-job, app-backend, rti, rest, profile, jobs, feedback, operation, agent-context
- Core output system: JSON envelope (`{"data":..., "count":N}` or `{"error":{"code":...,"message":...}}`), table, plain, CSV, TSV formats
- Structured error system: `ErrorCode` enum (AUTH_REQUIRED, NOT_FOUND, RATE_LIMITED, CAPACITY_INACTIVE, API_ERROR, TIMEOUT, etc.) + `FabioError`
- Global options fully wired: `--output/-o`, `--query/-q` (dot-notation field extraction), `--quiet` (suppresses stdout), `--profile`, `--dry-run`, `--limit`, `--all`, `--continuation-token`, `--lro-timeout`
- HTTP client: async get/post/put/patch/delete with LRO polling (`Location` + `x-ms-operation-id` + resource follow)
- OneLake operations: DFS upload (create+append+flush), download, file listing; Blob API copy (server-side async)
- **Parallel file/table operations**: Upload, copy, move support glob patterns with concurrent execution and rate-limit retry
- **Sync command**: `lakehouse sync` copies new/modified files between lakehouses using ETag/MD5 comparison
- **LRO polling**: 2s default interval (respects `Retry-After` header, capped at 60s), 120s max, handles 200/202, checks `status` field until Succeeded/Failed
- **Transport retry**: Automatic retry on 502/503/504 gateway errors (3 attempts, linear backoff 1-3s)
- **Error code headers**: Extracts `x-ms-public-api-error-code` / `x-ms-error-code` response headers into error messages
- **Server-side file copy/move**: Blob API `PUT` with `x-ms-copy-source`, move = copy + delete
- **Server-side table copy/move/delete**: Root listing + prefix filter, per-file Blob copy, recursive DFS delete
- **Shortcuts**: Create/get/delete OneLake, ADLS Gen2, S3 shortcuts
- **Lakehouse table maintenance**: optimize-table (V-Order + Z-Order via Jobs API), vacuum-table (retention period formatting), table-schema (Delta log parsing from OneLake DFS)
- **Notebook run**: Captures job instance ID from Location header, status/stop via Jobs API
- **Notebook `--wait` flag**: Polls job status every 5s until Completed/Failed/Cancelled, with configurable `--timeout` (default 600s)
- **Item copy/move**: getDefinition LRO + create in dest workspace LRO; move = copy + delete source
- **Item exists/url/inspect**: exists returns `{exists: true/false}` (never errors on 404); url returns Fabric portal URL; inspect aggregates metadata + definition + connections in single response
- **Private link URL routing**: `with_private_link()` builder on FabricClient; `fabric_url()`, `onelake_dfs_url()`, `onelake_blob_url()` helpers transform URLs when `private_link_workspace` is configured via profile
- **Workspace**: 47 subcommands (CRUD + capacity + identity + role assignments + settings + networking + storage format + folders + OneLake + lifecycle policies + url)
- **Warehouse**: list/show/create/update/delete/query/connection-string (endpoint resolved, stdin/file/flag SQL input)
- **Git integration**: status, commit, pull, connect, disconnect, initialize, switch (branch), connection/credentials management, show-tracked
- **Ontology management**: list, show, create, update, delete, get-definition, update-definition (RDF file support, --dir for Fabric definition format, --decode for readable output)
- **Environment**: list, show, create, update, delete, publish, cancel-publish, get-spark-settings, get-staging-spark-settings, upload-staging-library
- **Data Pipeline**: list, show, create, update, delete, run (triggers Pipeline job)
- **Eventhouse**: list, show, create, update, delete
- **Eventstream**: list, show, create, update, delete, get-definition, update-definition, get-topology, pause, resume, get/pause/resume-source, get-source-connection, get/pause/resume-destination, get-destination-connection, add-source, add-destination
- **KQL Database**: list, show, create, update, delete, get-definition, update-definition (ReadWrite/ReadOnlyFollowing)
- **KQL Queryset**: list, show, create, update, delete, get-definition, update-definition, run (executes saved query tabs against configured data source)
- **KQL Dashboard**: list, show, create, update, delete, get-definition, update-definition (RealTimeDashboard.json)
- **Mirrored Database**: list, show, create, update, delete, get/update-definition, start, stop, status, table-status
- **Reflex**: list, show, create, update, delete, get-definition, update-definition (Data Activator triggers)
- **ML Model**: list, show, create, update, delete (CRUD only, no definition support)
- **ML Experiment**: list, show, create, update, delete (CRUD only, no definition support)
- **Copy Job**: list, show, create, update, delete, get-definition, update-definition (data movement)
- **Dataflow**: list, show, create, update, delete, get-definition, update-definition, discover-parameters, run, execute-query (Power BI transformation)
- **GraphQL API**: list, show, create, update, delete, get-definition, update-definition (schema.graphql)
- **Report**: list, show, create (from definition file), update, delete, get-definition, update-definition
- **Semantic Model**: list, show, create (from model.bim), update, delete, get-definition, update-definition, query, refresh, bind-connection, unbind-connection, takeover
- **Map**: list, show, create, update, delete, get-definition, update-definition (geospatial visualization with Azure Maps)
- **Spark Job Definition**: list, show, create, update, delete, get-definition, update-definition, run
- **Capacity**: list, show, suspend, resume, create, update, delete, list-skus, check-name (Fabric read-only API + ARM API for lifecycle management)
- **Connection**: list, show, create, update, delete, list-supported-types
- **Deployment Pipeline**: list, show, create, update, delete, list-stages, list-stage-items, assign-workspace, unassign-workspace, deploy
- **Domain**: list, show, create, update, delete, list-workspaces, assign-workspaces, unassign-workspaces, assign-by-capacity, assign-by-principal
- **Job Scheduler**: list-instances, get-instance, run-on-demand (with `--wait`/`--timeout`/`--cancel-on-timeout`), cancel-instance, list-schedules, get-schedule, create-schedule, update-schedule, delete-schedule
- **Spark**: get-settings, update-settings, list-pools, get-pool, create-pool, update-pool, delete-pool
- **OneLake Security**: list, show, upsert, delete, create (data access roles for row/column-level security)
- **Managed Private Endpoint**: list, show, create, delete (workspace private networking)
- **Pagination**: `--all` fetches all pages, `--continuation-token` resumes from a specific token, `--limit` truncates client-side
- **Agent-native compliance** (all 10 principles implemented):
  - Principle 1: Non-interactive by default
  - Principle 2: Structured parseable output
  - Principle 3: Errors that teach and enumerate
  - Principle 4: Safe retries (`--dry-run`)
  - Principle 5: Bounded responses (`--limit`, `--continuation-token`, truncation metadata)
  - Principle 6: Consistent vocabulary (list/show/create/delete/copy/move)
  - Principle 7: `fabio agent-context` machine-readable schema
  - Principle 8: Async-aware (`--wait`, jobs ledger)
  - Principle 9: Named profiles (`fabio profile save/use/list/show/delete`)
  - Principle 10: Two-way I/O (`fabio feedback send/list`)
- **SQL Database**: list/show/create/update/delete/query/connection-string/import (TDS + type inference)
- **SQL Database import**: Reads CSV/JSON files, infers column types (Int/BigInt/Float/Bit/Date/NVarChar), generates CREATE TABLE + batched INSERTs via TDS. Supports --drop-if-exists, --no-create-table, --batch-size.
- **SQL Endpoint**: list/show/connection-string/refresh-metadata/get-audit-settings/update-audit-settings/set-audit-actions (read-only companion to lakehouses)
- **Variable Library**: list/show/create/update/delete/get-definition/update-definition (variables.json + settings.json)
- **Event Schema Set**: list/show/create/update/delete/get-definition/update-definition (EventSchemaSetDefinition.json)
- **User Data Function**: list/show/create/update/delete/get-definition/update-definition (Python runtime)
- **Operations Agent**: list/show/create/update/delete/get-definition/update-definition (Configurations.json, goals/instructions/dataSources/actions)
- **Digital Twin Builder**: list/show/create/update/delete/get-definition/update-definition (links to lakehouse)
- **Digital Twin Builder Flow**: list/show/create/update/delete/get-definition/update-definition (requires parent DTB)
- **Cosmos DB Database**: list/show/create/update/delete/get-definition/update-definition (empty shell creation supported)
- **Snowflake Database**: list/show/create/update/delete/get-definition/update-definition (requires connection payload)
- **Anomaly Detector**: list/show/create/update/delete/get-definition/update-definition (Configurations.json)
- **Deploy**: plan/apply/export/init-params (CI/CD deployment engine: content-hash diffing, parameter substitution, rename detection, creationPayload, post-deploy hooks, logical ID resolution)
- **Gateway**: list/show/create/update/delete, list-members/update-member/delete-member, list/add/show/update/delete-role-assignments (VNet gateways)
- **Admin**: 49 subcommands (tenant settings, tags, workloads, workspaces, items, users, domains, labels, sharing links, external data shares, network policies)
- **Apache Airflow Job**: list/show/create/update/delete/get-definition/update-definition, start-environment/stop-environment/get-environment, list-files/get-file/upload-file/delete-file, get-compute/get-workspace-settings/deploy-requirements
- **App Backend**: list/show/create/update/delete (`--hard-delete` support, create uses LRO, update requires `--name` and/or `--description`)
- **Mirrored Catalog**: list/show/create/update/delete/get-definition/update-definition, refresh-metadata/mirroring-status/tables-status (requires tenant feature flag)
- **Mirrored Databricks Catalog**: list/show/create/update/delete/get-definition/update-definition, discover-catalogs/refresh-metadata/mirroring-status
- **Mirrored Warehouse**: list (requires tenant feature flag for mutations)
- **Warehouse Snapshot**: list/show/create/update/delete (requires --warehouse-id on create)
- **Graph Model**: list/show/create/update/delete/get-definition/update-definition, refresh-graph/execute-query/get-queryable-graph-type (portal initialization required for refresh)
- **Graph Query Set**: list/show/create/update/delete/get-definition/update-definition (definition is read-only export)
- **Catalog**: search (tenant-level full-text search across workspaces)
- **Dashboard**: list (read-only, portal-created)
- **Datamart**: list (read-only, portal-created)
- **Paginated Report**: list/update (read-only creation via portal/SSRS)
- **RTI (Real-Time Intelligence)**: nl-to-kql (natural language to KQL translation via POST /realTimeIntelligence/nltokql?beta=true)
- **Lakehouse query**: Resolves SQL analytics endpoint from lakehouse properties, executes T-SQL via shared TDS utilities
- **Rest**: Raw REST passthrough command (`fabio rest call`); supports GET/POST/PUT/PATCH/DELETE; `--body` accepts inline JSON, `@file`, `@-` (stdin); `--query-params` for URL params; `--poll` for LRO; dry-run for mutating methods; `--api powerbi` targets Power BI REST API
- **Power BI API pass-through**: `fabio rest call --api powerbi` routes to `https://api.powerbi.com/v1.0/myorg`; reuses Fabric token (no separate scope needed); supports all HTTP methods; dry-run shows `"api": "powerbi"` in output
- **Semantic Model Power BI commands** (12 subcommands via Power BI REST API): `list-parameters`, `update-parameters`, `list-datasources`, `update-datasources`, `list-users`, `add-user`, `delete-user`, `refresh-status`, `list-upstream`, `clone`, `export-pbix`, `import-pbix`
- **Semantic Model clone**: POST `/groups/{ws}/datasets/{id}/Default.Clone` with `--name`, `--target-workspace` (optional cross-workspace clone)
- **Semantic Model export-pbix**: POST `.../Default.Export` → binary download to `--file` path; creates parent dirs; reports `size_bytes` in output
- **Semantic Model import-pbix**: POST `/groups/{ws}/imports` multipart/form-data; `--name`, `--file`, `--name-conflict` (Abort|Overwrite|CreateOrOverwrite|GenerateUniqueName); validates file exists client-side
- **Item bulk-create/bulk-delete**: Client-side parallel operations using `execute_parallel` with bounded concurrency and rate-limit retry; per-item success/failure reporting
- **Item move-to-folder**: `POST /workspaces/{ws}/items/{id}/move` with `targetFolderId`; omit folder-id to move to workspace root
- **Item create-external-data-share**: Polymorphic recipients (`--recipient-type User|ServicePrincipal`, `--recipient-id`)
- **`--hard-delete` on all item deletes**: 38 item type delete commands support `--hard-delete` flag to permanently delete (skip recycle bin); appends `?hardDelete=true` to URL
- **Semantic Model unbind-connection**: Sends `{"connectionId": null}` to the bind endpoint to unbind
- **Dataflow discover-parameters**: `GET /workspaces/{ws}/dataflows/{id}/parameters` with pagination support
- **Warehouse connection-string extended**: `--guest-tenant-id` and `--private-link-type` optional query params for cross-tenant and private link scenarios
- **Error `isRetriable` field**: Parsed from API response `error.isRetriable` into `FabioError.retriable: Option<bool>`; emitted in structured error output when present
- **Notebook --strip-output**: `get-definition --strip-output` clears `outputs`/`execution_count` from ipynb cells; gracefully passes through `.py` format
- **CSV/TSV output**: Global `--output csv|tsv` on all commands; RFC 4180 quoting via `format_csv_value()`
- **Deploy validate**: Local-only pre-flight checks on source directory (validates .platform files, item types, definition structure, logical ID references); no API calls required
- **1286 Rust tests** (487 unit + 137 offline integration + 662 E2E requiring live tenant), zero clippy warnings, rustfmt clean
- **CI/CD**: GitHub Actions (6-target matrix: x64+arm64 for linux/macos/windows), Dependabot auto-merge, CodeQL, Secret Scanning
- **Release workflow**: Triggered on tags, builds 6 binaries, publishes GitHub Release with SHA256 checksums
- Release binary: ~16 MB, stripped, full LTO, panic=abort

### Blocked
- (none)

## Key Decisions
- JSON envelope always wraps output: lists get `{"data":[...],"count":N}`, objects get `{"data":{...}}`
- Errors on stderr as `{"error":{"code":"...","message":"..."}}` with non-zero exit
- `--query` supports simple dot-notation field projection (not full JMESPath; users can pipe to `jq`)
- `--quiet` suppresses all stdout; errors still go to stderr
- OneLake upload uses DFS create+append+flush 3-step pattern
- Notebook creation builds minimal .ipynb JSON, base64-encodes for Fabric API; `source` must be list of strings
- Item copy fetches definition from source via LRO, posts to destination workspace via LRO
- LRO polling: 2s default interval (respects `Retry-After` header, capped at 60s), 120s max wait, handles `Location`/`x-ms-operation-id` headers
- `post()` accepts `poll: bool` for LRO-aware operations
- Load-table requires PascalCase values (`"Overwrite"`, `"Csv"`) and `format` inside `formatOptions`
- **Load-table only supports Csv and Parquet**: The Fabric REST API `formatOptions` discriminated union only has `Csv` (with `header`/`delimiter`) and `Parquet` (format only). JSON is NOT supported — must convert to CSV/Parquet first. Sending CSV-specific fields (header, delimiter) with Parquet format causes API rejection.
- **SQL Database import**: Uses type inference with `Unknown` initial state → first non-empty observation sets the type, subsequent observations widen (Int→BigInt→Float→NVarChar, never narrows)
- **Server-side copy**: OneLake Blob API supports `PUT` with `x-ms-copy-source`; returns 202 with pending status. Poll via HEAD.
- **No native move/rename**: OneLake rejects `x-ms-rename-source`. Move = copy + delete.
- **Table file listing**: Must list from root (no `directory` param) to get real paths prefixed with item ID.
- **Recursive delete**: DFS `DELETE /{ws}/{lh}/Tables/{name}?recursive=true` works for directories.
- All destructive actions use consistent verb `delete` (not `remove`)
- Cross-workspace ops use `--source-workspace`/`--dest-workspace` with `visible_alias` short forms
- Auth relies on `DefaultAzureCredential` chain (az login, environment, managed identity)
- `azure_identity`/`azure_core` with `default-features = false` (no OpenSSL dependency)
- `unsafe_code = "forbid"` in lints
- **KQL Queryset definition format**: Uses `RealTimeQueryset.json` (NOT `RawQueryset.kql`). JSON structure: `{"queryset":{"version":"1.0.0","dataSources":[{"id","clusterUri","type","databaseName"}],"tabs":[{"id","content","title","dataSourceId"}]}}`. The `content` field holds the KQL query text with `\n` for newlines.
- **KQL Queryset run**: Fetches definition via LRO, decodes `RealTimeQueryset.json`, selects tab by name or index, resolves data source (clusterUri + databaseName), executes via Kusto REST API. Tab selection is case-insensitive by title.
- **Deploy diff strategy**: Content hash vs live workspace (not git diff) — detects portal edits, works without git, idempotent convergence
- **Deploy parallelism**: Semaphore-bounded `tokio::spawn` per-item within type batch (default 8); sequential for DataPipeline; deletes always sequential
- **Deploy parameter format**: JSON (not YAML) — no extra crate dependency, agent-native consistency
- **Deploy plan staleness**: Workspace fingerprint = SHA256 of sorted `(id, type, name)` tuples; mismatch → error unless `--force`
- **Deploy logical ID resolution**: String replacement in base64 payloads; resolves items created earlier in same session
- **Deploy rename detection**: Two-pass matching — first by (type, name), then unmatched source items with logical IDs get candidates checked via `fetch_deployed_logical_id()` which reads `.platform` part from deployed item definition
- **Deploy creationPayload**: Separate `creationPayload.json` file in item directory; merged into creation body as `creationPayload` field; parameter substitution applied
- **Deploy post-hooks**: Opt-out via `--no-post-hooks`; hooks never fire during `--dry-run`; failures are non-fatal (reported in output, don't fail the deploy). SemanticModel → `POST /refreshes`, Environment → `POST /staging/publish`
- **Deploy empty definitions**: Items with no parts (Lakehouse, MLModel) omit `definition` field on create; skip `updateDefinition` on update
- **Deploy ordering**: 42 item types in `DEPLOY_ORDER`; deployed in dependency order (storage → compute → code → models → reactive → APIs → ML → graph → viz)
- **Deploy no state file**: Stateless — always queries live workspace. No `.tfstate` equivalent.

## Critical Context
- User's tenant: set locally via secure environment configuration (redacted)
- Active capacity: set locally via secure environment configuration (redacted)
- Inactive capacity: set locally via secure environment configuration (redacted)
- Source workspace/lakehouse: set locally via secure environment configuration (redacted)
- Destination workspace/lakehouse: set locally via secure environment configuration (redacted)
- Notebook ID: set locally via secure environment configuration (redacted)
- Fabric REST base URL: `https://api.fabric.microsoft.com/v1`
- OneLake DFS base URL: `https://onelake.dfs.fabric.microsoft.com`
- OneLake Blob base URL: `https://onelake.blob.fabric.microsoft.com`
- Fabric scope: `https://api.fabric.microsoft.com/.default`
- Storage scope: `https://storage.azure.com/.default`
- Spark rate limit on small capacity: LRO reports 430 `TooManyRequestsForCapacity` (non-standard code)
- Test env vars: `FABIO_TEST_SOURCE_WORKSPACE`, `FABIO_TEST_SOURCE_LAKEHOUSE`, `FABIO_TEST_DEST_WORKSPACE`, `FABIO_TEST_DEST_LAKEHOUSE`, `FABIO_TEST_NOTEBOOK_ID`, `FABIO_TEST_CAPACITY_ID`
- Fabric REST API specs (OpenAPI): `https://github.com/Azure/azure-rest-api-specs/` (look under `specification/fabric/`)

## Relevant Files
- `Cargo.toml`: Project config, dependencies, clippy/lints config, release profile (LTO+strip)
- `rust-toolchain.toml`: stable channel, rustfmt+clippy components
- `src/main.rs`: Entry point, `#![recursion_limit = "256"]`, tokio async main, error handling dispatch
- `src/cli.rs`: Clap derive CLI definition, OutputFormat enum, Command enum with 66 subcommand groups
- `src/errors.rs`: ErrorCode enum + FabioError struct with thiserror
- `src/output.rs`: render_list_with_token, render_object, render_error (respects --quiet/--query), apply_query, dry_run_guard, unit tests
- `src/parallel.rs`: Parallel execution framework for concurrent file/table operations with rate-limit retry
- `src/client.rs`: FabricClient with async HTTP (get/post/put/patch/delete), LRO polling, OneLake DFS/Blob ops, ARM API methods (arm_get/post/put/patch/delete with ARM LRO polling), Power BI API methods (get/post/put/patch/delete/bytes/multipart_powerbi), run_notebook, trigger_item_job
- `src/commands/mod.rs`: Command dispatch
- `src/commands/auth.rs`: login/logout/status (DefaultAzureCredential chain)
- `src/commands/workspace.rs`: 47 subcommands (CRUD + capacity + identity + role assignments + settings + networking + storage format + folders + OneLake + lifecycle policies + url)
- `src/commands/item.rs`: 18 subcommands (CRUD + copy/move + definitions + list-connections + exists/url/inspect + bulk-create/bulk-delete + move-to-folder + create-external-data-share)
- `src/commands/lakehouse.rs`: 23 subcommands (CRUD + tables, files, upload, download, load-table, copy-file, delete-file, move-file, delete-table, copy-table, move-table, sync, create-shortcut, get-shortcut, delete-shortcut, optimize-table, vacuum-table, table-schema, query)
- `src/commands/notebook.rs`: create/get-definition (with --strip-output)/run (with --wait/--timeout/--parameters/--compute-type/--execution-data)/status/stop/delete
- `src/commands/warehouse.rs`: list/show/create/update/delete/query/connection-string (endpoint resolved, stdin/file/flag SQL input)
- `src/commands/sql_database.rs`: list/show/create/update/delete/query/connection-string/import (TDS + type inference)
- `src/commands/tds_utils.rs`: Shared TDS utilities (resolve_sql_input, parse_connection_string, execute_and_render_sql, column_value_to_json)
- `src/commands/dataagent.rs`: list/show/create/update/delete/query
- `src/commands/git.rs`: status/commit/pull/connect/disconnect/initialize/switch/connection/credentials/show-tracked
- `src/commands/ontology.rs`: list/show/create/update/delete/get-definition/update-definition
- `src/commands/environment.rs`: list/show/create/update/delete/publish/cancel-publish/get-spark-settings/get-staging-spark-settings/upload-staging-library
- `src/commands/data_pipeline.rs`: list/show/create/update/delete/run
- `src/commands/report.rs`: list/show/create/update/delete/get-definition/update-definition
- `src/commands/semantic_model.rs`: list/show/create/update/delete/get-definition/update-definition + query/refresh/bind-connection/unbind-connection/takeover + list-parameters/update-parameters/list-datasources/update-datasources/list-users/add-user/delete-user/refresh-status/list-upstream/clone/export-pbix/import-pbix
- `src/commands/eventhouse.rs`: list/show/create/update/delete
- `src/commands/eventstream.rs`: list/show/create/update/delete/get-definition/update-definition
- `src/commands/kql_database.rs`: list/show/create/update/delete/get-definition/update-definition
- `src/commands/kql_queryset.rs`: CRUD + get-definition/update-definition + run (fetch definition, select tab, execute against Kusto REST API)
- `src/commands/kql_dashboard.rs`: list/show/create/update/delete/get-definition/update-definition (RealTimeDashboard.json)
- `src/commands/mirrored_database.rs`: list/show/create/update/delete/get-definition/update-definition/start/stop/status/table-status
- `src/commands/reflex.rs`: list/show/create/update/delete/get-definition/update-definition (Data Activator)
- `src/commands/ml_model.rs`: list/show/create/update/delete (CRUD only)
- `src/commands/ml_experiment.rs`: list/show/create/update/delete (CRUD only)
- `src/commands/copy_job.rs`: list/show/create/update/delete/get-definition/update-definition
- `src/commands/dataflow.rs`: list/show/create/update/delete/get-definition/update-definition/discover-parameters/run/execute-query
- `src/commands/graphql_api.rs`: list/show/create/update/delete/get-definition/update-definition (schema.graphql)
- `src/commands/spark.rs`: get-settings/update-settings/list-pools/get-pool/create-pool/update-pool/delete-pool
- `src/commands/spark_job_definition.rs`: list/show/create/update/delete/get-definition/update-definition/run
- `src/commands/map.rs`: list/show/create/update/delete/get-definition/update-definition (geospatial Azure Maps)
- `src/commands/capacity.rs`: list/show (Fabric API) + suspend/resume/create/update/delete/list-skus/check-name (ARM API)
- `src/commands/connection.rs`: list/show/create/update/delete/list-supported-types
- `src/commands/deployment_pipeline.rs`: list/show/create/update/delete/list-stages/list-stage-items/assign-workspace/unassign-workspace/deploy
- `src/commands/domain.rs`: list/show/create/update/delete/list-workspaces/assign-workspaces/unassign-workspaces/assign-by-capacity/assign-by-principal
- `src/commands/job_scheduler.rs`: list-instances/get-instance/run-on-demand (with `--wait`/`--timeout`/`--cancel-on-timeout`), cancel-instance/list-schedules/get-schedule/create-schedule/update-schedule/delete-schedule
- `src/commands/onelake_security.rs`: list/show/upsert/delete/create (data access roles)
- `src/commands/managed_private_endpoint.rs`: list/show/create/delete
- `src/commands/variable_library.rs`: list/show/create/update/delete/get-definition/update-definition (variables.json + settings.json)
- `src/commands/event_schema_set.rs`: list/show/create/update/delete/get-definition/update-definition (EventSchemaSetDefinition.json)
- `src/commands/user_data_function.rs`: list/show/create/update/delete/get-definition/update-definition (definition.json, Python runtime)
- `src/commands/operations_agent.rs`: list/show/create/update/delete/get-definition/update-definition (Configurations.json)
- `src/commands/digital_twin_builder.rs`: list/show/create/update/delete/get-definition/update-definition (definition.json, links to lakehouse)
- `src/commands/digital_twin_builder_flow.rs`: list/show/create/update/delete/get-definition/update-definition (requires parent DTB)
- `src/commands/cosmos_db_database.rs`: list/show/create/update/delete/get-definition/update-definition (definition.json)
- `src/commands/snowflake_database.rs`: list/show/create/update/delete/get-definition/update-definition (requires connection payload)
- `src/commands/sql_endpoint.rs`: list/show/connection-string/refresh-metadata/get-audit-settings/update-audit-settings/set-audit-actions
- `src/commands/anomaly_detector.rs`: list/show/create/update/delete/get-definition/update-definition (Configurations.json)
- `src/commands/deploy/mod.rs`: DeployCommand enum (plan/apply/export/init-params/validate); execute dispatch; workspace name resolution
- `src/commands/deploy/apply.rs`: execute_changeset, execute_post_hooks, Rename handling (PATCH + updateDefinition), build_resolution_map, resolve_logical_ids_in_payload
- `src/commands/deploy/plan.rs`: build_changeset (two-pass with rename), validate_references, fetch_deployed_logical_id, compute_workspace_fingerprint
- `src/commands/deploy/params.rs`: Parameter substitution: find_replace, key_value_replace, spark_pool, semantic_model_binding
- `src/commands/deploy/init_params.rs`: scan_for_candidates, diff_for_parameters (GUID discovery, cross-environment diffing)
- `src/commands/deploy/changeset.rs`: Change, ChangeAction (Create/Update/Rename/Delete/Skip), Changeset (with warnings/errors), DeployResult
- `src/commands/deploy/ordering.rs`: DEPLOY_ORDER (42 types), deploy_priority, delete_priority, topological_sort
- `src/commands/deploy/platform.rs`: parse_source_directory (creationPayload.json parsing), SourceItem, SourceWorkspace, PlatformMetadata
- `src/commands/deploy/export.rs`: export_workspace (getDefinition LRO per item, write .platform + parts)
- `src/commands/gateway.rs`: list/show/create/update/delete, members, role assignments (VNet gateways)
- `src/commands/admin.rs`: 49 subcommands for tenant administration
- `src/commands/apache_airflow_job.rs`: CRUD + environment lifecycle + file ops + compute settings
- `src/commands/mirrored_catalog.rs`: CRUD + definition + mirroring operations
- `src/commands/mirrored_databricks_catalog.rs`: CRUD + definition + discover/refresh/status
- `src/commands/mirrored_warehouse.rs`: list only (tenant feature flag blocks mutations)
- `src/commands/warehouse_snapshot.rs`: list/show/create/update/delete
- `src/commands/graph_model.rs`: CRUD + definition + refresh-graph/execute-query/get-queryable-graph-type
- `src/commands/graph_query_set.rs`: CRUD + get-definition/update-definition (read-only export)
- `src/commands/catalog.rs`: search (tenant-level)
- `src/commands/dashboard.rs`: list (read-only)
- `src/commands/datamart.rs`: list (read-only)
- `src/commands/paginated_report.rs`: list/update (read-only creation)
- `src/commands/profile.rs`: save/use/list/show/delete (named profiles with defaults)
- `src/commands/jobs.rs`: list/get/prune (local async job ledger)
- `src/commands/feedback.rs`: send/list (two-way I/O for CLI friction reporting)
- `src/commands/agent_context.rs`: Machine-readable command schema for AI agents
- `src/commands/rest.rs`: Raw REST passthrough (method/path/body/query-params/poll); `resolve_body()` for @file/@- support; `--api powerbi` targets Power BI REST API
- `src/commands/rti.rs`: nl-to-kql (natural language to KQL translation)
- `tests/common/mod.rs`: Shared E2E test harness (TestConfig, helpers)
- `tests/e2e_auth.rs`: Auth integration tests
- `tests/e2e_workspace.rs`: Workspace CRUD + assign-capacity + networking + OneLake settings + folders + storage format + roles filter tests
- `tests/e2e_global_options.rs`: --query, --quiet, --output format tests
- `tests/e2e_item.rs`: Item list/show/create/delete/copy/move/bulk-create/bulk-delete tests
- `tests/e2e_lakehouse.rs`: Tables/files/upload/download/query tests
- `tests/e2e_lakehouse_files.rs`: File copy/move/delete tests
- `tests/e2e_lakehouse_tables.rs`: Table load/copy/move/delete tests
- `tests/e2e_lakehouse_shortcuts.rs`: Shortcut create/get/delete tests
- `tests/e2e_notebook.rs`: Notebook create/get-definition/run/run --wait/status/stop/delete/strip-output tests
- `tests/e2e_warehouse.rs`: Warehouse list/show/query/query-stdin tests
- `tests/e2e_sql_database.rs`: SQL Database CRUD + query + import tests
- `tests/e2e_dataagent.rs`: Data agent tests
- `tests/e2e_git.rs`: Git command group tests
- `tests/e2e_ontology.rs`: Ontology CRUD + definition tests
- `tests/e2e_agent_native.rs`: Agent-native compliance tests (principles 1-10)
- `tests/e2e_sync.rs`: Lakehouse sync tests
- `tests/e2e_connection.rs`: Connection CRUD + list-supported-types tests
- `tests/e2e_environment.rs`: Environment CRUD tests
- `tests/e2e_data_pipeline.rs`: Data pipeline CRUD + run tests
- `tests/e2e_eventhouse.rs`: Eventhouse CRUD tests
- `tests/e2e_eventstream.rs`: Eventstream CRUD tests
- `tests/e2e_kql_database.rs`: KQL database tests
- `tests/e2e_kql_queryset.rs`: KQL queryset tests
- `tests/e2e_kql_dashboard.rs`: KQL dashboard tests
- `tests/e2e_mirrored_database.rs`: Mirrored database tests
- `tests/e2e_reflex.rs`: Reflex CRUD + definition (get/update with simulator pipeline) tests
- `tests/e2e_graphql_api.rs`: GraphQL API CRUD tests
- `tests/e2e_ml_model.rs`: ML model CRUD tests
- `tests/e2e_ml_experiment.rs`: ML experiment CRUD tests
- `tests/e2e_copy_job.rs`: Copy job CRUD tests
- `tests/e2e_dataflow.rs`: Dataflow CRUD + run + execute-query tests
- `tests/e2e_report.rs`: Report CRUD tests
- `tests/e2e_semantic_model.rs`: Semantic model CRUD tests
- `tests/e2e_map.rs`: Map CRUD + definition tests
- `tests/e2e_spark_job_definition.rs`: Spark job definition tests
- `tests/e2e_deployment_pipeline.rs`: Deployment pipeline tests
- `tests/e2e_domain.rs`: Domain management tests
- `tests/e2e_job_scheduler.rs`: Job scheduler tests (11 tests: list, dry-run, fire-and-forget, --wait with polling)
- `tests/e2e_spark.rs`: Spark settings and pool tests
- `tests/e2e_capacity.rs`: Capacity list/show tests + ARM dry-run tests (suspend/resume/create/update/delete)
- `tests/e2e_onelake_security.rs`: OneLake security tests
- `tests/e2e_managed_private_endpoint.rs`: Managed private endpoint tests
- `tests/e2e_admin.rs`: Admin API tests (63 tests: listing, tag lifecycle, domain lifecycle, dry-run validations, sharing links, labels, external data shares)
- `tests/e2e_deploy.rs`: Deploy plan/apply/export/validate tests (42 tests: create, update, rename, creationPayload, parameters, staleness, logical ID resolution, post-hooks, init-params, validate)
- `tests/e2e_gateway.rs`: Gateway CRUD + role assignment tests
- `tests/e2e_apache_airflow_job.rs`: Apache Airflow job CRUD + environment + file ops tests
- `tests/e2e_mirrored_catalog.rs`: Mirrored catalog tests
- `tests/e2e_mirrored_databricks_catalog.rs`: Mirrored Databricks catalog tests
- `tests/e2e_mirrored_warehouse.rs`: Mirrored warehouse tests
- `tests/e2e_warehouse_snapshot.rs`: Warehouse snapshot tests
- `tests/e2e_graph_model.rs`: Graph model CRUD + refresh + query tests
- `tests/e2e_graph_query_set.rs`: Graph query set tests
- `tests/e2e_catalog.rs`: Catalog search tests
- `tests/e2e_dashboard.rs`: Dashboard list tests
- `tests/e2e_datamart.rs`: Datamart list tests
- `tests/e2e_paginated_report.rs`: Paginated report tests
- `tests/e2e_anomaly_detector.rs`: Anomaly detector CRUD + definition tests
- `tests/e2e_cosmos_db_database.rs`: Cosmos DB database CRUD tests
- `tests/e2e_snowflake_database.rs`: Snowflake database tests
- `tests/e2e_digital_twin_builder.rs`: Digital Twin Builder CRUD tests
- `tests/e2e_digital_twin_builder_flow.rs`: Digital Twin Builder Flow tests
- `tests/e2e_event_schema_set.rs`: Event Schema Set CRUD tests
- `tests/e2e_operations_agent.rs`: Operations Agent CRUD + definition tests
- `tests/e2e_mounted_data_factory.rs`: Mounted Data Factory tests
- `tests/e2e_user_data_function.rs`: User Data Function CRUD tests
- `tests/e2e_variable_library.rs`: Variable Library CRUD + definition tests
- `tests/e2e_sql_endpoint.rs`: SQL Endpoint tests
- `tests/e2e_profile.rs`: Profile save/use/list/show/delete tests
- `tests/e2e_jobs.rs`: Jobs ledger tests
- `tests/e2e_feedback.rs`: Feedback send/list tests
- `tests/e2e_agent_context.rs`: Agent context schema tests
- `tests/e2e_rest.rs`: REST passthrough tests (dry-run, body resolution, live calls)
- `tests/e2e_rti.rs`: RTI nl-to-kql tests (dry-run + live failure)
- `.github/workflows/ci.yml`: Rust CI (fmt, clippy, test, build) on 6 targets (x64+arm64 x linux/macos/windows)
- `.github/workflows/release.yml`: Release workflow (tag-triggered, 6 binaries, SHA256 checksums, GitHub Release)
- `.github/workflows/dependabot-auto-merge.yml`: Auto-merge Dependabot PRs on CI pass
- `.github/dependabot.yml`: Cargo + GitHub Actions dependency updates

## Ontology API Behaviors Discovered
- **Definition format**: Fabric ontology uses a proprietary JSON definition format (NOT RDF). Structure: `definition.json` (root, usually `{}`), `EntityTypes/{ID}/definition.json`, `EntityTypes/{ID}/DataBindings/{UUID}.json`, `RelationshipTypes/{ID}/definition.json`.
- **Schema URLs**: Entity types use `https://developer.microsoft.com/json-schemas/fabric/item/ontology/entityType/1.0.0/schema.json`, data bindings use `.../dataBinding/1.0.0/schema.json`, relationship types use `.../relationshipType/1.0.0/schema.json`.
- **Data binding format**: Requires `dataBindingConfiguration` wrapper (NOT flat fields). Structure: `{"id":"<uuid>","dataBindingConfiguration":{"dataBindingType":"NonTimeSeries","sourceTableProperties":{...},"propertyBindings":[...]}}`. The `sourceTableProperties` uses `itemId` (not `lakehouseId`) and `sourceTableName` (not `tableName`).
- **Data binding ID must be UUID format**: The `id` field in data bindings must be a valid UUID (e.g., `c0000001-0001-0001-0001-000000000001`). Non-UUID values (e.g., `db-equipment-001`) are silently dropped.
- **Property bindings use `targetPropertyId`**: Each entry in `propertyBindings` requires `sourceColumnName` and `targetPropertyId` (NOT `propertyId`). The `targetPropertyId` must match a property `id` in the entity type definition.
- **`sourceSchema` field in `sourceTableProperties`**: Include `"sourceSchema": "dbo"` alongside `sourceType`, `workspaceId`, `itemId`, `sourceTableName`. Required for lakehouse table bindings.
- **Data binding type enum**: `NonTimeSeries` (for lakehouse tables) or `TimeSeries` (requires `timestampColumnName`).
- **Source type enum in sourceTableProperties**: `LakehouseTable` or `KustoTable` (for Eventhouse).
- **CRITICAL: JSON key ordering sensitivity**: The Fabric Ontology API uses ordered JSON deserialization for data bindings. The `sourceType` field MUST be the first key in `sourceTableProperties`. If other keys (like `itemId`) come before `sourceType` (e.g., alphabetical order from serde_json without `preserve_order`), the API throws: `"Import of the {0} artifact '{1}' threw an exception with this message: {2}"`. The CLI normalizes key order automatically via `normalize_data_binding()`.
- **Entity type required fields**: `id`, `namespace` (must be `"usertypes"`), `name`, `namespaceType` (must be `"Custom"`). Optional: `baseEntityTypeId`, `entityIdParts`, `displayNamePropertyId`, `visibility` (must be `"Visible"`), `properties`, `timeseriesProperties`.
- **Property value types**: `String`, `Boolean`, `DateTime`, `Object`, `BigInt`, `Double`.
- **Relationship type required fields**: `id`, `namespace`, `name`, `namespaceType`, `source.entityTypeId`, `target.entityTypeId`.
- **Server auto-adds `$schema` URLs**: When you upload definitions, the server adds the appropriate `$schema` URL to the response. You don't need to include it in your upload.
- **Server adds `untypedProperties: []`**: Entity types returned by `getDefinition` include an extra `untypedProperties` array not present in the upload.
- **getDefinition/updateDefinition are LRO**: Both use the standard Fabric LRO polling pattern (202 + Location header).
- **`--decode` flag**: Adds `decodedPayload` field alongside original `payload` (JSON objects or text strings). Preserves backward compatibility.
- **`--dir` flag**: Reads Fabric ontology directory structure (`EntityTypes/`, `RelationshipTypes/` with `definition.json`, `DataBindings/`, `Documents/`, `Overviews/`, `ResourceLinks/`).
- **`preserve_order` feature**: `serde_json` is configured with `preserve_order` to support JSON key-order normalization for data bindings.

## OneLake API Behaviors Discovered
- Blob API copy (`x-ms-copy-source`): works for server-side file copy, async (202 with pending status)
- DFS rename (`x-ms-rename-source`): NOT supported (returns `UnsupportedHeader`)
- DFS recursive delete (`?recursive=true`): works for directories
- DFS listing with `directory` param on a table path shows virtual lakehouse structure (not real files)
- Root listing (no `directory` param): returns real paths prefixed with item ID
- Table files live at `Tables/{name}/_delta_log/` and `Tables/{name}/*.parquet`
- **DFS directory parameter "virtual lakehouse-in-lakehouse" view**: When `directory=X` is specified, the API returns ALL paths prefixed with `X/`, where top-level lakehouse dirs appear doubled (e.g., `Files/Files/myfile.csv` for a file at `Files/myfile.csv`). With `recursive=false`, only immediate virtual children show. Fix: always use `recursive=true` and strip the doubled prefix client-side.
- **Notebook Jobs API**: `POST /workspaces/{ws}/items/{id}/jobs/instances?jobType=RunNotebook` returns 202 + Location header with job instance URL. Status endpoint returns `NotStarted`, `InProgress`, `Completed`, `Failed`, `Cancelled`. Cancel via `POST .../cancel`.
- **Spark cold start on small capacity**: First notebook run can take 2-5 minutes to transition from `NotStarted` to `InProgress` due to Spark session allocation.

## Data Agent API Behaviors Discovered
- **Definition schema is minimal**: The `getDefinition`/`updateDefinition` API only controls `$schema`, `aiInstructions`, and `experimental` fields. Data sources are NOT configured through definitions — they are managed internally by Fabric (portal-only).
- **Definition schema URLs**:
  - `dataAgent/2.1.0/schema.json` — top-level, only has `$schema`
  - `stageConfiguration/1.0.0/schema.json` — has `$schema`, `aiInstructions`, `experimental`
  - `publishInfo/1.0.0/schema.json` — has `$schema`, `description`
- **Definition parts structure** (observed):
  - `Files/Config/data_agent.json` — schema version reference only
  - `Files/Config/draft/stage_config.json` — AI instructions (draft stage)
  - `Files/Config/published/stage_config.json` — AI instructions (published stage)
  - `Files/Config/publish_info.json` — publish metadata
  - `.platform` — git integration metadata (type, displayName, description, logicalId)
- **V3 Management Plane**: `GET /workspaces/{ws}/dataAgents/{id}/settings` endpoint EXISTS but returns `FeatureNotAvailable` (HTTP 403) with message "Data Agent V3 Public Management Plane is not enabled." This implies a tenant-level feature flag controls access.
- **Publishing is portal-only**: No REST API endpoint exposes publish functionality. Tried: `POST .../publish`, `POST .../jobs/instances?jobType=Publish`, `POST .../jobs/instances?jobType=PublishDataAgent` — all return 404 or InvalidJobType. The portal "Publish" button activates the server-side chat endpoint.
- **Published URL**: Only available from the portal Settings page AFTER publishing. Not exposed in `GET /dataAgents/{id}` response (which only returns `id`, `type`, `displayName`, `description`, `workspaceId`). Will be in `/settings` once V3 is enabled.
- **Published URL pattern**: `https://api.fabric.microsoft.com/v1/workspaces/{wsId}/dataagents/{agentId}/aiassistant/openai` — this is the OpenAI Assistants-compatible endpoint activated by publishing from the portal.
- **Chat protocol**: Data agents expose an OpenAI Assistants-compatible API at the published URL. Flow: `POST /assistants` → `POST /threads` → `POST /threads/{id}/messages` → `POST /threads/{id}/runs` → poll until terminal → `GET /threads/{id}/messages`. Query param: `?api-version=2024-05-01-preview`.
- **Authentication for chat**: Uses same Fabric bearer token (`https://api.fabric.microsoft.com/.default` scope), sent as `Authorization: Bearer {token}`.
- **PATCH /dataAgents/{id}**: Only accepts `displayName` and `description` fields. Passing `properties` or other fields returns `InvalidInput: UpdateArtifactRequest should have at least one valid field to update`.
- **Admin tenant settings API**: `GET /v1/admin/tenantsettings` returns error (insufficient privileges required). PowerBI admin API (`api.powerbi.com/v1.0/myorg/admin/tenantsettings`) returns 404.
- **Data source configuration via definition IS supported**: Include `datasource.json` parts at path `Files/Config/draft/{type}-{DisplayName}/datasource.json`. The server normalizes the path (e.g., `lakehouse-SalesLH` → `lakehouse-tables-SalesLH`). Schema: `dataSource/1.0.0/schema.json`. The server adds `$schema` URL automatically.
- **Data source path convention**: `Files/Config/{stage}/{type}-{DisplayName}/datasource.json` where stage is `draft` or `published`, and type is the full type value (e.g., `lakehouse-tables`, `data-warehouse`, `kusto`). The server normalizes shorthand prefixes.
- **Data source type enum**: `unknown`, `lakehouse_tables`, `lakehouse`, `data_warehouse`, `kusto`, `semantic_model`, `graph`, `mirrored_database`, `mirrored_azure_databricks`.
- **Elements array for table/column selection**: The `elements` field in `datasource.json` defines which tables and columns the agent can access. Each element has `display_name`, `type` (e.g., `lakehouse_tables.table`, `lakehouse_tables.column`), `is_selected`, `description`, `data_type`, and `children` (nested). Setting `is_selected: true` makes them available to the agent.
- **Element type enum**: `lakehouse_tables.table`, `lakehouse_tables.column`, `warehouse_tables.table`, `warehouse_tables.column`, `kusto.table`, `kusto.column`, `kusto.functions`, `semantic_model.table`, `semantic_model.column`, `semantic_model.measure`, `graph.nodeType`, `graph.edgeType`, `mirrored_database.table`, `mirrored_database.column`, plus schema-level types.
- **Server strips unknown fields from datasource.json**: Custom fields like `tables`, `connectionInfo` are stripped; only schema-defined fields are kept (`$schema`, `artifactId`, `workspaceId`, `displayName`, `type`, `userDescription`, `dataSourceInstructions`, `metadata`, `elements`).
- **Server strips experimental.dataSources**: Putting data sources in `experimental` field of `stage_config.json` is ignored; the experimental object is emptied. Data sources MUST use dedicated `datasource.json` files at the correct paths.
- **Update PATCH returns full object**: `PATCH /workspaces/{ws}/dataAgents/{id}` returns the full updated item object (not just `{status: "updated"}`).
- **Graph datasource type IS supported via definition API**: `type: "graph"` in `datasource.json` is accepted and persisted. Path: `Files/Config/draft/graph-{DisplayName}/datasource.json`. Server auto-adds `$schema`, `metadata: null`, `elements: []`. The `artifactId` should be the Graph Model item ID.
- **Full definition required for datasource persistence**: Single-part `updateDefinition` with only the datasource file is silently dropped (202 accepted but not persisted). Must include ALL parts together: `data_agent.json` + `stage_config.json` + `datasource.json`. This applies to all datasource types (not just graph).
- **Graph datasource path convention**: `Files/Config/draft/graph-{DisplayName}/datasource.json` — server does NOT normalize the prefix (unlike lakehouse which becomes `lakehouse-tables-`). The `graph-` prefix is kept as-is.
- **Graph datasource fields**: `artifactId` (Graph Model ID), `workspaceId`, `displayName`, `type` ("graph"), `userDescription`, `dataSourceInstructions`. Server adds: `$schema`, `metadata`, `elements`.

## Semantic Model API Behaviors Discovered
- **TMDL vs model.bim**: Direct Lake semantic models REQUIRE TMDL format (v4.0 pbism). The older model.bim JSON format (compat level 1550) does NOT support DirectLake mode partitions.
- **model.bim requires V3 (compat 1604)**: Import-mode models created via the Fabric Items API MUST use `compatibilityLevel: 1604` and `"defaultPowerBIDataSourceVersion": "powerBI_V3"`. Compat level 1550 returns "Import from JSON supported for V3 models only".
- **TMDL enum value for data source version**: Must be `powerBI_V3` (not `powerBIDataSourceVersion3`). The latter returns `InvalidValueFormat` parsing error.
- **definition.pbism is always required**: Fabric Items API for semantic model creation always requires a `definition.pbism` file in the definition parts. Without it, creation fails silently or produces a broken model.
- **TMDL definition.pbism format**: `{"$schema":"https://developer.microsoft.com/json-schemas/fabric/item/semanticModel/definitionProperties/1.0.0/schema.json","version":"4.2","settings":{}}` — v4.2 with the Fabric schema URL.
- **model.bim definition.pbism format**: `{"version": "3.0"}` — no `datasetReference` property (rejected by schema validator).
- **TMDL file structure**: A Direct Lake TMDL semantic model requires: `definition.pbism`, `model.tmdl` (model-level settings + expressions), and `definition/tables/{TableName}.tmdl` (one per table). The expression in `model.tmdl` provides the lakehouse connection via `DatabaseQuery` with a placeholder connection string.
- **Direct Lake partition annotation**: Each table partition needs `mode: directLake` in the TMDL source definition. Without it, the model defaults to Import mode.
- **Connection flag**: `semantic-model create --connection <lakehouse-sql-endpoint-id>` wires the Direct Lake connection. The connection ID is the SQL Analytics Endpoint ID (not the lakehouse ID itself).
- **Creation is LRO**: Semantic model creation uses the standard Fabric LRO pattern (202 + Location header polling).
- **Format auto-detection**: `.tmdl` files → TMDL format (v4.0 pbism); `.bim` file → model.bim format (v3.0 pbism). The CLI auto-detects from the file extension.
- **DirectQuery requires interactive credential binding**: DirectQuery models to Fabric warehouses need OAuth2 credentials configured via portal "Manage connections and gateways". The Power BI REST API `GetBoundGatewayDataSources` returns empty for API-created models. `BindToGateway` with virtual gateway `00000000-...` succeeds but doesn't configure credentials. OAuth2 credential type is "not supported for this API" when creating connections. The `executeQueries` DAX API works (uses caller's token directly), but report viewers fail (service needs stored credentials for the double-hop).
- **Direct Lake avoids credential issues**: Direct Lake models read directly from OneLake Delta files — no SQL connection credentials needed. The framing refresh uses the workspace identity automatically. Prefer Direct Lake over DirectQuery for programmatically-created reports.
- **Direct Lake Sql.Database() second parameter must be SQL endpoint ID**: The M expression `Sql.Database("<server>", "<database>")` must use the SQL Analytics Endpoint ID (not the lakehouse ID). Using the lakehouse ID causes `DM_InvalidRequest_DatamartNotFound` with `artifactType: 2000`.
- **Direct Lake needs refresh to frame**: After creation or updateDefinition, a `POST /refreshes` with `{"type": "Full"}` is required. Without framing, DAX queries fail with error code `3242524690`.
- **Direct Lake entity partition format**: `partition 'Name' = entity` with `mode: directLake`, `source` block containing `entityName: <table_name>`, `schemaName: dbo`, `expressionSource: DatabaseQuery`.
- **TMDL models are "definition-managed" (read-only in portal web editor)**: Models created via Fabric Items API with a `definition` are marked as definition-managed. The portal web modeler shows "This dataset is read-only" and blocks schema editing. Fix: call `POST /v1.0/myorg/groups/{ws}/datasets/{id}/Default.TakeOver` (with empty `{}` body) after creation. This converts the model to "service-managed" while preserving Direct Lake functionality, DAX queries, and refresh capability. The model keeps `targetStorageMode: Abf` (required for Direct Lake).
- **Do NOT change targetStorageMode to PremiumFiles for Direct Lake**: Switching to `PremiumFiles` breaks Direct Lake refresh ("cannot access source column" errors). Direct Lake REQUIRES `Abf` storage mode. The `PATCH /datasets/{id}` with `{"targetStorageMode": "PremiumFiles"}` only works for Import-mode models.
- **TakeOver preserves full functionality**: After TakeOver, `updateDefinition` still works (can redeploy TMDL), `refreshes` still work, DAX queries still work. TakeOver + refresh is the correct post-creation step for editable Direct Lake models.
- **definition.pbism v4.2 schema**: The correct pbism for TMDL models deployed via Fabric Items API is `{"$schema":"https://developer.microsoft.com/json-schemas/fabric/item/semanticModel/definitionProperties/1.0.0/schema.json","version":"4.2","settings":{}}` — NOT the older `{"version":"3.0","datasetReference":{...}}` format (which fails with schema validation error).
- **model.bim pbism format**: For model.bim, use just `{"version": "3.0"}` (no `datasetReference` — that property is rejected by schema validator).

## Report API Behaviors Discovered
- **definition.pbir is the report definition entry point**: Not `report.json`. The report definition file at `definition.pbir` references the semantic model binding.
- **definition.pbir format**: `{"version": "4.0", "datasetReference": {"byConnection": {"connectionString": null, "pbiServiceModelId": null, "pbiModelVirtualServerName": "sobe_wowvirtualserver", "pbiModelDatabaseName": "<semantic-model-id>", "name": "EntityDataSource", "connectionType": "pbiServiceXmlaStyleLive"}}}` — the `pbiModelDatabaseName` is the semantic model ID.
- **Blank report.json**: A minimal valid report is `{"config": "{\"version\":\"5.56\"}", "layoutOptimization": 0, "pods": [{"config": "{\"name\":\"Page 1\"}"}]}`
- **report create --dataset**: Generates both `definition.pbir` (with semantic model binding) and `report.json` (blank page) automatically. No definition file needed from the user.
- **Definition path changed**: The report definition entry point is `definition.pbir` (not `report.json`). Both `create` and `update-definition` use this path.
- **updateDefinition ALWAYS requires definition.pbir**: The API rejects requests missing the `definition.pbir` part, even if only updating `report.json`. Always include both parts when updating visuals.
- **updateDefinition CAN switch formats**: Format conversion works in both directions — send PBIR parts to convert to PBIR; send report.json to convert to PBIR-Legacy. Invalid schema fields cause silent rejection.
- **PBIR-Legacy is REQUIRED for programmatic visuals that render data**: Despite PBIR being the "future" format, only PBIR-Legacy with `prototypeQuery` produces visuals that actually display data. The portal itself creates PBIR-Legacy reports. Use `report.json` with `sections[].visualContainers[]` for programmatic report creation.
- **PBIR version.json requires semver**: The `version` field must match `^[1-9][0-9]*\.(0|[1-9][0-9]*)\.0$` (e.g., `"4.0.0"`, NOT `"4.0"`).
- **PBIR report.json requires layoutOptimization as string**: Must be `"None"` (string), not `0` (integer). Unlike PBIR-Legacy which uses integer 0.
- **PBIR-Legacy visual containers**: Reports use `report.json` with `sections[].visualContainers[]` array. Each visual container has `x`, `y`, `z`, `width`, `height`, `config` (JSON string), `filters`, and `tabOrder`.
- **Visual config structure**: The `config` JSON string contains `name`, `layouts[]`, and `singleVisual` with `visualType`, `projections`, `properties`, `objects`, and `dataTransforms`.
- **Supported visualType values**: `card` (KPI cards), `barChart` (bar charts), `tableEx` (data tables), `columnChart`, `lineChart`, `pieChart`, `donutChart`, etc.
- **Projections role names**: Card: `Values`; Bar/Column chart: `Category` + `Y`; Table: `Values`; Line chart: `Category` + `Y`.
- **queryRef format**: `TableName.ColumnName` for columns, `TableName.MeasureName` for measures. Must match the semantic model's exact table and field names.
- **dataTransforms for field binding**: Include `projectionOrdering`, `queryMetadata.Select[]` (with `Restatement`, `Name`, `Type`), and `selects[]` (with `displayName`, `queryName`, `roles`, `type`). Type values: 1=text, 2=numeric/measure, 260=aggregate.
- **Server preserves dataTransforms**: The API correctly stores and returns `dataTransforms` in visual configs, confirming programmatic visual creation is supported.
- **prototypeQuery is REQUIRED for visuals to render data**: Without `prototypeQuery` in `singleVisual`, the visual container appears but shows NO data. The `prototypeQuery` is a semantic query that tells the Power BI renderer how to construct the DAX query for the visual. Format: `{"Version": 2, "From": [{"Name": "<alias>", "Entity": "<TableName>", "Type": 0}], "Select": [...]}`. Each `Select` entry uses `Column` or `Measure` with `SourceRef.Source` referencing the `From` alias. The `dataTransforms.selects[].expr` must also use `SourceRef.Source` (not `SourceRef.Entity`).
- **PBIR format does NOT support programmatic visual data rendering**: PBIR visuals with `query.queryState` are stored correctly but render NO data in the portal. The PBIR schema does not allow `prototypeQuery` (rejected by schema validator). PBIR appears to require internal metadata that only Power BI Desktop or the portal editor generates. **Use PBIR-Legacy with `prototypeQuery` for programmatic report creation with working visuals.**
- **Server preserves original binding**: When `updateDefinition` is called with a new `definition.pbir` that has null values, the server uses the connection string from the original creation. The binding is stable.
- **publish-to-web**: `POST https://api.powerbi.com/v1.0/myorg/groups/{groupId}/reports/{reportId}/publishtoweb` returns 404 for Fabric reports. Attempted with various body formats (`{"accessLevel":"View","allowFullScreen":true}`). Likely requires: (1) tenant admin to enable "Publish to web" in admin portal, AND (2) may only work with classic Power BI reports (not Fabric-native reports created via Items API).
- **PowerBI API scope**: Report publish-to-web uses `api.powerbi.com` (not `api.fabric.microsoft.com`). Requires the same bearer token (`https://api.fabric.microsoft.com/.default` scope).

## Power BI File Formats Overview

Power BI has multiple file formats spanning different eras and use cases. Understanding these is critical for choosing the right approach when creating or managing semantic models and reports via the Fabric REST API.

| File Format | Purpose | Human Readable? | Fabric REST API Support | Era |
|---|---|---|---|---|
| `.pbix` | Standard Power BI report (binary) | No | Not directly (import only) | Original |
| `.pbit` | Power BI template (no data) | Partially | Not directly | Early |
| `.pbip` | Power BI Project (folder structure) | Yes | Maps to definition parts | 2023+ |
| `.pbir` | Report definition entry point | Yes | Required for all report ops | 2024+ |
| `model.bim` | Tabular model definition (JSON) | Yes | Supported via Items API | Legacy + supported |
| `TMDL` | Tabular Model Definition Language | Yes | Supported via Items API | Current |
| `.rdl` | Paginated report (XML) | XML | Limited | SSRS heritage |

### Format Selection for Fabric REST API

| Scenario | Format | Notes |
|---|---|---|
| Direct Lake semantic model | TMDL | Required for `mode: directLake` partitions |
| Import-mode semantic model | `model.bim` | Must use `compatibilityLevel: 1604` + `powerBI_V3` |
| Report with working visuals | PBIR-Legacy (`report.json`) | Only format supporting `prototypeQuery` for data rendering |
| Report for source control | PBIR (`definition/` folder) | Better diffs but limited programmatic visual support |
| Semantic model source control | TMDL (folder-based) | One `.tmdl` file per table, better Git diffs |

### Evolution Timeline

| Era | Main Formats | Fabric CLI Relevance |
|---|---|---|
| Early Power BI | `.pbix`, `.pbit` | Import-only, not definition-managed |
| Enterprise tabular | `model.bim` | `fabio semantic-model create --file model.bim` |
| Modern DevOps/Git | `.pbip`, `.pbir`, TMDL | `fabio semantic-model create --file *.tmdl`, `fabio report create/update-definition` |
| Paginated reporting | `.rdl` | `fabio item get-definition` (limited) |

### Key Constraints

- **Direct Lake requires TMDL**: `model.bim` cannot express `mode: directLake` partitions. Always use TMDL for Direct Lake.
- **model.bim requires V3**: `compatibilityLevel: 1604` and `defaultPowerBIDataSourceVersion: powerBI_V3` are mandatory.
- **PBIR cannot render data programmatically**: PBIR format visuals with `query.queryState` store correctly but display no data in the portal. Use PBIR-Legacy with `prototypeQuery` for programmatic report creation.
- **PBIR is the future**: PBIR will become the only supported format at GA. PBIR-Legacy is deprecated but still required for programmatic visual data rendering.
- **definition.pbir is always required**: Both PBIR and PBIR-Legacy reports need this file for semantic model binding.

## Power BI Report Definition Formats Reference

Power BI reports use one of two definition formats: **PBIR-Legacy** (single `report.json` file) or **PBIR** (individual files per visual/page in a `definition/` folder). Both formats use `definition.pbir` as the entry point for semantic model binding.

### Format Detection

The Fabric Items API returns the format in `getDefinition` response:
- `"format": "PBIR-Legacy"` → Single `report.json` contains all pages and visuals
- `"format": "PBIR"` → `definition/` folder with structured files per visual

New reports created in the Fabric Service default to PBIR. Existing reports are auto-converted to PBIR when edited in the Service (unless opted out via tenant setting). PBIR will become the only supported format at GA.

### definition.pbir (Common to Both Formats)

The `definition.pbir` file is **always required** and defines the semantic model binding. Two schema versions exist:

**Version 2 (Recommended for Fabric REST API deployments):**
```json
{
  "$schema": "https://developer.microsoft.com/json-schemas/fabric/item/report/definitionProperties/2.0.0/schema.json",
  "version": "4.0",
  "datasetReference": {
    "byConnection": {
      "connectionString": "semanticmodelid=<SEMANTIC-MODEL-UUID>"
    }
  }
}
```
When deploying via Fabric REST API, only `semanticmodelid=<UUID>` is needed in `connectionString`. The server auto-resolves workspace/name.

**Version 1 (Legacy, full connection details):**
```json
{
  "$schema": "https://developer.microsoft.com/json-schemas/fabric/item/report/definitionProperties/1.0.0/schema.json",
  "version": "4.0",
  "datasetReference": {
    "byConnection": {
      "connectionString": "Data Source=powerbi://api.powerbi.com/v1.0/myorg/<WorkspaceName>;initial catalog=\"<ModelName>\";integrated security=ClaimsToken;semanticmodelid=<UUID>",
      "pbiServiceModelId": null,
      "pbiModelVirtualServerName": "sobe_wowvirtualserver",
      "pbiModelDatabaseName": "<SEMANTIC-MODEL-UUID>",
      "connectionType": "pbiServiceXmlaStyleLive",
      "name": "EntityDataSource"
    }
  }
}
```

**Local path reference (PBIP only, not for API deployment):**
```json
{
  "version": "4.0",
  "datasetReference": {
    "byPath": {
      "path": "../Sales.Dataset"
    }
  }
}
```

| Version | Supported formats |
|---------|-------------------|
| 1.0     | PBIR-Legacy only (`report.json`) |
| 4.0+    | PBIR-Legacy (`report.json`) or PBIR (`definition/` folder) |

### PBIR-Legacy Format (`report.json`)

A single JSON file containing ALL report pages, visuals, filters, and formatting. Not publicly documented for editing — modifications may break on Desktop reload. Used by `fabio report update-definition --file <pbir> --report-json <report.json>`.

#### File Structure (API parts)
```
definition.pbir          # Semantic model binding (always required)
report.json              # All pages + visuals in one file
.platform                # Git integration metadata
```

#### report.json Top-Level Structure
```json
{
  "config": "<JSON-string: version, theme, activeSectionIndex>",
  "layoutOptimization": 0,
  "resourcePackages": [],
  "sections": [
    {
      "name": "ReportSection",
      "displayName": "Page Title",
      "displayOption": 1,
      "width": 1280.0,
      "height": 720.0,
      "ordinal": 0,
      "config": "<JSON-string: name, layouts>",
      "filters": "[]",
      "visualContainers": [ ... ]
    }
  ]
}
```

#### visualContainers[] Entry (PBIR-Legacy)
```json
{
  "x": 30.0,
  "y": 20.0,
  "z": 1000,
  "width": 250.0,
  "height": 110.0,
  "config": "<JSON-string: see Visual Config below>",
  "filters": "[]",
  "tabOrder": 0
}
```
- `x`, `y`: position on page canvas (pixels)
- `z`: stacking order (higher = on top)
- `width`, `height`: visual dimensions
- `config`: JSON-encoded string containing the visual definition
- `filters`: JSON-encoded array of visual-level filters
- `tabOrder`: keyboard navigation order

#### Visual Config Structure (PBIR-Legacy, inside `config` string)
```json
{
  "name": "unique_visual_name",
  "layouts": [{"id": 0, "position": {"x": 30, "y": 20, "z": 1000, "width": 250, "height": 110, "tabOrder": 0}}],
  "singleVisual": {
    "visualType": "barChart",
    "projections": {
      "Category": [{"queryRef": "TableName.columnName"}],
      "Y": [{"queryRef": "TableName.MeasureName"}]
    },
    "objects": {},
    "dataTransforms": {
      "projectionOrdering": {"Category": [0], "Y": [1]},
      "queryMetadata": {
        "Select": [
          {"Restatement": "columnName", "Name": "TableName.columnName", "Type": 1},
          {"Restatement": "MeasureName", "Name": "TableName.MeasureName", "Type": 2}
        ]
      },
      "selects": [
        {"displayName": "columnName", "queryName": "TableName.columnName", "roles": {"Category": true}, "type": {"category": null, "underlyingType": 1}},
        {"displayName": "MeasureName", "queryName": "TableName.MeasureName", "roles": {"Y": true}, "type": {"category": null, "underlyingType": 260}}
      ]
    }
  }
}
```

#### queryRef Format
- Columns: `TableName.columnName` (e.g., `Sales Summary.country`)
- Measures: `TableName.MeasureName` (e.g., `Sales Summary.Total Revenue`)
- Must match semantic model table/column/measure names exactly (case-sensitive)

#### dataTransforms Type Values
| Type | underlyingType | Description |
|------|---------------|-------------|
| 1    | 1             | Text/categorical (columns) |
| 2    | 260           | Numeric/measure/aggregate |

#### Projection Role Names by Visual Type
| visualType | Roles |
|------------|-------|
| `card` | `Values` (single measure or column) |
| `multiRowCard` | `Values` (multiple fields) |
| `barChart` | `Category` + `Y` |
| `columnChart` | `Category` + `Y` |
| `lineChart` | `Category` + `Y` (+ optional `Series`) |
| `pieChart` | `Category` + `Y` |
| `donutChart` | `Category` + `Y` |
| `tableEx` | `Values` (array of columns) |
| `matrix` | `Rows` + `Columns` + `Values` |
| `map` | `Category` (location) + `Size` + `Color` |
| `scatterChart` | `Category` + `X` + `Y` + `Size` |
| `slicer` | `Values` |
| `kpi` | `Indicator` + `TrendAxis` + `Goal` |

### PBIR Format (`definition/` folder)

A structured folder with individual JSON files per visual, page, and bookmark. Publicly documented with JSON schemas. Supports external editing and merge-friendly diffs.

#### File Structure (API parts)
```
definition.pbir                              # Semantic model binding
definition/
├── version.json                             # Required: PBIR version
├── report.json                              # Required: report-level settings
├── reportExtensions.json                    # Optional: report-level measures
├── pages/
│   ├── pages.json                           # Page ordering and active page
│   └── <pageName>/
│       ├── page.json                        # Required: page settings
│       └── visuals/
│           └── <visualName>/
│               ├── visual.json              # Required: visual definition
│               └── mobile.json              # Optional: mobile layout
└── bookmarks/
    ├── bookmarks.json                       # Bookmark ordering/groups
    └── <bookmarkName>.bookmark.json         # Individual bookmark state
.platform                                    # Git integration metadata
```

#### definition/version.json
```json
{
  "$schema": "https://developer.microsoft.com/json-schemas/fabric/item/report/definition/versionMetadata/1.0.0/schema.json",
  "version": "4.0.0"
}
```
Note: `version` must match `^[1-9][0-9]*\.(0|[1-9][0-9]*)\.0$` (semver with trailing `.0`).

#### definition/report.json (PBIR — NOT the same as PBIR-Legacy report.json)
```json
{
  "$schema": "https://developer.microsoft.com/json-schemas/fabric/item/report/definition/report/1.0.0/schema.json",
  "layoutOptimization": "None",
  "themeCollection": {
    "baseTheme": {
      "name": "CY24SU06",
      "reportVersionAtImport": "5.55",
      "type": "SharedResources"
    }
  },
  "annotations": [
    {"name": "defaultPage", "value": "<pageName>"}
  ]
}
```

#### definition/pages/pages.json
```json
{
  "$schema": "https://developer.microsoft.com/json-schemas/fabric/item/report/definition/pagesMetadata/1.0.0/schema.json",
  "pageOrder": ["page1Name", "page2Name"],
  "activePageName": "page1Name"
}
```

#### definition/pages/<pageName>/page.json
```json
{
  "$schema": "https://developer.microsoft.com/json-schemas/fabric/item/report/definition/page/1.2.0/schema.json",
  "name": "salesOverview",
  "displayName": "Sales Overview",
  "displayOption": "FitToPage",
  "height": 720,
  "width": 1280
}
```

**displayOption values**: `FitToPage`, `FitToWidth`, `ActualSize`

#### definition/pages/<pageName>/visuals/<visualName>/visual.json
```json
{
  "$schema": "https://developer.microsoft.com/json-schemas/fabric/item/report/definition/visualContainer/2.0.0/schema.json",
  "name": "barByCountry",
  "position": {
    "x": 30,
    "y": 150,
    "z": 3000,
    "width": 580,
    "height": 380,
    "tabOrder": 2000
  },
  "visual": {
    "visualType": "barChart",
    "query": {
      "queryState": {
        "Category": {
          "projections": [
            {
              "field": {
                "Column": {"Expression": {"SourceRef": {"Entity": "Sales Summary"}}, "Property": "country"}
              },
              "queryRef": "Sales Summary.country"
            }
          ]
        },
        "Y": {
          "projections": [
            {
              "field": {
                "Measure": {"Expression": {"SourceRef": {"Entity": "Sales Summary"}}, "Property": "Total Revenue"}
              },
              "queryRef": "Sales Summary.Total Revenue"
            }
          ]
        }
      }
    }
  }
}
```

#### PBIR Field Expression Types (in `field` property)

**Column reference:**
```json
{"Column": {"Expression": {"SourceRef": {"Entity": "TableName"}}, "Property": "columnName"}}
```

**Measure reference:**
```json
{"Measure": {"Expression": {"SourceRef": {"Entity": "TableName"}}, "Property": "measureName"}}
```

**Aggregation (e.g., SUM of a column):**
```json
{"Aggregation": {"Expression": {"Column": {"Expression": {"SourceRef": {"Entity": "TableName"}}, "Property": "columnName"}}, "Function": 0}}
```
Aggregation Function values: 0=Sum, 1=Avg, 2=Count, 3=Min, 4=Max, 5=CountNonNull, 6=Median, 7=StandardDeviation, 8=Variance

#### PBIR Naming Convention
- Page/visual/bookmark folder names default to 20-char unique IDs (e.g., `90c2e07d8e84e7d5c026`)
- Can be renamed to human-friendly names (letters, digits, underscores, hyphens)
- The `name` property inside each JSON must match the folder name and be unique

#### PBIR Annotations
Custom name-value pairs for external tools (ignored by Power BI Desktop):
```json
"annotations": [{"name": "myCustomKey", "value": "myCustomValue"}]
```
Supported on `visual.json`, `page.json`, and `report.json`.

### Key Differences Between Formats

| Aspect | PBIR-Legacy | PBIR |
|--------|-------------|------|
| File structure | Single `report.json` | `definition/` folder tree |
| Visual definition | JSON string in `visualContainers[].config` | `visual.json` per visual |
| Field binding | `projections` + `dataTransforms` | `query.queryState` with semantic expressions |
| Schema validation | No public schema | Full JSON schemas with IntelliSense |
| External editing | Not supported (may break) | Officially supported |
| Merge conflicts | Entire report in one file | Per-visual file diffs |
| Size limits | N/A | 1000 pages, 1000 visuals/page, 300MB total |
| Future | Deprecated at GA | Only supported format at GA |
| API export format | Matches what's stored in service | Matches what's stored in service |

### Fabric REST API Usage

**Creating a report (both formats):**
```
POST /workspaces/{ws}/reports
Body: {"displayName": "My Report", "definition": {"parts": [...]}}
```

**Updating definition (both formats):**
```
POST /workspaces/{ws}/reports/{id}/updateDefinition
Body: {"definition": {"parts": [...]}}
```

Required parts depend on format:
- **PBIR-Legacy**: `definition.pbir` (always required) + `report.json`
- **PBIR**: `definition.pbir` + `definition/version.json` + `definition/report.json` + `definition/pages/pages.json` + page/visual files

**fabio CLI commands:**
```bash
# Create report bound to semantic model (auto-generates blank definition)
fabio report create --workspace $WS --name "My Report" --dataset $SEMANTIC_MODEL_ID

# Update with visuals (PBIR-Legacy)
fabio report update-definition --workspace $WS --id $REPORT_ID \
  --file definition.pbir --report-json report.json

# Get definition (returns format + all parts base64-encoded)
fabio report get-definition --workspace $WS --id $REPORT_ID
```

### JSON Schema URLs (PBIR)
- Visual container: `https://developer.microsoft.com/json-schemas/fabric/item/report/definition/visualContainer/2.0.0/schema.json`
- Visual configuration: `https://developer.microsoft.com/json-schemas/fabric/item/report/definition/visualConfiguration/2.0.0/schema-embedded.json`
- Page: `https://developer.microsoft.com/json-schemas/fabric/item/report/definition/page/1.2.0/schema.json`
- Semantic query: `https://developer.microsoft.com/json-schemas/fabric/item/report/definition/semanticQuery/1.2.0/schema.json`
- Report: `https://developer.microsoft.com/json-schemas/fabric/item/report/definition/report/1.0.0/schema.json`
- definition.pbir: `https://developer.microsoft.com/json-schemas/fabric/item/report/definitionProperties/2.0.0/schema.json`
- All schemas: `https://github.com/microsoft/json-schemas/tree/main/fabric/item/report/definition`

## Git Integration API Behaviors Discovered
- **GitHub provider REQUIRES credentials**: `fabio git connect --provider github` ALWAYS requires `--connection-id` pointing to a pre-configured `GitHubSourceControl` connection. Without it, returns: `"The property myGitCredentials is required for the GitProviderType GitHub."`. Azure DevOps can use "Automatic" credentials without a connection ID.
- **Fabric Git does NOT track table data**: Delta tables created via `load-table` are NOT version-controlled. Only item definitions (`.platform`, metadata files, notebook code) are tracked. `git status` shows NO changes after creating a table. CI/CD best practice: version-control the Notebook/Pipeline that creates the table.
- **Lakehouse definition does NOT include table schema**: `lakehouse.metadata.json` remains `{}` even after tables are created. The definition only tracks: `.platform` (type metadata), `alm.settings.json` (shortcuts/data access roles config), `shortcuts.metadata.json`.
- **Git status API is LRO-aware**: `GET /workspaces/{ws}/git/status` uses the LRO pattern. Returns `{"changes": [...], "workspaceHead": "<sha>", "remoteCommitHash": "<sha>"}`.
- **Initialize strategy for new workspaces**: Use `prefer-workspace` when connecting a workspace with existing items to an empty repo. Use `prefer-remote` when the repo already has content to pull into the workspace.
- **Commit auto-fetches workspaceHead**: The commit API requires `workspaceHead` but fabio auto-fetches it from `git status` if not provided. Agents don't need to track it manually.
- **Item naming in git**: Folders use `{DisplayName}.{ItemType}` convention: `SalesLakehouse.Lakehouse`, `CreateSalesTable.Notebook`.
- **Notebook format in git**: `{Name}.Notebook/.platform` + `{Name}.Notebook/notebook-content.py`. Cell separators: `# CELL ********************`.
- **ObjectId vs LogicalId**: First commit assigns only `objectId`. After commit, items gain a `logicalId` (stored in `.platform`) for cross-workspace portability.
- **remoteChange is null**: When there's no remote change, the field is `null` (not `"None"`), but `workspaceChange` uses string values like `"Added"`, `"Modified"`, `"None"`.
- **Git connection state**: `fabio git connection show` returns `gitConnectionState: "ConnectedAndInitialized"` with `gitSyncDetails.head` and `lastSyncTime`.
- **Commit is LRO**: Returns 202 with operation ID. With `--wait`, polls until `Succeeded`/`Failed`. Returns `percentComplete: 100` on success.
- **Full CI/CD workflow via fabio**: Validated complete flow: `workspace create` → `workspace assign-capacity` → `lakehouse create` → `git connect` → `git init` → `git commit` → (create items) → `git commit`.
- **Azure DevOps cross-service identity requirement**: Fabric's git integration uses the authenticated user's identity to access Azure DevOps. The user (OID from the Fabric token) must be a member of the Azure DevOps organization AND have at least Contributor access to the project/repo. Without this, `git connect` returns `InsufficientPrivileges` (403) — the error looks like a workspace permission issue but is actually Azure DevOps rejecting the identity.
- **Azure DevOps org must share the same AAD tenant**: The Azure DevOps organization must be backed by (connected to) the same Azure AD tenant as the Fabric workspace. Cross-tenant git integration is not supported with "Automatic" credentials.
- **`directoryName` is required in the connect body**: The Fabric API rejects `git connect` without a `directoryName` field in `gitProviderDetails`. The CLI defaults to `"/"` (repo root). Omitting it returns `InvalidInput: The DirectoryName field is required.`
- **Azure DevOps "Automatic" credentials work without connection ID**: Unlike GitHub (which always requires `--connection-id`), Azure DevOps uses the caller's OAuth token directly to access repos. No pre-configured Fabric connection is needed. The Fabric service requests Azure DevOps access on behalf of the user transparently.
- **Azure DevOps permission propagation delay**: After adding a user to an Azure DevOps org/project, it may take 5-10 seconds for permissions to propagate. Fabric's git connect can fail with 403 immediately after granting access.
- **One repo can be connected to multiple workspaces**: Different Fabric workspaces can connect to the same Azure DevOps repo and branch (same `directoryName`). Each workspace maintains independent sync state. Useful for CI/CD workspace per environment pattern.

## Cross-Database Query Behaviors Discovered
- **Lakehouse SQL endpoint supports three-part naming**: From a lakehouse SQL endpoint, you can query other databases in the same workspace using `[DatabaseName].[schema].[table]` syntax. Example: `SELECT * FROM SalesDB.dbo.orders` works from the ProductCatalog lakehouse SQL endpoint.
- **SQL Database does NOT support three-part naming**: Fabric SQL Database (`.database.fabric.microsoft.com`) rejects cross-database references with error 40515: "Reference to database and/or server name is not supported in this version of SQL Server."
- **Cross-database direction is one-way**: Lakehouse/Warehouse SQL endpoint → SQL Database works. SQL Database → Lakehouse/Warehouse does NOT work.
- **Warehouse and Lakehouse can cross-query each other**: Both share the same `.datawarehouse.fabric.microsoft.com` TDS endpoint and can query any database visible in `sys.databases` (all lakehouses, warehouses, and SQL Databases in the same workspace).
- **Practical pattern for cross-database analytics**: Use the lakehouse SQL endpoint as the query hub. It can JOIN local Delta tables with SQL Database tables in a single query: `SELECT l.col FROM dbo.local_table l JOIN SqlDb.dbo.remote_table r ON l.id = r.id`.
- **Date columns from cross-DB queries**: TDS returns date columns as "N days since 0001-01-01" format when crossing database boundaries. May need client-side conversion.
- **SQL Database requires F4+ capacity**: On F2 capacity, SQL Database TDS connections fail with error 18456 State 240 ("Validation of user's permissions failed"). This is not a permissions issue — it's insufficient compute to serve the TDS endpoint. F4 resolves the issue completely.
- **SQL Database auto-creates a SQLEndpoint item**: Creating a SQL Database automatically creates a companion SQLEndpoint item with the same display name. This is the mirrored read-only analytics endpoint.
- **Initial catalog must be set explicitly**: Fabric TDS connection strings from the REST API contain only the server hostname (no `database=` or `Initial Catalog=`). The TDS client must set the initial catalog to the item's `displayName` to connect to the correct database context. Without it, the server defaults to an arbitrary database in the workspace.

## KQL Queryset API Behaviors Discovered
- **Definition uses `RealTimeQueryset.json`** (NOT `RawQueryset.kql`): The definition part path is `RealTimeQueryset.json` containing a JSON object with `queryset.version`, `queryset.dataSources[]`, and `queryset.tabs[]`.
- **Empty queryset returns `{}`**: A newly created queryset has `RealTimeQueryset.json` with payload `e30=` (base64 for `{}`). Must check for empty object before attempting to run.
- **Data source type is always `AzureDataExplorer`**: Even for Fabric Eventhouses, the `type` field in data sources is `"AzureDataExplorer"` (not `"Eventhouse"` or `"Fabric"`).
- **clusterUri for Fabric Eventhouse**: Uses the Kusto query URI format `https://<id>.<region>.kusto.fabric.microsoft.com`. This is the same URI used for direct KQL database queries.
- **Tab content uses literal `\n`**: In the JSON definition, KQL query newlines are stored as literal `\n` characters within the string (not `\\n` escape sequences). Multi-line queries work correctly.
- **Tab selection is case-insensitive by title**: The portal stores tab titles as-is, but `kql-queryset run` matches case-insensitively for agent ergonomics.
- **No server-side run API exists**: KQL Querysets have no Jobs API or `/run` endpoint. Execution requires client-side: get definition → extract tab content → POST to Kusto REST API.
- **getDefinition is LRO**: Like other Fabric definition APIs, `POST .../getDefinition` returns 202 and requires polling.
- **updateDefinition is LRO**: Returns 202 with empty body on success (after polling). The response body from LRO completion is empty/null.
- **Server normalizes CRLF**: If you upload a definition with LF line endings, the server may return it with CRLF (`\r\n`). Decode must handle both.
- **Multiple data sources supported**: A queryset can reference multiple clusters/databases. Each tab has a `dataSourceId` field linking to a specific data source.

## GraphQL API Behaviors Discovered
- **Query endpoint**: `POST /workspaces/{ws}/graphqlApis/{id}/graphql` with body `{"query": "...", "variables": {...}, "operationName": "..."}`.
- **Scope is standard Fabric scope**: Uses `https://api.fabric.microsoft.com/.default` (same as all Fabric APIs, NOT a GraphQL-specific scope).
- **Response envelope**: Returns `{"data": {...}}` on success, `{"errors": [...]}` on failure, or both for partial results.
- **Introspection blocked by default**: `__schema` and `__type` introspection queries return a security error unless explicitly enabled in tenant settings.
- **Definition format**: `graphql-definition.json` with `datasources[]` array. Each datasource has `sourceItemId`, `sourceWorkspaceId`, `sourceType` (e.g., `SqlAnalyticsEndpoint`, `Warehouse`), and `objects[]` with field mappings.
- **updateDefinition is LRO**: Returns 202 and must be polled. Creating a GraphQL API with a datasource requires the LRO pattern.
- **sourceType values**: `SqlAnalyticsEndpoint` (for lakehouses), `Warehouse`, `SqlDatabase`. The source item ID is the SQL analytics endpoint ID (not the lakehouse/warehouse item ID directly).
- **Object field mappings**: Each object in `objects[]` maps GraphQL types to source table columns. Field names are auto-generated from table column names.
- **No schema.graphql in initial definition**: Newly created GraphQL APIs have no `schema.graphql` part until a datasource is configured and the schema is generated.

## Warehouse API Behaviors Discovered
- **Connection string format**: `<unique-id>.datawarehouse.fabric.microsoft.com` — no port, no protocol prefix. TDS client connects via port 1433 (default).
- **Views appear in INFORMATION_SCHEMA.TABLES**: Both tables and views show up. Distinguish via `TABLE_TYPE` column (`BASE TABLE` vs `VIEW`).
- **System views are visible**: `queryinsights.*` and `sys.*` views appear alongside user objects. Filter with `WHERE TABLE_SCHEMA = 'dbo'` for user objects only.
- **Date columns via TDS**: Date values come through as "N days since 0001-01-01" string representation in the mssql-rs crate. Conversion: `chrono::NaiveDate::from_num_days_from_ce(days + 1)`.
- **Cross-workspace queries NOT supported**: Three-part naming only works within the same workspace. Cross-workspace requires explicit data copy or shortcuts.

## Semantic Model + Report Creation Workflow
- **DirectQuery to warehouse**: model.bim with `compatibilityLevel: 1604`, partition `mode: "directQuery"`, M expression using `Sql.Database("<connectionInfo>", "<displayName>")`.
- **M expression pattern for warehouse**: `let Source = Sql.Database("server.datawarehouse.fabric.microsoft.com", "WarehouseName"), table = Source{[Schema="dbo",Item="table_name"]}[Data] in table`.
- **Measures in model.bim**: Defined at table level in `measures[]` array with `name` and `expression` (DAX). Works for both Import and DirectQuery models.
- **Report creation with `--dataset`**: Simplest path — generates `definition.pbir` + blank `report.json` automatically. No need to craft definition files manually.
- **Report visuals are fully programmable**: CLI-created reports can include working visuals (cards, bar charts, tables) that render data — no portal interaction needed. The key requirement is including `prototypeQuery` in each visual's `singleVisual` config.
- **Semantic model ID links report to data**: The `definition.pbir` file's `pbiModelDatabaseName` field is the semantic model ID (UUID), not the display name.
- **End-to-end creation order**: Warehouse (data source) → Semantic Model (definition + connection) → Report (bound to semantic model). Each step depends on the previous item's ID.

## EventStream API Behaviors Discovered
- **Definition format**: `eventstream.json` contains the topology with `sources`, `destinations`, `streams`, `operators`, and `compatibilityLevel` fields. Separate `eventstreamProperties.json` controls retention and throughput.
- **Definition update is LRO**: `POST .../updateDefinition` returns 202 and requires polling. The response body after LRO completion is empty/null.
- **Source types**: `CustomEndpoint`, `AzureEventHub`, `AzureIoTHub`, `SampleData`, `AmazonKinesis`, `ApacheKafka`, `ConfluentCloud`, `GooglePubSub`, plus CDC types (`AzureSQLDBCDC`, `MySQLCDC`, `PostgreSQLCDC`) and Fabric events (`FabricWorkspaceItemEvents`, `FabricJobEvents`, `FabricOneLakeEvents`).
- **Destination types**: `Eventhouse`, `Lakehouse`, `CustomEndpoint`, `Activator`.
- **CustomEndpoint source exposes Event Hub-compatible interface**: Creates an Azure Event Hub-compatible endpoint. Connection info retrieved via `GET .../sources/{sourceId}/connection` returns `fullyQualifiedNamespace`, `eventHubName`, and `accessKeys` with SAS connection strings.
- **Eventhouse destination `itemId` is the KQL Database ID**: Despite documentation examples showing Eventhouse ID, the topology `itemId` field must be the **KQL Database item ID** (not the Eventhouse ID). Using the Eventhouse ID causes errors ("Unable to extract cluster URL from the Eventhouse KQL database item ID").
- **Two ingestion modes for Eventhouse destination**:
  - `ProcessedIngestion`: Auto-creates the destination table with extra system columns (`EventEnqueuedUtcTime`, `EventProcessedUtcTime`, `PartitionId`). Does NOT require pre-created table or mapping. Requires `inputSerialization` in properties.
  - `DirectIngestion`: Uses a pre-created KQL table and JSON mapping rule. Requires `connectionName` (arbitrary unique string) and `mappingRuleName`. Only maps fields defined in the mapping — no extra system columns.
- **DirectIngestion requires pre-created table + mapping**: Use `.create-merge table` and `.create-or-alter table ... ingestion json mapping` via `kql-database query` BEFORE configuring the destination.
- **Destination status transitions**: `Creating` → `Running` (or `Warning`). The `Warning` state appears when the Eventhouse ID is used instead of KQL Database ID. With correct KQL Database ID, destination transitions to `Running` within ~90 seconds.
- **Source status transitions**: `Creating` → `Running`. Custom Endpoint sources become Running quickly (~15-30 seconds).
- **Stream status**: Always shows `Created` (not `Running`). This is expected — streams are routing constructs, not active processes.
- **Graph-like topology**: Nodes reference each other by `name` via `inputNodes` arrays. A source feeds into a stream, which feeds into a destination or operator. The `name` field must be unique across all nodes (sources, destinations, streams, operators).
- **Default stream naming convention**: `{eventstream-name}-stream` for the default stream fed by the primary source.
- **No REST API for individual source/destination CRUD**: Sources and destinations can only be created/deleted via `update-definition` (full definition replacement). The individual `GET .../sources/{id}` and `GET .../destinations/{id}` endpoints are read-only.
- **`databaseName` field is optional in topology properties**: The server stores it but it's not required for either DirectIngestion or ProcessedIngestion. The `itemId` (KQL Database ID) is sufficient for routing.
- **`connectionName` for DirectIngestion**: Any unique string up to 40 characters. Recommended pattern: `es-eh-conn-{random4}`.
- **ProcessedIngestion auto-creates table**: When using ProcessedIngestion mode, the destination table (e.g., `SensorEvents2`) is automatically created in the KQL database when the first events flow through. No need to pre-create it.
- **Ingestion latency**: ProcessedIngestion: ~60 seconds from event send to queryable. DirectIngestion: ~60-90 seconds. Both modes batch events for efficiency.
- **Event Hub SDK for sending**: Use `azure-eventhub` Python SDK (or equivalent) with the SAS connection string from `get-source-connection`. Standard Event Hub producer pattern works.
- **Pause/Resume for stream control**: `POST .../pause` and `POST .../resume` control the entire eventstream. Individual sources/destinations can be paused/resumed independently.
- **`eventstreamProperties.json`**: Controls `retentionTimeInDays` (1-90, default 1) and `eventThroughputLevel` (`Low`, `Medium`, `High`). Optional in definition updates.
- **Compatibility level**: Current version is `"1.1"`. Always include it in the definition.
- **New commands added**: `fabio eventstream add-source` and `fabio eventstream add-destination` — high-level helpers that fetch current definition, merge in the new node, auto-create default streams, and push the updated definition. Simplifies agent workflow vs. manually crafting full definition JSON.

## RTI (Real-Time Intelligence) End-to-End Workflow
- **Creation order**: Workspace → Eventhouse → KQL Database (with `--eventhouse-id`) → EventStream → Configure topology (add-source + add-destination) → Send events → Query via KQL.
- **Required items**: Workspace (with Fabric capacity assigned), Eventhouse, KQL Database, EventStream.
- **Pre-requisites for DirectIngestion**: Create table schema and JSON ingestion mapping in KQL database BEFORE configuring the EventStream destination.
- **Querying EventStream data**: Query the KQL database directly using `fabio kql-database query`. The EventStream itself is not queryable — it's a routing/processing layer.
- **fabio commands for full RTI pipeline**:
  ```
  fabio workspace create --name "my-rti-workspace"
  fabio workspace assign-capacity --id <ws-id> --capacity <cap-id>
  fabio eventhouse create --workspace <ws-id> --name "MyEventhouse"
  fabio kql-database create --workspace <ws-id> --name "MyDB" --eventhouse-id <eh-id>
  fabio kql-database query --workspace <ws-id> --id <db-id> --kql ".create-merge table ..."
  fabio kql-database query --workspace <ws-id> --id <db-id> --kql ".create-or-alter table ... ingestion json mapping ..."
  fabio eventstream create --workspace <ws-id> --name "MyStream"
  fabio eventstream add-source --workspace <ws-id> --id <es-id> --name "app-source" --source-type CustomEndpoint
  fabio eventstream add-destination --workspace <ws-id> --id <es-id> --name "kql-dest" --destination-type Eventhouse --input-node "app-source-stream" --properties '{"dataIngestionMode":"DirectIngestion","workspaceId":"<ws-id>","itemId":"<kql-db-id>","tableName":"<table>","connectionName":"es-conn-1","mappingRuleName":"<mapping>"}'
  # Send events via Event Hub SDK using connection from:
  fabio eventstream get-source-connection --workspace <ws-id> --id <es-id> --source-id <src-id>
  # Query data:
  fabio kql-database query --workspace <ws-id> --id <db-id> --kql "MyTable | take 10"
  ```

## RTI NL-to-KQL API Behaviors Discovered
- **Endpoint**: `POST /workspaces/{ws}/realTimeIntelligence/nltokql?beta=true` (workspace-scoped, requires `beta=true` query param).
- **Request body (required fields)**: `{"itemIdForBilling": "<item-uuid>", "clusterUrl": "<kusto-uri>", "databaseName": "<db-name>", "naturalLanguage": "<question>"}`. The `itemIdForBilling` is any KQL Database or Eventhouse item ID used for capacity billing.
- **Request body (optional fields)**: `"userShots"` (JSON array of `{"naturalLanguage":"...","kqlQuery":"..."}` examples), `"chatMessages"` (JSON array of `{"role":"User|Assistant","content":"..."}` for multi-turn context).
- **Response**: Returns JSON with `"kqlQuery"` field containing the generated KQL, plus `"explanation"` and other metadata.
- **Authentication**: Uses standard Fabric scope (`https://api.fabric.microsoft.com/.default`).
- **Error on invalid item**: Returns standard Fabric API error if item ID is not found or user lacks permissions.

## Eventhouse API Behaviors Discovered
- **Standard CRUD**: list, show, create, update, delete at `/workspaces/{ws}/eventhouses/{id}`.
- **Definition file**: `EventhouseProperties.json` (PascalCase, NOT `eventhouse.json`).
- **Create is LRO**: Returns 202, requires polling. Creation can take 30-60 seconds.
- **getDefinition is LRO**: Returns 202, requires polling.
- **Endpoint pattern**: `/workspaces/{ws}/eventhouses/{id}`.

## Graph Model API Behaviors Discovered
- **Job type for refresh is `RefreshGraph` (PascalCase)**: The Jobs API uses `?jobType=RefreshGraph` query parameter. The legacy path-based format (`/jobs/refreshGraph/instances`) returns `InvalidJobType`. Must use `POST /workspaces/{ws}/graphModels/{id}/jobs/instances?jobType=RefreshGraph`.
- **Execute query requires `?preview=true`**: The `executeQuery` endpoint requires `?preview=true` query parameter (NOT `?beta=true`). Without it, returns "InvalidParameter: 'preview' is a required parameter".
- **`getQueryableGraphType` also requires `?preview=true`**: Same pattern as executeQuery. Returns 204 No Content when graph has no queryable type (not yet loaded).
- **Fresh graph model only has `.platform` in definition**: A newly created graph model's `getDefinition` only returns the `.platform` metadata file. No `GraphModel.json` part exists until an ontology is linked.
- **Ontology linking via definition on creation**: Pass `GraphModel.json` part in the `definition` at creation time with `{"ontologyId": "<ontology-id>"}`. The API accepts this via LRO (202) but does NOT return the `GraphModel.json` part in subsequent `getDefinition` calls — the link is stored internally.
- **`updateDefinition` with `GraphModel.json` is silently accepted but not persisted**: The server accepts `updateDefinition` with arbitrary content in `GraphModel.json` but doesn't persist it in `getDefinition`. Ontology linking appears to be a creation-time-only operation through the definition.
- **`queryReadiness` field values**: `None` (no graph loaded), potentially `Ready` after successful refresh. Observed in `properties.queryReadiness`.
- **`lastDataLoadingStatus` field**: Contains `status` (`NotStarted`, `InProgress`, `Completed`, `Failed`), `lastUpdateTime`, and `jobInstanceId`. Null before first refresh.
- **Graph must be loaded before queries**: `executeQuery` on an unloaded graph returns error `GraphNotQueryable` with message `GraphIsNotLoaded`.
- **Graph model `show` includes properties**: Unlike many other item types, `GET /graphModels/{id}` returns `properties` with `queryReadiness` and `lastDataLoadingStatus`.
- **`--ontology` flag on create**: fabio wraps the ontology ID in a `GraphModel.json` definition part with `{"ontologyId":"<id>"}` and includes it in the creation request body.
- **Creation with definition is LRO**: When `definition` is included in the creation body, the API returns 202 and requires polling (unlike simple creation without definition which returns the object directly).
- **Refresh requires portal initialization (VersionConfig)**: Graph model refresh via REST API fails with `InternalError: "Job failed to start: VersionConfig does not exist or failed to retrieve ETag."` when the graph model has NOT been initialized through the Fabric portal. The REST API can create a graph model and link an ontology, but the internal loading infrastructure (`VersionConfig`) is only provisioned by the portal's graph editor. This is similar to Data Agent publishing being portal-only.
- **Refresh fails regardless of ontology state**: Even with a properly configured ontology (entity types + data bindings to lakehouse tables), the refresh fails if the graph has never been opened in the portal. Creating fresh graph models with `--ontology` pointing to a fully-bound ontology still produces the `VersionConfig` error.
- **Jobs API reveals actual failure**: The `show` command shows `lastDataLoadingStatus.status: "NotStarted"` even when the job has already `Failed`. Must check the Jobs API directly (`GET /jobs/instances/{jobId}`) to see the real status with `failureReason`.

## Graph Query Set API Behaviors Discovered
- **Definition file is `exportedDefinition.json`**: NOT `definition.json`. The definition uses `exportedDefinition.json` path with structure: `{"dependencies":[],"indirectDependencies":[],"ArtifactContents":[],"ConfigurationCategories":[]}`.
- **`exportedDefinition.json` is read-only (export only)**: The server accepts `updateDefinition` but consistently strips `ArtifactContents`, `dependencies`, and `ConfigurationCategories` values. The content always returns as empty arrays. Query set content is managed only through the portal UI.
- **PATCH update fails on empty query sets**: `PATCH /graphQuerySets/{id}` with `displayName` change returns `GraphQuerySetUpdate.UserError.GraphQuerySetEmpty: Query set payload is empty, cannot update artifact`. This is a server-side limitation — must have content before renaming.
- **Create returns item immediately**: Unlike graph models with definition, graph query set creation returns the item object directly (not LRO).
- **Delete works regardless of content**: Even empty query sets can be deleted successfully.
- **`getDefinition` is LRO**: Returns 202 and requires polling, same as other Fabric definition APIs.

## Map API Behaviors Discovered
- **Definition file is `map.json`**: NOT `definition.json`. The definition part path is `map.json` containing the full map configuration (basemap, data sources, layers).
- **Schema URL**: `https://developer.microsoft.com/json-schemas/fabric/item/map/definition/2.0.0/schema.json` — the current version is 2.0.0.
- **Definition structure**: `{"$schema":"...","basemap":{},"dataSources":[],"iconSources":[],"layerSources":[],"layerSettings":[]}`. A newly created map has all arrays empty and `basemap: {}`.
- **getDefinition is LRO**: Returns 202 and requires polling. Returns `map.json` + `.platform` parts.
- **updateDefinition returns item object**: Unlike other items that return null/empty on update, map `updateDefinition` returns the full item object (id, type, displayName, description, workspaceId).
- **Server adds `refreshIntervalMs: 0`**: Layer sources automatically get `refreshIntervalMs: 0` added if not specified.
- **Data source types**: `Lakehouse`, `KqlDatabase`, `Ontology` (workspace items with `itemType`, `workspaceId`, `itemId`) or `Connection` (with `connectionId`).
- **Layer source types**: `table` (for lakehouse Delta tables). References a data source via `itemId` and uses `relativePath` (e.g., `Tables/my_table`).
- **Layer settings options**: `type` (`vector` or `raster`), `pointLayerType` (`bubble`, `heatmap`, `marker`), with corresponding sub-options (`bubbleOptions`, `heatmapOptions`, `markerOptions`, `lineOptions`, `polygonOptions`, `polygonExtrusionOptions`).
- **Geospatial columns**: Layers reference geographic data via `latitudeColumnName`/`longitudeColumnName` (for point data) or `geometryColumnName` (for GeoJSON/WKT geometry columns). These appear at both the `layerSettings` level and inside `options`.
- **Bubble options for data-driven sizing**: Use `sizeType: "data-driven"` with `sizeProperty: "<column_name>"` to size bubbles proportional to a numeric column. `sizeType: "fixed"` with `fixedSize` for uniform sizing.
- **Basemap styles**: `road`, `satellite_road_labels`, `grayscale_light`, `grayscale_dark`, `night`, `road_shaded_relief`, `high_contrast_dark`, `high_contrast_light`.
- **Controls**: `zoom`, `pitch`, `compass`, `scale`, `traffic`, `style` — each boolean to enable/disable.
- **Filters support**: Layer settings support `filters` array with types: `text`, `boolean`, `number`, `datetime`. Each filter has an `id` (UUID), `field`, `locked` flag, and type-specific value fields.
- **Map visual IDs must be UUID format**: `layerSources[].id` and `layerSettings[].id` must be valid UUIDs.
- **Create is LRO**: Returns 202 and requires polling (item returned after LRO completes).
- **Conflict on duplicate names**: Creating a map with an existing name returns `409 Conflict` with message "Requested '<name>' is already in use".

## Reflex (Activator) API Behaviors Discovered
- **Definition file is `ReflexEntities.json`**: Contains a JSON array of entity objects. Empty reflex = `[]`.
- **Entity structure**: Each entity has `uniqueIdentifier` (GUID, required), `payload` (object, required), and `type` (string, required). Entities reference each other by `uniqueIdentifier`.
- **Entity types**: `container-v1`, `simulatorSource-v1`, `kqlSource-v1`, `realTimeHubSource-v1`, `eventstreamSource-v1`, `fabricItemAction-v1`, `timeSeriesView-v1`.
- **`timeSeriesView-v1` subtypes**: Determined by `payload.definition.type`: `Event`, `Object`, `Attribute`, `Rule`. This single entity type covers events, objects, attributes, and rules.
- **Processing pipeline hierarchy**: Container → Data Source → Event View → Object View → Attribute Views + Rule Views. Each entity references its parent via `payload.parentContainer.targetUniqueIdentifier` and (for attributes/rules) `payload.parentObject.targetUniqueIdentifier`.
- **`definition.instance` is a JSON-encoded string**: The `instance` field contains a stringified JSON template definition (not a nested object). Must be escaped when building the definition file.
- **Template structure**: `{"templateId":"<name>","templateVersion":"1.1","steps":[{"name":"<step>","id":"<guid>","rows":[{"name":"<row>","kind":"<kind>","arguments":[...]}]}]}`.
- **Template IDs for events**: `SourceEvent` (selects from data source), `SplitEvent` (splits by object identity).
- **Template IDs for attributes**: `IdentityPartAttribute` (object identity field), `IdentityTupleAttribute` (composite identity), `BasicEventAttribute` (extracts field value).
- **Template IDs for rules**: `EventTrigger` (fires on event occurrence), `AttributeTrigger` (fires on threshold condition).
- **Rule action types (in ActStep)**: `TeamsMessage` (Teams notification), `EmailMessage` (email notification), `FabricItemInvocation` (runs a Pipeline/Notebook).
- **TeamsMessage action arguments**: `messageLocale`, `recipients` (array), `headline` (array), `optionalMessage` (array), `additionalInformation` (array). All array values use `{"type":"string","value":"..."}` format.
- **EmailMessage action arguments**: `messageLocale`, `sentTo` (array), `copyTo` (array), `bCCTo` (array), `subject` (array), `headline` (array), `optionalMessage` (array), `additionalInformation` (array).
- **FabricItemInvocation action**: References a `fabricItemAction-v1` entity by `uniqueIdentifier`. The action entity defines `fabricItem.itemId`, `fabricItem.workspaceId`, `fabricItem.itemType`, and `jobType`.
- **Rule settings**: `definition.settings.shouldRun` (boolean, enables/disables rule), `definition.settings.shouldApplyRuleOnUpdate` (boolean, apply to historical data).
- **Simulator source types**: `PackageShipment` (with `version: "V2_0"`). Supports `runSettings.startTime` and `runSettings.stopTime` (ISO 8601).
- **KQL source**: Requires `query.queryString` (KQL), `eventhouseItem.targetUniqueIdentifier` (references Eventhouse item), and `runSettings.executionIntervalInSeconds`.
- **Real-time Hub source**: Requires `connection.scope`, `connection.tenantId`, `connection.workspaceId`, `connection.eventGroupType`, and `filterSettings.eventTypes[]`.
- **Eventstream source**: Requires `metadata.eventstreamArtifactId`.
- **updateDefinition does NOT accept `format` field**: Unlike `createItem` which accepts `"format": "json"` in the definition, `updateDefinition` rejects it with `InvalidDefinitionFormat`. Only send `{"definition":{"parts":[...]}}`.
- **updateDefinition returns 200 (not 202 LRO)**: For valid definitions, the API returns 200 immediately. Invalid content returns 400 with `Activator_Alm_GenericError` (500 from internal service).
- **`.platform` part is optional for updateDefinition**: Only `ReflexEntities.json` is required. `.platform` is accepted if `?updateMetadata=true` is set.
- **Container `type` field values**: `samples` (for simulator-based), `kqlQueries` (for KQL-based), and likely others for Real-time Hub and Eventstream containers.
- **AttributeTrigger rule steps**: `ScalarSelectStep` (selects attribute + summary), `ScalarDetectStep` (condition check), optional `DimensionalFilterStep` (filter by another attribute), `ActStep` (action to execute).
- **NumberBecomes operators**: `BecomesGreaterThan`, `BecomesLessThan`, `BecomesGreaterThanOrEqualTo`, `BecomesLessThanOrEqualTo`.
- **NumberSummary operators**: `Average`, `Min`, `Max`, `Sum`, `Count`.
- **TimeDrivenWindowSpec**: `width` and `hop` in milliseconds (e.g., 600000 = 10 minutes).
- **EventTrigger template step structure is undocumented**: The `EventTrigger` template requires an `EventSelector` row, but the correct step/row placement is not documented. Attempts with `EventDetectStep` + `EventSelector` (kind: `Event` or `EventSelector`) all fail with "Expected at least 1 occurrences of EventSelector, but got: 0". Microsoft docs recommend: "configure a Reflex in the Fabric UI, then use Get Item Definition to retrieve the definition." Use `AttributeTrigger` for programmatic rule creation (fully validated).
- **KQL source (`kqlSource-v1`) requires portal initialization**: Always fails via REST API with `Activator_Alm_UserError: "The importArtifactRequest field is required"`. Similar to Graph Model refresh requiring portal `VersionConfig` initialization. Configure KQL sources through the Fabric portal, then manage definitions programmatically afterward.
- **Real-time Hub event subscriptions create server-side state**: When a `realTimeHubSource-v1` is pushed via `updateDefinition`, the server creates an event subscription. If the Reflex is later updated without the same source (or with incorrect UUIDs), subsequent `updateDefinition` calls fail with "eventSubscriptions/{id} not found". Fix: delete the Reflex and create a fresh one.
- **Duplicate entity UUID tracking**: The server tracks entity UUIDs across definition updates. Reusing a UUID from a previously-deleted entity in the same Reflex causes "duplicate" errors. Always use fresh UUIDs when replacing entities.
- **Real-time Hub filter immutability**: Once an RTH source is created with specific `filterSettings.eventTypes`, the filters cannot be updated. The server returns: "Updating event subscription filters is not supported yet. Please create a new source." Must use a completely new `uniqueIdentifier` and fresh subscription.
- **Validated working pipeline patterns**:
  - Simulator source + AttributeTrigger + EmailMessage action (HTTP 200)
  - Simulator source + AttributeTrigger + TeamsMessage action (HTTP 200)
  - Real-time Hub source with workspace events (HTTP 200, creates subscription)
  - `updateDefinition` replaces entire entity set atomically (not incremental)

## Workspace API Behaviors Discovered
- **Endpoint scope**: All workspace operations are tenant-level at `/workspaces/{id}` (no parent scope).
- **Capacity assignment body**: `POST /workspaces/{id}/assignToCapacity` with `{"capacityId": "<id>"}`. Unassign uses empty body `{}` to `POST /workspaces/{id}/unassignFromCapacity`.
- **Capacity assignment is idempotent**: Re-assigning the same capacity succeeds without error.
- **Identity provisioning is LRO**: `POST /workspaces/{id}/provisionIdentity` uses `poll: true` (may return 202). Deprovision is fire-and-forget.
- **Identity provisioning response**: Returns `{"applicationId": "<uuid>", "servicePrincipalId": "<uuid>"}`. Re-provisioning is idempotent — returns the same identity without error.
- **Deprovision identity response**: CLI synthesizes `{"workspaceId": "<id>", "status": "deprovisioned"}` (API returns empty 200).
- **Role assignment validation**: Roles are case-insensitive against `["Admin", "Member", "Contributor", "Viewer"]`. Principal types: `["User", "Group", "ServicePrincipal", "ServicePrincipalProfile"]`.
- **Role assignment body**: `{"principal": {"id": "<principal_id>", "type": "<principal_type>"}, "role": "<role>"}`.
- **Folder management**: Workspaces support folders via `/workspaces/{ws}/folders` (CRUD + move). Move body: `{"targetFolderId": "<id>" | null}` (null moves to root).
- **Tags**: `POST /workspaces/{ws}/applyTags` and `/unapplyTags` with body `{"tagIds": [...]}`.
- **Domain assignment**: `POST /workspaces/{ws}/assignToDomain` with `{"domainId": "<id>"}`. Unassign uses empty body.
- **OneLake settings**: `GET /workspaces/{ws}/onelake/settings` returns tier, diagnostics, immutability. Modify via individual POST endpoints (`/modifyDefaultTier`, `/modifyDiagnostics`, `/modifyImmutabilityPolicy`).
- **Lifecycle policies**: Export/import via `/workspaces/{ws}/onelake/lifecycle/exportPolicy` and `/importPolicy`.
- **Network policy**: `GET/PUT /workspaces/{ws}/networking/communicationPolicy`.
- **Firewall rules**: `GET/PUT /workspaces/{ws}/networking/communicationPolicy/inbound/firewall`. Body: `{"rules":[{"displayName":"<name>","value":"<CIDR>"}]}`. Max 256 rules. PUT replaces all rules.
- **Git outbound policy**: `GET/PUT /workspaces/{ws}/networking/communicationPolicy/outbound/git`. Body: `{"defaultAction":"Allow|Deny","rules":[]}`. Requires Outbound Access Protection (OAP) enabled at tenant level.
- **Inbound Azure resource rules**: `GET/PUT /workspaces/{ws}/networking/communicationPolicy/inbound/azureResourceInstances`. Requires inbound network restriction enabled.
- **Outbound cloud connection rules**: `GET/PUT /workspaces/{ws}/networking/communicationPolicy/outbound/cloudConnections`. Requires OAP enabled.
- **Outbound gateway rules**: `GET/PUT /workspaces/{ws}/networking/communicationPolicy/outbound/gateways`. Requires OAP enabled.
- **Dataset storage format (Power BI API)**: `GET /v1.0/myorg/groups/{id}` returns `defaultDatasetStorageFormat` field (value: `"Small"` or `"Large"`). `PATCH /v1.0/myorg/groups/{id}` with `{"defaultDatasetStorageFormat":"Large"}` changes it. PATCH returns empty 200.
- **`modifyDefaultTier` uses query parameter**: `POST /workspaces/{ws}/onelake/modifyDefaultTier?defaultTier=Hot` with empty body `{}`. NOT a JSON body field. Supported values: `Hot`, `Cool`, `Cold`.
- **Default tier values (corrected)**: `"Hot"`, `"Cool"`, or `"Cold"` (PascalCase). All three tiers are supported.
- **List workspaces `roles` filter**: `GET /workspaces?roles=Admin,Member` supports server-side filtering by the caller's role in the workspace. Comma-separated values.
- **Reset shortcut cache is LRO**: `POST /workspaces/{ws}/onelake/resetShortcutCache` returns 200 or 202 (LRO). Requires `OneLake.ReadWrite.All` scope. Returns `API_ERROR` ("missing or invalid information") on workspaces that have no cached shortcut data — this is a no-op error, not a permission issue.
- **Folder create body**: `POST /workspaces/{ws}/folders` with `{"displayName": "<name>", "description"?: "<desc>", "parentFolderId"?: "<id>"}`. Returns created folder with `id`, `displayName`.
- **Folder move body**: `POST /workspaces/{ws}/folders/{id}/move` with `{"targetFolderId": "<id>"}`. Use `null` or omit to move to workspace root.
- **Folder update returns updated object**: `PATCH /workspaces/{ws}/folders/{id}` with `{"displayName"?: "...", "description"?: "..."}` returns the updated folder object.
- **Folder delete requires empty children**: Deleting a folder with items/subfolders inside returns an error. Delete children first.
- **Network policy GET returns full topology**: `GET /workspaces/{ws}/networking/communicationPolicy` returns an object with `inbound` and `outbound` sections showing all configured rules.
- **Create body**: `{"displayName": "<name>", "description"?: "<desc>"}` — minimal, no capacity needed at creation time.
- **`get-settings` response**: `GET /workspaces/{ws}` returns full workspace object including `id`, `displayName`, `description`, `type`, `capacityId`, `capacityRegion`, `oneLakeEndpoints` (with `blobEndpoint` and `dfsEndpoint`), and `capacityAssignmentProgress` (value: `"Completed"`). The CLI extracts a `properties` sub-object if present; otherwise returns full response.
- **`update-settings` is generic PATCH**: `PATCH /workspaces/{ws}` with free-form JSON body. Same endpoint as `workspace update` but accepts any JSON (vs. `--name`/`--description` flags). Only `displayName` and `description` fields are accepted by the API; unknown fields (e.g., `automaticMetadataSync`) are silently ignored — the response omits them without error.
- **`automaticMetadataSync` is NOT exposed in any REST API**: This setting is portal-only. Passing it in PATCH body is silently dropped. No known REST endpoint configures this property.
- **applyTags/unapplyTags returns API_ERROR on some tenants**: `POST /workspaces/{ws}/applyTags` with `{"tagIds":["<uuid>"]}` returns 400 "The request has an invalid input" on certain tenant configurations. Same body format fails for item-level `POST /workspaces/{ws}/items/{id}/applyTags`. Root cause unknown — body format matches documented spec. Tags CAN be created/deleted via admin API, but workspace/item-level tag application fails. May require a specific tenant setting or license level not yet identified.
- **OAP outbound restriction requires paid capacity (F64+)**: `PUT /workspaces/{ws}/networking/communicationPolicy` with outbound `defaultAction: "Deny"` returns FORBIDDEN ("Enabling outbound restriction is not allowed") on Trial (FTL4) capacity. All outbound sub-rule SET commands (git-outbound, cloud-connections, gateways) depend on outbound restriction being enabled first.
- **OAP inbound restriction works on Trial**: `PUT /workspaces/{ws}/networking/communicationPolicy` with inbound `defaultAction: "Deny"` succeeds on Trial capacity. However, `GET .../inbound/azureResourceInstances` returns NOT_FOUND even with inbound restriction enabled — requires actual Azure Private Endpoint infrastructure to populate.
- **Git outbound policy GET works without outbound restriction**: `GET .../outbound/git` returns `{"defaultAction":"Deny"}` even when workspace-level outbound restriction is not enabled. Only the SET (PUT) operation requires OAP to be active.
- **Tenant settings for networking**: `WorkspaceBlockOutboundAccess` and `WorkspaceBlockInboundAccess` must be enabled at tenant level (via admin API) as prerequisites for workspace-level networking policies. `AllowAccessOverPrivateLinks` controls private link access but does not affect the tag or basic networking functionality.

## Item API Behaviors Discovered
- **Type filter on list**: `GET /workspaces/{ws}/items?type={ItemType}` filters server-side. Type values are PascalCase (e.g., `Lakehouse`, `Notebook`, `Warehouse`).
- **Valid item types for create**: `CopyJob`, `Dashboard`, `DataAgent`, `DataPipeline`, `Dataflow`, `Environment`, `Eventhouse`, `Eventstream`, `GraphQLApi`, `KQLDashboard`, `KQLDatabase`, `KQLQueryset`, `Lakehouse`, `MLExperiment`, `MLModel`, `MirroredDatabase`, `MirroredWarehouse`, `Notebook`, `Ontology`, `Paginated Report`, `Reflex`, `Report`, `SQLDatabase`, `SQLEndpoint`, `SemanticModel`, `SparkJobDefinition`, `Warehouse`. Sorted, PascalCase. Hinted on invalid type errors.
- **Copy pattern**: `getDefinition` (LRO) from source → `GET` source metadata → `POST /workspaces/{dest}/items` with definition (LRO). Result includes new item's `id`, `displayName`, `type`.
- **Move pattern**: Copy + `DELETE /workspaces/{source}/items/{id}`. Atomic: delete only after successful copy.
- **Definition format query param**: `POST /workspaces/{ws}/items/{id}/getDefinition?format={fmt}` supports format selection.
- **Update definition metadata**: `POST /workspaces/{ws}/items/{id}/updateDefinition?updateMetadata=true` updates `.platform` metadata alongside definition parts.
- **Bulk operations (all LRO)**:
  - `POST /workspaces/{ws}/items/bulkExportDefinitions` — exports multiple item definitions
  - `POST /workspaces/{ws}/items/bulkImportDefinitions` — imports multiple item definitions
  - `POST /workspaces/{ws}/items/bulkMove` — moves multiple items between folders/workspaces
- **External data shares**: CRUD at `/workspaces/{ws}/items/{id}/externalDataShares`. Create body: `{"paths": [...], "recipient": {"tenantId": "<id>"}}`. Accept invitations at `/externalDataShares/invitations/{id}/accept`. Supports polymorphic recipients: add `"userPrincipal": {"userPrincipalName": "<upn>"}` or `"servicePrincipal": {"id": "<sp-id>"}` to the `recipient` object.
- **Move to folder**: `POST /workspaces/{ws}/items/{id}/move` with `{"targetFolderId": "<id>"}`. Omit `targetFolderId` or pass `null` to move to workspace root.
- **Identity assignment**: `POST /workspaces/{ws}/items/{id}/identities/default/assign`.
- **Tags**: `POST /workspaces/{ws}/items/{id}/applyTags` and `/unapplyTags` with `{"tagIds": [...]}`.
- **Hard delete query param**: `DELETE /workspaces/{ws}/items/{id}?hardDelete=true` permanently deletes (skips recycle bin). Supported on all workspace-scoped item types.
- **List server-side filtering**: `GET /workspaces/{ws}/items` supports query params: `type={ItemType}` (single type filter), `rootFolderId={folderId}` (items in a specific folder), `recursive={true|false}` (include items in subfolders), `include={type1,type2}` (additional metadata to include in response). The `--folder`, `--recursive`, and `--include` CLI flags map to these query params.

## Lakehouse API Behaviors Discovered
- **Load table format validation**: Only `"Csv"` and `"Parquet"` are valid (PascalCase). JSON is NOT supported by the Fabric REST API. Mode values: `"Overwrite"`, `"Append"` (PascalCase).
- **Load table body (Csv)**: `{"relativePath": "<path>", "pathType": "File", "mode": "Overwrite", "formatOptions": {"format": "Csv", "header": true, "delimiter": ","}}`. The `format` key is INSIDE `formatOptions` (discriminated union pattern).
- **Load table body (Parquet)**: `{"relativePath": "<path>", "pathType": "File", "mode": "Overwrite", "formatOptions": {"format": "Parquet"}}`. Do NOT include `header`/`delimiter` with Parquet — API rejects mixed format options.
- **Load table with schema (multi-schema lakehouses)**: `POST /workspaces/{ws}/lakehouses/{id}/schemas/{schemaName}/tables/{table}/load?beta=true`. Uses same body format as standard load-table. Requires `?beta=true` query param. Falls back to standard path when `--schema` is not specified.
- **Upload-table workflow**: Upload file to `Files/.staging/{filename}` → POST load-table → delete staging file (best-effort cleanup).
- **Table listing uses `"data"` key**: Unlike other list endpoints that use `"value"`, `GET /workspaces/{ws}/lakehouses/{id}/tables` returns `{"data": [...]}`.
- **Shortcut creation**: `POST /workspaces/{ws}/items/{id}/shortcuts` with body `{"name": "<name>", "path": "<target_path>", "target": {<target_type>: <target_config>}}`. Optional `?shortcutConflictPolicy={policy}` query param (`Abort` or `GenerateUniqueName`).
- **Bulk shortcut creation**: `POST /workspaces/{ws}/items/{id}/shortcuts/bulkCreate?shortcutConflictPolicy={policy}` with `{"createShortcutRequests": [...]}`. LRO.
- **Shortcut get/delete path**: `GET/DELETE /workspaces/{ws}/items/{id}/shortcuts/{path}/{name}` — path and name are URL path segments.
- **Enable schemas on create**: `{"displayName": "...", "creationPayload": {"enableSchemas": true}}` enables multi-schema lakehouse.
- **Sync algorithm**: Lists both source and destination from root (avoiding DFS virtual view doubling), builds file maps keyed by relative path, compares ETags (default) or Content-MD5 (`--checksum`), copies files with different/missing ETags, optionally deletes orphan files at destination (`--delete`).
- **Parallel execution**: All multi-file operations (upload, copy-file, move-file, delete-table, copy-table, move-table, sync) use concurrent execution with rate-limit retry.
- **Glob patterns**: Local globs via `glob::glob()`, remote globs via listing + pattern match, table globs via table list API + pattern match.
- **Materialized views**: `POST /workspaces/{ws}/lakehouses/{id}/jobs/refreshMaterializedLakeViews/instances` triggers refresh. Schedule management at `.../jobs/refreshMaterializedLakeViews/schedules`.
- **Table maintenance**: `POST /workspaces/{ws}/items/{id}/jobs/instances?jobType=TableMaintenance` with `executionData` payload. NOT the legacy path-based endpoint.
- **Optimize-table payload**: `{"executionData": {"tableName": "X", "optimizeSettings": {"vOrder": true, "zOrderBy": ["col1","col2"]}}}`. The `vOrder` flag enables V-Order compaction. `zOrderBy` is optional — accepts an array of column names for Z-Order clustering.
- **Vacuum-table payload**: `{"executionData": {"tableName": "X", "vacuumSettings": {"retentionPeriod": "7:00:00:00"}}}`. Retention format is `D:HH:MM:SS` (days:hours:minutes:seconds). Example: 30 hours → `"1:06:00:00"`, 48 hours → `"2:00:00:00"`, 168 hours (default) → `"7:00:00:00"`.
- **Table maintenance schema support**: Both optimize and vacuum accept optional `"schemaName"` in `executionData` for multi-schema lakehouses.
- **Table maintenance response**: Returns 202 (accepted) with job instance details, or empty body on some capacity sizes. Fire-and-forget (no LRO polling needed — job runs asynchronously).
- **Table-schema via Delta log**: Read table schema without Spark/SQL by downloading `_delta_log/*.json` commit files from OneLake DFS. Delta commit files are NDJSON (newline-delimited JSON). The `metaData` action contains `schemaString` which is a JSON-encoded string of the Spark StructType schema.
- **Delta log path**: `Tables/{tableName}/_delta_log/` contains numbered JSON commit files (e.g., `00000000000000000000.json`). Schema may only exist in the first commit or in commits that change the schema — must iterate from newest to oldest.
- **Delta schemaString format**: `{"type":"struct","fields":[{"name":"col1","type":"string","nullable":true,"metadata":{}}]}`. Field types include: `string`, `integer`, `long`, `double`, `float`, `boolean`, `date`, `timestamp`, `binary`, `decimal(P,S)`, plus complex types (`array<T>`, `map<K,V>`, `struct<...>`).
- **DFS directory listing for Delta log**: Use `list_onelake_files(ws, id, Some("Tables/{name}/_delta_log"))`. Returns file paths that may include the item-id prefix (e.g., `{item_id}/Tables/...`). Strip prefix before downloading.
- **No checkpoint support**: Current implementation only reads `.json` commit files (matching Microsoft's `fab` CLI behavior). For tables with 10+ commits, the schema may exist only in a Parquet checkpoint — not yet handled.
- **Livy sessions**: `GET /workspaces/{ws}/lakehouses/{id}/livySessions` lists active sessions.
- **Get/Update definition**: LRO via `/workspaces/{ws}/lakehouses/{id}/getDefinition` and `/updateDefinition`.

## Notebook API Behaviors Discovered
- **Creation uses generic items endpoint**: `POST /workspaces/{ws}/items` with `{"type": "Notebook", "displayName": "...", "definition": {...}}`. NOT `/notebooks`.
- **Delete uses generic items endpoint**: `DELETE /workspaces/{ws}/items/{id}` (not `/notebooks/{id}`).
- **ipynb format**: Definition uses `"format": "ipynb"` with part path `notebook-content.py`. The payload is a base64-encoded Jupyter notebook JSON.
- **Cell source must be list of strings**: Each cell's `source` field is an array of strings (one per line with `\n` suffix), NOT a single string.
- **Lakehouse binding via `trident` metadata**: `--lakehouse` flag injects `metadata.trident.lakehouse` into the ipynb JSON with `default_lakehouse`, `default_lakehouse_name`, `default_lakehouse_workspace_id`, `known_lakehouses`.
- **Run mechanism**: `client.run_notebook(workspace, id)` → `POST /workspaces/{ws}/items/{id}/jobs/instances?jobType=RunNotebook`. Returns 202 + Location header with job instance URL.
- **Status polling (--wait)**: Polls `GET /workspaces/{ws}/items/{id}/jobs/instances/{job_id}` every 5 seconds. Default timeout 600s.
- **Terminal statuses**: `Completed`, `Failed`, `Cancelled`. Continue polling on `NotStarted`, `InProgress`, `Deduped`.
- **Failure info**: Extracted from `failureReason.message` in job instance response.
- **Cancel**: `POST /workspaces/{ws}/items/{id}/jobs/instances/{job_id}/cancel`.
- **Get job instance (beta)**: `GET /workspaces/{ws}/notebooks/{id}/jobs/execute/instances/{job_id}?beta=true` — uses notebook-specific path with beta flag.
- **Livy sessions**: `GET /workspaces/{ws}/notebooks/{id}/livySessions` lists active Livy sessions for a notebook.
- **Spark cold start**: First notebook run on small capacity can take 2-5 minutes to transition from `NotStarted` to `InProgress`.
- **Run with parameters**: `POST /workspaces/{ws}/items/{id}/jobs/instances?jobType=RunNotebook` accepts optional body `{"parameters": [{"name":"p1","value":"v1","type":"Text"}], "executionData": {"computeType": "..."}}`. `--parameters` is a JSON array of name/value/type objects. `--compute-type` wraps in executionData. `--execution-data` provides full JSON override.
- **Parameter type values**: `Text`, `Int`, `Long`, `Double`, `Bool`, `DateTime` (match Fabric Notebook parameter types).
- **executionData fields**: `computeType` (e.g., `"Spark"`, `"DataFactory"`) plus other job-type-specific fields. `--execution-data` JSON is merged into the request body directly.

## Environment API Behaviors Discovered
- **Staging/publish workflow**: Changes are staged first, then published as a separate step. All modifications go to staging area.
- **Publish is fire-and-forget**: `POST /workspaces/{ws}/environments/{id}/staging/publish` with empty body `{}`. Not LRO — returns immediately.
- **Cancel publish**: `POST /workspaces/{ws}/environments/{id}/staging/cancelPublish` with empty body.
- **Spark settings dual endpoints**: `GET .../sparkcompute` (published) vs `GET .../staging/sparkcompute` (pending changes). Update goes to staging: `PATCH .../staging/sparkcompute`.
- **Definition file**: Part path is `environment.metadata.json`.
- **Library management**: Published at `/libraries`, staging at `/staging/libraries`. Delete uses query param: `DELETE .../staging/libraries?libraryToDelete={name}`.
- **External libraries**: Export via `GET .../libraries/exportExternalLibraries`. Import via `POST .../staging/libraries/importExternalLibraries`. Remove via `POST .../staging/libraries/removeExternalLibrary` with `{"libraryToRemove": "<name>"}`.
- **Upload staging library**: `POST /workspaces/{ws}/environments/{id}/staging/libraries/{libraryName}` with `Content-Type: application/octet-stream` body. Library name defaults to the filename if `--library-name` not specified. Returns 200 on success.
- **Get/Update definition are LRO**: Both use `poll: true`.
- **Create is LRO**: Returns 202, requires polling.

## Mirrored Database API Behaviors Discovered
- **Definition file**: Part path is `mirroring.json`.
- **Start/stop mirroring**: `POST /workspaces/{ws}/mirroredDatabases/{id}/startMirroring` and `/stopMirroring` with empty body `{}`. Fire-and-forget (no LRO).
- **Status endpoints use GET (not POST)**: Despite verb-like paths, `GET .../getMirroringStatus` and `GET .../getTablesMirroringStatus` are GET requests.
- **Create uses type-specific endpoint**: `POST /workspaces/{ws}/mirroredDatabases` (not generic `/items`). No `"type"` field needed in body — endpoint implies type.
- **Create is LRO**: Returns 202, requires polling.
- **Get/Update definition are LRO**: Both use `poll: true`.

## Deployment Pipeline API Behaviors Discovered
- **Tenant-level scope**: All endpoints use `/deploymentPipelines/{id}` (NO `/workspaces/` prefix). Pipelines are not workspace-scoped.
- **Deploy body**: `{"sourceStageId": "<id>", "targetStageId"?: "<id>", "items"?: [...], "note"?: "<text>"}`. `targetStageId` optional (defaults to next stage). `items` optional (defaults to all items).
- **Deploy is LRO**: `POST /deploymentPipelines/{id}/deploy` with `poll: true`. May return empty/null response (treated as "accepted").
- **Items array format**: `[{"itemId": "...", "itemType": "Notebook"}]` — PascalCase item types.
- **Stage management**: `GET .../stages` lists stages. `GET .../stages/{stageId}/items` lists items in stage. Items have `itemDisplayName`, `itemId`, `itemType` fields.
- **Workspace assignment**: `POST .../stages/{stageId}/assignWorkspace` with `{"workspaceId": "<id>"}`. Unassign uses empty body.
- **Operations history**: `GET .../operations` lists past deployments. `GET .../operations/{opId}` shows details.
- **Role assignments**: `GET/POST .../roleAssignments`. Delete uses principal ID: `DELETE .../roleAssignments/{principalId}`.
- **Role assignment body**: `{"principal": {"id": "<id>", "type": "<type>"}, "role": "<role>"}`.
- **Permissions**: Deploy requires "Contributor"; all other mutations require "Admin".

## Domain API Behaviors Discovered
- **Admin scope**: All domain endpoints use `/admin/domains/{id}` prefix. Requires admin privileges.
- **Batch workspace assignment**: `POST /admin/domains/{id}/assignWorkspaces` with `{"workspacesIds": [...]}`. Unassign uses same pattern at `/unassignWorkspaces`.
- **Assign by capacity**: `POST /admin/domains/{id}/assignWorkspacesByCapacities` with `{"capacitiesIds": [...]}`.
- **Assign by principal**: `POST /admin/domains/{id}/assignWorkspacesByPrincipals` with body containing principals array and `type` field.
- **List domain workspaces**: `GET /admin/domains/{id}/workspaces` returns workspaces associated with domain.
- **Create body**: `{"displayName": "<name>", "description"?: "<desc>"}`.
- **Update uses PATCH**: `PATCH /admin/domains/{id}` with `{"displayName"?: "...", "description"?: "..."}`.

## Connection API Behaviors Discovered
- **Tenant-level scope**: All connection endpoints use `/connections/{id}` (no workspace prefix). Connections are shared across workspaces.
- **Connectivity types**: `ShareableCloud`, `OnPremises`, `VirtualNetworkGateway`, `PersonalCloud`.
- **Credential types**: `Basic`, `OAuth2`, `Key`, `Anonymous`, `ServicePrincipal`, `SharedAccessSignature`, `WorkspaceIdentity`, `KeyPair`.
- **Privacy levels**: `None`, `Public`, `Organizational`, `Private`.
- **Parameters format conversion**: User provides JSON object `{"key": "value"}` which is converted to array format `[{"dataType": "Text", "name": "key", "value": "value"}]` for the API.
- **Create body structure**: `{"displayName": "...", "connectivityType": "...", "connectionDetails": {"type": "...", "creationMethod": "...", "parameters": [...]}, "credentialDetails": {"singleSignOnType": "None", "connectionEncryption": "NotEncrypted", "skipTestConnection": bool, "credentials": {"credentialType": "..."}}, "privacyLevel": "..."}`.
- **Test connection**: `POST /connections/{id}/testConnection` with empty body `{}`.
- **Role assignments**: Full CRUD at `/connections/{id}/roleAssignments/{assignmentId}`. Roles: `Owner`, `User`, `UserWithReshare`.
- **Role assignment body**: `{"principal": {"id": "...", "type": "User|Group|ServicePrincipal"}, "role": "Owner|User|UserWithReshare"}`.
- **List supported types**: `GET /connections/supportedConnectionTypes` returns all available connection type definitions.

## Spark API Behaviors Discovered
- **Workspace-level settings**: `GET/PATCH /workspaces/{ws}/spark/settings`.
- **Workspace pools**: CRUD at `/workspaces/{ws}/spark/pools/{poolId}`.
- **Capacity-level settings (beta)**: `GET/PATCH /capacities/{capId}/spark/settings?beta=true`.
- **Capacity pools (beta)**: CRUD at `/capacities/{capId}/spark/pools/{poolId}?beta=true`.
- **Livy sessions**: `GET /workspaces/{ws}/spark/livySessions` and `GET .../livySessions/{id}`.
- **Pool create body**: Accepts JSON from `--file` or `--content` with pool configuration (name, node size, auto-scale settings, dynamic executor allocation).
- **Settings update**: PATCH with JSON body from `--file` or `--content`.
- **Beta flag required for capacity-level operations**: All capacity-scoped Spark endpoints require `?beta=true` query parameter.

## Spark Job Definition API Behaviors Discovered
- **Definition file**: Uses type-specific endpoint `/workspaces/{ws}/sparkJobDefinitions/{id}/getDefinition` and `/updateDefinition`.
- **Run job type**: `POST /workspaces/{ws}/items/{id}/jobs/instances?jobType=sparkjob` (lowercase `sparkjob`).
- **Create is LRO**: `POST /workspaces/{ws}/sparkJobDefinitions` with `poll: true`.
- **Get/Update definition are LRO**: Both use `poll: true`.
- **Definition format**: JSON content with Spark job configuration (main file path, arguments, language, etc.).

## Data Pipeline API Behaviors Discovered
- **Run job type**: `POST /workspaces/{ws}/items/{id}/jobs/instances?jobType=Pipeline` (PascalCase `Pipeline`).
- **Definition file**: Uses `/workspaces/{ws}/dataPipelines/{id}/getDefinition` and `/updateDefinition`. Both LRO.
- **Schedule management**: `POST /workspaces/{ws}/dataPipelines/{id}/jobs/execute/schedules` creates a schedule. Note: uses `/jobs/execute/schedules` (not `/jobs/Pipeline/schedules`).
- **Create is LRO**: `POST /workspaces/{ws}/dataPipelines` with `poll: true`.

## KQL Database API Behaviors Discovered
- **Query endpoint routing**: Management commands (starting with `.`) use `/v1/rest/mgmt`; data queries use `/v2/rest/query`. Both at the Kusto query URI.
- **Query body**: `{"db": "<database_name>", "csl": "<kql_text>"}`.
- **Token scoping**: Acquires token scoped to `{kusto_uri}/.default` (not the standard Fabric scope).
- **Query URI resolution priority**: `properties.queryServiceUri` → `properties.queryUri` → `properties.databaseUrl` → `--query-uri` override. Falls back to error with hint.
- **Database name**: Uses `displayName` from the KQL database item metadata.
- **V1 response format**: `{"Tables": [{"TableName": "...", "Columns": [...], "Rows": [[...], ...]}]}`. Uses first table as primary result.
- **V2 response format**: Array of frames. Finds `DataTable` frame with `TableKind: "PrimaryResult"`. Checks `DataSetCompletion` frame for `HasErrors`.
- **Shortcuts**: `GET /workspaces/{ws}/items/{id}/shortcuts` lists shortcuts on KQL databases.
- **Create types**: `ReadWrite` and `ReadOnlyFollowing`. ReadWrite requires `--eventhouse-id` in creation payload. ReadOnlyFollowing requires source database reference.
- **Get/Update definition are LRO**: Both use `poll: true` at type-specific endpoints.

## OneLake Security API Behaviors Discovered
- **Upsert-all pattern**: `PUT /workspaces/{ws}/items/{id}/dataAccessRoles` replaces ALL roles atomically. There is no individual role create/update endpoint.
- **Delete pattern**: GET all roles → filter out target role → PUT remaining roles back. Errors if role not found.
- **Show pattern**: GET all roles → find by name (client-side filter). No server-side individual GET.
- **Body format**: PUT body is the complete array of role definitions. Each role has `name` and members/permissions.
- **No individual role endpoints**: All CRUD operations go through the same PUT endpoint with the full role set.
- **Create (POST) endpoint**: `POST /workspaces/{ws}/items/{id}/dataAccessRoles?dataAccessRoleConflictPolicy={policy}` creates a single role. Accepts the role JSON directly as body (not wrapped in array).
- **Conflict policy values**: `Abort` (default — fails if role exists) or `Overwrite` (replaces existing role with same name). Query parameter: `dataAccessRoleConflictPolicy`.
- **Native show by roleName**: `GET /workspaces/{ws}/items/{id}/dataAccessRoles/{roleName}` returns a single role directly (no client-side filtering needed).
- **Native delete by roleName**: `DELETE /workspaces/{ws}/items/{id}/dataAccessRoles/{roleName}` removes a single role without requiring GET-all + PUT-minus-one pattern.
- **Role JSON input**: `--role` accepts inline JSON or `@path/to/file.json` (file prefix). Validated client-side before sending.

## Managed Private Endpoint API Behaviors Discovered
- **Create body**: `{"name": "<endpoint_name>", "privateLinkResourceId": "<ARM_resource_id>", "groupId": "<subresource_type>", "requestMessage"?: "<approval_message>"}`.
- **Group ID values**: `blob`, `sqlServer`, `dfs`, `queue`, etc. (maps to Azure resource sub-resource types).
- **Create is LRO**: Returns 202, requires polling.
- **No update**: Endpoints are immutable after creation. Only create and delete.
- **Response status fields**: `provisioningState` and `connectionState` track endpoint lifecycle.
- **Requires Admin role**: All mutations require workspace Admin.

## Capacity API Behaviors Discovered
- **Dual API design**: Read operations (list/show) use Fabric API (`api.fabric.microsoft.com/v1/capacities`). Lifecycle operations (suspend/resume/create/update/delete) use ARM API (`management.azure.com`).
- **ARM API version**: `2023-11-01` for all capacity lifecycle operations.
- **ARM resource path**: `/subscriptions/{sub}/resourceGroups/{rg}/providers/Microsoft.Fabric/capacities/{name}`.
- **Capacity name constraints**: 3-63 chars, pattern `^[a-z][a-z0-9]*$` (lowercase only, starts with letter).
- **ARM auth scope**: `https://management.azure.com/.default` — separate from Fabric scope. Requires Azure RBAC (Contributor) on the capacity resource.
- **Create (PUT)**: Returns 200/201 directly or 202 with LRO. Body: `{"location": "...", "sku": {"name": "F2", "tier": "Fabric"}, "properties": {"administration": {"members": ["admin@..."]}}}`.
- **Update (PATCH)**: Supports partial updates — sku, admin, tags individually. Returns 200 or 202 with LRO.
- **Delete (DELETE)**: Returns 202 with LRO or 204 (no content).
- **Suspend/Resume (POST)**: `POST .../suspend` and `POST .../resume` with empty body. Returns 202 with LRO.
- **ARM LRO pattern**: Uses `Azure-AsyncOperation` header (preferred) or `Location` header. Poll body has `status` field: `Succeeded`, `Failed`, `Canceled`, or in-progress values.
- **List SKUs**: `GET /subscriptions/{sub}/providers/Microsoft.Fabric/skus?api-version=2023-11-01` returns available SKU names and regions.
- **Check name**: `POST /subscriptions/{sub}/providers/Microsoft.Fabric/locations/{location}/checkNameAvailability?api-version=2023-11-01` with `{"name": "...", "type": "Microsoft.Fabric/capacities"}`. Returns `{"nameAvailable": true/false}`.
- **SKU values**: F2, F4, F8, F16, F32, F64, F128, F256, F512, F1024, F2048 (Fabric tier).
- **State values**: Includes `Active`, `Inactive` (paused/suspended), `Provisioning`, `Deleting`.
- **Tenant-level scope (Fabric)**: `GET /capacities` (no workspace context). Individual: `GET /capacities/{id}`.
- **Response fields**: `displayName`, `id`, `sku`, `region`, `state`.

## Job Scheduler API Behaviors Discovered
- **Generic item-scoped**: All endpoints use `/workspaces/{ws}/items/{id}/jobs/...` pattern (works for any item type).
- **Job type required**: Most endpoints include `{job_type}` in path: `/jobs/{job_type}/schedules`.
- **Run on demand**: `POST /workspaces/{ws}/items/{id}/jobs/instances?jobType={job_type}` with optional body.
- **Run on demand response**: Returns 202 + `Location` header containing the job instance URL. Extract job ID from `Location` path segment.
- **Cancel**: `POST /workspaces/{ws}/items/{id}/jobs/instances/{instance_id}/cancel`.
- **Schedule CRUD**: At `/workspaces/{ws}/items/{id}/jobs/{job_type}/schedules/{schedule_id}`.
- **Create schedule body**: Includes `enabled`, `configuration` with cron or interval settings.
- **Known job types**: Vary by item type — `RunNotebook`, `Pipeline`, `sparkjob`, `RefreshGraph`, `refreshMaterializedLakeViews`, `TableMaintenance`, etc.
- **`--wait` polling**: Polls `GET /workspaces/{ws}/items/{id}/jobs/instances/{job_id}` every 5 seconds. Terminal statuses: `Completed`, `Failed`, `Cancelled`. Continue on: `NotStarted`, `InProgress`, `Deduped`.
- **`--timeout` default**: 600 seconds. On timeout without `--cancel-on-timeout`, returns TIMEOUT error with hint showing how to check status manually.
- **`--cancel-on-timeout`**: Fires `POST .../cancel` on the job instance, then returns TIMEOUT error. Cancel is best-effort.
- **Job ID extraction from Location header**: Pattern: `/workspaces/{ws}/items/{id}/jobs/instances/{job_id}`. Falls back to `x-ms-operation-id` header, then response body `id` field.
- **TableMaintenance cold start**: On small capacity (F2), table maintenance jobs can take 2-5 minutes to complete due to Spark session allocation. First run is always slowest.
- **Fire-and-forget mode**: Without `--wait`, returns immediately with `{"status":"accepted","jobId":"..."}` after recording in local job ledger.

## Copy Job API Behaviors Discovered
- **Definition file**: Part path is `CopyJobV1.json`.
- **Create is LRO**: `POST /workspaces/{ws}/copyJobs` with `poll: true`.
- **Get/Update definition are LRO**: Both use `poll: true`. Get Definition sends empty body `{}`.
- **Required roles**: Create/Delete require "Member"; Update/Definition require "Contributor".

## Dataflow API Behaviors Discovered
- **Definition file**: Part path is `dataflow.json`.
- **Create is LRO**: `POST /workspaces/{ws}/dataflows` with `poll: true`.
- **Get/Update definition are LRO**: Both use `poll: true`. Get Definition sends empty body `{}`.
- **Required roles**: Create/Delete require "Member"; Update/Definition require "Contributor".
- **Identical structure to Copy Job**: Same LRO patterns, same role requirements, different definition file name.
- **Discover parameters**: `GET /workspaces/{ws}/dataflows/{id}/parameters` returns paginated list of M parameters. Uses standard `get_list()` with `"value"` key.
- **Run job types**: Two job types — `execute` (default, runs the dataflow) and `applyChanges` (applies pending definition changes). Endpoints: `POST /workspaces/{ws}/dataflows/{id}/jobs/execute/instances` and `.../jobs/applyChanges/instances`.
- **Run executionData**: Optional body with `executionOption` ("NoRefreshDuringSave", "AutomaticRefresh") and `parameters` (JSON object). Only applies to `execute` job type; `applyChanges` rejects `executionData` with API_ERROR.
- **Run with --wait**: Polls job status at `/workspaces/{ws}/items/{id}/jobs/instances/{job_id}` every 5s. Terminal states: `Completed`, `Failed`, `Cancelled`. Supports `--timeout` (default 600s) and `--cancel-on-timeout`.
- **Execute query endpoint**: `POST /workspaces/{ws}/dataflows/{id}/executeQuery` with body `{"queryName": "<name>", "customMashupDocument"?: "<M expression>"}`. Returns binary Apache Arrow IPC stream (NOT JSON).
- **Execute query response handling**: Binary response saved to `--file` path. If `--file` is not specified, reports metadata only (size in bytes). Uses `post_fabric_bytes()` method for binary response.
- **Execute query requires Contributor role**: Returns 403 without sufficient permissions.

## SQL Database API Behaviors Discovered
- **Creation modes**: `New` (fresh database), `Restore` (point-in-time restore from existing), `RestoreDeletedDatabase` (restore from deleted). Each mode has different `creationPayload` fields.
- **Create body (New)**: `{"displayName": "...", "creationPayload": {"creationMode": "New", "backupRetentionDays": 7, "collation": "..."}}`.
- **Restore body**: Requires `restorePointInTime` (ISO 8601) and `sourceDatabaseReference` with `workspaceId` + `id`.
- **Hard delete**: `DELETE /workspaces/{ws}/sqlDatabases/{id}?hardDelete=true` permanently removes (vs soft delete for restore).
- **List deleted**: `GET /workspaces/{ws}/sqlDatabases/restorableDeletedDatabases` lists soft-deleted databases available for restore.
- **TDS connection resolution**: `GET /workspaces/{ws}/sqlDatabases/{id}` → extracts `properties.serverFqdn` (may include port as `host,1433`) and `properties.databaseName` (falls back to `displayName`).
- **SQL auth token**: Uses `client.require_sql_auth()` for SQL-scoped AAD token.
- **Connection string output**: `Server=tcp:{server},{port};Initial Catalog={database};Encrypt=True;TrustServerCertificate=False;Authentication=ActiveDirectoryDefault`.
- **Import type inference**: `Unknown` → first non-empty observation sets type → subsequent observations widen (Int→BigInt→Float→NVarChar, never narrows). JSON number with i32 fit → Int, else BigInt. Strings try parse order: Int→BigInt→Float→Bit→Date→NVarChar(len).
- **Import SQL generation**: `CREATE TABLE [dbo].[{name}] (...)` with nullable columns. Batched `INSERT INTO ... VALUES` (default batch_size=100, 120s timeout per batch). Optional `DROP TABLE IF EXISTS`.
- **NVarChar length calculation**: `clamp(observed_max_len * 2, 50, 4000)` — doubles observed length with floor/ceiling.
- **Mirroring support**: `POST .../startMirroring` and `POST .../stopMirroring` (same pattern as Mirrored Database).
- **Audit settings**: `GET/PATCH .../settings/sqlAudit`. Body: `{"state": "Enabled|Disabled", "retentionDays": N, "auditActionsAndGroups": [...], "predicateExpression": "..."}`.
- **Definition formats**: Supports `dacpac` and `sqlproj` via `?format={fmt}` query parameter.
- **Revalidate CMK**: `POST .../revalidateCMK` (LRO) — revalidates customer-managed key encryption.
- **F4+ capacity requirement**: SQL Database TDS connections require F4+ capacity. F2 fails with error 18456 State 240.

## KQL Dashboard API Behaviors Discovered
- **Definition file**: Part path is `RealTimeDashboard.json`.
- **Endpoint pattern**: Standard CRUD at `/workspaces/{ws}/kqlDashboards/{id}`.
- **Get/Update definition are LRO**: Both use `poll: true` at type-specific endpoints.
- **Create is LRO**: `POST /workspaces/{ws}/kqlDashboards` with `poll: true`.

## ML Model API Behaviors Discovered
- **CRUD only**: No definition support (no getDefinition/updateDefinition).
- **Endpoint pattern**: Standard at `/workspaces/{ws}/mlModels/{id}`.
- **Create body**: `{"displayName": "...", "description"?: "..."}`.
- **Create is LRO**: Returns 202, requires polling.

## ML Experiment API Behaviors Discovered
- **CRUD only**: No definition support (no getDefinition/updateDefinition).
- **Endpoint pattern**: Standard at `/workspaces/{ws}/mlExperiments/{id}`.
- **Create body**: `{"displayName": "...", "description"?: "..."}`.
- **Create is LRO**: Returns 202, requires polling.

## Anomaly Detector API Behaviors Discovered
- **Definition format**: `AnomalyDetectorV1`. Definition file path is `Configurations.json` (NOT `AnomalyDetector.json`).
- **Definition schema URL**: `https://developer.microsoft.com/json-schemas/fabric/item/anomalyDetector/definition/1.0.0/schema.json`
- **Definition structure**: `{"$id": "<schema_url>", "$schema": "https://json-schema.org/draft-07/schema#", "univariateConfigurations": []}`. The `univariateConfigurations` array holds the anomaly detection model configurations.
- **Create is LRO**: Returns via standard LRO polling.
- **getDefinition is LRO**: Returns 202, polled to completion. Returns `Configurations.json` + `.platform` parts.
- **Response includes `attributes` field**: Item responses include `"attributes": []` (empty array for new items).
- **Endpoint pattern**: Standard at `/workspaces/{ws}/anomalyDetectors/{id}`.
- **409 Conflict on duplicate name**: Creating with an existing name returns `"Requested '<name>' is already in use"`.

## Common API Patterns Across All Command Groups
- **List pagination**: All list endpoints use `get_list()` with `"value"` key (except lakehouse tables which use `"data"`). Supports `--all` (fetches all pages), `--continuation-token` (resumes from token), `--limit` (client-side truncation).
- **Create responses**: Return the created object with at minimum `id`, `displayName`, `type`.
- **Delete responses**: Return `{"status": "deleted", "id": "<id>"}`.
- **Hard delete**: All 38 workspace-scoped item delete commands support `--hard-delete` flag. Appends `?hardDelete=true` to the DELETE URL. Permanently removes items (skips recycle bin). Non-item deletes (connection, deployment-pipeline, domain, gateway, managed-private-endpoint, onelake-security, profile, workspace) do NOT have this flag.
- **Update validation**: All update commands require at least one field (`--name` or `--description`). Fail with `INVALID_INPUT` if neither provided.
- **LRO standard pattern**: POST returns 202 + `Location` header. Poll every 2s, max 120s. Terminal: `status == "Succeeded"` or `"Failed"`.
- **Error enrichment**: All commands use `enrich_forbidden()` to add required role hints on 403 errors. Not-found errors include `fabio <group> list` suggestions.
- **Error `isRetriable` field**: API responses may include `error.isRetriable: bool`. When present, serialized in `FabioError` output as `"retriable": true/false`.
- **Dry-run guard**: All mutations support `--dry-run` which returns the planned request body without executing. Output: `{"status": "dry_run", "message": "Would <action>..."}`.
- **Definition operations pattern**: `POST .../getDefinition` (LRO, empty body `{}`) returns base64-encoded parts. `POST .../updateDefinition` (LRO) accepts `{"definition": {"parts": [{"path": "<file>", "payload": "<base64>", "payloadType": "InlineBase64"}]}}`.
- **Tenant-level vs workspace-scoped resources**:
  - Tenant-level (no workspace prefix): `/capacities`, `/connections`, `/deploymentPipelines`, `/admin/domains`, `/externalDataShares/invitations`
  - Workspace-scoped: All other resources at `/workspaces/{ws}/<resource>`

## Variable Library API Behaviors Discovered
- **Definition format**: Two definition files: `variables.json` (variable definitions) + `settings.json` (ordering/display).
- **variables.json schema**: `https://developer.microsoft.com/json-schemas/fabric/item/variableLibrary/definition/variables/1.0.0/schema.json`. Structure: `{"$schema":"...","variables":[]}`. Each variable has `name`, `type`, `defaultValue`, `valueSets`.
- **settings.json schema**: `https://developer.microsoft.com/json-schemas/fabric/item/variableLibrary/definition/settings/1.0.0/schema.json`. Structure: `{"$schema":"...","valueSetsOrder":[]}`.
- **updateDefinition requires valid content structure**: The API validates variable definitions. Sending a well-formed JSON with incorrect variable structure returns "Item content cannot be used". Both files may need to be included for a successful update.
- **Create is LRO**: Returns 202, requires polling.
- **getDefinition is LRO**: Returns 202, requires polling. Returns `variables.json` + `settings.json` + `.platform`.
- **409 Conflict on duplicate name**: Same pattern as all other items.
- **Endpoint pattern**: `/workspaces/{ws}/variableLibraries/{id}`.

## Event Schema Set API Behaviors Discovered
- **Definition file**: `EventSchemaSetDefinition.json` (NOT `definition.json`).
- **Definition structure**: `{"eventTypes":[],"schemas":[]}`. No `$schema` URL included (unlike most other items).
- **updateDefinition validates content**: Sending invalid event types returns "An error occurred while processing the operation". The `eventTypes` and `schemas` arrays have specific schema requirements.
- **Create is LRO**: Returns 202, requires polling.
- **getDefinition is LRO**: Returns `EventSchemaSetDefinition.json` + `.platform`.
- **Endpoint pattern**: `/workspaces/{ws}/eventSchemaSets/{id}`.

## User Data Function API Behaviors Discovered
- **Definition file**: `definition.json` (standard path).
- **Definition schema**: `https://developer.microsoft.com/json-schemas/fabric/item/userDataFunction/definition/1.1.0/schema.json` (version 1.1.0).
- **Definition structure**: `{"$schema":"...","runtime":"PYTHON","connectedDataSources":[],"functions":[],"libraries":{"public":[],"private":[]}}`.
- **Runtime values**: `"PYTHON"` (likely also supports other runtimes in future).
- **Functions array**: Defines the function code and metadata for the user data function.
- **Libraries**: Supports public (PyPI packages) and private (uploaded wheels/archives) libraries.
- **Create is LRO**: Returns 202, requires polling.
- **getDefinition is LRO**: Returns `definition.json` + `.platform`.
- **Endpoint pattern**: `/workspaces/{ws}/userDataFunctions/{id}`.

## Operations Agent API Behaviors Discovered
- **Definition file**: `Configurations.json` (same name as anomaly-detector, NOT `definition.json`).
- **Definition format**: `OperationsAgentV1` (reported in getDefinition response).
- **Definition schema**: `https://developer.microsoft.com/json-schemas/fabric/item/operationsAgents/definition/1.0.0/schema.json`.
- **Definition structure**: `{"$schema":"...","configuration":{"goals":"","instructions":"","dataSources":{},"actions":{}},"shouldRun":false}`.
- **Configuration fields**: `goals` (natural language objective), `instructions` (natural language instructions), `dataSources` (object mapping data source names to configs), `actions` (object mapping action names to configs).
- **`shouldRun` controls activation**: Boolean that determines if the agent is actively running.
- **updateDefinition works with single part**: Unlike variable-library, operations-agent successfully updates with just the `Configurations.json` part.
- **Create is LRO**: Returns 202, requires polling.
- **getDefinition is LRO**: Returns `Configurations.json` + `.platform`.
- **Endpoint pattern**: `/workspaces/{ws}/operationsAgents/{id}`.

## Digital Twin Builder API Behaviors Discovered
- **Definition file**: `definition.json` (standard path).
- **Definition structure**: `{"LakehouseId":"<uuid>"}`. Links the DTB to a lakehouse for data storage.
- **Naming constraint**: Item name must start with a letter, be less than 90 characters, and contain only letters, numbers, and underscores. Hyphens are NOT allowed (unlike most other item types).
- **Create is LRO**: Returns 202, requires polling.
- **getDefinition is LRO**: Returns `definition.json` + `.platform`.
- **Endpoint pattern**: `/workspaces/{ws}/digitalTwinBuilders/{id}`.

## Digital Twin Builder Flow API Behaviors Discovered
- **Create requires parent DTB**: The create API requires a `creationPayload` referencing the parent Digital Twin Builder artifact ID. Without it, returns "Parent artifact is inaccessible or required fields are missing from request".
- **creationPayload format**: `{"digitalTwinBuilderItemReference": {"referenceType": "ById", "itemId": "<dtb-id>", "workspaceId": "<ws-id>"}}`. The `referenceType` must be `"ById"`.
- **Definition file**: `definition.json` containing `{"DigitalTwinBuilderId": "<parent-dtb-id>", "OperationIds": [], "IsOnDemand": false}`.
- **show returns properties**: `GET /digitalTwinBuilderFlows/{id}` includes `properties.digitalTwinBuilderItemReference` with the parent DTB reference.
- **Naming constraint**: Same as DTB — letters, numbers, underscores only, no hyphens. Must start with a letter, max 90 characters.
- **Endpoint pattern**: `/workspaces/{ws}/digitalTwinBuilderFlows/{id}`.
- **Create is LRO**: Returns 202, requires polling (when payload is correct).
- **getDefinition is LRO**: Returns `definition.json` + `.platform`.

## Mounted Data Factory API Behaviors Discovered
- **Create requires ADF resource ID in definition**: Creation uses a `definition` body (NOT `creationPayload`) with a single part `mountedDataFactory-content.json` containing `{"dataFactoryResourceId": "<ARM-resource-id>"}`. The ARM ID format: `/subscriptions/<sub>/resourceGroups/<rg>/providers/Microsoft.DataFactory/factories/<name>`.
- **Do NOT include `format` field**: Including `"format": "MountedDataFactoryV1"` in the definition causes "Requested item definition format is invalid". Send definition without format field.
- **Definition file**: `mountedDataFactory-content.json` (NOT `definition.json`).
- **Create is LRO**: Returns 202, requires polling.
- **getDefinition is LRO**: Returns `mountedDataFactory-content.json` + `.platform`.
- **Endpoint pattern**: `/workspaces/{ws}/mountedDataFactories/{id}`.
- **Response includes `attributes: []`**: Same as other newer item types.

## Cosmos DB Database API Behaviors Discovered
- **Creates without external connection**: Unlike Snowflake Database, Cosmos DB Database items can be created as empty shells (no Azure Cosmos DB account required upfront).
- **Definition file**: `definition.json` (standard path).
- **Definition schema**: `https://developer.microsoft.com/json-schemas/fabric/item/CosmosDB/definition/CosmosDB/2.0.0/schema.json` (note: schema path uses `CosmosDB/CosmosDB`).
- **Definition structure**: `{"$schema":"...","containers":[]}`. The `containers` array defines mirrored Cosmos DB containers.
- **Create is LRO**: Returns 202, requires polling.
- **getDefinition is LRO**: Returns `definition.json` + `.platform`.
- **Endpoint pattern**: `/workspaces/{ws}/cosmosDbDatabases/{id}`.
- **Response includes `attributes` field**: Item responses include `"attributes": []`.

## Snowflake Database API Behaviors Discovered
- **Create requires connection payload**: Unlike Cosmos DB, creating a Snowflake Database with just `displayName` returns "Invalid payload." A connection reference (Snowflake account credentials/connection ID) is required in the creation request.
- **Endpoint pattern**: `/workspaces/{ws}/snowflakeDatabases/{id}`.
- **Create is LRO**: Returns 202, requires polling (when payload is valid).
- **getDefinition is LRO**: Returns definition + `.platform`.

## SQL Endpoint API Behaviors Discovered
- **Read-only companion item**: SQL Endpoints are auto-created as companion items alongside Lakehouses (one per lakehouse). They cannot be created or deleted independently.
- **No getDefinition/updateDefinition**: SQL Endpoints do not support definition operations.
- **Available commands**: list, show, connection-string, refresh-metadata, get-audit-settings, update-audit-settings, set-audit-actions.
- **Connection string format**: Returns the DW-style endpoint hostname (e.g., `*.datawarehouse.fabric.microsoft.com`).
- **refresh-metadata returns table sync status**: Each table shows `status` (`NotRun`, `Succeeded`, `Failed`), `startDateTime`, `endDateTime`, `lastSuccessfulSyncDateTime`.
- **Audit settings structure**: `{"state":"Disabled|Enabled","retentionDays":N,"auditActionsAndGroups":["GROUP1","GROUP2",...]}`.
- **Default audit groups**: `SUCCESSFUL_DATABASE_AUTHENTICATION_GROUP`, `FAILED_DATABASE_AUTHENTICATION_GROUP`, `BATCH_COMPLETED_GROUP`.
- **Endpoint pattern**: `/workspaces/{ws}/sqlEndpoints/{id}`.

## Apache Airflow Job API Behaviors Discovered
- **Definition format**: Main definition file is `apacheairflowjob-content.json` with a companion `dags/requirements.txt`.
- **Definition structure**: `{"properties":{"type":"Airflow","typeProperties":{"airflowProperties":{...},"computeProperties":{...}}}}`. Airflow properties include `airflowVersion`, `pythonVersion`, `enableAADIntegration`, `enableTriggerers`, `airflowConfigurationOverrides`, `airflowEnvironmentVariables`, `airflowRequirements`. Compute properties include `computePool`, `computeSize`, `enableAutoscale`, `enableAvailabilityZones`, `extraNodes`, `poolId`, `poolName`.
- **Environment lifecycle**: `start-environment` and `stop-environment` control the Airflow runtime. Environment has states: `Initial`, `Starting`, `Started`, `Stopping`, `Stopped`. Can only start from `Initial`/`Stopped` states.
- **File operations use `?beta=true`**: All file CRUD endpoints (`list-files`, `get-file`, `upload-file`, `delete-file`) require `?beta=true` query parameter.
- **File upload requires `text/plain` content type**: `PUT /workspaces/{ws}/apacheAirflowJobs/{id}/files/{path}?beta=true` with `Content-Type: text/plain` body. JSON content-type is rejected with "Please set the 'Content-Type' header to either 'text/plain' or 'application/octet-stream'".
- **File download returns raw text (not JSON)**: `GET /files/{path}?beta=true` returns the raw file content as text/plain. Must use `get_text()` instead of `get()` (which expects JSON parsing).
- **deploy-requirements requires `text/plain` content type**: `POST .../environment/deployRequirements?beta=true` with raw requirements text body (not JSON). Same content-type requirement as file upload.
- **deploy-requirements requires running environment**: Returns error if environment is in `Stopping`/`Stopped` state.
- **list-files returns directory structure**: `{"value":[{"filePath":"dags/","sizeInBytes":null},{"filePath":"plugins/","sizeInBytes":null}]}`. Directories have null `sizeInBytes`.
- **get-compute returns pool template details**: Includes `poolTemplateId`, `poolTemplateName`, `nodeSize`, `computeScalability.minNodeCount/maxNodeCount`, `apacheAirflowJobVersion`, `apacheAirflowJobVersionDetails.apacheAirflowVersion/pythonVersion`, `availabilityZones`, `shutdownPolicy`.
- **Pool templates available**: `StarterPool` (ID: `00000000-...-000000000000`, Auto Pausing) and `Starter Pool (Always On)` (ID: `...000000000001`). Both are Small size, 5 nodes, Airflow 2.10.5, Python 3.12.
- **get-workspace-settings**: Returns `{"defaultPoolTemplateId":"00000000-..."}`.
- **Shutdown policies**: `OneHourInactivity` (auto pausing) and `AlwaysOn`.
- **Availability zones**: `"Enabled"` or `"Disabled"` string values.
- **get-settings returns generic error**: `"An error occured"` (API-side bug/limitation, spelling is theirs).
- **get-environment response**: `{"status":"Started|Stopped|Starting|Stopping","airflowWebUrl":null}`. The `airflowWebUrl` may only populate once environment is fully started.
- **Create is LRO**: Returns 202, requires polling.
- **getDefinition is LRO**: Returns 202, requires polling.
- **Response includes `attributes: []`**: Item responses include empty attributes array.
- **Endpoint pattern**: `/workspaces/{ws}/apacheAirflowJobs/{id}`.

## App Backend API Behaviors Discovered
- **Preview item type**: App Backend is available as a dedicated workspace-scoped item type via `/appBackends` endpoints.
- **Available commands**: list, show, create, update, delete.
- **Create is LRO**: `POST /workspaces/{ws}/appBackends` returns asynchronous operation semantics and is polled by the CLI.
- **Update input guard**: Update requires at least one of `--name` or `--description`; otherwise returns `INVALID_INPUT` with a corrective hint.
- **Hard delete support**: Delete supports `--hard-delete`, which appends `?hardDelete=true` and bypasses recycle bin behavior.
- **Agent-context coverage**: `fabio agent-context` now includes a full `app-backend` schema (mutability, async create, and `--hard-delete` bool flag metadata).
- **Endpoint patterns**: `/workspaces/{ws}/appBackends` and `/workspaces/{ws}/appBackends/{id}`.

## Gateway API Behaviors Discovered
- **Tenant-level scope**: `GET /gateways` (no workspace prefix). Individual: `GET /gateways/{id}`.
- **Create requires VNet infrastructure**: `POST /gateways` needs capacity ID, VNet subscription/resource group/name/subnet. Subnet must be delegated to `Microsoft.PowerPlatform/vnetaccesslinks`. The `Microsoft.PowerPlatform` resource provider must be registered on the Azure subscription.
- **Gateway type**: Only `VirtualNetwork` type supported via REST API. On-premises gateways are managed by the gateway application installer.
- **`virtualNetworkAzureResource` uses component fields**: The API expects separate `subscriptionId`, `resourceGroupName`, `virtualNetworkName`, `subnetName` fields — NOT a full ARM resource ID.
- **`inactivityMinutesBeforeSleep` is required**: Must be one of: 30, 60, 90, 120, 150, 240, 360, 480, 720, 1440. Default in CLI: 120.
- **`numberOfMemberGateways` is required**: Must be between 1 and 9. Default in CLI: 1.
- **Creation is slow**: Gateway creation takes 60-90 seconds to return. No LRO pattern (returns 201 directly, but response is delayed).
- **Update requires `type` field**: `PATCH /gateways/{id}` body MUST include `"type": "VirtualNetwork"` (or `"OnPremises"` for on-prem). Without it, returns "The request has an invalid input". The CLI auto-fetches the current type via GET before PATCH.
- **VNet gateways have no "members" endpoint**: `GET /gateways/{id}/members` returns NOT_FOUND for VNet gateways. Members are an on-premises gateway concept.
- **Role assignment uses nested principal object**: `POST /gateways/{id}/roleAssignments` body format: `{"principal": {"id": "<uuid>", "type": "User|Group|ServicePrincipal"}, "role": "Admin|ConnectionCreator|ConnectionCreatorWithResharing"}`. Flat `principalId`/`principalType` format is rejected.
- **Cannot demote last Admin**: Attempting to update the sole Admin's role to a lower level returns `DMTS_CannotDeleteLastGatewayPrincipalError`.
- **Duplicate role assignment returns CONFLICT**: Adding a role for a principal that already has one returns 409 with "Gateway role assignemnt already exists" (note: API has typo "assignemnt").
- **Non-existent principal returns 500**: Adding a role for a UUID that doesn't resolve to a real Entra ID principal returns "An unexpected error occurred" (internal server error, not a clean validation error).
- **Delete is immediate**: `DELETE /gateways/{id}` returns immediately. However, the Azure VNet's `serviceAssociationLinks/PowerPlatformSAL` persists for several minutes after deletion, blocking VNet/subnet removal until Power Platform cleans up.
- **Available commands**: list, show, create, update, delete, list-members, update-member, delete-member, list-role-assignments, add-role-assignment, show-role-assignment, update-role-assignment, delete-role-assignment.
- **Roles enum**: `Admin`, `ConnectionCreator`, `ConnectionCreatorWithResharing` (hierarchical, Admin is highest).
- **Load balancing settings**: `Failover` (default), `DistributeEvenly`. Only applicable to on-premises gateways with multiple members.

## Mirrored Catalog API Behaviors Discovered
- **Requires tenant-level feature flag (NOT capacity SKU)**: Creating mirrored catalogs returns `FeatureNotAvailable` (HTTP 403) even on F64 capacity. The error `"The feature is not available"` is controlled by a tenant admin setting (likely "Mirrored Catalog" or "Unity Catalog mirroring"), not capacity size. Both the type-specific endpoint (`POST /mirroredCatalogs`) and generic items endpoint (`POST /items` with `type: MirroredCatalog`) fail identically. The `?beta=true` query param does not help.
- **List works without the feature flag**: `GET /workspaces/{ws}/mirroredCatalogs` and `GET /workspaces/{ws}/items?type=MirroredCatalog` both return empty results successfully (HTTP 200). Only mutations (create) are blocked.
- **Definition file**: `mirroring.json` (same as Mirrored Database).
- **Endpoint pattern**: `/workspaces/{ws}/mirroredCatalogs/{id}`.
- **Additional endpoints (untestable)**: `refreshCatalogMetadata?beta=true` (POST, LRO), `mirroringStatus?beta=true` (GET), `tablesMirroringStatus?beta=true` (GET). Workspace-level: `catalogmirroring/scopes?beta=true` (GET), `catalogmirroring/tables?beta=true` (GET).
- **Cannot test without admin enabling feature**: All mutation commands (create/update/delete/update-definition) and item-specific read commands (show/get-definition/mirroring-status) require an existing item, which cannot be created without the tenant setting.
- **Distinct from MirroredAzureDatabricksCatalog**: `MirroredCatalog` is a separate, newer item type. `MirroredAzureDatabricksCatalog` creates successfully on F2 capacity without any Databricks account. `MirroredCatalog` (and `MirroredWarehouse`) are blocked by the same tenant feature flag — these are likely for generic/Snowflake catalog mirroring.
- **MirroredWarehouse has same blocker**: `POST /workspaces/{ws}/items` with `type: MirroredWarehouse` also returns `FeatureNotAvailable` (403). Same tenant setting controls both.

## Mirrored Databricks Catalog API Behaviors Discovered
- **Creates without external connection**: Unlike Snowflake Database, MirroredAzureDatabricksCatalog items can be created as empty shells (no Databricks account/workspace required upfront). The item is created successfully but cannot perform mirroring operations without a configured Databricks connection.
- **Naming constraint**: Item names cannot contain hyphens. Names like `test-mdc-e2e` return "Invalid Display Name ... contains invalid characters". Must use alphanumeric characters and underscores only (similar to Digital Twin Builder).
- **Create is LRO**: Returns 202, requires polling.
- **Definition file**: `mirroring.json`.
- **get-definition returns empty definition**: Newly created items have no meaningful content in `mirroring.json`.
- **discover-catalogs requires connection**: Returns "The request has an invalid input" without a configured Databricks connection.
- **refresh-metadata requires catalog configuration**: Returns "Catalog configuration for Artifact with ID ... not found" on items without a configured Databricks source.
- **Response includes `attributes: []`**: Same as other newer item types.
- **Endpoint pattern**: `/workspaces/{ws}/mirroredAzureDatabricksCatalogs/{id}`.

## Graph Model API Behaviors Discovered (Additional)
- **execute-query uses `--query` flag** (not `--kql`): Command syntax is `fabio graph-model execute-query --workspace <WS> --id <ID> --query "<KQL>"`.
- **Graph must be loaded before queries**: `execute-query` on an unloaded graph returns `GraphIsNotLoaded` error.
- **get-queryable-graph-type**: Returns `null` when graph has no queryable type (not yet loaded). Requires `?preview=true`.
- **refresh-graph returns immediately**: `{"id":"...","status":"refresh_triggered"}`. The actual refresh runs asynchronously.
- **Refresh requires portal initialization**: As documented previously, REST-only graph models fail refresh with `VersionConfig does not exist`.

## Graph Query Set API Behaviors Discovered (Additional)
- **Definition is read-only**: `exportedDefinition.json` content (`ArtifactContents`, `dependencies`, `ConfigurationCategories`) is always empty arrays when retrieved via API. Query content is portal-managed only.

## Warehouse Snapshot API Behaviors Discovered
- **Create requires `creationPayload` with warehouse ID**: Simple `displayName`-only creation returns "Invalid payload used for operation." Must include `{"creationPayload":{"warehouseId":"<warehouse-id>"}}`.
- **Requires existing warehouse**: Cannot test without a warehouse item in the workspace.
- **Endpoint pattern**: `/workspaces/{ws}/warehouseSnapshots/{id}`.
- **Available commands**: list, show, create (with --warehouse-id), update, delete.

## Dashboard/Datamart/Paginated Report API Behaviors Discovered
- **Read-only list items**: Dashboard has only `list` command. Datamart has only `list`. Paginated Report has `list` and `update`.
- **No creation via REST API**: These item types are created through the portal or Power BI Desktop. The REST API provides read-only access.
- **Endpoint patterns**: `/workspaces/{ws}/dashboards`, `/workspaces/{ws}/datamarts`, `/workspaces/{ws}/paginatedReports/{id}`.

## Catalog API Behaviors Discovered
- **Single command**: `search` is the only subcommand.
- **Requires `--content` with JSON body**: `fabio catalog search --content '{"searchString":"...","top":N}'`. Returns items matching the search string across workspaces.
- **Endpoint**: `POST /catalog/search` (tenant-level, no workspace prefix).

## Operation API Behaviors Discovered
- **Uses `--operation-id`** (not `--id`): Unique among all command groups. Matches the operation ID returned in LRO `Location` headers.
- **get-state**: Returns the current state of a long-running operation.
- **get-result**: Returns the final result after operation completes.
- **404 for nonexistent IDs**: Standard error handling for invalid operation IDs.
- **Endpoint pattern**: `/operations/{operationId}` (tenant-level).

## Admin API Behaviors Discovered
- **Requires Fabric admin role**: All admin endpoints require elevated tenant-level permissions. Standard workspace Member/Admin roles are insufficient.
- **Scope error message**: "The caller does not have sufficient scopes to perform this operation".
- **50 subcommands**: Covers tenant settings, workspace management, items, users, labels, tags, external data shares, domains — all at admin scope.
- **Required delegated scope**: `Tenant.Read.All` or `Tenant.ReadWrite.All` for most read endpoints. `Tenant.ReadWrite.All` for mutations.
- **Non-standard response array keys**: Unlike most Fabric APIs that use `"value"` as the array key, admin endpoints use varied keys:
  - `/admin/workspaces` → `"workspaces"` (NOT `"value"`)
  - `/admin/items` → `"itemEntities"` (NOT `"value"`)
  - `/admin/workspaces/{id}/users` → `"accessDetails"` (NOT `"value"`)
  - `/admin/workspaces/{ws}/items/{id}/users` → `"accessDetails"` (NOT `"value"`)
  - `/admin/users/{id}/access` → `"accessEntities"` (NOT `"value"`)
  - `/admin/domains` → `"domains"` (NOT `"value"`)
  - `/admin/tenantsettings` → `"tenantSettings"` (NOT `"value"`)
  - `/admin/tags` → `"value"` (standard)
  - `/admin/workloads` → `"value"` (standard)
  - `/admin/workloads/assignments` → `"value"` (standard)
  - `/admin/workspaces/discoverGitConnections` → `"value"` (standard)
  - `/admin/workspaces/networking/communicationpolicies` → `"value"` (standard)
- **Workspace response uses `name` not `displayName`**: The admin workspace endpoints return `name` field (not `displayName`). Fields: `id`, `name`, `state`, `type`, `capacityId`, `tags`.
- **Item response uses `name` not `displayName`**: The admin items endpoint returns `name` field. Fields: `id`, `type`, `name`, `state`, `lastUpdatedDate`, `creatorPrincipal`, `workspaceId`, `capacityId`.
- **Tag creation body format**: `POST /admin/tags/bulkCreateTags` requires `{"createTagsRequest": [{"displayName": "..."}]}`. Optional `"scope"` field: `{"type": "Tenant"}` or `{"type": "Domain", "domainId": "<uuid>"}`. Response: `{"tags": [{"id": "...", "displayName": "...", "scope": {...}}]}`.
- **Tag update uses PATCH**: `PATCH /admin/tags/{tagId}` with `{"displayName": "...", "description": "..."}`.
- **Tag delete uses DELETE**: `DELETE /admin/tags/{tagId}` returns 200 on success.
- **External data shares requires tenant setting**: `GET /admin/items/externalDataShares` returns FORBIDDEN with message "The operation is not allowed since tenant setting 'External data sharing' is disabled" if the tenant setting is off.
- **Grant admin access may fail with NOT_FOUND**: `POST /admin/workspaces/{id}/grantAdminTemporaryAccess` returns `RequestFailed` (mapped to NOT_FOUND) for some workspaces despite the workspace being visible in the admin listing. Root cause unclear — may require specific tenant configuration.
- **Pagination uses `continuationToken` and `continuationUri`**: Admin endpoints that support pagination return these fields in the response alongside the array data.
- **Rate limits**: Tag operations limited to 25 requests/minute. User/item access details limited to 200 requests/hour.
- **Bulk assign/unassign domain roles**: `POST /admin/domains/{id}/roleAssignments/bulkAssign` and `/bulkUnassign` with body `{"type": "Contributors", "principals": [{"id": "<uuid>", "type": "User"}]}`. Type values: `"Contributors"` or `"Admins"`. Returns 200 with empty body (null) on success. Pass-through via `--content`.
- **Sync roles to subdomains**: `POST /admin/domains/{id}/roleAssignments/syncToSubdomains` with body `{"role": "Contributor"}`. Required field `role` (values: `"Contributor"`, `"Admin"`). Note: "Syncing admins to subdomains is not supported" — only Contributors can be synced.
- **Capacity tenant setting overrides**: Only settings with `"delegateToCapacity": true` in their tenant settings response can have capacity-level overrides. Attempting to override a non-delegatable setting returns "The request could not be processed due to missing or invalid information". Example delegatable setting: `PlatformMonitoringTenantSetting`.
- **Override update body**: `{"enabled": true/false, "delegateToWorkspace"?: bool, "enabledSecurityGroups"?: [...], "excludedSecurityGroups"?: [...]}`. Minimum required field: `enabled`.
- **Override update response**: Returns `{"overrides": [<CapacityTenantSetting>]}` with full setting details including `delegatedFrom`, `settingName`, `title`, `enabled`, `canSpecifySecurityGroups`, `tenantSettingGroup`.
- **Domain-level overrides**: Only settings with `"delegateToDomain": true` can have domain-level overrides. Same pattern as capacity overrides.
- **`update-tenant-setting` response**: Returns `{"tenantSettings": [...]}` — all settings in the SAME group (not just the updated one). Endpoint: `POST /admin/tenantsettings/{settingName}/update`. Body minimum: `{"enabled": true/false}`.
- **`grant-admin-access` / `remove-admin-access`**: Returns NOT_FOUND (404) when the caller already has permanent Admin access to the workspace. These endpoints manage TEMPORARY admin access only — they create/remove time-limited admin records for workspaces the caller doesn't own.
- **`show-item` response includes `defaultIdentity`**: Admin item detail returns extra fields not in standard item responses: `defaultIdentity`, `creatorPrincipal`, `workspaceId`, `capacityId`, `state`, `lastUpdatedDate`.
- **`list-external-data-shares` requires tenant setting**: Returns FORBIDDEN with message "The operation is not allowed since tenant setting 'External data sharing' is disabled" when the tenant setting is off.
- **50 E2E tests**: All passing — covers read-only listing, tag lifecycle (create→list→update→delete), domain lifecycle, workspace assignment, bulk role assign/unassign, sync roles, capacity override roundtrip, tenant setting update roundtrip, dry-run validations for all destructive commands.
- `tests/e2e_admin.rs`: 63 tests (50 original + 3 Phase B + 4 Phase C + 6 Phase D live tests)
- **`assign-domain-workspaces-by-capacities`**: `POST /admin/domains/{id}/assignWorkspacesByCapacities` with `{"capacitiesIds": ["<uuid>"]}`. Assigns ALL workspaces on that capacity to the domain. Returns 200 with empty body.
- **`assign-domain-workspaces-by-principals`**: `POST /admin/domains/{id}/assignWorkspacesByPrincipals` with `{"principals": [{"id": "<uuid>", "type": "User"}]}`. Requires `--principal-type` flag. Assigns all workspaces owned/administered by those principals.
- **`unassign-all-domain-workspaces`**: `POST /admin/domains/{id}/unassignAllWorkspaces` with empty body `{}`. Removes all workspace-domain associations atomically.
- **Workspace restore**: `POST /admin/workspaces/{id}/restore` with `{"restoredWorkspaceName": "<name>", "capacityId": "<uuid>"}`. Returns 200 with null body. The `restoredWorkspaceName` parameter appears to be IGNORED — workspace keeps its original name. The `capacityId` may also be overridden server-side.
- **Workload assignment body format**: Requires discriminated union with `type` field. Three shapes:
  - Tenant: `{"type": "Tenant", "workloadId": "<id>"}`
  - Capacity: `{"type": "Capacity", "workloadId": "<id>", "capacityId": "<uuid>"}`
  - Workspace: `{"type": "Workspace", "workloadId": "<id>", "workspaceId": "<uuid>"}`
- **Workload assignment response**: Returns 201 Created with `{"id": "<uuid>", "type": "Tenant|Capacity|Workspace", "workloadId": "..."}`. Capacity/workspace variants also include `capacityName`/`workspaceName`.
- **`delete-workload-assignment`**: `DELETE /admin/workloads/assignments/{assignmentId}`. Returns 200 on success.
- **Domain workspace assignment is additive but capped by existing domain membership**: `assign-domain-workspaces-by-principals` only assigns workspaces NOT already assigned to another domain. If all user's workspaces are already in other domains, count=0 is returned.
- **`remove-all-sharing-links` is LRO**: `POST /admin/items/removeAllSharingLinks` with `{"sharingLinkType":"OrgLink"}`. Returns 202, polls to completion. LRO response: `{"status":"Succeeded","percentComplete":100,"error":null}`. Safe no-op when no links exist.
- **`bulk-remove-sharing-links` is LRO**: `POST /admin/items/bulkRemoveSharingLinks`. Returns 202, polls to completion. Response includes `itemRemoveSharingLinksStatus` per-item array with `status` (`NotFound` for non-existent items). Only supports Report type — other types return "not supported for the requested item type".
- **`sharingLinkType` enum values**: `OrgLink`, `GuestLink`, `AnonymousLink`, `SpecificPeopleLink`.
- **`bulk-remove-labels` returns per-item status**: Response: `{"itemsChangeLabelStatus":[{"status":"NotFound"}]}` when item has no label set. Does not require Purview labels to execute (unlike `bulk-set-labels`).
- **`bulk-set-labels` requires Microsoft Purview**: Returns "Label is not assigned to user" when Purview sensitivity labels are not configured in the tenant. Requires M365 E5 licensing + Purview label policy.
- **`revoke-external-data-share`**: Returns NOT_FOUND for non-existent share IDs. Endpoint: `POST /admin/workspaces/{ws}/items/{item}/externalDataShares/{share}/revoke`.
- **`list-external-data-shares` requires tenant setting**: Only works after enabling "External data sharing" (`AllowExternalDataSharingSwitch`) in tenant admin settings. Returns FORBIDDEN otherwise.

## Power BI REST API Integration Behaviors Discovered
- **Single token for both APIs**: The Fabric token (`https://api.fabric.microsoft.com/.default` scope) is accepted by both `api.fabric.microsoft.com` and `api.powerbi.com`. No separate Power BI scope is needed.
- **Power BI API base URL**: `https://api.powerbi.com/v1.0/myorg`. Workspaces are referenced as "groups": `/groups/{workspace-id}/datasets/{dataset-id}`.
- **`datasets` = semantic models**: The Power BI REST API uses the legacy term "datasets" for what Fabric calls "semantic models". The ID is the same UUID.
- **`--api powerbi` flag on `fabio rest call`**: Routes requests to the Power BI API instead of Fabric. Dry-run output includes `"api": "powerbi"` field.
- **Env var override**: `FABIO_POWERBI_ENDPOINT` overrides the Power BI base URL (for sovereign clouds).
- **Auth reuse**: All Power BI methods (`get_powerbi`, `post_powerbi`, `put_powerbi`, `patch_powerbi`, `delete_powerbi`) share the same `require_auth()` token cache as Fabric methods.
- **list-parameters**: `GET /groups/{ws}/datasets/{id}/parameters` → returns `{"value": [...]}` with M parameters.
- **update-parameters**: `POST /groups/{ws}/datasets/{id}/Default.UpdateParameters` with `{"updateDetails": [...]}`.
- **list-datasources**: `GET /groups/{ws}/datasets/{id}/datasources` → returns `{"value": [...]}`.
- **update-datasources**: `POST /groups/{ws}/datasets/{id}/Default.UpdateDatasources` with `{"updateDetails": [...]}`.
- **list-users**: `GET /groups/{ws}/datasets/{id}/users` → returns `{"value": [...]}` with access rights per principal.
- **add-user**: `POST /groups/{ws}/datasets/{id}/users` with `{"identifier": "...", "principalType": "...", "datasetUserAccessRight": "..."}`.
- **delete-user**: `DELETE /groups/{ws}/datasets/{id}/users/{user}` where `user` is the email or object ID.
- **refresh-status**: `GET /groups/{ws}/datasets/{id}/refreshes?$top=N` returns refresh history (status, startTime, endTime).
- **list-upstream**: `GET /groups/{ws}/datasets/{id}/upstreamDatasets` returns upstream dataset dependencies.
- **clone**: `POST /groups/{ws}/datasets/{id}/Default.Clone` with `{"name": "...", "targetWorkspaceId"?: "..."}`. Returns new dataset ID.
- **export-pbix**: `POST /groups/{ws}/datasets/{id}/Default.Export` → returns binary .pbix stream. Uses `post_powerbi_bytes()` for binary download. Reports `size_bytes` in output.
- **import-pbix**: `POST /groups/{ws}/imports?datasetDisplayName={name}&nameConflict={policy}` with `multipart/form-data` file upload. Uses `post_powerbi_multipart()`. Validates file existence client-side before upload.
- **import-pbix cannot retry on 401**: The multipart form body is consumed on first send attempt. If auth expires mid-upload, returns auth error instead of retrying.
- **nameConflict values**: `Abort` (default, fails if exists), `Overwrite`, `CreateOrOverwrite`, `GenerateUniqueName`.
- **accessRight values for add-user**: `Read`, `ReadWrite`, `ReadWriteReshare`, `ReadWriteReshareExplore`, `ReadExplore`, `ReadReshareExplore`, `ReadWriteExplore`.
- **principalType values for add-user**: `User`, `Group`, `App` (service principal).
- **`--content` flag pattern**: Phase 2 mutation commands use `--content` for inline JSON (not `--file`). Validated with `parse_json_content()` which provides error hints showing expected format.

## Deploy Command Design & Behaviors

The `fabio deploy` command group is a CI/CD deployment engine for Fabric workspaces. It provides stateless, content-hash-based convergence similar to Terraform but without a state file — always queries the live workspace for the current state.

### Architecture

```
fabio deploy export   → getDefinition per item → write .platform + parts
fabio deploy plan     → parse source + list workspace → diff → changeset
fabio deploy apply    → execute changeset (create/update/rename/delete)
fabio deploy init-params → scan/diff definitions → generate parameters.json
```

### Source Directory Format

Each item is a directory named `{DisplayName}.{ItemType}/` containing:
- `.platform` (required) — metadata JSON with `$schema` URL, `metadata` block, `config` block
- Definition part files (e.g., `notebook-content.py`, `report.json`, `model.tmdl`) — base64-encoded when sent to API
- `creationPayload.json` (optional) — merged into item creation body as `creationPayload` field

**`.platform` structure:**
```json
{
  "$schema": "https://developer.microsoft.com/json-schemas/fabric/gitIntegration/platformProperties/2.0.0/schema.json",
  "metadata": {
    "type": "Notebook",
    "displayName": "MyNotebook",
    "description": "optional"
  },
  "config": {
    "version": "2.0",
    "logicalId": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
    "definitionFormat": "ipynb"
  }
}
```

**Reserved filenames** (excluded from definition parts, not hashed):
- `.platform` — metadata, generated on export
- `creationPayload.json` — creation-time configuration

**Directory scanning behavior:**
- Non-directory entries at source root are silently skipped
- Directories without `.platform` are silently skipped
- Subdirectories within item dirs are recursively traversed for definition parts
- Backslash paths normalized to forward slashes (Windows compatibility)

### Workspace Resolution

The `--workspace` parameter accepts either a GUID or a display name:
- **GUID detection**: 36 chars, all hex digits + dashes, exactly 4 dashes → used directly
- **Name resolution**: Lists all workspaces, matches by `displayName` (case-insensitive)
- Error if name not found (with workspace list hint)

### Changeset Actions

| Action | Trigger | Execution |
|--------|---------|-----------|
| `Create` | Source item has no match in workspace (by type+name) | POST `/items` with LRO |
| `Update` | Content hash differs between source and deployed | POST `updateDefinition` with LRO |
| `Rename` | Source logicalId matches deployed item but name differs | PATCH displayName + `updateDefinition` |
| `Delete` | Deployed item has no match in source (requires `--delete-orphans`) | DELETE `/items/{id}` |
| `Skip` | Content hash matches — item is already in sync | No-op |

**Change struct fields:** `name`, `item_type`, `action`, `reason`, `logical_id?`, `deployed_id?`, `source_hash?`, `previous_name?` (optional fields omitted from JSON when None).

### Content Hash Calculation

- **Algorithm**: SHA-256 over sorted `(path, payload)` pairs with `\x00` separators between fields
- **Format**: `"sha256:<64-hex-chars>"`
- **Source hash**: Computed from base64-encoded file contents (stable across runs)
- **Deployed hash**: Computed from API response parts via `getDefinition` (same algorithm)
- **Determinism**: Parts sorted by path before hashing — filesystem read order doesn't matter
- **Empty parts**: Valid case (Lakehouse, MLModel) — produces consistent empty-input hash
- **Items without definition support**: `getDefinition` returns NOT_FOUND/not supported → treated as "always changed" (Update, never Skip)
- **Hash recomputation**: After parameter substitution, content hash is recomputed to reflect substituted values

### Rename Detection (Two-Pass Matching)

1. **First pass**: Match source items to deployed items by `(type, displayName)` — standard create/update/skip
2. **Second pass**: For unmatched source items WITH a `logicalId`:
   - Find unmatched deployed items of the same type (case-insensitive type comparison)
   - Call `getDefinition` on each candidate
   - Extract `.platform` part, base64-decode, parse JSON, read `config.logicalId`
   - If logical IDs match → `Rename` action (with `previous_name` field set)
   - Any failure in extraction (invalid base64, non-UTF-8, no `.platform` part, parse error) → gracefully returns `None`, candidate skipped

**Graceful degradation**: `fetch_deployed_logical_id` never errors — all failures return `Ok(None)`.

### Logical ID Resolution

When items reference each other by logical ID (e.g., a notebook referencing a lakehouse), the deploy engine resolves these at apply time:

1. **`build_resolution_map()`**: Maps logical IDs → deployed item GUIDs. Sources:
   - Items already in workspace (via `type_name_index` + existing items)
   - Items created earlier in the same deploy session (via `created_ids` accumulator)
   - Only items WITH a `logical_id` produce mappings
2. **`resolve_logical_ids_in_payload()`**: For each definition part:
   - Base64-decodes the payload
   - Performs `String::replace` for each logical_id→deployed_id
   - Re-encodes to base64
   - Returns original unchanged if: map is empty, invalid base64, non-UTF-8, or no matches found
3. **Dependency ordering**: Items deployed via `DEPLOY_ORDER` so referenced items exist before referencing items

**Parallel batch resolution**: Each type-batch gets a snapshot of `created_ids` at batch start. Items within the same priority batch cannot resolve each other's logical IDs (they execute concurrently).

**Substring safety**: `String::replace` is used — if a logical ID is a substring of another string in the payload, it will be replaced within that longer string. Logical IDs should be UUID-format to minimize false matches.

### Parameter Substitution

The `--parameters <file> --env <name>` flags enable environment-aware value replacement. Both flags are required together (bail if one without the other).

**Application order**: find_replace → key_value_replace → spark_pool → semantic_model_binding (later rules can override earlier results).

#### 1. `find_replace`
Simple string replacement in definition payloads AND `creationPayload`.

```json
{
  "find_replace": [
    {
      "find_value": "source-workspace-guid",
      "replace_value": {"dev": "dev-guid", "prod": "prod-guid", "_ALL_": "fallback"},
      "is_regex": false,
      "item_type": "Notebook",
      "item_name": "MyNB",
      "file_path": "notebook-content.py"
    }
  ]
}
```

- `is_regex: true`: Only capture group 1 is replaced (surrounding match text preserved)
- `item_type`, `item_name`, `file_path`: Optional scoping filters (case-insensitive, `StringOrVec` supports single value or array)
- `_ALL_` key in `replace_value`: Universal fallback if specific env not found (case-insensitive lookup)

#### 2. `key_value_replace`
JSONPath-targeted replacement in specific files. Payloads parsed as JSON.

```json
{
  "key_value_replace": [
    {
      "find_key": "$.parentEventhouseItemId",
      "replace_value": {"dev": "dev-eh-id", "prod": "prod-eh-id"},
      "item_type": "KQLDatabase",
      "item_name": null,
      "file_path": null
    }
  ]
}
```

- Uses `jsonpath_rust` crate for JSONPath evaluation
- Replacement values can be any JSON type (string, number, object, array)
- Non-JSON payloads are silently skipped (graceful degradation)
- Also applies to `creationPayload` (virtual path `"creationPayload.json"` for filter matching)

#### 3. `spark_pool`
Replaces Spark pool references in notebook/SparkJobDefinition metadata.

```json
{
  "spark_pool": [
    {
      "instance_pool_id": "current-pool-guid",
      "replace_value": {
        "dev": {"pool_type": "Workspace", "name": "dev-pool"},
        "prod": {"pool_type": "Capacity", "name": "prod-pool"}
      },
      "item_name": null
    }
  ]
}
```

- Recursive JSON tree walk finds objects with `instancePoolId` or `instance_pool_id` matching the target
- Replaces `type` and `name` fields in the pool configuration
- Leaves `instancePoolId` unchanged (identifies the pool slot, not the target)

#### 4. `semantic_model_binding`
Replaces semantic model connection IDs for cross-environment binding.

```json
{
  "semantic_model_binding": {
    "default": {
      "connection_id": {"dev": "dev-sm-guid", "prod": "prod-sm-guid"}
    },
    "models": [
      {
        "semantic_model_name": "SalesModel",
        "connection_id": {"dev": "override-guid", "prod": "override-guid"}
      }
    ]
  }
}
```

- Only processes `SemanticModel` items
- Model-specific override checked first, then falls back to `default`
- Recursive JSON walk replaces GUID-shaped strings (36-char) in `connectionId`, `connection_id`, `pbiModelDatabaseName`
- Also replaces UUID within `connectionString` containing `semanticmodelid=`

#### Dynamic Variables in Replacement Values

String replacement values support dynamic variable expansion:
- `$workspace.id` → deployed workspace GUID
- `$workspace.name` → workspace display name (only available if resolved by name)
- `$ENV:VAR_NAME` → environment variable value (errors if not set)
- `$items.Type.Name.id` → deployed GUID of another item in the workspace
- Non-`$` strings pass through unchanged

### Init-Params (Scaffold Generation)

`fabio deploy init-params` helps bootstrap `parameters.json`:

**Scan mode** (`--source` only):
- Finds all GUIDs matching `[0-9a-fA-F]{8}-...-[0-9a-fA-F]{12}` in definition payloads
- Filters out well-known GUIDs: all-zeros, all-`f`s, near-zero (`00000000-0000-0000-0000-00000000000X`)
- Generates `find_replace` rules with `"_ALL_": "TODO_REPLACE_<first8chars>"`
- Scopes rules to `item_type`/`item_name` if all occurrences are in a single item
- Output: `{"status":"generated","mode":"scan","source_items":N,"rules_generated":N,"guids_found":N}`

**Diff mode** (`--source` + `--compare` + `--source-env` + `--compare-env`):
- Parses both directories, matches items by `(type, name)`
- Items only in one side are skipped (no diff possible)
- For matching items: compares each definition part's base64-decoded content
- Finds GUIDs unique to each side; positional pairing when counts are equal
- Also discovers non-GUID string differences via recursive JSON comparison (5-500 char filter)
- Generates rules with both environment values pre-filled
- Uses `BTreeMap`/`BTreeSet` for deterministic output ordering
- Deduplicates via `seen_pairs` (same diff won't produce multiple rules)

### Post-Deploy Hooks

After successful deployment, hooks fire automatically (opt-out via `--no-post-hooks`):
- **SemanticModel**: `POST /workspaces/{ws}/semanticModels/{id}/refreshes` with `{"type":"Full"}` — triggers Direct Lake framing
- **Environment**: `POST /workspaces/{ws}/environments/{id}/staging/publish` with `{}` — publishes staged changes

**Hook rules:**
- Only fire for Create/Update/Rename actions (not Skip/Delete)
- Only fire for changes with a `deployed_id` (must have succeeded)
- Never fire during `--dry-run`
- Failures are non-fatal: reported in `post_hooks` output array but don't fail the deploy
- Progress messages emitted to stderr: `[deploy] post-hook: refreshing semantic model "..."`

### Plan Staleness Detection

When using `--out` to save a plan file and later `--plan` to apply it:
1. At plan time: compute workspace fingerprint (SHA256 of sorted `(id, type, name)` tuples with `\x00` separators)
2. Plan file saved with: `version: 1`, `workspace_id`, `workspace_fingerprint`, `changeset`, `source_path`, `source_git`
3. At apply time: re-compute fingerprint from live workspace and compare to saved value
4. If mismatch → error with "workspace has changed since plan was created" (override with `--force`)

**Fingerprint scope**: Only considers `(id, type, name)` — definition content changes don't affect fingerprint. Adding/removing items DOES change it.

### Reference Validation

At plan time, `validate_references()` cross-checks logical ID references:
- Builds set of "resolvable" logical IDs from changeset (Create/Update/Skip actions all contribute)
- Delete actions do NOT contribute (those items will be gone)
- For each source item WITH a logical_id: base64-decodes each part's payload
- If payload contains another item's logical ID that is NOT in the resolvable set → warning added to `changeset.warnings`
- Skips self-references (uses `std::ptr::eq` pointer equality)
- Items without any `logical_id` are not scanned (no false positives)

### Export Behaviors

`fabio deploy export` fetches all item definitions from a workspace and writes them to disk:
- Uses generic items endpoint (`GET /workspaces/{ws}/items`) with full pagination
- For each item: calls `getDefinition` (LRO POST with empty body `{}`)
- **Items that fail `getDefinition`**: Added to `skipped` list with reason (not fatal)
- **Items without definition parts**: Skipped with reason "no definition parts"
- **`.platform` part from API is discarded**: Export generates its own `.platform` from item metadata
- **Logical ID extracted from API's `.platform`** BEFORE filtering (read then discard)
- **`definition_format`**: Captured from `data.definition.format` if present in API response
- **`--overwrite`**: Required if output directory is non-empty (checked via iterator peek)
- **`--dry-run`**: Counts items without writing to disk
- **`--item-types`**: Case-insensitive filter on item types
- Items with empty `id`, `type`, or `displayName` are silently skipped

### Deploy Order (42 Types)

Items are deployed in dependency order to satisfy references:
```
VariableLibrary → Warehouse → WarehouseSnapshot → MirroredDatabase →
MirroredAzureDatabricksCatalog → Lakehouse → SQLDatabase → CosmosDbDatabase →
SnowflakeDatabase → Environment → UserDataFunction → Eventhouse → KQLDatabase →
SparkJobDefinition → Notebook → SemanticModel → Report → PaginatedReport →
Dashboard → CopyJob → KQLQueryset → KQLDashboard → Reflex → Eventstream →
EventSchemaSet → Dataflow → DataPipeline → GraphQLApi → ApacheAirflowJob →
MountedDataFactory → DataAgent → OperationsAgent → AnomalyDetector →
MLExperiment → MLModel → Ontology → GraphModel → GraphQuerySet →
DigitalTwinBuilder → DigitalTwinBuilderFlow → Map → Connection
```

**Priority rules:**
- Unknown item types get `DEPLOY_ORDER.len()` priority (deployed last, not an error)
- Case-insensitive matching via `eq_ignore_ascii_case`
- Delete priority is reversed: `DEPLOY_ORDER.len() - deploy_priority` (dependents deleted first)
- `topological_sort` (Kahn's algorithm) used within DataPipeline batch for `ExecutePipeline` references

### Empty Definition Handling

Some item types (Lakehouse, MLModel, MLExperiment) have no definition parts:
- On **Create**: Omit `definition` field entirely from request body (only send `displayName` + optional `creationPayload`)
- On **Update**: Skip `updateDefinition` call (nothing to update)
- Content hash is still computed (empty hash) for idempotency detection

### Concurrency & Rate Limiting

- **Default concurrency**: 8 parallel operations per type batch (`--concurrency N`)
- **Parallel execution**: Uses `tokio::spawn` + `tokio::sync::Semaphore` for bounded parallelism
- **Sequential fallback**: Used when `concurrency == 1` or batch has single item
- **DataPipeline special case**: Always deployed sequentially with topological sort by `ExecutePipeline` activity references
- **Delete operations**: Always execute sequentially in reverse dependency order
- **`fail_fast`**: In parallel mode, stops processing on first failure (in-flight tasks still complete)
- **Rate limit retry**: Inherited from `FabricClient` HTTP layer (exponential backoff on 429)
- **Progress messages**: `[deploy] <message>` emitted to stderr (respects `--quiet`)
- **Duration tracking**: Uses `u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)`

### DataPipeline Topological Sorting

Within the DataPipeline type batch, items are sorted by their `ExecutePipeline` activity references:
- `extract_pipeline_references()` scans base64-decoded definition parts for `ExecutePipeline` activities
- Only checks parts whose path contains "pipeline" or has `.json` extension
- Extracts `typeProperties.pipeline.referenceName` from each activity
- `order_pipelines()` builds dependency graph and runs Kahn's algorithm
- External references (pipelines not in the batch) are silently ignored
- Circular dependencies produce an error listing involved items
- Short-circuits if ≤1 pipeline in batch

### Create Item Details

When creating an item, the POST body is constructed as:
```json
{
  "displayName": "<name>",
  "type": "<ItemType>",
  "definition": {                          // OMITTED if no parts
    "format": "<definitionFormat>",        // OMITTED if not specified
    "parts": [{"path":"...","payload":"...","payloadType":"InlineBase64"}]
  },
  "creationPayload": {...},                // OMITTED if no creationPayload.json
  "description": "..."                     // OMITTED if not in .platform
}
```
- POST to `/workspaces/{ws}/items` with LRO (`poll: true`)
- Returns new item's `id` from response

### Rename Item Details

Rename is a two-step operation:
1. **PATCH displayName**: `PATCH /workspaces/{ws}/items/{id}` with `{"displayName":"<new>"}` (+ optional `description`)
2. **updateDefinition**: If parts exist, POST `updateDefinition` with LRO (same as Update)

### Plan File Format

Saved via `--out`:
```json
{
  "version": 1,
  "workspace_id": "<guid>",
  "workspace_fingerprint": "sha256:<64-hex>",
  "changeset": {"changes": [...], "warnings": [...], "errors": [...]},
  "source_path": "/absolute/path/to/source",
  "source_git": {"commit": "<sha>", "branch": "<name>", "dirty": false}
}
```

When applying from plan file:
- Source is re-parsed from `source_path` (must still exist on disk)
- Parameters are re-applied to the re-parsed source
- `--plan` is mutually exclusive with `--source`/`--workspace` (clap `conflicts_with_all`)

### CLI Flags Reference

```
fabio deploy plan --source <DIR> --workspace <ID|NAME>
  [--item-types <T1,T2>] [--delete-orphans] [--allow-unresolved]
  [--force-all] [--out <FILE>] [--parameters <FILE> --env <NAME>]

fabio deploy apply --source <DIR> --workspace <ID|NAME>
  [--plan <FILE>] [--item-types <T1,T2>] [--delete-orphans]
  [--allow-unresolved] [--fail-fast] [--force] [--force-all]
  [--concurrency <N>] [--parameters <FILE> --env <NAME>]
  [--no-post-hooks]

fabio deploy export --workspace <ID|NAME> --dir <DIR>
  [--item-types <T1,T2>] [--overwrite] [--dry-run]

fabio deploy init-params --source <DIR>
  [--compare <DIR>] [--source-env <NAME>] [--compare-env <NAME>]
  [--out <FILE>]
```

**Flag interactions:**
- `--plan` is mutually exclusive with `--source`/`--workspace` in `apply`
- `--parameters` requires `--env` (and vice versa)
- `--force` only relevant with `--plan` (overrides staleness check)
- `--force-all` skips content-hash comparison (all matched items become Update)
- `--dry-run` supported on all subcommands (returns planned actions without executing)

### Output Envelope

**Plan output (stdout):**
```json
{"data":{"workspace_id":"...","changes":[...],"warnings":[...],"errors":[...],"summary":{"create":N,"update":N,"rename":N,"delete":N,"skip":N},"source_git":{"commit":"...","branch":"...","dirty":false}}}
```

**Apply output (stdout):**
```json
{"data":{"status":"succeeded|partial_failure|no_changes","succeeded":N,"failed":N,"skipped":N,"duration_ms":N,"failures":[{"change":{...},"error":"...","code":"AUTH_REQUIRED"}],"post_hooks":[...]}}
```

**Export output (stdout):**
```json
{"data":{"status":"exported","workspace_id":"...","output_dir":"...","total_items":N,"exported":N,"skipped":["ItemName: reason"]}}
```

**Init-params output (stdout):**
```json
{"data":{"status":"generated","mode":"scan|diff","source_items":N,"compare_items":N,"rules_generated":N,"guids_found":N}}
```

**Error output (stderr, non-zero exit):**
- Empty source directory: "No items found in source directory"
- Nonexistent source: "Source directory does not exist"
- Workspace not found: "Workspace not found: <name>"
- Plan staleness: "workspace has changed since plan was created"
- Deployment failures: "N deployment(s) failed" (after outputting results)

### Git Metadata Capture

When deploying from a git repository, `get_git_metadata()` automatically captures:
- `branch`: current branch name (`git branch --show-current`; `None` on detached HEAD)
- `commit`: HEAD commit SHA (`git rev-parse HEAD`; `None` if not a git repo)
- `dirty`: whether working tree has uncommitted changes (`git status --porcelain` is non-empty)

Git commands are run with CWD set to source directory. Returns `None` entirely if `git rev-parse HEAD` fails (not a git repo).

### Error Handling Patterns

- **Per-item failures in apply**: Captured in `DeployFailure` with `error` string and `code` (extracted from `FabioError` via downcast, or `"UNKNOWN"`)
- **Post-hook failures**: Non-fatal, reported in output
- **Items without definition support**: Treated as "changed" during plan (Update, never Skip)
- **`getDefinition` failures during rename detection**: Gracefully return `None` (candidate skipped)
- **Invalid base64/non-UTF8 in payloads**: Original payload returned unchanged (no substitution)
- **API errors during apply**: Non-zero exit code with failure count in bail message
- **Partial failures**: Status is `"partial_failure"` (not `"failed"`); succeeded items are still reported

### Known Limitations

- **No incremental plan apply**: Applying a saved plan re-executes all actions (no "only do remaining" resume)
- **creationPayload not validated client-side**: Invalid payloads are rejected by the server at apply time
- **Rename requires logicalId in both source and deployed**: Items without logicalId cannot be rename-detected
- **Large workspaces**: getDefinition is called per-item for rename detection and hash comparison (can be slow with 100+ items)
- **No cross-workspace references**: Logical ID resolution only works within a single workspace deployment
- **Parallel batch isolation**: Items within the same priority batch cannot resolve each other's logical IDs (they execute concurrently with a snapshot)
- **Substring logical ID matches**: `String::replace` is used — a logical ID that appears as substring of longer text will be replaced within it
- **Plan source path must persist**: When applying from a plan file, the source directory at `source_path` must still exist on disk
- **No definition-managed items detection**: Items that don't support `getDefinition` are always marked as Update
- **`_ALL_` wildcard precedence**: Specific env name is checked first (case-insensitive); `_ALL_` is fallback only

