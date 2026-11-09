# Running deployment and governance E2E tests

For live Foundry-backed skill routing/safety evaluations and the hermetic
evaluation-runner tests, see [eval/README.md](eval/README.md). These do not execute
the generated Fabric commands and do not replace the live API suites below.

PR #188 added mocked HTTP, input-validation, LRO, and dry-run tests. The
`e2e_deployment_governance` suite adds opt-in **live** lifecycle tests for those
APIs. Ordinary `cargo test` runs the deterministic tests without tenant access.

## Unit tests and coverage measurement

The focused unit tests cover definition file/envelope parsing (including CRLF,
Unicode, missing files, and invalid UTF-8), every deployment-plan scope variant,
expiration-policy branches/calendar boundaries, native semantic-model mode and
URL validation, and Gen1 batch/byte boundaries. The defensive byte-limit check is
tested independently because valid 50-UUID batches cannot reach it. Thin HTTP
wrappers remain covered by the per-group mocked CLI tests instead of duplicating
their implementation in unit mocks.

Local receipt durability is tested without tenant access: `cargo test --bin fabio
commands::jobs::` checks strict parsing, legacy/extension-field compatibility,
read/write failures and stable locking. Test-only subprocesses exit abruptly on
each side of atomic publication and run concurrent append/update transactions.
`cargo test --test e2e_jobs --test e2e_gen1_execution` covers isolated-home CLI
reads/pruning, corruption preservation, bounded contention and mocked Gen1
submission/acknowledgement behavior. No tests prune the developer's ledger or
replay a live upgrade; these tests do not simulate hardware power loss or replace
Windows runtime CI.

Selected published BI workflow/schema/output examples also have a clap-contract
gate (`commands::context::recipe_tests`). It uses the test-only `shlex` parser to
handle quoting and `&&` chains, then parses each invocation with the actual CLI.
It never executes a shell, expands variables, reads a definition file, or contacts
the network. This catches repeated single-value flags and invalid creation-mode
combinations without maintaining a parallel flag registry. Coverage is explicitly
limited to the selected BI documents, not every legacy workflow in the repository.

```bash
cargo test --bin fabio commands::deployment_plan::tests
cargo test --bin fabio commands::capacity::actors::tests
cargo test --bin fabio commands::semantic_model::crud::tests
cargo test --bin fabio commands::dataflow::tests
cargo test --test e2e_deployment_governance live::tests
cargo test --bin fabio recipe_tests
cargo test --test e2e_bi_guidance
cargo test --bin fabio workspace::diagnostics
cargo test --bin fabio remediation
cargo test --test e2e_workspace_diagnostics
cargo test --bin fabio connection::diagnostics
cargo test --test e2e_connection_diagnostics
cargo test --bin fabio environment::diagnostics
cargo test --bin fabio commands::follow
cargo test --test e2e_environment_diagnostics
cargo test --test e2e_bi_readiness
cargo test --test e2e_bi_refresh
cargo test --test e2e_bi_source
cargo test --bin fabio semantic_model::source
cargo test --bin fabio onelake_metadata
```

`tests/common/live.rs` is included only by the governance suite. Its deterministic
tests inject an in-memory cleanup runner and a fake polling clock: they exercise
cleanup on normal exit and assertion unwinding, explicit-delete disarming,
fallback cleanup after failures, error redaction, and readiness/error/timeout
paths without tenant credentials, real sleeps, or global environment changes.

To measure the default unit **and offline integration** suite:

```bash
cargo install cargo-llvm-cov --version 0.9.1 --locked
rustup component add llvm-tools-preview
cargo llvm-cov --json --output-path target/coverage.json
```

Ignored live tests are not included. Line/region coverage is not branch coverage;
the stable-toolchain report does not enable LLVM's experimental branch coverage.
Whole-file totals can include embedded unit-test code, so inspect the targeted
helper functions and uncovered regions rather than treating a passing-test count
or an aggregate percentage as proof that all CLI behavior is exercised.

