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
- Managing deployment pipelines (dev/test/prod stages).
- Managing variable libraries and activating environment value sets.

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

## Troubleshooting
| Symptom | Fix |
|---|---|
| Plan shows a rename as delete+create | Ensure the item has a stable logicalId in its .platform file so rename detection matches it. |
| Re-running apply keeps changing the same items | The .platform part is excluded from the content hash; a real convergent deploy should show 0 changes — check for portal edits. |
| bulk strategy fails on a Git-connected workspace | Bulk import requires no Git integration; use --strategy default. |
| Connections resolve to TODO in params | Run deploy init-params --resolve-connections and fill in the correct connection IDs before apply. |

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
| `fabio context best-practices fabric-cicd-migration` | Guide for teams migrating from Microsoft's fabric-cicd Python library to fabio's deploy commands. Shows the equivalent config mappings, parameter format translation, and additional capabilities available in fabio. |
| `fabio context best-practices lro` | Many Fabric operations are async (return 202). fabio polls them automatically. Use --wait for job operations. |

## See also
- fabio context persona migration-engineer
- fabio context workflow cicd-deploy
- fabio context best-practices deploy-parameters
