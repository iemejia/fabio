# PR #206: live acceptance and quality-of-life plan

## Scope and evidence

Reviewed the 66-endpoint change against public Fabric specification commit
`ec85f05`, then exercised the PR build against a disposable, capacity-backed
workspace on 2026-11-18. All fixtures contained synthetic data. Tenant identifiers,
resource names, endpoints, credentials, and raw live logs are intentionally absent.

**Do not equate specification parity, passing mocks, successful item creation,
or an empty list with a fully live-validated feature.** The matrix below is the
acceptance boundary for this review.

## Live acceptance matrix

| Family | Verified live | Remaining boundary |
| --- | --- | --- |
| Environment custom live pools | Disabled schedule lifecycle passed; QoL follow-up submitted one approved job, exercised bounded timeout/scoped cancellation, retained its receipt and independently observed `Cancelled` | Fully hydrated/currently warm capacity was not verified. Release compute configuration was prepared explicitly, not through an implicit fallback. See execution ledger. |
| Lakehouse semantic views | Owned lakehouse creation and successful empty semantic-view list | No authored semantic-view fixture; publish is mock/dry-run covered, not live accepted. |
| Ontology versions | Initial lifecycle passed; QoL follow-up additionally restored deliberate generation-2 TMDL namespace changes and compared full model-part readback to the baseline | Bound data and dependent-item readiness remain unverified. See the execution ledger for completion/output/format safeguards and validation. |
| Microsoft IQ | Read-only `am-i-in-scope` and `retrieve`; empty permitted retrieval accepted; both ignored live tests passed | No permitted ranked-context fixture was available. Nonempty retrieval, permission filtering across principals, and content dereferencing are not live verified. No returned URI was followed. |
| Playbook | Workspace list succeeded with no Playbooks | Create returned HTTP 500 `MiqArtifactLifecycleForwardingFailed`, with unknown submission state and `retriable:false`. No automatic replay. Subsequent read-only reconciliation found no Playbook. Hierarchy/context/roles/definition lifecycle is mock covered; the ignored live lifecycle remains blocked. |
| PostgreSQL Database | Workspace list succeeded | Create returned a feature-disabled forbidden error. No setting was changed. Database creation, `pgschema` round-trip, and rebuild behavior remain live blocked. |
| Share | Workspace list succeeded | Create returned a feature-disabled forbidden error. No recipients, invitations, policies, or external data access were created. Bulk management remains mock covered. |
| Share Subscription | Workspace list succeeded | No approved invitation or enabled producer fixture. Provisioned assets/policies and synchronize remain mock covered, not live accepted. |
| SQL endpoint migration | The owned lakehouse's SQL endpoint was independently readable; status and preflight requests were attempted and later reconciled | Both new routes returned `EntityNotFound` on the readable endpoint. Cause is unresolved: rollout, backend eligibility, or another service prerequisite. Do not claim a specific cause from this evidence. No metadata upgrade was run. |

The operator approved disposable-fixture creation and cleanup, not tenant-setting
changes, external sharing, or changes to existing SQL endpoints. Raw evidence is
kept outside the repository. The disposable workspace was deleted and a subsequent
read returned `NOT_FOUND`, verifying final cleanup including backing items. No
test resources were retained to work around blocked APIs.

## Coverage strengthened in this review

Added **13 unit tests and 41 integration/live-test cases**, in addition to
strengthening existing assertions. A source/test inventory audit accounted for
all 66 new subcommand names across the nine families; this is a discoverability
check, not a substitute for request assertions or live acceptance.

- Replace literal-only unit assertions with production request-builder tests for
  Microsoft IQ and PostgreSQL creation.
- Unit-test blank queries, malformed IQ responses, PostgreSQL UTF-16 metadata
  limits, omitted versus explicit-null creation payloads, and actual encoded
  `pgschema` parts.
- Unit-test Playbook segment encoding, root/traversal deletion refusal, same-parent
  rename constraints, and UTF-8 byte-counted context summaries. Root reads remain
  valid; deletion through the root resource path does not.
- Unit-test custom-pool schedule file/inline equivalence, CRLF and malformed JSON,
  plus ontology metadata boundaries, description clearing, and restore-option files.
- Add mock-backed CLI coverage across all nine API families: canonical method,
  case-sensitive route, beta query, body, rename header, accepted job identity,
  definition format, binary bytes, continuation tokens, and structured output.
- Cover readonly interception without network dispatch, scoped invalid-input
  failures even with dry-run, destructive agent notices, and rebuild warnings.
- Reproduce Playbook's uncertain-create error without replay; test IQ forbidden
  and malformed responses separately from empty success; preserve SQL route
  not-found errors and follow preflight LROs without resubmitting.
- Add opt-in owned-fixture live tests and a shared fixture guard with explicit
  asserted cleanup and a best-effort panic fallback. Error parsing tolerates
  diagnostic lines on stderr without weakening the structured-error assertion.

