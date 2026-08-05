# fabio v0.58.0

## What's New

This release makes fabio a **complete Power BI / Fabric semantic-model authoring tool** — you can now create and edit every kind of model object (measures, columns, tables, relationships, RLS roles, hierarchies, partitions, calculation groups, translations, Power Query parameters, DAX functions, and perspectives) directly from the CLI. It reaches feature parity with Microsoft's [`powerbi-modeling-mcp`](https://github.com/microsoft/powerbi-modeling-mcp) server, but **without XMLA/TOM**: every edit is a definition read-modify-write (`getDefinition` → edit the TMDL → `updateDefinition`), so it works over the plain Fabric REST API. The `semantic-model` group grew from **37 to 89 subcommands** (52 new), each with unit tests and a live end-to-end lifecycle test.

### New Commands — semantic-model granular authoring

All are `--dry-run`-guarded, mutate the model definition, and support both TMDL and `model.bim` models.

**Measures**
- `set-description`, `add-measure`, `update-measure`, `delete-measure`, `rename-measure`, `move-measure`

**Columns**
- `add-calculated-column`, `update-column`, `rename-column`, `delete-column`

**Tables**
- `add-table` (calculated), `rename-table`, `update-table` (hidden / data-category / description), `delete-table` (cascades relationships + RLS filters)

**Relationships**
- `add-relationship`, `update-relationship`, `delete-relationship`, (read via `list-relationships`)

**Row-level security**
- `add-role`, `delete-role`, `set-rls`, `delete-rls`, `list-roles`

**Hierarchies**
- `add-hierarchy`, `delete-hierarchy`, `list-hierarchies`

**Partitions**
- `add-partition`, `update-partition`, `delete-partition`, `list-partitions`

**Calculation groups**
- `add-calculation-group`, `delete-calculation-group`, `add-calculation-item`, `update-calculation-item`, `delete-calculation-item`, `list-calculation-groups`

**Named expressions / Power Query parameters**
- `add-expression`, `update-expression`, `delete-expression`, `list-expressions`

**DAX user-defined functions (UDFs)**
- `add-function`, `update-function`, `delete-function`, `list-functions`

**Translations / cultures**
- `add-culture`, `delete-culture`, `set-translation`, `list-cultures`

**Perspectives**
- `add-perspective`, `delete-perspective`, `add-perspective-member`, `remove-perspective-member`, `list-perspectives`

### Highlights

**No XMLA/TOM required.** The Power BI Modeling MCP edits models over a live Analysis Services connection. fabio achieves the same authoring tasks purely over REST by round-tripping the model definition — so it runs anywhere a Fabric token works, with no gateway or client library.

**Add row-level security in two commands:**
```bash
fabio semantic-model add-role --id <model> --name Regional
fabio semantic-model set-rls --id <model> --role Regional --table Customer \
  --filter "'Customer'[Region] = \"West\""
```

**Build a time-intelligence calculation group:**
```bash
fabio semantic-model add-calculation-group --id <model> --name "Time Intelligence"
fabio semantic-model add-calculation-item --id <model> --group "Time Intelligence" \
  --name YTD --expression "CALCULATE(SELECTEDMEASURE(), DATESYTD('Date'[Date]))"
```

**Translate a model to French:**
```bash
fabio semantic-model add-culture --id <model> --culture fr-FR
fabio semantic-model set-translation --id <model> --culture fr-FR \
  --table Sales --caption Ventes
```

**Create a relationship, add a calculated column, or a drill-down hierarchy:**
```bash
fabio semantic-model add-relationship --id <model> \
  --from-table Sales --from-column CustomerKey --to-table Customer --to-column CustomerKey
fabio semantic-model add-calculated-column --id <model> --table Sales \
  --name "Margin %" --expression "DIVIDE('Sales'[Margin], 'Sales'[Amount])"
fabio semantic-model add-hierarchy --id <model> --table Geo --name Geography \
  --level Country --level City
```

### Safety & correctness

- Every mutation is marked `mutates`/`destructive` in the agent command schema and is `--dry-run`-guarded.
- `delete-table` **cascades**: it also removes relationships and RLS filters that reference the table, because `updateDefinition` rejects a dangling reference.
- Guardrails ground-truthed against live Fabric: calculation groups auto-set `discourageImplicitMeasures`; DAX UDFs auto-bump the model to `compatibilityLevel` 1702; a table can't delete its last partition; duplicate/unknown objects return typed `CONFLICT`/`NOT_FOUND` errors.

### Documentation

- New "Semantic Model API Behaviors Discovered" notes documenting the exact TMDL shapes and Fabric quirks for every object type.
- The `fabio-bi` agent sub-skill, the machine-readable command schema, and the docs-site command reference all cover the 52 new commands.
- 14 new promptfoo eval cases exercising the authoring workflows.

### Stats

- **15** feature commits; **27** files changed (**+14,817 / -24**).
- **13** new `semantic_model/` source modules; **52** new subcommands (37 → 89).
- **14** new live end-to-end lifecycle tests plus ~100 new unit tests; full suite green on Linux (x64/arm64), macOS (x64/arm64), and Windows (x64).

**Full Changelog**: https://github.com/iemejia/fabio/compare/v0.57.0...v0.58.0
