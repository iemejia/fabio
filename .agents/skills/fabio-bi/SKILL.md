---
name: fabio-bi
description: >-
  Intent-scoped fabio skill for the Fabric BI layer: semantic models (datasets), Power BI reports, paginated reports, and dashboards. Use for creating/refreshing semantic models, running DAX, and managing report items. Triggers: "semantic model", "dataset", "dax query", "refresh dataset", "power bi report", "paginated report", "dashboard", "direct lake".
license: MIT
---

# fabio-bi — Business Intelligence — semantic models, reports, dashboards

> **Generated file — do not edit by hand.** This intent-scoped sub-skill of the `fabio` skill is generated from fabio's command schema plus authored judgment. Regenerate with `cargo test generate_subskills -- --ignored`. For install, auth, output envelope, global flags, and agent-safety rules, see the root `fabio` skill.

> **Prefer runtime introspection.** This index is a snapshot; the installed binary is always authoritative. Use `fabio context agent --group <group>` and `fabio context describe <group> <command>` for exact flags and output shapes.

## When to use
- Optimizing a semantic model / checking it against best practices (semantic-model analyze — Best Practice Analyzer over INFO.VIEW; measure-dependencies for AI data schema scoping).
- Auto-generating a Direct Lake semantic model from a lakehouse/warehouse (semantic-model generate — the portal's 'New semantic model' flow, no hand-authored TMDL).
- Creating/updating semantic models from TMDL and binding them to a SQL endpoint.
- Running DAX queries (EVALUATE) and refreshing models.
- Creating/managing reports, paginated reports, and dashboards bound to a model.
- Building Direct Lake reports over lakehouse Delta tables.

## When NOT to use (route elsewhere)
- Building the underlying lakehouse/warehouse data -> use fabio-lakehouse or fabio-warehouse-sql.
- Real-time KQL dashboards -> use fabio-rti-kql.
- Natural-language Q&A over data (fabio's AI analog) -> use the data-agent group / app-developer persona.

## Command index

Generated from fabio's command schema. For full flag details use `fabio context agent --group <group>` or `fabio context describe <group> <command>`.

### fabio semantic-model
Manage semantic models (Power BI datasets)

| Command | Mutates | Description |
|---|---|---|
| `fabio semantic-model add-measure` | yes | Add a measure to a table by editing the model definition (getDefinition → edit TMDL/model.bim → updateDefinition). Overwrites the definition (irreversible) — dry-run guarded |
| `fabio semantic-model add-relationship` | yes | Add a relationship between two tables by editing the model definition. Overwrites the definition (irreversible) — dry-run guarded |
| `fabio semantic-model add-role` | yes | Add a security role by editing the model definition. Overwrites the definition (irreversible) — dry-run guarded |
| `fabio semantic-model add-user` | yes | Add a user to a semantic model |
| `fabio semantic-model analyze` | yes | Analyze a model against best-practice rules (Best Practice Analyzer / Memory Analyzer over INFO.VIEW metadata) — descriptions, naming, implicit aggregation, duplicate measures, relationship hygiene, star schema, calculated columns, and (opt-in) high cardinality |
| `fabio semantic-model bind-connection` | yes | Bind a semantic model to a connection |
| `fabio semantic-model bind-to-gateway` | yes | Bind a semantic model's data sources to an on-premises/VNet data gateway |
| `fabio semantic-model cancel-refresh` | yes | Cancel an in-progress enhanced refresh by its request id |
| `fabio semantic-model clone` | yes | Clone a semantic model to the same or different workspace |
| `fabio semantic-model create` | yes | Create a new semantic model from a definition file (model.bim) |
| `fabio semantic-model delete` | yes | Delete a semantic model |
| `fabio semantic-model delete-measure` | yes | Delete a measure from the model by editing the definition. Overwrites the definition (irreversible) — dry-run guarded |
| `fabio semantic-model delete-relationship` | yes | Delete a relationship (by --relationship-id or by the from/to columns). Overwrites the definition (irreversible) — dry-run guarded |
| `fabio semantic-model delete-rls` | yes | Remove a row-level-security (RLS) filter from a table for a role. Overwrites the definition (irreversible) — dry-run guarded |
| `fabio semantic-model delete-role` | yes | Delete a security role (and its RLS filters) by editing the model definition. Overwrites the definition (irreversible) — dry-run guarded |
| `fabio semantic-model delete-user` | yes | Remove a user from a semantic model |
| `fabio semantic-model export-pbix` | no | Export a semantic model as a .pbix file |
| `fabio semantic-model generate` | yes | Generate a Direct Lake semantic model from a lakehouse or warehouse (reads the SQL analytics endpoint schema and picks tables, like the Fabric portal's "New semantic model") |
| `fabio semantic-model get-bound-gateway-datasources` | no | List the gateway datasources bound to a semantic model |
| `fabio semantic-model get-definition` | no | Get the definition of a semantic model |
| `fabio semantic-model get-refresh-schedule` | no | Get the scheduled (automatic) refresh configuration |
| `fabio semantic-model import-pbix` | yes | Import a .pbix file as a new semantic model |
| `fabio semantic-model list` | no | List semantic models in a workspace |
| `fabio semantic-model list-columns` | no | List columns of a semantic model (via DAX INFO.VIEW.COLUMNS) |
| `fabio semantic-model list-datasources` | no | List datasources of a semantic model |
| `fabio semantic-model list-measures` | no | List measures of a semantic model (via DAX INFO.VIEW.MEASURES) |
| `fabio semantic-model list-parameters` | no | List parameters of a semantic model |
| `fabio semantic-model list-relationships` | no | List relationships of a semantic model (via DAX INFO.VIEW.RELATIONSHIPS) |
| `fabio semantic-model list-roles` | no | List security roles (RLS) of a semantic model (name, model permission, and per-table filters) — read-only |
| `fabio semantic-model list-tables` | no | List tables of a semantic model (via DAX INFO.VIEW.TABLES — no definition parsing) |
| `fabio semantic-model list-upstream` | no | List upstream (lineage) datasets that this semantic model depends on |
| `fabio semantic-model list-users` | no | List users (permissions) of a semantic model |
| `fabio semantic-model measure-dependencies` | no | List each measure's dependencies (the measures/columns/tables its DAX references) — useful for including dependent objects in an AI data schema |
| `fabio semantic-model move-measure` | yes | Move a measure to a different home table (name and definition preserved). Overwrites the definition (irreversible) — dry-run guarded |
| `fabio semantic-model query` | no | Execute a DAX query against a semantic model |
| `fabio semantic-model refresh` | yes | Refresh a semantic model (required to frame Direct Lake models after creation) |
| `fabio semantic-model refresh-details` | no | Get execution details of a specific (enhanced) refresh by its request id |
| `fabio semantic-model refresh-status` | no | Get refresh history and status for a semantic model |
| `fabio semantic-model rename-measure` | yes | Rename a measure (its declaration only; DAX references are NOT rewritten). Overwrites the definition (irreversible) — dry-run guarded |
| `fabio semantic-model set-description` | yes | Set the description of a table, column, or measure by editing the model definition (getDefinition → edit TMDL/model.bim → updateDefinition). Overwrites the definition (irreversible) — dry-run guarded |
| `fabio semantic-model set-rls` | yes | Set a row-level-security (RLS) filter on a table for a role (a DAX predicate). Overwrites the definition (irreversible) — dry-run guarded |
| `fabio semantic-model show` | no | Show details of a semantic model |
| `fabio semantic-model takeover` | yes | Take over a semantic model (converts definition-managed to service-managed for portal editing) |
| `fabio semantic-model unbind-connection` | yes | Unbind a connection from a semantic model |
| `fabio semantic-model update` | yes | Update semantic model properties (name and/or description) |
| `fabio semantic-model update-datasources` | yes | Update datasources of a semantic model |
| `fabio semantic-model update-definition` | yes | Update the definition of a semantic model from a file |
| `fabio semantic-model update-measure` | yes | Update an existing measure's expression and/or properties by editing the model definition (getDefinition → edit TMDL/model.bim → updateDefinition). Overwrites the definition (irreversible) — dry-run guarded |
| `fabio semantic-model update-parameters` | yes | Update parameters of a semantic model |
| `fabio semantic-model update-refresh-schedule` | yes | Update the scheduled (automatic) refresh configuration |
| `fabio semantic-model update-relationship` | yes | Update a relationship's active state and/or cross-filter direction (by --relationship-id or by the from/to columns). Overwrites the definition (irreversible) — dry-run guarded |

### fabio report
Manage reports (Power BI)

| Command | Mutates | Description |
|---|---|---|
| `fabio report create` | yes | Create a new report from a definition file |
| `fabio report delete` | yes | Delete a report |
| `fabio report export` | no | Export (render) the Power BI report to a file (PDF, PPTX, PNG) |
| `fabio report get-definition` | no | Get the definition of a report |
| `fabio report list` | no | List reports in a workspace |
| `fabio report publish-to-web` | yes | Publish a report to the web (generates a publicly accessible embed URL) |
| `fabio report show` | no | Show details of a report |
| `fabio report update` | yes | Update report properties (name and/or description) |
| `fabio report update-definition` | yes | Update the definition of a report |
| `fabio report validate` | no | Validate a Power BI report definition on disk (PBIR or PBIR-Legacy) |

### fabio paginated-report
Manage paginated reports

| Command | Mutates | Description |
|---|---|---|
| `fabio paginated-report create` | yes | Create a paginated report in the specified workspace (requires an RDL definition file) |
| `fabio paginated-report delete` | yes | Delete a paginated report |
| `fabio paginated-report export` | no | Export (render) the paginated report to a file (PDF, PPTX, XLSX, DOCX, CSV, IMAGE, ...) |
| `fabio paginated-report get-definition` | no | Get the public definition of a paginated report (returns the .rdl file encoded in base64) |
| `fabio paginated-report list` | no | List paginated reports in a workspace |
| `fabio paginated-report show` | no | Show details of a paginated report |
| `fabio paginated-report update` | yes | Update paginated report properties (name and/or description) |
| `fabio paginated-report update-definition` | yes | Update the definition of a paginated report |

### fabio dashboard
Manage dashboards (Power BI)

| Command | Mutates | Description |
|---|---|---|
| `fabio dashboard list` | no | List dashboards in a workspace |

## Must / Prefer / Avoid
### MUST
- Treat 'dataset' as a semantic model (use the semantic-model group, NOT report); see 'fabio context disambiguate semantic-model'.
- Bind semantic-model create to a valid --connection (SQL endpoint) for import/DirectQuery.
- Validate a PBIR/PBIP report folder offline with 'report validate --source <folder>' before 'report create --definition' or deploy — it catches missing required files, bad $schema, and byPath-vs-byConnection issues without a tenant call.

### PREFER
- semantic-model analyze (--with-cardinality, --severity, --strict) to check a model against best practices before shipping or using it as a data-agent source — it flags missing descriptions, cryptic names, implicit aggregation on identifier columns, duplicate measures, ambiguous dates, relationship hygiene, non-star schemas, calculated + high-cardinality columns. Add --fix to auto-apply the ONE safe mechanical fix (set default summarization to None on identifier columns) via a dry-run-guarded definition overwrite; naming/dedup/schema/description issues are NOT auto-fixed (they need human judgment). Use semantic-model measure-dependencies to include every dependent measure/column when scoping an AI data schema. See 'fabio context best-practices semantic-model-optimization'.
- semantic-model generate --lakehouse <id> (or --warehouse) to auto-build a Direct Lake model from a data source WITHOUT hand-authoring TMDL — it reads the SQL analytics endpoint schema, maps types (dropping unmappable columns like Fabric does), synthesizes the portal-EXACT TMDL definition, creates it, and frames it. Use --tables to pick specific tables and --schema (default dbo). Relationships/measures are NOT generated (same as the portal) — add them with update-definition. This is the fast path; use create --file/--definition only when you already have authored TMDL.
- Introspect a model's schema with semantic-model list-tables/list-columns/list-measures/list-relationships (DAX INFO.VIEW.* — the Analysis Services Schema Rowsets) to understand tables, types, StorageMode (Direct Lake), measures, and relationships WITHOUT fetching/parsing the TMDL/TMSL definition.
- report create --definition <folder> to create a FULL multi-page PBIR report from generated files (the documented, agent-authorable format); it gathers definition.pbir + report.json or definition/** and validates first. Use --dataset with it to rebind a byPath folder to a concrete model by connection.
- Direct Lake over import mode when data already lives in a lakehouse (no refresh cost).
- fabio rest call --api powerbi for Power BI-specific endpoints not on the Fabric surface.
- semantic-model query --dax for validation before wiring a report.

### AVOID
- Inventing a 'fabio dataset' command — datasets are semantic models.
- Feeding a byPath definition.pbir to 'report create' without --dataset (byPath needs a co-deployed model; 'deploy' rewrites byPath→byConnection automatically, but 'report create' needs byConnection — pass --dataset to bind by id).
- Refreshing on inactive capacity (CAPACITY_INACTIVE).

## Key gotchas
- 'dataset' (legacy Power BI term) == semantic model; Power BI REST still uses /datasets (reach it via rest call --api powerbi).
- Schema introspection uses DAX INFO.VIEW.* (list-tables/columns/measures/relationships) over the same executeQueries endpoint as query --dax; the raw INFO.TABLES()/INFO.COLUMNS() variants are rejected by executeQueries (HTTP 400) — only INFO.VIEW.* works.
- PBIR (the enhanced per-file 'definition/' folder) is Microsoft's documented, agent-authorable report format (each page/visual has its own $schema-bearing JSON) and becomes the only format at GA; conform to the published visual.json/page.json schemas and run 'report validate' before create/deploy. fabio 'report create --definition' pushes a full PBIR tree (previously only 'deploy' could).
- Direct Lake reads Delta directly — the report is empty until the lakehouse tables are populated.
- semantic-model generate reads the source schema over the SQL analytics endpoint (TDS), so it needs a SQL-scoped token from the ambient credential chain (az login / device-code cache) — do NOT set a Fabric-only static FABIO_ACCESS_TOKEN for it. It generates the portal-EXACT Direct Lake TMDL (definition.pbism v4.2, model.tmdl defaultMode directLake, database.tmdl compatibilityLevel 1604, per-table tables/*.tmdl, expressions.tmdl Sql.Database(server, <sqlEndpointId>)) and frames it with a Full refresh; wait ~15-30s before the first DAX query. A freshly loaded lakehouse table can lag ~30-60s before it appears on the SQL endpoint. The Sql.Database catalog is the SQL analytics endpoint item id (a GUID), matching what the portal emits.
- To edit individual model objects fabio uses definition read-modify-write (getDefinition->edit TMDL->updateDefinition), NOT XMLA/TOM: set-description; measures add/update/delete/rename/move (rename does NOT rewrite DAX references; move changes the home table); relationships add/delete/update (in definition/relationships.tmdl, match by --relationship-id or the full from/to column tuple); security roles + RLS via add-role/delete-role/set-rls/delete-rls/list-roles (RLS filters are a DAX predicate per table; distinct from add-user which grants dataset permissions to a principal). These OVERWRITE the definition (irreversible, dry-run guarded); a measure lands after the table scalar props (canonical measures-first) so it does not break TMDL indentation.

## Troubleshooting
| Symptom | Fix |
|---|---|
| semantic-model create fails to bind | Pass --connection with a valid SQL analytics endpoint; the model needs a data source. |
| report create --definition rejected / opaque API error | Run 'fabio report validate --source <folder>' first; conform each PBIR file to its published $schema and ensure required files (definition.pbir, definition/{report,version}.json, pages/**/page.json) are present. |
| Report shows no data | For Direct Lake, populate the lakehouse tables first; for import, refresh the model. For a hand-authored PBIR folder, validate it and conform visual.json to the published visual container schema. |
| Refresh fails with CAPACITY_INACTIVE | Resume the capacity (fabio capacity resume) before refreshing. |
| No 'fabio dataset' command | Use the semantic-model group; 'dataset' is the legacy name for a semantic model. |

## Safety
- Refreshing a large model consumes capacity — confirm headroom before a full refresh.
- Overwriting a semantic model definition replaces its measures/relationships — confirm with the user.
- semantic-model analyze --fix overwrites the model definition (irreversible) to apply the safe summarization fix — it is dry-run guarded; preview with --dry-run first. It only changes default summarization on identifier columns, never renames or restructures.

## Shared references
Cross-cutting operational guidance (the "common" layer) — consult the relevant topic before non-trivial work:

| Reference | Covers |
|---|---|
| `fabio context best-practices throttling` | fabio transparently handles 429 (Too Many Requests) and gateway errors. Agents do NOT need to implement retry logic. |
| `fabio context best-practices pagination` | fabio handles pagination via --all (auto-fetch all pages), --continuation-token (resume), and --limit (truncate). Agents rarely need to paginate manually. |
| `fabio context best-practices lro` | Many Fabric operations are async (return 202). fabio polls them automatically. Use --wait for job operations. |
| `fabio context best-practices semantic-model-optimization` | How to optimize a Power BI semantic model for performance, correctness, and use as an AI/data-agent source: run the Best Practice Analyzer, fix descriptions/naming/aggregation/relationships, understand measure dependencies, and know which AI-prep steps are portal-only. Based on Microsoft's 'Semantic model best practices for data agent' guidance. |

## See also
- fabio context persona bi-developer
- fabio context workflow direct-lake-report
- fabio context workflow semantic-model-ai-readiness
- fabio context disambiguate semantic-model
