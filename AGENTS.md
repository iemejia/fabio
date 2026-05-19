# Fabio CLI - Session Context

## Goal
- Design and implement an agent-first CLI (`fabio`) to manage Microsoft Fabric artifacts and data, inspired by AWS/gcloud/Azure CLI principles, with structured JSON output, composability via stdin/stdout, and machine-readable errors.

## Constraints & Preferences
- CLI designed for AI agents first (structured output, no interactive prompts, explicit params)
- JSON output by default with `--output json|table|plain` flag
- Composable: manage inputs and produce outputs for piping
- Machine-readable error codes in structured JSON envelope
- Rust (edition 2024, rust-version 1.85), uses clap derive, tokio, reqwest, azure_identity, serde, comfy-table, thiserror/anyhow
- Linting: clippy pedantic+nursery (zero warnings), rustfmt
- CI: GitHub Actions (cargo fmt, clippy, test, build release) on ubuntu/macos/windows
- Installable via `cargo install --git https://github.com/iemejia/fabio.git`

## Progress
### Done
- **Full Rust implementation** (40 commands across 6 groups): auth, workspace, item, lakehouse, notebook, warehouse
- Core output system: JSON envelope (`{"data":..., "count":N}` or `{"error":{"code":...,"message":...}}`), table, plain formats
- Structured error system: `ErrorCode` enum (AUTH_REQUIRED, NOT_FOUND, RATE_LIMITED, CAPACITY_INACTIVE, API_ERROR, TIMEOUT, etc.) + `FabioError`
- Global options fully wired: `--output/-o`, `--query/-q` (dot-notation field extraction), `--quiet` (suppresses stdout)
- HTTP client: async get/post/delete with LRO polling (`Location` + `x-ms-operation-id` + resource follow)
- OneLake operations: DFS upload (create+append+flush), download, file listing; Blob API copy (server-side async)
- **LRO polling**: 2s interval, 120s max, handles 200/202, checks `status` field until Succeeded/Failed
- **Server-side file copy/move**: Blob API `PUT` with `x-ms-copy-source`, move = copy + delete
- **Server-side table copy/move/delete**: Root listing + prefix filter, per-file Blob copy, recursive DFS delete
- **Shortcuts**: Create/get/delete OneLake, ADLS Gen2, S3 shortcuts
- **Notebook run**: Captures job instance ID from Location header, status/stop via Jobs API
- **Notebook `--wait` flag**: Polls job status every 5s until Completed/Failed/Cancelled, with configurable `--timeout` (default 600s)
- **Item copy/move**: getDefinition LRO + create in dest workspace LRO; move = copy + delete source
- **Warehouse show**: Shows warehouse details including connection string and properties
- **Warehouse query stdin**: SQL can be provided via `--sql`, `@file`, or piped from stdin
- **E2E verified** against live Fabric tenant: all 40 commands tested
- **65 Rust tests** (6 unit + 59 E2E integration), zero clippy warnings, rustfmt clean
- **CI/CD**: GitHub Actions (6-target matrix: x64+arm64 for linux/macos/windows), Dependabot for cargo + github-actions
- **Release workflow**: Triggered on tags, builds 6 binaries, publishes GitHub Release with SHA256 checksums
- Release binary: 4.4 MB, stripped, LTO-optimized

### Blocked
- (none)

## Key Decisions
- JSON envelope always wraps output: lists get `{"data":[...],"count":N}`, objects get `{"data":{...}}`
- Errors on stderr as `{"error":{"code":"...","message":"..."}}` with non-zero exit
- `--query` supports simple dot-notation field projection (not full JMESPath; users can pipe to `jq`)
- `--quiet` suppresses all stdout; errors still go to stderr
- OneLake upload uses DFS create+append+flush 3-step pattern
- Notebook creation builds minimal .ipynb JSON, base64-encodes for Fabric API; `source` must be list of strings
- Item copy fetches definition from source via LRO, posts to destination workspace via LRO
- LRO polling: 2s default interval, 120s max wait, handles `Location`/`x-ms-operation-id` headers
- `post()` accepts `poll: bool` for LRO-aware operations
- Load-table requires PascalCase values (`"Overwrite"`, `"Csv"`) and `format` inside `formatOptions`
- **Server-side copy**: OneLake Blob API supports `PUT` with `x-ms-copy-source`; returns 202 with pending status. Poll via HEAD.
- **No native move/rename**: OneLake rejects `x-ms-rename-source`. Move = copy + delete.
- **Table file listing**: Must list from root (no `directory` param) to get real paths prefixed with item ID.
- **Recursive delete**: DFS `DELETE /{ws}/{lh}/Tables/{name}?recursive=true` works for directories.
- All destructive actions use consistent verb `delete` (not `remove`)
- Cross-workspace ops use `--source-workspace`/`--dest-workspace` with `visible_alias` short forms
- Auth relies on `DefaultAzureCredential` chain (az login, environment, managed identity)
- `azure_identity`/`azure_core` with `default-features = false` (no OpenSSL dependency)
- `unsafe_code = "forbid"` in lints

