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
| `--query`, `-q` `<expression>` | Project output with a JMESPath expression (like Azure CLI's `--query`, **not jq**). It runs on the value under `data`, so write `[].name` — not `.data[].name` (jq) or `data[].name` (envelope). A jq-shaped or malformed query fails fast with `INVALID_INPUT` and a corrected suggestion. |
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

## Update notifications

When an AI agent is detected as the caller, a successful JSON response may carry an additive `updateAvailable` object announcing a newer released version of Fabio, the detected install method, and the matching upgrade command:

```json
{
  "data": { "...": "..." },
  "updateAvailable": {
    "current": "0.60.0",
    "latest": "0.63.0",
    "installMethod": "cargo",
    "upgradeCommand": "cargo install --git https://github.com/iemejia/fabio.git --force",
    "agentNotice": "Note for AI agents (...): a newer fabio (0.63.0) is available. ... re-run `fabio context agent` to refresh the command schema ..."
  }
}
```

The check is passive and cheap: it reads a locally cached result (`~/.fabio/version-check.json`) and, at most once every 24 hours, refreshes that cache in a detached background process — it never performs a network request on the command's own path, and never blocks or fails the command. The refresh interval is enforced even when a refresh fails (offline or GitHub rate-limited), so fabio makes at most one release-API request per day, not one per command. The field is additive (present only when an update exists), so it does not affect scripts or `--query` projections.

| Environment variable | Purpose |
| --- | --- |
| `FABIO_NO_VERSION_CHECK` | Set to any value to disable the update check entirely (no cache read, no background refresh, no `updateAvailable` field). |
| `FABIO_NO_BACKGROUND_REFRESH` | Set to any value to keep the passive cached notice but never spawn the background refresher (air-gapped environments). |
| `FABIO_AUTO_UPGRADE` | Set to a truthy value to self-update: when the cached check finds a newer release, spawn a detached `fabio upgrade` so the new binary takes effect on the next invocation (the notice then carries `"autoUpgrade": "launched"`). Off by default; standalone installs only; throttled to one attempt per hour; disabled by the two variables above. |

Run `fabio --help` for the flags supported by your installed version.
