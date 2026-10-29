# Running deployment and governance E2E tests

PR #188 added mocked HTTP, input-validation, LRO, and dry-run tests. The
`e2e_deployment_governance` suite adds opt-in **live** lifecycle tests for those
APIs. Ordinary `cargo test` runs the deterministic tests without tenant access.

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
| `FABIO_TEST_CONNECTION_PRINCIPAL_ID` | Explicitly approved **other** principal to receive temporary ownership of a newly created anonymous Web connection. Never an arbitrary discovered tenant user. |
| `FABIO_TEST_CONNECTION_PRINCIPAL_TYPE` | `User` or `ServicePrincipal` for that recipient. |

The admin-connection tests require tenant-level Fabric Administrator access;
workspace or capacity administration alone is not sufficient. The pipeline test
also requires permission to create deployment pipelines.

## Run selected live tests

```bash
# DeploymentPlan create/list/show/update/get-definition/update-definition/delete
cargo test --test e2e_deployment_governance live_deployment_plan_lifecycle -- --ignored

# Native semantic-model creation and report creation/binding
cargo test --test e2e_deployment_governance live_empty_semantic_model -- --ignored
cargo test --test e2e_deployment_governance live_onelake_semantic_model -- --ignored
cargo test --test e2e_deployment_governance live_report_native_model_reference -- --ignored

# All nine actor APIs; uses disposable workspaces, never existing workloads
cargo test --test e2e_deployment_governance live_capacity_ -- --ignored --test-threads=1

# Pool history/filtering and missing-job errors; optional existing-job diagnostics
cargo test --test e2e_deployment_governance live_environment_pool_activation_history -- --ignored
cargo test --test e2e_deployment_governance live_environment_pool_activation_diagnostics -- --ignored

# Connection effective role; admin CRUD and separate explicit-recipient ownership
cargo test --test e2e_deployment_governance live_connection_effective_role -- --ignored
cargo test --test e2e_deployment_governance live_admin_connection_ -- --ignored --test-threads=1

# KQL dashboard creation/read/delete after the response-header spec change
cargo test --test e2e_deployment_governance live_kql_dashboard_ -- --ignored

# Beta plan references through bulk import and deployment pipelines
cargo test --test e2e_deployment_governance live_bulk_import_ -- --ignored
cargo test --test e2e_deployment_governance live_pipeline_deploy_ -- --ignored
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

For deployment-plan content and service-specific findings, see the October 2026
section of [API behaviors](../.agents/API-BEHAVIORS-DISCOVERED.md).