## Critical Context
- User's tenant: set locally via secure environment configuration (redacted)
- Active capacity: set locally via secure environment configuration (redacted)
- Inactive capacity: set locally via secure environment configuration (redacted)
- Source workspace/lakehouse: set locally via secure environment configuration (redacted)
- Destination workspace/lakehouse: set locally via secure environment configuration (redacted)
- Notebook ID: set locally via secure environment configuration (redacted)
- Fabric REST base URL: `https://api.fabric.microsoft.com/v1`
- OneLake DFS base URL: `https://onelake.dfs.fabric.microsoft.com`
- OneLake Blob base URL: `https://onelake.blob.fabric.microsoft.com`
- Fabric scope: `https://analysis.windows.net/powerbi/api/.default`
- Storage scope: `https://storage.azure.com/.default`
- Spark rate limit on small capacity: LRO reports 430 `TooManyRequestsForCapacity` (non-standard code)
- Test env vars: `FABIO_TEST_SOURCE_WORKSPACE`, `FABIO_TEST_SOURCE_LAKEHOUSE`, `FABIO_TEST_DEST_WORKSPACE`, `FABIO_TEST_DEST_LAKEHOUSE`, `FABIO_TEST_NOTEBOOK_ID`, `FABIO_TEST_CAPACITY_ID`

## Relevant Files
- `Cargo.toml`: Project config, dependencies, clippy/lints config, release profile (LTO+strip)
- `rust-toolchain.toml`: stable channel, rustfmt+clippy components
- `src/main.rs`: Entry point, tokio async main, error handling dispatch
- `src/cli.rs`: Clap derive CLI definition, OutputFormat enum, Command enum with 6 subcommand groups
- `src/errors.rs`: ErrorCode enum + FabioError struct with thiserror
- `src/output.rs`: render_list, render_object, render_error (respects --quiet/--query), apply_query, unit tests
- `src/client.rs`: FabricClient with async HTTP, LRO polling, OneLake DFS/Blob ops, run_notebook
- `src/commands/mod.rs`: Command dispatch
- `src/commands/auth.rs`: login/logout/status (DefaultAzureCredential chain)
- `src/commands/workspace.rs`: list/show/create/delete/assign-capacity
- `src/commands/item.rs`: list/show/create/delete/copy/move
- `src/commands/lakehouse.rs`: 14 subcommands (tables, files, upload, download, load-table, copy-file, delete-file, move-file, delete-table, copy-table, move-table, create-shortcut, get-shortcut, delete-shortcut)
- `src/commands/notebook.rs`: create/get-definition/run (with --wait/--timeout)/status/stop/delete
- `src/commands/warehouse.rs`: list/show/query (endpoint resolved, stdin/file/flag SQL input, ODBC execution TODO)
- `tests/common/mod.rs`: Shared E2E test harness (TestConfig, helpers)
- `tests/e2e_auth.rs`: Auth integration tests
- `tests/e2e_workspace.rs`: Workspace CRUD + assign-capacity tests
- `tests/e2e_global_options.rs`: --query, --quiet, --output format tests
- `tests/e2e_item.rs`: Item list/show/create/delete/copy/move tests
- `tests/e2e_lakehouse.rs`: Tables/files/upload/download tests
- `tests/e2e_lakehouse_files.rs`: File copy/move/delete tests
- `tests/e2e_lakehouse_tables.rs`: Table load/copy/move/delete tests
- `tests/e2e_lakehouse_shortcuts.rs`: Shortcut create/get/delete tests
- `tests/e2e_notebook.rs`: Notebook create/get-definition/run/run --wait/status/stop/delete tests
- `tests/e2e_warehouse.rs`: Warehouse list/show/query/query-stdin tests
- `.github/workflows/ci.yml`: Rust CI (fmt, clippy, test, build) on 6 targets (x64+arm64 x linux/macos/windows)
- `.github/workflows/release.yml`: Release workflow (tag-triggered, 6 binaries, SHA256 checksums, GitHub Release)
- `.github/dependabot.yml`: Cargo + GitHub Actions dependency updates

## OneLake API Behaviors Discovered
- Blob API copy (`x-ms-copy-source`): works for server-side file copy, async (202 with pending status)
- DFS rename (`x-ms-rename-source`): NOT supported (returns `UnsupportedHeader`)
- DFS recursive delete (`?recursive=true`): works for directories
- DFS listing with `directory` param on a table path shows virtual lakehouse structure (not real files)
- Root listing (no `directory` param): returns real paths prefixed with item ID
- Table files live at `Tables/{name}/_delta_log/` and `Tables/{name}/*.parquet`
- **DFS directory parameter "virtual lakehouse-in-lakehouse" view**: When `directory=X` is specified, the API returns ALL paths prefixed with `X/`, where top-level lakehouse dirs appear doubled (e.g., `Files/Files/myfile.csv` for a file at `Files/myfile.csv`). With `recursive=false`, only immediate virtual children show. Fix: always use `recursive=true` and strip the doubled prefix client-side.
- **Notebook Jobs API**: `POST /workspaces/{ws}/items/{id}/jobs/instances?jobType=RunNotebook` returns 202 + Location header with job instance URL. Status endpoint returns `NotStarted`, `InProgress`, `Completed`, `Failed`, `Cancelled`. Cancel via `POST .../cancel`.
- **Spark cold start on small capacity**: First notebook run can take 2-5 minutes to transition from `NotStarted` to `InProgress` due to Spark session allocation.

## Next Steps
- Add ODBC support to warehouse query (`odbc-api` crate)
- Add `warehouse create`/`warehouse delete` commands
- Add `notebook update` command (update definition)
- Add pagination support for list commands (continuationToken)
- Consider adding `--filter` flag for list commands
