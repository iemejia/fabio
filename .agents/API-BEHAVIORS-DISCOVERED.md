# API Behaviors Discovered
> Extracted from AGENTS.md to reduce context size for coding agents.
> This file contains runtime behaviors, quirks, and undocumented API details
> discovered during fabio development. Reference this file when working on
> specific command groups — do NOT load the entire file into context.

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
- DFS rename (`x-ms-rename-source`): SUPPORTED within same item (returns 201). Works for files AND directories. Fails with 403 for cross-item/cross-workspace. Requires `x-ms-version: 2021-06-08` header.
- DFS recursive delete (`?recursive=true`): works for directories
- DFS listing with `directory` param on a table path shows virtual lakehouse structure (not real files)
- Root listing (no `directory` param): returns real paths prefixed with item ID
- Table files live at `Tables/{name}/_delta_log/` and `Tables/{name}/*.parquet`
- **DFS directory parameter "virtual lakehouse-in-lakehouse" view**: When `directory=X` is specified, the API returns ALL paths prefixed with `X/`, where top-level lakehouse dirs appear doubled (e.g., `Files/Files/myfile.csv` for a file at `Files/myfile.csv`). With `recursive=false`, only immediate virtual children show. Fix: always use `recursive=true` and strip the doubled prefix client-side.
- **DFS upload Content-MD5**: Including `x-ms-content-md5` header on the flush call (Step 3) stores the MD5 as a file property. OneLake does NOT compute hashes server-side — the client must provide the hash. Without it, `Content-MD5` is absent from HEAD responses.
- **Content-MD5 preserved on DFS rename**: When `x-ms-content-md5` was set at upload time, DFS rename preserves both the `Content-MD5` property AND the `ETag`. OneLake treats files with stored MD5 as having "sealed" content — rename is a pure path operation.
- **Content-MD5 preserved on server-side blob copy**: The `Content-MD5` property (if set) is preserved when a file is copied via Blob API `x-ms-copy-source`, including cross-lakehouse/cross-workspace copies.
- **ETag format**: ETags in OneLake are .NET DateTime ticks (100-nanosecond intervals since 0001-01-01) encoded in hex (e.g., `"0x8DEC5A604A12DD4"`). They represent the last-modified timestamp, NOT a content hash.
- **ETag behavior on DFS rename**: Without `x-ms-content-md5` stored, DFS rename generates a new ETag (new modification timestamp). With `x-ms-content-md5` stored, the ETag is preserved (file treated as immutable content).
- **ETag preserved on server-side blob copy**: Blob API copy preserves the source file's ETag at the destination.
- **x-ms-content-crc64**: Always returns `AAAAAAAAAAA=` (all zeros) in HEAD responses. The field exists but OneLake does not compute CRC64 checksums.
- **Fabric-generated files lack content hashes**: Files written by Spark, data pipelines, and load-table operations (via Hadoop ABFS driver) do NOT include `x-ms-content-md5` on flush. These files have no Content-MD5 in HEAD responses and their ETags change on rename.
- **DFS listing fields**: Returns `name`, `contentLength`, `etag`, `lastModified`, `creationTime` (Windows FILETIME ticks), `owner`, `group`, `permissions`, `expiryTime`. Does NOT include Content-MD5 — requires per-file HEAD requests.
- **Notebook Jobs API**: `POST /workspaces/{ws}/items/{id}/jobs/instances?jobType=RunNotebook` returns 202 + Location header with job instance URL. Status endpoint returns `NotStarted`, `InProgress`, `Completed`, `Failed`, `Cancelled`. Cancel via `POST .../cancel`.
- **Spark cold start on small capacity**: First notebook run can take 2-5 minutes to transition from `NotStarted` to `InProgress` due to Spark session allocation.
- **OneLake Table API (Iceberg REST Catalog)**: Available at `https://onelake.table.fabric.microsoft.com/iceberg/v1/...`. Uses storage-scoped auth (`https://storage.azure.com/.default`). Standard Apache Iceberg REST Catalog v1 protocol.
- **Table API warehouse identifier**: `{workspaceId}/{itemId}` (both URI-encoded). Used in URL path segments and as `?warehouse=` query parameter.
- **Table API config response**: Returns `endpoints` array listing available operations (13 endpoints including CRUD for tables and namespaces), plus `overrides.prefix` matching the workspace/item path.
- **Table API namespaces**: Standard lakehouses expose a single `dbo` namespace. Multi-schema lakehouses may expose additional namespaces. Response: `{"namespaces": [["dbo"]], "next-page-token": null}`.
- **Table API namespace properties**: Each namespace has a `location` property pointing to the OneLake storage path (e.g., `{wsId}/{itemId}/Tables/dbo`).
- **Table API table listing**: Response: `{"identifiers": [{"name": "tableName", "namespace": ["dbo"]}], "next-page-token": null}`. Lists all Delta tables that OneLake exposes as Iceberg.
- **Table API table metadata**: Returns full Apache Iceberg `TableMetadata` (format-version 2): `schemas` (full column definitions with id/name/type/required), `partition-specs`, `sort-orders`, `snapshots` (with manifest lists), `properties` (compression codec, write paths), `metadata-location` (abfss:// path to metadata JSON).
- **Delta-to-Iceberg via UniForm/XTable**: Table properties include `XTABLE_METADATA` with `sourceTableFormat: "DELTA"`, confirming Delta tables are exposed as Iceberg via Microsoft's XTable (formerly OneTable) integration. The `iceberg-version` in snapshot summary shows `Apache Iceberg 1.10.1`.
- **Table API is read-only for now**: The config endpoint lists POST/DELETE endpoints in `endpoints` array, but write operations may not be available in all tenants (preview feature). Read operations (GET) work universally.
- **Table API env override**: `FABIO_ONELAKE_TABLE_ENDPOINT` overrides the base URL (for sovereign clouds or testing environments).
- **Table API HEAD for existence checks**: `HEAD /iceberg/v1/{prefix}/namespaces/{ns}` and `HEAD .../tables/{table}` return 204 (exists) or 404 (not found). No response body. Lightweight alternative to GET.
- **Table API credentials endpoint**: `GET /iceberg/v1/{prefix}/namespaces/{ns}/tables/{table}/credentials` returns vended storage credentials scoped to a specific table's location. Enables external tools (DuckDB, Polars) to read table data directly.
- **Table API snapshot summary fields**: Each snapshot's `summary` object contains: `operation` (append/overwrite/delete), `added-records`, `total-records`, `added-data-files`, `total-data-files`, `total-files-size`, `iceberg-version`. These enable client-side stats extraction without additional API calls.

## Data Agent API Behaviors Discovered
- **Public staging management API (Jun 2026)**: The Fabric REST API now exposes 31 dedicated endpoints for data agent configuration management at `/workspaces/{ws}/dataAgents/{id}/staging/...`. This eliminates the need for the previous `getDefinition`/`updateDefinition` read-modify-write approach for management operations.
- **Two-stage model (staging/published)**: All configuration changes go to staging (draft). `POST .../staging/publish` promotes to production. `POST .../staging/reset` reverts staging to published state. Read commands accept `--stage staging|published` to inspect either state.
- **Staging Settings**: `GET/PATCH .../staging/settings` manages `aiInstructions` field. Published settings at `GET .../settings` (official, no longer V3-experimental).
- **Staging Datasources**: Full CRUD at `.../staging/datasources`. `POST` is LRO (triggers async schema discovery, 1-5 minutes). `PATCH` updates `instructions`/`description`. Datasource types: `FabricItem` (generic + `fabricItemType`) or `LakehouseTables`.
- **Supported fabricItemType values**: `Report`, `SemanticModel`, `Lakehouse`, `KQLDatabase`, `Warehouse`, `MirroredDatabase`, `MirroredAzureDatabricksCatalog`, `GraphModel`, `SQLDatabase`, `Ontology`.
- **Staging Elements**: `GET .../staging/datasources/{dsId}/elements` returns schema tree level-by-level via `?rootId=` parameter. `PATCH ...?id={elemId}` updates `isSelected`/`description`. `DELETE ...?id={elemId}` removes stale elements.
- **Element types**: `Root`, `Files`, `Directory`, `Schemas`, `Tables`, `Views`, `Functions`, `Schema`, `Table`, `ExternalTable`, `MaterializedView`, `View`, `Column`, `Measure`, `Function`, `NodeType`, `EdgeType`, `Entity`.
- **Element states**: `Available`, `NotAvailable`, `AccessDenied`, `AccessDeniedOap`, `DatasourceNotFound`, `SchemaUnavailable`.
- **Element index states**: `Indexed`, `Indexing`, `NotIndexed`.
- **Staging Fewshots**: Full CRUD at `.../staging/datasources/{dsId}/fewshots`. `POST .../fewshots/deleteAll` for bulk clear. Server-side validation returns `validationStatus` (`Validating`, `Valid`, `Invalid` + `reason`). NOT supported for SemanticModel/Ontology datasources.
- **Datasource creation is LRO**: `POST .../staging/datasources` returns 202 and triggers async schema discovery. Can take 1-5 minutes on cold lakehouses. Use `--lro-timeout 300` for reliable completion.
- **Published URL resolution**: `GET /workspaces/{ws}/dataAgents/{id}/settings` (now official public endpoint) returns `publishedUrl`. Fallback: check `properties.publishedUrl` in item GET response.
- **Query protocol**: OpenAI Assistants API at the published URL (`{publishedUrl}/assistants`, `/threads`, `/messages`, `/runs`). Uses `?api-version=2024-05-01-preview`. Standard Fabric bearer token for auth.
- **M365 Copilot Agent Store publishing**: NOT available via public REST API. Only accessible through Fabric portal or `fabric-data-agent-sdk` Python package (internal workload endpoint).
- **Datasource ID resolution**: The staging API uses its own UUID for datasources. fabio resolves by matching `displayName`, datasource `id`, or `itemReference.itemId` (artifact ID) — all three work as `--datasource` input.
- **Schema discovery is asynchronous**: After `add-datasource`, schema elements may be empty for 1-5 minutes. The `list-elements` command will show elements once indexing completes (`indexState: "Indexed"`).
- **New scopes**: `DataAgent.Read.All` and `DataAgent.ReadWrite.All` (in addition to generic `Item.*` scopes).
- **Max 5 datasources per agent**: Official limit.
- **Max 100 fewshot examples per datasource**: Official limit.
- **Response cap**: Agent responses are capped at 25 rows and 25 columns maximum.

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
- **SHOWPLAN_XML via TDS**: `SET SHOWPLAN_XML ON` followed by the query returns the estimated execution plan as an XML result set instead of executing the query. Works on Warehouse, Lakehouse SQL Endpoint, and SQL Database. Safe for DDL/DML (never executed). Must execute `SET SHOWPLAN_XML ON` as a separate batch before the query, then `SET SHOWPLAN_XML OFF` afterward for cleanup.
- **sys.dm_exec_requests columns on Fabric**: Does NOT have `login_name` column — that's in `sys.dm_exec_sessions`. Available columns include: `session_id`, `status`, `command`, `start_time`, `total_elapsed_time`, `sql_handle`, `plan_handle`. Filter `WHERE status != 'background'` to exclude internal TASK MANAGER sessions.
- **queryinsights schema views**: `queryinsights.frequently_run_queries` (columns: `last_run_command`, `number_of_runs`, `avg_total_elapsed_time_ms`, `min_run_total_elapsed_time_ms`, `max_run_total_elapsed_time_ms`, `number_of_successful_runs`, `query_hash`), `queryinsights.long_running_queries` (columns: `last_run_command`, `number_of_runs`, `median_total_elapsed_time_ms`, `last_run_total_elapsed_time_ms`, `last_run_start_time`, `query_hash`), `queryinsights.exec_requests_history` (columns: `command`, `status`, `total_elapsed_time_ms`, `login_name`, `start_time`, `end_time`, `row_count`, `query_hash`). All views work on both Warehouse and Lakehouse SQL Endpoints.
- **sys.dm_db_stats_properties NOT supported on Lakehouse SQL Endpoints**: The DMV `sys.dm_db_stats_properties` is NOT available on Lakehouse SQL Endpoints (fails with "DMV not supported"). Use `sys.stats` + `sys.stats_columns` + `sys.tables` for statistics listing instead (works on both surfaces).
- **DBCC SHOW_STATISTICS requires two-argument form**: Syntax is `DBCC SHOW_STATISTICS (table_name, statistics_name)` — the first argument must be the table name (not the statistic name alone). Use a `sys.stats` lookup to resolve the owning table from the statistic name. Returns three result sets (header, density vector, histogram) — TDS returns the first result set (STAT_HEADER) with: `Name`, `Updated`, `Rows`, `Rows Sampled`, `Steps`, `Density`, `Average key length`, `String Index`, `Filter Expression`, `Unfiltered Rows`, `Persisted Sample Percent`.
- **Statistics on Lakehouse SQL Endpoint**: Both `auto_created` and system `ClusteredIndex` statistics exist. User-created statistics require Warehouse (DDL not supported on SQL endpoints). `CREATE STATISTICS`, `UPDATE STATISTICS`, and `DROP STATISTICS` are DW-only operations.

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
- **Source types**: `CustomEndpoint`, `AzureEventHub`, `AzureIoTHub`, `SampleData`, `AmazonKinesis`, `ApacheKafka`, `ConfluentCloud`, `GooglePubSub`, plus CDC types (`AzureSQLDBCDC`, `MySQLCDC`, `PostgreSQLCDC`) and Fabric events (`FabricWorkspaceItemEvents`, `FabricJobEvents`, `FabricOneLakeEvents`, `FabricAnomalyDetectionEvents`).
- **`FabricAnomalyDetectionEvents` source properties**: `workspaceId`, `itemId` (the anomaly-detection-capable item, e.g. Eventhouse), `configurationId` (the anomaly detection configuration), `includedEventTypes` (array, currently only `Microsoft.Fabric.AnomalyDetection.AnomalyDetected` is defined but more may be added), `filters` (Azure Event Grid advanced filter objects for server-side event filtering).
- **CDC source snapshot controls**: `snapshotMode` (`Initial` | `InitialOnly` | `NoData`) is defined on the shared `BaseSQLCDCSourceProperties` and applies to ALL SQL CDC source types (`AzureSQLDBCDC`, `AzureSQLMIDBCDC`, `SQLServerOnVMDBCDC`, `MySQLCDC`, `PostgreSQLCDC`). `excludedColumns` (comma-separated column list), `databaseApplicationIntent` (`ReadWrite` | `ReadOnly`), and `snapshotSelectStatementOverrides` (array of `{tableName, selectStatement}` to override the initial-snapshot SELECT per table) are SQL Server-family-only, defined on `BaseSQLServerCDCSourceProperties` (`AzureSQLDBCDC`/`AzureSQLMIDBCDC`/`SQLServerOnVMDBCDC` only — not `MySQLCDC`/`PostgreSQLCDC`).
- **PostgreSQL CDC additions**: `snapshotLockingMode` gained a new `None` value (alongside `Minimal`/`Extended`) meaning no lock is taken during the initial snapshot. Also added `heartbeatActionQuery` (a SQL statement executed periodically to keep the replication slot active) and `snapshotSelectStatementOverrides` (same shape as the SQL Server family).
- **TLS settings for Kafka/MQTT sources**: `ApacheKafka`/`ConfluentCloud` and `MQTT`-family sources gained an optional `tlsSettings` object: `{"trustCACertificate": {"certificate": <CertificateResource>, "verifyHostname": bool, "cipherSuites": "<str>"}, "clientCertificate": {"certificate": <CertificateResource>, "revocationMode": "Off"|"CRL"|"OCSP"|"CRLAndOCSP"}}`. `CertificateResource` is a discriminator (`type`) union; currently only `KeyVault` is supported (`KeyVaultCertificateResource`: `azureKeyVaultResourceId` (full ARM resource ID, not a UUID) + `certificateName`). These are passthrough JSON fields in fabio (no client-side validation) — pass them via the raw definition JSON in `update-definition`/`add-source`.
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
- **UPDATE (Jun 2026): VersionConfig error resolved, but loading still doesn't complete**: With the new 4-part CI/CD definition format (`graphType.json`, `graphDefinition.json`, `dataSources.json`, `stylingConfiguration.json`), `refresh-graph` now triggers without the VersionConfig error. However, data loading status stays at `NotStarted` indefinitely — the graph never becomes queryable. The definition parts are accepted by `updateDefinition` (LRO Succeeded) but `getDefinition` only returns `.platform` (parts not persisted in the expected format). Creating with definition at creation time returns 202 LRO but the item gets cleaned up (creation fails silently). Conclusion: the new format is documented but not fully operational on all tenants yet (Jun 2026).
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
- **KQL source (`kqlSource-v1`) requires portal initialization**: Always fails via REST API. Previously returned `Activator_Alm_UserError: "The importArtifactRequest field is required"`. As of Jun 2026, the error changed to `"Invalid definition"` (400 Bad Request). Despite being officially documented with full schema (Mar 2026), the `updateDefinition` endpoint does not accept `kqlSource-v1` entities in practice. Configure KQL sources through the Fabric portal, then manage definitions programmatically afterward.
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
- **Inbound External Data Shares bypass policy (new, July 2026 spec, Preview)**: `GET/PUT /workspaces/{ws}/networking/communicationPolicy/inbound/externalDataShares`. Body: `{"defaultAction":"Allow|Deny"}` — no `rules` array (unlike the other inbound/outbound policies). GET requires *viewer*+ role; PUT (`fabio workspace set-inbound-external-data-shares-policy`) requires *admin* role and is marked Preview by Microsoft. Both GET and PUT return an `ETag` response header (not present on the other networking policy endpoints) for optimistic concurrency — PUT accepts an optional `If-Match` request header. fabio surfaces the header as an `etag` field merged into the JSON body (`client.get_with_etag()` / `client.put_with_if_match()`); `fabio workspace get-inbound-external-data-shares-policy` returns `{"defaultAction":"...","etag":"\"...\""}` and `set-inbound-external-data-shares-policy --if-match "<etag>"` passes it back. The PUT response body itself is empty — fabio synthesizes `{"etag":"..."}` from the header alone.
- **Dataset storage format (Power BI API)**: `GET /v1.0/myorg/groups/{id}` returns `defaultDatasetStorageFormat` field (value: `"Small"` or `"Large"`). `PATCH /v1.0/myorg/groups/{id}` with `{"defaultDatasetStorageFormat":"Large"}` changes it. PATCH returns empty 200.
- **`modifyDefaultTier` uses query parameter**: `POST /workspaces/{ws}/onelake/modifyDefaultTier?defaultTier=Hot` with empty body `{}`. NOT a JSON body field. Supported values: `Hot`, `Cool`, `Cold`.
- **Default tier values (corrected)**: `"Hot"`, `"Cool"`, or `"Cold"` (PascalCase). All three tiers are supported.
- **List workspaces `roles` filter**: `GET /workspaces?roles=Admin,Member` supports server-side filtering by the caller's role in the workspace. Comma-separated values.
- **`capacityRegion` moved to `WorkspaceInfo`**: The spec relocated the read-only `capacityRegion` field from the create-workspace response definition to the shared `WorkspaceInfo` definition used by `list`/`show`/`update` responses (no field removed, just consolidated so it now consistently appears across all workspace read responses, including `list`). No fabio code change needed — fabio passes workspace JSON through untyped, and `capacityRegion` was already surfaced in `workspace show`/`list`/`create` output examples.
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
- **CMK encryption endpoints (Preview)**: Three workspace-scoped encryption endpoints:
  - `GET /workspaces/{ws}/encryption` — Returns `WorkspaceEncryptionDetail` with `encryptionDetail.keyIdentifier`, `encryptionDetail.encryptionStatus`, optional `previousEncryptionDetail`, and optional `workspaceEncryptionItemsDetails`.
  - `POST /workspaces/{ws}/encryption/assign` — Body: `{"keyIdentifier": "<versionless-key-uri>"}`. Returns 200 or 202 (LRO). Assigns a customer-managed key to the workspace. Requires Admin role.
  - `POST /workspaces/{ws}/encryption/reset` — Body: `{}`. Returns 200. Removes CMK config and reverts to Microsoft-managed keys. Requires Admin role.
- **EncryptionStatus enum values**: `Disabled`, `Active`, `EnableInProgress`, `DisableInProgress`, `Failed`.
- **Versionless key identifier required**: The `keyIdentifier` for `assign-encryption` must be a versionless Azure Key Vault URI (e.g., `https://myvault.vault.azure.net/keys/mykey`), NOT a versioned URI with the version GUID appended.
- **Admin list-workspaces encryption filter**: `GET /admin/workspaces?include=encryption` adds `encryption.status` and `encryption.keyIdentifier` fields to each workspace in the response. `?encryptionStatus=<status>` filters results — only valid when `include=encryption` is also specified.

## Item API Behaviors Discovered
- **Type filter on list**: `GET /workspaces/{ws}/items?type={ItemType}` filters server-side. Type values are PascalCase (e.g., `Lakehouse`, `Notebook`, `Warehouse`).
- **Valid item types for create**: `CopyJob`, `Dashboard`, `DataAgent`, `DataPipeline`, `Dataflow`, `Environment`, `Eventhouse`, `Eventstream`, `GraphQLApi`, `KQLDashboard`, `KQLDatabase`, `KQLQueryset`, `Lakehouse`, `MLExperiment`, `MLModel`, `MirroredDatabase`, `MirroredWarehouse`, `Notebook`, `Ontology`, `Paginated Report`, `Reflex`, `Report`, `SQLDatabase`, `SQLEndpoint`, `SemanticModel`, `SparkJobDefinition`, `Warehouse`. Sorted, PascalCase. Hinted on invalid type errors.
- **Copy pattern**: `getDefinition` (LRO) from source → `GET` source metadata → `POST /workspaces/{dest}/items` with definition (LRO). Result includes new item's `id`, `displayName`, `type`.
- **Move pattern**: Copy + `DELETE /workspaces/{source}/items/{id}`. Atomic: delete only after successful copy.
- **Definition format query param**: `POST /workspaces/{ws}/items/{id}/getDefinition?format={fmt}` supports format selection.
- **Update definition metadata**: `POST /workspaces/{ws}/items/{id}/updateDefinition?updateMetadata=true` updates `.platform` metadata alongside definition parts.
- **Bulk operations (all LRO)**:
  - `POST /workspaces/{ws}/items/bulkExportDefinitions?beta=True` — exports multiple item definitions. Body: `{"mode":"All"}` (all items) or `{"mode":"Selective","items":[{"id":"<uuid>"},...]}`. Requires `?beta=True` query param. Response: `{"itemDefinitionsIndex":[{"id":"...","rootPath":"...","displayName":"...","type":"..."}],"definitionParts":[{"path":"...","payload":"...","payloadType":"InlineBase64"}]}`. Only exports items caller has read+write permissions for. Items with protected sensitivity labels are excluded.
  - `POST /workspaces/{ws}/items/bulkImportDefinitions?beta=True` — imports multiple item definitions. Body: `{"itemDefinitions":[{"displayName":"...","type":"...","definition":{"parts":[...]}}],"allowPairingByName":true}`. The `allowPairingByName` option matches items by display name instead of logicalId (useful for initial clones). Requires `?beta=True` query param.
  - `POST /workspaces/{ws}/items/bulkMove` — moves multiple items between folders/workspaces
  - **Git integration blocker**: Both `bulkExportDefinitions` and `bulkImportDefinitions` fail with `ActiveCiCdOperationInProgress` error when the workspace has Git integration connected. Disconnect Git first (`fabio git disconnect --workspace <WS>`) or use `deploy export/apply` instead (which uses per-item `getDefinition`/`updateDefinition` and is not affected by Git).
  - **Workspace clone**: `fabio workspace clone --source <WS> --dest <WS>` orchestrates bulk export → transform → bulk import. Supports `--allow-pairing-by-name` and `--item-types` for selective cloning.
- **External data shares**: CRUD at `/workspaces/{ws}/items/{id}/externalDataShares`. Create body: `{"paths": [...], "recipient": {"tenantId": "<id>"}}`. Accept invitations at `/externalDataShares/invitations/{id}/accept`. Supports polymorphic recipients: add `"userPrincipal": {"userPrincipalName": "<upn>"}` or `"servicePrincipal": {"id": "<sp-id>"}` to the `recipient` object.
- **Move to folder**: `POST /workspaces/{ws}/items/{id}/move` with `{"targetFolderId": "<id>"}`. Omit `targetFolderId` or pass `null` to move to workspace root.
- **Identity assignment**: `POST /workspaces/{ws}/items/{id}/identities/default/assign`.
- **Tags**: `POST /workspaces/{ws}/items/{id}/applyTags` and `/unapplyTags` with `{"tagIds": [...]}`.
- **Tags returned inline**: The `Item` response schema includes a `tags` array (`[{"id": "<uuid>", "displayName": "..."}]`) when tags are applied. Absent when no tags — same omission pattern as `sensitivityLabel`. This applies to both standard workspace-scoped endpoints and admin endpoints. No `include` parameter needed.
- **Tags cannot be set at creation**: The `CreateItemRequest` body does NOT support a `tags` field. Tags must be applied post-creation via `applyTags`.
- **No server-side tag filtering**: No query parameter exists to filter items/workspaces by tag. Client-side filtering via JMESPath: `--query "data[?tags[?displayName=='Production']]"`.
- **Tag names inline (no UUID resolution needed)**: Unlike sensitivity labels (which return only UUIDs), tags return `displayName` directly in the item response. No separate resolution step needed.
- **Hard delete query param**: `DELETE /workspaces/{ws}/items/{id}?hardDelete=true` permanently deletes (skips recycle bin). Supported on all workspace-scoped item types.
- **List server-side filtering**: `GET /workspaces/{ws}/items` supports query params: `type={ItemType}` (single type filter), `rootFolderId={folderId}` (items in a specific folder), `recursive={true|false}` (include items in subfolders), `include={type1,type2}` (additional metadata to include in response). The `--folder`, `--recursive`, and `--include` CLI flags map to these query params.
- **Relations (beta)**: `GET /workspaces/{ws}/items/{id}/relations/upstream?beta=true` and `.../relations/downstream?beta=true` return `{"items":[...],"relations":[...],"workspaces":[...]}` — a graph fragment, not a paginated list. `items` are related items (id/type/displayName/workspaceId), `relations` are edges (`itemId`, `dependentOnItemId`, `relationType`), `workspaces` resolves the workspace IDs referenced by cross-workspace relations. `relationType` values: `CascadeDelete`, `WeakAssociation`, `Datasource`, `PushData`, `Orchestration`, `Shortcut`, `HiddenInWorkspace`. Requires `?beta=true` query param — omitting it returns an error. Rendered via `render_object` (not `render_list_with_token`) since it is not paginated.

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
- **Sync server-side dedup**: When a file needs copying, checks if any existing destination file has the same content hash (ETag in default mode, Content-MD5 in checksum mode). If so, performs a same-lakehouse copy (faster than cross-lakehouse). Output includes `"dedupCopied"` count.
- **Sync rename detection**: When `--delete` is active, detects files renamed at source by matching source-only files with dest-only files. Two-pass detection: (1) ETag match (zero-cost, works for files uploaded with MD5 stored), (2) Content-MD5/size match via HEAD requests when `--checksum` is active (works for all files including Fabric-generated). Detected renames use atomic O(1) DFS rename at the destination instead of copy + delete. Output includes `"renamed"` count.
- **Sync rename detection limitation**: OneLake DFS rename (`x-ms-rename-source`) changes the ETag when the file was NOT uploaded with `x-ms-content-md5`. Files uploaded with fabio (which stores MD5) preserve ETags on rename. Fabric-generated files (Spark, pipelines) do not have Content-MD5, so checksum mode falls back to unique-size matching.
- **Sync filtering**: `--include`/`--exclude` glob patterns (semicolon-separated); `--min-size`/`--max-size` with K/M/G suffixes; `--no-recursive` for top-level only. Filters apply to source map before comparison. With `--delete`, excluded files are also excluded from deletion scope.
- **Sync modes**: `--size-only` (compare by size only), `--no-overwrite` (only copy new files), `--force` (mirror mode, overwrite all), `--existing` (only update files already at dest).
- **Sync safety**: `--max-delete=NUM` skips ALL deletions if count exceeds NUM (prevents catastrophic mistakes). Output includes `"deletionsSkipped": true`.
- **Sync move semantics**: `--remove-source-files` deletes source files after successful transfer. Output includes `"sourceRemoved": N`.
- **Sync observability**: `--itemize` outputs per-file actions on stderr (`[copy]`, `[rename]`, `[delete]`, `[skip]`).
- **Sync command flag structure**: The `lakehouse sync` command uses explicit source/destination flags (NOT the standard `--workspace`/`--id` pattern used by other lakehouse commands). Source flags: `--source-workspace`, `--source-id`, `--source-path`. Destination flags: `--dest-workspace`, `--dest-id`, `--dest-path`. The `--local` flag replaces the source flags for local-to-remote sync.
- **Sync local-to-remote** (`--local`): Syncs a local directory to a remote lakehouse path. Builds file map from local filesystem, compares by size (default) or Content-MD5 (`--checksum`), uploads only new/changed files via parallel DFS upload (which stores Content-MD5). All filtering flags work (`--include`, `--exclude`, `--min-size`, `--max-size`, `--no-recursive`). Rename detection and server-side dedup are skipped (not applicable for local sources). `--remove-source-files` deletes local files after successful upload (move semantics). Mutually exclusive with `--source-workspace`/`--source-id`/`--source-path`.
- **Parallel execution**: All multi-file operations (upload, copy-file, move-file, delete-table, copy-table, move-table, sync) use concurrent execution with rate-limit retry.
- **Glob patterns**: Local globs via `glob::glob()`, remote globs via listing + pattern match, table globs via table list API + pattern match.
- **Materialized views**: `POST /workspaces/{ws}/lakehouses/{id}/jobs/refreshMaterializedLakeViews/instances` triggers refresh. Schedule management at `.../jobs/refreshMaterializedLakeViews/schedules`. Schedule request bodies (`CreateLakehouseRefreshMaterializedLakeViewsScheduleRequest`/`Update...`) may include an optional `executionData.mlvExecutionDefinitionId` field referencing a materialized lake view execution definition — pass it via `--file`/`--content` JSON body.
- **MLV execution definitions (new)**: CRUD at `/workspaces/{ws}/lakehouses/{id}/mlvexecutiondefinitions` (list/create) and `.../mlvexecutiondefinitions/{defId}` (show/update/delete). An execution definition groups: `currentLakehouseExecutionContext` (discriminated union, `mode`: `All` | `Selected` with `selectedMlvs: [<fqn>]`), `extendedLineageExecutionContext` (same discriminator shape but with `selectedLakehouses: [{id}]` for cross-lakehouse lineage), and optional `settings` (`environment`: Spark environment item reference, `refreshMode`: `Optimal` (default) | `Full`). `displayName` and `currentLakehouseExecutionContext` are required on create; all fields are optional (partial-update) on update. List response key is `"value"` (standard pagination via `continuationUri`/`continuationToken`).
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
- **ADLS Gen2 shortcut list-files limitation**: After creating an ADLS Gen2 shortcut, `list-files` on the shortcut path may not show the actual storage files. The OneLake DFS layer virtualizes the path and may return the lakehouse internal structure (Files/, Tables/, Functions/) instead of the blob contents. This is a Fabric platform behavior — the files ARE accessible for `load-table` operations. Agents should NOT waste time debugging list-files when shortcuts don't show expected files; instead proceed directly with load-table using the expected path (e.g., `Files/shortcutname/file.csv`).
- **Shortcut propagation delay**: After creating a shortcut, allow 5-10 seconds before accessing files through it. If `load-table` returns NOT_FOUND, retry after a short wait.

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
- **Connectivity types**: `ShareableCloud`, `OnPremises`, `VirtualNetworkGateway`, `StreamingVirtualNetworkGateway`, `PersonalCloud`.
- **Credential types**: `Basic`, `OAuth2`, `Key`, `Anonymous`, `ServicePrincipal`, `SharedAccessSignature`, `WorkspaceIdentity`, `KeyPair`.
- **Privacy levels**: `None`, `Public`, `Organizational`, `Private`.
- **Parameters format conversion**: User provides JSON object `{"key": "value"}` which is converted to array format `[{"dataType": "Text", "name": "key", "value": "value"}]` for the API.
- **Create body structure**: `{"displayName": "...", "connectivityType": "...", "connectionDetails": {"type": "...", "creationMethod": "...", "parameters": [...]}, "credentialDetails": {"singleSignOnType": "None", "connectionEncryption": "NotEncrypted", "skipTestConnection": bool, "credentials": {"credentialType": "..."}}, "privacyLevel": "..."}`.
- **`gatewayId` required for gateway-routed connections**: `VirtualNetworkGateway` and `StreamingVirtualNetworkGateway` connections both require a top-level `gatewayId` field (the object ID of the gateway the connection is created under) in the create request. `fabio connection create --gateway-id <ID>` maps to this field; the CLI rejects create requests for these two connectivity types when `--gateway-id` is omitted.
- **`StreamingVirtualNetworkGateway` connectivity type (new)**: Mirrors `VirtualNetworkGateway`'s create/update request shape exactly (`gatewayId` + `credentialDetails` on create; `displayName` + `credentialDetails` on update) but connects through a streaming virtual network gateway (`GatewayType` = `StreamingVirtualNetwork`) instead of a regular virtual network gateway.
- **`testConnection` not supported for `StreamingVirtualNetworkGateway`**: The Fabric API explicitly does not support `POST /connections/{id}/testConnection` for connections whose `connectivityType` is `StreamingVirtualNetworkGateway`.
- **Test connection**: `POST /connections/{id}/testConnection` with empty body `{}`.
- **Role assignments**: Full CRUD at `/connections/{id}/roleAssignments/{assignmentId}`. Roles: `Owner`, `User`, `UserWithReshare`.
- **Role assignment body**: `{"principal": {"id": "...", "type": "User|Group|ServicePrincipal"}, "role": "Owner|User|UserWithReshare"}`.
- **List supported types**: `GET /connections/supportedConnectionTypes` returns all available connection type definitions.
- **`gatewayId` is now a base `Connection` response property (July 2026 spec update)**: Previously `gatewayId` only appeared on the `OnPremisesGatewayConnection`/`OnPremisesGatewayPersonalConnection`/`VirtualNetworkGatewayConnection` discriminated response subtypes. The spec moved it to the base `Connection` schema, so it can now appear on ANY connectivity type's response (e.g., a `ShareableCloud` connection can report a `gatewayId` if it routes through a gateway). No request-shape change — `fabio connection create --gateway-id` already sent the field unconditionally when provided. `fabio connection list` now shows a dynamic `GATEWAY ID` column when any returned connection has a non-null `gatewayId` (mirrors the sensitivity-label dynamic-column pattern).

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
- **Schedule CRUD**: Full lifecycle at `/workspaces/{ws}/dataPipelines/{id}/jobs/execute/schedules/{scheduleId}`. GET (show), PATCH (update), DELETE (remove). List returns `{"value": [...]}` with `id`, `enabled`, `createdDateTime`, `configuration`, `owner` fields.
- **Schedule configuration types**: `Cron` (with `interval` in minutes), `Weekly` (with `weekdays` array + `times` array), `Daily`. All include `startDateTime`, `endDateTime`, `localTimeZoneId`.
- **Job instances**: `GET /workspaces/{ws}/dataPipelines/{id}/jobs/execute/instances` lists execution history. Individual instance at `.../instances/{instanceId}`. Fields: `id`, `itemId`, `jobType`, `invokeType` (Manual/Scheduled), `status`, `rootActivityId`, `startTimeUtc`, `endTimeUtc`, `failureReason`.
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
- **Schema discovery (`.show database schema as json`)**: Returns nested JSON: `{"Databases":{"<db-id>":{"Tables":{...},"Functions":{...},"MaterializedViews":{...},"ExternalTables":{...}}}}`. The top-level key is the database GUID (not display name). Tables include `OrderedColumns` with `Name`, `Type` (System.X), `CslType` (KQL type).
- **Inline ingestion (`.ingest inline into table`)**: Accepts CSV data after `<|` separator. Limited to ~4MB payload. Returns extent info on success. Requires management endpoint (v1/rest/mgmt).
- **Query plan (`.show queryplan <| query`)**: Returns execution plan rows with operator tree, estimated row counts, concurrency hints. Uses management endpoint.
- **Cluster diagnostics**: `.show capacity`, `.show cluster`, `.show diagnostics` are independent commands. Each may fail independently due to permissions (Fabric KQL databases may restrict some admin commands). The `diagnostics` command aggregates results gracefully, reporting errors per section.
- **Deeplink URL patterns**: Fabric KQL databases use `https://app.fabric.microsoft.com/groups/{ws}/kqlDatabases/{id}?query={encoded}&database={name}`. ADX clusters use `https://dataexplorer.azure.com/clusters/{uri}/databases/{db}?query={encoded}`. Auto-detection uses URI pattern: `.kusto.fabric.microsoft.com` → Fabric, `.kusto.windows.net` → ADX.
- **Query monitoring commands**: `.show running queries` returns currently executing queries (may return empty on idle clusters). `.show journal` returns operations history (schema changes, ingestion operations). `.show queries` returns recently completed queries with full detail (duration, CPU, memory peak, scanned extents, cache statistics). All use management endpoint (`/v1/rest/mgmt`). Verified live — `.show queries` includes `ClientRequestProperties` with user agent, timeout settings, and resource usage breakdown.

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
- **Schedule response format**: `GET .../schedules` returns `{"value":[{"id":"uuid","enabled":true,"createdDateTime":"...","owner":{"id":"...","type":"User"},"configuration":{"type":"Cron","interval":5,"startDateTime":"...","endDateTime":"...","localTimeZoneId":"UTC"}}]}`.
- **Deploy schedule export**: `deploy export` queries schedules for item types: Notebook (DefaultJob), DataPipeline (Pipeline), SparkJobDefinition (SparkJob), Lakehouse (DefaultJob), SemanticModel (DefaultJob), Dataflow (DefaultJob), CopyJob (DefaultJob). Writes `schedules.metadata.json` per item (strips `id`, `createdDateTime`, `owner`; adds `jobType`).
- **Deploy schedule apply**: `deploy apply` creates schedules via `POST .../schedules` as a post-hook. Additive (doesn't delete existing schedules). Non-fatal (failures warn but don't block deploy).
- **Schedule export portability**: The `configuration` block varies by type — Cron uses `interval`/`startDateTime`/`endDateTime`/`localTimeZoneId`; other types may use different fields. The entire `configuration` object is preserved as-is for round-trip fidelity.
- **Max 20 schedules per item**: Fabric enforces a limit of 20 schedules per item. Deploy apply should be used carefully to avoid hitting this limit on repeated deploys.

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
- **Execute query is LRO-aware (Jun 2026)**: `POST .../executeQuery` now returns 202 for long-running queries (up to 90s server-side). Supports `Accept: application/vnd.apache.arrow.stream;pq-arrow-version=1|2` header for Arrow format version selection. fabio's `--arrow-version` flag (default 1) controls this.

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
- **Error `isRetriable` field**: API responses may include `error.isRetriable: bool`. When present, included in error output as `"retriable": true/false`. Omitted from output when not provided by the API (backward compatible — not present when null).
- **Error `requestId` field**: API error responses may include `error.requestId` (correlation ID for support tickets). When present, included in error output as `"requestId": "<uuid>"`. Omitted from output when not provided.
- **Error `moreDetails` field**: API error responses may include `error.moreDetails` (array of nested sub-errors with `code` and `message`). When present, included in error output as `"moreDetails": [{"code":"...","message":"..."}]`. Omitted from output when not provided.
- **Error `relatedResource` field**: API error responses may include `error.relatedResource` (object with `resourceType` and `resourceId`). When present, included in error output as `"relatedResource": {"resourceType":"...","resourceId":"..."}`. Omitted from output when not provided.
- **Dry-run guard**: All mutations support `--dry-run` which returns the planned request body without executing. Output: `{"status": "dry_run", "message": "Would <action>..."}`.
- **Definition operations pattern**: `POST .../getDefinition` (LRO, empty body `{}`) returns base64-encoded parts. `POST .../updateDefinition` (LRO) accepts `{"definition": {"parts": [{"path": "<file>", "payload": "<base64>", "payloadType": "InlineBase64"}]}}`.
- **Tenant-level vs workspace-scoped resources**:
  - Tenant-level (no workspace prefix): `/capacities`, `/connections`, `/deploymentPipelines`, `/admin/domains`, `/externalDataShares/invitations`
  - Workspace-scoped: All other resources at `/workspaces/{ws}/<resource>`

## Variable Library API Behaviors Discovered
- **Definition format**: Three definition parts: `variables.json` (variable definitions) + `settings.json` (ordering/display) + `valueSets/<name>.json` (one per alternate value set). Path must use `valueSets/` (plural, forward slash).
- **variables.json schema**: `https://developer.microsoft.com/json-schemas/fabric/item/variableLibrary/definition/variables/1.0.0/schema.json`. Structure: `{"$schema":"...","variables":[{"name":"...","type":"String","value":"...","note":""}]}`. Each variable has `name` (required), `type` (required), `value` (required), `note` (optional). Supported types: String, Number, Integer, DateTime, Boolean, ItemReference.
- **settings.json schema**: `https://developer.microsoft.com/json-schemas/fabric/item/variableLibrary/definition/settings/1.0.0/schema.json`. Structure: `{"$schema":"...","valueSetsOrder":["setName1","setName2"]}`. The `valueSetsOrder` can be empty or partial; missing names are appended alphabetically.
- **Value set file schema**: `https://developer.microsoft.com/json-schemas/fabric/item/variableLibrary/definition/valueSet/1.0.0/schema.json`. Structure: `{"$schema":"...","name":"prod","variableOverrides":[{"name":"var1","value":"override_val"}]}`. Key field is `variableOverrides` (NOT `values`). Only variables that differ from the default need to be listed.
- **Value set path**: Must be `valueSets/<name>.json` (plural "valueSets", forward slash). NOT `valueSet/` (singular). The singular path causes "Item definition contains an unexpected definition part" error.
- **updateDefinition requires valid content structure**: The API validates variable definitions. Sending a well-formed JSON with incorrect variable structure returns "Item content cannot be used". All required parts (variables.json + settings.json) must be included for a successful update.
- **Active value set is a workspace-level setting**: NOT stored in the definition. Each workspace can have a different active set. Use `PATCH /workspaces/{ws}/variableLibraries/{id}` with `{"properties":{"activeValueSetName":"prod"}}` to switch.
- **Default active value set name**: Freshly created libraries report `activeValueSetName: "Default value set"` (not just `"Default"`). The API returns this full string.
- **Activating a non-existent value set**: Returns an API error (the API validates the name against defined value sets).
- **Deploy ordering**: VariableLibrary is FIRST in deploy order (tier 0), deployed before all other item types.
- **Deploy post-hook**: When `--env` is specified, fabio auto-activates the matching value set after deploying variable libraries. Non-fatal (failures warn but don't block).
- **fabric-cicd compatibility**: fabric-cicd auto-activates value sets when environment name matches value set name. fabio implements the same behavior.
- **Create is LRO**: Returns 202, requires polling.
- **getDefinition is LRO**: Returns 202, requires polling. Returns `variables.json` + `settings.json` + `.platform` + any `valueSets/*.json` parts.
- **409 Conflict on duplicate name**: Same pattern as all other items.
- **Endpoint pattern**: `/workspaces/{ws}/variableLibraries/{id}`.
- **Service principal support**: Full support (create, update, delete, get/update definition, activate value set).

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
- **Available commands**: list, show, connection-string, query, refresh-metadata, get-audit-settings, update-audit-settings, set-audit-actions.
- **Query uses TDS via shared utilities**: `sql-endpoint query` fetches the connection string from `GET /workspaces/{ws}/sqlEndpoints/{id}/connectionString`, resolves the display name as the initial catalog, then delegates to `execute_and_render_sql()`. Supports `--sql` (inline text), `@file` path, or stdin piping.
- **Connection string format**: Returns the DW-style endpoint hostname (e.g., `*.datawarehouse.fabric.microsoft.com`).
- **refresh-metadata returns table sync status**: Each table shows `status` (`NotRun`, `Succeeded`, `Failed`), `startDateTime`, `endDateTime`, `lastSuccessfulSyncDateTime`.
- **Audit settings structure**: `{"state":"Disabled|Enabled","retentionDays":N,"auditActionsAndGroups":["GROUP1","GROUP2",...]}`.
- **Default audit groups**: `SUCCESSFUL_DATABASE_AUTHENTICATION_GROUP`, `FAILED_DATABASE_AUTHENTICATION_GROUP`, `BATCH_COMPLETED_GROUP`.
- **Endpoint pattern**: `/workspaces/{ws}/sqlEndpoints/{id}`.
- **Query insights views available**: SQL Endpoints expose the same `queryinsights.*` schema views as warehouses (`frequently_run_queries`, `long_running_queries`, `exec_requests_history`). The `sys.dm_exec_requests` DMV is also available for running query monitoring. Verified live — all views return data successfully.
- **No queries-kill**: SQL Endpoints are read-only views, so `KILL <session_id>` is not appropriate (and would not work for user sessions).

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
- **update-compute endpoint**: `POST /workspaces/{ws}/apacheAirflowJobs/{id}/environment/updateCompute?beta=true` with body `{"poolTemplateId": "<uuid>"}`. LRO (202 with `Retry-After: 30`). Updates which pool template is assigned to the environment. Requires `Contributor` role.
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
- **Agent-context coverage**: `fabio context agent` now includes a full `app-backend` schema (mutability, async create, and `--hard-delete` bool flag metadata).
- **Endpoint patterns**: `/workspaces/{ws}/appBackends` and `/workspaces/{ws}/appBackends/{id}`.

## Azure Databricks Storage API Behaviors Discovered
- **Item type**: `AzureDatabricksStorage` (Fabric integration with Azure Databricks for storage management).
- **Endpoint pattern**: `/workspaces/{ws}/azureDatabricksStorages/{id}`.
- **Definition format**: `AzureDatabricksStorageV1`. Definition file path is `definition.json` (NOT `AzureDatabricksStorage.json` — the API spec examples explicitly use `definition.json`).
- **Create is LRO**: Returns 202, requires polling. Supports optional `definition`, `folderId`, `sensitivityLabelSettings` in request body.
- **getDefinition is LRO**: Returns 202 or 200. Response includes `definition.json` + `.platform` parts.
- **updateDefinition is LRO**: Supports `?updateMetadata=true` query parameter. Body: `{"definition":{"format":"AzureDatabricksStorageV1","parts":[{"path":"definition.json","payload":"<base64>","payloadType":"InlineBase64"}]}}`.
- **Delete returns 200**: Not LRO. Supports `?hardDelete=true`.
- **Feature availability is workspace-specific**: The feature may be enabled on some workspaces but not others within the same tenant. `FeatureNotAvailable` (403) is returned on workspaces where the feature is not active.
- **Registered in DEPLOY_ORDER**: Position after `MirroredAzureDatabricksCatalog`, before `Lakehouse` (position 6 in storage tier).
- **Response fields**: Standard item fields (`id`, `displayName`, `description`, `type`, `workspaceId`). No `properties` or `attributes` observed.

## Gateway API Behaviors Discovered
- **Tenant-level scope**: `GET /gateways` (no workspace prefix). Individual: `GET /gateways/{id}`.
- **Create requires VNet infrastructure**: `POST /gateways` needs capacity ID, VNet subscription/resource group/name/subnet. Subnet must be delegated to `Microsoft.PowerPlatform/vnetaccesslinks`. The `Microsoft.PowerPlatform` resource provider must be registered on the Azure subscription.
- **Gateway type**: `VirtualNetwork` and `StreamingVirtualNetwork` types are supported via REST API (`fabio gateway create` / `fabio gateway create-streaming`, respectively). On-premises gateways are managed by the gateway application installer.
- **`StreamingVirtualNetworkGateway` type (new)**: `POST /gateways` with `"type": "StreamingVirtualNetwork"` only requires `displayName` and `virtualNetworkAzureResource` — no `capacityId`, `inactivityMinutesBeforeSleep`, or `numberOfMemberGateways` (unlike the regular `VirtualNetwork` type). Update (`UpdateStreamingVirtualNetworkGatewayRequest`) only supports the base `displayName`/`type` fields — no other mutable properties. `fabio gateway create-streaming` implements the create path; `fabio gateway update` (shared with all gateway types) covers the update path.
- **`virtualNetworkAzureResource` uses component fields**: The API expects separate `subscriptionId`, `resourceGroupName`, `virtualNetworkName`, `subnetName` fields — NOT a full ARM resource ID.
- **`inactivityMinutesBeforeSleep` is required**: Must be one of: 30, 60, 90, 120, 150, 240, 360, 480, 720, 1440. Default in CLI: 120. Not applicable to `StreamingVirtualNetwork` gateways.
- **`numberOfMemberGateways` is required**: Must be between 1 and 9. Default in CLI: 1. Not applicable to `StreamingVirtualNetwork` gateways.
- **Creation is slow**: Gateway creation takes 60-90 seconds to return. No LRO pattern (returns 201 directly, but response is delayed).
- **Update requires `type` field**: `PATCH /gateways/{id}` body MUST include `"type": "VirtualNetwork"` (or `"OnPremises"`/`"StreamingVirtualNetwork"` for other gateway types). Without it, returns "The request has an invalid input". The CLI auto-fetches the current type via GET before PATCH.
- **VNet gateways have no "members" endpoint**: `GET /gateways/{id}/members` returns NOT_FOUND for VNet gateways. Members are an on-premises gateway concept.
- **Role assignment uses nested principal object**: `POST /gateways/{id}/roleAssignments` body format: `{"principal": {"id": "<uuid>", "type": "User|Group|ServicePrincipal"}, "role": "Admin|ConnectionCreator|ConnectionCreatorWithResharing"}`. Flat `principalId`/`principalType` format is rejected.
- **Cannot demote last Admin**: Attempting to update the sole Admin's role to a lower level returns `DMTS_CannotDeleteLastGatewayPrincipalError`.
- **Duplicate role assignment returns CONFLICT**: Adding a role for a principal that already has one returns 409 with "Gateway role assignemnt already exists" (note: API has typo "assignemnt").
- **Non-existent principal returns 500**: Adding a role for a UUID that doesn't resolve to a real Entra ID principal returns "An unexpected error occurred" (internal server error, not a clean validation error).
- **Delete is immediate**: `DELETE /gateways/{id}` returns immediately. However, the Azure VNet's `serviceAssociationLinks/PowerPlatformSAL` persists for several minutes after deletion, blocking VNet/subnet removal until Power Platform cleans up.
- **Available commands**: list, show, create, create-streaming, update, delete, list-members, update-member, delete-member, list-role-assignments, add-role-assignment, show-role-assignment, update-role-assignment, delete-role-assignment.
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
- **Jobs API reveals actual failure**: The `show` command shows `lastDataLoadingStatus.status: "NotStarted"` even when the job has already `Failed`. Must check the Jobs API directly (`GET /jobs/instances/{jobId}`) to see the real status with `failureReason`.

## Graph Query Set API Behaviors Discovered (Additional)
- **Definition is read-only**: `exportedDefinition.json` content (`ArtifactContents`, `dependencies`, `ConfigurationCategories`) is always empty arrays when retrieved via API. Query content is portal-managed only.

## Warehouse Snapshot API Behaviors Discovered
- **Create requires `creationPayload` with warehouse ID**: Simple `displayName`-only creation returns "Invalid payload used for operation." Must include `{"creationPayload":{"warehouseId":"<warehouse-id>"}}`.
- **Requires existing warehouse**: Cannot test without a warehouse item in the workspace.
- **Endpoint pattern**: `/workspaces/{ws}/warehouseSnapshots/{id}`.
- **Available commands**: list, show, create (with --warehouse-id), update, delete.

## Dashboard/Datamart/Paginated Report API Behaviors Discovered
- **Read-only list items**: Dashboard has only `list` command. Datamart has only `list`.
- **Paginated Report now supports full CRUD + definitions**: As of spec commit 49e5f16, the Fabric REST API exposes `create`, `show` (GET), `delete`, `getDefinition` (POST LRO), and `updateDefinition` (POST LRO) endpoints for paginated reports in addition to the existing `list` and `update` (PATCH) commands.
- **Create is LRO**: `POST /workspaces/{ws}/paginatedReports` returns 202, requires polling. Body requires `displayName` and `definition` (with `format: "PaginatedReportDefinition"` and `parts` array).
- **Definition format is `PaginatedReportDefinition`**: Definition parts contain the `.rdl` file(s). Each part: `{"path": "ContosoReport.rdl", "payload": "<base64>", "payloadType": "InlineBase64"}`.
- **getDefinition is LRO**: `POST .../getDefinition` with empty body `{}`. Returns 202, requires polling.
- **updateDefinition supports `?updateMetadata=true`**: Append to URL to propagate `.platform` metadata changes.
- **delete returns 200**: `DELETE /workspaces/{ws}/paginatedReports/{id}` returns immediately (not LRO). Supports `?hardDelete=true` for permanent deletion.
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
- For each item: calls `getDefinition` (LRO POST with empty body `{}`) **in parallel** (bounded by `--concurrency`, default 8)
- **Auto-provisioned types excluded by default**: SQLEndpoints are filtered out of the item list before processing — they don't appear in `total_items`, `exported`, or `skipped`. This avoids confusing count gaps for agents. They can still be explicitly inspected via `--item-types SQLEndpoint`.
- **Items that fail `getDefinition`**: Added to `skipped` list with reason (not fatal), UNLESS the item type is a "shell-only" type (see below)
- **Shell-only types** (Warehouse, SQLDatabase, MLExperiment, MLModel): These types don't support `getDefinition` but ARE valid deployment targets. When `getDefinition` fails, they are exported with just a `.platform` metadata file (no definition parts). `deploy apply` creates them with just `displayName` + `type` and skips `updateDefinition`. This aligns with fabric-cicd's `SHELL_ONLY_PUBLISH` concept.
- **SQLEndpoint is always skipped**: SQLEndpoints are auto-provisioned by Fabric when a Lakehouse, Warehouse, or SQL Database is created. They are NOT independently deployable — fabric-cicd doesn't even include them as a supported item type. Skipping them during export is correct behavior.
- **Legacy-format items fail `getDefinition`**: Some SemanticModel and Report items created through the portal UI or from Microsoft-provided templates use the older PBIX format internally. These do NOT support `getDefinition` — only items using newer definition formats (TMDL for SemanticModel, PBIR/PBIR-Legacy for Report) expose definitions through the API. Known examples: pre-installed "Microsoft Fabric Capacity Metrics" workspace items, Direct Lake semantic models created via the portal in legacy format. These appear in the `skipped` list with reason "getDefinition not supported" — this is a Fabric platform constraint, not a fabio bug.
- **Items without definition parts**: Skipped with reason "no definition parts" (unless shell-only type)
- **`.platform` part from API is discarded**: Export generates its own `.platform` from item metadata
- **Logical ID extracted from API's `.platform`** BEFORE filtering (read then discard)
- **`definition_format`**: Captured from `data.definition.format` if present in API response
- **`--concurrency`**: Max parallel `getDefinition` LRO requests (default 8). Higher values speed up large workspaces but risk throttling.
- **`--overwrite`**: Required if output directory is non-empty (checked via iterator peek)
- **`--dry-run`**: Counts items without writing to disk
- **`--item-types`**: Case-insensitive filter on item types. When specified, auto-provisioned types are NOT excluded (user explicitly asked for them).
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

## Data Build Tool Job API Behaviors Discovered
- **Item type**: `DataBuildToolJob` (preview item type for dbt integration).
- **Endpoint pattern**: `/workspaces/{ws}/dataBuildToolJobs/{id}`.
- **Run uses item-specific path**: `POST /workspaces/{ws}/dataBuildToolJobs/{id}/jobs/execute/instances` (NOT the generic items job endpoint). Uses `trigger_item_job(workspace, id, "execute", None)` for proper job ID extraction from Location header.
- **Run supports --wait/--timeout/--cancel-on-timeout**: Polls `GET /workspaces/{ws}/items/{id}/jobs/instances/{job_id}` every 5 seconds. Default timeout 600s. Terminal statuses: `Completed`, `Failed`, `Cancelled`.
- **Create is LRO**: Returns 202, requires polling.
- **getDefinition/updateDefinition are LRO**: Both use standard Fabric LRO polling pattern.
- **Definition format**: Not yet documented (pending live tenant validation).
- **Added to DEPLOY_ORDER**: Position between existing items in dependency chain.

## OrgApp API Behaviors Discovered
- **Item type**: `OrgApp` (Organizational App — published app packages for workspace content distribution).
- **Endpoint pattern**: `/workspaces/{ws}/orgApps/{id}`.
- **Standard CRUD + definitions**: Full lifecycle via list/show/create/update/delete/get-definition/update-definition.
- **Create is LRO**: Returns 202, requires polling.
- **getDefinition/updateDefinition are LRO**: Both use standard Fabric LRO polling pattern.
- **Added to DEPLOY_ORDER**: Positioned after visualization items.
- **`format` field and part path renamed (July 2026 spec update)**: `OrgAppPublicDefinition.format` is no longer a constrained enum (`OrgAppV1` value + `x-ms-enum` removed) — it's now a free-form string. The create/get/update examples also changed the part path from `OrgAppV1.json` to `definition.json`. No fabio code change needed: `org_app.rs` already builds `update-definition` requests with `path: "definition.json"` and never hardcoded a `format` field.

## OrgAppAudience API Behaviors Discovered
- **Item type**: `OrgAppAudience` (audience targeting for Organizational Apps).
- **Endpoint pattern**: `/workspaces/{ws}/orgAppAudiences/{id}`.
- **Standard CRUD + definitions**: Full lifecycle via list/show/create/update/delete/get-definition/update-definition.
- **Create is LRO**: Returns 202, requires polling.
- **getDefinition/updateDefinition are LRO**: Both use standard Fabric LRO polling pattern.
- **Added to DEPLOY_ORDER**: Positioned after OrgApp (dependent item).
- **`format` field and part path renamed (July 2026 spec update)**: Same change as `OrgApp` — `OrgAppAudiencePublicDefinition.format` (previously the `OrgAppAudienceV1` enum) is now a free-form string, and the `CreateOrgAppAudience` example changed the part path from `OrgAppAudienceV1.json` to `definition.json` (the `"format": "OrgAppAudienceV1"` field was also dropped). No fabio code change needed: `org_app_audience.rs` never sets a `format` field and already uses `definition.json` as the part path.

## Copy Job Reset API Behaviors Discovered
- **Reset endpoint**: `POST /workspaces/{ws}/copyJobs/{id}/resetCopyJob` resets copy job entities to allow re-copying.
- **Reset all entities**: Body `{"resetAllCopyJobEntities": true}` resets everything.
- **Reset specific entities**: Body `{"copyJobEntityIds": ["uuid1", "uuid2"]}` resets selected entities by UUID.
- **Mutually exclusive flags**: `--all` and `--entity-ids` cannot be used together; omitting both is a client-side error.
- **No LRO**: Returns immediately (fire-and-forget).

## Gateway Lifecycle API Behaviors Discovered
- **Check status**: `GET /gateways/{id}/checkStatus` returns gateway connectivity status.
- **Check member status**: `GET /gateways/{id}/members/{memberId}/checkStatus` returns individual member connectivity status.
- **Restart**: `POST /gateways/{id}/restart` with empty body `{}`. LRO (polls until complete). Requires Admin permission.
- **Shutdown**: `POST /gateways/{id}/shutdown` with empty body `{}`. LRO (polls until complete). Requires Admin permission.
- **All require gateway Admin role**: Lifecycle operations restricted to gateway administrators.

## Deploy Fabric-CICD Compatibility Behaviors Discovered
- **`.platform` is a definition part**: The Fabric API uses `.platform` in definition parts for metadata updates (`?updateMetadata=true`). fabio includes `.platform` in parts sent to API but excludes it from content hash (API modifies `logicalId`, breaking skip detection).
- **`.children/` discovery**: Eventhouses use `.children/` subdirectories to hold child items (KQL Databases). Discovered and deployed as independent items, not parts of the parent.
- **`.pbi/` exclusion**: Power BI Desktop creates `.pbi/` directories with local metadata. Always excluded from definition parts.
- **`creationPayload` in `.platform` metadata**: fabric-cicd stores `creationPayload` inside `.platform` JSON's `metadata` block. fabio reads this as fallback when no standalone `creationPayload.json` exists.
- **`SparkJobDefinitionV2` format auto-detection**: When `.platform` lacks `definitionFormat`, SparkJobDefinition items auto-use `"SparkJobDefinitionV2"`.
- **Notebook format auto-detection**: When `.platform` lacks `definitionFormat`, the format is inferred from the content file name: `notebook-content.ipynb` → `format: "ipynb"` (JSON ipynb), `notebook-content.py` → no format (server auto-detects the native Fabric `.py` format starting with `# Fabric notebook source`). This matches the behavior of repos using Fabric Git Integration (e.g., microsoft/fabric-toolbox).
- **fabric-toolbox deployment results (Jul 2026)**: Tested deploying all 6 workspace directories from `microsoft/fabric-toolbox`. Results: 85/115 items (74%) deploy successfully. Failures are Fabric API limitations: Reports (9) require SemanticModel refresh before import, DataPipelines (21) contain hardcoded workspace GUIDs needing parameter substitution, Other (9) have similar reference issues. These are the same limitations fabric-cicd faces — require `parameters.json` or manual post-deploy steps.
- **Report `byPath`→`byConnection` transform**: PBIP format `byPath` references (unsupported by API) are auto-converted to `byConnection` with the semantic model's resolved GUID.
- **Notebook part ordering**: Content files (`.py`, `.ipynb`) must precede settings (`.json`). fabio sorts at deploy time.
- **`ItemDisplayNameNotAvailableYet`**: After deletion, name may be reserved up to 5 minutes. fabio retries 10x at 30s intervals.
- **Binary payloads skipped**: Non-UTF-8 payloads silently skipped during parameter replacement and reference validation.
- **Lakehouse `enableSchemas`**: Inferred from `lakehouse.metadata.json` containing `"defaultSchema"`.
- **Workspace ID placeholder**: `00000000-0000-0000-0000-000000000000` is auto-replaced with target workspace UUID (regex-based, workspace-reference keys only, skips shortcuts).
- **Shortcut self-reference**: When shortcut `target.oneLake.itemId` is the default GUID, it means "this lakehouse itself" — replaced with the lakehouse's own deployed GUID (not the workspace ID).

## Context Extract Behaviors Discovered
- **Three-layer relationship discovery**: Layer 1 (properties) finds typed edges from item GET responses. Layer 2 (`--deep`) decodes base64 definition payloads and regex-scans for UUID references. Layer 3 (`--include-connections`) fetches `/items/{id}/connections`. Each layer is additive — deeper layers find significantly more edges.
- **Properties layer alone finds very few edges**: In a 154-item tenant, properties-only discovered 2 edges (both `has_endpoint`). Deep mode found 88 edges. Most relationships are embedded inside definitions, not exposed in the item's GET response.
- **GUID scanning discovers all cross-references generically**: By building a registry of known item/workspace IDs and regex-matching `[0-9a-fA-F]{8}-...-[0-9a-fA-F]{12}` in decoded definitions, all embedded references are found without type-specific parsing logic.
- **Items without definition support must be skipped**: SQLEndpoint, Dashboard, Datamart, PaginatedReport, MLModel, MLExperiment never support `getDefinition` (always return errors). Skipping them saves 20% of LRO calls in deep mode.
- **Type-specific endpoints expose richer properties**: Items fetched via their type-specific GET (e.g., `/kqlDatabases/{id}` vs `/items/{id}`) include a `properties` object with parent references, connection strings, and status fields not available from the generic items endpoint.
- **Workspace IDs appear frequently in definitions**: ~30% of definition-discovered edges are `workspace_ref` — notebooks and agents embed their workspace ID in metadata (trident, datasource configs). These are informational rather than item-to-item edges.
- **Relationship classification by file path/content**: The definition file path and content context determine the semantic relationship type (e.g., `definition.pbir` → `bound_to_model`, `default_lakehouse` in content → `default_lakehouse`, `ExecutePipeline` → `executes`).
- **Well-known GUIDs must be excluded**: All-zeros, all-f's, and near-zero GUIDs (`00000000-0000-0000-0000-00000000000X`) are placeholder values that should not be treated as item references.
- **`bulkExportDefinitions` is documented but insufficient for context tenant**: The API is documented at `learn.microsoft.com/rest/api/fabric/core/items/bulk-export-item-definitions(beta)`. Correct format: `POST /workspaces/{ws}/items/bulkExportDefinitions?beta=True` with body `{"mode":"All"}` or `{"mode":"Selective","items":[{"id":"<uuid>"}]}`. Response: `{"itemDefinitionsIndex":[{"id","rootPath"}],"definitionParts":[{"path","payload","payloadType"}]}`. However, it only exports items the caller has **read+write** permissions for (vs `getDefinition` which works with read-only). Benchmarked: bulk exported 14/154 items (55 edges) vs per-item 35/154 items (88 edges). The per-item approach is preferred for context tenant because completeness matters more than speed — missing `bound_to_model`, `queries`, `streams_to` edges means incomplete dependency graphs. The 2x speed gain (2m vs 4m) does not justify losing 38% of relationships.
- **Parallel workspace listing is safe**: Concurrent `GET /workspaces/{ws}/items` calls (one per workspace) do not trigger rate limiting on typical tenants (tested with 20 concurrent calls).
- **LRO polling is the deep mode bottleneck**: Each `getDefinition` LRO takes 2-6 seconds (POST → 202 → poll at 2s intervals). With 8 concurrent slots and 123 items: ~4 minutes total. Wall-clock time is dominated by server-side processing, not client overhead.
- **Performance benchmarks (20 workspaces, 154 items)**: Shallow mode: 7.7s. Deep + connections: 4 min 18s. Output size: 55-57 KB. Graph: 154 nodes, 88 edges, 10 relationship types.
- **`--no-properties` skips type-specific GETs**: Only calls `GET /workspaces/{ws}/items` (listing) — no per-item detail fetching. Nodes have `id`, `type`, `name`, `workspaceId`, `workspaceName` but no `properties`. Edges are limited to what can be discovered without properties. Useful for fast initial orientation (~3s for 20 workspaces).
- **`--output-file` writes JSON envelope to disk**: Writes `{"data": {...}}` envelope (pretty-printed) to the specified path. Reports `{"status":"written","file":"...","nodes":N,"edges":N,"workspaces":N}` to stdout. Parent directories must exist.
- **`--merge` enables incremental graph building**: Loads an existing graph JSON file, extracts the new workspace(s), and merges results. Merge semantics: nodes are deduped by ID (new overwrites old), edges are unioned (exact match dedup), workspaces are deduped by ID (new overwrites old). Summary is recomputed from the merged data. Supports both `{"data":{...}}` envelope format and bare graph object.
- **Incremental workflow pattern**: (1) `--no-properties --output-file g.json` for fast inventory, (2) `--deep --merge g.json --output-file g.json` to deepen a specific workspace, (3) repeat step 2 for additional workspaces. Re-extracting the same workspace with `--merge` updates it in place (idempotent).
- **Merge is idempotent**: Extracting the same workspace twice with `--merge` produces the same graph as extracting it once. New nodes overwrite old nodes with the same ID, so re-extraction captures any name/description/property changes.
- **`--format jsonld` produces RDF-compatible output**: JSON-LD format with `@context` vocabulary (`https://api.fabric.microsoft.com/ontology/`) and `@graph` array. Items become typed resources (`@id: urn:fabric:item:{uuid}`, `@type: fabric:{ItemType}`). Edges are inlined as typed link properties on source nodes (e.g., `fabric:defaultLakehouse: {"@id": "urn:fabric:item:{target}"}`). Workspaces are separate resources (`urn:fabric:workspace:{uuid}`). The output is simultaneously valid JSON (agents consume as-is) and valid RDF (importable into Neptune, Stardog, Jena, or any SPARQL endpoint via standard JSON-LD parsers). No external RDF crate needed — pure `serde_json` construction.

## Profile System

Named profiles store per-environment default settings, eliminating repetitive flags. Implements Agent-Native Principle 9 (persistent identity through profiles).

### Storage
- File: `~/.fabio/profiles.json`
- Unix permissions: directory `0700`, file `0600` (atomic write avoids TOCTOU)
- Windows: standard file write (DPAPI encryption is for token cache only, not profiles)

### Configurable Fields

| Field | CLI flag on `profile save` | Env var injected | Effect |
|-------|---------------------------|-----------------|--------|
| `workspace` | `--workspace <ID>` | `FABIO_WORKSPACE` | Default workspace for all workspace-scoped commands |
| `capacity` | `--capacity <ID>` | *(none)* | Default capacity ID for capacity operations |
| `output` | `--default-output <fmt>` | `FABIO_OUTPUT` | Default output format (json, table, plain, csv, tsv) |
| `private_link_workspace` | `--private-link-workspace <ID>` | *(none)* | Routes all Fabric/OneLake API calls through private link URLs |

### Precedence Chain

Defaults are injected at the **lowest priority** — any explicit source wins:

```
CLI flag (--workspace X)  >  env var (FABIO_WORKSPACE)  >  active profile value  >  clap default
```

Profile values are injected by setting env vars **before** clap parses arguments (in `main.rs`). Clap's `env = "FABIO_..."` attributes pick them up as fallbacks.

### Commands

```bash
fabio profile save --name <NAME> [--workspace <ID>] [--capacity <ID>] [--default-output <FMT>] [--private-link-workspace <ID>]
fabio profile use --name <NAME>       # Set active profile
fabio profile list                    # List all profiles (shows active marker)
fabio profile show --name <NAME>      # Show profile details
fabio profile delete --name <NAME>    # Delete a profile (supports --dry-run)
```

### Global Flag

`--profile <NAME>` on any command overrides the active profile for that single invocation:

```bash
# Active profile is "dev", but this command uses "prod" defaults
fabio lakehouse list --profile prod
```

### Private Link Routing

When `private_link_workspace` is set, the `FabricClient` transforms URLs:
- `https://api.fabric.microsoft.com/v1/...` → `https://<ws-id>-api.privatelink.analysis.windows.net/v1/...`
- `https://onelake.dfs.fabric.microsoft.com/...` → `https://<ws-id>-onelake.dfs.fabric.microsoft.com/...`
- `https://onelake.blob.fabric.microsoft.com/...` → `https://<ws-id>-onelake.blob.fabric.microsoft.com/...`

This enables fabio to work in environments where public Fabric endpoints are blocked and only private link access is permitted.

### Notes
- Profiles do NOT store credentials — authentication is managed separately via `fabio auth login`
- `save` overwrites all fields (not merge) — omitted fields become `null`
- `delete` removes the profile; if it was active, `active` is cleared
- Profiles are NOT authentication identities — switching profiles does not change the authenticated user/service principal

## Sensitivity Labels API Behaviors Discovered

### Read Behavior (Items API)

- **Inline in responses**: The `sensitivityLabel` field is returned on all items that have a label assigned, as part of the standard item response. No special `include` parameter is needed.
- **Field shape**: `"sensitivityLabel": {"id": "<uuid>"}` — only the label UUID is returned, never the human-readable name.
- **Absent when unset**: Items without a sensitivity label omit the `sensitivityLabel` field entirely (not `null`, just absent).
- **Works on all item endpoints**: Both `GET /workspaces/{ws}/items` (list) and `GET /workspaces/{ws}/items/{id}` (show) return the field. Type-specific endpoints (e.g., `/workspaces/{ws}/lakehouses`) also return it.
- **No admin required for reading**: Any workspace Viewer role can see sensitivity labels on items they have access to.
- **`ItemIncludeOption` does NOT include sensitivity labels**: The only valid `include` value is `DefaultIdentity`. Sensitivity labels are always returned by default — no opt-in needed.

### Write Behavior (Create)

- **Set at creation time**: The Create Item API accepts `sensitivityLabelSettings` in the request body: `{"sensitivityLabelSettings": {"sensitivityLabelId": "<uuid>"}}`. This works on all item types.
- **Cannot update via PATCH**: The Update Item API (`PATCH /workspaces/{ws}/items/{id}`) only accepts `displayName` and `description`. There is no way to change a sensitivity label on an existing item via the workspace-scoped API.
- **Requires Purview configuration**: Setting a label requires Microsoft Purview Information Protection configured in the tenant, M365 E5 licensing, and the label published to the calling user via label policy.
- **Error on invalid label**: If the label UUID doesn't exist or isn't in the user's label policy, the API returns a descriptive error (not a generic 400).

### Admin Bulk Operations

- **Bulk set**: `POST /admin/items/bulkSetLabels` — sets a label on up to 2,000 items per request. Returns per-item status (`Succeeded`, `Failed`, `NotFound`, `InsufficientUsageRights`, `FailedToGetUsageRights`).
- **Bulk remove**: `POST /admin/items/bulkRemoveLabels` — removes labels from up to 2,000 items per request. Same status response shape.
- **Rate limit**: Maximum 25 requests per hour for both bulk endpoints.
- **Admin role required**: Both require Fabric Administrator role. Service principals are NOT supported — only user principals.
- **Delegated principal**: `bulk-set-labels` supports a `delegatedPrincipal` field to set labels on behalf of another user (the delegated user is marked as the label issuer).
- **Assignment method**: `Standard` (automated/default) or `Priviledged` (manual assignment by admin). Note: the API misspells "Privileged" as "Priviledged" — use the misspelled value.
- **Auto-cascades to linked items**: Labels set on Lakehouse, Warehouse, Datamart, SQLDatabase, or MirroredDatabase automatically cascade to their auto-provisioned linked items (e.g., SQLEndpoint).

### Label Name Resolution (NOT in Fabric API)

- **Fabric API only returns UUIDs**: There is no Fabric REST API endpoint to list available sensitivity labels or resolve UUIDs to names.
- **Label definitions live in Microsoft Purview**: Names, descriptions, priority, and classification levels are managed in Purview Information Protection, not Fabric.
- **Resolution via `fabio label list`**: Queries Microsoft Graph (`GET /beta/security/informationProtection/sensitivityLabels`) and returns all labels with `id`, `name`, `description` fields. Uses a dedicated Graph token (scope: `https://graph.microsoft.com/.default`).
- **Alternative via `az rest`**: `az rest --method GET --url "https://graph.microsoft.com/beta/security/informationProtection/sensitivityLabels" --resource "https://graph.microsoft.com"` — equivalent but requires manual token handling.
- **No admin role needed for listing**: Reading label definitions from Graph only requires `InformationProtection.Read` scope — not a Fabric Administrator role.
- **Requires M365 E5 + Purview**: The Graph endpoint returns 403 if the tenant does not have Microsoft Purview Information Protection configured or the user lacks the appropriate license.
- **Labels change infrequently**: The UUID→name mapping is stable (labels are rarely added/removed). Agents should cache the output of `fabio label list` rather than querying on every invocation.
- **Beta endpoint**: Uses `/beta/security/informationProtection/sensitivityLabels` because the v1.0 segment is not yet GA. Env override: `FABIO_GRAPH_SCOPE`.

### Agent Guardrail Pattern

The complete workflow for an AI agent implementing sensitivity-label-based guardrails:

1. **Get label mapping** (one-time or periodic): `fabio label list --query "data[].{id:id, name:name}"` → cache the UUID→name map
2. **Read item labels** (per operation): `fabio item list -w $WS -o json --query "data[?sensitivityLabel]"` or `fabio <type> show --id $ID --query "data.sensitivityLabel.id"`
3. **Classify**: Cross-reference the label UUID with the cached map to determine classification level
4. **Decide**: Block, escalate, or proceed based on classification (e.g., block writes to "Highly Confidential" items)
5. **Audit unlabeled**: `fabio item list -w $WS -o json --query "data[?!sensitivityLabel]"` to find governance gaps
6. **Remediate** (admin only): `fabio admin bulk-set-labels --content '{"items":[...],"labelId":"<uuid>"}'`

### fabio CLI Support

- **Label resolution**: `fabio label list` resolves UUIDs to names via Microsoft Graph (no az rest needed)
- **All create commands**: `--sensitivity-label <uuid>` flag on every item-type create command (50 commands total)
- **All list commands**: Dynamic `SENSITIVITY LABEL` column in table output when any item in the response has a label
- **JSON output**: Full `sensitivityLabel` object passes through in all JSON responses
- **JMESPath queries**: Filter by label presence (`data[?sensitivityLabel]`) or absence (`data[?!sensitivityLabel]`)
- **Admin bulk operations**: `fabio admin bulk-set-labels` and `fabio admin bulk-remove-labels`
- **Best-practice guide**: `fabio context best-practices sensitivity-labels` provides governance patterns and agent guardrail guidance
