---
name: fabio-ontology
description: >-
  Intent-scoped fabio skill for Fabric knowledge/graph and digital-twin modeling: ontology items (entity/relationship types and bindings), graph models, graph querysets, and Digital Twin Builder models/flows. Use to define/evolve ontologies, query graphs for agent grounding, and build operational digital twins. fabio can also export a tenant scan as OWL (context tenant --format owl) and import it. Triggers: "ontology", "fabric iq ontology", "knowledge graph", "graph model", "graph query", "entity type", "relationship type", "digital twin", "digital twin builder", "owl".
license: MIT
---

# fabio-ontology — Ontology, Graph & Digital Twins — Fabric IQ ontologies, graph models, digital twin builder

> **Generated file — do not edit by hand.** This intent-scoped sub-skill of the `fabio` skill is generated from fabio's command schema plus authored judgment. Regenerate with `cargo test generate_subskills -- --ignored`. For install, auth, output envelope, global flags, and agent-safety rules, see the root `fabio` skill.

> **Prefer runtime introspection.** This index is a snapshot; the installed binary is always authoritative. Use `fabio context agent --group <group>` and `fabio context describe <group> <command>` for exact flags and output shapes.

## When to use
- Creating/evolving an ontology item (entity types, relationship types, data bindings).
- Generating an ontology FROM a Power BI semantic model OR a lakehouse ('ontology generate') — client-side reproduction of the portal's 'Generate Ontology' (which has no REST API): with --semantic-model it reads the model's tables/columns/relationships; with --lakehouse (no --semantic-model) it reads the lakehouse SQL endpoint's INFORMATION_SCHEMA. Either way it synthesizes entity types + typed properties + (relationships, model source only) + keys + lakehouse bindings.
- Binding an entity to one or MULTIPLE data sources — Lakehouse Delta (--lakehouse) and/or Eventhouse KustoTable — with generated entity Documents + ResourceLinks and entity-type inheritance carried from the imported schema.
- Managing graph models and running graph querysets.
- Modeling IoT/operational digital twins (Digital Twin Builder models and flows).
- Grounding an agent in a knowledge graph over Fabric data.
- Exposing an ontology to external AI systems as an MCP server ('ontology mcp-url').
- Querying an ontology's data in natural language ('ontology search') — fabio consumes the ontology MCP server's search_ontology tool as an MCP client.
- Importing an OWL schema (e.g. one produced by 'fabio context tenant --format owl').

## When NOT to use (route elsewhere)
- Relational T-SQL modeling -> use fabio-warehouse-sql.
- The Delta/lakehouse data the ontology binds to -> use fabio-lakehouse.
- Semantic (tabular) models for BI -> use fabio-bi.

## Command index

Generated from fabio's command schema. For full flag details use `fabio context agent --group <group>` or `fabio context describe <group> <command>`.

### fabio ontology
Manage ontologies (entity types, data bindings)

| Command | Mutates | Description |
|---|---|---|
| `fabio ontology bind` | yes | Bind an existing ontology's types to data sources (no OWL re-import) |
| `fabio ontology create` | yes | Create an ontology |
| `fabio ontology delete` | yes | Delete an ontology |
| `fabio ontology export` | no | Export a Fabric Ontology to OWL format (RDF/XML or JSON-LD) |
| `fabio ontology generate` | yes | Generate an ontology from a semantic model or lakehouse (entity types, properties, relationships) |
| `fabio ontology get-definition` | no | Get the ontology definition (entity types, bindings) |
| `fabio ontology import` | yes | Import an OWL ontology (RDF/XML or JSON-LD) and convert to Fabric format |
| `fabio ontology list` | no | List ontologies in a workspace |
| `fabio ontology list-entity-types` | no | List the ontology's entity types and their properties (schema exploration) |
| `fabio ontology mcp-url` | no | Print the Model Context Protocol (MCP) server URL for consuming this ontology |
| `fabio ontology search` | no | Ask a natural-language question over the ontology's data (MCP `search_ontology` tool) |
| `fabio ontology show` | no | Show details of an ontology |
| `fabio ontology update` | yes | Update ontology properties (name and/or description) |
| `fabio ontology update-definition` | yes | Update the ontology definition (replaces current definition) |

### fabio graph-model
Manage graph models (knowledge graph)

| Command | Mutates | Description |
|---|---|---|
| `fabio graph-model create` | yes | Create a new graph model |
| `fabio graph-model delete` | yes | Delete a graph model |
| `fabio graph-model execute-query` | no | Execute a GQL query against the graph |
| `fabio graph-model get-definition` | no | Get the definition of a graph model |
| `fabio graph-model get-queryable-graph-type` | no | Get the queryable graph type |
| `fabio graph-model initialize` | yes | Initialize a graph model for querying (portal-only operation) |
| `fabio graph-model list` | no | List graph models in a workspace |
| `fabio graph-model refresh-graph` | yes | Trigger a graph refresh job |
| `fabio graph-model show` | no | Show details of a graph model |
| `fabio graph-model update` | yes | Update graph model properties (name and/or description) |
| `fabio graph-model update-definition` | yes | Update the definition of a graph model |

