---
title: Check sources before creating semantic models
description: Bounded OneLake/Delta observations, shared with native and generated semantic-model creation.
---

Source checking is integrated with model creation through **`--check-source`**.
The standalone command and creation paths use the same observation engine and
the same fail-closed creation gate. It is initially **opt-in**: existing calls
do not silently gain more permissions, latency or source reads.

## Inspect sources without creating anything

```bash
fabio --wrap-untrusted --readonly semantic-model check-source \
  --one-lake-table "$TABLE_URL" --strict
```

Repeat `--one-lake-table` for up to 100 explicit sources. The canonical HTTPS URL
contains source workspace/item UUIDs and the actual `Tables/<table>` or
`Tables/<schema>/<table>` path. Standalone checking takes **no `--workspace` or
`--id`**; those source identities are already in the URLs.

Only selected item metadata and non-recursive `_delta_log` metadata are read.
No table rows/Parquet files are scanned, Spark jobs started, grants assigned,
refreshes submitted, or source definitions changed. Delta logs may internally
contain statistics and partition values; those values are not evaluated or
emitted. Reports expose counts, recognized protocol facts and input indexes—not
arbitrary names, properties or raw log contents.

## Gate native creation

```bash
# Offline preview: validates inputs, but does NOT run/certify source checks
fabio --wrap-untrusted semantic-model create --workspace "$WS" \
  --name "$MODEL_NAME" --one-lake-table "$TABLE_URL" --check-source --dry-run

# Checks run before the creation POST
fabio --wrap-untrusted semantic-model create --workspace "$WS" \
  --name "$MODEL_NAME" --one-lake-table "$TABLE_URL" --check-source
```

Every requested observation must be `ready` before creation. Otherwise the
command prints `status: not_created` with `sourceCheck`, exits non-zero, and sends
**no creation POST**. Global `--force` does not bypass the gate. No generator,
storage-mode switch, column conversion or permission change is attempted.

Successful checked creation retains `sourceCheck` alongside the normal model
ID. The report is evidence observed before creation, **not an atomic guarantee**
about a source that can subsequently change.

Authored `--file`/`--definition` and deliberately empty creation cannot use this
flag: arbitrary expressions are not guessed. You can inspect explicit URLs
separately, but that does not prove an authored definition uses those sources.

## Gate generated models too

```bash
fabio --wrap-untrusted semantic-model generate --workspace "$WS" \
  --name "$MODEL_NAME" --lakehouse "$LH" --storage-mode onelake --check-source
```

Both generator storage modes support the gate; preserve the explicitly chosen
mode. The generator derives the exact selected table paths from its existing
SQL schema plan. It does not silently substitute native creation or another
binding/security mode. Unsupported source columns still require review; the
preflight inspects source-table metadata, not just a projected subset of columns.

**Generator dry-run differs from native dry-run:** it retains existing SQL
schema discovery, but skips the additional OneLake probe phase, model creation
and framing. The preflight timeout does not cover that existing SQL discovery
or the creation LRO.

## Bounds apply across all sources

Defaults are a shared **30-second deadline**, **3 directory pages** (25 entries
each), **5 JSON commit files**, and **1 MiB of OneLake metadata bodies**. The
controls are:

- `--source-timeout`: at most 300 seconds, including preflight auth/in-flight reads.
- `--source-page-limit`: at most 100 total directory pages.
- `--source-log-limit`: at most 100 total JSON commit files.
- `--source-byte-limit`: at most 8 MiB of OneLake body data.

Creation limit flags require `--check-source`. `--all` and `--limit` do not
override these bounds or silently reduce the selected set. Fabric item metadata
uses the normal API response cap; the deadline covers both Fabric and OneLake.
A failed bounded body read conservatively charges the remaining byte budget and
reports `byteUsageExact: false`, rather than issuing more unaccounted reads.

## Interpret findings conservatively

- Complete directory evidence is needed before choosing the newest JSON commits.
  A contiguous suffix is inspected only until both metadata and protocol are
  observed. Gaps, partial listings, newer/checkpoint-only evidence and unsupported
  replay remain `unknown`; no Parquet checkpoint is loaded to manufacture success.
- Reader and writer requirements differ. Writer-v7 alone is not reader-v3.
  Recognized profiles cover basic reader-v1 and explicit column-mapping/vector
  declarations, with required writer features and a closed writer-only list.
  Mapping IDs/physical names must be consistent but are never emitted. Vector
  descriptors are checked structurally; paths/inline values are never emitted,
  vector files are not opened, and row masks are not decoded or applied.
  Other/future features stay unknown rather than
  being declared incompatible or ignored.
- Complex/binary source columns require review. Unrecognized types, mappings or
  configuration remain unknown; nothing is silently dropped or converted by
  preflight.
- Schema-less native sources have a known `dbo`-assumption compatibility risk.
  Standalone `--creation-mode generated` is for an **explicitly planned generator**
  path, not a way to make native creation bypass findings.
- Potential inferred table-name collisions require review. Exact duplicate URLs
  fail offline validation. Initial layout profiles cover lakehouses/warehouses;
  other Fabric items may expose valid Delta but remain unknown to this preflight.

Always review `notChecked`. Source `ready` does **not** establish loaded/nonempty
or fresh data, complete native type inference/precision, target permissions,
capacity/region alignment, SQL security, grants for other identities, model
framing, DAX readiness, or report rendering. Continue with
[BI readiness](../bi-readiness/) after creation.

References: [Direct Lake considerations](https://learn.microsoft.com/en-us/fabric/fundamentals/direct-lake-overview)
and the [Delta protocol](https://github.com/delta-io/delta/blob/master/PROTOCOL.md).
