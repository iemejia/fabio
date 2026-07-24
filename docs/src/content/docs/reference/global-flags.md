---
title: Global flags
description: Output, projection, pagination, safety, access-control, and diagnostic options shared by Fabio commands.
---

Global flags apply across Fabio's command surface. They can appear before or after the command group.

## Output and projection

| Flag | Purpose |
| --- | --- |
| `--output`, `-o` `<format>` | Select the output format: `json\|table\|plain\|csv\|tsv`. Defaults to `json`. Also set with `FABIO_OUTPUT`. |
| `--json` | Shorthand for `--output json`. |
| `--query`, `-q` `<expression>` | Project output with a JMESPath expression. |
| `--quiet` | Suppress successful stdout while preserving errors on stderr. |

## Pagination

| Flag | Purpose |
| --- | --- |
| `--limit <number>` | Bound client-side list results. |
| `--all` | Fetch every page (auto-paginate). Without it, only the first page is returned. |
| `--continuation-token <token>` | Resume a paginated list from a token returned by a previous call. |

## Safety and access control

| Flag | Purpose |
| --- | --- |
| `--dry-run` | Preview supported mutations without sending them. |
| `--force` | Skip confirmation on destructive operations. Safety-bypass — agents should not add it without explicit human approval. |
| `--readonly` | Block all mutating operations (POST/PUT/PATCH/DELETE) before network dispatch; read-only calls are unaffected. Also set with `FABIO_READONLY`. |
| `--wrap-untrusted` | Wrap API-returned free-text fields (`displayName`, `description`, `message`) with untrusted-content markers to prevent prompt injection. Also set with `FABIO_WRAP_UNTRUSTED`. |
| `--enable-commands <paths>` | Allow only these comma-separated command paths; unlisted commands are blocked. Also set with `FABIO_ENABLE_COMMANDS`. |
| `--disable-commands <paths>` | Block these comma-separated command paths; deny overrides allow. Also set with `FABIO_DISABLE_COMMANDS`. |

## Identity and diagnostics

| Flag | Purpose |
| --- | --- |
| `--profile <name>` | Apply saved defaults from a named profile. |
| `--verbose`, `-v` | Enable verbose HTTP diagnostics on stderr (request/response tracing). |
| `--lro-timeout <seconds>` | Maximum seconds to wait for long-running operations (default: 120). |

Run `fabio --help` for the flags supported by your installed version.
