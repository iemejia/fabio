---
title: Use remediation hints safely
description: Interpret proposed fixes in existing error hints and diagnostics without automatic policy changes.
---

Fabio suggests recovery through its existing `hint`, `hintType`, and
`verifyAfter` error fields, and through diagnostic `nextSteps`. Shared builders
keep those recommendations consistent. They do not execute commands or make
additional tenant probes.

## Inspect, review, or preview a fix

| `nextSteps.kind` | Meaning |
| --- | --- |
| `inspection` | Read scoped evidence. |
| `review` | A human decision or more information is needed; there is no command. |
| `remediation` | A proposed change: a guarded preview or an explicitly incomplete template. |

Concrete remediation commands include **both `--readonly` and `--dry-run`**.
They carry `hintType: semantic_correction`, `requiresApproval: true`, the command's
`destructive` annotation, and a read-only `verifyAfter` where available.
Approval is required to execute the actual change, not to inspect its preview.
Do not automatically remove either guard. Destructive previews also emit Fabio's
existing confirmation notice for detected coding agents.

For a confirmed unexpired block, a preview has this form:

```bash
fabio --wrap-untrusted --readonly capacity delete-blocked-actor \
  --id "$CAPACITY_ID" --actor-id "$ACTOR_ID" --dry-run
```

Diagnostics only generates that concrete preview from complete actor scans with
known identifiers and no conflicting mission-critical record. Expired,
unrecognized, partial, or forbidden evidence is not a reason to remove policy.
An observed record also does **not** prove it caused a particular workload
failure. Review scope before approval and verify the specific condition afterward.

## Do not invent missing inputs

When inputs are missing, `command` is absent. The hint provides `missingInputs`
and a **`commandTemplate`**, not a runnable solution:

- An unassigned workspace needs a user-selected `targetCapacityId` before
  assignment can be previewed.
- Inactive F-series capacity recovery needs the Azure subscription, resource
  group, and capacity name, plus confirmation that it is a resumable ARM resource.
  A Fabric capacity UUID is not a valid `--id` argument to `capacity resume`.
- Unknown or non-F capacity types receive review guidance rather than an invented
  resume command. Failed or in-progress assignments are not silently redirected
  to a new target.

## Ordinary command errors

Capacity-specific failures on supported run commands and semantic-model
refresh/query can suggest scoped `workspace diagnostics`. This adds guidance,
not API calls: no automatic actor lookup, grant, retry, or policy change occurs.
Generic throttling or permission errors do not prove a workspace block exists.

Strict diagnostics put the same confirmed preview or missing-input template into
`error.hint`, with `semantic_correction` and verification guidance. API codes,
retriability, correlation IDs, details, parameters, and existing verification are
preserved. Safety-bypass hints and tenant-feature-gate recovery remain protected.

## Verify the intended condition

Use `verifyAfter` after an approved change. Check the matching workspace/capacity
and the relevant condition—not universal workload readiness. An unblocked record
does not eliminate capacity-level surge thresholds, source permissions, or other
blockers. Verify the workload separately before declaring the original problem
resolved.

```bash
fabio --wrap-untrusted context best-practices remediation-hints
fabio --wrap-untrusted --readonly workspace diagnostics --id "$WS" \
  --include-surge-protection \
  --query "checks[].nextSteps[] | [?kind=='remediation']"
```
