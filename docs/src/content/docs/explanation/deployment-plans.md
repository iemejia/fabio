---
title: Fabric deployment plans and Fabio CI/CD
description: Understand native deployment orchestration, distinguish it from Fabio's diff planner, and validate portable plan artifacts safely.
---

A Fabric deployment plan is an **optional orchestration extension**, not a
replacement for CI/CD. Existing deployments without an attached plan behave as
before. Fabio's build, test, and release workflows are unaffected.

## Three different concepts

| Concept | Responsibility |
| --- | --- |
| `fabio deploy plan` | Preview a content-hash create/update/delete/skip changeset. |
| Fabric `DeploymentPlan` item | Version explicit item order and pre/post job actions in `plan.yml`. |
| Fabric deployment pipeline | Promote content between stages/workspaces, optionally with a plan attached. |

Deploying a lakehouse definition does not populate tables. Deploying a notebook
definition does not run it. A native plan can deploy a preparation notebook, run
it to create required tables, then deploy a dependent semantic model and report.
That execution **between** item deployments is different from a global
post-deployment hook.

Each group deploys one item. `dependsOn` controls group/action order; file order
does not. Supported action items currently include notebooks, data pipelines,
Dataflows Gen2, copy jobs, and user data functions, using job type `Execute`.
Actions are not arbitrary shell or Fabio commands.

## Artifact portability is not execution

The plan is a versioned item containing `plan.yml` and optional `.platform`
metadata. Creating or importing it does **not** run its actions.

Fabio's generic `deploy apply` strategies can store the artifact, but none attaches
it as an executable native plan. Explicit attachment is currently available on:

- `git pull --deployment-plan-logical-id`: Update From Git.
- `deployment-pipeline deploy --deployment-plan-id`: stage deployment.
- `item bulk-import-definitions --deployment-plan-logical-id`: low-level bulk import.

The high-level `deploy apply --strategy bulk` path uses a compatible API but does
not yet pass a plan reference. Do not silently switch a per-item deployment into
bulk mode to obtain orchestration.

Plan `logicalId` references and `.platform` identities must remain **logical** IDs
across workspaces. They are not runtime bindings to rewrite into target physical
object IDs. Fabio preserves these fields in generic per-item deployment while
continuing to resolve physical bindings in other definition payloads.

Assigned identity comes from first-class item metadata: `getDefinition` can still
emit a zero `.platform` placeholder after assignment. Export and rename detection
prefer metadata. In a plan-bearing per-item deployment, Fabio restores source
logical IDs on newly created items, but refuses to silently change an existing
target item's identity. Review and explicitly resolve existing identity conflicts
before deploying; override flags do not bypass that check.

## Validate before attaching a plan

```bash
fabio --wrap-untrusted deployment-plan validate --file plan.yml
fabio --wrap-untrusted deployment-plan validate --file plan.yml --source ./items --strict
fabio --wrap-untrusted context examples deployment-plan validate
```

Validation is offline and never executes jobs or downloads a user-supplied
schema. It accepts raw YAML or a Fabric definition envelope. Checks include the
current schema/version, non-zero UUIDs, names, limits, group/action dependencies,
cycles, action placement, parameter names, and supported action types when a
local source catalog is supplied.

An item absent from `--source` may already exist in the target. Absence therefore
produces a warning, as do incomplete draft actions or known execution-readiness
issues; `--strict` turns warnings into failures. This keeps storing/versioning a
draft distinct from approving it for execution. This is not a tenant
readiness check: it does not verify permissions, active variable values,
connections, runtime lineage, or data availability.

`item validate-definition --type DeploymentPlan` shares intrinsic checks, and
generic deploy source parsing rejects malformed plan artifacts before mutating
the workspace. A valid plan is not proof its jobs are safe or will succeed.

## Prepare the target before actions run

Native actions execute inside the deployment operation. Fabio's generic `--env`
value-set activation is a **post-hook**, so it cannot prepare values for actions
that already ran. New workspaces initially use Default. Prepare required
configuration and activate the intended value set before starting a native
operation. Avoid running the same work through both plan actions and
`--post-run-item` or automatic post-hooks.

Plans do not provision capacities, workspace identities, connections, gateway
bindings, or Spark settings. External CI still owns approvals, authentication,
infrastructure preparation, and multi-workspace release coordination.

## Failure and safety semantics

- A reference does not automatically select/import every referenced item. Select
  an incoming source-stage plan itself when using a selective stage deployment.
- Actions can change data even when a metadata changeset reports no deletions.
- Failures stop subsequent deployment but do not roll back completed items or
  job side effects. Inspect completed work before retrying; design idempotent jobs.
- Groups/actions currently execute serially.
- Job completion does not guarantee downstream SQL/catalog readiness; perform
  readiness checks inside producing jobs.
- API automation documents `beta=true`, the original operation's scope, and
  `Item.Execute.All`. Service identities must be supported by all participating jobs.
- One operation attaches at most one plan and targets one workspace.

For deployment workflow setup, see [GitHub Actions CI/CD](../../guides/github-actions-cicd/).
The [Microsoft automation guide](https://learn.microsoft.com/en-us/fabric/cicd/deployment-plan/deployment-plan-automation)
documents the native request contracts and permissions.
