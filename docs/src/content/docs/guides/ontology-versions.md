---
title: Back up and restore an ontology version
description: Capture an exact snapshot ID, preserve private definition backups, and verify a destructive restore independently.
---

Ontology version APIs are beta. A version captures the current definition, not a
backup of lakehouse rows, access grants, or dependent-item readiness. Listing or
showing a version returns **metadata only**, not its stored definition.

A newly created ontology currently uses **generation 2 TMDL**. Preserve its actual
exported format and all required parts. The legacy JSON entity editor and
`list-entity-types` refuse TMDL models rather than silently treating them as empty;
inspect the raw or decoded definition instead. Do not mix `EntityTypes/*.json`
parts into a TMDL model or switch generations as error recovery.

## Save a private baseline and create a snapshot

Read the current definition and save its raw `definition` payload to a **new,
private file**. Shell redirection can overwrite files: choose a fresh destination
and do not save decoded output as an importable backup.

```bash
fabio --wrap-untrusted ontology get-definition --workspace "$WS" --id "$ONTOLOGY" --readonly --query definition -o plain
fabio --wrap-untrusted ontology create-version --workspace "$WS" --id "$ONTOLOGY" --name before-change --creation-source Manual --dry-run
fabio --wrap-untrusted ontology create-version --workspace "$WS" --id "$ONTOLOGY" --name before-change --creation-source Manual
```

Retain the returned version ID with the baseline and requested scope. Names are
not unique: each create produces a distinct version, even for identical input.
An ontology supports at most 50 versions. Do not retry an uncertain create or
automatically delete old versions to free space.

## Preview the exact restore

Before restoring, save a **separate** backup of the current definition. Confirm
the workspace, ontology and selected version; do not infer the target from a
name or the latest list position.

```bash
fabio --wrap-untrusted ontology show-version --workspace "$WS" --id "$ONTOLOGY" --version-id "$VERSION" --readonly
fabio --wrap-untrusted ontology restore-version --workspace "$WS" --id "$ONTOLOGY" --version-id "$VERSION" --dry-run
```

Restore irreversibly replaces the current definition. Review the destructive
preview and obtain approval before removing `--dry-run`. A missing-options error
does not authorize adding `allowOverrideItems` or other override options.

## Verify separately after approval

```bash
fabio --wrap-untrusted ontology restore-version --workspace "$WS" --id "$ONTOLOGY" --version-id "$VERSION"
fabio --wrap-untrusted ontology get-definition --workspace "$WS" --id "$ONTOLOGY" --decode --readonly
```

Success now returns `status: restored`, the requested workspace/ontology/version,
`destructive: true`, and the original response under `serviceResponse` (often
`null`). Both `definitionVerified` and `dependentItemsVerified` remain `false`:
the CLI has not compared the model or queried its data. `verifyAfter` suggests
the read-only definition inspection command without executing it automatically.

Compare decoded authored model parts **by path**, not array order,
against the saved baseline. Review service-managed metadata separately. Validate
bindings, source permissions and dependent artifacts before claiming query
readiness. For a legacy JSON ontology, `list-entity-types --include-properties
--readonly` is also available. After a timeout or server error, inspect the existing item rather
than replaying restore.

The machine-readable recipe is available through
`fabio context workflow ontology-version-recovery`; inspect the response shape
with `fabio context examples ontology restore-version`.