## Prerequisites

Authenticate with `az login` / `fabio auth login`, then set only the variables
required by the tests you intend to run. Use resource IDs through environment
variables; never commit them or publish raw tenant output.

| Variable | Purpose |
| --- | --- |
| `FABIO_TEST_SOURCE_WORKSPACE` | An active workspace where the caller can create and delete test items. |
| `FABIO_TEST_CAPACITY_ID` | An active capacity the caller can assign to newly created workspaces. Actor tests additionally require capacity-administrator permissions. |
| `FABIO_TEST_ONELAKE_TABLE_URL` | Optional canonical HTTPS OneLake URL of an existing, readable **Delta table, not a SQL view**. This source is read-only. When omitted, the test creates/deletes a schema-enabled lakehouse with a two-row CSV/Delta fixture. |
| `FABIO_TEST_ENVIRONMENT_ID` | Existing environment in the source workspace, for the read-only diagnostics test. |
| `FABIO_TEST_POOL_ACTIVATION_ID` | Existing pool-activation job in that environment. No Spark job is started just to create history. |
| `FABIO_TEST_SPARK_ITEM_ID` | Existing readable Notebook/Lakehouse/Spark job definition owning a monitoring session. |
| `FABIO_TEST_SPARK_ITEM_TYPE` | `notebook`, `lakehouse`, or `spark-job-definition` for that item. |
| `FABIO_TEST_SPARK_LIVY_ID` | Existing monitoring session UUID with readable application/log metadata; not an interactive Livy state name. |
| `FABIO_TEST_CONNECTION_PRINCIPAL_ID` | Explicitly approved **other** principal to receive temporary ownership of a newly created anonymous Web connection. Never an arbitrary discovered tenant user. |
| `FABIO_TEST_CONNECTION_PRINCIPAL_TYPE` | `User` or `ServicePrincipal` for that recipient. |
| `FABIO_TEST_GEN1_DATAFLOW_ID` | Explicitly approved existing Gen1 dataflow for GET-only planning and offline previews. Its workspace-wide Gen1 assessment is read within bounds; no upgrade or refresh runs. |
| `FABIO_TEST_ALLOW_GEN1_UPGRADE` | Must be exactly `true` ONLY for explicit approval of one irreversible upgrade of a fresh disposable test-owned Gen1 fixture. Never set this for arbitrary existing items. |

The admin-connection tests require tenant-level Fabric Administrator access;
workspace or capacity administration alone is not sufficient. The pipeline test
also requires permission to create deployment pipelines.

Delegated admin-connection reads additionally need `Tenant.Read.All` or
`Tenant.ReadWrite.All`. Audience/role activation alone does not certify API
authorization. Do not acquire consent, change roles/settings or switch identities
as test setup; use an explicitly approved authorized context.

## Run selected live tests

