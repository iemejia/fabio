---
title: Choose a BI creation path
description: Select native, generated, empty, or authored models and distinguish report shells from actual PBIR authoring.
---

Start with the intended result, not a required set of definition files:

| You need | Use |
| --- | --- |
| Model over known compatible Delta tables | `semantic-model create --one-lake-table` |
| Deliberately empty model | `semantic-model create` without source/definition inputs |
| Schema-aware generation from a lakehouse or warehouse | `semantic-model generate` |
| Import a complete authored model folder | `semantic-model create --definition` |
| Import one model file | `semantic-model create --file` — never repeated `--file` flags |
| Empty report bound to a model | `report create --semantic-model` |
| Report with pages and visuals | `report scaffold`, then validate and publish its PBIR folder |

Runtime guidance is available without contacting a tenant:

```bash
fabio --wrap-untrusted context best-practices bi-creation
fabio --wrap-untrusted context workflow direct-lake-report
fabio --wrap-untrusted context workflow report-authoring
```

## Native model creation

Preview creation over a real Delta table:

```bash
fabio --wrap-untrusted semantic-model create \
  --workspace "$WS" --name "Sales" \
  --one-lake-table "$ONELAKE_TABLE_URL" --dry-run
```

Repeat `--one-lake-table` to select up to 100 tables. URLs must use the canonical
HTTPS OneLake DFS endpoint and identify actual Delta tables, not SQL views.
Inspect the source schema and permissions first; valid URL syntax does not prove
compatibility or readiness. Cross-workspace sources require access to every
referenced source item.

This native path needs no local definition files, `--connection`, or client-side
SQL/TDS schema discovery. Schema-qualified tables on a schema-enabled lakehouse
are live-validated. A schema-less source can fail with `DirectLakeSchemaError`
when the service assumes `dbo`.

## Generator and authored alternatives

`semantic-model generate --storage-mode onelake` remains useful for schema-aware
definition generation, including schema-less lakehouses. Both generator storage
modes read SQL metadata over TDS and need SQL-audience credentials; see
[authentication](../authentication/). Direct OneLake binding and SQL-endpoint
binding have different security/fallback semantics. Choose explicitly rather than
silently switching after a native-creation failure.

For an existing authored model, preserve its actual expressions and partitions:

```bash
fabio --wrap-untrusted semantic-model create \
  --workspace "$WS" --name "Sales" \
  --definition ./Sales.SemanticModel --dry-run
```

The folder must contain `definition.pbism` and the model body. `--file` accepts
one file only. Do not universally add `--connection`: a complete folder already
carries its source expressions, and empty/native creation rejects that option.

## Creation is not query readiness

Successful creation means the artifact exists. Before designing visuals:

1. Inspect model tables, columns, measures, and relationships.
2. Request framing when needed, after reviewing capacity consumption.
3. `semantic-model refresh` returns `refresh_triggered`, not completion. Poll
   `refresh-status` for the **new** refresh with a bounded interval/deadline. Stop
   on failure; do not keep submitting refreshes or mistake an earlier completed
   refresh for the new request.
4. Verify a bounded DAX read against the actual selected table.

Direct Lake avoids importing a copy of the source data; it does not eliminate
framing/readiness checks. Do not use ownership takeover, data purge, new access
grants, or a changed source binding as automatic recovery steps.

## Report shell versus authored report

For an empty bound report, preview:

```bash
fabio --wrap-untrusted report create \
  --workspace "$WS" --name "Sales report" \
  --semantic-model "$MODEL_ID" --dry-run
```

Add `--semantic-model-workspace` when the model resides in another workspace.
This creates no dashboard visuals. Inspect the persisted `definition.pbir` to
verify the binding and the stored report format before using format-specific
editors.

For actual pages and visuals, use the `report-authoring` workflow: scaffold a PBIR
folder using real model fields, validate it offline, then create from the folder.
Use `--dataset` to rebind an authored folder; native `--semantic-model` is not an
alias for `--dataset` in scaffold or definition import. The existing
`report create --dataset` blank-PBIR-Legacy path remains supported, but is not a
finished report or the preferred starting point for enhanced-PBIR-only editing.
