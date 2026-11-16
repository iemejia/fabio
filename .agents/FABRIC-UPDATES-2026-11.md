# Fabric blog follow-up — October 26–November 16, 2026

Work is sequential, with one validated commit per item. No push is requested.
Live output stays private; never record resource identifiers or endpoints here.

| Item | Scope | Status |
|---|---|---|
| 1 | Unified SQL pool compatibility and guidance | Complete |
| 2 | OneLake endpoint discovery and hosted MCP discovery | Complete |
| 3 | Safe table/column additional metadata | Complete (shortcut backend limitation documented) |
| 4 | Eventstream variable-library references and deployment ordering | Complete |
| 5 | Native Iceberg catalog writes | Complete |
| 6 | Connection-policy diagnostics | Complete (live policy-denial induction excluded) |
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

## Item 4 acceptance

- Added connector/property-scoped symbolic reference validation and extraction;
  preserved authored references instead of exporting resolved topology. Existing
  Eventstream definition reads now support readonly LRO transport.
- Scoped plan/UUID guards and explicit reviewed --env/default-or-sequential/hooks
  requirements precede mutation. Referenced libraries activate and verify before
  consumer publication, including skipped libraries. Uncertain/failed activation
  halts later mutations; per-library outcome records survive partial failures and final
  hooks do not repeat verified activation. Activation is not an atomic cutover.
- Unit and CLI e2e tests cover syntax/matrix/conflicts, no-op/deleted consumers,
  missing scopes/value sets, previews, readonly reads, creation ordering,
  skipped-library activation, single activation, forbidden/uncertain PATCH and
  failed readback with no consumer writes.
- Live scoped update verified pre_stream activation, symbolic authored metadata
  without non-null resolved IDs and runtime routing to the alternate lakehouse.
  The separate ignored live read test passed. No events were sent, so event
  delivery and source/destination data authorization are not claimed verified.
- Deleted the isolated stream, library and alternate lakehouse; scoped inventory
  verified absence. The shared base fixture remains for subsequent items.
- Focused model evaluation passed 1/1 with zero provider/grading errors at 100%.
- Destructive checklist: existing deploy apply guard covers all added mutations;
  previews expose unverified prerequisites and conservative destructive/agent
  signals; client PATCH enforces readonly; source scope/UUID guards fail before
  writes; no bypass flag/protected type added; ordering/failure/dry-run tests pass.
- Final format and warning-free test clippy passed; 3,085 tests passed, zero
  failed, 1,131 ignored. Both destructive runtime audits, docs check and 13 script
  tests passed. Outcome records carry stable workspace/item IDs; duplicate plan
  entries/IDs and library name/ID mismatches cannot activate an unrelated library.

## Item 5 acceptance

- Native catalog create/commit/delete use storage auth, bounded UTF-8 request
  input, discovered-prefix validation and exact UUID/namespace/table scope.
  Create allocates an empty table, not data files/manifests/rows. Commit preserves
  caller assertions and rejects UUID/location reassignment or mismatched identifiers.
- Unit and CLI e2e tests cover native/virtual/shortcut guards, input limits,
  prefix mismatches, readonly, canonical destructive previews, exact wire payloads,
  no credential-config output, uncertain/async submissions and catalog absence.
- Live create via `cargo run` allocated a format-v2 native table; commit/readback
  passed. The stale schema assertion failed with CONFLICT/retriable:false and did
  not apply its property. The ignored lifecycle passed and deleted the table;
  catalog absence was verified. No physical purge or row-writer validation claimed.
- Live read-only virtual-Delta deletion probe hit the native-only input guard.
- Destructive checklist: commit/delete use canonical dry-run guards after pure
  scope validation and native/identity reads; shared mutation helpers enforce
  readonly and no replay; generated destructive+mutates metadata; no new bypass
  flags or protected item types; UUID/exact-path guard and preview e2e coverage.
  DELETE's identity snapshot is explicitly non-atomic and purge is unverified.
- Final format, warning-free test clippy, 3,098 tests and both destructive runtime
  audits passed (1,132 tests ignored). Docs check and 13 script tests passed.
  Focused evaluation passed 1/1 after regenerating the updated skill; the first
  attempt used stale generated guidance and failed. No provider/grading errors.

## Item 6 acceptance

- Narrow authentication-kind/inter-tenant policy signals now produce tenant-admin
  review guidance after command-specific enrichment. Service code, nested details,
  request IDs and submission evidence are preserved; automatic retry is forbidden.
- Unit tests distinguish documented signals from generic 403/Offline/policy text;
  CLI e2e verifies nested signals and one-request/no-admin-fallback behavior.
- Live bounded existing connection metadata read passed, without credential tests,
  role changes or allowlist changes. Policy-denial paths are mock-tested, not
  live-induced: tenant-wide mutation to manufacture denial was deliberately avoided.
- Guidance distinguishes allowlists from workspace outbound rules, feature gates
  and PolicySet; no unsupported management API or duplicate connection command added.
- Docs checks/13 script tests and focused evaluation (1/1 at 100%) passed.
- Final format, warning-free test clippy and full suite passed: 3,102 tests,
  zero failed, 1,133 ignored. No mutating feature/flags were added.
