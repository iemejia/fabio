# Fabric REST API sync: live acceptance plan

## Scope and safeguards

Validate the capabilities consolidated from PRs #191, #192, and #200 against
the live Fabric service. Offline/mock tests are necessary but are not evidence
that a tenant feature, permission, capacity SKU, or service backend is available.

- Discover the current identity's accessible resources; do not assume old
  `FABIO_TEST_*` fixture bindings are still usable.
- Keep resource IDs/names, account details, invitation URLs, raw service logs,
  and credentials in private local state outside the repository. Publish only
  check names, outcomes, and non-identifying error classifications.
- Use a newly created workspace on an accessible paid capacity and synthetic
  data only. Do not alter existing workspace contents or security policies.
- Keep schedules disabled; do not execute background copy schedules.
- Exercise retention changes only in the isolated workspace.
- Actor auto-detection rules target **all workspaces on their capacity**. Only
  mutate them on a separate, empty, session-owned test capacity. A shared
  capacity's successful read probe is not permission to change its rules.
- Do not enable tenant settings, consent new application permissions, grant
  directory roles, or distribute an agent tenant-wide to make a test pass.
- Never replay an accepted/unknown mutation automatically. Reconcile the
  original scoped operation before continuing or cleaning it up.
- Delete session-owned artifacts/workspaces and any newly provisioned capacity;
  preserve unrelated resources. Verify cleanup instead of assuming it succeeded.

## Execution stages

1. **Preflight:** verify credential source, accessible workspace/capacity inventory,
   private fixture visibility, capacity assignment, and relevant read permissions.
2. **Isolated fixtures:** create a workspace, attach a paid capacity, create a tiny
   Warehouse/Lakehouse source and a Data Agent, and record exact identities privately.
3. **Workspace-scoped lifecycles:** run the matrix below, inspect persisted state,
   and distinguish schema/configuration readiness from actual data readiness.
4. **Capacity governance:** provision a small dedicated capacity if needed and
   authorized by available subscription permissions/quota; test only session-owned
   rules and remove them before deleting that capacity.
5. **Regression closure:** fix demonstrated CLI defects with unit/mock regressions,
   then repeat the exact live scenario. Service rollout or fixture limitations stay
   explicitly blocked rather than becoming inferred passes.
6. **Cleanup and report:** verify absence of every owned resource, run local quality
   gates for code changes, and update the sanitized API behavior record.

## Acceptance matrix

| Area | Live checks |
|---|---|
| Warehouse definitions | Create/get/update SQL project and DACPAC definitions; inspect actual returned part paths and resulting schema; verify export does not hide a failed fetch. |
| Copy Job schedules | Create disabled schedule, list/show/update it without enabling execution, then delete it. |
| Data Agent topics | Resolve a synthetic datasource; upload/list/download; observe ETags; test service enforcement separately from client stale-snapshot rejection; format without saving; delete; check published-stage reads after publishing. |
| Data Agent feedback | Search/list, continuation and show when records exist. Download attachments only from an explicitly synthetic record with the label-loss acknowledgment. No record means not exercised, not passed. |
| Environment publish logs | Complete a bounded Spark-backed publish using a harmless synthetic library; retrieve log content and metadata. No completed Spark publish means no valid log fixture. |
| Custom Stream Connector | Create/show/update/list; get and round-trip a valid public definition; delete. Stored-definition success does not prove a running connector. |
| Share | Use synthetic assets only; create/show/update/list; inspect and round-trip its definition; delete. Do not grant unrelated recipients access. |
| Share Subscription | Consume only a session-owned invitation if the public API supports the available tenant topology; show/update/list/delete. Never substitute an external recipient or tenant. |
| OneLake file soft-delete | Enable/read/change/disable on the isolated workspace only; verify service responses and persisted settings. |
| Actor auto-detection | Read capabilities; create/list/show/update/delete a rule on the dedicated empty capacity only; verify the capacity and rules are removed afterward. |

## Reporting

Use `passed`, `failed`, `blocked`, and `not_exercised` distinctly. Preserve the
private request/response evidence for debugging, but record only sanitized findings
in `.agents/API-BEHAVIORS-DISCOVERED.md`. Full acceptance requires the relevant
happy paths; a deliberate error response or dry-run is not a successful lifecycle.
