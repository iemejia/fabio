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
- **`preserve_order` feature (LOAD-BEARING — release-only regression, fixed in 0.49.1)**: `normalize_data_binding()` round-trips each DataBinding through `serde_json::Value` to force `sourceType` first in `sourceTableProperties`. This depends on `serde_json`'s `preserve_order` feature. In 0.49.0 the feature was declared ONLY in `[dev-dependencies]`, so `cargo test` had it (unit tests passed) but the SHIPPED release binary did NOT — `Value` fell back to a `BTreeMap` that alphabetized keys, pushing `sourceType` into 4th position (`itemId, sourceSchema, sourceTableName, sourceType, workspaceId`). Result: EVERY `ontology create/update-definition/import` carrying DataBindings failed live with the generic `ALMOperationImportFailed`, while a byte-identical raw `updateDefinition` POST succeeded. Root-caused by bisecting the definition (entities-only ✓, +relationships ✓, +any DataBinding ✗) and reproducing with an alphabetized binding. Fix (0.49.1): move `preserve_order` into `[dependencies]` so release builds match tests; added a byte-level wire-order regression test (`normalize_data_binding_wire_order_puts_source_type_first`) and load-bearing comments in `Cargo.toml` + `normalize_data_binding`. Lesson: a feature enabled only via dev-dependencies is invisible to `cargo test` failures — never rely on dev-dep feature unification for shipped behavior.
- **`ontology create --dir` diagnostic parity (0.49.1)**: `create` now also runs `enrich_ontology_definition_error` (previously only `import`/`bind`/`update-definition` did), so a definition rejected at create time surfaces the ranked self-correction checklist instead of the raw `{0} {1} {2}` template.
- **Failed ontology `create` leaks backing items (live-confirmed)**: Fabric auto-provisions a backing Lakehouse (`<name>_lh_<hash>`), its SQLEndpoint, and a GraphModel (`<name>_graph_<hash>`) for each ontology. When the definition import fails, the Ontology item itself is rolled back (it does NOT appear in `ontology list`) but these child items are left behind as orphans. Deleting a SUCCESSFULLY created ontology cascades and removes them; a failed create requires manual cleanup of the `_lh_*`/`_graph_*` items.
- **UNTYPED properties are NOT bindable (live-confirmed)**: A `DataBinding` whose `propertyBindings` targets an untyped property (`valueType: "Any"`, i.e. an `untypedProperties[]` entry) is rejected at `updateDefinition` with the generic `ALMOperationImportFailed`. Untyped properties must appear ONLY under `entityType.untypedProperties` and never in a binding. `fabio import` excludes them from `binding_props` automatically (`generate_fabric_parts`); hand-authored parts must do the same. Isolated by elimination — it is NOT caused by `.platform`, part ordering, `$schema`, Documents, or a missing table.
- **`ALMOperationImportFailed` is a generic, low-signal error**: The top-level `error.message` is frequently an unfilled template (`"{0} {1} {2}"`). The actionable detail lives in `error.errorCode` and the `error.moreDetails[]` array. fabio now flattens all three into one message (`lro_failure_message` in `client.rs`) and `enrich_ontology_definition_error` attaches a ranked self-correction checklist. Likely causes, in order: (1) an untyped property in a binding, (2) a binding/contextualization referencing an `entityTypeId`/`propertyId`/`relationshipTypeId` not defined in the same push (IDs are case-sensitive), (3) a TimeSeries binding missing `timestampColumn`, (4) a malformed part (bad `$schema`, missing required field, or a Contextualization whose `sourceEntityTypeId`/`targetEntityTypeId` don't match the relationship's endpoints).
- **No table/column existence validation at `updateDefinition` (live-confirmed)**: The server does NOT verify that a bound Lakehouse/Kusto table or its columns exist when the definition is pushed — validation is deferred to query time. A binding to a non-existent table imports successfully. Therefore a missing table is NEVER the cause of `ALMOperationImportFailed`, and import success does not imply the ontology is queryable.
- **`.platform` and cross-part ordering are not required (live-confirmed)**: `updateDefinition` accepts a definition with no `.platform` part, and parts may be in any order relative to each other (entity defs interleaved with bindings, or all defs first — both work). Only key order WITHIN a single DataBinding's `sourceTableProperties` matters (`sourceType` first).
- **`updateMetadata=true` REQUIRES a `.platform` part (live-confirmed)**: While a plain `updateDefinition` needs no `.platform` (above), passing it with `?updateMetadata=true` (fabio's `--update-metadata`) is rejected with `InvalidInput: UpdateMetadata is true but .platform file was not provided` when the definition has no `.platform` part. Since `fabio ontology import` never generates a `.platform`, an import-generated directory must be pushed WITHOUT `--update-metadata`. fabio now fails fast locally (`ensure_platform_when_updating_metadata`) with an actionable hint instead of round-tripping the whole upload to discover this server-side.
- **TimeSeries binding requires `timestampColumn` (live-confirmed)**: Convention bind-all (`import`/`bind` with no per-entity timestamp) fails for any time-series entity: `"Entity 'X' has a TimeSeries binding but no timestampColumn"`. Supply `timestampColumn` via the `--entities` map / bind config, or model the entity as non-time-series.
- **Multiple DataBindings per entity are supported (live-confirmed)**: One entity type can carry several bindings (e.g. a static `NonTimeSeries` Lakehouse table plus a telemetry `TimeSeries` Eventhouse/Kusto table); each round-trips through `getDefinition`.
- **KustoTable/Eventhouse TimeSeries bindings work end-to-end (live-confirmed)**: A `TimeSeries` binding with `sourceType: KustoTable` (Eventhouse `itemId` + `clusterUri` + `databaseName` + `timestampColumnName`) is accepted and round-trips.
- **Capacity + preview requirement**: Ontology items need a capacity with the Ontology / Digital Twin Builder preview enabled. Each `create`/`import`/`getDefinition`/`updateDefinition` is an LRO taking ~60-100s in practice — budget accordingly in tests (the e2e suite is `#[ignore] + #[serial]`).
- **Export is a lossy subset of import (verified in source)**: `ontology export` (`fabric_definition_to_model` → `serialize_to_rdf_xml`/`serialize_to_jsonld`) emits entity types, datatype properties, relationships, and the property annotations `ont:isIdentifier` / `ont:isTimeSeries` / `ont:isUntyped` / `ont:propertyType`. It does NOT emit entity inheritance (`baseEntityTypeId` → `rdfs:subClassOf`), entity Documents/Overviews/ResourceLinks, or data bindings/contextualizations — the `OwlClass` model only carries uri+label and the exporter never reads those parts. Import is the richer direction (it parses `rdfs:subClassOf` into `baseEntityTypeId` and generates Documents/ResourceLinks/bindings), so a Fabric→RDF→Fabric round-trip drops inheritance and metadata unless the original RDF (or the binding map's `baseEntityType`) resupplies them.

## OneLake API Behaviors Discovered
- **Shortcut transformations (CSV→Delta only via REST; Parquet/JSON/Excel + AI are portal-only) — live-confirmed**: [Shortcut transformations](https://learn.microsoft.com/en-us/fabric/onelake/shortcuts/transformations) convert structured source files referenced by a shortcut into a queryable Delta table (Fabric Spark polls the source ~every 2 min and keeps it in sync). The docs describe a portal flow, but the shortcut **create** REST body accepts an optional top-level `transform` object — live-verified: `POST .../shortcuts` with `transform` returns 200/201 and echoes it back. **Only `csvToDelta` is in the public API** — the [Create Shortcut REST reference](https://learn.microsoft.com/en-us/rest/api/fabric/core/onelake-shortcuts/create-shortcut) defines exactly one transform (`CsvToDeltaTransform`); Parquet/JSON/Excel and the AI-powered transforms (summarization/translation/sentiment/PII/name-recognition) are portal-only (no REST surface). Exact shape (live-confirmed, a **type-discriminated flat object**, NOT keyed like `target`): `transform: {"type": "csvToDelta", "includeSubfolders": <bool, default false>, "properties": {"delimiter": <"," default; one of , ; \t | & space>, "useFirstRowAsHeader": <bool, default true>, "skipFilesWithErrors": <bool, default true>}}`. `fabio lakehouse create-shortcut` gained `--transform csvToDelta` + `--csv-delimiter`/`--csv-no-header`/`--csv-keep-error-files`/`--transform-include-subfolders`, plus a raw `--transform-json` escape hatch (future-proofs for new transform types). A portal-only type (`--transform parquet|json|excel|...`) is rejected client-side with a "not available via the REST API" hint. Pure `build_transformation`/`normalize_transform_type` in `src/commands/lakehouse/shortcuts.rs` are unit-tested; the create-echo + rejection paths are live-validated (`lakehouse_create_shortcut_with_csv_transform`, `lakehouse_create_shortcut_transform_parquet_rejected`). NOTE: `GET .../shortcuts/{path}/{name}` and the list endpoint do NOT project the `transform` field back (it only appears in the create response); create the shortcut under `Tables` for a table shortcut. Materialization is an async Fabric Spark job whose documented sources are EXTERNAL (ADLS/S3/…); a OneLake→OneLake transform is accepted by the API but did not materialize a Delta table in testing (the source of the shortcut for a transform is normally an external file folder).
- **OneLake Shortcuts (list + typed targets, live-confirmed against MS Fabric MCP)**: `GET /v1/workspaces/{ws}/items/{id}/shortcuts` lists shortcuts (paginated `value`/`continuationToken`, optional `?parentPath=` to scope under a folder) — `fabio lakehouse list-shortcuts`. The list can be flooded by **DW-managed shortcuts** (internal OneLake→OneLake references that Warehouse/SQL endpoints auto-create under `Tables/…`), so fabio hides them by default (heuristic: path is/starts-with `Tables` AND target is `oneLake`) and shows them with `--include-managed`, matching Microsoft's Fabric MCP server. Shortcut **create** (`POST .../shortcuts`) takes a `target` object keyed by a discriminator (NOT a `type` field — the `type` field is only present in GET responses and must NOT be sent). The nine discriminators and their exact bodies (from the MCP's `ShortcutModels.cs`, live-confirmed for OneLake): `oneLake` `{workspaceId,itemId,path,connectionId?}`; `adlsGen2`/`amazonS3`/`azureBlobStorage`/`googleCloudStorage` `{location,subpath?,connectionId}`; `s3Compatible` `{location,subpath?,connectionId,bucket}`; `dataverse` `{environmentDomain,deltaLakeFolder?,connectionId}`; `externalDataShare` `{connectionId}`; `oneDriveSharePoint` `{location,subpath?,connectionId,updateFabricItemSensitivity?}`. `fabio lakehouse create-shortcut` builds these from typed flags (validating required fields per type) and normalizes/validates `--target-type` (aliases + any case), with a raw `--target` JSON escape hatch, plus an optional `transform` (see the shortcut-transformations note above). Shortcut-cache reset is workspace-scoped (`POST /workspaces/{ws}/onelake/resetShortcutCache` = `fabio workspace reset-shortcut-cache`); there is no per-shortcut reset endpoint.

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
- **Preview runtime toggle (Advanced NL2SQL for SQL sources) (live-confirmed)**: The runtime a data agent uses for its built-in NL2SQL/NL2KQL/NL2DAX tools is a nested boolean in staging settings: `experimental.enableExperimentalFeatures`. Absent `experimental`/empty object/flag absent = standard runtime (GA NL2SQL); `true` = preview runtime (Advanced NL2SQL, multi-step reasoning over the SAME SQL-source config — schema selection, data source instructions, example queries). It is toggled by `PATCH .../staging/settings` with `{"experimental": {"enableExperimentalFeatures": true|false}}` — `fabio data-agent update-config --enable-preview-runtime|--disable-preview-runtime`. **The flag is NESTED under `experimental`** — a top-level `{"enableExperimentalFeatures": true}` is silently accepted (200) but ignored (not persisted, not echoed). `GET .../staging/settings` echoes the `experimental` object back once set (a fresh agent's settings return only `{"aiInstructions": null}` — no `experimental` key). fabio does a read-modify-write on toggle so it preserves sibling keys the server owns under `experimental` (e.g. `mcpServers`); only the one flag flips. This mirrors the Fabric data agent Python SDK's `update_configuration(enable_preview_runtime=...)` (formerly `enable_preview_features`/`enable_experimental_features`) which writes the identical JSON key `experimental.enableExperimentalFeatures`. The runtime a *published* agent uses is fixed at publish time (republish to change it). `fabio data-agent get-config` surfaces it as a top-level `previewRuntime` boolean; `update-config` echoes the effective value back as `previewRuntime`. Runtime selection does NOT change which LLM the agent uses (model upgrades apply to both runtimes) and does NOT gate which data sources you can add.
 - **SQL sources (Lakehouse, Warehouse, SQL Database, Mirrored Database) are the `fabricItemType` values `Lakehouse`/`Warehouse`/`SQLDatabase`/`MirroredDatabase`**: all four are added with `add-datasource` and configured identically — schema selection via `select-tables` (Tables/Views/Functions via `--elements`), data source instructions via `update-datasource --instructions`, data source description via `update-datasource --description`, and example queries via the fewshots commands. Both NL2SQL (standard runtime) and Advanced NL2SQL (preview runtime) consume this same per-source config; no reconfiguration is needed to switch runtimes.
 - **NL2DAX over a Semantic Model works, but the source's TABLES are NOT auto-selected (live-validated)**: Adding a `SemanticModel` source (`add-datasource --artifact-type SemanticModel`) enables the agent's built-in NL2DAX tool, and querying a published agent grounded on it generates + executes real DAX. Live-verified end-to-end: a question like "count stores per region" produced a run step `analyze.database.nl2code` (`datasource_type: "SemanticModel"`) emitting `EVALUATE SUMMARIZECOLUMNS('DimStore'[Region], "Store Count", COUNTROWS('DimStore')) ORDER BY 'DimStore'[Region]`, executed via `analyze.database.execute`, returning the real per-region counts. **CRITICAL gotcha**: `add-datasource` auto-selects the semantic model's COLUMNS but leaves each TABLE `"selected": false`. With no selected table the agent has no schema in scope and **HALLUCINATES** a plausible-but-fake answer (observed: it invented US retail stores instead of the model's real EU data) — it does NOT do NL2DAX. You MUST `select-tables --datasource <modelId> --tables <TableName>` (then re-`publish`, since published config is a snapshot) for the agent to generate DAX. `list-elements --datasource <modelId>` shows `type:"Table"`/`"Column"` with `selected` state. Validated by `dataagent_semantic_model_nl2dax_lifecycle` (gated on `FABIO_TEST_SEMANTIC_MODEL`, a model WITH data). Same select-tables requirement likely applies to all sources, but is most impactful for semantic models (bare columns aren't enough).
 - **NL2KQL over a KQL database works; `select-tables` was BROKEN for KQL sources (bug fixed + live-validated)**: Adding a `KQLDatabase` source (`add-datasource --artifact-type KQLDatabase`) enables the agent's NL2KQL tool. Live-verified end-to-end: "total Amount per Region" produced a run step `analyze.database.nl2code` emitting `Sales | summarize TotalAmount=sum(Amount) by Region`, executed (`analyze.database.execute` / `trace.analyze_kusto_database`), returning the real per-region totals. **Bug found & fixed**: a KQL database's staging elements are NOT flat — the tables are nested under grouping containers whose `type` is `"Tables"` (and siblings `"Functions"`/`"Shortcuts"`/`"MaterializedViews"`), unlike Lakehouse/Warehouse (`"Schema"`) or SemanticModel (flat). `select-tables` only drilled into `"Schema"`/`"Schemas"` containers, so `--tables Sales` returned "No matching elements found" and the table stayed unselected → the agent hallucinated. Fixed by recognizing the KQL grouping containers in `is_grouping_container` (`select_tables`) AND by walking the element tree breadth-first through EVERY container level (not just one) to reach the nested `Table` leaves — see the multi-level select-tables fix below. Unit-tested (`is_grouping_container_recognizes_schema_and_kql_containers`) + self-seeding live e2e `dataagent_kql_database_nl2kql_lifecycle` (creates an eventhouse + KQL DB + table, runs the full NL2KQL flow). **NL2 language matrix (all live-validated now)**: NL2SQL (`Lakehouse`/`Warehouse`/`SQLDatabase`/`MirroredDatabase`), NL2KQL (`KQLDatabase`), NL2DAX (`SemanticModel`); the source `fabricItemType` picks the language and the agent generates the query (`analyze.database.nl2code`). Graph sources (`Ontology`/`GraphModel`) ground via Fabric IQ, not NL2SQL/KQL/DAX.
  - **NL2SQL over a Lakehouse: `add-datasource` discriminator is source-specific — Lakehouse needs `LakehouseTables`, NOT `FabricItem` (bug fixed + live-validated)**: This CORRECTS the earlier "IDENTICAL FabricItem body for every source" claim. The datasource API's `type` discriminator is source-specific: a **Lakehouse** MUST be sent as `{type:"LakehouseTables", lakehouseReference:{itemId,workspaceId}}` — sending it as a `FabricItem` (fabio's old behavior for ALL sources) fails schema discovery with `BadRequest: Failed to fetch schema for the data source`, because a Lakehouse *item* is not itself the SQL database (its tables live on a separate auto-provisioned SQL analytics endpoint). Every OTHER source — `Warehouse`, `SQLDatabase`, `MirroredDatabase`, `KQLDatabase`, `SemanticModel`, `GraphModel`, `Ontology` — is correctly `{type:"FabricItem", itemReference:{itemId,workspaceId}, fabricItemType:<Type>}` (the item IS the queryable surface). Fixed in `build_add_datasource_body` (`src/commands/dataagent/datasources.rs`, unit-tested: `build_add_datasource_body_uses_lakehousetables_for_lakehouse`, `..._uses_fabricitem_for_non_lakehouse`). A `LakehouseTables` datasource's response reports `fabricItemType: null`, `type: 1`, and a populated `lakehouseReference` (so success is asserted via `status: datasource_added`, not `fabricItemType`). Live-validated end-to-end (`dataagent_lakehouse_nl2sql_lifecycle`, gated on `FABIO_TEST_LOADED_LAKEHOUSE`): add-datasource → `select-tables --all-tables` (modified:3) → publish → query "how many rows in factsales" → grounded "6 rows". All four NL2SQL sources share ONE parameterized e2e (`run_nl2sql_source_lifecycle` + wrappers `dataagent_{lakehouse,warehouse,sql_database,mirrored_database}_nl2sql_lifecycle`, each gated on `FABIO_TEST_LOADED_{LAKEHOUSE,WAREHOUSE,SQLDATABASE,MIRRORED_DATABASE}`): **Lakehouse, Warehouse, and SQLDatabase are all live-validated end-to-end** (Warehouse grounded per-Region totals; SQLDatabase grounded Products). Two provisioning caveats: a freshly-created Warehouse's schema-discovery LRO is timing-sensitive (~60-180 s), so the fixture must be a pre-provisioned warehouse; and **MirroredDatabase could not be live-validated — the test tenant has zero mirrored databases** (one requires an external mirror source — Snowflake/Cosmos/Azure SQL — actively replicating), so its wrapper skips (its `FabricItem` body is byte-identical to the validated Warehouse/SQLDatabase, differing only by the `fabricItemType` string, and is covered by the unit test).
  - **`select-tables` drills ONLY one level → misses schema-nested tables (bug fixed + live-validated)**: A Lakehouse/Warehouse SQL source nests its tables THREE containers deep — `Schemas` (container) → `Schema` e.g. `dbo` (container) → `Tables` (container) → `factsales` (`Table` leaf). fabio's `select-tables` drilled only ONE level, so it reached `Schema:dbo` (still a container) and stopped — `--tables factsales` returned "No matching elements found" and NL2SQL had no table in scope (the agent hallucinated). Fixed by rewriting the drill as a breadth-first walk (`VecDeque` worklist, `MAX_DEPTH` guard) that expands EVERY container level until the selectable leaves are reached. This also improves `--all-tables`/`--all-elements` (now selects leaves at any depth). Live-verified: `select-tables --all-tables` on OntoCompareLH selected all 3 dbo tables (`modified:3`).
  - **Data-agent SQL-source schema fetch is on-behalf-of the caller's token — fabio's OWN login token is rejected (auth gap, NOT a fabio code bug)**: When `add-datasource` targets a SQL-endpoint-backed source (Lakehouse/Warehouse/SQLDatabase/MirroredDatabase), the data-agent SERVICE fetches the source's SQL schema *on-behalf-of (OBO) the caller's Fabric token*. This OBO exchange to reach the SQL analytics endpoint succeeds for a broadly-trusted client app (an **Azure-CLI-issued** Fabric token — `az account get-access-token --resource https://api.fabric.microsoft.com`, and by extension `FABIO_ACCESS_TOKEN` set from it) but is REJECTED for fabio's own device-code/browser login token (`credential_source: fabio_cache`, the "Fabio CLI" public-client app), returning `BadRequest: Failed to fetch schema for the data source`. This is DETERMINISTIC by token, not a transient outage (verified: az token → succeeds attempt 1 repeatedly; fabio-cache token → fails every attempt over minutes). It does NOT affect non-SQL sources (`KQLDatabase`/`SemanticModel`/`GraphModel` schema fetch needs no SQL-endpoint OBO — those succeed with the fabio-cache token), which is why NL2KQL/NL2DAX were validatable with the default credential but NL2SQL was not. Root cause is an Azure AD OBO trust/pre-authorization gap between the "Fabio CLI" app and the data-agent service (the app already holds Azure SQL/Storage `user_impersonation` delegated permissions, so this is a service-side pre-authorization/knownClientApplications relationship, not a missing scope) — resolving it is a separate app-registration epic. **Workaround for NL2SQL today**: run fabio with an Azure-CLI Fabric token, e.g. `FABIO_ACCESS_TOKEN=$(az account get-access-token --resource https://api.fabric.microsoft.com --query accessToken -o tsv) fabio data-agent add-datasource ...`. The e2e test (`dataagent_lakehouse_nl2sql_lifecycle`) self-provisions this token via `az` and skips if unavailable.
 - **NL2GQL (graph sources): generation WORKS; the data-agent's own GQL execution currently fails server-side (live-validated, NOT a Fabric IQ issue)**: Adding a `GraphModel` source enables the agent's NL2GQL tool. Live-verified on a portal-LOADED graph: the source adds cleanly (`fabricItemType: GraphModel`), its **node types AUTO-select** (`selected: true` — unlike SQL/semantic-model tables which start unselected), and a query routes to `trace.analyze_graph`, which **translates NL into valid GQL** — e.g. "list DimStore StoreName values" produced `MATCH (node_DimStore:\`DimStore\`) RETURN node_DimStore.\`StoreName\` AS \`StoreName\``. So NL2GQL GENERATION is real and works (validated by `dataagent_graph_model_nl2gql_generation_lifecycle`, gated on `FABIO_TEST_LOADED_GRAPH_ID`). The current blocker is the data agent's server-side GQL EXECUTION: it reports `Failed to execute GQL: Unable to process the request`, even though the SAME generated query executes perfectly via fabio's direct `graph-model execute-query` (`executeQuery?preview=true`, status `00000`, returns the store names). This is a server-side data-agent execution gap, NOT Fabric IQ and NOT fabio (a corrected earlier note that wrongly attributed it to Fabric IQ; the earlier `UnableToGenerateGQL` was a prompt-specific generation miss, not a provisioning gate). Node-type scoping uses `select-tables --elements`.
- **Staging Datasources**: Full CRUD at `.../staging/datasources`. `POST` is LRO (triggers async schema discovery, 1-5 minutes). `PATCH` updates `instructions`/`description`. Datasource types: `FabricItem` (generic + `fabricItemType`) or `LakehouseTables`.
- **Supported fabricItemType values**: `Report`, `SemanticModel`, `Lakehouse`, `KQLDatabase`, `Warehouse`, `MirroredDatabase`, `MirroredAzureDatabricksCatalog`, `GraphModel`, `SQLDatabase`, `Ontology`.
- **Staging Elements**: `GET .../staging/datasources/{dsId}/elements` returns schema tree level-by-level via `?rootId=` parameter. `PATCH ...?id={elemId}` updates `isSelected`/`description`. `DELETE ...?id={elemId}` removes stale elements.
- **Element types**: `Root`, `Files`, `Directory`, `Schemas`, `Tables`, `Views`, `Functions`, `Schema`, `Table`, `ExternalTable`, `MaterializedView`, `View`, `Column`, `Measure`, `Function`, `NodeType`, `EdgeType`, `Entity`.
- **Element states**: `Available`, `NotAvailable`, `AccessDenied`, `AccessDeniedOap`, `DatasourceNotFound`, `SchemaUnavailable`.
- **Element index states**: `Indexed`, `Indexing`, `NotIndexed`.
- **Staging Fewshots**: Full CRUD at `.../staging/datasources/{dsId}/fewshots`. `POST .../fewshots/deleteAll` for bulk clear. Server-side validation returns `validationStatus` (`Validating`, `Valid`, `Invalid` + `reason`). NOT supported for SemanticModel/Ontology datasources.
- **Datasource creation is LRO**: `POST .../staging/datasources` returns 202 and triggers async schema discovery. Can take 1-5 minutes on cold lakehouses. Use `--lro-timeout 300` for reliable completion.
- **Published URL resolution**: `GET /workspaces/{ws}/dataAgents/{id}/settings` is the published-state probe (200 vs 404) but in practice does NOT return a `publishedUrl` field, nor does `properties.publishedUrl` on the item GET. fabio therefore constructs the canonical consumption URL as a fallback (see "Constructed published consumption URL" below).
- **Query protocol**: OpenAI Assistants API at the published URL (`{publishedUrl}/assistants`, `/threads`, `/messages`, `/runs`). Uses `?api-version=2024-05-01-preview`. Standard Fabric bearer token for auth.
- **M365 Copilot Agent Store publishing**: NOT available via public REST API. Only accessible through Fabric portal or `fabric-data-agent-sdk` Python package (internal workload endpoint).
- **Datasource ID resolution**: The staging API uses its own UUID for datasources. fabio resolves by matching `displayName`, datasource `id`, or `itemReference.itemId` (artifact ID) — all three work as `--datasource` input.
- **Schema discovery is asynchronous**: After `add-datasource`, schema elements may be empty for 1-5 minutes. The `list-elements` command will show elements once indexing completes (`indexState: "Indexed"`).
- **New scopes**: `DataAgent.Read.All` and `DataAgent.ReadWrite.All` (in addition to generic `Item.*` scopes).
- **Max 5 datasources per agent**: Official limit.
- **Max 100 fewshot examples per datasource**: Official limit.
- **Response cap**: Agent responses are capped at 25 rows and 25 columns maximum.
- **add-datasource server response is PascalCase; fabio normalizes it (live-confirmed)**: The staging `POST .../datasources` response object uses PascalCase keys (`FabricItemType`, `Id`, `DisplayName`, `ItemReference`) — unlike the camelCase `fabricItemType` used in the REQUEST body, the `list-datasources` response, and fabio's empty-LRO fallback. Because the shape otherwise depended on LRO timing (empty 202 → fabio's camelCase fallback; populated body → server's PascalCase), fabio now recursively lower-cases the first letter of every key (`camel_case_keys`) so the output contract is stable: agents always read `data.fabricItemType`.
 - **Ontology as a data-agent datasource (live-confirmed)**: `add-datasource --artifact-type Ontology` grounds an agent on an ontology; `select-tables --all-elements` (or `--elements`) selects its entity/relationship elements. Fewshots are NOT supported for Ontology (or SemanticModel) datasources.
 - **MCP endpoint is the canonical runtime/consumption surface (SDK-documented)**: A *published* data agent is consumed through its Model Context Protocol endpoint — `{fabricBase}/mcp/workspaces/{ws}/dataagents/{id}/agent` (note lowercase `dataagents`). External MCP clients (Claude, Copilot Studio, Azure AI Foundry, custom tools) connect there to ask questions. `fabio data-agent mcp-url` constructs this URL deterministically (honoring `FABIO_FABRIC_API_ENDPOINT`) and reports published state. The URL is distinct from the older OpenAI-Assistants endpoint (`.../aiassistant/openai`) still used by `data-agent query`.
 - **Reliable published-state detection (live-confirmed)**: The published-stage settings endpoint `GET /workspaces/{ws}/dataAgents/{id}/settings` returns `200` for a published agent and `404 DataAgentNotPublished` for a draft one. This status is the reliable "is published" signal — the response body does NOT contain a `publishedUrl` field (an earlier assumption), so detection must be by request success, not by field presence. `data-agent mcp-url` uses this and emits `published: false` + a publish hint when the agent is still a draft. The MCP endpoint only works after publishing.
 - **Constructed published consumption URL (live-confirmed)**: Because `GET .../settings` returns NO `publishedUrl` (verified: a published agent's settings body is just `{"aiInstructions": ...}`), fabio constructs the canonical OpenAI-Assistants consumption URL `{fabricBase}/workspaces/{ws}/dataagents/{id}/aiassistant/openai` (note the lowercase `dataagents`) as a last resort in `get_published_url`. This URL — on `api.fabric.microsoft.com`, NOT the SDK's internal workload host — serves the full Assistants API (`/assistants`, `/threads`, `/messages`, `/runs`, `/files`) for a *published* agent and was live-confirmed to answer queries. Previously `query` errored ("Published URL not found") unless the user passed `--published-url` manually; now `query`/`evaluate` work with just `--workspace`/`--id`. If the agent is not actually published, the first Assistants call 404s and `enrich_query_error` surfaces a publish hint.
 - **Multi-turn conversations via thread reuse (live-confirmed)**: The Assistants `thread` persists across calls. `data-agent query` now always returns the `threadId`; `--keep-thread` skips the best-effort thread deletion so the id can be reused, and `--thread-id <id>` posts the next message to that existing thread instead of creating a new one. Live-confirmed that a second, separate `fabio` invocation on the same thread recalls prior context (it correctly answered "7" to "what number did I ask you to remember?"). fabio only deletes a thread it created and only when `--keep-thread` was not set — a caller-supplied `--thread-id` is never deleted.
 - **Answer-attached files via the OpenAI files API (endpoint on the published base)**: When an answer attaches generated files, the file IDs appear in the assistant message under text-content `annotations` (type `file_path`/`file_citation`, `.file_id`), `image_file` content items (`.file_id`), and/or message-level `attachments` (`.file_id`). `data-agent query --download-files <dir>` extracts these, then downloads each via `GET {base}/files/{id}/content?api-version=2024-05-01-preview` (filename resolved best-effort from `GET {base}/files/{id}` `filename`, sanitized to a safe basename), writing them to `<dir>` and adding a `files` array to the output. Plumbing is live-validated (empty `files` array when no file is generated); the download path mirrors the SDK's `client.files.content(file_id)`.
  - **Query stage is production-only via the public API**: Only a *published* agent is reachable through the public `aiassistant/openai` endpoint. `data-agent query`/`evaluate` accept `--stage production|published` (default `production`); `--stage sandbox|staging` (draft) now fails fast with `INVALID_INPUT` + a publish-first hint (previously the flag was a silent no-op that queried production regardless). Querying the true draft/sandbox stage requires the SDK's internal workload host (`x-ms-ai-aiskill-stage: sandbox`), which has no public endpoint.
  - **Visual/chart responses ARE reachable via the run steps (`data-agent query --visuals`) — the docs' "portal-only" is only about the RENDERED image, not the spec (live-confirmed)**: The [data-agent visuals](https://learn.microsoft.com/en-us/fabric/data-science/data-agent-visuals) feature docs say visuals are "currently only supported in the data agent experience in Fabric and not in other clients like SDK, M365 Copilot, Teams, or Foundry". That is true for the RENDERED chart image and for the `report_specs_*.json` file the answer references — that file id (a `file_path` annotation on the assistant message) is a dangling reference: `GET {published}/files/{id}[/content]` returns `404 EntityNotFound` for it on the published Assistants base (every api-version tried: 2024-05-01/2024-07-01/2025-01-01-preview), so `--download-files` cannot fetch it (records `download failed with HTTP 404`). **HOWEVER**, the COMPLETE chart specification is reachable another way: when the agent decides to chart, it invokes an internal function tool named `*.VisualizeDataset` (both a `trace.VisualizeDataset` mirror AND the canonical `AIFunction.VisualizeDataset`), whose JSON-encoded `arguments` carry the entire spec — `chart_type` (one of the supported set: line/multi-line/column/multi-column/stacked-column/pie/scatter/area/stacked-area `_chart`), `title`, `x_column`, `y_columns[]`, `x_axis_title`, `y_axis_title`, `sort_by`, `sort_order`, AND the aggregated data as `inline_csv_data`. That tool call IS exposed by the run-steps endpoint (`GET {base}/threads/{tid}/runs/{rid}/steps`), which fabio already reads for `--show-steps`. `data-agent query --visuals` fetches the steps, extracts every `VisualizeDataset` call, de-duplicates the trace/canonical pair by content, attaches the referenced `reportSpecFile` name (parsed from the tool `output` text), and emits a `visuals[]` array of chart specs — so a client CAN reconstruct the chart (or hand the spec to a renderer) without the portal. The tool `output` itself confirms the intent: *"The file contains the JSON specifications for the requested visualization. Client will be able to render UI from this specification."* Live-validated end-to-end against a published Lakehouse-grounded agent: a bar-chart prompt → `chart_type: column_chart` + inline CSV; a trend prompt → `chart_type: line_chart` + `sort_by`/`sort_order`; a text-only prompt → empty `visuals[]`. Pure `extract_visuals`/`parse_report_spec_filename` in `src/commands/dataagent/query.rs` are unit-tested against the captured step shape; the full publish→query→extract loop is `dataagent_query_visuals_lifecycle` (gated on `FABIO_TEST_LOADED_LAKEHOUSE` + `az`). NOTE the run-steps shape fabio extracts from is the SIMPLIFIED one `retrieve_run_steps` produces (a flat array of `{type, name, arguments, output}` tool calls), not the raw OpenAI `{data:[{step_details:{tool_calls:[{function:{…}}]}}]}`.
 - **`data-agent evaluate` is a batch primitive, not a judge**: Runs a questions file (JSON array of strings or `{question,expected}` objects, or CSV/TSV with a `question` column and optional `expected`) against the published agent — each question on its own throwaway thread, `--repeats N` times — and emits the answers (`--show-steps` adds run steps). It deliberately does NOT perform semantic/LLM grading by default; when a question carries `expected`, it adds only a NAIVE case/whitespace-insensitive `match.exact`/`match.contains` signal, leaving real judgment to the calling agent. Fails only if EVERY run errors; otherwise returns partial results with `failedRuns > 0`. **Optional LLM grading**: pass `--llm-endpoint`/`--llm-key`/`--llm-model` (or `FABIO_LLM_*`) to grade each answer with a judge model — each answer then carries a `grade` (`{correct,score,rationale}`) and the summary adds `gradedRuns`/`passedRuns`/`passRate`. Grading is non-fatal: a judge error (e.g. Azure content-filter 400) is recorded as a per-answer `grade.error`.
 - **Responses API and M365 publishing are NOT on the public API (live 404 evidence)**: The SDK's data-plane OpenAI *Responses* client and its M365 Copilot Agent Store publishing both target the internal Fabric **workload host** (discovered via `synapse.ml.fabric.service_discovery` inside a notebook), not `api.fabric.microsoft.com`. Verified live against a published agent: `POST {published}/aiassistant/openai/responses` → `404 EntityNotFound` (the public endpoint serves the Assistants API — `/assistants`,`/threads`,`/runs` — but not `/responses`), and every candidate public path for the M365 package (`.../metaosapppackage`, `.../staging/publishToM365`, `.../publishToM365`, `.../staging/m365`) → `404 EntityNotFound`. Both are therefore unimplementable from an external CLI. `data-agent publish --to-m365` reports `m365Status: "unsupported"` with a message pointing to the portal or the notebook SDK; Responses adds nothing over fabio's existing thread-based multi-turn `query`.
 - **External LLM judge for agent-quality features (`validate-fewshots`, `evaluate --llm-*`)**: Fabio hosts no model — the caller supplies one via `--llm-endpoint`/`--llm-key`/`--llm-model` (env `FABIO_LLM_ENDPOINT`/`FABIO_LLM_KEY`/`FABIO_LLM_MODEL`; `--llm-api-version` / `FABIO_LLM_API_VERSION` defaults to `2024-10-21`). Flavor auto-detects from the endpoint host: a `*.azure.com` host → **Azure OpenAI** (`{endpoint}/openai/deployments/{model}/chat/completions?api-version=…` with the `api-key` header; `--llm-model` is the *deployment* name, not the base model), anything else → **OpenAI-compatible** (`{endpoint}/chat/completions` with `Authorization: Bearer`, model in the body). Live-validated against an Azure AI Foundry resource (`*.cognitiveservices.azure.com`, `gpt-5-mini` deployment, `api-version=2024-10-21`). NOTE: Azure OpenAI's content-management policy can 400-reject even benign grading/validation prompts (`code: content_filter`) — fabio treats this as a non-fatal per-item error rather than aborting.
 - **`validate-fewshots` supported/unsupported sources**: Reads the few-shots of a data source (`--stage staging|published`) and reviews them with the judge. Few-shots exist only for datasource types that support them (Lakehouse/Warehouse/SQL/KQL/Mirrored — NOT SemanticModel/Ontology), so validate-fewshots is only meaningful for those. It is read-only (never mutates the agent).


## Semantic Model API Behaviors Discovered
- **`semantic-model add-culture` / `delete-culture` / `set-translation` / `list-cultures` (translations / cultures via definition read-modify-write) — live-confirmed**: Multi-language translations live in `definition/cultures/<culture>.tmdl` and are `ref`-ed from `model.tmdl` — but the ref kind is **`ref cultureInfo <culture>`** (NOT `ref culture`), and the file's root object is **`cultureInfo <culture>`** (ground-truthed). The file is a nested translation TREE: `cultureInfo <c>` → `translations` → `model <ModelName>` → `table <T>` (`caption: <text>` directly under it) → `column <C>`/`measure <M>` (each with a nested `caption: <text>`). fabio parses that tree into a small in-memory model (`Culture`/`TableTr`), edits it, and RE-RENDERS the whole file — far more robust than line-editing the 4-to-5-level indentation. `add-culture --culture <name>` creates the file (reading the model name from `model.tmdl` via `model_name`) + the `ref cultureInfo` line; `delete-culture` removes both. `set-translation --culture --table [--column | --measure] --caption` sets/updates a translated caption (creating the table/column/measure node as needed; `--column` + `--measure` together → `INVALID_INPUT`; the culture must already exist → `NOT_FOUND` otherwise). `list-cultures` is READ-ONLY (`mutates:false`) and returns `[{culture, translationCount}]`. In `model.bim` the same data lives under `model.cultures[].translations.model.tables[].{translatedCaption, columns[], measures[]}` (the TMDL `caption:` == the TMSL `translatedCaption`). All mutations OVERWRITE the definition → `mutates:true`/`destructive:true`, `--dry-run` guarded; duplicate culture → `CONFLICT`. Pure helpers (`parse_culture`, `render_culture`, `apply_translation`, plus the `model.bim` variants) in `src/commands/semantic_model/translations.rs` are unit-tested (incl. a parse→render→re-parse round-trip); the create → add-culture → set-translation×2 → list → verify → delete loop is live-validated (`semantic_model_translation_lifecycle`). This realizes the MCP's "translate my model to French" scenario. `model_name` was added to the shared `tmdl.rs`.
- **`semantic-model add-table` / `delete-table` / `rename-table` (table lifecycle via definition read-modify-write, with cascade) — live-confirmed**: Tables are `definition/tables/<name>.tmdl` files, each `ref`-ed from `model.tmdl` (`ref table <name>`). `add-table --name --expression <DAX>` creates a CALCULATED table — `table <name>` + `partition <name> = calculated` / `mode: import` / `source = <DAX>` — plus the `ref table` line. It needs NO explicit columns: **Fabric AUTO-INFERS a calculated table's columns** (ground-truthed — a calculated table pushed with no columns came back from getDefinition with an inferred `column n` carrying `isNameInferred` + `sourceColumn: [n]`). `rename-table --new-name` does THREE things: rewrites the `table <old>` declaration, MOVES the part to the new file path (`tables/<old>.tmdl` → `tables/<new>.tmdl`), and updates the `ref table` in `model.tmdl` (DAX/relationship references are NOT rewritten — documented). `delete-table --name` removes the table file + its ref AND **CASCADES**: it removes every relationship in `relationships.tmdl` referencing the table (`remove_relationships_referencing_table` in relationships.rs) and strips `tablePermission <table>` lines from all role files (`cascade_remove_table_from_roles` in roles.rs) — this cascade is REQUIRED because `updateDefinition` rejects a model with a relationship pointing at a deleted table. The removed relationship ids + affected roles are reported in the output (and previewable via `--dry-run`). All three OVERWRITE the definition → `mutates:true`/`destructive:true`, `--dry-run` guarded; duplicate name → `CONFLICT`, unknown table → `NOT_FOUND`. Pure helpers (`render_calculated_table`, `rename_table_decl`, plus the `model.bim` variants which cascade `model.relationships[]`) in `src/commands/semantic_model/tables.rs` are unit-tested; the seed-relationship → add(calculated) → rename(file+ref) → verify → delete(+cascade) loop is live-validated (`semantic_model_table_lifecycle`).
- **`semantic-model add-calculated-column` / `delete-column` / `rename-column` / `update-column` (column authoring via definition read-modify-write) — live-confirmed**: Columns are `column <name>` blocks inside a table's `definition/tables/<T>.tmdl` — a DATA column has `dataType:`/`sourceColumn:` (indent 2); a CALCULATED column carries `= <DAX>` on its declaration line (`column 'Full Name' = [First] & " " & [Last]`) plus a `dataType:` (fabio requires/defaults it). `add-calculated-column --table --name --expression [--data-type/--format-string/--summarize-by/--display-folder/--description/--hidden]` inserts the block via the shared `insert_table_child_lines` (after the table's scalar properties — same indentation-safety rule as measures; `--data-type` normalized string/int64/double/decimal/dateTime/boolean, `--summarize-by` none/sum/count/min/max/average/distinctCount). `delete-column` removes the block (via the shared `child_span`); `rename-column --new-name` rewrites ONLY the declaration, preserving a calculated column's ` = expr` remainder (DAX/relationship references are NOT rewritten — documented); `update-column` sets properties in place (replacing an existing `dataType:`/`formatString:`/`summarizeBy:`/`displayFolder:`/`isHidden`, appending if absent). All OVERWRITE the definition → `mutates:true`/`destructive:true`, `--dry-run` guarded; duplicate column → `CONFLICT`, unknown column → `NOT_FOUND` (the handlers return a typed `FabioError::not_found`, NOT a generic `bail!`, since the TABLE exists — the missing COLUMN must still map to a NOT_FOUND code; a `bail!` would surface a generic code and fail an agent's error-code check). The reusable TMDL helpers (`is_child_object_decl`, `insert_table_child_lines`, `child_span`, `join_preserving_trailing_newline`) were promoted from `authoring.rs` to the shared `tmdl.rs` so measures and columns share one implementation. Pure editors (`build_calculated_column_lines`, `delete_column_tmdl`, `rename_column_tmdl`, `update_column_tmdl`, plus `model.bim` variants) in `src/commands/semantic_model/columns.rs` are unit-tested (6 tests); the add → duplicate(CONFLICT) → update → rename → verify → delete → missing(NOT_FOUND) loop is live-validated (`semantic_model_column_lifecycle`).
- **`semantic-model add-role` / `delete-role` / `set-rls` / `delete-rls` / `list-roles` (security roles + row-level security via definition read-modify-write) — live-confirmed**: Security roles live in `definition/roles/<name>.tmdl` (ONE file per role) and MUST be `ref`-ed from `model.tmdl` (`ref role <name>`) — unlike relationships (auto-discovered). A role file is `role <name>` + `modelPermission: read` (or none/readRefresh/refresh) + zero or more `tablePermission <Table> = <DAX predicate>` lines — the DAX predicate is the RLS filter. `add-role --name [--model-permission]` creates the role file AND adds the `ref role` line (via the shared `add_model_ref`); `delete-role` removes BOTH the file and the ref (via `remove_model_ref`). `set-rls --role --table --filter` adds/replaces a `tablePermission` line in the role file (an existing filter for that table is replaced, not duplicated); `delete-rls --role --table` removes it. `list-roles` is READ-ONLY (`mutates:false`) and returns `[{name, modelPermission, tablePermissions:[{table, filter}]}]`. This is DISTINCT from `add-user`/`list-users`, which grant dataset *permissions* to a principal (a Power BI API concept) — RLS roles are a MODEL concept (definition parts). All mutations OVERWRITE the definition → `mutates:true`/`destructive:true`, `--dry-run` guarded; duplicate role → `CONFLICT`, unknown role → `NOT_FOUND`. The `model.tmdl` `ref role` requirement was ground-truthed by creating a model.bim with a role and reading back the getDefinition TMDL (Fabric emitted `ref role WestOnly`). Pure helpers (`parse_role_tmdl`, `set_table_permission`, `remove_table_permission`, `collect_roles`, plus the `model.bim` variants) in `src/commands/semantic_model/roles.rs` are unit-tested; the create → add-role → set-rls → list → delete-rls → delete-role loop is live-validated (`semantic_model_role_lifecycle`). `add_model_ref`/`remove_model_ref` were added to the shared `tmdl.rs`.
- **`semantic-model add-relationship` / `delete-relationship` / `update-relationship` (relationship authoring via definition read-modify-write) — live-confirmed**: Relationships live in `definition/relationships.tmdl` as top-level `relationship <guid>` blocks — `fromColumn: Table.Column` + `toColumn: Table.Column`, with only NON-DEFAULT properties serialized (`isActive` default true, `crossFilteringBehavior` default `oneDirection`, `fromCardinality` default `many`, `toCardinality` default `one`). Crucially, the relationships file is **NOT `ref`-ed in `model.tmdl`** (unlike tables/roles) — it is auto-discovered, and it does not exist until the first relationship is added (fabio `upsert_part`s it, and `remove_part`s it when the last relationship is deleted). `add-relationship --from-table/--from-column/--to-table/--to-column [--cross-filter oneDirection|bothDirections|automatic] [--inactive] [--from-cardinality] [--to-cardinality]` generates a fresh v4 GUID id and appends a block. `delete-`/`update-relationship` match a block EITHER by `--relationship-id <guid>` OR by the full four-column tuple (`build_rel_spec` enforces "all four or an id"; a partial tuple → `INVALID_INPUT` offline); update sets `isActive`/`crossFilteringBehavior` in place (re-inserting after `toColumn:`). Column refs are quoted only when needed (`'Sales Fact'.'Net Amount'`; `parse_column_ref` handles the doubled-`''` escape). All three OVERWRITE the definition → `mutates:true`/`destructive:true`, `--dry-run` guarded; `updateDefinition` validates (a dangling column ref is rejected). Referenced tables are existence-checked before mutating (nice error). Pure helpers (`render_relationship_block`, `parse_relationship_blocks`, `parse_column_ref`, `remove_relationship_block`, `update_relationship_block`, `add_relationship_bim`) in `src/commands/semantic_model/relationships.rs` are unit-tested (8 tests); the create → add×2 → update(inactive) → verify → delete loop is live-validated (`semantic_model_relationship_lifecycle`). The shared definition plumbing was factored into `src/commands/semantic_model/tmdl.rs` (fetch/push/upsert/remove parts, `quote_tmdl_name`, `column_ref`, `find_table_file`).
- **`semantic-model delete-measure` / `rename-measure` / `move-measure` (measure lifecycle via definition read-modify-write) — live-confirmed**: Complete the measure CRUD started by `add-`/`update-measure`. `delete-measure --measure` removes the whole measure block — its leading contiguous `///` description comments, the `measure 'X' = …` decl, and its trailing indent≥2 property/expression lines (`measure_span` computes the line range). `rename-measure --measure --new-name` rewrites ONLY the declaration name (DAX references in OTHER measures are NOT rewritten — documented; matches the "rename the object, not its refs" scope), rejecting a collision with an existing measure (`CONFLICT`). `move-measure --measure --to-table` extracts the full block from its current table file and re-inserts it into the destination table (via the shared `insert_measure_lines`, so it lands after the destination table's scalar properties — canonical "measures first" — never breaking indentation); same-table move → `INVALID_INPUT`. All three OVERWRITE the definition → `mutates:true`/`destructive:true`, `--dry-run` guarded. Pure helpers (`measure_span`, `delete_measure_tmdl`, `rename_measure_tmdl`, `extract_measure_block`, `insert_measure_lines`, plus the `model.bim` variants `delete_/rename_/move_measure_bim`) are unit-tested; the create → seed → rename → move(Sales→Customer) → verify → delete loop is live-validated (`semantic_model_measure_lifecycle`).
- **`semantic-model set-description` / `add-measure` / `update-measure` (granular object authoring via definition read-modify-write, no XMLA/TOM) — live-confirmed**: Microsoft's `powerbi-modeling-mcp` server edits model objects live over XMLA/TOM; fabio is a REST CLI with NO XMLA/TOM, so it achieves the SAME authoring tasks through a definition read-modify-write — `getDefinition` → edit the TMDL `definition/tables/*.tmdl` (or `model.bim`) in place → `updateDefinition` (LRO). Handles BOTH storage forms (a model authored from `model.bim` is stored by Fabric AS TMDL, so the TMDL path is the common case; the `model.bim` path is still handled for models that keep that form). **`set-description --table T [--column C] [--measure M] --description "…"`** sets the object's `///` description comment (table at indent 0, column/measure at indent 1) — replacing any existing `///` block. **`add-measure --table T --name N --expression DAX [--format-string] [--display-folder] [--description]`** inserts a new measure; **`update-measure --measure N [--expression] [--format-string] [--display-folder] [--description]`** edits an existing one in place (replacing its expression while preserving other properties). All three OVERWRITE the definition (irreversible) → `mutates:true`/`destructive:true`, `--dry-run` guarded; the Fabric `updateDefinition` API VALIDATES the result (a malformed TMDL edit is rejected, never silently corrupts the model). **Key TMDL-ordering gotcha (live-caught)**: a table-level child object (`measure`/`column`/…) MUST come AFTER the table's own scalar properties (`lineageTag:`, …) — inserting a measure BETWEEN the `table X` declaration and its `lineageTag` produces `Workload_FailedToParseFile: TMDL Format Error … Invalid indentation`. So `add_measure_tmdl` inserts the measure before the FIRST child-object declaration (or its leading `///` comment) at indent 1 — i.e. after the table's scalar properties — yielding the canonical "measures first" layout. **Measure DAX must be read from the definition, not `INFO.VIEW.MEASURES`** (that view's `Expression` is null over `executeQueries` — the same limitation `measure-dependencies` hit). **LRO latency**: `updateDefinition` after an expression change can take >120s to reframe (an inline `set-description`/property change returns in a few seconds; a SUMX expression change exceeded 120s live). Duplicate measure name → `CONFLICT` (checked client-side after fetch); unknown table/measure → `NOT_FOUND`; no target on `set-description` → `INVALID_INPUT` (resolved offline, before any network call). Pure editors (`tmdl_set_description`, `add_measure_tmdl`, `update_measure_tmdl`, `render_measure_expr`, `is_child_object_decl`, `is_measure_property_line`, plus the `model.bim` variants) in `src/commands/semantic_model/authoring.rs` are unit-tested (11 tests, incl. the lineageTag-ordering regression); the full create → set-description → add-measure → update-measure → get-definition verify → delete loop is live-validated (`semantic_model_authoring_lifecycle`). This closes the "fabio can't edit individual model objects" gap without adding an XMLA/TOM dependency.
- **`semantic-model analyze` + `measure-dependencies` (Best Practice Analyzer / Memory Analyzer over DAX introspection) — live-confirmed**: Both are read-only model-quality tools built entirely on the DAX `INFO.VIEW.*` surface fabio already uses (`executeQueries`), inspired by the Fabric "Semantic model best practices for data agent" guidance. **`analyze`** fetches `INFO.VIEW.{TABLES,COLUMNS,MEASURES,RELATIONSHIPS}` and runs pure best-practice rules → `{issueCount, summary:{error,warning,info}, issues:[{rule,severity,objectType,object,message,fix}]}`. Rules: `missing-description`, `non-descriptive-name` (cryptic `TR_AMT`/`DIM_GEO_01` heuristic), `implicit-aggregation` (a numeric identifier column whose `SummarizeBy` is not `None` — live-verified it flags `FactSales[SaleId]` with `SummarizeBy='Default'`), `duplicate-measure`, `ambiguous-dates`, `inactive-/bidirectional-/many-to-many` relationships, `flat-schema`/`no-relationships` (non-star), `calculated-column`, and `high-cardinality` (opt-in `--with-cardinality`). `--severity` filters to a minimum level; `--strict` exits non-zero for CI. **`analyze --fix` auto-fixes ONLY the safe, mechanical class**: `implicit-aggregation` → set the column's default summarization to `None`, via a read-modify-write on the model **definition** (`getDefinition` → set `summarizeBy: none` on the flagged columns in the TMDL `tables/*.tmdl` or `model.bim` → `updateDefinition`). It is `--dry-run` guarded (shows `wouldFix`) and reports `{fixApplied, fixed:[...]}`. When run WITHOUT `--fix` and auto-fixable issues exist, the output adds `autoFixable: N` + a `hint` with the exact `analyze … --fix` command (so a coding agent discovers the remediation). Live-verified end-to-end: `Sales[StoreId]` `SummarizeBy` went `Default`→`None`, an untargeted `Amount` was left `Default`, and re-`analyze` showed the issue resolved. Because `--fix` OVERWRITES the definition (irreversible), `analyze` is marked `mutates:true`/`destructive:true`. Deliberately NOT auto-fixed (would break DAX/relationships or need human judgment): renaming, duplicate consolidation, schema restructuring, relationship direction/cardinality, materializing calculated columns, cardinality reduction, and descriptions (content is model-specific). **`measure-dependencies`** lists each measure's dependent measures/columns/tables (the get_measure_dependencies equivalent). **Key limitation discovered**: `INFO.VIEW.MEASURES().Expression` comes back **null** over `executeQueries` (redacted), and the raw `INFO.MEASURES()` DMV is **rejected** (HTTP 400, `DatasetExecuteQueriesError`) — so measure DAX is NOT reachable via DAX introspection. fabio therefore reads measure expressions from the model **definition** (`getDefinition`), parsing both TMDL `tables/*.tmdl` (`measure X = <expr>`, single- and multi-line) and `model.bim` (`model.tables[].measures[].expression`). Live-verified: `Avg Price = DIVIDE([Total Amount],[Total Qty])` → `dependsOnMeasures: [Total Amount, Total Qty]`; base measures → their `Sales[Amount]` columns + `Sales` table. **Cardinality** is probed in ONE `EVALUATE ROW("c0", DISTINCTCOUNT('T'[C]), ...)` batch query (best-effort; empty map on failure). Pure helpers (`is_non_descriptive`, `run_rules`, `parse_measure_refs`, `extract_measures`/`extract_measures_tmdl`/`extract_measures_bim`, `apply_summarization_fix`/`fix_summarize_by_tmdl`/`fix_summarize_by_bim`, `parse_object_ref`) in `src/commands/semantic_model/analyze.rs` are unit-tested; the create→analyze→fix→re-analyze→measure-dependencies loop is live-validated (`semantic_model_analyze_and_measure_dependencies`). The three "Prep for AI" components (AI data schema, verified answers, AI instructions) are portal/Desktop-only (no public REST API) — documented in `context best-practices semantic-model-optimization`, not implemented.
- **`semantic-model generate` (Direct Lake from a data source, portal-EXACT TMDL, no REST API) — live-confirmed & diff-verified**: The Fabric portal's "New semantic model" on a lakehouse/warehouse/SQL analytics endpoint (open item → pick tables → Direct Lake model) has NO public REST API. fabio reproduces it CLIENT-SIDE and emits **byte-shape-identical TMDL** to the portal: resolve the source's SQL analytics endpoint `(server, database, sqlEndpointId)` → read `INFORMATION_SCHEMA.COLUMNS` for BASE TABLEs over TDS (optionally `--tables`, `--schema` default `dbo`) → map each SQL type to a Power BI `dataType`, **dropping unmappable columns** (varbinary/geography/geometry/hierarchyid/sql_variant/xml/… → dropped, matching Fabric's own sync rule) → synthesize the portal's exact TMDL definition folder → create → frame with a `Full` refresh. **Verified against a real portal-generated Direct Lake model over the SAME lakehouse** (`scripts/compare-semantic-models.py` diffs the two definitions): fabio matches the portal on format (TMDL), `definition.pbism` (`version: "4.2"`, `settings: {}`), `model.tmdl` (`defaultMode: directLake`, `culture: en-US`, `defaultPowerBIDataSourceVersion: powerBI_V3`, `ref table` per table), `database.tmdl` (`compatibilityLevel: 1604`), per-table files (columns are `dataType` + `sourceColumn` ONLY — no `summarizeBy`; a `directLake` **entity** partition with `entityName`=physical table, `schemaName`, `expressionSource: DatabaseQuery`), and `expressions.tmdl` (`DatabaseQuery = Sql.Database("<server>", "<sqlEndpointId>")`). The comparator reported ZERO column/type/partition/setting gaps. The only residual difference vs the specific reference model is that it had 2 relationships + `isKey` (that model was ontology-derived; the PLAIN portal pick-tables flow creates NEITHER relationships NOR keys — so fabio's 0-relationships/0-keys MATCHES the plain flow). Relationships/measures remain a manual follow-up (`update-definition`), exactly as in the portal. Reads the schema over TDS, so it needs a **SQL-scoped token from the ambient credential chain** (az/device-code cache), NOT a Fabric-only static `FABIO_ACCESS_TOKEN`. Mirrors `ontology generate`. Pure helpers (`map_sql_type_to_powerbi`, `plan_tables`, `build_schema_query`, `build_tmdl_parts`/`tmdl_model`/`tmdl_database`/`tmdl_expression`/`tmdl_table`, `summarize`) in `src/commands/semantic_model/generate.rs` are unit-tested against the captured portal shape; the full generate→frame→DAX loop is `semantic_model_generate_direct_lake_lifecycle` (gated on `FABIO_TEST_LOADED_LAKEHOUSE`).
- **The portal's Direct Lake `Sql.Database(server, catalog)` catalog is the SQL ANALYTICS ENDPOINT ITEM ID (a GUID), not the lakehouse/warehouse id and not the display name — live-verified**: For a lakehouse the catalog GUID is `properties.sqlEndpointProperties.id` (distinct from the lakehouse item id); for a warehouse it is the warehouse item id (a warehouse IS its own SQL endpoint). A GUID is rename-stable, which is why the portal uses it. fabio's `generate` now uses this GUID (an earlier version used the lakehouse display name, which also works but is not what the portal emits). NOTE: the display name still works reliably as the TDS *connection catalog* for READING the schema (`execute_sql_rows`), so fabio uses the display name for the schema read and the SQL endpoint GUID for the model's `Sql.Database` expression — two different values by design.
- **model.bim CAN express Direct Lake at compat 1604 (fabio uses TMDL for portal parity, but the model.bim form also works) — live-confirmed**: A `model.bim` (TMSL JSON) with `compatibilityLevel: 1604`, `model.defaultPowerBIDataSourceVersion: "powerBI_V3"`, `model.defaultMode: "directLake"`, one `directLake` **entity** partition per table, and a shared `DatabaseQuery` M expression IS accepted by the Fabric Items API and produces a queryable Direct Lake model (fabio's `generate` originally emitted this and it returned live DAX results). So Direct Lake does NOT strictly require TMDL — the earlier "Direct Lake REQUIRES TMDL" was true only for the OLD `model.bim` at compat **1550**. fabio switched `generate` to TMDL purely to match the portal byte-for-byte (`create --file model.bim` still accepts a hand-authored 1604 model.bim).
- **Default semantic models are sunset (not a fabio gap)**: Fabric no longer auto-creates a "default" semantic model when a lakehouse/warehouse/mirrored item is created (since 2025-09-05), and existing defaults were decoupled to independent items (2025-11-30). So there is no "default dataset attached to a lakehouse" to manage — new models are explicitly created (via `semantic-model generate`/`create`). Ref: Fabric blog "Sunsetting Default Semantic Models" / "Decoupling Default Semantic Models".
- **TMDL vs model.bim**: Direct Lake works in BOTH TMDL (v4.0/4.2 pbism) AND model.bim at compat 1604/powerBI_V3 (see the corrected note above). The OLD model.bim at compat level 1550 does NOT support Direct Lake mode partitions.
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
- **PBIR-Legacy reliably renders programmatically-authored visuals (empirical)**: In fabio's hand-authoring experiments, PBIR-Legacy `report.json` with `prototypeQuery` produced visuals that display data, and it is a safe pre-GA path. This does NOT mean PBIR can't be authored programmatically — Microsoft documents PBIR (per-visual `visual.json`) as the format for programmatic generation; the correct PBIR path is to conform to the published `visualContainer` schema (see the reconciled note below). Use `report.json` with `sections[].visualContainers[]` for the PBIR-Legacy route.
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
- **PBIR visual rendering is encoding-sensitive (reconciled)**: fabio's earlier hand-built PBIR visual used `query.queryState` and stored but rendered no data, and a `prototypeQuery` was rejected by the PBIR schema — leading to an earlier (too-strong) "PBIR does NOT support programmatic visual data rendering" conclusion. Microsoft's PBIP docs document PBIR as THE programmatic authoring format: each `visual.json` has a published `$schema` under `report/definition/visualContainer/**`. The takeaway is to conform PBIR visuals to that published schema (not the ad-hoc `query.queryState` shape), validate offline with `fabio report validate`, then `fabio report create --definition <folder>`. A full portal-authored PBIR report round-trips and renders (live-verified). PBIR-Legacy with `prototypeQuery` remains a working alternative until PBIR GA. (This bullet supersedes the removed absolute claim.)
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
| Paginated reporting | `.rdl` | `fabio paginated-report create/get-definition/update-definition` (full CRUD) |

### Key Constraints

- **Direct Lake requires TMDL**: `model.bim` cannot express `mode: directLake` partitions. Always use TMDL for Direct Lake.
- **model.bim requires V3**: `compatibilityLevel: 1604` and `defaultPowerBIDataSourceVersion: powerBI_V3` are mandatory.
- **PBIR visual rendering — encoding-sensitive (earlier blanket claim was too strong)**: An early fabio experiment found a hand-built PBIR visual using `query.queryState` stored but showed no data, and concluded "PBIR cannot render data programmatically." Microsoft's PBIP docs contradict the blanket claim: PBIR (the enhanced per-file `definition/` folder) is the DOCUMENTED format for programmatic authoring — each `visual.json`/`page.json` has its own published `$schema`. The earlier symptom reflects a specific incorrect visual-query encoding, not a format limitation. Guidance: author PBIR visuals against the published `visualContainer` schema (`microsoft/json-schemas/fabric/item/report/definition/visualContainer/**`), run `fabio report validate` offline, then `fabio report create --definition <folder>`. A full portal-authored PBIR report round-trips and renders (live-verified: export → create-from-folder → byte-identical PDF). (PBIR-Legacy `report.json` with `prototypeQuery` remains a working alternative but is retired at PBIR GA.)
- **PBIR is the future**: PBIR becomes the only supported report format at GA; PBIR-Legacy is deprecated. Prefer PBIR for new agent-generated reports and conform to the published per-file schemas.
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
- **Configurable data retention (verified live)**: the warehouse time-travel/data-retention window is set via T-SQL (NOT a REST API): `ALTER DATABASE CURRENT SET TIME_TRAVEL_RETENTION_PERIOD = <N> DAYS` (valid 1–120; default 30). Read it back with `SELECT time_travel_retention_period_days FROM sys.databases WHERE name = DB_NAME()`. This window governs time travel (`FOR TIMESTAMP AS OF`), table clones, restore points, and warehouse snapshots. Verified live end-to-end (set 45 → read 45 → restore 30). DECREASING is irreversible — a background GC permanently removes history older than the new window (increasing again can't recover it). `fabio warehouse get-retention`/`set-retention --days <N>` wrap this over the existing warehouse TDS path. Warehouse-only: the Lakehouse SQL analytics endpoint is read-only (can't `ALTER DATABASE`), and SQL Database is a different engine.
- **Connection string format**: `<unique-id>.datawarehouse.fabric.microsoft.com` — no port, no protocol prefix. TDS client connects via port 1433 (default).
- **Views appear in INFORMATION_SCHEMA.TABLES**: Both tables and views show up. Distinguish via `TABLE_TYPE` column (`BASE TABLE` vs `VIEW`).
- **System views are visible**: `queryinsights.*` and `sys.*` views appear alongside user objects. Filter with `WHERE TABLE_SCHEMA = 'dbo'` for user objects only.
- **Date columns via TDS**: Date values come through as "N days since 0001-01-01" string representation in the mssql-rs crate. Conversion: `chrono::NaiveDate::from_num_days_from_ce(days + 1)`.
- **Cross-workspace queries NOT supported**: Three-part naming only works within the same workspace. Cross-workspace requires explicit data copy or shortcuts.
- **SHOWPLAN_XML via TDS**: `SET SHOWPLAN_XML ON` followed by the query returns the estimated execution plan as an XML result set instead of executing the query. Works on Warehouse, Lakehouse SQL Endpoint, and SQL Database. Safe for DDL/DML (never executed). Must execute `SET SHOWPLAN_XML ON` as a separate batch before the query, then `SET SHOWPLAN_XML OFF` afterward for cleanup.
- **sys.dm_exec_requests columns on Fabric**: Does NOT have `login_name` column — that's in `sys.dm_exec_sessions`. Available columns include: `session_id`, `status`, `command`, `start_time`, `total_elapsed_time`, `sql_handle`, `plan_handle`. Filter `WHERE status != 'background'` to exclude internal TASK MANAGER sessions.
 - **queryinsights schema views**: `queryinsights.frequently_run_queries` (columns: `last_run_command`, `number_of_runs`, `avg_total_elapsed_time_ms`, `min_run_total_elapsed_time_ms`, `max_run_total_elapsed_time_ms`, `number_of_successful_runs`, `query_hash`), `queryinsights.long_running_queries` (columns: `last_run_command`, `number_of_runs`, `median_total_elapsed_time_ms`, `last_run_total_elapsed_time_ms`, `last_run_start_time`, `query_hash`), `queryinsights.exec_requests_history` (columns: `command`, `status`, `total_elapsed_time_ms`, `login_name`, `start_time`, `end_time`, `row_count`, `query_hash`). All views work on both Warehouse and Lakehouse SQL Endpoints.
 - **`queryinsights.sql_pool_insights` view (verified live)**: SQL Pool Insights logs pool state changes + sustained pressure events for the two built-in SQL pools. Columns (verified via `INFORMATION_SCHEMA.COLUMNS`): `sql_pool_name` (`varchar`, values `SELECT` / `NONSELECT`), `timestamp` (`datetime2`), `max_resource_percentage` (`int`), `is_optimized_for_reads` (`bit`), `is_pool_under_pressure` (`bit`), `cache_cooldown_minutes` (`int`, nullable), `current_workspace_capacity` (`varchar`, e.g. `F8`). Present and queryable on BOTH the Warehouse and the Lakehouse SQL analytics endpoint (confirmed live on both). Surfaced by `warehouse pool-insights` / `sql-endpoint pool-insights` / `lakehouse pool-insights` (each reuses that group's existing insights connection resolver; SQL built by the shared pure `tds_utils::pool_insights_sql`).
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

### Activator remote MCP server (rule management, live-verified)
- **URL & transport**: `{fabricBase}/mcp/workspaces/{ws}/reflexes/{artifactId}` — server identifies as `"Reflex Mcp Server" v0.0.1`, OAuth (Fabric bearer token), streamable-HTTP with `text/event-stream` (SSE) responses, **stateless** (no `Mcp-Session-Id`). The URL follows the data-agent shape (`/mcp/workspaces/{ws}/dataagents/{id}/...`), NOT the ontology/kql `dataPlane/.../items/...` shape.
- **Six tools** (`tools/list`) — the docs list only four; `delete_rule` and `get_activations_for_rule` are UNDOCUMENTED but present:
  - `list_rules` — `{listRulesParams:{artifactId, workspaceId}}` → `{"rules":[...]}` (both as a JSON `text` content block and `structuredContent`). Empty reflex → `{"rules":[]}`.
  - `start_rule` / `stop_rule` / `delete_rule` — `{<name>Params:{artifactId, workspaceId, ruleId}}`. Mutating.
  - `get_activations_for_rule` — `{getActivationsParams:{artifactId, workspaceId, ruleId, startTime?, endTime?, objectIds?, maxResults?}}`. Fired-alert history; `maxResults` default 100 / max 2000; window defaults to last 24h. A bad `ruleId` returns an MCP tool error (`isError`) carrying the server's `FailedToResolveEntity` BadRequest.
  - `create_rule` — `{createRuleParams:{name, description, source, model}}`. NOT NL-only: a strict, deterministically-callable schema (live-verified by fabio driving it end-to-end). `source` is `oneOf` **KQL** (`{runSettings{executionIntervalInSeconds}, query{queryString}, eventhouseItem: oneOf[{databaseName, clusterHostName} (ADX) | {itemId, workspaceId, itemType:"KustoDatabase"} (Fabric eventhouse)], eventTimeSettings?, queryParameters?}`) or **Ontology** (`{..., connection:{itemId, workspaceId, itemType:"Ontology"|"DigitalTwinBuilder"}}`). `model` = `{stream{splitColumn, filters}, detection{condition, occurrence}, action}`. **Every function argument is `{name, isColumnReference, value:{type, value}}`** where `value.type` ∈ `string|boolean|number|array`. Verified catalog: a single-value condition (e.g. `isGreaterThan`, `increasesAbove`) has args `Column` (isColumnReference:true) + `Value`; `occurrence` defaults to `{occurrenceType:"everyTime", arguments:[]}`; **email** action = `actionType:"email"` with required args `messageLocale, to (array), subject, body, headline`; **Teams** = `actionType:"TeamsMessage"` with `messageLocale, recipientEmail (string), headline, message`. **CHANGE** functions (`increasesAbove`/`decreasesBelow`/…) require a non-empty `stream.splitColumn`; **STATE** functions (`isGreaterThan`/…) work with or without it (server-enforced). Creation does NOT validate KQL connectivity — a rule with a placeholder cluster still creates and returns `{ruleId, artifactId, message}`, then auto-starts (`isRunning:true`). fabio drives this via **`reflex create-rule`** (`--rule <json|@file|stdin>` passthrough or typed flags → `build_create_rule_params`, both live-verified).
- **fabio coverage**: `reflex mcp-url` (prints the URL + existence check), `reflex create-rule` (creates a rule — `--rule` JSON passthrough or typed flags), and native MCP-client commands `reflex list-rules` / `start-rule` / `stop-rule` / `delete-rule` / `rule-activations` (in `src/commands/reflex_mcp.rs`). Errors from a tool come back as HTTP-200 MCP responses with `isError:true`; fabio maps them to a non-zero `API_ERROR`.
- **Server limitations** (from the doc): KQL data sources only (ADX cluster URL, or Fabric eventhouse KQL DB item id + workspace); email/Teams actions only (no webhooks/Power Automate); one eventstream per rule (no cross-stream correlation); no aggregation/summarization (per-event detection only).

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
- **Service principal workspace creation is gated by a separate tenant setting (returns 401, not 403)**: A service principal that is allowed to use Fabric APIs can `GET /workspaces` (list/read succeeds with 200), but `POST /workspaces` (create) returns **`401 Unauthorized`** with `errorCode: "Unauthorized"` and message `"The caller is not authenticated to access this resource"` unless the tenant setting **"Service principals can create workspaces, connections, and deployment pipelines"** (Admin Portal > Tenant settings > Developer settings) is enabled AND the SP (or a security group containing it) is in the allowed list. This is a DIFFERENT setting from "Service principals can use Fabric APIs" (the latter only enables read access). Note the surprising status code: Fabric returns 401 (looks like an auth failure) rather than 403 (permission) — the token is fully valid (correct `aud`, `tid`, `appid`, `idtyp: app`, not expired), so re-authenticating does NOT help. fabio's `auth_required_hint()` (`src/errors.rs`) detects the `"caller is not authenticated"` message and emits a hint pointing at this tenant setting instead of the generic "run fabio auth login".
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
- **Table health check — `sys.sp_get_table_health_metrics` (GA, verified live)**: `fabio lakehouse table-health --table <schema.table>` runs `EXEC sys.sp_get_table_health_metrics @table_name = N'<table>'` over TDS against the lakehouse SQL analytics endpoint (reuses `resolve_lakehouse_sql` + `execute_and_render_sql`). Read-only (no data mutation). Result is a **single row** with `PotentialAnomalyType` (int: `0`=None, `1`=Invalid file statistics, `3`=Many small files), `PotentialAnomalyDescription`, `PhysicalRowCount`, `DeletedRowCount`, `FileCount`, `FileSizeInBytes`, and histogram bins (`FileRowCount[…]`, `FileSize[…]`, `FileDeletedRowCount[…]`). Schema defaults to `dbo` if omitted (`--table FactSales` == `--table dbo.FactSales`). **A newly created/loaded table is NOT immediately visible** to the SQL endpoint — metadata sync lags ~30–60s after `upload-table`/`load-table`, returning SQL error 208 (`Invalid object name`) or 33268 until synced (the e2e test polls a `SELECT COUNT(*)` before running the health check). The table name is embedded as an escaped `N'…'` literal (single quotes doubled) so it is injection-safe; empty/whitespace names are rejected client-side.
- **Table listing uses `"data"` key**: Unlike other list endpoints that use `"value"`, `GET /workspaces/{ws}/lakehouses/{id}/tables` returns `{"data": [...]}`.: Unlike other list endpoints that use `"value"`, `GET /workspaces/{ws}/lakehouses/{id}/tables` returns `{"data": [...]}`.
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
- **External-libraries import/export are RAW FILES, not JSON (verified live — fixes a fabio bug)**: `importExternalLibraries` requires `Content-Type: application/octet-stream` and the RAW `environment.yml` bytes (public-libraries / Azure Artifact Feed YAML). Sending JSON (or any other content type) fails with `EnvironmentValidationFailed: Invalid request. Expected content type application/octet-stream.`; a valid `environment.yml` returns `{}` (200). Symmetrically, `exportExternalLibraries` returns the RAW `environment.yml` TEXT (not JSON) — parsing it as JSON fails with "Invalid JSON response". fabio previously POSTed JSON on import (broken — couldn't even read a YAML file, failed at its own JSON parse) and JSON-parsed the export response (broken). Fixed: `environment import-staging-libraries` now reads the file/`--content` verbatim and posts it as octet-stream via `post_octet_stream`; `export-staging-libraries`/`export-libraries` fetch via `get_text` and wrap the raw text as `{"externalLibraries": "<yaml>"}`. The octet-stream body is always parsed by the server as `environment.yml`; the Maven **pom.xml** "Import pom.xml" path (Spark 4.0+/runtime 2.0, Full mode, no OAP) uses a DIFFERENT, not-yet-identified endpoint (posting a pom.xml to `importExternalLibraries` — even with `?fileName=pom.xml`/`?type=maven` — still errors "Missing 'dependencies' key in environment.yml").
- **Upload staging library**: `POST /workspaces/{ws}/environments/{id}/staging/libraries/{libraryName}` with `Content-Type: application/octet-stream` body. Library name defaults to the filename if `--library-name` not specified. Returns 200 on success.
- **Get/Update definition are LRO**: Both use `poll: true`.
- **Create is LRO**: Returns 202, requires polling.
- **Staging spark compute — `runtimeVersion` + `sparkProperties` (verified live)**: `GET/PATCH .../staging/sparkcompute` returns/accepts `{instancePool, driverCores, driverMemory, executorCores, executorMemory, dynamicExecutorAllocation, sparkProperties, runtimeVersion}`. `runtimeVersion` defaults to `"1.3"` (Spark 3.5); `"2.0"` (Spark 4.1 / Delta 4.2 preview) is accepted and persists. `sparkProperties` is a flat `{key: "value"}` string map (e.g. `spark.native.enabled`, `spark.remote.shuffle.enabled`, `spark.synapse.diagnostic.emitter.*`). **PATCH semantics**: top-level fields MERGE (omitted fields preserved), but a supplied `sparkProperties` object REPLACES the whole map. fabio's `--runtime-version`/`--spark-property KEY=VALUE` (repeatable) flags therefore do a **read-merge-write** (GET current staging compute → set `runtimeVersion`, merge properties → PATCH) so existing properties/fields survive. The typed flags conflict with `--file`/`--content` (raw full-body replace). `--spark-property` splits on the FIRST `=` (value may contain `=`); empty key is rejected.

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
- **Code-first / gateway-usage top-level booleans (verified live)**: the create-connection body accepts two top-level booleans (siblings of `connectionDetails`/`credentialDetails`, present for `ShareableCloud`): `allowUsageInUserControlledCode` (default false) — "Allow this connection to be used by items that allow user-controlled code such as Notebook" (the portal "Allow Code-First Artifacts like Notebooks to access this connection" toggle) — and `allowConnectionUsageInGateway` (default false) — allow use with on-premises/VNet gateways. `allowUsageInUserControlledCode` is CREATE-TIME ONLY (the portal notes it can't be changed later). Verified live: creating a `Web`+`Anonymous` connection with `allowUsageInUserControlledCode: true` succeeds and `GET /connections/{id}` reports it back as `true` (and `allowConnectionUsageInGateway: false`). fabio exposes them as `connection create --allow-code-first-artifacts` and `--allow-gateway-usage`, only emitting each field when the flag is passed (server default false otherwise).
- **`creationMethod` frequently differs from the connection `type` (verified live)**: `connectionDetails.creationMethod` is NOT always equal to `connectionDetails.type`. `GET /connections/supportedConnectionTypes` (raw `data.value[]`) is the source of truth — each type carries a `creationMethods[]` array. Verified live: only ~12 of ~276 types have a method matching the type name (`Web`, `Oracle`, `MySql`, `Salesforce`, `AzureBlobs`, …); the rest differ (`SQL`→`Sql`, `PostgreSQL`→`PostgreSql`, `EventHub`→`EventHub.Contents`, `ConfluentCloud`→`ConfluentCloud.Contents`, `MQTT`→`MQTT.Contents`). Only 2 types expose MULTIPLE creation methods on the plain endpoint (`AzureDataExplorer`→`{Contents, KqlDatabase}`, `Spark`→`{AzureSpark.Tables, ApacheSpark.Tables}`). Sending the wrong method (e.g. `creationMethod: "EventHub"`) fails with `InvalidConnectionDetails: The ConnectionDetails input provided is not valid`; the correct `EventHub.Contents` is accepted and the API proceeds to test the credential (e.g. `WorkspaceIdentity` → `DMTS_UntrustedEndpointForWorkspaceIdentity` for an unreachable/fake endpoint). fabio previously HARDCODED `creationMethod = connection_type`, so `connection create` was broken for the ~264 differing types (only the ~12 matching ones worked — which is why the `Web`-based e2e test never caught it). **Fix**: `connection create --creation-method <METHOD>` (optional); when omitted, fabio AUTO-RESOLVES the method from `supportedConnectionTypes` (`resolve_creation_method`) — single method → use it; type-name match → use it; unknown type or catalog-fetch failure → fall back to the type name (non-blocking); multiple methods → a teaching error enumerating the valid values (`--creation-method` required). The connection is the auth carrier for Eventstream external sources; the eventstream side just references it.
- **Azure Event Hubs Workspace Identity connection**: `connection create --connection-type EventHub --credential-type WorkspaceIdentity --parameters '{"endpoint":"sb://<ns>.servicebus.windows.net","entityPath":"<hub>"}'` (no `--creation-method` needed — fabio auto-resolves `EventHub.Contents`). The workspace identity's service principal needs an Event Hubs data role (e.g. Azure Event Hubs Data Receiver) on the namespace. This is the connection an Eventstream `AzureEventHub` extended-features source uses for keyless (Workspace Identity) authentication.

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
- **Workspace default runtime version — `environment.runtimeVersion` (verified live)**: `GET /workspaces/{ws}/spark/settings` returns `{automaticLog, highConcurrency, pool, environment: {runtimeVersion}, job}`. The workspace DEFAULT Spark runtime is the nested `environment.runtimeVersion` (`"1.3"` = Spark 3.5 default; `"2.0"` = Spark 4.1 / Delta 4.2 preview, accepted and persists). The PATCH MERGES top-level keys (omitted settings preserved), but a supplied `environment` object REPLACES it wholesale — so fabio's typed `spark update-settings --runtime-version <ver>` does a **read-merge-write** (GET → set `environment.runtimeVersion` → PATCH) to preserve any sibling `environment` keys, and conflicts with the raw `--settings` flag. This is the workspace counterpart to `environment update-staging-spark-compute --runtime-version` (which sets the runtime for a single Environment item). No `releaseChannel` field is exposed on this endpoint (Runtime Release Channels remain a portal-only preview with no observed REST surface).

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
- **Remote MCP server URL (verified live)**: A KQL database (in an eventhouse) is consumable as a hosted remote MCP server at `{fabricBase}/mcp/dataPlane/workspaces/{ws}/items/{itemId}/kqlEndpoint`, where `itemId` is the KQL DATABASE item id (the portal's "Database details → MCP Server URI"), NOT the eventhouse id. Same generic `dataPlane/.../items/...` shape as the ontology MCP URL, with a `kqlEndpoint` suffix. `fabio kql-database mcp-url` constructs it (deterministic — agents cannot guess the suffix). Live-verified end-to-end: POSTing an MCP `initialize` to the constructed URL with a Fabric bearer token returns `{serverInfo:{name:"KustoMCP",version:"1.0.0"},capabilities:{tools:{...}}}` — a real, working MCP server exposing schema-discovery / NL→KQL / execute / sample tools. A global variant also exists (`{fabricBase}/mcp/dataPlane/kqlEndpoint`, with `workspaceId`+`itemId` supplied per tool call), and tools accept optional `clusterUrl`/`databaseName` params; fabio prints the deterministic per-item URL.
- **Eventhouse MCP server tools (verified live via `tools/list`)**: the KQL/eventhouse MCP server (`KustoMCP` v1.0.0) exposes exactly 4 tools. (1) `executeQuery(kqlQuery, maxRecords, [clusterUrl, databaseName])` — runs KQL, duplicates `kql-database query`. (2) `getSchema(referenceText)` — returns a rich, LLM-oriented schema bundle: tables/materialized-views/external-tables + functions + COLUMN VALUE SAMPLES + cardinality/distinct-value STATS + KQL-authoring guidance (richer than the raw schema from `describe`/`list-entities`). (3) `getGeneralKQLExamples(referenceText)` — curated PUBLIC NL→KQL example pairs relevant to the reference (markdown text). (4) `getSpecificKQLExamples(referenceText)` — examples curated/LEARNED from the specific database (empty on a fresh DB). All three grounding tools return an `isError` result with text "Database is empty" when the KQL database has no tables (they ground on schema/data). `fabio kql-database examples` drives tools (3) and (4) — the ones with no offline fabio equivalent; `fabio kql-database schema-context` drives (2) `getSchema` (grounding fabio can't produce offline — the samples come from real data); (1) `executeQuery` is not wrapped (it overlaps `query`).
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
- **Run tracking is via the Fabric-hosted MLflow REST API (live-confirmed)**: Each workspace hosts an MLflow tracking server. The MLflow tracking URI shown by the `synapseml-mlflow` plugin is `sds://api.fabric.microsoft.com/v1/workspaces/{ws}/mlflow`; the underlying REST API is at `https://api.fabric.microsoft.com/v1/workspaces/{ws}/mlflow/api/2.0/mlflow/...` and accepts the **standard Fabric bearer token** (no separate scope). **The experiment item's GUID doubles as the MLflow `experiment_id`.** `fabio ml-experiment list-runs` → `POST .../runs/search` (`{experiment_ids, max_results, filter, order_by}`); `get-run` → `GET .../runs/get?run_id=`; `get-metric-history` → `GET .../metrics/get-history?run_id=&metric_key=`. Runs can also be created/logged entirely over REST (`runs/create`, `runs/log-metric`, `runs/update` with `status:"FINISHED"`), which the e2e test uses to seed a run without a notebook. `runs/search` returns `{runs:[{info, data:{metrics,params,tags}}], next_page_token}`; empty experiments return `{"runs":[]}`. Fabric injects `synapseml.*` tags on runs. Artifact URIs use the OneLake `sds://onelake<region>.pbidedicated.windows.net/...` scheme.

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
- **Invocation is portal-only for URL discovery (no public API)**: The public Fabric REST API for UserDataFunction is CRUD-only (7 ops). Each *published* function exposes its own unique public REST endpoint, but the URL is obtainable only from the portal (Run-only mode → Functions explorer → "Copy Function URL", with public access enabled) — there is no API to list functions or construct the URL. `fabio user-data-function invoke --url <public-url> [--parameter name=value]... [--body <json>]` therefore takes the URL directly (mirroring `data-agent query --published-url`), attaches the standard Fabric bearer token, and POSTs the JSON parameter body. The response schema is `{functionName, invocationId, status, output, errors}` where `status ∈ {Succeeded, BadRequest, Failed, Timeout, ResponseTooLarge}` (HTTP 200 even when `status != Succeeded`; agents inspect the field). HTTP codes: 400 bad/missing params or public access off, 422 `UserThrownError`, 401/403 auth. The URL is SSRF-guarded via `validate_trusted_url` (must be `*.fabric.microsoft.com` HTTPS). Live-validated plumbing only (auth + POST + error handling reach Fabric and surface a clean 404); the happy path needs a published function with public access, which cannot be provisioned via REST.

## Operations Agent API Behaviors Discovered
- **Definition file**: `Configurations.json` (same name as anomaly-detector, NOT `definition.json`).
- **Definition format**: `OperationsAgentV1` (reported in getDefinition response).
- **Definition schema**: `https://developer.microsoft.com/json-schemas/fabric/item/operationsAgents/definition/1.0.0/schema.json`.
- **Definition structure**: `{"$schema":"...","configuration":{"goals":"","instructions":"","dataSources":{},"actions":{}},"shouldRun":false}`.
- **Configuration fields**: `goals` (natural language objective), `instructions` (natural language instructions), `dataSources` (object mapping data source names to configs), `actions` (object mapping action names to configs).
- **`shouldRun` controls activation**: Boolean that determines if the agent is actively running.
- **Start/stop is definition-driven (no dedicated endpoint)**: Fabric has no `POST .../start` or `POST .../stop` route for operations agents. Activation is the top-level `shouldRun` boolean inside `Configurations.json`. `fabio operations-agent start`/`stop` implement this as a read-modify-write (`getDefinition` (LRO) → decode the `Configurations.json` part → set `shouldRun` true/false → `updateDefinition` (LRO) with just that part), then **re-read the definition** to report the persisted value. `fabio operations-agent status` reads `shouldRun` back and reports `running`/`stopped`. While running, Fabric evaluates the agent's rule queries every 5 minutes.
- **Fabric coerces `shouldRun` to false for unconfigured agents (verified live)**: `updateDefinition` accepts and persists other configuration changes (e.g. `configuration.instructions`), but the server silently forces `shouldRun` back to `false` if the agent has no configured data source (eventhouse/ontology) and generated playbook. The `updateDefinition` call still returns success — the coercion is silent. Because of this, `operations-agent start` re-reads the definition and reports the actual `shouldRun` (via a `requestedShouldRun` vs `shouldRun` split plus an explanatory `note` and an actionable `hint` when activation was refused) rather than optimistically claiming success. `operations-agent status` likewise emits a `hint` suggesting the exact `operations-agent start` command when the agent is stopped. A properly configured agent (data source + playbook, typically set up in the portal/Copilot) is required for `shouldRun: true` to stick.
- **Full configuration + activation is NOT achievable via the public REST/definition API (verified live)**: The definition schema (`operationsAgents/definition/1.0.0/schema.json`) requires three top-level keys — `configuration`, `playbook`, and `shouldRun` — where `configuration.dataSources` maps aliases to `{id, type: KustoDatabase|Ontology, workspaceId}` objects and `playbook` is an opaque `object`. Attempting to bind a real `KustoDatabase` data source via `updateDefinition` **persists the structure but zeroes the `id` to `00000000-0000-0000-0000-000000000000`** — Fabric does not accept data-source binding through raw definition writes. No `playbook` is generated either. Data-source binding and the "Generate Playbook" step are portal/Copilot-only operations with no public REST API, so an agent cannot be driven from unconfigured → running purely through the CLI. Consequently there is no E2E test that "fully starts on create"; the `operations_agent_start_stop_status_lifecycle` test asserts the documented unconfigured-path coercion behavior instead.
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
- **Public REST API is CRUD + definition ONLY**: The published DTB REST surface (`/rest/api/fabric/digitaltwinbuilder/items`) is exactly 7 operations — Create/Delete/Get/Get-Definition/List/Update/Update-Definition — which fabio fully covers. Ontology modeling (namespaces/entity types/properties/relationship types), data mapping, contextualization, and the Explorer are LOW-CODE PORTAL experiences with NO public API; the `definition.json` blob is authored by the portal.
- **Auto-provisioned data lakehouse (`<name>dtdm`)**: Creating a DTB auto-creates a linked lakehouse named `<item-name>dtdm` — even a brand-new, unmodeled DTB has `{"LakehouseId":"<uuid>"}` in its definition and a working SQL analytics endpoint. The lakehouse SQL endpoint exposes the **base layer** (`dbo` schema — delta tables, ~24 metamodel objects on a fresh DTB) and, once entity types are modeled, the **domain layer** (`dom` schema — normalized views, the RECOMMENDED query surface). fabio surfaces this via `digital-twin-builder show-lakehouse` (resolve `LakehouseId` → the dtdm lakehouse + SQL endpoint) and `digital-twin-builder query --sql` (resolve + run T-SQL against `dom`/`dbo`, reusing the TDS query path). Querying the SQL endpoint needs a SQL-scoped token (the credential chain / `az login`), NOT a Fabric-API-scoped static `FABIO_ACCESS_TOKEN` (that yields TDS login-failed 18456).
- **Deleting a DTB ORPHANS its `dtdm` lakehouse**: `DELETE /digitalTwinBuilders/{id}` removes the item (and cascades its child flows) but does NOT delete the associated `<name>dtdm` lakehouse — it is left behind. `fabio digital-twin-builder delete --delete-lakehouse` resolves the `LakehouseId` (via getDefinition, before deleting) and cascades the lakehouse deletion; the default delete emits a `note` warning about the orphan.
- **Flow execution is portal-gated**: The flow item accepts the Jobs API (`GET /items/{flowId}/jobs/instances` → 200) but no public/guessable job type triggers a run (every candidate → `InvalidJobType`). Flows run on-demand (portal Run button) or on a fabric-managed schedule; there is no reliable REST run.

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
- **execute-query uses `--gql` flag (was `--query`, which was BROKEN)**: The GQL query string is passed as `fabio graph-model execute-query --workspace <WS> --id <ID> --gql "<GQL>"`. It was originally `--query`, which **clashed with fabio's global `--query` JMESPath projection flag** (`cli.rs`, `global = true`): clap fed the same string into BOTH the GQL body AND `cli.query`, so `render_object` applied the GQL text as a JMESPath expression to the response. Any real query (`MATCH (n) RETURN ...`) is an invalid JMESPath → the command returned `{"data":null}` with exit 0 — all results silently dropped, and GQL errors never surfaced. The bug was invisible in e2e because the only query test ran against an *unloaded* graph (HTTP 4xx path, before the JMESPath projection). Fixed by renaming the flag to `--gql` (matching `graphql-api query --gql`). Live-verified against a portal-loaded graph: `--gql "MATCH (n:DimStore) RETURN n.StoreName LIMIT 2"` now returns the tabular result set.
- **executeQuery returns HTTP 200 even on failure — check `status.code`**: The GQL Query API (`POST .../graphModels/{id}/executeQuery?preview=true`) always responds HTTP 200; the outcome is in the response `status.code` (ISO/IEC 39075 GQL status code). Prefixes `00`/`01`/`02`/`03` = success/warning/no-data/info; anything else (e.g. `42000` syntax error) is an error, with a human-readable `description` and a nested `cause` chain. Success bodies carry `result.{kind,columns,data}` (`kind: "TABLE"` or `"NOTHING"`). fabio now parses this (`gql_status_error`) and maps an error status to a non-zero-exit `API_ERROR` instead of reporting success. A raw `fabio rest call --method post --path .../executeQuery?preview=true` returns the whole envelope unparsed.
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
- **Create is LRO**: `POST /workspaces/{ws}/paginatedReports` returns 202, requires polling. Body requires `displayName` and `definition` (with a `parts` array — and NO `format` field, see below).
- **Definition must NOT include a `format` field (CORRECTED — root cause of the earlier "server-side block")**: The definition object is `{"parts": [...]}` ONLY. Sending `definition.format: "PaginatedReportDefinition"` is rejected with `InvalidDefinitionFormat`. This mirrors the working `report create` body (which also omits `format`). Each part: `{"path": "<displayName>.rdl", "payload": "<base64>", "payloadType": "InlineBase64"}`. The RDL part path MUST equal `<displayName>.rdl`; any other path fails with `MissingDefinitionParts: Definition for '<displayName>.rdl' is missing`. A `.platform` part is optional (the portal includes one, but create/updateDefinition succeed without it). `fabio paginated-report create` synthesizes the part path from `--name` (not the file basename); `update-definition` resolves it from the item's current display name via a GET.
- **getDefinition is LRO**: `POST .../getDefinition` with empty body `{}`. Returns 202, requires polling.
- **updateDefinition supports `?updateMetadata=true`**: Append to URL to propagate `.platform` metadata changes.
- **delete returns 200**: `DELETE /workspaces/{ws}/paginatedReports/{id}` returns immediately (not LRO). Supports `?hardDelete=true` for permanent deletion.
- **Endpoint patterns**: `/workspaces/{ws}/dashboards`, `/workspaces/{ws}/datamarts`, `/workspaces/{ws}/paginatedReports/{id}`.
- **Export to file (Power BI API, not Fabric REST) — live-confirmed**: Rendering a report or paginated report to a file uses the Power BI `exportToFile` flow, NOT the Fabric item API: `POST {powerbiBase}/groups/{ws}/reports/{id}/ExportTo` (202 + job `id`) → poll `GET .../reports/{id}/exports/{jobId}` until `status: Succeeded|Failed` → `GET .../reports/{id}/exports/{jobId}/file` (binary). `fabio report export` / `paginated-report export` implement this. Format enum: PDF/PPTX (both), PNG (Power BI reports only), IMAGE/XLSX/DOCX/CSV/XML/MHTML/ACCESSIBLEPDF (paginated only). Paginated parameters go in `paginatedReportConfiguration.parameterValues`. A bogus report id returns a clean `PowerBIEntityNotFound` (404), confirming the request path/auth.
 - **✅ Create with definition WORKS (earlier "capacity block" finding was WRONG — it was a fabio bug)**: A prior investigation concluded `POST /workspaces/{ws}/paginatedReports` was blocked server-side on this tenant/capacity because it failed both **with** `format:"PaginatedReportDefinition"` (→ `InvalidDefinitionFormat`) and **without** the `format` field (→ `UnknownError`). That conclusion was incorrect. Re-tested (2026-08) by exporting a **real portal-created** paginated report's definition and re-creating it: the create succeeds when the body sends `definition:{parts:[...]}` with **NO `format` field** AND the single RDL part is named `<displayName>.rdl`. Isolation matrix (real portal RDL, live tenant): `{no format, .rdl only}` → **created**; `{no format, .rdl + .platform}` → **created**; `{format present, either}` → `InvalidDefinitionFormat`. The earlier "`UnknownError` without format" almost certainly came from an additional malformed field (the earlier attempts also mismatched the part path). **Fix shipped**: `paginated_report.rs` `create`/`update_definition` now omit `format` and set the part path to `<displayName>.rdl` (create from `--name`, update from a GET of the item). Live-validated end-to-end: `fabio paginated-report create --file` (file basename intentionally ≠ display name) → created, exports byte-identical CSV to the portal original; `update-definition` round-trips; the `paginated_report_create_show_delete_lifecycle` E2E test now passes with a minimal valid textbox RDL. Pure helpers `single_rdl_part`/`definition_object` are unit-tested (regression guard: `definition_object` must not emit `format`).

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
- **`list-domains` gained `withAssignedWorkspacesOnly` filter (July 2026 spec update)**: `GET /admin/domains?preview=false` now accepts a second boolean query parameter, `withAssignedWorkspacesOnly` (default `false`), alongside the existing `nonEmptyOnly`. `withAssignedWorkspacesOnly=true` returns only domains that have at least one workspace assigned to them or to any subdomain (a superset condition vs. `nonEmptyOnly`, which additionally requires the caller to have read access to an item in one of those workspaces). `fabio admin list-domains` now supports both `--non-empty-only` and `--with-assigned-workspaces-only` boolean flags (previously neither filter was exposed); both are query-string append-only and default to omitted (server default `false`) when not passed.

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
- **Service principal / managed identity support added (July 2026 spec update)**: All OrgApp endpoints (list, create, get, update, delete, getDefinition, updateDefinition) now document `Service principal and Managed identities` support as `Yes` (previously `No`, user-only). No fabio code change needed — fabio's auth layer already supports service-principal tokens uniformly for every command; this was purely a permissions/documentation update on the Fabric side.

## OrgAppAudience API Behaviors Discovered
- **Item type**: `OrgAppAudience` (audience targeting for Organizational Apps).
- **Endpoint pattern**: `/workspaces/{ws}/orgAppAudiences/{id}`.
- **Standard CRUD + definitions**: Full lifecycle via list/show/create/update/delete/get-definition/update-definition.
- **Create is LRO**: Returns 202, requires polling.
- **getDefinition/updateDefinition are LRO**: Both use standard Fabric LRO polling pattern.
- **Added to DEPLOY_ORDER**: Positioned after OrgApp (dependent item).
- **`format` field and part path renamed (July 2026 spec update)**: Same change as `OrgApp` — `OrgAppAudiencePublicDefinition.format` (previously the `OrgAppAudienceV1` enum) is now a free-form string, and the `CreateOrgAppAudience` example changed the part path from `OrgAppAudienceV1.json` to `definition.json` (the `"format": "OrgAppAudienceV1"` field was also dropped). No fabio code change needed: `org_app_audience.rs` never sets a `format` field and already uses `definition.json` as the part path.
- **Service principal / managed identity support added (July 2026 spec update)**: All OrgAppAudience endpoints (list, create, get, update, delete, getDefinition, updateDefinition) now document `Service principal and Managed identities` support as `Yes` (previously `No`, user-only). No fabio code change needed — same rationale as OrgApp above.

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
- **`maxMemberGatewayCount`/`minMemberGatewayCount` supersede `numberOfMemberGateways`**: `CreateVirtualNetworkGatewayRequest`/`UpdateVirtualNetworkGatewayRequest`/`GatewayProperties` now support a range pair (`maxMemberGatewayCount`, `minMemberGatewayCount`) as an alternative to the legacy fixed `numberOfMemberGateways`. The two forms are **mutually exclusive** — sending both a fixed count and a range returns `400 ConflictingMemberGatewayCountProperties`. fabio enforces this client-side via clap `conflicts_with_all`/`requires` constraints (`--member-count` conflicts with `--max-member-gateway-count`/`--min-member-gateway-count`; the two range flags require each other).
- **`numberOfMemberGateways` no longer required on create**: The Fabric API removed it from `CreateVirtualNetworkGatewayRequest`'s `required` array (since the range pair is now a valid alternative). fabio's `gateway create` defaults to `numberOfMemberGateways: 1` only when neither the fixed count nor the range pair is supplied (preserves prior default behavior for existing scripts); `gateway update` does NOT apply this default — omitting all three flags leaves member count unchanged (partial PATCH semantics).

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
- **Items without definition support must be skipped**: SQLEndpoint, Dashboard, Datamart, MLModel, MLExperiment never support `getDefinition` (always return errors). Skipping them avoids wasted LRO calls in deep mode. **PaginatedReport is NOT in this list** — it DOES support `getDefinition` (verified live on the generic `/items/{id}/getDefinition` endpoint), and its `.rdl` embeds a `semanticmodelid=<uuid>`, so scanning it recovers report→semantic-model lineage.
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

## Git Workspace Relations API Behaviors Discovered (Preview)

- **New endpoints**: `GET/POST /workspaces/{workspaceId}/git/workspaceRelations` and `DELETE /workspaces/{workspaceId}/git/workspaceRelations/{workspaceRelationId}`. Tag `WorkspaceRelations` in the Fabric REST spec. Models the "Branch out to workspace" base/branch linkage as a first-class, independently manageable relation (previously only implicit via `git branch-out`).
- **List response envelope**: `GET .../workspaceRelations` returns `{"value": [...]}` (standard array-field key), paginated the same way as other list endpoints — fabio's generic `client.get_list(path, "value", ...)` handles it with no special-casing.
- **`WorkspaceRelationType` enum**: `Base` | `Branch` | `RelatedWorkspace`. Only `Base`/`Branch` are valid values for `CreateWorkspaceRelationRequest` — `RelatedWorkspace` is a read-only/derived type returned by the server (e.g., to describe the reciprocal side of a relation) and is rejected by the create endpoint with `WorkspaceRelationInvalidType`. fabio's `git relation create --relation-type` clap `value_parser` only accepts `base`/`branch` (lowercase), mapped to PascalCase for the request body.
- **Relation is directional and typed from the caller's perspective**: creating a relation with `relationType: "Base"` on workspace A pointing at workspace B means "B is the base of A" (i.e., A is the branch); `relationType: "Branch"` means the reverse. There is no separate "target relation type" field — the type describes what the *related* workspace is relative to the calling workspace.
- **Mutual-exclusivity / integrity error codes**: `WorkspaceRelationAlreadyExists`, `WorkspaceRelationBidirectionalExists` (can't create both directions between the same pair), `WorkspaceRelationSelfReferencing` (workspace can't relate to itself), `WorkspaceRelationBaseIsBranch`/`WorkspaceRelationTargetHasBranches`/`WorkspaceRelationDifferentBase`/`WorkspaceRelationTypeNotBranch` (branch-tree shape constraints — a workspace can only have one base, and a base's branches must share consistent lineage), `WorkspaceRelationRootDirectoryMismatch` (git-connected workspaces in a relation must point at the same root directory in the repo), `WorkspaceRelationNotFound`/`WorkspaceNotInvolvedInRelation` (delete/lookup misses), `WorkspaceRelationInvalidArgument`. All pass through fabio's generic HTTP-status→`ErrorCode` mapping (400→`InvalidInput`, 404→`NotFound`, 409→`Conflict`) with the server's raw code/message surfaced in the error body — no new `ErrorCode` variants were needed.
- **Preview status**: No `?preview=true`/`?beta=true` query flag observed in the spec's example requests for these endpoints (unlike some other preview features) — the preview designation is documentation-only in the spec at this time. fabio's `git relation` command doc comments note "(Preview)" for agent visibility, but no query-parameter gating was added; if Fabric later requires a preview flag, add it to `git/relation.rs`'s URL construction.
- **fabio implementation**: `fabio git relation list|create|delete` in `src/commands/git/relation.rs`, wired into `git/mod.rs` as a `Relation(RelationCommand)` subcommand via `mod relation;` (the `git` command is a directory module).

## Fabric JSON Schema Conformance

Microsoft publishes authoritative JSON Schemas for Fabric item definitions and the
git-integration `.platform` file at **https://github.com/microsoft/json-schemas/tree/main/fabric**
(served publicly at `https://developer.microsoft.com/json-schemas/fabric/...`). Any
definition part fabio synthesizes that the schema marks `$schema`-required MUST include
a `$schema` URL at the latest matching published version and conform to the schema's
`required`/`properties`/`enum`.

### Repo layout (as of the audit)
- `fabric/item/<type>/definition/**/<version>/schema.json` — per-item, versioned. Item types WITH a published definition schema: CosmosDB, azureDatabricksStorage, dataAgent, eventSchemaSet, graphIndex, graphQuerySet, graphqlApi, map, metricSet, mirroredAzureDatabricksCatalog, mirroredCatalog, mlExperiment, mlModel, ontology, operationsAgents, orgapp, orgappaudience, plan, report, semanticModel, userDataFunction, variableLibrary, version.
- `fabric/gitIntegration/platformProperties/{2.0.0,2.1.0}/schema.json` — the `.platform` file. `2.1.0` adds an optional `metadata.sensitivityLabelId` (the ONLY delta from 2.0.0).
- `fabric/gitIntegration/schedules/1.0.0`, `fabric/common/*`, `fabric/pbip/*`.
- Item types WITHOUT a published schema here have NO conformance obligation: Notebook, DataPipeline, Eventstream, Reflex/Activator, KQL Queryset/Dashboard, GraphModel, SparkJobDefinition, Dataflow, MirroredDatabase, MountedDataFactory, CopyJob, etc.

### Audit findings + fixes (fabio ↔ MS schema)
- **`report/definitionProperties` — `$schema` is REQUIRED** (both 1.0.0 and 2.0.0; top-level `additionalProperties:false`; `required:[$schema,version,datasetReference]`). The `byConnection` shape differs by version: **1.x** = 6 fields (`connectionString`,`pbiServiceModelId`,`pbiModelVirtualServerName`,`pbiModelDatabaseName`,`name`,`connectionType`); **2.x** = ONLY `connectionString` (`additionalProperties:false`). fabio's `report create --dataset` binds by ID via `pbiModelDatabaseName`, which is the 1.x shape → it now emits the **1.0.0** `$schema` URL (`build_dataset_pbir`). **FIXED**: previously omitted `$schema` entirely. Live: create succeeds and **Fabric normalizes the stored pbir to 2.0.0** (rewrites `byConnection` to just `connectionString`).
- **`semanticModel/definitionProperties/1.0.0` — `$schema` is REQUIRED** (`required:[$schema,version]`, `additionalProperties:false`). fabio's `semantic-model create` now emits `$schema` (1.0.0) + `version` ("4.0" TMDL / "3.0" model.bim) in `definition.pbism` (`build_pbism`). **FIXED**: previously `{"version":...}` only. Live: create succeeds; Fabric normalizes the stored `version` (e.g. to "4.2").
- **`.platform` (platformProperties) — bumped 2.0.0 → 2.1.0** and now round-trips `metadata.sensitivityLabelId`. `parse_platform_file` reads it (previously silently dropped when importing Fabric Git Integration / fabric-cicd repos), `build_platform_json` emits it (and the 2.1.0 `$schema`) when present, and `deploy apply` applies it on create as a **fallback** when no `governance.metadata.json` sidecar label is present. fabio's own `deploy export` still writes labels to the governance sidecar (unchanged); the 2.1.0 field is the interop path for external repos. Live: `deploy export` emits 2.1.0 `.platform`; `deploy plan` re-parses cleanly (content-hash excludes `.platform`, so no spurious diffs).
- **CONFORMING already (no change needed)**: ontology `entityType`/`relationshipType`/`dataBinding`/`contextualization` (correct `$schema` 1.0.0 + consts/enums), operations-agent `Configurations.json` (1.0.0, read-modify-write only), report deploy transform (`transform_report_pbir_bypath` preserves the source `$schema`). Pass-through user files (variableLibrary parts, map.json, graphql schema, mirrored catalogs, eventSchemaSet) carry conformance responsibility on the user, not fabio.
- **Not applicable**: MLModel/MLExperiment are SHELL_ONLY (no getDefinition/updateDefinition on the REST API); the MS `mlModel/mlExperiment` schemas describe a Git-export `dependencies` shape, not a REST payload.

### Rule for new code
When adding any command that SYNTHESIZES a Fabric definition part (not a pass-through of a user file), check `microsoft/json-schemas/fabric/item/<type>/...` for a published schema. If it exists and marks `$schema` required, emit the latest matching `$schema` URL and conform field-for-field. Add a pure builder + a unit test asserting the `$schema` and required fields (see `build_dataset_pbir`, `build_pbism`, `build_platform_json`).

## Power BI Project (PBIP) / PBIR Report Support

Microsoft documents the plain-text Power BI Project format at
<https://learn.microsoft.com/power-bi/developer/projects/projects-overview>
(+ `projects-report`, `projects-dataset`). It is the format coding agents should
generate/edit. fabio adds first-class support for validating and creating these.

### Format recap (what agents produce)
- **PBIP root**: `<name>.Report/`, `<name>.SemanticModel/`, `<name>.pbip` (pointer), `.gitignore`.
- **Report** (`.Report/`): required `definition.pbir` (`$schema` + `version` + `datasetReference` with `byPath` XOR `byConnection`), plus EITHER `report.json` (PBIR-Legacy, version 1.0) OR a `definition/` folder (PBIR enhanced, version 4.0+): `definition/report.json`, `definition/version.json`, `definition/pages/pages.json` (optional), `definition/pages/<page>/page.json` (required per page), `definition/pages/<page>/visuals/<visual>/visual.json` (required per visual), optional `bookmarks/`, `reportExtensions.json` (report-level measures). Every PBIR JSON carries its own `$schema`.
- **byPath vs byConnection**: Git Integration exports `byPath` (relative path to the `.SemanticModel`). Deploying via the REST API REQUIRES `byConnection` (a `connectionString` with `semanticmodelid=<id>`, or the 6-field v1 form). fabio deploy already rewrites `definition.pbir` byPath→byConnection.
- **`.pbi/`** (localSettings.json, cache.abf) is git-ignored user state — never a definition part.
- PBIR is preview; at GA it becomes the ONLY report format (PBIR-Legacy retired).

### fabio commands
- **`fabio report validate --source <path>`** — OFFLINE structural + `$schema` validation. Accepts a `.Report` folder, a `definition.pbir` file, or a PBIP root (validates each `*.Report`). Checks: definition.pbir present + valid JSON; `$schema` (warn if missing — Fabric is lenient but MS marks it required); `version`; `datasetReference` has exactly one of byPath/byConnection (byPath → warning that create needs byConnection, and the target path is resolved); format detection (PBIR vs PBIR-Legacy); required PBIR files (`definition/report.json`, `version.json`, `pages/` with ≥1 `page.json`, each `visual.json`); version/format compatibility (version 1.0 with a `definition/` folder → error). Emits `{status, report|reports, summary}` with machine-readable `code`s (`MISSING_PBIR`, `MISSING_REQUIRED`, `INVALID_JSON`, `VERSION_FORMAT_MISMATCH`, `BYPATH_NEEDS_BYCONNECTION`, …); exits non-zero when invalid. Live-validated on a real 54-check exported PBIR report.
- **`fabio report create --definition <folder>`** — creates a FULL PBIR report (all pages/visuals) from a folder, not just a single `definition.pbir`. Gathers every file recursively (excluding `.platform`, `.pbi/`, `.children/`, and deploy sidecars), validates first (clear error instead of an opaque API rejection), and posts the parts. With `--dataset`, rebinds the folder's `definition.pbir` to that model by connection (so a byPath-referenced generated report can bind to a concrete deployed model at create time). Previously only `deploy` could push a full PBIR tree (and it required `.platform` scaffolding). Live-validated: export → validate → create-from-folder → byte-identical PDF render.
- Pure helpers `validate_report_folder`, `validate`, `gather_report_parts`, `rebind_pbir_part` in `src/commands/report_pbir.rs` are unit-tested; e2e in `tests/e2e_report.rs` (`report_validate_pbir_folder_offline`, `report_validate_and_create_from_folder_lifecycle`).

### Deploy synthesis of a raw Desktop PBIP (no `.platform`)
Power BI Desktop saves a PBIP as plain `<name>.Report` / `<name>.SemanticModel`
folders WITHOUT the Git-integration `.platform` sidecar (that file is only added
by Fabric Git Integration or fabric-cicd). `fabio deploy plan/apply/validate`
now discover such folders directly:
- `synthesize_platform_metadata(path, dir_name)` (in `deploy/platform.rs`) infers
  the item from the folder-name suffix: `<name>.Report` → (Report, entry file
  `definition.pbir`), `<name>.SemanticModel` → (SemanticModel, entry file
  `definition.pbism`). It returns `Some(..)` ONLY when the suffix maps to one of
  these two PBIP types AND the required entry-point definition file exists on
  disk (and the base name is non-empty). Any other folder returns `None` and is
  recursed into as a plain workspace folder — an arbitrary directory is never
  misclassified as an item.
- The synthesized `PlatformMetadata` has `logical_id: None` (no authored
  logicalId exists), so rename tracking is unavailable — `deploy plan` emits a
  `"… has no logicalId in .platform — rename tracking won't work"` warning, and
  items match deployed items by `(type, name)`.
- Report→model binding still works WITHOUT modifying the resolver: the model
  deploys first (SemanticModel precedes Report in `DEPLOY_ORDER`) and is
  registered in the name→id map; the report's `definition.pbir` byConnection
  carries `initial catalog=<model_name>` (v2 PBIR), which
  `resolve_report_byconnection_model_id` rewrites to the newly-created model's
  `semanticmodelid`. (v1 PBIR-Legacy, which sets only `pbiModelDatabaseName`, is
  not name-rebound — but Desktop PBIP emits v2.)
- `.pbi/` Desktop user state is excluded from definition parts via the existing
  `read_parts_recursive` skip; the root `<name>.pbip` pointer and `.gitignore`
  are files (not item dirs) and are ignored by discovery.
- Live-validated: export `SalesReport` + `sales_semantic_model` → delete every
  `.platform` → `deploy plan` discovers/types both (with the no-logicalId
  warning) → `deploy apply` to a fresh workspace creates both AND the deployed
  report's `semanticmodelid` matches the newly-created model's id.
- Pure `synthesize_platform_metadata` + a `parse_source_directory` discovery test
  are unit-tested in `platform.rs`; e2e `deploy_plan_raw_pbip_without_platform_is_discovered`
  in `tests/e2e_deploy.rs`.

### PBIR body-schema conformance is INFEASIBLE offline (Fabric emits unpublished `$schema` versions)

`report validate` performs **structural + `$schema`-presence** checks, NOT
per-property JSON-Schema conformance. A spike to add full offline JSON-Schema
conformance (validating every PBIR file against Microsoft's published schemas at
`github.com/microsoft/json-schemas`) established that **body-file conformance is
impossible offline**, and only the stable `definitionProperties` schemas could be
validated:

- **Real Fabric reports declare body-schema versions that do NOT exist upstream.**
  A live export (workspace `fabio-e2e-dest`, report `SalesReport`) declares, in
  its definition files:
  - `definition/report.json` → `report/definition/report/**3.3.0**` (upstream max: 3.3.0 — OK)
  - `definition/pages/<p>/page.json` → `page/**2.1.0**` (upstream max: 2.1.0 — OK)
  - `definition/pages/<p>/visuals/<v>/visual.json` → `visualContainer/**2.11.0**`
    — **upstream max is 2.9.0**; `2.11.0` is a **404** in the published repo AND
    at `developer.microsoft.com`.
  - `definition/pages/pages.json` → `pagesMetadata/1.1.0`;
    `definition/version.json` → `versionMetadata/1.0.0`.
  So Fabric's runtime emits `$schema` URLs **ahead of** (or disconnected from) the
  published `microsoft/json-schemas` repo. Any vendored body schema will therefore
  never match a real file's declared version → the file is skipped, so real
  reports get **zero** body conformance. Falling back to the latest *published*
  version (e.g. validate a `2.11.0` visual against vendored `2.9.0`) is WORSE:
  every body schema is `additionalProperties: false`, and 2.9→2.11 adds
  properties, so a valid real report would emit **false** `SCHEMA_VIOLATION`
  errors on the newer properties.
- **Only `definitionProperties` is stable + published + declared by real files.**
  `definition.pbir` declares `report/definitionProperties/2.0.0` (upstream has
  exactly `1.0.0`, `2.0.0`) and a real `definition.pbir` **conforms cleanly**
  (0 errors) against the vendored 2.0.0 schema. `definition.pbism` declares
  `semanticModel/definitionProperties/1.0.0` (the only published version).
  These are the files fabio itself emits and agents hand-author — but full
  conformance on them adds only strictness (unknown-property / type checks) over
  the existing structural checks (`MISSING_VERSION`, `MISSING/AMBIGUOUS_DATASET_REFERENCE`,
  `BYPATH_NEEDS_BYCONNECTION`), which was judged not worth promoting the
  `jsonschema` crate from a dev-dependency to a **runtime** dependency (it pulls
  `fraction`, `num-bigint`, `fancy-regex`, `ahash`, `referencing`, …).
- **Implementation notes (for any future retry):** the vendored-schema retriever
  approach works — `jsonschema` (already a dev-dep) validates offline via a custom
  `Retrieve` impl over an in-memory `HashMap<url, Value>`, keying each doc by both
  its `SCHEMA_BASE+rel` URL and its `$id` (embedded schemas are fetched at
  `schema-embedded.json` but self-identify as `schema.embedded.json`). One upstream
  wart: some schemas (e.g. `bookmark/1.0.0`) use generic-style `definitions` keys
  containing `<`/`>` (`DecomposedTree<QueryExpressionContainer>`) which are invalid
  URI-fragment characters — a strict `$ref` URI parser rejects them; percent-encode
  `<`→`%3C`/`>`→`%3E` in `$ref` values (JSON-Pointer semantics are preserved). The
  full transitive `$ref` closure of the report/semantic-model definition schemas is
  only ~385 KB raw / ~41 KB gzip (largest single file `semanticQuery` ~72 KB), so
  size was never the blocker — the version-drift mismatch is.

**Conclusion:** closed as "won't implement". Structural + `$schema`-presence
validation stays; per-property conformance is not viable until Fabric's emitted
body-schema versions are published in `microsoft/json-schemas` in lockstep.

### Known gaps / roadmap (not yet implemented)
- Report scaffolding from a compact spec (pages/visuals) — emit schema-conformant PBIR from a high-level agent description.

## Analysis Services specs → fabio surface (semantic-model introspection)

Fabric/Power BI Premium tabular semantic models ARE Analysis Services tabular
models (compat level 1200+), so they inherit the AS reference specs
(https://learn.microsoft.com/analysis-services/analysis-services-references).
Mapping to what fabio (a REST CLI) can reach:

| AS spec | Fabric relation | fabio surface |
|---|---|---|
| **TMSL** (`model.bim`, JSON) | semantic model definition (TMSL format, version 1.0) | `semantic-model create/update-definition --file model.bim` |
| **TMDL** (per-object folder) | semantic model definition (version 4.0+) | `--file *.tmdl`; deploy handles `definition/` folder |
| **DAX** | query language | `semantic-model query --dax` (Power BI `executeQueries` REST) |
| **Schema Rowsets** (`TMSCHEMA_*`/`DISCOVER_*` DMVs) | model metadata | `semantic-model list-tables/list-columns/list-measures/list-relationships` via DAX `INFO.VIEW.*` |
| **Power Query M** | mashup/queries | `dataflow execute-query --mashup` |
| **XMLA / TOM / AMO / ADOMD.NET** | SOAP protocol + .NET client libs | NOT reachable from a Rust REST CLI (need an AS client); fabio uses the Power BI REST API instead |
| **MDX** | multidimensional query | N/A — Fabric semantic models are tabular, not multidimensional |

### Schema introspection via DAX `INFO.VIEW.*` (live-verified)
- **`INFO.VIEW.TABLES()` / `COLUMNS()` / `MEASURES()` / `RELATIONSHIPS()` WORK** through the Power BI `executeQueries` endpoint (the standard `semantic-model query` path). They return readable model metadata WITHOUT fetching/parsing the TMDL/TMSL definition. Backs `semantic-model list-tables/list-columns/list-measures/list-relationships`.
- **The raw `INFO.TABLES()` / `INFO.COLUMNS()` / etc. FAIL** over `executeQueries` (HTTP 400 `DatasetExecuteQueriesError`) — they return columns/types that the REST query serializer rejects. Always use the `INFO.VIEW.*` variants (added 2024, designed for readability).
- Result columns come back DAX-bracketed (`[Name]`, `[StorageMode]`, …); fabio strips the brackets (`strip_bracket_keys`) so output keys are agent-friendly (`Name`, `StorageMode`). Empty results (e.g. a model with no measures/relationships) render as a clean `{"data":[],"count":0}`.
- Rich metadata surfaced: tables (StorageMode incl. `Direct Lake`, DataCategory, IsHidden, Expression, LineageTag), columns (DataType, SummarizeBy, FormatString, SourceColumn, IsKey/IsUnique/IsNullable), measures (Expression, FormatString, DisplayFolder, State), relationships (From/To table+column, Cardinality, CrossFilteringBehavior, IsActive).
- Pure helper `strip_bracket_keys` in `src/commands/semantic_model/operations.rs` is unit-tested; the full create→introspect(tables/columns/measures/relationships)→delete loop is live-validated in `tests/e2e_semantic_model.rs` (`semantic_model_schema_introspection_lifecycle`).

### Enhanced/granular refresh + lifecycle (TMSL refresh over REST — implemented)
- `semantic-model refresh` maps the TMSL `refresh` command's granular options onto the Power BI enhanced-refresh API (`POST /datasets/{id}/refreshes`). Basic refresh sends `{type}`; adding `--objects` (JSON array of `{table, partition?}`), `--commit-mode` (transactional|partialBatch), `--max-parallelism`, or `--retry-count` produces an ENHANCED-API refresh (`refresh-status` reports `refreshType: ViaEnhancedApi` vs `DirectLakeFraming`/`ViaApi`). `--objects` refreshes specific tables/partitions (e.g. reframe one Direct Lake table, or refresh one incremental partition). Live-verified: a granular `--objects '[{"table":"Sales"}]'` on the Direct Lake model registered as `ViaEnhancedApi`.
- **Lifecycle**: `refresh-status` returns each refresh's `requestId` (+ `extendedStatus`, `refreshAttempts`). **`semantic-model refresh-details --refresh-id <requestId>`** (`GET /datasets/{id}/refreshes/{requestId}`) returns per-request enhanced detail — `type`, `commitMode`, `status`, `currentRefreshType`, `numberOfAttempts`, and OBJECT-LEVEL status (`objects: [{table, partition, status}]`). **`semantic-model cancel-refresh --refresh-id <requestId>`** (`DELETE /datasets/{id}/refreshes/{requestId}`) cancels an in-progress enhanced refresh (`--dry-run`-guarded, `destructive: true` — it kills a running job); cancelling a completed job returns a clean 409 `CONFLICT` ("has status 'Completed' and cannot be cancelled"). Live-verified: trigger → refresh-details (object-level status) → cancel (`cancellation_requested` while running).
- The pure body builder `build_refresh_body` + validators `parse_refresh_objects`/`normalize_commit_mode` are unit-tested; the enhanced body + validation errors are asserted via `--dry-run`, and the trigger→details→cancel loop is live-validated (`semantic_model_enhanced_refresh_lifecycle`).

### Scheduled (automatic) refresh — Power BI `refreshSchedule`
- **`semantic-model get-refresh-schedule`** (`GET .../refreshSchedule`) and **`update-refresh-schedule`** (`PATCH .../refreshSchedule`) configure automatic refresh for import/Direct Lake models. Body shape: `{"value":{"days":[weekday names],"times":["HH:00"/"HH:30"],"enabled":bool,"localTimeZoneId":str,"notifyOption":NoNotification|MailOnFailure|MailOnCompletion}}`. fabio exposes typed flags (`--enabled/--days/--times/--local-time-zone-id/--notify-option`); only provided fields are PATCHed (partial update).
- **Two live-learned API constraints, enforced client-side with clear hints**: (1) times MUST be on the full or half hour (`HH:00`/`HH:30`) — a `07:15` PATCH returns "Refresh schedule time must be full or half hour"; (2) **disabling must be sent ALONE** — `{"value":{"enabled":false,"times":[]}}` returns "Refresh schedule disable should not modify other settings", so fabio sends ONLY `{"value":{"enabled":false}}` when `--enabled false` and refuses `--enabled false` combined with other flags.
- `directQueryRefreshSchedule` is a SEPARATE endpoint (different body: `frequency`/`days`/`times`, no `enabled`/`notifyOption`) that only works on DirectQuery/Live-Connection models (returns "This API can only be called on a DirectQuery or Live Connection dataset" on import/Direct Lake). Reach it via `fabio rest call --api powerbi` when needed.
- Pure `build_schedule_body` + validators `validate_schedule_time`/`normalize_days`/`normalize_notify_option` are unit-tested; the get→enable→verify→disable loop is live-validated (`semantic_model_refresh_schedule_lifecycle`).

### Folder-based model create (full multi-file TMDL — implemented)
- A real TMDL semantic model is a FOLDER (`definition.pbism` + `definition/{model,database,expressions}.tmdl` + `definition/tables/*.tmdl` + optional relationships/cultures/roles), but `semantic-model create --file` only sends ONE file. **`semantic-model create --definition <folder>`** gathers the whole folder recursively (excluding `.platform`, `.pbi/`, and deploy sidecars) and creates the model — the way a multi-file TMDL model ships (previously only `deploy` could push it, and only with `.platform` scaffolding). Validated first (`validate_model_folder`: needs `definition.pbism` + a model body — `model.bim` or `definition/model.tmdl`). Pure `gather_model_parts`/`validate_model_folder`/`build_single_file_parts` are unit-tested; the export→create-from-folder→introspect→delete loop is live-validated (`semantic_model_create_from_tmdl_folder_lifecycle`). This is the semantic-model analog of `report create --definition <folder>` (PBIR).

### Dataset gateway binding (Power BI) + non-reachable dataset ops
- **`semantic-model get-bound-gateway-datasources`** (`GET .../Default.GetBoundGatewayDatasources`) lists the gateway data sources bound to a model — read-only, live-verified (returns `{value:[]}` for a cloud/Direct Lake model with no gateway sources). **`semantic-model bind-to-gateway --gateway-id <id> [--datasource-ids a,b]`** (`POST .../Default.BindToGateway`, body `{gatewayObjectId, datasourceObjectIds?}`) binds an import model's sources to an on-premises/VNet gateway; `--dry-run`-guarded; pure `build_bind_body` unit-tested. The bind HAPPY-PATH needs an actual gateway + gateway-eligible data sources — on a cloud/Direct Lake model it is a harmless no-op (returns `null`; the model's binding is unchanged and it stays queryable). Mirrors the existing `bind-connection`/`unbind-connection` pair.
- **NOT added (probed, not cleanly reachable/valuable on this tenant)**: `Default.DiscoverGateways` consistently returns 401 (needs gateway-admin permission and a gateway-eligible model); `executeQueries` with `impersonatedUserName` (RLS-as-user testing) returns 404 `PowerBIEntityNotFound` even for the caller's own UPN unless the model has RLS roles defined — shipping an unvalidatable RLS flag was declined; `queryScaleOut/syncStatus` returns `StorageModeNotSupported` (Premium scale-out read replicas are not applicable to Direct Lake/import here). `directQueryRefreshSchedule` is DirectQuery/Live-only (import/Direct Lake use `refreshSchedule`). All remain reachable via `fabio rest call --api powerbi` for users whose models/permissions support them.

### Deferred (REST-reachable but not yet implemented)
- **TMSL command execution** (createOrReplace/alter/backup/restore/synchronize) and **TOM editing** require an XMLA/AS client — out of scope for a REST CLI (fabio edits definitions via the Fabric items `updateDefinition` API instead).

## Item Definition Part Requirements & Offline Validation (live-verified)

Ground-truthed live against the tenant by creating items, capturing `getDefinition`, and
round-tripping `updateDefinition`. Source of truth: `src/commands/context/data/agent/definition_requirements.json`
(loaded by `src/definition_spec.rs`), which powers `fabio item validate-definition`,
definition-authoring error hints, and the `definition_requirements` block merged into
`fabio context schema <Type>`.

### Canonical part paths differ from some fabio emitters — Fabric is LENIENT
- The Fabric `updateDefinition` API tolerates alias part filenames: sending `CopyJobV1.json`
  (instead of the canonical `copyjob-content.json`) or `dataflow.json` (instead of the canonical
  `queryMetadata.json`) is accepted and its content is parsed/validated — but `getDefinition`
  (and Git-integration export / `deploy`) always return the CANONICAL paths. Proven by sending
  deliberately invalid content under each alias: Fabric returned the SAME JSON-parse error under
  both the alias and the canonical path, so both are read.
- **CopyJob**: canonical part is `copyjob-content.json` = `{"properties":{"jobMode":"Batch"},"activities":[]}`.
- **Dataflow**: canonical parts are `queryMetadata.json` (settings: `{"formatVersion":"202502","computeEngineSettings":{"allowFastCopy":false},"name":null,"allowNativeQueries":false}`) **+** `mashup.pq` (the Power Query M script, starts `section Section1;`). `queryMetadata.json` is REQUIRED — sending only `mashup.pq` fails (`Unexpected character encountered while parsing value: s`, i.e. Fabric tried to JSON-parse the M script). A full 2-part envelope round-trips cleanly.
- **SparkJobDefinition**: the part FILENAME is `SparkJobDefinitionV1.json` but the `definitionFormat` is `SparkJobDefinitionV2` — do not confuse them. Content = `{"executableFile","defaultLakehouseArtifactId","mainClass","additionalLakehouseIds":[],"retryPolicy","commandLineArguments","additionalLibraryUris","language","environmentArtifactId"}`.
- **DataPipeline**: `pipeline-content.json` = `{"properties":{"activities":[...]}}` (empty: `{"properties":{"activities":[]}}`).

### fabio alignment fixes (align emitters with Fabric)
- `copy-job update-definition` now emits `copyjob-content.json` (was `CopyJobV1.json`).
- `dataflow update-definition` was effectively a NO-OP for real content: it wrapped the input as a
  single `dataflow.json` part, which Fabric parsed but IGNORED (the canonical `queryMetadata.json` +
  `mashup.pq` were left unchanged). It now uses the shared `definition_spec::build_update_definition_body`,
  which PASSES THROUGH a full envelope (`{"definition":{"parts":[...]}}` / `{"parts":[...]}`) verbatim —
  so the reliable pattern is `get-definition` → edit parts → `update-definition --file <envelope.json>`.
  A single raw file still wraps under the type's canonical part path. Applied to `copy-job`,
  `dataflow`, and `spark-job-definition`; the other type-specific `update-definition` commands already
  emit their canonical single part.

### Offline validator (`fabio item validate-definition`)
- Read-only, no API call. Inputs: `--file` (JSON envelope), `--definition` (inline JSON), or `--dir`
  (a folder of parts assembled into an envelope). `--type <T>` enables per-type canonical-part checks.
- Universal envelope rules are ERRORS (deterministic, zero false positives on real definitions):
  `MISSING_PARTS`, `EMPTY_PARTS`, `MISSING_PART_PATH`, `MISSING_PAYLOAD_TYPE`/`INVALID_PAYLOAD_TYPE`
  (only `InlineBase64` is valid), `MISSING_PAYLOAD`, `INVALID_BASE64`, `INVALID_JSON_PART`,
  `DUPLICATE_PART`. Per-type canonical-part gaps are WARNINGS (`MISSING_CANONICAL_PART`,
  `MISSING_ONE_OF`, `UNKNOWN_ITEM_TYPE`, `PLATFORM_MISSING_METADATA`) — because Fabric tolerates
  aliases — promoted to failures with `--strict`. Verified `--strict` clean (0 warnings) against real
  exported CopyJob/Dataflow/DataPipeline/SparkJobDefinition/Notebook folders.

## Ontology MCP Server (Preview) — live-verified

A Fabric ontology (preview) item can be consumed as a Model Context Protocol (MCP)
server by external AI systems. `fabio ontology mcp-url --workspace <ws> --id <id>`
constructs the endpoint (agents cannot guess it).

- **URL format** (per Microsoft docs, verified live):
  `{fabricBase}/mcp/dataPlane/workspaces/{ws}/items/{id}/ontologyEndpoint`
  = `https://api.fabric.microsoft.com/v1/mcp/dataPlane/workspaces/{ws}/items/{id}/ontologyEndpoint`.
  This DIFFERS from the data-agent MCP URL (`/mcp/workspaces/{ws}/dataagents/{id}/agent`):
  ontology uses the generic `dataPlane/.../items/...` path with an `ontologyEndpoint` suffix.
- **The endpoint is a real MCP server (HTTP transport)**. A JSON-RPC `initialize` handshake
  (`POST` the endpoint with `{"jsonrpc":"2.0","method":"initialize",...}`) returns
  `serverInfo: {"name":"Microsoft Fabric Ontology","version":"1.0.0"}`, `protocolVersion
  2025-06-18`, and `capabilities.tools` (with instructions describing ontology schema
  exploration + natural-language querying of the ontology data estate). Verified via
  `fabio rest call --api fabric --method post --path /mcp/dataPlane/workspaces/{ws}/items/{id}/ontologyEndpoint --body <initialize>`.
- **Prerequisites**: an F2+/P1 capacity and the "Ontology item (preview)" tenant setting.
  There is NO "publish" step (unlike data agents) — the endpoint is live as soon as the
  ontology item exists. fabio therefore does a light existence check (GET
  `/workspaces/{ws}/ontologies/{id}`) and surfaces an `exists` flag + prerequisite `note`,
  rather than a published/draft gate.
- **Distinct from grounding**: `data-agent add-datasource --artifact-type Ontology` grounds a
  fabio DATA AGENT on an ontology; `ontology mcp-url` exposes the ontology ITSELF as an MCP
  server. Both are valid, different integration paths.

## Ontology MCP tools vs. pure fabio (live comparison)

Enumerated both Fabric MCP servers' tools live (`tools/list`) and mapped them to fabio commands.

### Ontology MCP server (`Microsoft Fabric Ontology` v1.0.0) — 2 tools
- `list_ontology_entity_types(entityName?, includeProperties=false)` → `{"values":[<entityType>...]}`. Each entity type carries `id`, `namespace`, `name`, `namespaceType`, `baseEntityTypeId` (only when it inherits), `entityIdParts`, `displayNamePropertyId`, `visibility`, `properties`/`timeseriesProperties`/`untypedProperties` (each `{id,name,valueType}`), `documents`/`mappings`/`resourceLinks`, and a server-assigned `etag`. `includeProperties=false` empties the three property arrays; `entityName` filters by exact name; ordered by `id` ascending.
- `search_ontology(naturalLanguageQuery, naturalLanguageResponse)` → NL query over the ontology data estate (server-side Fabric IQ reasoning).

**`fabio ontology list-entity-types` reproduces `list_ontology_entity_types` byte-for-byte** (verified live for includeProperties true/false and the entityName filter). It derives the answer offline from `getDefinition`'s `EntityTypes/*/definition.json` parts: reorders fields to the tool's order (preserve_order), strips `$schema` and null property fields (`redefines`, `baseTypeNamespaceType`), and defaults `documents`/`mappings`/`resourceLinks` to `[]`. The ONLY field it cannot reproduce is the server-assigned `etag` (a per-entity concurrency token with no offline source) — it is omitted, and every other field matches value-for-value and in the same key order. `search_ontology` has NO pure-fabio equivalent (LLM reasoning over the semantic layer); the workarounds are grounding a data agent on the ontology or consuming the ontology MCP server directly.

### Data-agent MCP server (`DataAgent MCP Server` v1.0.0) — 1 tool
- `DataAgent_<name>(userQuestion)` → NL answer over the agent's configured sources. Advertises a `resources` capability but `resources/list` returns "Feature is not enabled".

**Already fully covered by `fabio data-agent query --prompt "…"`** (verified live: same published agent, same NL answer; fabio uses the OpenAI-Assistants endpoint, the tool uses MCP — same outcome). fabio also adds `data-agent evaluate` (batch). No reason for fabio to consume the data-agent MCP server.

**Net:** of the three tools across both servers, two are already achievable in pure fabio (`list_ontology_entity_types` → `ontology list-entity-types`; `DataAgent_<name>` → `data-agent query`); only `search_ontology` (ontology NL query) is a genuine gap requiring an LLM/MCP-client path.

## MCP client (fabio consuming external MCP servers) — `ontology search`

fabio gained its first MCP-CLIENT capability: `src/mcp_client.rs` is a generic
Model Context Protocol client over the streamable-HTTP transport (`McpClient::connect`
→ `initialize` handshake + `notifications/initialized`, then `list_tools`/`call_tool`).
It is the counterpart of `fabio mcp serve` (fabio as an MCP *server*). Nothing in the
module is ontology-specific — it takes an endpoint URL + an optional `Authorization`
header and returns tool results.

- **Transport behavior (Fabric ontology MCP server, verified live)**: the server is
  STATELESS — it does NOT return an `Mcp-Session-Id` header on `initialize`, and each
  JSON-RPC request is an independent POST. It responds with `application/json` (not SSE)
  for the ontology endpoint, but the client handles both `application/json` and
  `text/event-stream` (SSE `data:` events). The client advertises protocol version
  `2025-06-18`, which the server accepts.
- **Auth**: the client attaches the Fabric bearer token (`FabricClient::require_auth()`);
  the endpoint is HTTPS + trusted-host validated (`validate_trusted_url`) before the
  token is sent.
- **`fabio ontology search --workspace <ws> --id <id> --prompt "<q>" [--raw]`**: builds
  the ontology MCP URL (same as `mcp-url`), connects, confirms the server exposes
  `search_ontology` (via `list_tools`), then calls it with
  `{naturalLanguageQuery, naturalLanguageResponse=!--raw}`. Output:
  `{"query","answer","isError"}` (the tool's text content is JSON-parsed when possible).
  `--dry-run` prints the plan (endpoint + query + tool) without any network call.
- **Live validation**: fabio's `ontology search` produces BYTE-IDENTICAL behavior to a
  raw MCP `tools/call` (verified by side-by-side curl). On a tenant/ontology WITHOUT
  Fabric IQ natural-language reasoning fully provisioned, `search_ontology` returns
  `{"isError":true,"...":"The natural language query could not be processed..."}` — the
  same response fabio surfaces (it correctly transmits the query and reports the
  server's result, exiting non-zero on `isError`). A successful NL answer therefore
  needs the ontology bound to data AND the capacity's Fabric IQ/Copilot reasoning
  enabled — a server-side prerequisite, not a fabio limitation.
- **`search_ontology` argument shape**: `{"naturalLanguageQuery": "<text>",
  "naturalLanguageResponse": <bool>}`. The tool always returns raw JSON results;
  `naturalLanguageResponse=true` additionally derives an NL answer.

## Ontology generation from a semantic model (`ontology generate`) — portal-parity, client-side

The Fabric portal's "Generate Ontology" has NO public REST API (the Fabric ontology REST
surface is CRUD-only: create/get/list/update/delete). fabio reproduces it CLIENT-SIDE with
`fabio ontology generate --workspace <ws> (--semantic-model <id> | --lakehouse <id>) --name
<name> [--lakehouse <id>] [--output-owl <file>]`, from EITHER of two schema sources (exactly
one required — `resolve_schema` in `ontology/generate.rs` dispatches; omitting both errors
with a hint):

### Source A — semantic model (`--semantic-model`)

- **Reads the model schema** via the DAX `INFO.VIEW.*` functions (the same metadata the portal
  uses), reusing the existing `semantic-model list-tables/list-columns/list-relationships`
  plumbing (`fetch_info_view` in `semantic_model/operations.rs`, `pub`).
- **Keys** come from relationships: the "one"-side `ToColumn` of each relationship is marked
  `isIdentifier`. Relationships become `owl:ObjectProperty` many-side→one-side.
- **INFO.VIEW DataType mapping (live-verified)**: `Text`→string, `Integer`→long, `Number`→
  double (Power BI's floating-point "Decimal Number" surfaces as `Number`, NOT `Double`),
  Currency/Decimal→decimal, DateTime→dateTime, Boolean→boolean. A synthetic hidden
  `RowNumber-*` column (`Type`/`DataCategory` = `RowNumber`) is excluded.

### Source B — lakehouse (`--lakehouse` WITHOUT `--semantic-model`)

- **Reads the lakehouse SQL analytics endpoint's** `INFORMATION_SCHEMA.COLUMNS` (base tables
  only — joined to `INFORMATION_SCHEMA.TABLES WHERE TABLE_TYPE='BASE TABLE'`, ordered by
  `ORDINAL_POSITION`) via the new `tds_utils::execute_sql_rows` (row-returning counterpart of
  `execute_and_render_sql`) and `tds_utils::resolve_lakehouse_sql` (moved from
  `lakehouse/insights.rs` so both callers share ONE resolver — a lakehouse's SQL catalog is
  named after its `displayName`, not the connection-string DB).
- **No relationships or PK exist in a lakehouse**, so there are NO `owl:ObjectProperty` edges,
  and the FIRST column of each table (lowest `ORDINAL_POSITION`) is the identifier heuristic
  (`lakehouse_schema_from_rows`), reviewable via `ontology update-definition`.
- **The `--lakehouse` is ALSO the bind target**: each entity auto-binds to its same-named
  lakehouse table (`DataBindings/*` parts with `sourceType: LakehouseTable`, `workspaceId`,
  `itemId`, `sourceTableName`, `sourceSchema: dbo`). Verified live on a throwaway lakehouse
  (dimstore/dimproducts/factsales) → 3-entity ontology, 17 typed properties, first-column keys,
  and one `DataBindings` part per entity referencing the correct table.
- **T-SQL `INFORMATION_SCHEMA.DATA_TYPE` mapping (`map_sql_type`, live-verified)**: bit→boolean;
  tinyint/smallint/int/bigint→long; real/float→double; decimal/numeric/money/smallmoney→decimal;
  date/time/datetime/datetime2/smalldatetime/datetimeoffset→dateTime; everything else
  (varchar/nvarchar/char/nchar/text/uniqueidentifier/binary/…)→string. A `float` Latitude and a
  `decimal` RevenueUSD both surfaced as `xsd:double`/`xsd:decimal`; SaleId/Units (`bigint`)→long;
  a `datetime2` SaleDate→dateTime.
- **SQL endpoint lag**: newly `load-table`'d Delta tables surface in `INFORMATION_SCHEMA` with a
  delay (seconds to ~1 min). The e2e test polls `generate --output-owl` until the tables appear.

### Shared

- **Synthesizes an OWL model** (pure `build_owl` in `ontology/generate.rs`): each table →
  `owl:Class` (entity type); each column → typed `owl:DatatypeProperty`; keys → `isIdentifier`;
  relationships → `owl:ObjectProperty`. Columns carry a pre-resolved `Xsd` field (lakehouse path)
  or a DAX `DataType` mapped on the fly (semantic-model path).
- **Runs it through the existing `ontology import` path** (create ontology → `import_owl`).
- **`--output-owl <file>`** writes the synthesized OWL and stops (inspect/compose); `--dry-run`
  prints the plan + OWL without creating.
- **Single clean output**: `import_owl` has a `suppress_output` flag so `generate` emits one
  JSON object (`{status, id, name, source, summary, note}`) instead of the import result + its own.
- **Follow-ups (matching the portal flow, left manual)**: time-series bindings
  (`ontology bind --eventhouse ...`), entity-key review, and relationship data bindings.

Pure `build_owl`/`map_datatype`/`map_sql_type`/`lakehouse_schema_from_rows`/`relationship_keys`/
`is_synthetic_column`/`summarize` are unit-tested; live e2e `ontology_generate_from_semantic_model`
and `ontology_generate_from_lakehouse` validate both create+import paths.

### Full ontology tutorial — live end-to-end validation (all parts)

The complete Fabric ontology tutorial was reproduced live with fabio (semantic-model pivot):
- **Part 0** — `lakehouse create` + `upload` + `load-table` (DimProducts/DimStore/FactSales/Freezer); `semantic-model create` (import-mode model.bim with inline data); `eventhouse create`; `kql-database query --kql ".create table FreezerTelemetry (...)"` + `kql-database ingest --data <csv rows>` (verified `FreezerTelemetry | count` = ingested rows). ✓
- **Part 1** — `ontology generate --semantic-model <SM> --lakehouse <LH>` → entity types + typed properties + relationships + queryable lakehouse `DataBindings`. ✓
- **Part 2** — enrich with time-series: `ontology import --eventhouse <EH> --cluster-uri <URI> --database <KQLDB> --timestamp-column timestamp --bindings <map>` produced a Freezer entity `DataBinding` with `dataBindingType: TimeSeries`, `sourceType: KustoTable`, and the eventhouse cluster URI. ✓ (An entity can carry a static lakehouse binding AND an eventhouse time-series binding.)
- **Part 3** — `ontology list-entity-types` (schema/instance inspection) ✓. The relationship-graph + query-builder are portal-only VISUALIZATIONS; their data queries map to `ontology search` (NL) — whose mechanism is validated but returns `isError:true` "could not be processed" on a capacity WITHOUT Fabric IQ NL reasoning provisioned.
- **Part 4** — `data-agent create` → `add-datasource --artifact-type Ontology --artifact <ONT>` (grounds the agent on the ontology; stored as `fabricItemType: "Ontology"`) → `update-config --instructions "Support group by in GQL"` → `publish` → `query --prompt "..."`. ✓ **The data agent returns REAL grounded NL answers** — it reasons over the ontology's entities (e.g. correctly reports which data the ontology contains and which entities are absent).

**Key distinction (live-verified)**: the Fabric **data agent** NL path (Copilot/Azure OpenAI) works for ontology grounding on this capacity, whereas the ontology's **native** `search_ontology` MCP tool (Fabric IQ reasoning) returns "could not be processed" — they are backed by different server-side AI features with different tenant/capacity prerequisites. fabio faithfully drives both and surfaces each one's real result.

### CORRECTION — why `ontology search` returns "could not be processed" (root cause: portal-only graph init, NOT capacity)

An earlier note attributed the `ontology search` / `search_ontology` failure to "Fabric IQ not
provisioned". That was IMPRECISE. Root cause, established empirically:

- An ontology (preview) item spawns a hidden **`GraphModel` child item** (`<name>_graph_<id>`) plus
  an internal lakehouse (`<name>_lh_<id>`). Both `ontology search` (NL) and direct GQL queries run
  against this backing graph.
- The backing graph is **not queryable until it is INITIALIZED, which is a PORTAL-ONLY operation**
  (no public REST API). Direct proof: `graph-model execute-query` on a freshly fabio-created
  ontology's graph returns `GraphNotQueryable: GraphIsNotLoaded`, and `graph-model initialize`
  reports *"Graph model initialization is a portal-only operation. The REST API refresh fails with
  'VersionConfig does not exist' until the portal provisions the internal loading infrastructure."*
  fabio can trigger `graph-model refresh-graph`, but refresh only works AFTER the portal has
  provisioned the graph's loading infrastructure the first time.
- Therefore `search_ontology` returns `isError:true` "could not be processed" for a fabio-created
  ontology whose graph was never opened in the portal. DIRECTLY VERIFIED (not inferred): on the
  SAME ontology, `fabio ontology search` returns "could not be processed" AND `graph-model
  execute-query` returns `GraphIsNotLoaded`; running `graph-model refresh-graph` and re-testing
  leaves BOTH failing identically — refresh alone does not load the graph without the prior
  portal initialization. A **portal-created** ontology (like the
  tutorial's) is initialized automatically, so its search works.
- **It is NOT a capacity issue.** The capacity was F8 (>> the F2 minimum); there is no separate
  "Fabric IQ" tenant/capacity setting; and the Fabric **data agent** grounded on the SAME ontology
  returned real answers on the SAME F8 capacity (it reasons over the ontology schema + bound data
  via a different path that does not require the loaded graph). A bigger capacity does NOT fix this.
- **Fix / workaround**: open the ontology (or its `GraphModel` child) once in the Fabric portal to
  initialize it, then `fabio graph-model refresh-graph` loads it and `ontology search` /
  `graph-model execute-query` work. There is currently no public REST API to initialize the graph,
  so fabio cannot fully bootstrap it headlessly.

### Bug fix — `ontology import`/`generate` empty `displayNamePropertyId` for numeric-only entity types

`ontology import` (and `ontology generate`, which reuses the import path) rejected with
`ALMOperationImportFailed: DisplayNamePropertyId cannot be empty or whitespace. (Parameter
'entityType')` whenever an entity type had **no String property** — e.g. a fact table bound
to `SaleId:long` + `RevenueUSD:double`. Cause: `generate_fabric_parts` assigned
`displayNamePropertyId` ONLY from the first `String` property; if an entity had none,
`display_name_id` stayed `None` and was serialized as **`""`** (empty string), which Fabric
rejects. **The rejection was specifically of the empty STRING — JSON `null` is accepted** (see
the portal-comparison note below, which supersedes the original fabricate-a-fallback fix).

### `ontology generate` — portal-parity comparison (fabio vs. Fabric server-side "Generate ontology")

We generated an ontology from the SAME Direct Lake semantic model two ways — fabio
`ontology generate --semantic-model <SM> --lakehouse <LH>` and the portal's **Generate
ontology** — and diffed the decoded definitions part-by-part. Result: **structurally
identical** (same part layout, same relationship types `{From}_has_{To}` with the same
source/target, same property `valueType`s String/Double/BigInt/DateTime, same `DataBindings`
column→property maps and `sourceTableProperties`). The portal's generate has NO public REST
API — it is a browser-UI-only action (probed exhaustively: `POST .../ontologies/generate`,
`.../generateFromSemanticModel`, `.../semanticModels/{id}/generateOntology`, and
`creationPayload`/`generationSource` bodies all 404 or are silently ignored), and it appears
to run the SAME kind of client-side schema→definition transformation fabio does.

The diff surfaced three fabio bugs (now FIXED in `generate_fabric_parts`) and two deliberate,
schema-justified deviations:

**Fixed to match the portal:**
1. **Fabricated keys.** fabio set `entityIdParts` to the first property when no key was marked
   — so a fact table with no relationship "one"-side got a bogus key (`FactSales.entityIdParts
   = [SaleId]`). The portal leaves such tables **keyless** (`entityIdParts: []`). Fix: keys now
   come ONLY from explicit identifiers (relationship "one"-side / `ont:isIdentifier`); no
   first-property fallback. Dimensions still get their natural key (both tools agree
   `DimStore=[StoreId]`, `DimProducts=[ProductId]`).
2. **Inconsistent / forced display name.** fabio always set `displayNamePropertyId`, and via a
   SEPARATE fallback path it could disagree with the key (`FactSales`: `entityIdParts=[SaleId]`
   but `displayNamePropertyId=StoreId`). The portal leaves `displayNamePropertyId: null` on
   EVERY generated entity. Fix: fabio now emits **`null`** (JSON null, not `""`) — the official
   `entityType/1.0.0` schema types the field `["string","null"]`, and `updateDefinition`
   accepts null at runtime (live-confirmed; the earlier "cannot be empty" error was only for the
   empty STRING). This also removes the need for the fabricate-a-fallback logic.
3. (These two together mean fabio's generated `entityIdParts` + `displayNamePropertyId` now
   match the portal byte-for-byte on the retail model: dims keyed, fact keyless, display names
   null.)

**Deliberate deviations where fabio is MORE correct than the portal:**
4. **`namespaceType`.** The portal emits `"Imported"` for generated entity/relationship types.
   The **official Microsoft schema** (`.../ontology/entityType/1.0.0/schema.json`) declares
   `namespaceType` as **`const: "Custom"`** ("should always be Custom"). So the portal
   **violates its own published schema**; fabio keeps `"Custom"` and stays schema-conformant
   (fabio's `assert_schema_valid` conformance test would reject `"Imported"`). NOT changed.
5. **`DataBinding.sourceSchema`.** The portal leaves it `null`; fabio emits `"dbo"` (the actual
   lakehouse schema — explicit and correct). Kept as-is.

Comparison harness (decode + per-entity diff of two ontology definitions) lives at
`/tmp/opencode/compare_onto.py`; the fix is unit-tested (`keyed_entity_uses_identifier_and_null_display_name`,
`keyless_entity_left_keyless_like_portal`) and live-validated (regenerated ontology diffs clean
against the portal except the two intentional deviations above).

## Deploy — same-run cross-item connection resolution (tutorial-scenario validation)

Deploying a multi-item scenario end-to-end (Lakehouse + Direct Lake SemanticModel +
Eventhouse + KQLDatabase + Ontology) via `deploy export` → `deploy apply` to a fresh
workspace surfaced (and we fixed) five gaps in cross-item wiring. Live-validated: all four
connection types now resolve to the DEPLOYED items on the target, and both post-hooks
(`sql_endpoint_poll: ready`, `refresh: triggered`) are green.

- **Export dropped source GUIDs (`logicalId: 0000…`)** — the root cause. `deploy export` now
  writes each item's `.platform` `logicalId` = its SOURCE item GUID (when there is no real Git
  logicalId). Deploy's existing `build_resolution_map` + `resolve_logical_ids_in_payload` then
  rewrite ANY reference holding another item's source GUID to the deployed GUID. This fixes,
  generically: Ontology `DataBinding.itemId` → deployed Lakehouse (previously only `workspaceId`
  was rewritten by the regex, leaving `itemId` pointing at the SOURCE lakehouse); and
  KQLDatabase `parentEventhouseItemId` → deployed Eventhouse. (`stable_logical_id` in `export.rs`.)

- **Direct Lake SemanticModel connection was NOT rewired** — a Direct Lake model's
  `Sql.Database("<server>","<sqlEndpointId>")` references the lakehouse's SQL *analytics
  endpoint* (a separate auto-provisioned item) + server FQDN, neither of which is a logicalId.
  Fix: `deploy export` writes a `sqlendpoint.metadata.json` sidecar per Lakehouse
  (`{id, server}` = source endpoint id + connectionString). On apply, an inter-tier hook
  (`populate_sql_endpoint_resolutions`) — after the Lakehouse tier, only for lakehouses whose
  source endpoint id/server is actually referenced by another item's parts — polls the DEPLOYED
  lakehouse's SQL endpoint and registers `source_endpoint_id → target_endpoint_id` and
  `source_server → target_server` in an `extra_resolutions` map merged into the resolver. The
  deployed model's connection is then rewired to the target endpoint (live-verified: both server
  FQDN and endpoint GUID replaced). `$items.*.sqlendpointid` params remain broken/unused; this
  zero-config path supersedes them for the Direct Lake case.

- **SemanticModel refresh post-hook 404'd (`EntityNotFound`)** — a deploy-created Direct Lake
  model is **definition-managed** and rejects a refresh until it is **taken over**. Fix:
  `refresh_semantic_model` now does a best-effort `POST /groups/{ws}/datasets/{id}/Default.TakeOver`
  (Power BI API) THEN refreshes via `POST /groups/{ws}/datasets/{id}/refreshes` (the Power BI
  datasets endpoint — the Fabric `/semanticModels/{id}/refreshes` path 404s for definition-managed
  models). Post-hook now reports `triggered` instead of `failed`. NOTE: framing still needs DATA
  in the target lakehouse (deploy moves item DEFINITIONS, not Delta tables), so the async refresh
  may still fail to frame over an empty lakehouse — that is expected, not a deploy bug.

- **Ontology auto-created children were exported as top-level items** — an Ontology spawns an
  internal Lakehouse `<name>_lh_<id>` and a backing GraphModel `<name>_graph_<id>` (auto-created
  when the Ontology is (re)created — confirmed on the target). `deploy export` now excludes them
  (`ontology_child_names`, derived exactly from each Ontology's id so a user's own `*_lh_*`/
  `*_graph_*` item is never falsely dropped).

- **Deploy poll URLs 404'd (malformed)** — `poll_lakehouse_sql_endpoint`, `poll_environment_publish`,
  and the `folders.rs` helpers built paths as `format!("workspaces/…")` with NO leading slash;
  `client::fabric_url` prepends the base (`https://api.fabric.microsoft.com/v1`, no trailing slash)
  verbatim, so the URL became `…/v1workspaces/…` → an IIS `HTTP 404` (not a Fabric JSON 404). Fixed
  to `/workspaces/…`. This is why the lakehouse SQL-endpoint poll always timed out with a 404.

Reproduction/validation harness: source workspace built with the tutorial items, exported, and
applied to throwaway target workspaces (`/tmp/opencode/deploy.env`, `/tmp/opencode/deploy_src4`).
New helpers unit-tested (`stable_logical_id`, `ontology_child_names`, `read_source_sql_endpoint`,
`source_references_any`).

## Fabric features WITHOUT a public REST API (verified live, not implementable in the CLI)

A live-probe pass confirmed the following
features have NO public REST surface, so they cannot be added to fabio (documented here
to prevent re-investigation):

- **Fabric Maps Tilesets (PMTiles)** — Generated by a portal wizard (Tileset Builder) from
  GeoJSON in a lakehouse; the output PMTiles are stored as ordinary lakehouse files and the
  refresh schedule is defined in the wizard. There is no `/maps/{id}/tilesets` (or equivalent)
  REST endpoint — `map` items remain generic CRUD + definition only. Tileset generation is a
  Spark/portal job, not an API operation.
- **Azure Monitor Logs mirroring ("Mirrored Azure Monitor" item, Preview)** — The new item type
  is NOT REST-creatable: `item create --type MirroredAzureMonitor|MountedAzureMonitor|AzureMonitorLogs`
  all return `InvalidItemType`, and the type is absent from the valid item-types list. Portal-only
  for now (unlike `MirroredDatabase`/`MirroredWarehouse`, which ARE valid item types).
- **"Allow Contributors/Members to change Git branch" (Preview)** — A per-workspace Git toggle with
  no REST endpoint: `GET /workspaces/{ws}/git/settings` and `/git/configuration` both return
  `EntityNotFound`; the `git/connection` response carries only `gitProviderDetails`/`gitSyncDetails`/
  `gitConnectionState`/`gitConnectionType` (no branch-switch-permission field). The setting is
  configured only in the portal's Git integration settings.
- **Runtime Release Channels (Preview)** — No `releaseChannel` (or similar) field on
  `GET /workspaces/{ws}/spark/settings` (which exposes only `environment.runtimeVersion`). Opting
  into an early-access channel is a portal-only workspace setting.

Other items that are automatic/UI-only (no CLI action needed or possible): AI Functions
(notebook/Spark runtime), usage-based resource estimations (automatic query optimizer), Real-Time
Dashboard tile-error UX, MLV Analytics & Insights (portal reporting), Operations Agent Investigator
Insights (Teams/portal), the Anomaly Detector configurations pane, and the OneLake catalog Govern
recommended-actions view. The Spark native-execution-engine/remote-shuffle/diagnostic-emitter and
"Enable Runtime 2.0" features ARE addressable — via `environment update-staging-spark-compute
--runtime-version/--spark-property` and `spark update-settings --runtime-version` (shipped). Custom
CA/mTLS and Event Hubs Workspace Identity auth are connection-layer concerns (Event Hubs unblocked by
`connection create --creation-method EventHub.Contents`). The Lakehouse table health check is shipped
as `lakehouse table-health`.

## Fabric REST API spec sync — 4a7d6e4 (Aug 2026): Plan item type, admin network policy filter, eventstream source/destination types

Sync against `microsoft/fabric-rest-api-specs` commit `4a7d6e4` (`admin/`, `common/`, `eventstream/`,
`plan/` (new), `platform/`). Full inventory below; every diff hunk was mapped to fabio and either
implemented or confirmed to require no code change.

- **New `Plan` item type (Connected Planning / "infobridge")**: A brand-new top-level command group
  `fabio plan {list,show,create,update,delete,get-definition,update-definition}` was added
  (`src/commands/plan.rs`), covering the full CRUD + definition surface at
  `/workspaces/{workspaceId}/plans[/{planId}][/getDefinition|/updateDefinition]`. Like most Fabric
  item types, create/update-definition are LRO-polled. `list` supports the standard
  `recursive`/`rootFolderId`/`continuationToken` query params. The canonical (and currently only)
  definition part is `connectedPlanning/infobridge.json` with `definitionFormat: "PlanV1"`.
  `get-definition` accepts an optional `--format` query parameter (mirrors the `ontology`/
  `sql-database`/`kql-database`/`lakehouse` convention: `?format=<value>` appended only when
  supplied — defaults to the server's canonical `PlanV1` format when omitted). `Plan` was added to
  `KNOWN_ITEM_TYPES`, `DEPLOY_ORDER` (deploy is generic content-hash based; Plan needs no special
  casing), and given its own skill family (`data/skills/planning.json`) plus
  `definition_requirements.json` entry. Plan has no `hardDelete` semantics and is not in
  `PROTECTED_DELETE_TYPES` (it holds planning metadata, not bulk data) or `SHELL_ONLY_TYPES` (it DOES
  support `getDefinition`/`updateDefinition`, unlike shell-only types such as Warehouse/SQLDatabase).
- **Admin network communication policy `filter` query parameter + response enrichment**:
  `GET /admin/workspaces/networking/communicationpolicies` gained an OData-style `filter` query
  parameter (e.g. `inbound/publicAccessRules/defaultAction eq 'deny'`) and three new response
  fields: `workspaceName`, `workspaceType` (sibling to the existing `workspaceId`), and per-workspace
  `inbound.firewall.rules[]` (`{displayName, value}` IP-range allow rules) and
  `outbound.managedPrivateEndpoints[]` (`{id, name, targetPrivateLinkResourceId,
  targetSubresourceType, provisioningState, connectionState}`). `fabio admin list-network-policies`
  already exposed `--filter` and rendered these fields (implemented in a prior sync session) — no
  further code change needed this round; re-verified against the updated example JSON
  (`ListNetworkingCommunicationPolicies.json`, and the new
  `ListNetworkingCommunicationPoliciesFilteredByInbound.json` example) which also shows a legacy
  `"Maria DB"` outbound connection-type rule being removed from the sample data (cosmetic example
  cleanup, not a schema change) and the pagination token values switching to URL-encoded form
  (`%3D` instead of raw `=` — an encoding-hygiene fix in the example only, `get_list`'s
  `continuationToken` handling already treats the token as an opaque string so this needed no code
  change).
- **Eventstream new source/destination types (generic passthrough, already covered)**:
  `eventstream/definitions/source.json` added `AzureIoTHubExtended`, `OracleDBCDC`, and
  `MirroredDatabaseChangeFeed` source types; `eventstream/definitions/destination.json` added a
  `Notebook` destination type; and the existing custom-endpoint HTTP source gained
  `responseDataJsonPointer` + a `pagination` object (`{type, initialPage, pageIncrement}`) for
  paginated REST polling. All of these were confirmed (in a prior sync session) to already round-trip
  correctly through `fabio eventstream add-source`/`add-destination`'s generic
  `--properties <json>` passthrough — the source/destination `type` enum values and their
  `properties` bag are not individually modeled in fabio (by design, to avoid churn on every new
  connector), so no code change was needed. The updated `GetEventstreamTopology.json` example
  (showing all four new node types plus the new HTTP pagination fields in a live topology response)
  was cross-checked and confirms `eventstream get-topology`'s raw-passthrough rendering already
  displays these fields correctly.
- **`GitConnectionType` (`Full`/`Selective`) read-only field on `GitSyncDetails`**: New enum field
  `gitConnectionType` was added to the Git connection status shape returned by
  `GET /workspaces/{workspaceId}/git/connection`. `fabio git connect show` (`connection_show()` in
  `src/commands/git/connect.rs`) is a raw JSON passthrough (`client.get()` → `render_object()`), so
  the new field surfaces automatically with zero code change — it distinguishes a full-workspace Git
  connection from a selective (folder-scoped) one.
- **`platform/swagger.json` — Git workspace-relations endpoints relocated, not new**: A large
  (~390-line, roughly balanced +/-) diff hunk moves the
  `/workspaces/{workspaceId}/git/workspaceRelations[/{workspaceRelationId}]` endpoint definitions to
  an earlier position in the file (swagger tag changed from `WorkspaceRelations` to `Git`, and
  `operationId` prefixes changed from `WorkspaceRelations_*` to `Git_*`). The actual HTTP
  method/path/request/response schemas are byte-for-byte unchanged — confirmed by finding the same
  unique error codes (`WorkspaceRelationAlreadyExists`, `WorkspaceRelationRootDirectoryMismatch`)
  appear as both a `+` line at the new position and a `-` line at the old position. This is a spec
  file reorganization only; fabio's `git relation {list,create,delete}`
  (`src/commands/git/relation.rs`) already implements these endpoints from a prior sync and needed
  no changes. Swagger `tags`/`operationId` are not consumed by fabio (only path/method/schema
  matter).
- **OneLake shortcuts/data-access-roles: preview-banner removal (GA promotion) for bulk operations**:
  `platform/swagger.json` removed the `> [!NOTE] This API is part of a Preview release...` banner
  from the descriptions of `POST /workspaces/{workspaceId}/items/{itemId}/shortcuts/bulkCreate`
  (bulk shortcut creation), and `GET`/`PUT /workspaces/{workspaceId}/items/{itemId}/dataAccessRoles`
  (list all roles / bulk upsert all roles). This signals these bulk operations have graduated from
  Preview to GA — a documentation-only change with no schema/behavior impact, so no fabio code
  change is required (`fabio lakehouse create-shortcut --bulk`-style bulk creation and
  `fabio onelake-security list`/`upsert` already call these endpoints unconditionally). By contrast,
  the single-role endpoints (`POST .../dataAccessRoles` upsert-one, `GET`/`DELETE
  .../dataAccessRoles/{roleName}`) keep their unchanged "callers must specify `true` for the
  `preview` query parameter" requirement text (that sentence was not touched by this diff — it
  predates this sync and is out of scope here since no diff hunk modified it), so it was not
  altered by this sync.
