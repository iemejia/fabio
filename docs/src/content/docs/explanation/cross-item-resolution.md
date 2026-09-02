---
title: How Fabio avoids the chicken-and-egg problem
description: Why Fabio resolves cross-item references in a single deploy pass, with no two-phase deploy — unlike fabric-cicd's live-workspace $items lookup.
---

A Fabric item's definition frequently references **another item**. A few common cases:

- A **Semantic Model**'s Direct Lake connection (`expressions.tmdl`) points at a **Lakehouse**.
- A **Notebook**'s `default_lakehouse` metadata block points at a **Lakehouse**.
- A **Data Agent** points at an **Ontology**.

The referenced item's real GUID is assigned by Fabric when it is created in the target workspace. On a **first deploy to an empty workspace**, the referenced item does not exist yet, so its GUID cannot be substituted ahead of time. This is the classic "chicken-and-egg" problem of Fabric CI/CD.

## Logical IDs vs raw GUIDs

In the Fabric Git-integration format, cross-item references are stored as the **referenced item's logical ID** — the stable, environment-independent `config.logicalId` from its `.platform` file — *not* its per-workspace item GUID. On deploy, each logical-ID reference must be rewritten to the deployed item's real GUID in the target workspace.

Sometimes a **raw GUID** must instead be injected into a definition that has no logical-ID slot for it — the canonical example is a Lakehouse GUID stored inside a Variable Library value set. This is the case fabric-cicd's `parameter.yml` `$items.<Type>.<Name>.$id` placeholder exists for.

## How fabric-cicd handles the raw-GUID case

fabric-cicd resolves `$items.<Type>.<Name>.$id` by **querying the live target workspace** during parameterization — *before* items are published. On a brand-new, empty workspace that query returns nothing, so the placeholder cannot resolve and the deploy fails. The documented workaround is a **two-phase deploy**:

1. **Phase 1** — publish only the dependency types (e.g. `item_type_in_scope=["Lakehouse", "Ontology"]`) to create them first.
2. **Phase 2** — publish the remaining items; now `$items.Lakehouse.<Name>.$id` resolves against the Phase 1 items' live GUIDs.

You must split the pipeline into two calls and know in advance which item types to seed first.

## How Fabio handles it

Fabio needs **no two-phase hack**. `fabio deploy apply`:

1. Groups all create/update actions by item type and sorts them by a **topological deploy priority** (`DEPLOY_ORDER`, aligned 1:1 with fabric-cicd's serial publish order). Dependencies such as Lakehouse and Ontology are created before the items that reference them.
2. As **each item is created**, records its freshly-assigned target-workspace GUID in an in-run `logicalId → deployed-GUID` map (`created_ids`).
3. When a later item is deployed, `resolve_logical_ids_in_payload` rewrites its logical-ID references using the dependencies **already created in the same run**.

The result: cross-item references resolve **in a single pass**, even against a completely empty workspace — no seeding phase, no second call.

```bash
# One command. Lakehouse is created first (topological order); its new GUID is recorded
# and used to rewrite the Semantic Model / Notebook / Data Agent logical-ID references
# to it later in the SAME run.
fabio deploy apply --source ./export --workspace "Production (empty)" --env prod
```

The map is also **seeded with the GUIDs of pre-existing items** being updated or skipped, so references resolve cleanly on incremental re-deploys to a workspace that already has content.

## The one exception: `--strategy bulk`

`fabio deploy apply --strategy bulk` submits all creates/updates in a single additive `bulkImportDefinitions` batch. Because every item in that batch is created at once, there is no "earlier item" whose GUID can be recorded mid-batch, so interdependent items are rejected with `DependenciesCouldNotBeResolved`. The bulk strategy is best for workspace-to-workspace cloning and deploying independent items to empty workspaces.

For first deploys that contain cross-item references, use the **default per-item strategy** (the default) — it resolves them in one pass.

## A note on the `$items` parameter

fabric-cicd's `parameter.yml` `$items.<Type>.<Name>.$id` is a *parameter substitution* construct resolved against the live workspace. Fabio's cross-item resolution is a different, deploy-native mechanism: it rewrites **Fabric logical-ID references** using GUIDs recorded during the same apply run. For the raw-GUID-into-a-value-set case, prefer a **Variable Library** (whose value set is activated per environment by `deploy apply --env`) or a `find_replace` parameter with an explicit per-environment literal / `$ENV:` value.

## Why this matters

- **Simpler pipelines** — one `deploy apply` step instead of two scoped calls plus the logic to decide which types to seed.
- **Fewer failure modes** — no "phase 1 succeeded, phase 2 failed halfway" partial states to reason about.
- **Works the same on empty and populated workspaces** — the in-run map is seeded from existing items, so first deploys and incremental re-deploys use the same code path.

See also: [Deploy parameter substitution](../../reference/commands/deploy/) and `fabio context best-practices deploy-parameters`.
