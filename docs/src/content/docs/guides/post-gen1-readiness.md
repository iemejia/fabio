---
title: Verify readiness after a Gen1 upgrade
description: Separate migration completion from first refresh, loaded data and consumer acceptance.
---

A successful in-place migration preserves item identity, not cached data, fresh
destinations, compatible consumers or reader permissions. Use the supported
acceptance workflow rather than adding an automatic refresh to the upgrade:

```bash
fabio --readonly context workflow gen1-upgrade-readiness
fabio --readonly context best-practices post-gen1-readiness
```

## Establish the exact work

Retain the original receipt and pre-upgrade baseline privately. Observe only that
receipt with `dataflow follow-upgrade`; require validated scope, counts and a
successful result for the selected dataflow. Partial or unknown results never
authorize another upgrade. A transport trace is not a local receipt ID.

The **first Gen2 refresh requires separate approval**. This acceptance workflow
does not submit one. If there is no separately approved refresh receipt, record
`first_refresh:not_checked` and stop acceptance. Observe the exact retained job
with `job-scheduler get-instance`, never latest history. Acceptance, a local
timeout or a cancel request is not confirmed completion.

## Check the destination, not just the job

Compare the current dataflow definition, actual destinations/mappings and complete
schedule inventory to the reviewed baseline. Check incremental-refresh settings,
watermarks, expected names and linked-query behavior explicitly. Missing or hidden
settings remain unknown; they do not prove equivalence.

Run reviewed read-only `SELECT` checks against the actual destination tables:

- Use `warehouse query` for a Warehouse, or `sql-endpoint query` with the actual
  SQL analytics endpoint ID for a Lakehouse—not the Lakehouse ID.
- Check expected schema, row bounds, batch/business keys and freshness
  high-watermarks. A nonempty old table can still be stale; a completed job alone
  does not prove the expected batch was loaded.
- Do not load, overwrite, change destinations or disable incremental refresh to
  make validation pass. Keep resource details and query results private.

## Check consumers and reader access

For every selected downstream model/report, `semantic-model readiness` can observe
schema, selected-table counts, the exact separately approved model refresh request
and report binding. Use explicit model/report workspaces and intended authorized
reader identities. Queryability does not prove freshness, visual/business
correctness, or permission for other users. Verify those expectations separately.

Do not grant roles, take ownership, rebind, change storage/security modes, rewrite
connectors or refresh consumers automatically. Legacy `PowerBI.Dataflows` consumers
and Viewer access require explicit compatibility review; preserved IDs do not fix
them.

## Acceptance gate

Record each applicable check as `ready`, `blocked`, `needs_action`, `unknown` or
`not_checked`, with its exact scope, evidence and observation time. Pass only when
every applicable required check is ready. Applicability comes from the approved
baseline, never from an absent API response. Missing fixtures, original scope,
baseline or authorization are blockers—not successful validation.

This is an observation workflow built from existing commands, **not an automatic
refresh or repair executor**. Live validation needs an existing authorized fixture
and separately approved refreshes; it must not recreate or replay an upgrade.
