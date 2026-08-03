---
title: Use Fabio with coding agents
description: Install Fabio skills and expose machine-readable Microsoft Fabric context to agents.
---

Fabio is designed for agents that must plan, execute, verify, and recover without interactive prompts.

## Install the skills

```bash
npx skills add https://github.com/iemejia/fabio
```

The root skill covers authentication, global flags, output envelopes, and safety. Focused skills cover lakehouse, data engineering, BI, real-time intelligence, CI/CD, administration, and other workloads without filling the agent context with unrelated commands.

## Discover commands programmatically

Return the complete machine-readable command schema:

```bash
fabio context agent
```

Limit discovery to one group:

```bash
fabio context agent --group lakehouse
```

Find relevant authored guidance:

```bash
fabio context find "deploy a notebook between environments"
```

## Give agents tenant context

Start with a bounded inventory:

```bash
fabio context tenant --summary-only
```

Resolve a display name without constructing a full graph:

```bash
fabio context tenant --resolve "Lakehouse:Sales"
```

## Preserve safety boundaries

- Keep JSON output enabled so the agent can inspect `data`, `count`, `error`, and safety metadata.
- Use `--dry-run` before mutations when supported.
- Do not automatically add a flag classified as `safety_bypass`; ask for human approval.
- Verify semantic corrections with the read-only command in `verifyAfter`.
- Bound list operations with `--limit`, or opt into all pages with `--all`.

## Compose output

Use JMESPath projections to reduce tokens:

```bash
fabio workspace list --query "[].{id:id,name:displayName}"
```

Use `--quiet` when an agent only needs the exit status.