```bash
# DeploymentPlan create/list/show/update/get-definition/update-definition/delete
cargo test --test e2e_deployment_governance live_deployment_plan_lifecycle -- --ignored

# Native semantic-model creation and report creation/binding
cargo test --test e2e_deployment_governance live_empty_semantic_model -- --ignored
cargo test --test e2e_deployment_governance live_onelake_semantic_model -- --ignored
cargo test --test e2e_deployment_governance live_report_native_model_reference -- --ignored

# Execute the published native BI recipe, including a real DAX read and report binding
cargo test --test e2e_deployment_governance live_bi_creation_recipe_verifies_data_and_report_binding -- --ignored

# All nine actor APIs; uses disposable workspaces, never existing workloads
cargo test --test e2e_deployment_governance live_capacity_ -- --ignored --test-threads=1

# GET-only diagnostics before/after test-owned block and mission-critical changes
cargo test --test e2e_deployment_governance live_workspace_diagnostics_observes_policy_without_remediation -- --ignored

# A user-selected target fills a missing-input template; the preview never assigns it
cargo test --test e2e_deployment_governance live_unassigned_workspace_hint_requires_target_selection -- --ignored

# Pool history/filtering and missing-job errors; optional existing-job diagnostics
cargo test --test e2e_deployment_governance live_environment_pool_activation_history -- --ignored
cargo test --test e2e_deployment_governance live_environment_pool_activation_diagnostics -- --ignored
cargo test --test e2e_deployment_governance live_environment_diagnostics_configuration_only -- --ignored
cargo test --test e2e_deployment_governance live_environment_diagnostics_runtime_metadata_only -- --ignored
cargo test --test e2e_deployment_governance live_bi_readiness_verifies_queries_refresh_and_binding -- --ignored
# The BI live test now uses returned verified refresh IDs and, for an owned
# source fixture, validates generated OneLake framing receipts after SQL-schema
# preflight. It submits each intended refresh ONCE and cleans up created items.
# It also validates standalone source checks and --check-source integration for
# native and generated creation. Source checks themselves never create/frame.

# Connection effective role; admin CRUD and separate explicit-recipient ownership
cargo test --test e2e_deployment_governance live_connection_effective_role -- --ignored
cargo test --test e2e_deployment_governance live_connection_diagnostics_ -- --ignored --test-threads=1
cargo test --test e2e_deployment_governance live_admin_connection_ -- --ignored --test-threads=1

# GET-only bounded admin hygiene; compares owned metadata/roles before and after
cargo test --test e2e_deployment_governance live_admin_connection_hygiene_is_readonly_and_scope_bounded -- --ignored --test-threads=1

# GET-only Gen1 assessment/metadata; original definition must remain unchanged
cargo test --test e2e_deployment_governance live_gen1_upgrade_plan_preserves_the_original_dataflow -- --ignored --test-threads=1

# ONLY with separate explicit approval and a fresh disposable owned fixture
cargo test --test e2e_deployment_governance live_gen1_upgrade_receipt_is_exact_scope_and_submit_once -- --ignored --test-threads=1

# KQL dashboard creation/read/delete after the response-header spec change
cargo test --test e2e_deployment_governance live_kql_dashboard_ -- --ignored

# Beta plan references through bulk import and deployment pipelines
cargo test --test e2e_deployment_governance live_bulk_import_ -- --ignored
cargo test --test e2e_deployment_governance live_pipeline_deploy_ -- --ignored

# Real non-empty native plans: action ordering and intentional failure propagation
cargo test --test e2e_deployment_governance live_native_plan_ -- --ignored --test-threads=1

# Generic export/create/update/rename preserves identities but never executes actions
cargo test --test e2e_deployment_governance live_generic_deploy_preserves_plan_identity -- --ignored
```

With **all** prerequisites available:

```bash
cargo test --test e2e_deployment_governance -- --ignored --test-threads=1
```

## Safety and interpreting results

- Cleanup guards are registered immediately after creation and run on assertion
  failures too. Only test-created items, anonymous connections, pipelines, actors,
  lakehouses, and workspaces are deleted. No hard-delete, tenant-setting changes,
  capacity-wide surge-protection changes, or mutations of existing source tables
  are performed.
- Workspace capacity assignment is polled to completion before dependent requests.
  Command execution and polling have finite timeouts.
- Missing prerequisites, permissions, and unexpected service rejections **fail the test**.
  `AUTH_REQUIRED`, `FORBIDDEN`, and `API_ERROR` are not counted as happy-path passes.
- Recovery-hint tests parse concrete previews/verification against clap, execute
  guarded previews with zero mutating HTTP calls, and preserve original error
  metadata. The live block test also compares stdout nextSteps with strict error
  hints and confirms the block remains until explicit test-owned cleanup. The
  unassigned test fills the target from an explicit fixture solely for a preview;
  the workspace remains unassigned. ARM resume is not executed live.
