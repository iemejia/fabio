---
name: fabio-geospatial
description: >-
  Intent-scoped fabio skill for Fabric geospatial maps: create, inspect, and update map items and their definitions. Use to manage location/geospatial visualizations backed by Fabric data. Triggers: "map", "geospatial", "create map", "location visualization", "spatial data", "map definition".
license: MIT
---

# fabio-geospatial — Geospatial — Fabric maps

> **Generated file — do not edit by hand.** This intent-scoped sub-skill of the `fabio` skill is generated from fabio's command schema plus authored judgment. Regenerate with `cargo test generate_subskills -- --ignored`. For install, auth, output envelope, global flags, and agent-safety rules, see the root `fabio` skill.

> **Prefer runtime introspection.** This index is a snapshot; the installed binary is always authoritative. Use `fabio context agent --group <group>` and `fabio context describe <group> <command>` for exact flags and output shapes.

## When to use
- Creating/listing/inspecting map items in a workspace.
- Editing a map's definition (get-definition / update-definition).
- Managing geospatial visualizations over Fabric data.

## When NOT to use (route elsewhere)
- The underlying spatial data (tables/files) -> use fabio-lakehouse or fabio-warehouse-sql.
- Non-spatial reports and dashboards -> use fabio-bi.
- Real-time location streams -> use fabio-rti-kql.

## Command index

Generated from fabio's command schema. For full flag details use `fabio context agent --group <group>` or `fabio context describe <group> <command>`.

### fabio map
Manage maps (geospatial)

| Command | Mutates | Description |
|---|---|---|
| `fabio map create` | yes | Create a new map |
| `fabio map delete` | yes | Delete a map |
| `fabio map get-definition` | no | Get the definition of a map |
| `fabio map list` | no | List maps in a workspace |
| `fabio map show` | no | Show details of a map |
| `fabio map update` | yes | Update map properties |
| `fabio map update-definition` | yes | Update the definition of a map |

## Must / Prefer / Avoid
### MUST
- Edit map content via the definition part (get-definition / update-definition), not plain metadata flags.
- Ensure the spatial data source the map reads from exists before wiring the map.

### PREFER
- Runtime introspection (context agent --group map, context describe map create) for exact flags and the definition shape.
- Building the spatial dataset first (lakehouse/warehouse), then the map on top.

### AVOID
- Expecting a metadata-only update to change map content — use update-definition.
- Confusing a map (geospatial visualization) with a graph model or an ontology.

## Key gotchas
- Maps carry their configuration in a definition part (base64); create/update take a --content/definition, and get-definition returns it for round-tripping.
- A map is a visualization layer — it renders data that lives in other Fabric items, which must exist and be populated.

## Troubleshooting
| Symptom | Fix |
|---|---|
| Map changes don't persist | Update the definition (map update-definition), not just metadata; metadata-only updates don't change map content. |
| Map renders empty | Confirm the backing spatial dataset exists and is populated; the map only visualizes other items' data. |

## Safety
- Deleting a map removes its definition and configuration — confirm with the user.

## Shared references
Cross-cutting operational guidance (the "common" layer) — consult the relevant topic before non-trivial work:

| Reference | Covers |
|---|---|
| `fabio context best-practices throttling` | fabio transparently handles 429 (Too Many Requests) and gateway errors. Agents do NOT need to implement retry logic. |
| `fabio context best-practices pagination` | fabio handles pagination via --all (auto-fetch all pages), --continuation-token (resume), and --limit (truncate). Agents rarely need to paginate manually. |
| `fabio context best-practices lro` | Many Fabric operations are async (return 202). fabio polls them automatically. Use --wait for job operations. |
| `fabio context best-practices workspace-oap` | Outbound Access Protection is a workspace 'default deny' model for outbound connections from workspace items. It now (preview) governs Operations Agent actions and Fabric Maps data sources (Lakehouse, KQL, Ontology, and external WMS/WMTS/WFS geospatial services). Configure allow-lists with fabio workspace set-outbound-* rules + connection create; audit tenant-wide with admin list-network-policies. |

## See also
- fabio context persona bi-developer
- fabio context persona data-engineer
