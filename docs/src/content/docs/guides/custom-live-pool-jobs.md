---
title: Observe a custom live-pool job safely
description: Submit hydration once, retain its receipt, and bound waiting or cancellation without confusing job completion with warm capacity.
---

A custom live pool requires an explicitly selected custom Spark pool and a
published Environment with live-pool support enabled. Starter pools are not
supported. Inspect the selected configuration and review capacity consumption
before running compute; never change an existing environment or enable a tenant
setting as an automatic repair.

## Keep schedule fixtures disabled

For schedule authoring, put the complete reviewed request in `schedule.json` and
keep `enabled` false. Choose future dates and the intended time zone before use:

```json
{
  "enabled": false,
  "configuration": {
    "type": "Cron",
    "startDateTime": "2027-01-01T00:00:00Z",
    "endDateTime": "2027-01-02T00:00:00Z",
    "localTimeZoneId": "UTC",
    "interval": 1440
  }
}
```

```bash
fabio --wrap-untrusted environment create-custom-live-pool-schedule --workspace "$WS" --id "$ENVIRONMENT" --file schedule.json --dry-run
```

The file path preserves the full configuration rather than inventing a subset
of typed schedule flags. Schedule creation alone does not prove that hydration
ran; enabling a schedule is a separately reviewed compute change.

## Submit once and keep the receipt

```bash
fabio --wrap-untrusted environment run-custom-live-pool --workspace "$WS" --id "$ENVIRONMENT" --dry-run
fabio --wrap-untrusted environment run-custom-live-pool --workspace "$WS" --id "$ENVIRONMENT"
```

The legacy `jobInstanceId` remains available. The response also includes the
exact `jobReceipt`, `receiptPersisted`, and `retryAllowed: false`. Preserve the
receipt privately, particularly when local bookkeeping fails. Do not replay a
submission to recover its ID or create another job after an uncertain error.

## Bound observation, not the pool's server-side lifetime

```bash
fabio --wrap-untrusted environment run-custom-live-pool --workspace "$WS" --id "$ENVIRONMENT" --wait --timeout 300 --dry-run
fabio --wrap-untrusted environment run-custom-live-pool --workspace "$WS" --id "$ENVIRONMENT" --wait --timeout 300
```

These are **new submissions**, not commands for waiting on a previously started
job. `--wait` observes only the returned job. `--timeout` accepts 1–3,600 seconds
(default 300), covering observation requests and polling delays after acceptance.
A timeout leaves server-side work running unless cancellation was explicitly
requested. Inspect the preserved receipt with the existing job API:

```bash
fabio --wrap-untrusted job-scheduler get-instance --workspace "$WS" --id "$ENVIRONMENT" --job-instance-id "$JOB" --readonly
```

The live-pool job can run for its configured lifespan. Fabric enforces at least
`PT20M` for cluster idle timeout and `PT30M` for pool lifespan. Finishing the
lifecycle is **not** a warm-capacity readiness check, and a short local deadline
does not imply a failed hydration. Inspect explicitly correlated pool activation
details before claiming that compute is available; historical success is not
proof that clusters are warm now.

## Request cancellation only with approval

```bash
fabio --wrap-untrusted environment run-custom-live-pool --workspace "$WS" --id "$ENVIRONMENT" --wait --timeout 300 --cancel-on-timeout --dry-run
```

This preview is marked destructive because it permits cancellation. Confirm the
scope and risk before removing `--dry-run`; `--cancel-on-timeout` requires
`--wait`. Only the accepted job is targeted. `cancelRequested: true` and receipt
status `cancel_requested` are acknowledgements, **not** proof of a terminal
cancelled state or released capacity. Read that exact job afterward; never retry
hydration because observation or cancellation failed.

## Configuration contract boundary

The release Spark compute API explicitly uses `beta=false`, a `sparkProperties`
array, and live-pool fields. The older `beta=true` contract uses dictionary Spark
properties and does not define those fields. Preserve the chosen contract and
request shape; do not silently fall back between them. The current typed compute
configuration command retains its beta contract, so prepare this prerequisite
through the reviewed release API or the Fabric UI. This job runner does not
configure, publish, convert, or enable environments automatically.

Discover the recipe with `fabio context workflow custom-live-pool-job` and the
response contract with `fabio context examples environment run-custom-live-pool`.
