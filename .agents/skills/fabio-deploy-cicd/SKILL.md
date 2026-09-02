---
name: fabio-deploy-cicd
description: >-
  Intent-scoped fabio skill for Fabric CI/CD: stateless content-hash deploy (export/validate/plan/apply), Git integration, deployment pipelines, and variable libraries for environment-specific config. Use for promoting Fabric items between environments, Git lifecycle, and parameterized deployments. Triggers: "deploy", "ci/cd", "promote to production", "deploy plan", "deploy apply", "git commit", "git pull", "deployment pipeline", "variable library", "value set".
license: MIT
---

# fabio-deploy-cicd — Deploy & CI/CD — stateless content-hash deployment, Git, pipelines, variable libraries

> **Generated file — do not edit by hand.** This intent-scoped sub-skill of the `fabio` skill is generated from fabio's command schema plus authored judgment. Regenerate with `cargo test generate_subskills -- --ignored`. For install, auth, output envelope, global flags, and agent-safety rules, see the root `fabio` skill.

> **Prefer runtime introspection.** This index is a snapshot; the installed binary is always authoritative. Use `fabio context agent --group <group>` and `fabio context describe <group> <command>` for exact flags and output shapes.

## When to use
- Exporting a workspace to disk and deploying it to another environment.
- Planning (dry-run diff) before applying changes; converging idempotently.
- Git integration: connect, status, commit, pull, checkout, branch-out.
- Managing base/branch workspace relations as an independent resource (git relation list/create/delete), separate from the branch-out flow.
- Managing deployment pipelines (dev/test/prod stages).
- Managing variable libraries and activating environment value sets.
- Branch-out development: after Fabric 'Branch out' creates a Git-synced feature workspace, rebind the local definition files from dev IDs to the feature workspace's IDs with 'deploy rebind' (NOT 'deploy apply' — that would drift a Git-synced workspace).
- Gating a PR back to the shared branch: 'deploy validate --pr-ready' asserts the repo is rebound to the expected env and carries no foreign-env IDs or stray value-set files.

## When NOT to use (route elsewhere)
- Porting from Synapse/Databricks/HDInsight -> use the migration-engineer persona + migration workflows.
- One-off item CRUD -> use the specific workload skill (fabio-lakehouse, fabio-rti-kql, etc.).

## Command index

Generated from fabio's command schema. For full flag details use `fabio context agent --group <group>` or `fabio context describe <group> <command>`.

### fabio deploy
Deploy item definitions from a local directory to a workspace

| Command | Mutates | Description |
|---|---|---|
| `fabio deploy apply` | yes | Execute deployment (create/update/delete items) |
| `fabio deploy export` | no | Export workspace item definitions to a local directory |
| `fabio deploy init-params` | no | Generate a parameters.json scaffold by scanning or diffing exported definitions |
| `fabio deploy plan` | no | Preview what would be deployed (create/update/delete/skip) |
| `fabio deploy rebind` | yes | Rewrite environment-specific IDs in local definition files, in place (offline) |
| `fabio deploy validate` | no | Validate source directory locally (no API calls). Checks .platform files, item types, duplicate names/logical IDs, cross-references, and parameters |

### fabio git
Manage Git integration (connect, commit, pull, status)

| Command | Mutates | Description |
|---|---|---|
| `fabio git branch-out` | yes | Create a feature workspace from the current branch (branch out) |
| `fabio git checkout` | yes | Switch to a different branch (disconnect + connect + init) |
| `fabio git commit` | yes | Commit workspace changes to the connected remote branch |
| `fabio git connect` | yes | Connect a workspace to a Git repository |
| `fabio git connection` | no | Show or manage Git connection and credentials |
| `fabio git credentials` | no | Manage Git credentials |
| `fabio git disconnect` | yes | Disconnect a workspace from Git |
| `fabio git init` | yes | Initialize a workspace Git connection (required after connect) |
| `fabio git pull` | yes | Pull remote changes into the workspace (update from Git) |
| `fabio git relation` | yes | Manage workspace relations (base/branch links between workspaces, Preview) |
| `fabio git show-tracked` | no | Show tracked items and their Git sync status |
| `fabio git status` | no | Show workspace Git status (changes, conflicts) |

### fabio deployment-pipeline
Manage deployment pipelines (CI/CD stages, deploy items)

