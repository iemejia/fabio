---
title: Use the expanded Fabric REST capabilities
description: Work with datasource topics, schedules, Warehouse definitions, sharing items, and governance settings.
---

These commands expose the November 2026 Fabric REST additions. Preview availability,
tenant settings, capacity support, and caller permissions still determine whether
an operation succeeds. Discover exact flags with `fabio context describe <group> <command>`.

## Manage datasource topics safely

Topics are Markdown basenames, not filesystem paths. Read staging or published
topics; upload, delete, and format are staging-only. A datasource name is resolved
against the selected stage, so published reads do not depend on draft configuration.

```bash
fabio data-agent list-topics --workspace "$WS" --id "$AGENT" --datasource "$SOURCE" --stage published
fabio data-agent download-topic --workspace "$WS" --id "$AGENT" --datasource "$SOURCE" --file-name guidance.md
fabio data-agent upload-topic --workspace "$WS" --id "$AGENT" --datasource "$SOURCE" --file-name guidance.md --file ./guidance.md --dry-run
```

Downloads and successful uploads expose `etag`. For a conditional upload or deletion,
pass that value unchanged, including its double quotes, with `--if-match`.
Fabio checks a current snapshot before conditional mutations and rejects an
already-stale version. **This is not atomic compare-and-swap:** the live service
accepted a stale header during validation. Inspect `preconditionCheck` and the
returned warnings, and avoid concurrent writers; the snapshot cannot close a
read/write race on a backend that ignores `If-Match`.
Uploads can replace existing content and therefore carry destructive preview notices.
`format-topic` returns transformed Markdown with `applied: false`; it never saves
the result. Explicitly upload the result after reviewing it.

## Create and replace Warehouse definitions

Warehouse definitions support SQL projects (`sqlproj`, the default) and DACPAC
packages (`dacpac`). They describe schema, not a backup of table data.

```bash
fabio warehouse get-definition --workspace "$WS" --id "$WAREHOUSE" --format sqlproj
fabio warehouse create --workspace "$WS" --name Analytics --definition-dir ./warehouse --definition-format sqlproj --dry-run
fabio warehouse update-definition --workspace "$WS" --id "$WAREHOUSE" --file ./Analytics.dacpac --format dacpac --dry-run
fabio warehouse update-definition --workspace "$WS" --id "$WAREHOUSE" --content @warehouse-definition.json --dry-run
```

Single-file inputs are normalized to `sqldw.sqlproj` or `sqldw.dacpac` on the wire;
the local filename can differ. Live exports can instead use the warehouse's
display name as the root project/package filename. Preserve those returned paths
when round-tripping a complete definition envelope. Directory inputs preserve deterministic relative
part paths and reject symbolic links and special files. JSON inputs accept either
a definition object or a `definition` envelope and validate base64 parts before
any request. Definition inputs cannot be combined with create-time `--collation`.

Replacement is destructive. Review the preview before executing without `--dry-run`.
Add `--update-metadata` only when intentionally applying a supplied `.platform` part.
Workspace exports no longer silently substitute an empty Warehouse shell after
a failed definition fetch.

## Inspect and schedule Copy Jobs

```bash
fabio copy-job list-runs --workspace "$WS" --id "$COPY_JOB"
fabio copy-job show-run --workspace "$WS" --id "$COPY_JOB" --job-instance-id "$INSTANCE"
fabio copy-job list-schedules --workspace "$WS" --id "$COPY_JOB"
fabio copy-job create-schedule --workspace "$WS" --id "$COPY_JOB" --enabled false --config @schedule.json --dry-run
fabio copy-job update-schedule --workspace "$WS" --id "$COPY_JOB" --schedule-id "$SCHEDULE" --enabled false --dry-run
```

The configuration is a complete `Cron`, `Daily`, `Weekly`, or `Monthly` object.
Use `--enabled false` to create a disabled schedule without starting automated work.

## Sharing and Custom Stream Connector items

`custom-stream-connector`, `share`, and `share-subscription` provide workspace CRUD
and folder-filtered lists. Connectors and producer-side shares additionally provide
get/update-definition. A subscription requires a trusted HTTPS invitation URL and
has no definition endpoints.

Replacing a share definition can change assets, recipients, and policies. Verify
the intended audience first; a Share item is distinct from OneLake external data
sharing. See the [command reference](../../reference/).

## Feedback, logs, and governance

- **Data Agent feedback:** `query-feedback` and `show-feedback` are beta,
  delegated-user APIs. A continuation token must be used alone, without query
  criteria or page size. Feedback may contain PII. Attachment downloads require
  explicit `--allow-download-without-sensitivity-label` approval and refuse to
  overwrite existing files.
- **Environment logs:** `environment get-publish-log` returns the latest completed
  Spark-backed publish log with `content`, `contentDisposition`, `contentType`,
  and `truncated`. Do not assume a returned log is complete when `truncated` is true.
- **Actor auto-detection rules:** the capacity command family manages rolling
  consumption thresholds and `Hours`/`Indefinite` block durations. Deleting a rule
  does not unblock actors already blocked by it.
- **OneLake file soft-delete:** `workspace modify-file-soft-delete` supports
  retention from 1 to 365 days. Disabling retention or reducing its duration
  requires destructive-operation review. Never experiment on an existing
  production workspace.

See [destructive previews](../destructive-previews/) and
[submission retry safety](../submission-retries/) before automating mutations.