- Connection diagnostics tests cover offline scope validation, safe provenance,
  PendingOwner, unknown roles/credentials, workspace identity context, implicit
  Automatic/SSO and unbound references, redaction, deduplication, bounded pagination,
  and total deadlines. Every diagnostic request is GET. The live tests use a
  test-owned anonymous connection and a Delta/native semantic-model fixture; they
  never test stored credentials, grant access, or transfer ownership. Live
  PendingOwner and provisioned identity metadata need explicit approved fixtures;
  source authorization is deliberately not checked by this command at all.
- Spark/environment diagnostic tests cover explicit/default context, publish and
  staging observations, bounded history, resource-count validation, scoped
  monitoring/application reads, redacted metadata-only logs, typed state filters,
  and terminal-aware follow. Deadlines also bound in-flight requests; row limits
  are exact. Live runtime tests require existing readable fixtures and do not
  start jobs, publish, or activate pools. The approved one-off tiny Spark workload
  was fixture setup, separate from diagnostics; positive activation-detail/follow
  coverage still needs an existing pool-activation fixture, never a policy change
  hidden in test setup.
- Admin hygiene has hermetic scope/recency/ownership/similarity, redaction,
  guarded-preview, pagination and total-deadline coverage. The owned live fixture
  was created and cleaned up, but the first admin snapshot returned
  `AUTH_REQUIRED`; positive hygiene and unchanged admin metadata/role coverage
  remain incomplete until the approved authorized context is available. Live
  unavailable-evidence probes report unknown without applying actions; they are
  NOT happy-path passes. No consent, grants, credential tests/writes, transfer or
  automatic ordinary-connection fallback occurs.
- The BI recipe test reads the runtime-published workflow instead of duplicating
  its command strings. It captures existing refresh IDs, submits framing once,
  waits for a new terminal refresh with a finite deadline, verifies the fixture's
  row count, and checks the report binding. Its default fixture lives entirely in
  a disposable workspace. An explicit external Delta fixture is read-only.
- The separate blocked-actor duplicate-create **negative** test expects the current
  service's structured rejection. It does not claim the spec's upsert behavior
  works live; normal create/show/list/update/delete have their own happy-path test.
- Failure messages deliberately omit commands, IDs, resource names, and API error
  bodies. Do not replace these assertions with `assert_cmd`'s raw failure reports
  when working against the tenant.
- `PendingOwner` completion requires the recipient to update connection credentials.
  The ownership test verifies the pending state and revokes its temporary grant;
  it does not impersonate the recipient or transfer credentials.
- Git pull plan references are tested against a mock server, including status-hash
  resolution, exact request bodies, and `beta=true`. Live Git synchronization needs
  a separately provisioned repository/connection; this suite does not connect or
  overwrite an existing workspace's Git binding.
- Gen1 upgrade request-size validation remains offline: do not upgrade a real
  dataflow irreversibly just to exercise a request-size bound.
- The Gen1 planning test uses an explicitly supplied existing fixture for
  bounded workspace assessment reads, original-definition comparison and
  guarded offline previews. It never upgrades, refreshes, takes over or rewrites
  the fixture. Test fixture creation via Power BI model.json import requires
  separate user approval and test-owned cleanup; it is not an automatic step of
  the planner or of the existing-fixture live test.
- The Gen1 execution test requires an additional explicit irreversible-upgrade
  approval variable and a fresh disposable fixture. It invokes upgrade ONCE,
  observes the same local receipt, validates exact per-item scope and checks the
  resulting Gen2 metadata without refreshing or changing consumers. Never retry
  this test after uncertain submission just to get a successful run. Keep the
  owned resource/receipt for investigation if acceptance or completion is
  unknown; cleanup is separately scoped to test-created resources.
- Non-empty plan fixtures use only isolated Wait/Fail data-pipeline jobs. The
  source items receive logical IDs before plans reference them, using the existing
  item metadata API only on items freshly created by that test. Job history proves
  execution order and that a failed action stops dependent deployment. Definitions'
  `.platform` placeholders are not used as authoritative identity metadata.

For deployment-plan content and service-specific findings, see the October 2026
section of [API behaviors](../.agents/API-BEHAVIORS-DISCOVERED.md).
