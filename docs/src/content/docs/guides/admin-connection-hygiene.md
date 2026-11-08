---
title: Review connections with bounded admin evidence
description: Historical recency, ownership and target-similarity review without automatic cleanup or grants.
---

`admin connection-hygiene` is **GET-only**. It produces scoped observations and
review previews—not deletion eligibility, an ownership executor, or a credential
test. Use explicitly authorized Fabric-admin or supported service-principal
credentials; no privilege elevation or ordinary-connection fallback occurs.

Delegated admin reads also require `Tenant.Read.All` or `Tenant.ReadWrite.All`.
A Fabric-audience token or directory-role activation alone is insufficient. If
the read reports `AUTH_REQUIRED`, use an already-authorized context approved by
the user; do not obtain consent or switch identities automatically.

## Choose the scope explicitly

```bash
# Specific connections; no tenant inventory request
fabio --wrap-untrusted --readonly admin connection-hygiene \
  --id "$CONNECTION_ID" --timeout 30

# Deliberately opt into a bounded admin inventory sample
fabio --wrap-untrusted --readonly admin connection-hygiene \
  --inventory --connection-limit 10 --page-limit 3 --role-limit 20 --timeout 30

# Offline read-scope preview (no metadata requests)
fabio --wrap-untrusted --readonly admin connection-hygiene \
  --id "$CONNECTION_ID" --dry-run
```

Repeat `--id` within `--connection-limit` (default 10, maximum 20). Do not combine
IDs with `--inventory`; explicit selections are never silently truncated.
There is no `--workspace` parameter. Aggregate review cannot resume one cursor;
use `admin list-connections` for inventory page resumption.

`--page-limit` applies per inventory/role list (default 3, maximum 20), and
`--role-limit` per connection (default 20, maximum 100). `--timeout` is a **total
budget** for all reads/auth/retries (default 30, maximum 300 seconds). `--all` and
`--force` do not expand scope, bypass limits or apply anything. Returned
continuation URLs are never followed; opaque tokens stay on the same admin route.

## Interpret signals, not guarantees

- `--unused-days` defaults to 90 (maximum 36,500). Old use is historical review
  evidence, not proof of current non-use or authorization to delete.
- `--created-after` cannot precede the reliable `2026-05-01` boundary or lie in
  the future. Older/null recency can be misleading. Missing, malformed, future
  or conflicting events remain **unknown**, not never-bound/never-used proof.
- Explicit null events on a post-cutoff connection mean no event was reported.
  They do not establish all current dependencies; recency can lag and legitimate
  seasonal connections may remain idle for months.
- Only complete recognized role projections establish reported owner counts.
  Partial/error-bearing/wrong-scope projections, unknown roles/principal types
  and conflicting assignments remain unknown. **PendingOwner is not effective
  ownership** and suppresses retirement/new-owner previews.
- A single individual owner is continuity risk—not proof the user departed,
  the connection is orphaned, or stored credentials failed. Principal existence,
  group membership, credential health and source grants are not tested.
- Similar target metadata is not equivalent identity, credentials, security
  settings, permissions or bindings. No winner/`keepId` is chosen and nothing
  is rebound. Names, paths, emails, secret properties and raw API bodies are not
  emitted.

`--strict` still prints the report but fails for review findings or unknown
evidence. Always inspect individual checks, `partial` and `notChecked`, not only
the summary status. Admin authority and regular `myRole` are distinct: absent
caller role does not mean an admin metadata request was unauthorized.

## Review plans never apply changes

The report always has `reviewPlan.applied: false` and
`reviewPlan.readyToApply: false`. Proposed actions carry approval requirements
and blockers. The `destructive` field marks potential irreversible retirement
**previews**, not mutation by this command.

A retirement preview is offered only with historical review evidence and
complete non-pending ownership—not proof of unused connections. All current
dependencies, owner consent and human approval still require review.

A continuity preview is a **template** with a missing supported owner identity,
offered only for known shareable types. Admin grants support `User` and
`ServicePrincipal`, **not `Group`**. The service-principal example does not select
an actual principal or change the credential mode. An admin Owner
request may initially be `PendingOwner` until credential handoff; the template
does not authorize updating credentials or transferring ownership automatically.

Concrete previews/templates retain **both `--readonly` and `--dry-run`**, plus
`requiresApproval`, destructive metadata and read-only verification. Do not strip
the guards or apply the proposal merely because recency, a preview, or an admin
role is available. Use existing mutation commands only as separate, explicitly
reviewed operations.

For caller-visible legacy helpers, see `connection find-stale`,
`find-duplicates`, and `find-single-owner`. They are candidate heuristics, not
bounded admin coverage or credential equivalence. For explicit connection/item
triage, see [connection diagnostics](../connection-diagnostics/).