### Re-running the new live tests

Use an explicitly approved **disposable** `FABIO_TEST_SOURCE_WORKSPACE` on an active
capacity. The new fixture guard needs no unrelated lakehouse/notebook variables.
Run exact tests, not the entire ignored suite (which includes unrelated mutations):

```bash
cargo test --test e2e_environment custom_live_pool_disabled_schedule_lifecycle_live -- --ignored --exact
cargo test --test e2e_ontology ontology_version_lifecycle_live -- --ignored --exact
cargo test --test e2e_microsoft_iq test_am_i_in_scope_live -- --ignored --exact
cargo test --test e2e_microsoft_iq retrieve_live_allows_empty_permitted_results -- --ignored --exact
```

After the Playbook backend is available, explicitly approve running:

```bash
cargo test --test e2e_playbook playbook_section_group_context_lifecycle_live -- --ignored --exact
```

Ontology provisioning can create backing items. The fixture guard deletes only
its directly created item; final disposable-workspace cleanup is required to
remove any backing resources. A failed create with uncertain submission must be
reconciled, not blindly retried.

## Prioritized quality-of-life roadmap

These are proposed follow-ups, **not additional features implemented by this
initial validation**. Execution progress is tracked below. Ship small,
independently testable increments and commit only after local and scoped live acceptance.

### P0 — finish acceptance and protect users

1. **Resolve service prerequisites without guessing.** Obtain admin-reviewed
   PostgreSQL/Share enablement, resolve the Playbook forwarding failure, and find
   an eligible migration endpoint. Inspect configuration first; only add named
   tenant-gate registry entries after the repository's live verification protocol.
   Acceptance: each blocked lifecycle passes on owned fixtures, or is explicitly
   documented as unavailable. Never enable settings automatically.
2. **No-clobber Playbook downloads.** Reuse the repository's safe-download pattern:
   refuse an existing destination by default, stage bytes before replacing, and
   require an existing approved overwrite mechanism if replacement is supported.
   Acceptance: pre-existing bytes survive failed/download/dry-run cases; readonly
   still permits the remote read without silently authorizing local replacement.
3. **Migration results must distinguish success from partial failure.** Preserve
   table-level blockers/errors and unknown future fields; expose an explicit
   completion/retry decision instead of relying on HTTP success alone. Add
   partial/failed LRO and accepted-but-unreadable-result regressions. Execution
   receipts and no-replay semantics must precede any convenience retry command.
4. **Complete nonempty acceptance fixtures.** Author one semantic view, a changed
   ontology snapshot, and permitted IQ context; use an explicitly approved
   same-tenant recipient/invitation for Share/Subscription only when available.
   Acceptance: publish/restore/retrieve/synchronize verifies content and identity,
   not merely that the call returned 200. No cross-tenant invitation by default.

### P1 — high-value operator and agent ergonomics

| Feature | Proposed improvement | Acceptance criteria |
| --- | --- | --- |
| Custom live pools | Reuse `item_job::run_and_wait` where compatible; honor bounded wait/timeout and preserve accepted job identity. Add typed disabled-schedule examples and file-based recipes. | Accepted work is never replayed after timeout; polling/cancellation are scoped to that job; schedules remain disabled in tests; unknown configuration fields round-trip. |
| Playbook | Add a content-file path that performs base64 encoding, typed reference validation, and explicit same-parent rename previews. Split by subcommand if the module crosses 1,500 lines. | Markdown/ZIP bytes round-trip; invalid references fail before dispatch; summaries remain byte-bounded; replacing context stays destructive and readonly guarded. |
| Ontology versions | Add a snapshot/restore recipe and scoped comparison/backup guidance; return an explicit restored status for empty service responses without claiming dependent-item readiness. | Version IDs are stable, restore never opts into overrides implicitly, and a changed-model live test verifies resulting definition state. |
| PostgreSQL | Add local `pgschema` definition preflight and examples for omitted/null/product creation modes, plus actionable endpoint discovery when provisioned. | Invalid inputs fail locally; no implicit `allowTableRebuild`; raw-schema support remains; do not route PostgreSQL SQL to the T-SQL transport. |
| Microsoft IQ | Teach query/result semantics and typed URI handling in authored context guidance. Offer projections for summary/rank/identifier, not automatic content fetching. | Empty context is distinct from authorization/backend failure; null scores and future URI discriminators survive; returned pointers are never followed automatically. |
| Share/Subscription | Add request templates, bulk dry-run summaries with counts and destructive classification, and an inspect-before-synchronize recipe. | Exact assets/recipients/policies are visible before writes; empty batches and mismatched policy IDs fail locally; invitation secrets and resource details are redacted; no hidden external grants. |
| Semantic views | Add a list/select/publish/verify workflow and actionable missing-view errors, preserving the original service error. | Publishing is verified against the returned semantic model; missing/unauthorized views do not become empty success; never invent view-authoring APIs. |
| SQL metadata migration | Add an assess-only workflow combining status and preflight with eligibility/blocker explanations and guarded upgrade examples. | Assessment never upgrades or drops metadata; upgrade remains a separate approved step; completed/partial/unknown outcomes and service rollout gaps are explicit. |

