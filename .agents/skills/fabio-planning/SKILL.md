---
name: fabio-planning
description: >-
  Intent-scoped fabio skill for Fabric plan items: create, inspect, and update plan items and their definitions. Use to manage connected-planning/FP&A style plan artifacts in a workspace. Triggers: "plan", "connected planning", "create plan", "plan definition", "fp&a".
license: MIT
---

# fabio-planning — Planning — Fabric plans (connected planning)

> **Generated file — do not edit by hand.** This intent-scoped sub-skill of the `fabio` skill is generated from fabio's command schema plus authored judgment. Regenerate with `cargo test generate_subskills -- --ignored`. For install, auth, output envelope, global flags, and agent-safety rules, see the root `fabio` skill.

> **Prefer runtime introspection.** This index is a snapshot; the installed binary is always authoritative. Use `fabio context agent --group <group>` and `fabio context describe <group> <command>` for exact flags and output shapes.

## When to use
- Creating/listing/inspecting plan items in a workspace.
- Editing a plan's definition (get-definition / update-definition).
- Scoping a plan listing to a specific folder via --root-folder-id / --no-recursive.

## When NOT to use (route elsewhere)
- Data pipelines/orchestration -> use fabio-data-engineering.
- Reports/dashboards over plan data -> use fabio-bi.
- Deploying a plan across environments -> use fabio-deploy-cicd (deploy export/plan/apply).

## Command index

Generated from fabio's command schema. For full flag details use `fabio context agent --group <group>` or `fabio context describe <group> <command>`.

### fabio plan
Manage plans (connected planning)

| Command | Mutates | Description |
|---|---|---|
| `fabio plan create` | yes | Create a new plan |
| `fabio plan delete` | yes | Delete a plan |
| `fabio plan get-definition` | no | Get the definition of a plan |
| `fabio plan list` | no | List plans in a workspace |
| `fabio plan show` | no | Show details of a plan |
| `fabio plan update` | yes | Update plan properties |
| `fabio plan update-definition` | yes | Update the definition of a plan |

## Must / Prefer / Avoid
### MUST
- Edit plan content via the definition part (get-definition / update-definition), not plain metadata flags.
- Use --name/--description on plan update for metadata-only changes (PATCH is fully optional — provide at least one field).

### PREFER
- Runtime introspection (context agent --group plan, context describe plan create) for exact flags and the definition shape.
- Creating an empty plan first (plan create makes an empty plan — there is no --definition flag on create), then populating its content via update-definition.

### AVOID
- Passing --hard-delete on plan delete — the API has no such parameter; plan delete is a simple, non-recoverable delete.
- Assuming plan list returns all folders by default without checking --no-recursive/--root-folder-id semantics.

## Key gotchas
- Plan definitions use the PlanV1 format with a canonical part path of connectedPlanning/infobridge.json; get-definition/update-definition round-trip this part.
- plan list supports --root-folder-id (scope to a folder; defaults to workspace root) and --no-recursive (list only direct children, not nested folders); the API default for recursion is true.
- plan delete has no --hard-delete flag — the Plan REST API defines no such query parameter, unlike some other item types.

## Troubleshooting
| Symptom | Fix |
|---|---|
| plan create fails with ItemDisplayNameAlreadyInUse | Choose a different --name, or fabio plan list --workspace <WS> to find the existing plan and update/delete it instead. |
| plan update-definition returns an empty body | This is expected — Fabric's updateDefinition endpoint often returns no content on success; check the command's status field. |

## Safety
- plan delete is destructive (dry-run guarded) and irreversible; no --hard-delete flag exists for this item type.
- plan update-definition irreversibly replaces the plan's entire definition (LRO-polled, dry-run guarded); there is no backup — export the current definition with get-definition first if you need to preserve it.

## Shared references
Cross-cutting operational guidance (the "common" layer) — consult the relevant topic before non-trivial work:

| Reference | Covers |
|---|---|
| `fabio context best-practices lro` | Many Fabric operations are async (return 202). fabio polls them automatically. Use --wait for job operations. |
| `fabio context best-practices pagination` | fabio handles pagination via --all (auto-fetch all pages), --continuation-token (resume), and --limit (truncate). Agents rarely need to paginate manually. |

## See also
- fabio context persona data-engineer
- fabio context persona fabric-admin