| Command | Mutates | Description |
|---|---|---|
| `fabio deployment-pipeline add-role-assignment` | yes | Add a role assignment to a deployment pipeline |
| `fabio deployment-pipeline assign-workspace` | yes | Assign a workspace to a deployment pipeline stage |
| `fabio deployment-pipeline create` | yes | Create a new deployment pipeline |
| `fabio deployment-pipeline delete` | yes | Delete a deployment pipeline |
| `fabio deployment-pipeline delete-role-assignment` | yes | Delete a role assignment from a deployment pipeline |
| `fabio deployment-pipeline deploy` | yes | Deploy items from one stage to another |
| `fabio deployment-pipeline list` | no | List deployment pipelines |
| `fabio deployment-pipeline list-operations` | no | List deploy operations for a deployment pipeline |
| `fabio deployment-pipeline list-role-assignments` | no | List role assignments for a deployment pipeline |
| `fabio deployment-pipeline list-stage-items` | no | List items in a deployment pipeline stage |
| `fabio deployment-pipeline list-stages` | no | List stages in a deployment pipeline |
| `fabio deployment-pipeline show` | no | Show details of a deployment pipeline |
| `fabio deployment-pipeline show-operation` | no | Show details of a deploy operation |
| `fabio deployment-pipeline show-stage` | no | Show details of a deployment pipeline stage |
| `fabio deployment-pipeline unassign-workspace` | yes | Unassign the workspace from a deployment pipeline stage |
| `fabio deployment-pipeline update` | yes | Update a deployment pipeline |
| `fabio deployment-pipeline update-stage` | yes | Update a deployment pipeline stage configuration |

### fabio variable-library
Manage variable libraries (shared variables)

| Command | Mutates | Description |
|---|---|---|
| `fabio variable-library activate-value-set` | yes | Activate a value set for a variable library in a workspace |
| `fabio variable-library create` | yes | Create a new variable library |
| `fabio variable-library delete` | yes | Delete a variable library |
| `fabio variable-library get-definition` | no | Get the definition of a variable library |
| `fabio variable-library list` | no | List variable librarys in a workspace |
| `fabio variable-library list-value-sets` | no | List value sets defined in a variable library |
| `fabio variable-library show` | no | Show details of a variable library |
| `fabio variable-library update` | yes | Update variable library properties |
| `fabio variable-library update-definition` | yes | Update the definition of a variable library |

## Must / Prefer / Avoid
### MUST
- Run 'deploy plan' (dry-run) and review the changeset before 'deploy apply'.
- After 'deploy apply', audit convergence: pass 'deploy apply --verify' (adds a `verification` block: converged + per-item discrepancies), or re-run 'deploy plan' (a converged deployment shows every item as Skip / summary.create/update/delete == 0). --verify is report-only (exit code unchanged) and reuses the plan's content-hash engine. Any discrepancy / non-Skip item means the deployment did NOT match the plan: a missing/failed item, drift, or API normalization of hand-authored content.
- Use the fabric Git Integration '.platform' directory format as the source.
- Name variable-library value sets to match --env values so they auto-activate on apply.

### PREFER
- --strategy default (per-item, content-hash skip) for iterative CI/CD; --strategy bulk only for large initial deploys to an empty workspace.
- deploy export + Git for snapshotting a workspace over ad-hoc manual recreation.
- deploy validate (offline, no API calls) as a fast pre-flight before plan/apply.

### AVOID
- deploy apply --force-all without a reviewed plan (it overwrites everything).
- --strategy bulk on a Git-connected workspace (not supported; use default).
- --delete-orphans on protected data types without --allow-delete-types and explicit user approval.

