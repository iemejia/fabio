---
title: Verify BI readiness without resubmitting work
description: Bounded model schema/data checks, exact refresh observations and report binding verification.
---

`semantic-model readiness` observes existing BI items. It does not create models,
submit/cancel refreshes, change storage modes, take ownership or rebind reports.
It works across explicitly chosen native, generated and authored model modes.

## Choose the evidence you need

```bash
# Metadata and a capped model table-schema read; no table-data probe by default
fabio --wrap-untrusted --readonly semantic-model readiness \
  --workspace "$WS" --id "$MODEL" --expected-table Sales

# Opt into data reads and require every selected table to have at least one row
fabio --wrap-untrusted --readonly semantic-model readiness \
  --workspace "$WS" --id "$MODEL" --expected-table Sales --expected-table Stores \
  --probe-data --min-rows 1 --strict

# Observe one exact refresh request and report dataset binding under a total budget
fabio --wrap-untrusted --readonly semantic-model readiness \
  --workspace "$WS" --id "$MODEL" --expected-table Sales --probe-data \
  --refresh-id "$REFRESH_ID" --report-id "$REPORT" \
  --wait --max-attempts 6 --interval 5 --timeout 60 --strict
```

`--expected-table` takes **model table names**, not source paths; repeat it up to
20 times. Model schema reads use `INFO.VIEW.TABLES`, capped at 200 tables.
Truncated, malformed, conflicting or unavailable schema evidence stays `unknown`.
An empty model is a shell, not a populated business model.

`--probe-data` requires expected tables and executes one `COUNTROWS` result
query. **This may scan data and consume capacity.** The default minimum is zero:
an empty but queryable table can pass. Use `--min-rows 1` for nonempty tables.
Counts are reported by expected-table argument index, without dumping row data
or arbitrary metadata. Success covers only those tables under the current caller.

## Correlate refreshes; never resubmit to poll

An accepted refresh is not a completed refresh. Pass the **exact request UUID**
to `--refresh-id`; readiness never picks the latest refresh or an older success.
If submission exposes no request identity, compare `refresh-status` against a
pre-submission baseline and select one unambiguous new request. Ambiguity needs
review, not another submission. A generator's legacy `framed: true` means only
that framing was submitted—not completed or verified by a fixed sleep.

The selected request is read through its detail endpoint. Missing/unknown status
or a conflicting returned request identity stays `unknown`. A failed, cancelled
or disabled selected refresh stops waiting even if schema queries succeed.
Its terminal failure is checked before schema/data queries, so an already failed
request does not trigger an unnecessary data scan. Conflicting completion
metadata (such as `Completed` with a failed extended status) stays `unknown`.

## Verify binding separately from rendering

`--report-id` compares the report's Power BI `datasetId` with the selected model.
Use `--report-workspace` when the report lives elsewhere; otherwise the model
workspace is used. A matching model UUID is binding evidence; an omitted
`datasetWorkspaceId` is explicitly recorded, not invented. No definition is
read or rewritten. Binding does **not** prove pages/visuals exist, render, or are
accessible to viewers.

## Bounds and interpretation

- One snapshot is the default. `--wait` repeats only observations, with at most
  six cycles by default (maximum 20).
- `--timeout` is a **total budget**, including every in-flight read/query, auth
  retry and polling sleep—not a per-call timeout. It defaults to 30 seconds and
  cannot exceed 300 seconds. Polling attempts provide a second bound.
- `--dry-run` previews the validated scope offline. `--strict` still prints the
  report, then exits non-zero unless all requested observations are `ready`.
- Always inspect `checks` and `notChecked`. Default `ready` does not establish
  an unrequested data read, refresh completion, or report binding.

Source Delta protocol/schema compatibility, data freshness, source grants for
other identities, exact storage/fallback behavior and report rendering remain
explicitly outside this check. A successful DAX query is not proof of any of them.
Do not silently switch native/generated/authored modes to repair a failed check.

### Read-only DAX transport

DAX queries and `INFO.VIEW` introspection now work under `--readonly`. The
semantically read-only POST transport constructs only the Power BI
`executeQueries` route; refresh, create, takeover and definition updates keep
their mutation guards. Error-bearing or malformed HTTP-200 DAX responses fail
rather than becoming an empty successful list.

For complementary checks, see [connection diagnostics](../connection-diagnostics/)
and [workspace diagnostics](../workspace-diagnostics/). Runtime schema and source
preflight are distinct: valid OneLake URL syntax alone never establishes a
compatible or authorized Delta source.
