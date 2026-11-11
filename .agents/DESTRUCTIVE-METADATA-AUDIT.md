# Destructive metadata coverage audit

The previous audit enumerated only entries already labelled `destructive:true`.
It proved wiring for that set but could not discover absent annotations. The new
regression was run against the old committed metadata first and failed as expected.

## Judgment and mechanics

- Authored policy: `src/commands/context/data/agent/safety_policy.json`. It records
  destructive vocabulary, reviewed workflows, risky local flags, action-scoped
  exceptions and explained reversible/read-only exceptions. This is judgment, not
  a manually maintained generated command catalogue.
- Generator: nested commands are generated recursively. Required safety judgment
  overrides missing or stale false annotations without losing unrelated semantic
  fields/examples. Conditional modes are represented separately so mode-off previews
  do not become falsely unconditional.
- Independent gate: walk real clap commands/flags and compare published metadata,
  not the set already carrying a destructive label. Synthetic future-command and
  stale-false tests anchor the rules; policy paths must be real and have rationale.
- Runtime: consume generated metadata only, including complete nested canonical
  paths. Do not reclassify from policy at runtime and silently conceal drift.

## Confirmed runtime wiring repairs

The scoped audit found missing Git checkout and ontology replacement dry-run
guards, and custom deployment/export previews missing the shared confirmation
signal. These now preview before mutation; checkout is offline. Deployment risk
includes definition updates/rename-republication, not just deletes or force-all.
Create-only/no-op action-scoped plans remain non-destructive.

Timeout cancellation modes now explicitly pass their selected risk into shared
preview helpers. Import table-drop is classified as dangerous (not a benign mode),
incompatible drop-without-recreate input is rejected, and import/statistics/local
overwrite paths enforce readonly before a connection or overwrite. Existing
backends/credentials are never exercised by these fixture tests.

## Verification scope and limits

The CLI tests exercise every definition-overwrite family with local inputs,
conditional modes both off/on, nested deletion, checkout validation/readonly,
deployment action plans and SQL readonly paths. Expanded ignored audits use local
mock endpoints, isolated home/working directories and dummy audience tokens;
unexpected HTTPS endpoints are contained by a loopback proxy. Definition/DAX
metadata reads remain allowed, not mistaken for mutating POSTs.

No live destructive operation is required or authorized by this audit. Vocabulary
and reviewed flags are not a semantic oracle for arbitrary future workflows.
Maintainers must register custom destructive effects in policy and add concrete
runtime guard tests rather than weakening a gate or assuming missing metadata is
safe. Native nested metadata and conservative MCP hints are covered; redesigning
every nested tool invocation interface is not part of this increment.
