# fabio API Behaviors & Quirks

Critical API behaviors that agents must know for correct operation. These are based on extensive testing against the live Fabric REST API.

## Output Envelope

### Standard Envelope
```json
// List commands
{"data": [...], "count": N}

// With pagination
{"data": [...], "count": N, "continuationToken": "eyJ..."}

// Single object
{"data": {"id": "...", "displayName": "...", ...}}
```

### Error Envelope (stderr)
```json
{"error": {"code": "NOT_FOUND", "message": "Item not found", "hint": "Run: fabio item list --workspace <ws>"}}
```

### Self-Correcting Error Hints (v0.23.0+)

~79% of error paths include structured `hint` fields that tell agents exactly how to fix mistakes — valid enum values, example commands, required roles. Agents should always read the `hint` field before retrying or asking for help. Coverage includes:

- **Deploy errors**: config/flag validation, plan file issues, source directory problems, circular dependency hints, YAML parameter parsing
- **Auth errors**: missing roles, forbidden operations with required role listed
- **Item operations**: invalid type names with valid enum, missing `--dry-run` removal hint
- **Notebook polling**: timeout/failure/cancelled hints pointing to status check commands
- **Capacity/Spark**: SKU enum values, pool validation

Example hint-driven recovery:
```json
{"error":{"code":"INVALID_INPUT","message":"Invalid item type 'notebook'","hint":"Valid types: CopyJob, DataAgent, DataPipeline, Dataflow, Environment, Eventhouse, Eventstream, GraphQLApi, KQLDashboard, KQLDatabase, KQLQueryset, Lakehouse, MLExperiment, MLModel, MirroredDatabase, Notebook, Ontology, Reflex, Report, SQLDatabase, SQLEndpoint, SemanticModel, SparkJobDefinition, Warehouse"}}
```

## Authentication & Token Scoping

| API Surface | Token Scope |
|---|---|
| Fabric REST API | `https://api.fabric.microsoft.com/.default` |
| OneLake DFS/Blob | `https://storage.azure.com/.default` |
| KQL (Kusto) queries | `{kusto_cluster_uri}/.default` |
| SQL (TDS) queries | SQL-scoped AAD token via `require_sql_auth()` |
| Power BI REST API | `https://api.fabric.microsoft.com/.default` (same token — reused) |
| ARM API (capacity lifecycle) | `https://management.azure.com/.default` |

### Authentication Methods

All login methods share the same `~/.fabio/token_cache.json` cache. On Windows, the cache is encrypted with DPAPI (matching Azure CLI behavior).

| Method | Command / Env Var | Notes |
|--------|---------|-------|
| Device code | `fabio auth login` | Headless/SSH; user must visit URL and enter code |
| Browser PKCE | `fabio auth login --browser` | Faster; SSO on macOS with Enterprise Extension |
| Service principal (secret) | `fabio auth login --service-principal --tenant T --client-id C --client-secret S` | CI/CD |
| Service principal (cert PEM) | `fabio auth login --service-principal --tenant T --client-id C --certificate /path/cert.pem` | |
| Service principal (cert PFX) | `fabio auth login --service-principal --tenant T --client-id C --certificate /path/cert.pfx --certificate-password pw` | |
| Federated token (OIDC) | `fabio auth login --service-principal --tenant T --client-id C --federated-token <jwt>` | GitHub Actions OIDC |
| Federated token file | `fabio auth login --service-principal --tenant T --client-id C --federated-token-file /path/token` | File is trimmed of whitespace |
| Windows WAM broker | `fabio auth login --wam` | Windows only; SSO with current Windows account; no browser/code |
| Static access token | `FABIO_ACCESS_TOKEN=<token>` | Fabric Notebooks and environments with pre-existing tokens; same token for all scopes |

**SP error handling**: Empty strings for `--tenant`, `--client-id`, `--client-secret`, `--certificate`, `--federated-token` are treated as "not provided" with structured JSON error output.

**Security**: `--verbose` output and `--dry-run` previews automatically redact sensitive JSON fields (password, client_secret, credentials, access_token, key, connectionString, etc.) before logging. Redaction is recursive and case-insensitive.

### CI/CD Authentication

**GitHub Actions — OIDC federated credentials (recommended, secretless):**

```yaml
permissions:
  id-token: write
  contents: read

steps:
  - uses: azure/login@v3
    with:
      client-id: ${{ vars.AZURE_CLIENT_ID }}
      tenant-id: ${{ vars.AZURE_TENANT_ID }}
      allow-no-subscriptions: true   # Fabric-only auth doesn't need a subscription
  - run: fabio workspace list
```

**GitHub Actions — service principal with client secret (simpler, no extra actions):**

```yaml
steps:
  - env:
      AZURE_CLIENT_ID: ${{ secrets.AZURE_CLIENT_ID }}
      AZURE_TENANT_ID: ${{ secrets.AZURE_TENANT_ID }}
      AZURE_CLIENT_SECRET: ${{ secrets.AZURE_CLIENT_SECRET }}
    run: fabio workspace list
```

Service principal env vars also work outside GitHub Actions. Set these three variables before running fabio:

```bash
export AZURE_CLIENT_ID="<app-id>"
export AZURE_TENANT_ID="<tenant-id>"
export AZURE_CLIENT_SECRET="<secret>"
fabio auth status   # confirms env-var credential source
```

Or use `fabio auth login --service-principal` directly (credentials stored in token cache):

```bash
fabio auth login --service-principal --tenant $TENANT_ID --client-id $CLIENT_ID --client-secret $CLIENT_SECRET
```

> **Fix in v0.16.0**: Prior versions panicked at runtime with "The reqwest feature is required to use the default HTTP client" when using client secret env vars. The `reqwest` and `tokio` features are now enabled on `azure_identity`/`azure_core`.

### Fabric Notebook Authentication

Use `FABIO_ACCESS_TOKEN` to run fabio from inside Microsoft Fabric Notebooks. This is the recommended auth method in notebook environments where `az login` and device code flows are unavailable.

```python
import subprocess, os, json

# Get a token from the current Fabric session
token = notebookutils.credentials.getToken("pbi")
env = {**os.environ, "FABIO_ACCESS_TOKEN": token}

# Run fabio commands with the session token
result = subprocess.run(
    ["fabio", "workspace", "list", "-o", "json"],
    env=env, capture_output=True, text=True
)
workspaces = json.loads(result.stdout)
```

> **Note:** `FABIO_ACCESS_TOKEN` uses the same token for ALL API scopes (Fabric, Storage, SQL, ARM, Graph). The `pbi` scope from `notebookutils.credentials.getToken("pbi")` covers Fabric REST API calls. For OneLake storage operations, you may need a separate token with the storage scope.

## Query Filtering (--query / JMESPath)

