---
title: How Fabio avoids the chicken-and-egg problem
description: Why Fabio resolves cross-item ID references in a single deploy pass, with no two-phase deploy — unlike fabric-cicd.
---

A Fabric item's definition frequently references **another item's environment-specific GUID**. A few common cases:

- A **Semantic Model**'s Direct Lake connection (`expressions.tmdl`) points at a **Lakehouse** ID.
- A **Notebook**'s `default_lakehouse` metadata block points at a **Lakehouse** ID.
- A **Variable Library** value set stores a Lakehouse or Warehouse ID that other items read at runtime.

Those GUIDs are assigned by Fabric when the item is created in the target workspace. On a **first deploy to an empty workspace**, the referenced item does not exist yet, so its ID cannot be substituted ahead of time. This is the classic "chicken-and-egg" problem of Fabric CI/CD: the dependent item needs an ID that only comes into existence once the dependency is created.

## How fabric-cicd handles it

`fabric-cicd`'s parameterization exposes a `$items.<Type>.<Name>.$id` placeholder. It is resolved by **querying the live target workspace** during parameterization — *before* items are published. On a brand-new, empty workspace that query returns nothing, so the placeholder cannot resolve and the deploy fails.

The documented workaround is a **two-phase deploy**:

1. **Phase 1** — call `publish_all_items()` scoped to only the dependency types (e.g. `item_type_in_scope=["Lakehouse", "Ontology"]`) to create them first.
2. **Phase 2** — call `publish_all_items()` again for the remaining items. Now the Phase 1 items exist, so `$items.Lakehouse.<Name>.$id` resolves against their live GUIDs.

You have to split the pipeline into two calls and know in advance which item types to seed first.

## How Fabio handles it

Fabio needs **no two-phase hack**. `fabio deploy apply`:

1. Groups all create/update actions by item type and sorts them by a **topological deploy priority** (`DEPLOY_ORDER`, aligned 1:1 with fabric-cicd's serial publish order, with Fabric types Fabio also supports slotted in). Dependencies such as Lakehouse and Ontology are created before the items that reference them.
2. As **each item is created**, records its freshly-assigned target-workspace GUID in an in-run `(Type, Name) → GUID` map.
3. When a later item's `$items.Type.Name.id` reference is resolved, the dependency has **already been created in the same run**, so the reference resolves against the real GUID.

The result: cross-item references resolve **in a single pass**, even against a completely empty workspace — no seeding phase, no second call.

```bash
# One command. Lakehouse is created first (topological order); its new GUID is recorded
# and used to resolve $items.Lakehouse.PatternsLakehouse.id for the Semantic Model,
# Notebook, and Variable Library later in the SAME run.
fabio deploy apply --source ./export --workspace "Production (empty)" \
  --parameters ./parameters.json --env prod
```

The same map is **seeded with the GUIDs of pre-existing items** being updated or skipped, so cross-references also resolve cleanly on incremental re-deploys to a workspace that already has content.

## The one exception: `--strategy bulk`

`fabio deploy apply --strategy bulk` submits all creates/updates in a single additive `bulkImportDefinitions` batch. Because every item in that batch is created at once, there is no "earlier item" whose GUID can be recorded mid-batch, so `$items` cross-references cannot be resolved within a single bulk call. The bulk strategy is best for workspace-to-workspace cloning and deploying independent items to empty workspaces.

For first deploys that contain cross-item `$items` references, use the **default per-item strategy** (the default) — it resolves them in one pass.

## Why this matters

- **Simpler pipelines** — one `deploy apply` step instead of two scoped calls plus the logic to decide which types to seed.
- **Fewer failure modes** — no "phase 1 succeeded, phase 2 failed halfway" partial states to reason about.
- **Works the same on empty and populated workspaces** — the in-run map is seeded from existing items, so first deploys and incremental re-deploys use the same code path.

See also: [Deploy parameter substitution](../../reference/commands/deploy/) and `fabio context best-practices deploy-parameters`.
