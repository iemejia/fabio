---
title: Why Fabio is agent-native
description: Understand the design choices that make Fabio reliable for autonomous tools.
---

Traditional CLIs optimize for a person at a terminal. Fabio treats a coding agent as a first-class operator.

## Non-interactive by default

Every input is explicit through flags, environment variables, files, or stdin. Commands fail fast instead of opening prompts that automation cannot answer.

## Structured contracts

Lists use `{"data":[...],"count":N}` and objects use `{"data":{...}}`. Errors use a machine-readable code, message, and actionable hint. Separating data on stdout from diagnostics on stderr makes pipelines predictable.

## Errors that teach

An error can enumerate valid values, suggest corrected syntax, or provide a read-only verification command. Hint types distinguish safe retries from changes that alter semantics.

## Explicit safety

Mutations support dry-run previews. Destructive plans identify themselves in structured output. Safety-bypass flags are called out so an agent cannot silently turn a guarded operation into an irreversible one.

## Progressive disclosure

The command schema provides mechanics. Focused skills provide judgment. Personas, disambiguations, workflows, and best practices route an agent to the smallest useful context instead of presenting the entire CLI at once.

Generated command tables prevent flag drift; authored recipes also need checks.
Selected BI workflow, schema, and output examples are parsed against the actual
CLI in hermetic tests, without executing a shell, expanding variables, reading
definition files, or contacting a tenant. The [BI creation guide](../../guides/bi-creation/)
explains the mode choices those recipes teach.

## Bounded and composable

Limits, pagination tokens, field projection, and quiet mode let agents control cost. Stable identifiers and JSON output connect one command to the next without scraping prose.
