---
title: Review destructive capabilities and previews
description: Interpret generated safety metadata, conditional modes and confirmation notices.
---

A command being mutating does not mean every invocation destroys data. Conversely,
a command missing a safety annotation is not proof that it is safe. Fabio now
checks destructive-discovery requirements against the **actual clap command tree**,
not just commands already labelled destructive.

## Read the native metadata

- `destructive: true` identifies a destructive capability, including deletion,
  definition replacement, task cancellation and reviewed workflows such as checkout
  or deployment apply. It must also be `mutates: true`.
- `destructive_flags` lists optional modes that can make an otherwise ordinary
  invocation destructive: examples include purge, local overwrite, table drop and
  cancellation on timeout. A normal invocation can have no destructive preview
  marker; selecting the risky mode adds the marker and confirmation notice.
- `destructive_scope: "actions"` identifies an explicitly reviewed custom preview
  whose selected action set determines risk. Deployment apply is such a workflow:
  create-only/no-op previews report false, while updates, rename/republication,
  deletion and force-all overwrites report true.
- Nested native `subcommands` contain their own flags and safety metadata. A Git
  relation delete is destructive; relation list is not. Runtime lookup uses the
  complete canonical path rather than treating every child as destructive.

MCP's `destructiveHint` is deliberately conservative: it reports a capability,
including risky optional modes and nested children, not permission to execute it.
This change does not redesign the invocation interface of every nested tool.

## Preview first, then obtain approval

```bash
fabio --wrap-untrusted --readonly --dry-run git checkout \
  --workspace "$WORKSPACE_ID" --branch "feature/review"

fabio --wrap-untrusted --readonly --dry-run sql-database import \
  --workspace "$WORKSPACE_ID" --id "$DATABASE_ID" \
  --file fixture.csv --table ReviewedTable --drop-if-exists
```

When a selected invocation is destructive, its preview includes `destructive:true`
and, for a detected AI agent, an `agentNotice` requiring human confirmation before
execution. These are **not approval tokens**. A preview is not a reservation or
proof that readiness/permissions were checked. Read-modify-write previews can need
read-only API calls; Git checkout's scoped preview is offline and explicitly says
readiness was not checked.

Actual checkout, SQL import, statistics writes and local overwrite export reject
`--readonly`. Dry-run remains available. `--drop-if-exists` cannot be combined with
`--no-create-table`: that would remove the table without recreating it for import.
The drop flag is a dangerous bypass, not a benign import mode.

Definition-overwrite previews preserve input validation before returning. The
ontology updater reports part paths/counts after local scope/input checks, without
dispatching its replacement POST. No implicit backup, cancellation, grant or cloud
mutation is performed by discovery or preview.

## How omissions are caught

Authored judgment lives in `src/commands/context/data/agent/safety_policy.json`.
Generation applies destructive vocabulary, reviewed workflow rules, real local
flags and explained exceptions. It overrides stale false annotations and emits
nested metadata. Global flags are not mistaken for every command's local capability.

An independent test walks clap and checks published metadata for the resulting
requirements—even when an entry is entirely absent or has no destructive label.
Other tests verify actual preview markers/notices, active/inactive conditional
modes, nested lookup, scoped plans, readonly rejection and malformed-input guards
using local fixtures and mocks.

This is not automatic understanding of arbitrary future business logic. A new
custom workflow with destructive behavior must receive reviewed policy judgment
and runtime guard tests; reversible configuration changes must not be blindly
labelled destructive. Tests do not require or authorize live destructive testing.