The `--query` / `-q` flag uses full **JMESPath** expressions (see [jmespath.org](https://jmespath.org)).

**Breaking change in v0.18.0**: `--query` on lists now requires explicit `[*].field` syntax. Old dot-notation implicit array projection no longer works.

```bash
# List projection (REQUIRED [*] prefix for lists):
fabio workspace list --query '[*].displayName'
fabio lakehouse list-tables --workspace $WS --id $LH --query '[*].name'

# Filter expressions:
fabio item list --workspace $WS --query '[?type==`Notebook`].displayName'

# Pipe and functions:
fabio workspace list --query 'length(@)'
fabio workspace list --query 'sort_by(@, &displayName)[*].id'

# Nested fields still work (backward-compatible):
fabio workspace show --id $WS --query 'data.displayName'
```

## Endpoint Scoping

### Workspace-scoped (most commands)
```
POST https://api.fabric.microsoft.com/v1/workspaces/{workspaceId}/{itemType}s
GET  https://api.fabric.microsoft.com/v1/workspaces/{workspaceId}/{itemType}s/{itemId}
```

### Tenant-scoped (no workspace prefix)
- `fabio connection list/show/create/update/delete`
- `fabio capacity list/show`
- `fabio deployment-pipeline list/show/create/update/delete`
- `fabio gateway list/show/create/update/delete`
- `fabio domain list/show/create/update/delete`
- `fabio admin *` (all admin commands)

## Long-Running Operations (LRO)

### Standard Pattern
1. Client sends POST/PUT/PATCH
2. Server returns 202 with `Location` header and optional `x-ms-operation-id`
3. Client polls Location URL every 2 seconds
4. Terminal states: `Succeeded` or `Failed`
5. On success, follow the resource URL in the response

### LRO Commands
- All `get-definition` and `update-definition` operations
- `item copy`, `item move`, `item bulk-*`
- `notebook run` (with `--wait`)
- `git status`, `git commit`, `git pull`, `git init`
- `sql-endpoint refresh-metadata`
- `lakehouse bulk-create-shortcuts`

### Notebook Job States
`NotStarted` -> `InProgress` -> `Completed` | `Failed` | `Cancelled`

Cold start on small capacity: 2-5 minutes from `NotStarted` to `InProgress`.

## Lakehouse Operations

### load-table Critical Rules
- **Mode values are PascalCase**: `Overwrite` or `Append` (NOT `overwrite`)
- **Format is PascalCase**: `Csv` or `Parquet` (NOT `csv`)
- **JSON is NOT supported**: Must convert to CSV/Parquet first
- **`format` goes inside `formatOptions`**: The discriminated union requires format field in the options object
- **CSV-specific fields with Parquet cause rejection**: Never send `header`/`delimiter` with Parquet format

### list-tables Response Key
Lakehouse tables use `"data"` key in the response. All other Fabric list endpoints use `"value"`.

### OneLake File Operations

#### Upload (DFS 3-step)
1. `PUT /{ws}/{lh}/{path}?resource=file` (create)
2. `PATCH /{ws}/{lh}/{path}?action=append&position=0` (append data)
3. `PATCH /{ws}/{lh}/{path}?action=flush&position={size}` (flush/commit)

#### Atomic Rename for Same-Item Moves (DFS API)
```
PUT https://onelake.dfs.fabric.microsoft.com/{ws}/{lh}/{dest-path}
x-ms-rename-source: /{ws}/{lh}/{src-path}
x-ms-version: 2021-06-08
```
Returns **201** on success (O(1) metadata operation — no data transfer). Works for **both files and directories** (entire table directory tree renamed atomically). **Fails with 403** for cross-item or cross-workspace moves (auth scope mismatch) — fabio automatically falls back to copy + delete in that case.

#### Server-Side Copy (Blob API)
```
PUT https://onelake.blob.fabric.microsoft.com/{ws}/{lh}/{dest-path}
x-ms-copy-source: https://onelake.blob.fabric.microsoft.com/{ws}/{lh}/{src-path}
```
Returns 202 with pending status. Poll via HEAD for completion.

#### No Native Rename/Move (cross-item only)
For cross-item or cross-workspace moves, OneLake rejects `x-ms-rename-source`. These use copy + delete. Same-item moves use atomic rename (see above).

#### Recursive Delete
```
DELETE https://onelake.dfs.fabric.microsoft.com/{ws}/{lh}/Tables/{name}?recursive=true
```

#### File Listing Quirk
When `directory` parameter is specified in DFS listing, paths appear doubled (e.g., `Files/Files/myfile.csv`). fabio normalizes this automatically. Use root listing (no `directory` param) to get real paths prefixed with item ID.

### Sync Command
Compares source and destination using ETag/MD5. Only copies new/modified files. `--delete` removes files in destination that don't exist in source. Supports rsync-inspired flags (`--include`, `--exclude`, `--size-only`, `--no-overwrite`, `--force`, `--no-recursive`, `--max-delete`, `--existing`, `--remove-source-files`, `--min-size`, `--max-size`, `--itemize`) and `--local` for local-to-remote sync. See the full behavior section at the end of this file.

#### Rename Detection
When `--delete` is active, `lakehouse sync` detects renamed/moved files and performs atomic O(1) renames at the destination instead of copy + delete:

- **ETag-based** (zero extra API calls): When source-only and dest-only files match by ETag + size, the file is renamed atomically. Works for files uploaded with `fabio lakehouse upload` (which stores Content-MD5 on flush, preserving ETag across renames).
- **Checksum-based** (`--checksum --delete`): A second pass compares Content-MD5 via HEAD requests. Only fires when exactly one candidate matches. Handles Fabric-generated files (Spark/pipelines) that lack Content-MD5, falling back to unique-size matching.

Output includes a `"renamed"` count for files handled via atomic rename.

**Note**: Fabric-generated files (Spark, data pipelines, load-table) do NOT have Content-MD5 stored. Their ETags change on rename, so they are not detectable by the ETag pass — only by checksum + unique-size fallback.

#### Content-MD5 on Upload
`lakehouse upload` stores an MD5 hash via `x-ms-content-md5` header on DFS flush. OneLake preserves this hash across server-side copy and atomic rename, enabling content-based matching.

## Warehouse & SQL Database

### Query Input Methods
```bash
# Inline SQL
fabio warehouse query --workspace $WS --id $WH --sql "SELECT 1"

# From file (prefix with @)
fabio warehouse query --workspace $WS --id $WH --sql @queries/report.sql

# From stdin (pipe)
echo "SELECT 1" | fabio warehouse query --workspace $WS --id $WH
```

### SQL Database Import Type Inference
Type widening rules (never narrows):
- `Unknown` -> first observation sets type
- `Int` -> `BigInt` -> `Float` -> `NVarChar` (progressive widening)
- NVarChar length: `clamp(observed_max_len * 2, 50, 4000)`
- Batch size: 100 rows per INSERT (configurable via `--batch-size`)
- Timeout: 120s per batch

### SQL Database Capacity Requirement
F4+ capacity required for TDS connections. F2 fails with error 18456 State 240.

### SQL Endpoint Query (v0.23.0+)
`fabio sql-endpoint query` resolves the connection string from the dedicated `/connectionString` API endpoint, uses `displayName` as the initial catalog, then delegates to TDS execution. Same input modes as warehouse/sql-database:
```bash
# Inline SQL
fabio sql-endpoint query --workspace $WS --id $SQLEP --sql "SELECT TOP 10 * FROM dbo.sales"

# From file
fabio sql-endpoint query --workspace $WS --id $SQLEP --sql @query.sql

# From stdin
echo "SELECT COUNT(*) FROM dbo.orders" | fabio sql-endpoint query --workspace $WS --id $SQLEP
```
SQL endpoints are read-only (auto-created alongside lakehouses). They cannot be created or deleted independently.

## KQL Database Queries

### Query Routing
- Management commands (starting with `.`): `POST {kusto_uri}/v1/rest/mgmt`
- Data queries: `POST {kusto_uri}/v2/rest/query`

### Token Scope
KQL uses a non-standard scope: `{kusto_cluster_uri}/.default` (NOT the standard Fabric scope).

### KQL Queryset Definition Format
```json
{
  "queryset": {
    "version": "1.0.0",
    "dataSources": [{"id": "...", "clusterUri": "...", "type": "...", "databaseName": "..."}],
    "tabs": [{"id": "...", "content": "KQL query\\nwith newlines", "title": "Tab Name", "dataSourceId": "..."}]
  }
}
```
Tab selection in `kql-queryset run` is case-insensitive by title.

## Semantic Model

### Format Requirements
| Scenario | Format | Key Requirement |
|---|---|---|
| Direct Lake | TMDL | Required for `mode: directLake` partitions |
| Import mode | model.bim | `compatibilityLevel: 1604` + `powerBI_V3` |

### Direct Lake Key Points
- Connection flag `--connection` takes the SQL Analytics Endpoint ID (NOT lakehouse ID)
- After creation, call `refresh` to frame the model (without framing, DAX queries fail with error 3242524690)
- `Sql.Database()` second parameter must be SQL endpoint ID (not lakehouse ID)
- Storage mode must be `Abf` (NOT `PremiumFiles`)
- Call `takeover` after creation to make editable in portal

### definition.pbism Format
```json
// For TMDL (v4.2)
{"$schema": "https://developer.microsoft.com/json-schemas/fabric/item/semanticModel/definitionProperties/1.0.0/schema.json", "version": "4.2", "settings": {}}

// For model.bim (v3.0)
{"version": "3.0"}
```

## Report Definitions

### Format Selection
- **PBIR-Legacy** (`report.json`): Required for programmatic visuals that render data (uses `prototypeQuery`)
- **PBIR** (`definition/` folder): Better for source control, but cannot render data programmatically

### definition.pbir (Always Required)
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

### updateDefinition Rules
- ALWAYS include `definition.pbir` part, even if only updating `report.json`
- Can switch formats by sending appropriate parts
- `prototypeQuery` is REQUIRED in visual config for data to render

## Eventstream

### add-source/add-destination Pattern
These high-level commands fetch the current definition, merge the new node, and update. They abstract the complex eventstream topology.

### Destination Properties (JSON)
```json
{
  "dataIngestionMode": "DirectIngestion",
  "workspaceId": "<ws-id>",
  "itemId": "<kql-db-id>",
  "tableName": "TableName",
  "connectionName": "conn-name",
  "mappingRuleName": "JsonMapping"
}
```

## Ontology API

### Critical: JSON Key Ordering
The Fabric Ontology API uses ordered JSON deserialization for data bindings. `sourceType` MUST be the first key in `sourceTableProperties`. fabio normalizes this automatically via `normalize_data_binding()`.

### Data Binding Structure
```json
{
  "id": "<uuid-format-required>",
  "dataBindingConfiguration": {
    "dataBindingType": "NonTimeSeries",
    "sourceTableProperties": {
      "sourceType": "LakehouseTable",
      "workspaceId": "...",
      "itemId": "...",
      "sourceTableName": "...",
      "sourceSchema": "dbo"
    },
    "propertyBindings": [
      {"sourceColumnName": "col", "targetPropertyId": "prop-id"}
    ]
  }
}
```

### OWL Import: Supported Formats and Mapping

`fabio ontology import` parses OWL ontologies and converts them to Fabric's internal definition format:

- **Format detection**: Extension-based — `.rdf`, `.owl`, `.xml` → RDF/XML (parsed via quick-xml); `.jsonld`, `.json` → JSON-LD
- **OWL → Fabric mapping**:
  - `owl:Class` → `EntityType`
  - `owl:DatatypeProperty` (with `rdfs:domain`) → property on the parent EntityType
  - `owl:ObjectProperty` → `RelationshipType` (domain/range → source/target)
  - `ont:isIdentifier` → `entityIdParts` (key property)
  - XSD types → Fabric `valueType`: `xsd:string` → `String`, `xsd:integer`/`xsd:long` → `BigInt`, `xsd:double`/`xsd:float` → `Double`, `xsd:boolean` → `Boolean`, `xsd:dateTime` → `DateTime`
- **Ontology Playground compatible**: Files from [microsoft/Ontology-Playground](https://github.com/microsoft/Ontology-Playground) (`.rdf` catalogue files) import directly with no transformation
- **Two modes**: `--workspace + --id` pushes directly to a Fabric Ontology item; `--output-dir` writes the EntityTypes/RelationshipTypes directory structure locally for inspection

### OWL Export: Fabric → OWL Conversion

`fabio ontology export` reads a Fabric Ontology via REST API and serializes to standard OWL:

- **Format `rdf`**: OWL RDF/XML — compatible with Ontology Playground, Protégé, rdflib, Apache Jena, and `fabio ontology import`
- **Format `jsonld`**: OWL JSON-LD — importable by `fabio ontology import` and standard JSON-LD processors
- **Fabric → OWL mapping**:
  - `EntityType` → `owl:Class`
  - EntityType properties → `owl:DatatypeProperty` (with XSD range types)
  - `entityIdParts` → `ont:isIdentifier` annotation
  - `RelationshipType` → `owl:ObjectProperty` (domain/range from source/target)
- **Round-trip verified**: Import → Export → Re-import preserves entity types, properties, and relationships

### Context Tenant OWL/RDF Formats

`fabio context tenant` now supports 5 output formats (3 new in v0.29.0):

| Format | Content | Use case |
|--------|---------|----------|
| `graph` | Instance data, native arrays | Agent memory, JMESPath, merge |
| `jsonld` | Instance data, RDF JSON-LD | Triple stores, SPARQL |
| `owl` | Schema only, OWL JSON-LD | `fabio ontology import` |
| `rdf` | Schema only, OWL RDF/XML | `fabio ontology import`, Ontology Playground, Protégé |
| `full` | Schema + instances, RDF/XML | Universal — triple stores + Fabric Ontology import |

- `rdf` output does NOT contain `rdf:Description` nodes (schema only)
- `full` output contains both `owl:Class` schema AND `rdf:Description` instance data in one file
- `owl` output is bare JSON-LD without `{"data":...}` envelope — directly consumable by ontology import parser

### Definition Persistence Rule
Single-part `updateDefinition` with only a datasource file is silently dropped. Must include ALL parts together:
- `data_agent.json` + `stage_config.json` + `datasource.json`

### Data Source Types
`unknown`, `lakehouse_tables`, `lakehouse`, `data_warehouse`, `kusto`, `semantic_model`, `graph`, `mirrored_database`, `mirrored_azure_databricks`, `ontology`, `sql_database`

### Datasource Type Mapping (Fabric item → internal type)
Supported artifact types for `add-datasource`: `Lakehouse` → `lakehouse_tables`, `Warehouse` → `data_warehouse`, `KQLDatabase` → `kusto`, `SemanticModel` → `semantic_model`, `Ontology` → `ontology`, `GraphModel` → `graph`, `MirroredDatabase` → `mirrored_database`, `SQLDatabase` → `sql_database`

### `--answer` vs `--query` on add-fewshot
The `add-fewshot` command uses `--answer` (not `--query`) for the SQL/KQL value because `--query` is the reserved global JMESPath flag. Using `--query` causes the JMESPath engine to evaluate the SQL text and produce null output. The flag `--sql` is a visible alias for `--answer`.

```bash
# Correct
fabio data-agent add-fewshot --workspace $WS --id $DA \
  --datasource $LH --question "Top customer?" \
  --answer "SELECT TOP 1 customer_name FROM orders ORDER BY total DESC"

# Wrong — --query is intercepted by JMESPath engine
fabio data-agent add-fewshot --workspace $WS --id $DA \
  --datasource $LH --question "..." --query "SELECT ..."  # null output
```

### Duplicate Few-shot Auto-rename
When uploading a few-shot question that already exists (exact match), the new entry is auto-renamed with a `[N]` suffix (e.g., `Who is the top customer? [1]`). This matches the official Python SDK behavior and prevents silent overwrite.

### Few-shot Upload File Formats
`upload-fewshots` accepts JSON or CSV/TSV:
- **JSON**: `[{"question":"...","query":"..."}]` (array of objects)
- **CSV**: header row with `question` and `query` (or `answer`) columns; case-insensitive headers; empty rows silently skipped
- **TSV**: same as CSV but tab-delimited (auto-detected by `.tsv` extension)

### update-config --instructions-file
`--instructions-file <path>` loads AI instructions from a file, useful for multi-line text. Mutually exclusive with `--instructions` (both cannot be specified together).

### Preview Runtime
`--enable-preview-runtime` sets `experimental.enableExperimentalFeatures: true` in `stage_config.json`, activating the agentic NL2SQL reasoning path. Use `--disable-preview-runtime` to turn it off.

### Operability Limits
- Max **5 datasources** per agent
- Max **100 few-shot examples** per datasource
- Agent responses capped at **25 rows** and **25 columns**
- Cross-region limitation: agent capacity must be in same region as data source capacity

### Publishing via CI/CD (Officially Supported)
`fabio data-agent publish` copies all `Files/Config/draft/*` parts to `Files/Config/published/*` and adds `publish_info.json` via `updateDefinition`. This is the officially documented CI/CD publish path (confirmed June 2026 — no portal required).

```bash
fabio data-agent publish --workspace $WS --id $DA
```

The publish_info.json format:
```json
{"$schema": "https://developer.microsoft.com/json-schemas/fabric/item/dataAgent/definition/publishInfo/1.0.0/schema.json", "description": "<publish description>"}
```

Publishing activates the OpenAI Assistants-compatible endpoint:
`https://api.fabric.microsoft.com/v1/workspaces/{wsId}/dataagents/{agentId}/aiassistant/openai`

### M365 Copilot Agent Store Publishing
`--to-m365` on `publish` additionally calls the internal M365 endpoint to register the agent in the Microsoft 365 Copilot Agent Store. Requires a successful standard publish first.

## Definition Operations (Generic Pattern)

### getDefinition
```
POST /workspaces/{ws}/{type}s/{id}/getDefinition
Body: {}
```
Returns 202 (LRO). Result contains base64-encoded definition parts:
```json
{
  "definition": {
    "parts": [
      {"path": "file.json", "payload": "<base64>", "payloadType": "InlineBase64"}
    ]
  }
}
```

### updateDefinition
```
POST /workspaces/{ws}/{type}s/{id}/updateDefinition
Body: {"definition": {"parts": [{"path": "...", "payload": "<base64>", "payloadType": "InlineBase64"}]}}
```

## Job Types (for job-scheduler run-on-demand)
- Notebook: `RunNotebook`
- Data Pipeline: `Pipeline` (PascalCase)
- Spark Job: `sparkjob` (lowercase)

## Connection Parameters Format
User-provided JSON `{"key": "value"}` is converted to the API format:
```json
[{"dataType": "Text", "name": "key", "value": "value"}]
```

## Naming Restrictions
- Digital Twin Builder names: no hyphens allowed
- MirroredAzureDatabricksCatalog names: no hyphens allowed
- Workspace names: most characters allowed
- Item names: alphanumeric + spaces + underscores (varies by type)

## Rate Limiting
- Spark on small capacity: LRO reports 430 `TooManyRequestsForCapacity`
- fabio retries automatically for parallel operations
- Prefer bulk APIs over repeated individual calls

## Capacity Requirements
- SQL Database TDS: requires F4+ capacity
- Spark notebooks: any capacity (but cold start delay on small)
- Most Fabric operations: any capacity with active state
- `CAPACITY_INACTIVE` error if workspace capacity is paused/deallocated

## Apache Airflow Specifics
- File operations require `?beta=true` query parameter
- File upload uses `Content-Type: text/plain` (JSON body rejected)
- DAGs are files, not definitions

## OneLake Security
- `upsert` replaces ALL roles atomically (PUT semantics, not PATCH)
- No individual role create/update — always send the full set

## Git Integration (Azure DevOps)

### Cross-Service Identity Requirement
Fabric's git integration uses the authenticated user's identity to access Azure DevOps. The user (OID from the Fabric token) must be:
1. A member of the Azure DevOps organization
2. Have at least Contributor access to the project/repo

Without this, `git connect` returns `InsufficientPrivileges` (403). The error looks like a workspace permission issue but is actually Azure DevOps rejecting the identity.

### Same AAD Tenant Required
The Azure DevOps organization must be backed by (connected to) the same Azure AD tenant as the Fabric workspace. Cross-tenant git integration is not supported with "Automatic" credentials.

### Azure DevOps vs GitHub Credentials
| Provider | Connection ID | Credential Mode |
|----------|---------------|-----------------|
| `azure-devops` | NOT required | "Automatic" — Fabric uses caller's OAuth token directly |
| `github` | ALWAYS required | Must pre-configure a `GitHubSourceControl` connection |

### `directoryName` is Required
The Fabric API rejects `git connect` without a `directoryName` field in `gitProviderDetails`. fabio defaults to `"/"` (repo root). Omitting it returns: `InvalidInput: The DirectoryName field is required.`

### Permission Propagation Delay
After adding a user to an Azure DevOps org/project, permissions take 5-10 seconds to propagate. Fabric's git connect can fail with 403 immediately after granting access. Retry after a brief wait.

### Multiple Workspaces on Same Repo
Different Fabric workspaces can connect to the same Azure DevOps repo and branch (same `directoryName`). Each workspace maintains independent sync state. Useful for CI/CD workspace-per-environment patterns.

### Repo Must Have a Branch
Azure DevOps repos without any commits have no `defaultBranch`. You must push an initial commit to create `main` before Fabric can connect.

## Gateway Operations
- PATCH requires `"type"` field in body or fails silently
- Non-existent principal returns 500 (not clean validation)
- Duplicate role assignment returns 409 with typo "assignemnt" in error message
- VNet gateway creation takes 60-90 seconds (no LRO — returns 201 directly after delay)
- Roles: `Admin`, `ConnectionCreator`, `ConnectionCreatorWithResharing`
- Cannot demote last Admin (returns error)
- `inactivityMinutesBeforeSleep` must be one of: 30, 60, 90, 120, 150, 240, 360, 480, 720, 1440
- `numberOfMemberGateways` must be 1-9

## Admin API

### Required Permissions
All admin endpoints require **Fabric Admin** role (tenant-level). Standard workspace Admin/Member roles are NOT sufficient. Errors include: `"The caller does not have sufficient scopes to perform this operation"`.

### Non-Standard Response Keys
Unlike most Fabric APIs that use `"value"` as the array key, admin endpoints use varied keys:

| Endpoint | Response Array Key |
|----------|-------------------|
| `/admin/workspaces` | `"workspaces"` |
| `/admin/items` | `"itemEntities"` |
| `/admin/workspaces/{id}/users` | `"accessDetails"` |
| `/admin/items/{id}/users` | `"accessDetails"` |
| `/admin/users/{id}/access` | `"accessEntities"` |
| `/admin/domains` | `"domains"` |
| `/admin/tenantsettings` | `"tenantSettings"` |
| `/admin/tags` | `"value"` (standard) |
| `/admin/workloads` | `"value"` (standard) |
| `/admin/workloads/assignments` | `"value"` (standard) |

fabio normalizes all these into the standard `{"data": [...], "count": N}` envelope.

### Admin Workspace Fields
Admin workspace responses use `name` (NOT `displayName`). Fields: `id`, `name`, `state`, `type`, `capacityId`, `tags`.

### Admin Item Fields
Admin item responses use `name` (NOT `displayName`). Fields: `id`, `type`, `name`, `state`, `lastUpdatedDate`, `creatorPrincipal`, `workspaceId`, `capacityId`.

### Tenant Settings

**Structure:**
```json
{
  "settingName": "ExportToImage",
  "title": "Export to image",
  "enabled": true,
  "tenantSettingGroup": "Export and sharing settings",
  "canSpecifySecurityGroups": true,
  "delegateToCapacity": false,
  "delegateToDomain": false,
  "enabledSecurityGroups": [],
  "excludedSecurityGroups": []
}
```

**Update body (minimum):**
```json
{"enabled": true}
```

**Update response:** Returns ALL settings in the SAME group (not just the updated one).

**Capacity override rules:**
- Only settings with `"delegateToCapacity": true` can have capacity-level overrides
- Attempting to override a non-delegatable setting returns: "The request could not be processed due to missing or invalid information"
- Override body: `{"enabled": true|false, "delegateToWorkspace"?: bool}`

**Domain override rules:**
- Only settings with `"delegateToDomain": true` can have domain-level overrides
- Same pattern as capacity overrides

### Tag Operations

**Create body:**
```json
{"createTagsRequest": [{"displayName": "Production"}]}
```
Optional scope: `{"type": "Tenant"}` or `{"type": "Domain", "domainId": "<uuid>"}`.

**Response:** `{"tags": [{"id": "...", "displayName": "...", "scope": {...}}]}`

**Rate limits:** Tag operations limited to 25 requests/minute.

### Domain Workspace Assignment

**By capacities:** Assigns ALL workspaces on that capacity to the domain.
```json
{"capacitiesIds": ["<uuid>"]}
```

**By principals:** Assigns all workspaces owned/administered by those principals.
```json
{"principals": [{"id": "<uuid>", "type": "User"}]}
```
Requires `--principal-type` flag.

**Additive behavior:** `assign-domain-workspaces-by-principals` only assigns workspaces NOT already assigned to another domain.

### Domain Role Sync
- `sync-domain-roles-to-subdomains` requires `--role` flag
- Only Contributors can be synced — syncing Admins returns: "Syncing admins to subdomains is not supported"

### Bulk Role Assignment
```json
{"type": "Contributors", "principals": [{"id": "<uuid>", "type": "User"}]}
```
Type values: `"Contributors"` or `"Admins"`.

### Sharing Links (LRO)
Both sharing link commands are LRO (return 202, must poll):
- `remove-all-sharing-links`: `{"sharingLinkType": "OrgLink"}` — type values: `OrgLink`, `GuestLink`, `AnonymousLink`, `SpecificPeopleLink`
- `bulk-remove-sharing-links`: Only supports Report type. Other types return "not supported for the requested item type"

### Labels (Microsoft Purview Required)
- `bulk-set-labels` requires M365 E5 licensing + Purview label policy configured in tenant
- `bulk-remove-labels` works without Purview (returns per-item status even if no label set)

### External Data Shares
- `list-external-data-shares` requires tenant setting `AllowExternalDataSharingSwitch` enabled
- Without it: FORBIDDEN with "tenant setting 'External data sharing' is disabled"

### Workload Assignment Body Format
Discriminated union with `type` field:
```json
// Tenant-level
{"type": "Tenant", "workloadId": "<id>"}

// Capacity-level
{"type": "Capacity", "workloadId": "<id>", "capacityId": "<uuid>"}

// Workspace-level
{"type": "Workspace", "workloadId": "<id>", "workspaceId": "<uuid>"}
```

### Workspace Restore
- `POST /admin/workspaces/{id}/restore` with `{"restoredWorkspaceName": "<name>", "capacityId": "<uuid>"}`
- Note: The `restoredWorkspaceName` parameter is ignored by server — workspace keeps original name

### Temporary Admin Access
- `grant-admin-access` / `remove-admin-access` manage TEMPORARY admin access only
- Returns NOT_FOUND if the caller already has permanent Admin access to the workspace

### Admin Error Enrichment
fabio provides 6 targeted error patterns for admin commands:
1. **External data sharing disabled** → exact setting name + CLI enable command
2. **Tenant setting disabled** → Admin Portal path + CLI command
3. **Item type not supported** → only Report type works for sharing link removal
4. **Purview labels not configured** → M365 E5 + licensing prerequisites
5. **Feature not available** → tenant admin feature flag guidance
6. **Sync admins not supported** → suggests `--role Contributor`

## Deploy Command

### Stateless Content-Hash Diffing
Deploy uses SHA-256 content hashing over sorted `path + \x00 + payload` pairs to detect changes. No state file exists — always queries the live workspace. No `.tfstate` equivalent.

### Source Directory Format
```
{DisplayName}.{ItemType}/
├── .platform                    # Required: {"$schema":"...","metadata":{"type":"...","displayName":"..."},"config":{"logicalId":"..."}}
├── definition-part.json         # Base64 payload parts (varies by item type)
└── creationPayload.json         # Optional: merged into creation body as `creationPayload` field
```

### Workspace Resolution
- GUID detection: 36 characters with 4 dashes → used directly as workspace ID
- Display name: resolved via `GET /workspaces?displayName=<name>` lookup

### Changeset Actions
| Action | Behavior |
|--------|----------|
| `Create` | POST to create item (with definition if present) |
| `Update` | POST updateDefinition (content hash differs) |
| `Rename` | PATCH displayName + updateDefinition |
| `Delete` | DELETE item (sequential, never parallel) |
| `Skip` | Content hash matches — no action needed |

### Rename Detection
Two-pass matching algorithm:
1. **First pass**: Match source items to deployed items by `(type, name)` pairs
2. **Second pass**: Unmatched source items with `logicalId` in `.platform` get candidates checked via `getDefinition` on deployed items — compares `logicalId` from their `.platform` part

### Logical ID Resolution
String replacement (`String::replace`) in base64 payloads at apply time. Resolves items created earlier in the same deploy session. Example: a report referencing a semantic model created in the same batch.

### Parameter Substitution
Applied in order (each stage feeds into the next):
1. **find_replace** — simple string replacement in payloads
2. **key_value_replace** — structured key-value pairs
3. **spark_pool** — Spark pool name/ID substitution
4. **semantic_model_binding** — semantic model ID replacement in report bindings

### Post-Deploy Hooks
| Item Type | Hook Action | Notes |
|-----------|-------------|-------|
| SemanticModel | `POST /refreshes` | Frames Direct Lake models |
| Environment | `POST /staging/publish` | Publishes staged changes |

- Failures are **non-fatal** (reported in output, don't fail the deploy)
- Hooks **never fire** during `--dry-run`
- Opt-out via `--no-post-hooks`

### Plan Staleness Detection
Workspace fingerprint = SHA-256 of sorted `(id, type, name)` tuples. If fingerprint changes between plan and apply, deploy fails unless `--force` is specified.

### Deploy Ordering
42 item types in `DEPLOY_ORDER` — deployed in dependency order:
```
storage → compute → code → models → reactive → APIs → ML → graph → viz
```

### Concurrency
- Default: 8 concurrent operations (semaphore-bounded `tokio::spawn`)
- `DataPipeline`: always sequential (ordering dependencies)
- Deletes: always sequential

### Empty Definitions
Items with no definition parts (Lakehouse, MLModel):
- On create: omit `definition` field entirely
- On update: skip `updateDefinition` call

## Workspace API Behaviors

### Folder Management
Standard CRUD at `/workspaces/{ws}/folders`:
- Create: `POST` with `{"displayName": "...", "parentFolderId": "<id>" | null}`
- Move items: `POST /workspaces/{ws}/folders/{id}/move` with body:
  ```json
  {"targetFolderId": "<id>" | null}
  ```
  `null` moves to workspace root.

### Tags
- Apply: `POST /workspaces/{ws}/applyTags` with `{"tagIds": ["<uuid>", ...]}`
- Unapply: `POST /workspaces/{ws}/unapplyTags` with `{"tagIds": ["<uuid>", ...]}`

### Domain Assignment
- Assign: `POST /workspaces/{ws}/assignToDomain` with `{"domainId": "<uuid>"}`
- Unassign: `POST /workspaces/{ws}/unassignFromDomain`

### OneLake Settings
- `POST /workspaces/{ws}/modifyDefaultTier?defaultTier={value}`
- **IMPORTANT**: The tier value goes as a **query parameter**, not in the request body
- Values: `Hot`, `Cool`, `Cold`

### Lifecycle Policies
- Export: `POST /workspaces/{ws}/exportLifecyclePolicy` (returns JSON)
- Import: `POST /workspaces/{ws}/importLifecyclePolicy` (accepts JSON body)

### Network Policies
| Policy | Endpoint |
|--------|----------|
| Firewall rules | `/workspaces/{ws}/networkPolicies/firewallRules` |
| Git outbound | `/workspaces/{ws}/networkPolicies/gitOutbound` |
| Inbound Azure resources | `/workspaces/{ws}/networkPolicies/inboundAzureResources` |
| Outbound cloud connections | `/workspaces/{ws}/networkPolicies/outboundCloud` |
| Outbound gateways | `/workspaces/{ws}/networkPolicies/outboundGateways` |

- **OAP outbound restriction** requires F64+ capacity
- **Inbound** works on Trial capacity

### Identity Provisioning
- `POST /workspaces/{ws}/provisionIdentity` is **LRO** (returns 202)
- Response includes `applicationId` + `servicePrincipalId`

## Item API Behaviors

### Type Filter on List
`GET /workspaces/{ws}/items?type={PascalCase}` — type value must be PascalCase (e.g., `Notebook`, `SemanticModel`, `DataPipeline`).

### Copy Pattern
1. `POST /workspaces/{ws}/{type}s/{id}/getDefinition` (LRO) — get definition parts
2. `GET /workspaces/{ws}/{type}s/{id}` — get metadata (displayName, description)
3. `POST /workspaces/{destWs}/items` with definition (LRO) — create in destination

### Move Pattern
Copy + DELETE source item. No native move API exists.

### Bulk Operations (All LRO)
| Operation | Endpoint |
|-----------|----------|
| `bulkExportDefinitions` | `POST /workspaces/{ws}/items/bulkExportDefinitions` |
| `bulkImportDefinitions` | `POST /workspaces/{ws}/items/bulkImportDefinitions` |
| `bulkMove` | `POST /workspaces/{ws}/items/bulkMove` |

### External Data Shares
Standard CRUD at `/workspaces/{ws}/items/{id}/externalDataShares`. Requires tenant setting `AllowExternalDataSharingSwitch` enabled.

### Identity Assignment
`POST /workspaces/{ws}/items/{id}/assignIdentity` — assigns workspace managed identity to the item.

## Cross-Database Query Behaviors

### Three-Part Naming Support
| Source Endpoint | Three-Part Naming | Notes |
|----------------|-------------------|-------|
| Lakehouse SQL endpoint | YES | Can query other DBs in same workspace |
| Warehouse | YES | Same TDS endpoint, sees `sys.databases` |
| SQL Database | NO | Error 40515: "Reference to database and/or server name is not supported" |

### Direction
Cross-database querying is **one-way**:
- Lakehouse/Warehouse → SQL Database: **works**
- SQL Database → Lakehouse/Warehouse: **does NOT work**

### Practical Pattern
Use the **lakehouse SQL endpoint as query hub** for JOINs across item types:
```sql
SELECT l.col
FROM dbo.local_table l
JOIN SqlDb.dbo.remote_table r ON l.id = r.id
```

## Report Definition Formats

### PBIR-Legacy vs PBIR
| Aspect | PBIR-Legacy | PBIR |
|--------|-------------|------|
| Structure | Single `report.json` | `definition/` folder tree |
| Data rendering | Works with `prototypeQuery` | Stores correctly but renders NO data |
| Future | Deprecated at GA | Only supported format at GA |

### definition.pbir v2.0 Schema (Recommended)
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

### PBIR-Legacy Visual Config
Required fields for visuals that render data:
- `visualType` — the chart/visual type
- `projections` — field-to-role bindings
- `dataTransforms` — metadata for rendering engine
- `prototypeQuery` — **REQUIRED** for data to actually render

### PBIR visual.json Limitation
PBIR visuals use `query.queryState` which stores correctly but **renders NO data** in the portal. The PBIR schema does not support `prototypeQuery`.

### Supported visualType Values
`card`, `barChart`, `columnChart`, `lineChart`, `pieChart`, `donutChart`, `tableEx`, `matrix`, `map`, `scatterChart`, `slicer`, `kpi`

### Projection Role Names by visualType
| visualType | Roles |
|------------|-------|
| `card` | `Values` |
| `barChart` / `columnChart` | `Category` + `Y` |
| `lineChart` | `Category` + `Y` (+ optional `Series`) |
| `pieChart` / `donutChart` | `Category` + `Y` |
| `tableEx` | `Values` (array) |
| `matrix` | `Rows` + `Columns` + `Values` |
| `map` | `Category` + `Size` + `Color` |
| `scatterChart` | `Category` + `X` + `Y` + `Size` |
| `slicer` | `Values` |
| `kpi` | `Indicator` + `TrendAxis` + `Goal` |

### updateDefinition Rule
ALWAYS include `definition.pbir` part, even if only updating `report.json` or visual files.

## Eventstream Behaviors

### Definition Format
Two files:
- `eventstream.json` — topology (sources, destinations, streams, operators, compatibilityLevel)
- `eventstreamProperties.json` — retention (`retentionTimeInDays`: 1-90) and throughput (`eventThroughputLevel`: Low/Medium/High)

### Source Types
`CustomEndpoint`, `AzureEventHub`, `AzureIoTHub`, `SampleData`, `AmazonKinesis`, `ApacheKafka`, `ConfluentCloud`, `GooglePubSub`, CDC types (`AzureSQLDBCDC`, `MySQLCDC`, `PostgreSQLCDC`), Fabric events (`FabricWorkspaceItemEvents`, `FabricJobEvents`, `FabricOneLakeEvents`)

### Destination Types
`Eventhouse`, `Lakehouse`, `CustomEndpoint`, `Activator`

### Eventhouse Destination Critical Rule
The `itemId` field must be the **KQL Database item ID** (NOT the Eventhouse ID). Using the Eventhouse ID causes: "Unable to extract cluster URL from the Eventhouse KQL database item ID".

### Two Ingestion Modes
| Mode | Table Creation | Requirements |
|------|---------------|--------------|
| `ProcessedIngestion` | Auto-creates table (with system columns) | `inputSerialization` in properties |
| `DirectIngestion` | Requires pre-created table + mapping | `connectionName` + `mappingRuleName` |

### Graph-Like Topology
Nodes reference each other by `name` via `inputNodes` arrays. Structure: source → stream → destination/operator. Names must be unique across all node types.

### No Individual Source/Destination CRUD
Sources and destinations can only be created/deleted via `update-definition` (full definition replacement). Individual `GET .../sources/{id}` and `GET .../destinations/{id}` are read-only.

### add-source / add-destination Helpers
High-level commands that:
1. Fetch current definition
2. Merge in the new node
3. Auto-create default streams
4. Push updated definition via `updateDefinition`

## Data Agent Behaviors (Expanded)

### Public Staging Management API (Jun 2026)
Data agent configuration uses dedicated staging endpoints — NOT definition-based read-modify-write. All config changes go to staging (draft); `publish` promotes to production.

Key endpoints:
- `GET/PATCH .../staging/settings` — manages `aiInstructions`
- `GET/POST/PATCH/DELETE .../staging/datasources` — full CRUD; POST is LRO (schema discovery 1-5 min)
- `GET/PATCH/DELETE .../staging/datasources/{id}/elements` — schema tree management
- `GET/POST/DELETE .../staging/datasources/{id}/fewshots` + `POST .../fewshots/deleteAll`
- `POST .../staging/publish` — promote draft to production
- `POST .../staging/reset` — discard draft, revert to published

### Read Commands Accept `--stage`
The `--stage staging|published` flag on read commands allows inspecting either draft or production state:
- `get-config`, `list-datasources`, `show-datasource`, `list-elements`, `list-fewshots`, `show-fewshot`

### Datasource Creation is LRO
`add-datasource` triggers async schema discovery (1-5 minutes on cold lakehouses). Use `--lro-timeout 300` for reliable completion.

```bash
fabio data-agent add-datasource --workspace $WS --id $DA \
  --artifact $LH --lro-timeout 300
# Wait for indexing before querying elements
fabio data-agent list-elements --workspace $WS --id $DA --datasource $LH
```

### Stdin Fallback for `--prompt`
When `--prompt` is omitted, `data-agent query` reads the question from stdin:

```bash
echo "How many orders last month?" | \
  fabio data-agent query --workspace $WS --id $DA
```

### M365 Copilot Agent Store Publishing
Publishing to the M365 Copilot Agent Store is **NOT available via public REST API**. It is only accessible through the Fabric portal or the `fabric-data-agent-sdk` Python package. The `--to-m365` flag on `publish` is deprecated/unsupported.

### Operability Limits
- Max **5 datasources** per agent
- Max **100 few-shot examples** per datasource
- Agent responses capped at **25 rows** and **25 columns**

## Environment API

### Staging/Publish Workflow
Environments use a two-stage model:
1. Make changes (libraries, Spark settings) — stored in **staging**
2. `POST /workspaces/{ws}/environments/{id}/staging/publish` — promotes to live

### Library Management
- Libraries exist in both **published** (active) and **staging** (pending) states
- Export/import available for both states
- Changes to libraries require publish to take effect

### Publish Behavior
- Publish is **fire-and-forget** (NOT LRO — returns immediately)
- Cancel via `POST /workspaces/{ws}/environments/{id}/staging/cancelPublish`
- Check publish state via `GET /workspaces/{ws}/environments/{id}`

## Mirrored Database/Catalog Behaviors

### Mirrored Catalog
- Requires **tenant feature flag** to be enabled
- Without it, mutations fail (list may still work)

### Mirrored Databricks Catalog
- Creates **without** external connection (unlike other mirrored types)
- Uses `discover-catalogs` to enumerate available Databricks catalogs

### Naming Constraints
- `MirroredAzureDatabricksCatalog`: **no hyphens** allowed in display name
- Standard `MirroredDatabase`: standard naming rules apply

## Apache Airflow Job Behaviors

### File Operations
- All file endpoints require `?beta=true` query parameter
- Without it, returns 404 or unsupported error

### File Upload
- Content-Type: `text/plain` (JSON body is rejected)
- Files are DAGs, not Fabric definitions

### Environment Lifecycle States
```
Initial → Starting → Started → Stopping → Stopped
```
- Start: `POST /workspaces/{ws}/apacheAirflowJobs/{id}/startEnvironment`
- Stop: `POST /workspaces/{ws}/apacheAirflowJobs/{id}/stopEnvironment`
- Get state: `GET /workspaces/{ws}/apacheAirflowJobs/{id}/getEnvironment`

## Power BI REST API Integration

### Single Token for Both APIs
The Fabric token (`https://api.fabric.microsoft.com/.default` scope) is accepted by both `api.fabric.microsoft.com` and `api.powerbi.com`. No separate Power BI scope is needed.

### Power BI API Base URL
`https://api.powerbi.com/v1.0/myorg`. Workspaces are referenced as "groups": `/groups/{workspace-id}/datasets/{dataset-id}`.

### `datasets` = semantic models
The Power BI REST API uses the legacy term "datasets" for what Fabric calls "semantic models". The ID is the same UUID.

### `--api powerbi` flag on `fabio rest call`
Routes requests to the Power BI API instead of Fabric. Dry-run output includes `"api": "powerbi"` field. Env var `FABIO_POWERBI_ENDPOINT` overrides the base URL (for sovereign clouds).

### Semantic Model Power BI Commands
12 subcommands via Power BI REST API:
- `list-parameters`: `GET /groups/{ws}/datasets/{id}/parameters`
- `update-parameters`: `POST /groups/{ws}/datasets/{id}/Default.UpdateParameters`
- `list-datasources`: `GET /groups/{ws}/datasets/{id}/datasources`
- `update-datasources`: `POST /groups/{ws}/datasets/{id}/Default.UpdateDatasources`
- `list-users`: `GET /groups/{ws}/datasets/{id}/users`
- `add-user`: `POST /groups/{ws}/datasets/{id}/users`
- `delete-user`: `DELETE /groups/{ws}/datasets/{id}/users/{user}`
- `refresh-status`: `GET /groups/{ws}/datasets/{id}/refreshes?$top=N`
- `list-upstream`: `GET /groups/{ws}/datasets/{id}/upstreamDatasets`
- `clone`: `POST /groups/{ws}/datasets/{id}/Default.Clone`
- `export-pbix`: `POST /groups/{ws}/datasets/{id}/Default.Export` (binary download)
- `import-pbix`: `POST /groups/{ws}/imports` (multipart/form-data)

### import-pbix nameConflict Values
`Abort` (default), `Overwrite`, `CreateOrOverwrite`, `GenerateUniqueName`

### add-user accessRight Values
`Read`, `ReadWrite`, `ReadWriteReshare`, `ReadWriteReshareExplore`, `ReadExplore`, `ReadReshareExplore`, `ReadWriteExplore`

## Capacity ARM API Lifecycle

### Dual API Design
- Read operations (list/show): Fabric API (`api.fabric.microsoft.com/v1/capacities`)
- Lifecycle operations (suspend/resume/create/update/delete): ARM API (`management.azure.com`)

### ARM API Details
- API version: `2023-11-01`
- Resource path: `/subscriptions/{sub}/resourceGroups/{rg}/providers/Microsoft.Fabric/capacities/{name}`
- Auth scope: `https://management.azure.com/.default` (separate from Fabric scope)
- Requires Azure RBAC (Contributor) on the capacity resource

### Capacity Name Constraints
3-63 chars, pattern `^[a-z][a-z0-9]*$` (lowercase only, starts with letter)

### SKU Values
F2, F4, F8, F16, F32, F64, F128, F256, F512, F1024, F2048 (Fabric tier)

### State Values
`Active`, `Inactive` (paused/suspended), `Provisioning`, `Deleting`

## REST Passthrough Command

### Usage
```bash
fabio rest call --method GET --path "/workspaces/{ws}/items" [--body <json|@file|@->] [--query-params "key=value"] [--poll] [--api <fabric|powerbi>]
```

### Body Resolution
- Inline JSON: `--body '{"key": "value"}'`
- From file: `--body @path/to/file.json`
- From stdin: `--body @-`

### `--poll` flag
Enables LRO polling on the response (follows Location header)

### `--api powerbi`
Routes to `https://api.powerbi.com/v1.0/myorg` instead of Fabric base URL

## RTI (Real-Time Intelligence) NL-to-KQL

### Endpoint
`POST /workspaces/{ws}/realTimeIntelligence/nltokql?beta=true`

### Request Body (Required Fields)
```json
{
  "itemIdForBilling": "<kql-database-or-eventhouse-id>",
  "clusterUrl": "<kusto-uri>",
  "databaseName": "<db-name>",
  "naturalLanguage": "<question>"
}
```

### Optional Fields
- `userShots`: Array of `{"naturalLanguage":"...","kqlQuery":"..."}` examples
- `chatMessages`: Array of `{"role":"User|Assistant","content":"..."}` for multi-turn

### Response
Returns JSON with `kqlQuery` field containing the generated KQL, plus `explanation` and metadata.

## CSV/TSV Output Format

All commands support `--output csv` and `--output tsv`. RFC 4180 quoting for CSV. Useful for piping to spreadsheet tools or data pipelines.

## Hard Delete

### `--hard-delete` on All Item Deletes
38 item type delete commands support `--hard-delete` flag to permanently delete (skip recycle bin). Appends `?hardDelete=true` to URL.

Non-item deletes (connection, deployment-pipeline, domain, gateway, managed-private-endpoint, onelake-security, profile, workspace) do NOT have this flag.

## Error `isRetriable` Field

API responses may include `error.isRetriable: bool`. When present, serialized in the structured error output as `"retriable": true/false`. Useful for agent retry logic.

## Item Exists/URL/Inspect

### `item exists`
Returns `{"exists": true/false}` — never errors on 404 (unlike `item show`).

### `item url`
Returns the Fabric portal URL for the item.

### `item inspect`
Aggregates metadata + definition + connections in a single response (reduces API calls).

## Notebook `--strip-output`

`get-definition --strip-output` clears `outputs` and `execution_count` from ipynb cells. Gracefully passes through `.py` format notebooks. Useful for git-friendly exports.

## Notebook Run with Parameters

```bash
fabio notebook run --workspace $WS --id $NB --wait \
  --parameters '[{"name":"p1","value":"v1","type":"Text"}]' \
  --compute-type "Spark"
```

Parameter type values: `Text`, `Int`, `Long`, `Double`, `Bool`, `DateTime`

`--execution-data` and `--parameters` accept `@file.json` (read from file) and `@-` (read from stdin), matching the `fabio rest call --body` convention:

```bash
fabio notebook run --workspace $WS --id $NB --execution-data @params.json
echo '{"type":"Full"}' | fabio notebook run --workspace $WS --id $NB --execution-data @-
```

## Profile-Aware Defaults (FABIO_WORKSPACE, FABIO_OUTPUT, and FABIO_CAPACITY)

When a profile is active (`fabio profile use --name <name>`), its `workspace`, `output`, and `capacity` defaults are injected as environment variable fallbacks for all commands.

**Precedence (highest to lowest):**
1. Explicit CLI flag (`--workspace`, `--output`, `--capacity`)
2. External environment variable (`FABIO_WORKSPACE`, `FABIO_OUTPUT`, `FABIO_CAPACITY`)
3. Active profile default

This means setting `FABIO_WORKSPACE` in the shell always overrides the active profile default.

**`--profile` flag** overrides the active profile for a single invocation — corrected in v0.25.0 to properly override workspace, output, AND capacity (previously only affected private link routing):

```bash
# Use prod defaults for one command while dev profile is active
fabio lakehouse list --profile prod
```

**`profile save` merges with existing profile** (v0.25.0+): Omitted fields preserve their current values. Previously, omitting a field would clear it to null.

```bash
# Save profile with workspace default
fabio profile save --name dev --workspace $DEV_WS --default-output json

# Add capacity to existing dev profile without clearing other fields
fabio profile save --name dev --capacity $CAP_ID

# Activate profile — subsequent commands use $DEV_WS as workspace default
fabio profile use --name dev

# Override for a single command via env var
FABIO_WORKSPACE=$PROD_WS fabio lakehouse list
```

**`FABIO_CAPACITY` from profiles** (v0.25.0+): The `capacity` field in a profile is injected as `FABIO_CAPACITY` and wired to `workspace assign-capacity` and `gateway create` commands.

**`--private-link-workspace`** flag on `profile save` (v0.25.0+): Configures private link routing without manual JSON editing. When set, `FabricClient` transforms all API URLs to use private link endpoints:
- `https://api.fabric.microsoft.com/v1/...` → `https://<ws-id>-api.privatelink.analysis.windows.net/v1/...`
- `https://onelake.dfs.fabric.microsoft.com/...` → `https://<ws-id>-onelake.dfs.fabric.microsoft.com/...`
- `https://onelake.blob.fabric.microsoft.com/...` → `https://<ws-id>-onelake.blob.fabric.microsoft.com/...`

## Deploy Validate

Local-only pre-flight checks on source directory (validates .platform files, item types, definition structure, logical ID references). No API calls required.

```bash
fabio deploy validate --source ./fabric-items
```

## Private Link URL Routing

When `private_link_workspace` is configured via profile, URLs are transformed for private network access. Use `fabio profile save --name private --private-link-workspace <ws-id>` to configure.

## Dataflow Execute Query

`POST /workspaces/{ws}/dataflows/{id}/executeQuery` returns binary Apache Arrow IPC stream (NOT JSON). Save with `--file` flag. Requires Contributor role.

### LRO Support (v0.30.0+)
For long-running queries, the endpoint returns **202 Accepted** and polls until completion (up to 90s server-side). Use the global `--lro-timeout` flag (default 120s) to control the polling timeout.

### Arrow Version Selection
Use `--arrow-version 1|2` to select the Apache Arrow IPC format version (default: 1). Sets `Accept: application/vnd.apache.arrow.stream;pq-arrow-version=<N>` header. Arrow v2 is required for newer Dataflow versions.

## App Backend (preview)

- **Endpoint pattern**: `/workspaces/{ws}/appBackends` and `/workspaces/{ws}/appBackends/{id}`
- **Create is LRO**: `POST /workspaces/{ws}/appBackends` returns 202 and is polled to completion
- **Hard delete**: `--hard-delete` appends `?hardDelete=true` to permanently delete (skip recycle bin)
- **Update requires at least one field**: `--name` or `--description` is mandatory; omitting both returns `INVALID_INPUT`
- **context coverage**: `fabio context agent` includes full `app-backend` schema with `--hard-delete` flag typed as bool

## Enhanced Error Output (v0.20.0+)

Structured error responses now include additional diagnostic fields from the Fabric API:
```json
{
  "error": {
    "code": "API_ERROR",
    "message": "...",
    "requestId": "abc-123",
    "moreDetails": [{"code": "Inner_Error", "message": "..."}],
    "relatedResource": {"resourceId": "<uuid>", "resourceType": "Lakehouse"}
  }
}
```
- `requestId` — Server-assigned correlation ID (include in support tickets)
- `moreDetails` — Array of nested error codes; may provide the root cause
- `relatedResource` — Resource involved in the error (absent when not applicable)
- All extra fields are omitted when null — backward compatible with older responses

## Pagination: continuationUri Preference

When a list response contains both `continuationToken` and `continuationUri`, fabio prefers `continuationUri` (the full server-provided URL) over constructing the next URL from the token. This improves reliability with APIs that use opaque URLs. Falls back to token-based construction if URI is absent.

## Lakehouse Sync: rsync-Inspired Flags

The `lakehouse sync` command supports rsync-inspired filtering and control flags:

### Filtering
- `--include <patterns>` — Semicolon-separated glob patterns; only matching files are synced (matches filename and full relative path)
- `--exclude <patterns>` — Semicolon-separated glob patterns; matching files are skipped (applied after include)
- `--min-size <size>` / `--max-size <size>` — Filter by file size; supports K/M/G suffixes (e.g., `--min-size 1K`, `--max-size 100M`)
- `--no-recursive` — Sync only top-level files (skip subdirectories)

### Copy Modes
- `--size-only` — Compare files by size only (skip ETag/checksum comparison); output `"strategy": "size-only"`
- `--no-overwrite` — Only copy files not present at destination; output `"strategy": "no-overwrite"`
- `--force` — Mirror mode: copy all source files regardless of content match; output `"strategy": "force"`
- `--existing` — Only update files already present at destination (don't create new files)

### Safety & Observability
- `--max-delete=N` — Skip ALL deletions if the count would exceed N; output includes `"deletionsSkipped": true`
- `--remove-source-files` — Delete source files after successful transfer (move semantics); output includes `"sourceRemoved": N`
- `--itemize` — Output per-file actions on stderr: `[copy]`, `[rename]`, `[delete]`, `[skip]`

### Server-Side Dedup
When copying, sync checks if any existing destination file has the same content hash. If so, performs a same-lakehouse copy (faster). Output includes `"dedupCopied"` count.

## Lakehouse Sync: Local-to-Remote (--local)

`--local <dir>` syncs a local directory to a remote lakehouse path. Mutually exclusive with `--source-workspace`/`--source-id`/`--source-path`.

```bash
fabio lakehouse sync --local ./data \
  --dest-workspace $WS --dest-id $LH --dest-path Files/data
```

- Default comparison: size (local files have no ETags)
- `--checksum`: computes local MD5, compares with remote Content-MD5 via HEAD
- Parallel uploads via DFS (create+append+flush with MD5 stored for future comparisons)
- All filtering flags work: `--include`, `--exclude`, `--min-size`, `--max-size`, `--no-recursive`
- Rename detection and server-side dedup are skipped (not applicable for local sources)
- `--remove-source-files` deletes local files after successful upload (move semantics)

**Use as superset of upload**: `fabio lakehouse sync --local ./dir --force` is equivalent to `fabio lakehouse upload` but with structured output and all sync flags.

## Copy Job Reset

```bash
# Reset all entities for re-processing
fabio copy-job reset --workspace $WS --id $ID --all

# Reset specific entities
fabio copy-job reset --workspace $WS --id $ID --entity-ids "uuid1,uuid2"
```
- `--all` and `--entity-ids` are mutually exclusive; omitting both returns `INVALID_INPUT`
- Endpoint: `POST /workspaces/{ws}/copyJobs/{id}/resetCopyJob`
- No LRO — returns immediately

## Gateway Lifecycle Commands

VNet gateway lifecycle management (requires gateway Admin role):
- **check-status**: `GET /gateways/{id}/checkStatus` — returns connectivity status
- **check-member-status**: `GET /gateways/{id}/members/{memberId}/checkStatus` — individual member (on-premises)
- **restart**: `POST /gateways/{id}/restart` with empty body; LRO
- **shutdown**: `POST /gateways/{id}/shutdown` with empty body; LRO

Both `restart` and `shutdown` use LRO polling and require the caller to have gateway Admin role.

## Data Build Tool Job (preview)

- **Item type**: `DataBuildToolJob` — dbt (Data Build Tool) integration
- **Endpoint pattern**: `/workspaces/{ws}/dataBuildToolJobs/{id}`
- **Run endpoint**: `POST /workspaces/{ws}/dataBuildToolJobs/{id}/jobs/execute/instances` (item-specific path, NOT the generic `/jobs/instances`)
- **Run supports `--wait`/`--timeout`/`--cancel-on-timeout`**: Same polling pattern as notebook run (5s interval, default 600s timeout)
- **Create/getDefinition/updateDefinition are all LRO**: Standard 202 + polling pattern
- **In deploy ordering**: Positioned after CopyJob in `DEPLOY_ORDER` (45 total types)

## OrgApp & OrgAppAudience

- **OrgApp** (`org-app`): Organizational App — published app packages for workspace content distribution
  - Endpoint: `/workspaces/{ws}/orgApps/{id}`
  - Full CRUD + get-definition/update-definition; all LRO
- **OrgAppAudience** (`org-app-audience`): Audience targeting for Organizational Apps
  - Endpoint: `/workspaces/{ws}/orgAppAudiences/{id}`
  - Full CRUD + get-definition/update-definition; all LRO
- Both added to `DEPLOY_ORDER` (45 total types — positioned after visualization items)

## Deploy: Lakehouse Shortcut Reconciliation

The deploy engine now reconciles lakehouse shortcuts as a post-deploy hook:
- Reads `shortcuts.metadata.json` from Lakehouse source directories (if present)
- Lists currently deployed shortcuts via `GET /items/{id}/shortcuts`
- Deletes orphan shortcuts (deployed but not in source definition)
- Creates/overwrites shortcuts from the source definition (CreateOrOverwrite policy)
- Shortcut failures are non-fatal (reported in `post_hooks` output; controlled by `--no-post-hooks`)

## Deploy: fabric-cicd Compatibility (v0.22.0)

fabio deploy is now a strict superset of Microsoft's [fabric-cicd](https://github.com/microsoft/fabric-cicd) Python library. Source directories exported by fabric-cicd or Fabric's git integration work identically with fabio.

### `.platform` in parts but excluded from content hash

**Symptom**: Re-planning already-deployed items always shows "update" instead of "skip".
**Cause**: The Fabric API modifies the `logicalId` field in `.platform` when returning definitions via `getDefinition` (resets to `00000000-...`). Including `.platform` in the content hash causes the source hash to never match the deployed hash.
**Fix**: `.platform` IS sent as a definition part (enables `?updateMetadata=true` for metadata propagation), but EXCLUDED from content hash computation.

### `.children/` KQL Database discovery

Fabric's git integration stores KQL databases under `Eventhouse/.children/` subdirectories. fabio now recurses into `.children/` directories within item directories to discover child items. This is transparent — items are deployed as independent Fabric items.

### `.pbi/` directory exclusion

Report, SemanticModel, and DataAgent items in PBIP format include a `.pbi/` subdirectory containing local metadata (`cache.abf`, `localSettings.json`). These are NOT part of the Fabric definition. fabio excludes `.pbi/` from definition parts automatically.

### Report `byPath` → `byConnection` transform

**Symptom**: API rejects PBIP-format reports with `byPath` semantic model reference.
**Cause**: The Fabric REST API requires `byConnection` format; `byPath` is a local filesystem reference used by Power BI Desktop.
**Fix**: fabio automatically converts `byPath` to `byConnection` by resolving the semantic model's logical ID from its `.platform` file and looking up the deployed GUID.

### Notebook part ordering (.py before .json)

The Fabric API processes Notebook definition parts in order. Content files (`.py`, `.ipynb`) must appear before settings files (`.json`). fabio sorts parts at deploy time automatically.

### `ItemDisplayNameNotAvailableYet` retry

**Symptom**: Creating an item returns HTTP 400 with error code `ItemDisplayNameNotAvailableYet`.
**Cause**: After deletion, an item's name may be reserved in the recycle bin for up to 5 minutes.
**Fix**: fabio retries up to 10 times with 30-second delays (~5 minutes total), matching fabric-cicd behavior. Critical for CI/CD pipelines that delete and recreate items with the same name.

### `SparkJobDefinitionV2` format auto-detection

When `.platform` does not specify `definitionFormat`, SparkJobDefinition items automatically use `"SparkJobDefinitionV2"` format. This matches fabric-cicd's `API_FORMAT_MAPPING` behavior.

### `creationPayload` from `.platform` metadata

fabric-cicd stores `creationPayload` inside `.platform`'s `metadata.creationPayload` field (not a separate file). fabio reads this as a fallback when no standalone `creationPayload.json` exists.

### Lakehouse `enableSchemas` inference

**Symptom**: Multi-schema lakehouses fail to create when deploying from fabric-cicd source directories.
**Cause**: fabric-cicd detects multi-schema lakehouses by checking for `defaultSchema` in `lakehouse.metadata.json` and adds `enableSchemas: true` to `creationPayload`.
**Fix**: fabio now performs the same detection automatically.

### Workspace ID placeholder replacement

`00000000-0000-0000-0000-000000000000` in definition payloads is auto-replaced with the target workspace UUID. Uses regex matching on workspace-reference keys (`workspaceId`, `default_lakehouse_workspace_id`, `workspace`) — NOT blanket string replacement (prevents corrupting `itemId` or other GUID fields). Shortcuts are excluded (handled separately with lakehouse GUID). Opt-out: `--no-workspace-id-replace`.

### Shortcut self-reference

When a shortcut's `target.oneLake.itemId` is `00000000-...`, it means "this lakehouse itself" (self-referencing shortcut). fabio replaces this with the lakehouse's own deployed GUID — NOT the workspace ID. The workspace ID replacement step skips shortcuts entirely.

### Binary file graceful handling

Non-UTF-8 payloads (images, compiled assets) are silently skipped during parameter substitution and reference validation. Previously this caused "Non-UTF8 content" errors.

## Deploy: Config File (v0.22.0)

`--config <file> --env <name>` loads a JSON or YAML config file with per-environment settings:

```yaml
# deploy.yaml
environments:
  dev:
    workspace: dev-workspace-name
    source: ./fabric-items
  prod:
    workspace: prod-workspace-id
    source: ./fabric-items
    parameters: parameters.json
    options:
      delete_orphans: false
      concurrency: 4
```

```bash
fabio deploy apply --config deploy.yaml --env prod
```

- CLI flags always override config file values
- `--env` is required when `--config` is specified
- Supports both JSON and YAML formats (`serde_yaml`)

## Deploy: Git-Diff Selective Deploy (v0.22.0)

`--git-diff <REF>` limits deployment to items changed since a git reference:

```bash
# Only deploy items changed since main branch
fabio deploy plan --source ./fabric-items --workspace $WS --git-diff main

# Only deploy items changed since a specific commit
fabio deploy apply --source ./fabric-items --workspace $WS --git-diff abc1234
```

Uses `git diff --name-status <REF>` to determine changed item directories. Items with no changed files are skipped at the plan stage (treated as `Skip`). Deleted items still appear as `Delete` if `--delete-orphans` is set.

## Context Tenant (renamed from Context Extract)

- **`fabio context tenant` is the renamed `context extract`**: All flags and behavior are unchanged — only the subcommand name changed (`extract` → `tenant`) to better reflect that this extracts workspace graph data from the live tenant.
- **Three-layer relationship discovery**: Layer 1 (properties) finds typed edges from item GET responses. Layer 2 (`--deep`) decodes base64 definition payloads and regex-scans for UUID references. Layer 3 (`--include-connections`) fetches `/items/{id}/connections`. Each layer is additive — deeper layers find significantly more edges. Properties-only found 2 edges in a 154-item tenant; deep mode found 88.
- **Items without definition support skipped in deep mode**: SQLEndpoint, Dashboard, Datamart, MLModel, MLExperiment never support `getDefinition`. Skipping them avoids wasted LRO calls. (PaginatedReport DOES support `getDefinition` and is scanned — its `.rdl` embeds a semantic-model UUID.)
- **GUID scanning finds all cross-references generically**: Builds a registry of known item/workspace IDs, then regex-matches `[0-9a-fA-F]{8}-...-[0-9a-fA-F]{12}` in decoded definitions. Excludes well-known placeholder GUIDs (all-zeros, all-`f`s, near-zero).
- **`bulkExportDefinitions` API format**: `POST /workspaces/{ws}/items/bulkExportDefinitions?beta=True` with `{"mode":"All"}`. Requires `?beta=True` query param. Only exports items the caller has **read+write** permissions for — silently excludes items with Viewer/Contributor role or protected labels. Benchmarked: bulk exported 14/154 items (55 edges) vs per-item 35/154 (88 edges). Per-item `getDefinition` is preferred for context tenant because completeness matters more than speed.
- **`--no-properties` mode**: Skips type-specific GET calls, only calls `GET /workspaces/{ws}/items` (listing). Nodes lack `properties` field. Ultra-fast (~3s for 20 workspaces). Useful for initial orientation.
- **`--output-file` writes the JSON envelope**: Writes `{"data": {...}}` (pretty-printed) to disk. Reports `{"status":"written","file":"...","nodes":N,"edges":N,"workspaces":N}` to stdout. Parent directories must exist.
- **`--merge` is idempotent**: Loads an existing graph, unions new nodes/edges. Merge semantics: nodes deduped by ID (new overwrites old), edges unioned (exact-match dedup), workspaces deduped by ID. Re-extracting the same workspace updates it in place.
- **`--format jsonld` produces valid RDF**: JSON-LD with `@context` vocabulary (`https://api.fabric.microsoft.com/ontology/`) and `@graph` array. Items become `urn:fabric:item:{uuid}` resources typed as `fabric:{ItemType}`. Edges inlined as typed properties (e.g., `fabric:defaultLakehouse`). Multiple edges of same type become JSON arrays. Compatible with Neptune, Stardog, Jena, and any SPARQL endpoint.
- **Performance benchmarks (20 workspaces, 154 items)**: Shallow: 7.7s, 2 edges. Deep + connections: 4m18s, 88 edges. No-properties: ~3s, 0 edges. LRO polling is the deep mode bottleneck (2-6s per `getDefinition` call, 8 concurrent).
- **Concurrency auto-scales to CPU count** (v0.25.0+): Default concurrency is `min(cpus * 4, 16)` instead of a hardcoded 8. I/O-bound workload (HTTP round-trips) benefits from higher concurrency. On a 4-core machine, default is now 16, approximately halving deep-mode time. Override with `--concurrency <N>`.

## KQL Database Intelligence (v0.28.0)

- **Schema-as-JSON format uses DB GUID key**: `.show database schema as json` returns `{"Databases":{"<db-guid>":{...}}}` — the key is the database's GUID, not the display name. fabio resolves this automatically.
- **`list-entities` calls `.show database entities`**: Returns tables, views, materialized views, external tables, and functions. Filter with `--entity-type Table|View|MaterializedView|ExternalTable|Function`.
- **`describe` returns flat row-per-column format**: Each row has `TableName`, `ColumnName`, `DataType`. Output includes all entities in the database.
- **`describe-entity` calls `.show entity <name> schema as json`**: Returns full column schema for a single named entity.
- **`sample` appends `| take <N>` to the entity query**: Default 5 rows. Works on any entity type (tables, views, functions with tabular output).
- **`ingest` uses management endpoint**: Inline CSV ingestion calls `POST /v1/rest/mgmt` with `.ingest inline into table <name> <| <data>` syntax. Limit ~4MB of inline data. Not suitable for large datasets.
- **`show-queryplan` uses management endpoint**: Calls `.show queryplan <kql>` via `/v1/rest/mgmt`. Returns operator tree — useful for diagnosing slow queries without executing them.
- **`diagnostics` may fail per-section**: Aggregates `.show capacity`, `.show cluster`, and `.show diagnostics` as separate calls. If any section fails (e.g., capacity API not available on Basic tier), the rest still succeed. Check `status` per section.
- **`deeplink` auto-detects portal vs ADX**: If KQL URI contains `.kusto.fabric.microsoft.com`, generates Fabric portal URL (`https://app.fabric.microsoft.com/...`). If it contains `.kusto.windows.net`, generates ADX Web Explorer URL (`https://dataexplorer.azure.com/...`). The KQL is URL-encoded and embedded in the link.

## Reflex Create-Trigger (v0.28.0)

- **Auto-generates 5 entities**: `create-trigger` builds the full ReflexEntities.json hierarchy: container → event → object → attribute → rule. This eliminates manual JSON authoring.
- **Graceful failure if definition push fails**: After creating the Reflex item, if `update-definition` fails (known limitation when using KQL source type), the command still succeeds and reports the created item ID with a hint to use `fabio reflex update-definition` manually.
- **`--action email|teams`**: Only two action types supported. `email` sends to `--recipients` addresses. `teams` posts to the Teams channel webhook in `--recipients`.
- **`--interval` defaults to 60 seconds**: Sets the polling interval for condition evaluation.

## Azure Databricks Storage API Behaviors

- **Item type**: `AzureDatabricksStorage` — Fabric item type for Azure Databricks integration.
- **Definition format**: `AzureDatabricksStorageV1`. Definition file path is `definition.json` (not `AzureDatabricksStorage.json` — corrected from initial implementation per API spec examples).
- **Create is LRO**: `POST /workspaces/{ws}/azureDatabricksStorages` returns 202, requires polling.
- **getDefinition is LRO**: Returns 202, polled to completion. Returns `definition.json` + `.platform` parts.
- **updateDefinition is LRO**: Returns 202, polled to completion.
- **Standard CRUD endpoints**: `/workspaces/{ws}/azureDatabricksStorages/{id}`.
- **`--hard-delete` supported**: `DELETE .../azureDatabricksStorages/{id}?hardDelete=true` permanently removes (skips recycle bin).
- **Registered in DEPLOY_ORDER**: AzureDatabricksStorage is included in the 46-item deploy ordering for CI/CD pipelines. DEPLOY_ORDER now contains 46 item types (up from 45).

## Data Pipeline Schedule and Instance Management

- **Schedule endpoints use `/jobs/execute/schedules`**: `POST /workspaces/{ws}/dataPipelines/{id}/jobs/execute/schedules` to create. Note: uses `/jobs/execute/schedules` (NOT `/jobs/Pipeline/schedules`).
- **list-schedules**: `GET /workspaces/{ws}/dataPipelines/{id}/jobs/execute/schedules` — paginated list of pipeline schedules.
- **get-schedule**: `GET /workspaces/{ws}/dataPipelines/{id}/jobs/execute/schedules/{schedule_id}`.
- **update-schedule**: `PATCH /workspaces/{ws}/dataPipelines/{id}/jobs/execute/schedules/{schedule_id}` with `--content` JSON body. Requires at least `--content` or `--file`. Supports `--dry-run`.
- **delete-schedule**: `DELETE /workspaces/{ws}/dataPipelines/{id}/jobs/execute/schedules/{schedule_id}`.
- **list-instances**: `GET /workspaces/{ws}/dataPipelines/{id}/jobs/execute/instances` — paginated job execution history.
- **get-instance**: `GET /workspaces/{ws}/dataPipelines/{id}/jobs/execute/instances/{instance_id}` — details of a specific pipeline run.
- **Requires Contributor role**: All schedule and instance management operations require Contributor role on the workspace.
- **Distinct from job-scheduler**: `data-pipeline list-schedules` is pipeline-specific via the `/dataPipelines/` endpoint. The generic `job-scheduler` command works with any item type via `/items/{id}/jobs/{jobType}/schedules`.
