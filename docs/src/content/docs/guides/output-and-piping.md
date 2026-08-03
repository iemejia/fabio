---
title: Work with output and pipes
description: Select Fabio output formats, project JSON, and compose commands safely.
---

Fabio writes successful data to stdout and diagnostics to stderr.

## Choose an output format

JSON is the default:

```bash
fabio workspace list
```

For a person:

```bash
fabio workspace list --output table
```

For shell pipelines:

```bash
fabio workspace list --output plain --query "[].id"
```

## Project with JMESPath

`--query` supports full JMESPath expressions. Like Azure CLI, it runs against the
**raw payload** — the value under `data` — so you do **not** prefix expressions with
`data.`. Query a list command's array directly (`[].id`, `[?…]`, `[0].id`) and an
object command's fields directly (`id`, `properties.status`). Count a list with the
JMESPath idiom `length([])`.

```bash
fabio workspace list --query "[?contains(displayName, 'Prod')].{id:id,name:displayName}"
```

(By contrast, `jq` operates on the whole envelope, so there you write `.data[].id`.)

Project early to reduce output size and agent token use.

## Bound results

Use `--limit` for predictable response sizes:

```bash
fabio item list --workspace <workspace-id> --limit 25
```

Use `--all` only when every page is required. A continuation token can resume pagination without repeating earlier requests.
