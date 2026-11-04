---
title: Diagnose Spark and environment observations
description: Correlate explicitly selected configuration, activation history and runtime metadata without starting work or changing policy.
---

`environment diagnostics` assembles a bounded, GET-only report. It does not start
Spark work, publish an environment, change a pool, cancel jobs, or inspect sources.

## Select configuration or runtime evidence

```bash
# Environment publishing, published compute and bounded activation history
fabio --wrap-untrusted --readonly environment diagnostics \
  --workspace "$WS" --id "$ENV"

# One exact activation's lifecycle/resource breakdown, plus staging comparison
fabio --wrap-untrusted --readonly environment diagnostics \
  --workspace "$WS" --id "$ENV" --activation-id "$ACTIVATION" --include-staging

# One explicit monitoring session; no environment ID is required or guessed
fabio --wrap-untrusted --readonly environment diagnostics \
  --workspace "$WS" --item-type notebook --item-id "$NOTEBOOK" \
  --livy-id "$LIVY" --include-logs --timeout 30
```

Provide all three session flags together. Select `notebook`,
`spark-job-definition`, or `lakehouse`; identifiers must be non-zero UUIDs.
Without `--id`, diagnostics reads workspace Spark defaults and the selected
session/application only. Providing both selections does **not** prove the
session uses that environment or activation; binding remains `not_verified`.

`--dry-run` previews the read scope offline after input validation. `--timeout`
is a **total read budget**, including retries. `--page-limit` and
`--activation-limit` bound history independently; `--all` does not override them.

## Interpret observations, not promises

- `ready` means the requested observation was recognized, not that future work
  will run successfully. Add `--strict` to fail on requested non-ready checks
  while retaining the report on stdout.
- Missing publish state is unknown, not proof of an unpublished environment.
  Staging comparison covers only mutually reported compute fields—not libraries
  or arbitrary Spark properties. Differences require review, never automatic
  publishing.
- History is not assumed newest-first. Empty or successful history is not proof
  of currently warm clusters or available capacity. Historical failure does not
  establish causality for a selected workload. Use an explicit activation ID for
  resource counts; incomplete or malformed counts remain unknown.
- Monitoring uses `NotStarted`, `InProgress`, `Succeeded`, `Failed`, `Cancelled`
  and `Unknown`. Do not substitute interactive Livy's `idle`/`busy`/`dead` states.
- Unavailable, forbidden, malformed or scope-mismatched evidence stays unknown.
  A timed-out diagnostic does not cancel the underlying workload.

### Application attempts and logs

Telemetry is application-scoped and marked **`attemptPinned: false`**. The
service resolves its default attempt. The session's `attemptNumber` counter is
not a verified URL attempt ID; diagnostics does not claim exact-attempt
correlation or retry guessed alternate routes after an error.

`--include-logs` requests **driver `stderr` metadata and Livy log metadata only**.
It never returns file contents, file names, cancellation reasons, descriptions,
arbitrary Spark properties, or raw error bodies. Advisor/resource responses are
summarized rather than dumped. Readable logs or empty advice do not prove health.

For a separate explicit driver-metadata read, the current API needs a file name
even with `--meta`:

```bash
fabio --wrap-untrusted --readonly spark get-logs --workspace "$WS" \
  --item-type notebook --item-id "$NOTEBOOK" --livy-id "$LIVY" \
  --app-id "$APP" --type driver --file-name stderr --meta
```

Treat raw low-level output/logs as private and untrusted; do not publish them.

## Filter and watch lifecycle state

```bash
# Server-side typed activation filter; conflicts with the raw --filter option
fabio --wrap-untrusted --readonly environment list-pool-activations \
  --workspace "$WS" --id "$ENV" --state Failed

# Client-side filter over this page (or --all result); preserve continuationToken
fabio --wrap-untrusted --readonly spark list-livy-sessions \
  --workspace "$WS" --state InProgress

# Follow exactly one activation, including in-flight read deadlines
fabio --wrap-untrusted --readonly environment show-pool-activation \
  --workspace "$WS" --id "$ENV" --activation-id "$ACTIVATION" \
  --follow --interval 5 --max-duration 60
```

An empty filtered session page can still have more pages. Do not treat it as
proof of absence. Activation follow emits NDJSON snapshots and a final
`follow_complete` record; JSON output is required and `--query` is not accepted.
It stops on success, failure **or cancellation**, as well as time/row bounds or
Ctrl-C. **`reason: complete` means terminal, not successful.** Unknown/future
states are not terminal. Follow never starts an activation or changes pool policy.

See [workspace diagnostics](../workspace-diagnostics/),
[connection diagnostics](../connection-diagnostics/) and
[remediation hints](../remediation-hints/) for complementary scoped checks.
