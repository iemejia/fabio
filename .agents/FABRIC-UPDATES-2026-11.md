# Fabric blog follow-up — October 26–November 16, 2026

Work is sequential, with one validated commit per item. No push is requested.
Live output stays private; never record resource identifiers or endpoints here.

| Item | Scope | Status |
|---|---|---|
| 1 | Unified SQL pool compatibility and guidance | Complete |
| 2 | OneLake endpoint discovery and hosted MCP discovery | Complete |
| 3 | Safe table/column additional metadata | Complete (shortcut backend limitation documented) |
| 4 | Eventstream variable-library references and deployment ordering | Pending |
| 5 | Native Iceberg catalog writes | Pending |
| 6 | Connection-policy diagnostics | Pending |
| 7 | Real-Time Dashboard authoring (verify public definition first) | Pending |
| 8 | Project Osmos feasibility spike (support-contract decision first) | Pending |

## Validation contract

Each implementation needs unit and hermetic CLI e2e coverage, targeted live
validation, then format, warning-free test clippy and the full test suite before
commit. Documentation changes also require docs checks. New CLI surfaces require
schema/sub-skill regeneration and routing eval cases. Destructive changes must
meet the full guardrail stack; live writes use disposable fixtures only. Do not
change tenant-wide settings, existing production configuration, or broaden
permissions to manufacture successful validation. A rollout unavailable in the
tenant is a blocker/limitation, not a passed acceptance check.

## Item 1 acceptance

- Unit tests cover legacy toggle-only input, flag-less unified payloads, future
  fields, classifier preservation, invalid types, and accidental output envelopes.
- CLI e2e tests cover GET/PATCH wire equality, offline destructive preview and
  agent notice, readonly rejection, malformed input, and invalid workspace scope.
- Live read passed against an explicitly selected accessible warehouse workspace;
  no pool configuration was changed. The service still returned the legacy flag;
  future unified responses/PATCH are mock-tested, not claimed live-observed.
- Private test resource references were inaccessible; session-local fixture
  selection did not overwrite `.env` or change tenant settings/roles.
- Focused model evaluation first failed 0/1; explicit troubleshooting guidance
  was added, then it passed 1/1 with zero provider/grading errors (100% gate).
- Final format and test clippy passed with no warnings. Full suite: 3,048 passed,
  zero failed, 1,127 ignored. Both ignored destructive runtime audits passed.
  Docs check and 13 script tests passed; evaluation configuration validation passed.
- Destructive checklist: canonical preview includes workspace/configuration and
  replacement warning; client PATCH enforces readonly; safety policy generates
  destructive+mutates metadata; UUID validation protects scope; no bypass flags
  or protected item types were added; preview/invalid-scope e2e tests are present.

## Item 2 acceptance

- `item get-endpoints` uses storage-authenticated GET and validates UUID input and
  exact response scope. It preserves unknown engine fields, marks connections
  unverified and never fetches a skill or opens a returned connection.
- Unit tests cover path injection, scope mismatch/duplication, unknown metadata
  and empty engine lists. CLI e2e tests verify storage auth, no-follow request
  counts, invalid input, 404 teaching guidance, scope rejection and introspection.
- Live tests and `cargo run` succeeded for Warehouse and Lakehouse discovery.
  The lakehouse returned hosted OneLake MCP connection metadata. No tools were
  invoked, settings changed or resources created.
- Focused model evaluation passed 1/1 with zero provider/grading errors (100% gate).
- Final format and test clippy passed with no warnings. Full suite: 3,057 passed,
  zero failed, 1,128 ignored. Docs check and 13 script tests passed; generated
  schema/sub-skills and evaluation configuration validation are current.

## Item 3 acceptance

- Added storage-scoped table/column metadata reads and default-preserving edits,
  explicit full replacement, exact-scope validation and destructive before/after
  previews. No atomic-concurrency guarantee is made.
- Unit and CLI e2e coverage includes merge preservation, resolved column-ID
  normalization, invalid fields/limits/scope, overwrite removals and warnings,
  readonly enforcement, unknown/async submissions, failed readback and unresolved
  shortcut failures without repair or POST.
- Created an isolated shared validation workspace/lakehouse with a two-row
  synthetic table. Direct metadata read, description/tag/column writes, combined
  update/readback and restoration passed live. Fixture cleanup state is private;
  these fixtures remain in use for the subsequent items in this same task.
- Initial live testing exposed the documented-in-code columnName/columnId
  conflict; fixed preservation and readback normalization before acceptance.
- An internal shortcut probe failed on its initial GET because the backend
  reported missing Iceberg metadata files. No POST was attempted through it.
  Documented target-table effects are not claimed live-validated; the teaching
  error tells agents to inspect, not synthesize files or change conversion.
- Focused evaluation initially failed 0/1. Strengthened MUST guidance and made the
  concurrency question explicit without weakening assertions; then passed 1/1
  with zero provider/grading errors at the 100% gate.
- Destructive checklist: canonical scope/before/after preview; shared guarded
  storage POST with retained no-replay evidence; generated mutates/destructive
  metadata; UUID/exact-table blast-radius guards; existing --overwrite dangerous
  flag with explicit warning; no new protected types; preview/guard regressions.
- Final serialized validation: format and warning-free test clippy passed;
  3,071 tests passed, zero failed, 1,130 ignored. Both destructive runtime audits,
  docs check and 13 script tests passed. Live combined round-trip/restoration and
  the read-only shortcut teaching-error probe passed. Earlier overlapping suite
  runs interfered with existing profile tests; the final run was not overlapped.
