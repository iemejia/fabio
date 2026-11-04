---
title: Diagnose workspace and capacity readiness
description: Inspect bounded read-only configuration evidence before retrying workloads or changing policy.
---

Start with one workspace:

```bash
fabio --wrap-untrusted --readonly workspace diagnostics --id "$WS"
```

The default report checks capacity assignment and the assigned capacity's state
using at most two logical reads. It does not scan tenant inventory or inspect
connections, credentials, source access, or Spark environments.

Capacity metadata requires administrator or contributor permissions on the
capacity. Workspace access alone may be insufficient; a forbidden capacity read
is reported as `unknown`, not as inactive or ready.

To include capacity-admin surge-protection evidence, explicitly opt in:

```bash
fabio --wrap-untrusted --readonly workspace diagnostics --id "$WS" \
  --include-surge-protection --actor-page-limit 3 --timeout 30
```

This adds configuration and workspace-specific blocked/mission-critical actor
evidence. Only matching workspace records are returned. Capacity-admin access may
be required even when you can read the workspace itself.

## Interpret the report

| Status | Meaning |
| --- | --- |
| `ready` | No blocker or unknown was found in the **requested configuration checks**. |
| `blocked` | An inactive capacity or an unexpired workspace block was observed. |
| `needs_action` | Assignment is absent, in progress, or failed; review or recheck before dependent work. |
| `unknown` | Evidence is unavailable, incomplete, unrecognized, malformed, or conflicting. |

Known blockers take precedence over `needs_action`, then `unknown`, then `ready`.
`partial: true` independently identifies unknown checks even when another check
establishes a blocker. Every check has a stable `checkId`, evidence, and typed
`nextSteps`. Machine identifiers stay usable with `--wrap-untrusted`; human text
remains wrapped as untrusted data.

**Ready is not an end-to-end workload guarantee.** Read `scope`, `notChecked`, and
`limitations`. Live headroom, SKU/license compatibility, source permissions,
connection credentials, identity authorization, and Spark readiness are not
inferred from this configuration report. Reads are a snapshot, not an atomic
transaction across all resources.

## Important surge-protection distinctions

- Configured `Enabled` thresholds do **not** reveal whether rejection is currently
  active. This check stays `unknown` and asks for live telemetry.
- Expired blocks can remain listed until background cleanup. Their presence does
  not mean enforcement is still active; diagnostics evaluates the UTC expiration.
- Mission-critical classification does not reserve compute or guarantee immunity
  from general capacity throttling. Conflicting active-block/mission-critical
  records are reported as `unknown`, not resolved by guessing precedence.
- Permission errors, malformed pages, and truncated scans are never treated as
  an empty actor list. The report retains error codes and available request IDs
  without copying raw error bodies or mutation workarounds.

## Bounded reads, no remediation

`--actor-page-limit` accepts 1–20 pages per collection (default 3). Each collection
inspects at most 1,000 actors and returns at most 10 matching rows. The default
surge-enabled budget is nine logical reads. Global `--all` cannot bypass the
budgets, and `--limit` cannot truncate a scan into a false absence proof.

`--timeout` accepts 1–300 seconds (default 30) per probe or entire actor scan. The
workspace read precedes parallel capacity probes, so the read phases take roughly
at most twice that value, plus local/startup processing. Client authentication or
transient retries may add GET attempts inside those deadlines.

Pagination uses encoded tokens on the same scoped route. Cycles, unsupported
URI-only continuations, limits, and read failures produce incomplete evidence.
Use the corresponding capacity list command to inspect a particular list; a
global continuation token cannot resume this multi-source report.

All diagnostic requests are GET. No capacity is resumed, block deleted, role
granted, or policy changed. `nextSteps.kind` distinguishes inspection, review,
and remediation. See [remediation hints](../remediation-hints/) for guarded fix
previews and missing-input templates. A proposed fix is never executed by the
diagnostic, and review messages are not shell instructions.

Connection configuration and identity metadata are a separate scoped diagnostic:
see [connection diagnostics](../connection-diagnostics/). It distinguishes caller
roles from untested stored credentials and workspace-identity source grants.
For runtime/configuration observations, see [Spark diagnostics](../spark-diagnostics/);
workspace/capacity readiness alone is not Spark readiness.

## Automation and discovery

Without `--strict`, producing a diagnostic report exits zero regardless of its
status. Add `--strict` to print the same report but exit non-zero unless every
requested check is `ready`. Input errors still fail before any request.

```bash
fabio --wrap-untrusted --readonly workspace diagnostics --id "$WS" --strict
fabio --wrap-untrusted workspace diagnostics --id "$WS" --dry-run
fabio --wrap-untrusted context examples workspace diagnostics
fabio --wrap-untrusted context best-practices workspace-diagnostics
```

Project unknown checks with JMESPath on the payload (no `data.` prefix):

```bash
fabio --wrap-untrusted --readonly workspace diagnostics --id "$WS" \
  --include-surge-protection \
  --query "checks[?status=='unknown'].{check:checkId,evidence:evidence}"
```