### fabio graph-query-set
Manage graph query sets

| Command | Mutates | Description |
|---|---|---|
| `fabio graph-query-set create` | yes | Create a new graph query set |
| `fabio graph-query-set delete` | yes | Delete a graph query set |
| `fabio graph-query-set get-definition` | no | Get the definition of a graph query set |
| `fabio graph-query-set list` | no | List graph query sets in a workspace |
| `fabio graph-query-set show` | no | Show details of a graph query set |
| `fabio graph-query-set update` | yes | Update graph query set properties |
| `fabio graph-query-set update-definition` | yes | Update the definition of a graph query set |

### fabio digital-twin-builder
Manage Digital Twin Builder models

| Command | Mutates | Description |
|---|---|---|
| `fabio digital-twin-builder create` | yes | Create a new Digital Twin Builder |
| `fabio digital-twin-builder delete` | yes | Delete a Digital Twin Builder |
| `fabio digital-twin-builder get-definition` | no | Get the definition of a Digital Twin Builder |
| `fabio digital-twin-builder list` | no | List Digital Twin Builders in a workspace |
| `fabio digital-twin-builder query` | no | Run a T-SQL query against the twin's data (the associated `dtdm` lakehouse SQL endpoint). Query the `dom` domain views (recommended) or `dbo` base tables |
| `fabio digital-twin-builder show` | no | Show details of a Digital Twin Builder |
| `fabio digital-twin-builder show-lakehouse` | no | Resolve the associated data lakehouse (the `<name>dtdm` lakehouse where the twin's ontology/instance data lives) and its SQL analytics endpoint |
| `fabio digital-twin-builder update` | yes | Update Digital Twin Builder properties |
| `fabio digital-twin-builder update-definition` | yes | Update the definition of a Digital Twin Builder |

### fabio digital-twin-builder-flow
Manage Digital Twin Builder flows

| Command | Mutates | Description |
|---|---|---|
| `fabio digital-twin-builder-flow create` | yes | Create a new Digital Twin Builder flow |
| `fabio digital-twin-builder-flow delete` | yes | Delete a Digital Twin Builder flow |
| `fabio digital-twin-builder-flow get-definition` | no | Get the definition of a Digital Twin Builder flow |
| `fabio digital-twin-builder-flow list` | no | List Digital Twin Builder flows in a workspace |
| `fabio digital-twin-builder-flow show` | no | Show details of a Digital Twin Builder flow |
| `fabio digital-twin-builder-flow update` | yes | Update Digital Twin Builder flow properties |
| `fabio digital-twin-builder-flow update-definition` | yes | Update the definition of a Digital Twin Builder flow |

## Must / Prefer / Avoid
### MUST
- Define entity/relationship types before adding data bindings.
- Use the item-definition format for ontology create/update (see 'fabio context schema ontology').

### PREFER
- ontology generate --semantic-model <ID> --lakehouse <LH> to bootstrap an ontology from an existing semantic model (entity types + properties + relationships + bindings in one shot), OR ontology generate --lakehouse <LH> (no --semantic-model) to bootstrap directly from lakehouse tables (entity types + typed properties + first-column-key heuristic + bindings; no relationships); inspect the synthesized OWL first with --output-owl.
- context tenant --format owl to bootstrap an ontology schema from a real workspace scan, then ontology import.
- ontology list-entity-types to explore an ontology's schema (entity types, properties, timeseries/untyped, inheritance) WITHOUT parsing the raw definition — byte-for-byte the same answer as the ontology MCP server's list_ontology_entity_types tool (minus the server-only etag), computed offline from getDefinition.
- ontology import --lakehouse <ID> --bindings <map.json> to generate DataBindings + relationship Contextualizations in the same step, so the imported ontology is queryable rather than a bare schema.
- ontology bind --lakehouse <ID> --bindings <map.json> to add/update data bindings on an EXISTING ontology (e.g. portal-authored) without re-importing OWL; types are matched by name.
- Runtime introspection (context agent --group ontology|graph-model) for exact flags.

### AVOID
- Binding to data sources that do not yet exist — create the underlying items first.
- Confusing an ontology (knowledge graph schema) with a semantic model (BI tabular model).

## Key gotchas
- Ontology definitions use the item-definition (base64 parts) format; fetch the template with 'fabio context schema ontology'.
- fabio's context tenant graph can emit OWL/RDF that imports directly via 'fabio ontology import --file'.
- OWL carries no data-binding info (workspace/lakehouse/table/column). 'ontology import' generates only the type schema unless you pass a source: --lakehouse (Delta) or Eventhouse/KustoTable source flags (+ --bindings for relationship key columns). An entity can carry MULTIPLE data bindings, and import also emits entity Documents + ResourceLinks and preserves entity-type inheritance.
- Untyped properties (valueType Any) are NOT bindable — they live only under entityType.untypedProperties. Putting one in a DataBinding makes updateDefinition fail with a generic ALMOperationImportFailed. fabio import excludes them automatically; hand-authored parts must too.
- Time-series entities need a timestampColumn on their TimeSeries binding. Convention bind-all (import/bind with no per-entity timestamp) errors: 'Entity X has a TimeSeries binding but no timestampColumn'. Supply it via the --entities/bind config or use non-time-series entities.
- updateDefinition validates all parts together; a bad reference in ANY part fails the whole push. The generic ALMOperationImportFailed's real cause is in error.errorCode + error.moreDetails (fabio surfaces both and adds a self-correction checklist).
- Fabric does NOT check that a bound Lakehouse table/column exists at updateDefinition time (deferred to query time) — a missing table imports fine and is never the cause of an import failure.
- Ontology needs a capacity with the Ontology/Digital Twin Builder preview enabled; each create/import/getDefinition is an LRO taking ~60-100s.
- An ontology can be consumed as an MCP server by external agents. 'fabio ontology mcp-url --workspace <WS> --id <ID>' prints the canonical endpoint ({fabricBase}/mcp/dataPlane/workspaces/{ws}/items/{id}/ontologyEndpoint) — a deterministic URL agents cannot guess. Distinct from grounding a fabio data-agent on the ontology; this exposes the ontology itself over MCP (HTTP transport, Fabric auth) to VS Code agent mode/Claude/Copilot Studio. Requires F2+/P1 capacity and the Ontology-item preview tenant setting.
- The ontology MCP server exposes two tools: list_ontology_entity_types (schema) and search_ontology (natural-language query over the ontology data estate). Both now have pure-fabio equivalents: 'fabio ontology list-entity-types' reproduces the first EXACTLY (offline, byte-for-byte), and 'fabio ontology search --prompt "..."' drives the second by consuming the ontology MCP server as an MCP CLIENT (fabio's first MCP-client feature). search returns raw JSON results + an optional derived NL answer; a successful answer needs the ontology bound to data AND server-side Fabric IQ NL reasoning provisioned on the capacity.
- Digital Twin Builder (DTB) modeling — entity types, data mapping, contextualization (relationships), and the Explorer — is PORTAL-ONLY (no public REST API). fabio's REST surface for `digital-twin-builder` / `digital-twin-builder-flow` is item CRUD + get/update-definition only; the definition.json (`{"LakehouseId"}` for a DTB, `{"DigitalTwinBuilderId","OperationIds","IsOnDemand"}` for a flow) is authored by the portal.
- DTB DATA is queryable though: each DTB auto-provisions a '<name>dtdm' lakehouse (LakehouseId in its definition). Query the twin's instances with `fabio digital-twin-builder query --id <DTB> --sql "SELECT * FROM dom.<View>"` (fabio resolves the dtdm lakehouse); the `dom` schema holds the domain views (recommended), `dbo` holds base-layer tables. `fabio digital-twin-builder show-lakehouse --id <DTB>` returns the linked lakehouse + SQL endpoint.
- Deleting a DTB does NOT delete its '<name>dtdm' data lakehouse — use `digital-twin-builder delete --id <DTB> --delete-lakehouse` to cascade, or delete the lakehouse manually. DTB/flow item names allow only letters/numbers/underscores (NO hyphens).

## Troubleshooting
| Symptom | Fix |
|---|---|
| ALMOperationImportFailed / generic 'import failed' on import/bind/update-definition | The top-level message is often an unfilled '{0} {1} {2}' template — read error.errorCode + error.moreDetails (fabio flattens these into the message + hint). Check in order: (1) no untyped property is bound, (2) every entityTypeId/propertyId/relationshipTypeId referenced by a binding or contextualization is defined in the same push and case-matches, (3) TimeSeries bindings have a timestampColumn, (4) contextualization source/target entity ids match the relationship endpoints. A missing Lakehouse table is NOT a cause. |
| 'Entity X has a TimeSeries binding but no timestampColumn' | Provide a timestampColumn for that entity (import --entities map / bind config), or model it as a non-time-series entity. |
| Ontology import rejected before push | Validate the OWL/JSON-LD against the ontology schema (context schema ontology); ensure entity types precede bindings. |
| Ontology query returns no data although import succeeded | updateDefinition does not validate table/column existence — verify the bound Lakehouse table and columns actually exist and the binding names match (import success != queryable). |

## Safety
- Overwriting an ontology definition replaces its type system and bindings — confirm with the user.

## Shared references
Cross-cutting operational guidance (the "common" layer) — consult the relevant topic before non-trivial work:

| Reference | Covers |
|---|---|
| `fabio context best-practices throttling` | fabio transparently handles 429 (Too Many Requests) and gateway errors. Agents do NOT need to implement retry logic. |
| `fabio context best-practices pagination` | fabio handles pagination via --all (auto-fetch all pages), --continuation-token (resume), and --limit (truncate). Agents rarely need to paginate manually. |
| `fabio context best-practices lro` | Many Fabric operations are async (return 202). fabio polls them automatically. Use --wait for job operations. |

## See also
- fabio context schema ontology
- fabio context workflow ontology_tutorial
- fabio context persona data-engineer
- fabio ontology mcp-url --workspace <WS> --id <ID> (consume the ontology as an MCP server)
- fabio data-agent add-datasource --artifact-type Ontology (ground an agent on the ontology; scope with select-tables --elements)