## Key gotchas
- Deploy is STATELESS — content-hash diffing against the live workspace, no state file. --workspace accepts a display name or GUID.
- The .platform part IS sent (enables metadata propagation) but is EXCLUDED from the content hash, so idempotent skip still works.
- --strategy: default (per-item, content-hash skip) | bulk (fast initial deploy to an empty, non-Git workspace) | sequential (debugging).
- git relation (WorkspaceRelations, preview) manages base/branch links between workspaces as a standalone resource — distinct from 'git branch-out', which creates+connects a feature workspace in one flow.
- Raw Power BI Desktop PBIP folders deploy directly: a '<name>.Report' / '<name>.SemanticModel' folder with NO '.platform' sidecar is discovered by folder-name suffix (Report needs definition.pbir, SemanticModel needs definition.pbism). Such items have no logicalId, so rename tracking is off (plan warns 'no logicalId') and they match deployed items by (type, name); a v2-PBIR report still rebinds to its model by name.
- 'deploy apply' resolves cross-item references stored as Fabric LOGICAL IDs (the referenced item's .platform logicalId) in a SINGLE pass even to an EMPTY workspace: it creates items in topological order and records each new GUID mid-run (created_ids), then rewrites later items' logical-ID references to them (resolve_logical_ids_in_payload). Unlike fabric-cicd's parameter.yml $items placeholder, which queries the live workspace before publish and needs a two-phase deploy on first run. Exception: --strategy bulk creates all items at once and rejects interdependent items (DependenciesCouldNotBeResolved); use the default per-item strategy for first deploys with cross-item references. Do NOT rely on the $items.Type.Name.id PARAMETER form for cross-item GUIDs — it resolves against an empty map during deploy and is skipped-with-warning; use logical-ID references, a Variable Library value set, or a find_replace literal/$ENV: instead.
- 'deploy rebind' is OFFLINE (no API calls): it rewrites the from-env literal values to the to-env values directly in the on-disk .platform files. It only handles LITERAL / $ENV: values; deploy-time dynamics ($workspace.id, $items.Type.Name.id) are skipped with a warning (those are resolved by 'deploy apply' against the live workspace). Reverse from/to before opening a PR.
- Not all item types are Git-tracked. 'deploy export' only captures types that support the item-definition API (getDefinition); types that don't are listed in the export's `skipped` + a `tracking_note` (category: not-git-tracked). Those items are NOT recreated by 'deploy apply' — promote them with 'deployment-pipeline deploy' (Deployment-Pipeline-only) or recreate them manually per environment. Never assume export+apply fully replicated a workspace without checking `skipped`/`tracking_note`. See 'context best-practices item-tracking-categories'.

## Troubleshooting
| Symptom | Fix |
|---|---|
| Plan shows a rename as delete+create | Ensure the item has a stable logicalId in its .platform file so rename detection matches it. |
| Re-running apply keeps changing the same items (or --verify reports 'definition content differs') | The .platform part is excluded from the content hash; a real convergent deploy should show 0 changes. Causes: (1) portal edits since deploy, or (2) the API NORMALIZES hand-authored content on first ingest (notebooks especially) so the source hash != deployed hash. Fix (2) by re-exporting after the first deploy ('deploy export') and deploying the canonical form — it then converges. Verify with 'deploy apply --verify'. |
| bulk strategy fails on a Git-connected workspace | Bulk import requires no Git integration; use --strategy default. |
| bulk deploy fails with DependenciesCouldNotBeResolved or BadSystemFiles | The bulk API creates all items in one batch and cannot sequence interdependent items (Report->SemanticModel, KQLDatabase->Eventhouse) or resolve duplicate/placeholder logicalIds. Re-run with --strategy default (per-item), which creates dependencies first and resolves cross-item logical-ID references in a single pass. fabio appends this hint to the bulk failure automatically. |
| Connections resolve to TODO in params | Run deploy init-params --resolve-connections and fill in the correct connection IDs before apply. |
| PR to dev blocked / feature workspace GUIDs leaked into the shared branch | Run 'deploy rebind --from-env <feature> --to-env dev' to restore dev IDs, delete any feature value-set files, then confirm with 'deploy validate --pr-ready --expect-env dev --allow-value-set Test,Prod'. |
| Semantic model / notebook in a branched-out workspace still points at the dev lakehouse | 'deploy apply' cannot target a Git-synced Branch-out workspace (causes drift). Rewrite the local files instead: 'deploy rebind --from-env dev --to-env <feature>', commit, then sync via 'git pull' (Update from Git). |

## Safety
- --force-all overwrites ALL matched items regardless of content changes — irreversible; run 'deploy plan' first.
- --delete-orphans removes workspace items not in source; protected data types (Lakehouse/Warehouse/SQLDatabase/Eventhouse/KQLDatabase) require --allow-delete-types.
- Deploy output includes a 'destructive' boolean — surface it to the human before applying.

## Shared references
Cross-cutting operational guidance (the "common" layer) — consult the relevant topic before non-trivial work:

| Reference | Covers |
|---|---|
| `fabio context best-practices cicd-lifecycle` | End-to-end CI/CD lifecycle for Microsoft Fabric solutions: Git integration, feature workspaces, variable libraries, deployment strategies, auto-binding, data orchestration, and release processes. Covers single-workspace and multi-workspace solutions. |
| `fabio context best-practices deploy-parameters` | Deploy parameters enable environment-specific value injection (dev/staging/prod) via find-replace, JSONPath key-value, Spark pool, and semantic model binding rules. Values support dynamic variables including $ENV:VAR_NAME for CI/CD secrets injection. |
| `fabio context best-practices variable-libraries` | Variable libraries are Microsoft's strategic Fabric capability for managing environment-specific settings across dev/test/prod. They store parameterized values (connection strings, paths, IDs) that items read at runtime, eliminating hardcoded environment references from item definitions. |
| `fabio context best-practices item-tracking-categories` | Not all Fabric item types can be managed the same way in CI/CD. From a lifecycle perspective they fall into three categories — Git-tracked (definition-backed, moved by fabio deploy export/plan/apply and Fabric Git integration), Deployment-Pipeline-only (promoted workspace-to-workspace via fabio deployment-pipeline deploy but NOT version-controllable in Git), and Manual (recreated by hand per environment). fabio's Git-based deploy only captures Git-tracked items; deploy export flags the rest with a tracking_note. Both official supported-items lists evolve, so always verify against Microsoft docs. |
| `fabio context best-practices fabric-cicd-migration` | Guide for teams migrating from Microsoft's fabric-cicd Python library to fabio's deploy commands. Shows the equivalent config mappings, parameter format translation, and additional capabilities available in fabio. |
| `fabio context best-practices lro` | Many Fabric operations are async (return 202). fabio polls them automatically. Use --wait for job operations. |

## See also
- fabio context persona migration-engineer
- fabio context workflow cicd-deploy
- fabio context best-practices deploy-parameters