### P2 — sustainable regression coverage

- Add a feature-scoped live CI matrix with explicit capacity/workspace approval,
  private logs, numeric-only summaries, and finally-style workspace cleanup.
  Missing prerequisites must be reported as blocked, not counted as passes.
- Add pagination second-page and LRO failure/timeout cases to families whose
  current tests only preserve a continuation token or cover synchronous success.
- Add schema-backed request fixtures and full lifecycle tests for Playbook role
  grants, reference contexts, and ZIP storage semantics; preserve raw-body escape
  hatches rather than hardcoding one sample payload.
- Expand authored sub-skill gotchas and promptfoo cases as each QoL increment
  lands, then regenerate sub-skills and run drift checks. Mechanics stay generated;
  rollout observations and safety judgment belong in context data.

## Definition of done for each follow-up

Unit tests exercise production helpers; mock CLI tests assert exact requests,
structured errors, readonly interception and offline dry-run; opt-in live tests
verify an owned-fixture lifecycle and cleanup. Follow repository destructive
guardrails, HTTPS-only transport, Windows portability, and generated-file rules.
Run format, zero-warning Clippy, full tests and docs checks. Cross-target lint is
required before pushing. No release or merge should describe blocked rows above
as live-validated until their acceptance evidence exists.

## Initial validation notes

- Final `cargo fmt -- --check` and `cargo clippy --tests -- -D warnings`
  passed. Full `cargo test` passed: **3,206 passed, 0 failed, 1,140 ignored**.
  Ignored tests are not included in the live acceptance claim.
- Four exact opt-in live tests passed: disabled schedule lifecycle, ontology
  version lifecycle, IQ scope read, and IQ retrieval read.
- Sub-skills were regenerated from authored judgment, not edited manually.
- Docs type/link checks passed; all 13 docs-script tests passed; the production
  docs build succeeded with content warnings about the empty `i18n` collection
  and missing custom `404` entry.
- The preflight LRO mock includes the successful poll response's result
  `Location` header; operation status by itself is not a preflight result.
- Tenant-detail review found no inspected tenant/resource identifiers in the
  outgoing diff. No tenant settings were changed, sharing invitations sent,
  actual pools hydrated, or existing SQL endpoints upgraded.

## QoL execution ledger

| Increment | Status | Acceptance / boundary |
| --- | --- | --- |
| Ontology selected-version recovery | Completed (`73e940f`) | Authored baseline TMDL namespace, snapshot, deliberate second namespace, restore without import overrides, independent full TMDL readback equality, stable version ID and legacy-editor rejection all passed. Explicit output leaves readiness unverified. Format, zero-warning Clippy, full tests (3,218 passed / 0 failed / 1,141 ignored), docs checks and both focused evals passed (100%); disposable-workspace cleanup verified. Dependencies/data readiness remain outside this fixture's claim. |
| Custom live-pool job observation/cancellation | Completed (`ee3a6ba`) | One approved job on an owned published Environment/custom Spark pool; timeout retained the exact receipt, cancellation targeted that job, and an independent read confirmed `Cancelled`. Format, zero-warning Clippy, full tests (3,225 passed / 0 failed / 1,142 ignored), docs checks and both focused evals passed (100%); workspace cleanup verified. No replay, tenant-setting change or existing-environment mutation. Warm readiness is not claimed. |
| Explicit release Spark compute selection | Completed (`0fcaf1b`) | Three settings commands accept explicit beta/release selection; default beta is preserved. Typed release updates retain unrelated array properties/fields/order, invalid shapes and error-bearing reads fail before PATCH, and no fallback occurs. Raw selection, typed replacement/addition, second-update preservation, disabled support, published readback, legacy dictionary read and zero activations passed live. Format, zero-warning Clippy, full tests (3,236 passed / 0 failed / 1,143 ignored), docs checks and both focused evals passed (100%); cleanup verified, no second hydration submission. |
| Microsoft IQ safe retrieval guidance/projections | Completed | Positive int32 budget, explicit false/empty-versus-error handling, quoted `$type` projections, open-pointer preservation/no-follow tests and data-only guidance. Three exact readonly tenant tests passed; the current scope probe was false and synthetic-query result count zero. Format, zero-warning Clippy, full tests (3,244 passed / 0 failed / 1,144 ignored), docs checks and six focused/regression evals passed (100%). Nonempty/cross-principal grounding remains separately unverified. No context authoring, content fetching or permission change. |
| Tenant/admin-gated families | Blocked as recorded above | No consent to enable tenant switches, send invitations, or upgrade existing SQL endpoints is inferred. |
