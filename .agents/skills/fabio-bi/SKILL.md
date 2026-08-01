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
| `fabio semantic-model add-user` | yes | Add a user to a semantic model |
| `fabio semantic-model bind-connection` | yes | Bind a semantic model to a connection |
| `fabio semantic-model clone` | yes | Clone a semantic model to the same or different workspace |
| `fabio semantic-model create` | yes | Create a new semantic model from a definition file (model.bim) |
| `fabio semantic-model delete` | yes | Delete a semantic model |
| `fabio semantic-model delete-user` | yes | Remove a user from a semantic model |
| `fabio semantic-model export-pbix` | no | Export a semantic model as a .pbix file |
| `fabio semantic-model get-definition` | no | Get the definition of a semantic model |
| `fabio semantic-model import-pbix` | yes | Import a .pbix file as a new semantic model |
| `fabio semantic-model list` | no | List semantic models in a workspace |
| `fabio semantic-model list-datasources` | no | List datasources of a semantic model |
| `fabio semantic-model list-parameters` | no | List parameters of a semantic model |
| `fabio semantic-model list-upstream` | no | List upstream (lineage) datasets that this semantic model depends on |
| `fabio semantic-model list-users` | no | List users (permissions) of a semantic model |
| `fabio semantic-model query` | no | Execute a DAX query against a semantic model |
| `fabio semantic-model refresh` | yes | Refresh a semantic model (required to frame Direct Lake models after creation) |
| `fabio semantic-model refresh-status` | no | Get refresh history and status for a semantic model |
| `fabio semantic-model show` | no | Show details of a semantic model |
| `fabio semantic-model takeover` | yes | Take over a semantic model (converts definition-managed to service-managed for portal editing) |
| `fabio semantic-model unbind-connection` | yes | Unbind a connection from a semantic model |
| `fabio semantic-model update` | yes | Update semantic model properties (name and/or description) |
| `fabio semantic-model update-datasources` | yes | Update datasources of a semantic model |
| `fabio semantic-model update-definition` | yes | Update the definition of a semantic model from a file |
| `fabio semantic-model update-parameters` | yes | Update parameters of a semantic model |

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
- Use PBIR-Legacy format when a report must render data programmatically (plain PBIR cannot).

### PREFER
- Direct Lake over import mode when data already lives in a lakehouse (no refresh cost).
- fabio rest call --api powerbi for Power BI-specific endpoints not on the Fabric surface.
- semantic-model query --dax for validation before wiring a report.

### AVOID
- Inventing a 'fabio dataset' command — datasets are semantic models.
- Expecting plain PBIR reports to render data (they need PBIR-Legacy).
- Refreshing on inactive capacity (CAPACITY_INACTIVE).

## Key gotchas
- 'dataset' (legacy Power BI term) == semantic model; Power BI REST still uses /datasets (reach it via rest call --api powerbi).
- Report visuals need PBIR-Legacy to render data; plain PBIR cannot render programmatically.
- Direct Lake reads Delta directly — the report is empty until the lakehouse tables are populated.

## Troubleshooting
| Symptom | Fix |
|---|---|
| semantic-model create fails to bind | Pass --connection with a valid SQL analytics endpoint; the model needs a data source. |
| Report shows no data | For Direct Lake, populate the lakehouse tables first; for import, refresh the model; ensure PBIR-Legacy if rendering programmatically. |
| Refresh fails with CAPACITY_INACTIVE | Resume the capacity (fabio capacity resume) before refreshing. |
| No 'fabio dataset' command | Use the semantic-model group; 'dataset' is the legacy name for a semantic model. |

## Safety
- Refreshing a large model consumes capacity — confirm headroom before a full refresh.
- Overwriting a semantic model definition replaces its measures/relationships — confirm with the user.

## Shared references
Cross-cutting operational guidance (the "common" layer) — consult the relevant topic before non-trivial work:

| Reference | Covers |
|---|---|
| `fabio context best-practices throttling` | fabio transparently handles 429 (Too Many Requests) and gateway errors. Agents do NOT need to implement retry logic. |
| `fabio context best-practices pagination` | fabio handles pagination via --all (auto-fetch all pages), --continuation-token (resume), and --limit (truncate). Agents rarely need to paginate manually. |
| `fabio context best-practices lro` | Many Fabric operations are async (return 202). fabio polls them automatically. Use --wait for job operations. |

## See also
- fabio context persona bi-developer
- fabio context workflow direct-lake-report
- fabio context disambiguate semantic-model
