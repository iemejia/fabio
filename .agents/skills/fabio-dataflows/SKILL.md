---
name: fabio-dataflows
description: >-
  Intent-scoped fabio skill for Fabric low-code data preparation: Dataflows Gen2 (Power Query / M mashup) and datamarts. Use for visual/low-code ETL that loads into a lakehouse or warehouse. Triggers: "dataflow", "dataflow gen2", "power query", "mashup", "low-code etl", "datamart", "transform without code".
license: MIT
---

# fabio-dataflows — Dataflows — low-code Power Query (Gen2) ETL and datamarts

> **Generated file — do not edit by hand.** This intent-scoped sub-skill of the `fabio` skill is generated from fabio's command schema plus authored judgment. Regenerate with `cargo test generate_subskills -- --ignored`. For install, auth, output envelope, global flags, and agent-safety rules, see the root `fabio` skill.

> **Prefer runtime introspection.** This index is a snapshot; the installed binary is always authoritative. Use `fabio context agent --group <group>` and `fabio context describe <group> <command>` for exact flags and output shapes.

## When to use
- Creating/updating/refreshing Dataflows Gen2 (Power Query mashup ETL).
- Inspecting or querying an existing dataflow definition.
- Managing datamarts.

## When NOT to use (route elsewhere)
- Code-first (PySpark/notebook) transformation -> use fabio-data-engineering.
- Orchestrating activities (copy, notebook, stored proc) -> that's a Data Pipeline (fabio-data-engineering).
- Loading files directly into Delta tables -> use fabio-lakehouse.

## Command index

Generated from fabio's command schema. For full flag details use `fabio context agent --group <group>` or `fabio context describe <group> <command>`.

### fabio dataflow
Manage dataflows (Power BI data transformation)

| Command | Mutates | Description |
|---|---|---|
| `fabio dataflow create` | yes | Create a new dataflow |
| `fabio dataflow delete` | yes | Delete a dataflow |
| `fabio dataflow discover-parameters` | no | Discover parameters of a dataflow |
| `fabio dataflow execute-query` | no | Execute a query against a dataflow (returns Apache Arrow IPC) |
| `fabio dataflow get-definition` | no | Get the definition of a dataflow |
| `fabio dataflow list` | no | List dataflows in a workspace |
| `fabio dataflow run` | yes | Run a dataflow on demand |
| `fabio dataflow show` | no | Show details of a dataflow |
| `fabio dataflow update` | yes | Update dataflow properties (name and/or description) |
| `fabio dataflow update-definition` | yes | Update the definition of a dataflow |

### fabio datamart
Manage datamarts (Power BI)

| Command | Mutates | Description |
|---|---|---|
| `fabio datamart list` | no | List datamarts in a workspace |

## Must / Prefer / Avoid
### MUST
- Treat 'dataflow' as Dataflow Gen2 (the current Power Query item); see 'fabio context disambiguate dataflow'.
- Provide both the mashup (Power Query M) and queryMetadata parts when building a definition.

### PREFER
- Dataflow Gen2 for new low-code ETL; migrate legacy Gen1 rather than authoring new Gen1.
- A Data Pipeline (fabio-data-engineering) when the task is orchestration, not transformation.

### AVOID
- Confusing a Dataflow (transform) with a Data Pipeline (orchestrate) — see disambiguate dataflow.
- Authoring new Gen1 dataflows (Gen2 is the strategic path).

## Key gotchas
- A Dataflow Gen2 definition combines a Power Query mashup with queryMetadata; both are required.
- Refresh is asynchronous — poll or use the run/wait semantics.

## Troubleshooting
| Symptom | Fix |
|---|---|
| Unsure whether to use a dataflow or a pipeline | Transform data -> dataflow; sequence activities -> data-pipeline. Run 'fabio context disambiguate dataflow'. |
| Definition rejected | Ensure both the mashup and queryMetadata parts are present and valid. |
| 'dataflow run' fails with 'Invalid QueriesMetadata ... must not be empty' | A REST-authored Gen2 dataflow needs a queriesMetadata map in queryMetadata.json: add "queriesMetadata": {"<QueryName>": {"queryId", "queryName", "loadEnabled"}}. NOTE the Gen2 lakehouse data destination is portal-generated (a hand-authored dataDestinations binding runs but persists no table). |

## Safety
- Deleting or overwriting a dataflow definition replaces its transformation logic — confirm with the user.

## Shared references
Cross-cutting operational guidance (the "common" layer) — consult the relevant topic before non-trivial work:

| Reference | Covers |
|---|---|
| `fabio context best-practices throttling` | fabio transparently handles 429 (Too Many Requests) and gateway errors. Agents do NOT need to implement retry logic. |
| `fabio context best-practices pagination` | fabio handles pagination via --all (auto-fetch all pages), --continuation-token (resume), and --limit (truncate). Agents rarely need to paginate manually. |
| `fabio context best-practices lro` | Many Fabric operations are async (return 202). fabio polls them automatically. Use --wait for job operations. |

## See also
- fabio context persona data-engineer
- fabio context disambiguate dataflow
- fabio context blueprint basic-data-analytics
- fabio context persona data-solution-architect
