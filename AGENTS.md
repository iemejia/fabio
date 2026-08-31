# Fabio CLI - Session Context

## Goal
- Design and implement an agent-native CLI (`fabio`) to manage Microsoft Fabric artifacts and data, inspired by AWS/gcloud/Azure CLI principles, with structured JSON output, composability via stdin/stdout, and machine-readable errors.

## Agent-Native CLI Principles

Fabio must always respect these 10 principles for agent-native CLIs:
https://trevinsays.com/p/10-principles-for-agent-native-clis

1. **Non-interactive by default** — No prompts; all inputs via flags/env/files. Non-TTY must fail fast.
2. **Structured, parseable output** — `--json` on every command; stdout = data, stderr = diagnostics; stable exit codes.
3. **Errors that teach and enumerate** — Errors include valid enum values, corrected command examples, and machine-readable codes with hints.
4. **Safe retries and explicit mutation boundaries** — `--dry-run` for mutations; idempotency-safe; stable returned IDs.
5. **Bounded responses** — `--limit` for list commands; default to concise output; truncation metadata in envelope.
6. **Cross-CLI vocabulary consistency** — Canonical agent verbs: `list`, `show`, `create`, `delete`, `copy`, `move`.
7. **Three-layer introspection** — `fabio context agent` provides machine-readable command schema (flags, types, mutability, examples). `fabio context` provides semantic knowledge (item definition schemas, workflow recipes, output examples, best-practices guidance).
8. **Async-aware execution** — `--wait` for async jobs; local job ledger (`fabio jobs list/get/prune`); status polling.
9. **Persistent identity through profiles** — Named profiles (`fabio profile save/use/list/show/delete`); `--profile` flag.
10. **Two-way I/O** — Feedback channel (`fabio feedback send/list`); artifact delivery via stdout/file.

## Constraints & Preferences
- **HTTPS-only endpoints (MANDATORY)** — fabio MUST only ever communicate with network endpoints over HTTPS. This applies to every endpoint fabio itself calls: the Fabric REST API, OneLake DFS/Blob/Table, ARM, Power BI, Kusto, a data agent's published consumption URL, and any user-supplied LLM endpoint (`--llm-endpoint`). Enforcement points already in place: (1) `client::validate_endpoint_env_overrides()` runs at startup and rejects any `FABIO_*_ENDPOINT`/`FABIO_*_SCOPE` override that is not `https://`; (2) `client::validate_trusted_url()` requires HTTPS + a trusted Microsoft host (used for `--published-url` and API-returned URLs); (3) `LlmClient::from_config()` rejects a non-HTTPS `--llm-endpoint` so the API key is never sent in plaintext. When you add ANY new outbound endpoint (a new base URL, a new user-supplied URL flag, a new env override), it MUST be validated as HTTPS (reuse `client::is_secure_or_loopback` / `validate_trusted_url`) and MUST NOT be constructed with an `http://` scheme. The ONLY permitted `http://` literals in the codebase are non-network identifiers: RDF/OWL namespace IRIs (`http://www.w3.org/...`, `http://example.org/...` in `ontology*.rs`) and the OAuth loopback redirect (`http://localhost:{port}` in `token_cache.rs`, required by RFC 8252). Plaintext `http://` to a **loopback** host (`localhost`, `127.0.0.0/8`, `::1`) is the one runtime exception (via `client::is_secure_or_loopback`) — it never leaves the machine, and is required for local mock-server tests and locally-hosted OpenAI-compatible model servers; a `http://` override to any non-loopback host is rejected. All documentation, examples, and `.md`/`.mdx` files MUST use `https://` for any real endpoint.
- **Windows-first compatibility** — All code must work on Windows. Use `Path::new().join()` (never hardcoded `/` for filesystem paths), `dirs::home_dir()` (never manual `HOME`/`USERPROFILE`), `.lines()` for text parsing (handles CRLF), no Unix-specific APIs. `.gitattributes` enforces LF line endings.
- **Throttling reduction** — Reduce the likelihood of API throttling by:
  - Use bulk and batch operations when available (e.g., `item bulk-create`, `item bulk-delete`, workspace role batch-assign, domain batch-assign).
  - Prefer list APIs over repeated single-resource requests (e.g., use a single list call + client-side filter rather than N individual show calls).
- CLI designed for AI agents first (structured output, no interactive prompts, explicit params)
- JSON output by default with `--output json|table|plain` flag
- Composable: manage inputs and produce outputs for piping
- Machine-readable error codes in structured JSON envelope
- Rust (edition 2024, rust-version 1.98.0), uses clap derive, tokio, reqwest, azure_identity, serde, serde_yaml, comfy-table, thiserror/anyhow
- Linting: clippy pedantic+nursery (zero warnings), rustfmt
- CI: GitHub Actions (cargo fmt, clippy, test, build release) on ubuntu/macos/windows
- Installable via `cargo install --git https://github.com/iemejia/fabio.git`
- **Dependency version freshness** — When introducing a new Cargo dependency or a new GitHub Action, always validate that you are using the most recent available and compatible version. Check crates.io for Rust crates and the action's repository releases/tags for GitHub Actions. Do NOT copy outdated versions from examples or memory — verify against the source of truth before adding. Additionally, reject any dependency with an incompatible license (GPL, LGPL, AGPL, SSPL, or any other copyleft license that would impose restrictions on the project). Only permissive licenses (MIT, Apache-2.0, BSD, ISC, Zlib, Unicode-3.0, etc.) are acceptable.
- **GitHub Actions pinning** — ALL GitHub Actions in `.github/workflows/*.yml` MUST be pinned to their full commit SHA with the version in a trailing comment. Format: `uses: owner/action@<40-char-sha> # v<major>` (or `# v<major>.<minor>.<patch>` for non-major tags). NEVER use floating tag references like `@v7` or `@stable`. This prevents supply-chain attacks where a tag is force-pushed to a compromised commit. When updating an action, always verify the new SHA matches the expected release tag from the action's repository.
- **Modern Rust idioms (MANDATORY)** — All code MUST leverage features available in the declared `rust-version` (currently 1.98.0). Do NOT write code using older patterns when a modern equivalent exists. When the MSRV is bumped, audit and migrate existing code. Key idioms to prefer:
  - `str::floor_char_boundary()` for safe string truncation (never raw `&s[..n]` on user/API text)
  - Let chains (`if let Some(x) = opt && condition { ... }`) instead of nested `if let` + `if`
  - `Option::is_none_or(|v| cond)` instead of `opt.is_none() || opt == Some(x)` or `opt.map_or(true, ...)`
  - `Option::is_some_and(|v| cond)` instead of `matches!(opt, Some(x) if cond)` or `opt.map_or(false, ...)`
  - `Duration::from_mins()` / `from_hours()` instead of `from_secs(N * 60)`
  - `std::io::read_to_string(reader)` instead of `let mut buf = String::new(); reader.read_to_string(&mut buf)`
  - `Vec::extract_if()` when you need both the removed elements and the remainder
  - `Value::from(x)` instead of `Value::String(x.to_string())` for `&str` values (canonical serde_json idiom)
  - `x.to_string()` instead of `format!("{x}")` for single-value Display formatting
  - `eq_ignore_ascii_case()` instead of `a.to_lowercase() == b.to_lowercase()` (allocation-free)
  - `HashSet`/`BTreeSet` for membership tests instead of `Vec::contains` or `.iter().any()` when the collection is checked multiple times
  - `const fn` for pure functions returning static data (enables compile-time evaluation)
  - `#[inline]` on small, hot-path functions called across module boundaries

## Irreversible Operations & Agent Safety (MANDATORY)

Fabio is agent-first. AI agents consume structured output and may automatically retry failed commands. When a command performs an irreversible or destructive operation, you MUST implement safety guardrails so agents are explicitly warned before proceeding.

### Rules for new commands or features:

1. **Identify irreversible operations** — Any operation that deletes data, overwrites definitions without backup, or cannot be undone. Examples: item deletion, `--hard-delete`, `--delete-orphans`, `--force-all` (overwrites all definitions), `updateDefinition` (replaces content permanently).

2. **Use `FabioError::with_hint()` for safety-bypass flags** — When an error or guard blocks execution and the hint suggests a flag that bypasses the safety check (e.g., `--force`, `--hard-delete`, `--allow-delete-types`), always use `with_hint()`. The hint text triggers the agent safety notice automatically when an AI agent is detected (`src/agent.rs`).

3. **Dangerous flags must be in `DANGEROUS_FLAGS`** — If you add a new safety-bypass flag, add it to the `DANGEROUS_FLAGS` array in `src/agent.rs`. This ensures the agent safety notice fires when the flag is suggested in an error hint. A deterministic test (`all_escalation_flags_are_triaged` in `src/agent.rs`) reads every escalation-style flag (`--allow*`, `--force*`, `--overwrite`, `--hard*`, `--drop*`, `--purge*`, `--prune*`, `--truncate*`, `--discard*`, `--delete-orphans`, `--cancel-on*`) out of `commands.json` and fails unless each is EITHER in `DANGEROUS_FLAGS` (a genuine bypass) OR in the test's explicit `BENIGN_ESCALATION_FLAGS` allowlist (a capability/behavior toggle). A newly-added escalation-style flag therefore cannot silently escape the notice mechanism — it fails the test until consciously classified. The companion test `every_dangerous_flag_triggers_the_notice` guards the wiring (every listed flag actually fires the notice).

4. **Add `"destructive": true/false` to batch output** — For commands that produce a plan or summary of multiple actions (like `deploy plan/apply`), include a `"destructive"` boolean field in the structured output. Set to `true` when the operation includes deletions, overwrites, or other irreversible actions. Agents use this field to decide whether to ask the human for confirmation.

5. **Protected types require explicit opt-in** — Data-bearing item types (Lakehouse, Warehouse, SQLDatabase, Eventhouse, KQLDatabase) require `--allow-delete-types` for deletion. If you add support for a new data-bearing item type, add it to `PROTECTED_DELETE_TYPES` in `src/commands/deploy/mod.rs`.

6. **Warn on force/override modes** — When `--force-all`, `--force`, or similar override flags are active, emit a warning in the output explaining the irreversibility. This helps agents surface the risk to the human.

7. **Never add interactive prompts** — Fabio is non-interactive (Principle 1). Do NOT add `y/N` prompts or `--auto-approve` flags. Instead, use structured output signals (`"destructive": true`, warnings, `agentNotice`) that agents can programmatically evaluate.

### Standard guardrail stack for a NEW destructive command (MANDATORY checklist)

ANY new command/subcommand/operation that deletes data, overwrites content without backup, permanently replaces a definition, kills a running session/job, or is otherwise irreversible MUST ship with the SAME guardrail stack the existing destructive ops use (`item delete`, `lakehouse delete-directory`/`delete-table`/`delete-file`, `deploy apply --delete-orphans`, `warehouse queries-kill`, `data-agent delete --hard-delete`, `git relation delete`, `updateDefinition`, `reset`, `prune`, …). Before you consider the feature done, verify EVERY box:

- [ ] **`--dry-run` guard** — call `output::dry_run_guard(cli, "<group> <subcommand>", &preview)` and return early when it returns `true`, BEFORE any mutating network call. `preview` must describe exactly what would be affected (ids, paths, counts). Put the guard AFTER input-scope validation so a dry-run of an unsafe request still surfaces the validation error. The operation string MUST be the canonical `"<group> <subcommand>"` key (NOT a descriptive sentence like `"Would delete X"`): `dry_run_guard` looks it up in `commands.json` and, for a `destructive: true` command, adds `"destructive": true` + an agent `agentNotice` automatically — a mismatched key silently drops that confirm-with-user signal (enforced by `destructive_dry_run_previews_carry_confirm_signal`).
- [ ] **`--readonly` enforcement** — the mutation must route through a client method that calls `guard_readonly("<METHOD>", …)` (all `post`/`put`/`patch`/`delete` client helpers do). Never bypass the client for a raw mutating request.
- [ ] **`"destructive": true` in `commands.json`** — after `cargo test generate_agent_schema -- --ignored`, confirm the subcommand has `"destructive": true`. The generator only auto-infers this for `delete*`-named subcommands; for destructive ops with other names (`reset`, `kill`, `prune`, `update-definition`, `--hard-delete`, `--force*`, `--delete-orphans`) you MUST set it manually. Also set `"mutates": true`.
- [ ] **Blast-radius input guard** — if a malformed/empty/wildcard input could destroy far more than intended (e.g. an empty/root path recursively deleting an entire item — see `validate_delete_directory_path`; a glob matching everything; a missing filter deleting all rows), add a pure `validate_*` function that refuses the catastrophic case with a clear `FabioError::with_hint`, and unit-test it. Fail BEFORE any network call.
- [ ] **Safety-bypass flags** — if the operation is gated behind a bypass flag (`--force`, `--hard-delete`, `--allow-delete-types`, `--delete-orphans`, `--overwrite`, …), add the flag to `DANGEROUS_FLAGS` in `src/agent.rs` and surface it via `FabioError::with_hint()` so the `agentNotice` fires (rules 2–3 above).
- [ ] **Tests** — an e2e test asserting the `--dry-run` output (`dry_run: true`, `would_execute`, key `details`) AND a test for the blast-radius guard error. Do NOT rely solely on live happy-path.
 - [ ] **Consistent verb** — destructive removal uses `delete` (never `remove`); see `.agents/KEY-DECISIONS.md`.

When you add or change a destructive command, re-read this checklist during the Pre-Commit Self-Review and confirm each box in your own review notes. A destructive command missing any box is INCOMPLETE.

### Automated guardrail audit (deterministic — `tests/e2e_destructive_guardrails.rs`)

Two tests enforce the guardrail invariant across ALL `destructive: true` commands in `commands.json` (currently 164), so a new destructive command that forgets a guard is caught mechanically:

- **`destructive_commands_are_annotated_mutates`** (runs in CI, fast, hermetic) — asserts every `destructive: true` subcommand is ALSO `mutates: true` (a destructive op must mutate state). Zero false positives; a metadata misannotation fails the suite.
- **`destructive_commands_dry_run_never_mutates`** (`#[ignore]` — exercise on demand with `cargo test --test e2e_destructive_guardrails -- --ignored`) — spawns the binary once per destructive command with auto-generated dummy required args + `--dry-run` and asserts the **core safety invariant**: a destructive command run under `--dry-run` must NEVER *succeed* (exit 0) without a dry-run marker (`"dry_run":true` or `"status":"dry_run"`). It either returns a dry-run preview (before any network call, or after a read-only expansion) or fails fast on input validation / auth — it must never complete a mutation. It runs with a dummy static token so no interactive auth is attempted (read-modify-write commands get a fast 401, still a non-zero exit). Both read the destructive set + required flags directly from `commands.json`, so the audit scope grows automatically with the CLI. Last full run (Aug 2026): **164 destructive commands, 0 metadata gaps, 0 runtime gaps** — the guardrail stack is complete.
- **`destructive_dry_run_previews_carry_confirm_signal`** (`#[ignore]`) — spawns each destructive command with `--dry-run` AND an AI-agent env var set, and asserts that whenever the dry-run guard fires offline (exit 0 with a dry-run marker) the preview carries BOTH `"destructive":true` AND an `agentNotice` (the confirm-with-user signal). This validates the destructive dry-run notice end-to-end and catches a command that passes a NON-canonical operation string to `dry_run_guard` (which would silently drop the marker — it caught `capacity delete`/`connection delete`/`connection delete-role-assignment` using descriptive sentences instead of their `"<group> <subcommand>"` key). Read-modify-write commands whose guard fires only after a network read get a fast 401 offline (no marker) and are covered instead by the deterministic unit test `agent::tests::every_destructive_command_is_recognized`.

### How agent safety notices work:

There are TWO complementary agent-notice paths:

**(a) Error path — safety-bypass hint.** When ALL of the following are true, the ERROR output includes an `agentNotice` field:
1. The error has a `hint` field
2. The hint text contains a flag from `DANGEROUS_FLAGS` (e.g., `--force`, `--hard-delete`)
3. An AI agent is detected via environment variables (see `AGENT_ENV_VARS` in `src/agent.rs`)

The notice warns the agent: *"do not retry with the safety-bypass flag suggested above unless the user has explicitly approved it."*

**(b) Dry-run path — destructive preview.** When a DESTRUCTIVE command is previewed with `--dry-run`, `output::dry_run_guard` annotates the preview with `"destructive": true`, and — when an AI agent is detected — an `agentNotice` telling the agent to *"confirm with the user before re-running it without --dry-run."* Destructiveness is read from `commands.json` (`agent::is_destructive_operation`, keyed by the `"<group> <subcommand>"` string passed to the guard), so the signal covers the FULL destructive command surface automatically — a new destructive command is covered the moment it is annotated `destructive: true` (no per-command wiring). This is why every destructive command MUST pass its canonical `"<group> <subcommand>"` key to `dry_run_guard` (NOT a descriptive sentence) — a mismatched key silently drops the destructive marker. Two deterministic tests enforce this: `agent::tests::every_destructive_command_is_recognized` (every `destructive: true` command is recognized by `is_destructive_operation`) and `tests/e2e_destructive_guardrails.rs::destructive_dry_run_previews_carry_confirm_signal` (`#[ignore]` — every destructive command whose guard fires offline emits `"destructive":true` + `agentNotice`; catches a command that passes the wrong operation-string key).

### Example output with agent notice (error path):

```json
{"error":{"code":"INVALID_INPUT","message":"Output directory is not empty: /tmp/export","hint":"Use --overwrite to replace existing content.","agentNotice":"Note for AI agents (Claude Code): do not retry with the safety-bypass flag suggested above unless the user has explicitly approved it. The flag bypasses a safety check and the operation may be irreversible."}}
```

### Example destructive dry-run preview (dry-run path):

```json
{"data":{"dry_run":true,"would_execute":"item delete","details":{"id":"..."},"hint":"Remove --dry-run to execute this operation.","destructive":true,"agentNotice":"Note for AI agents (Claude Code): this is a destructive, potentially irreversible operation. Confirm with the user before re-running it without --dry-run."}}
```

### Example deploy output with destructive field:

```json
{"data":{"status":"dry_run","summary":{"create":1,"delete":3,"skip":2},"destructive":true,"warnings":["--force-all is active: ALL matched items will be overwritten regardless of content changes. This is irreversible."]}}
```

## Tenant-Feature Gates (MANDATORY)

Many Fabric features are gated by a **tenant setting** an admin can toggle; when a setting is disabled the API returns an opaque `403 FeatureNotAvailable`. fabio turns this into an **admin-aware teaching error** generically via `src/commands/tenant_gate.rs`, wired ONCE into `commands::execute`. Two layers:

- **Automatic (no wiring)** — detection (`is_feature_disabled`, marker-based) + the admin probe (`is_fabric_admin`) + a generic admin-aware hint fire for ANY command that returns a feature-disabled error. A brand-new command gated by a brand-new setting is therefore NEVER an opaque failure, even with zero changes.
- **Opt-in per feature (one registry row)** — naming the EXACT setting + feature-specific fallbacks requires an entry in `setting_for_command`, because the API never reveals WHICH setting is disabled (there are ~169; the command→setting mapping is knowledge only fabio has).

**When you add (or discover) a fabio command/feature gated by a tenant setting, you MUST:**

1. **Find the exact `settingName`** — run `fabio admin list-tenant-settings` and match on the `title`. Do NOT guess the name; a wrong name yields a misleading enable command.
2. **Verify it actually gates the fabio REST command** — disable it (`fabio admin update-tenant-setting --setting-name <NAME> --content '{"enabled": false}'`), run the command, confirm it returns a `FeatureNotAvailable`-family 403, then RE-ENABLE it. Many settings gate only the portal UI and do NOT affect the REST path (e.g. `ExportToImage` gates the portal's "Export to image", NOT `report export` — verified live). If it doesn't gate the REST command, do NOT add a row.
3. **Add a registry row** to `setting_for_command` in `src/commands/tenant_gate.rs` — map the command path (`group.subcommand` for a specific command, or `group` for a whole preview-item family) → `{name, title, fallback?}`. Add a `fallback` only when there's a meaningful non-gated alternative (like the PowerBI-MCP `semantic-model query --dax` fallbacks).
4. **Extend the unit test** `registry_maps_known_commands` in `tenant_gate.rs` to assert the new mapping.
5. **If the API returns a NOVEL feature-disabled phrasing** (not `FeatureNotAvailable` / `TenantSwitchDisabled` / "not enabled in the tenant" / "tenant setting … disabled"), add the new marker to `is_feature_disabled` AND a detection unit test, so it is caught by Layer 1.

Currently registered (verified live via a tenant-settings dump): `PowerBIMCP` (semantic-model generate-dax/copilot-schema, report copilot-metadata), `ArtifactDatabricksStoragePreview` (azure-databricks-storage), `OntologyPreview` (ontology), `DigitalOperationsPreview` (digital-twin-builder), `ArtifactMirroredCatalogPreview` (mirrored-catalog), `AppBackendTenant` (app-backend), `AllowExternalDataSharingSwitch`/`AllowExternalDataSharingReceiverSwitch` (item external-data-share / accept), `PublishToWeb` (report publish-to-web). Live-validated on `PowerBIMCP` (subcommand-mapped) and `ArtifactDatabricksStoragePreview` (group-mapped). NEVER add an unverified mapping — an unverified row is worse than none (it names the wrong setting); the generic Layer-1 hint already covers the un-registered case actionably.

Agent discovery of this behavior is served by the `tenant-feature-gates` best-practice (`fabio context best-practices tenant-feature-gates`, also surfaced via `context find` and referenced from the `admin`/`bi` sub-skills' `shared_references`) — it teaches agents to read the named setting from a feature-disabled `FORBIDDEN`, enable it (admin) or ask an admin, and NOT to blindly retry.

## Command File Structure (MANDATORY)

Any command module that exceeds **1500 lines of code** MUST be refactored into a directory module with one file per subcommand group. Follow the pattern established by `context/`, `deploy/`, and `lakehouse/`:

```
src/commands/<command>/
├── mod.rs          — Subcommand enum, execute() dispatch, shared helpers
├── <subcommand_a>.rs  — Handler for one subcommand (or small cohesive group)
├── <subcommand_b>.rs  — Handler for another subcommand
└── ...
```

**Rules:**
- `mod.rs` contains the `<Command>Command` enum, the `execute()` dispatch function, and any helpers shared across submodules.
- Split by **subcommand**, not by abstract concern. Each file maps directly to one or a small group of related subcommands (e.g., `iceberg.rs` for all iceberg-* subcommands, `sync.rs` for the sync subcommand, `crud.rs` for list/show/create/update/delete).
- Functions called from `execute()` are `pub(super)`. Internal helpers stay private.
- Embedded data files (JSON schemas, templates) go in a `data/` subdirectory within the module.
- When adding new subcommands to an existing directory module, place the handler in the appropriate submodule file — do NOT add it to `mod.rs`.
- When a single-file command grows past 1500 lines, split it proactively rather than waiting for the next feature addition.

**Current directory modules:** `context/` (7 files), `deploy/` (12 files), `lakehouse/` (10 files), `warehouse/` (7 files), `item/` (6 files), `ontology/` (5 files: `mod`, `crud`, `definitions`, `import`, `mcp`), `git/` (5 files: `mod`, `sync`, `connect`, `branch_out`, `relation`).

**Scope:** This rule applies only to `src/commands/` source files. E2E test files (`tests/e2e_*.rs`) are NOT subject to the 1500-line limit — a single test file per command group is the preferred structure.

## Reusable Abstractions — Don't Duplicate (MANDATORY)

fabio has ~80 command groups that share a small number of highly-repeated shapes. **Before hand-writing a handler body, use the shared helper.** Reintroducing a copy of one of these patterns is a review-blocking regression. Check for an existing abstraction first; if you find a NEW pattern repeated ≥3 times, extract a helper rather than copy-pasting.

- **MCP clients → `src/mcp_client.rs` (`McpClient`) is the ONE transport.** Never hand-roll an MCP streamable-HTTP client (initialize / `tools/list` / `tools/call` / JSON-or-SSE parsing / `Mcp-Session-Id`). Consumers: `ontology`, `powerbi_mcp`, `sql_mcp`, `kql_database/mcp`, `reflex_mcp`, `dataagent/mcp`. Use `McpClient::connect`/`connect_with_timeout` (pass a timeout for calls that can run minutes), `list_tools`, `call_tool` → `ToolResult` (`.text()`, `.content`, `.is_error`, `.raw` = full result incl. `structuredContent`), and `primary_tool_argument(tool)` for the single-tool "bind a question to the tool's primary input property" pattern. `data-agent mcp-url`/`warehouse mcp-url`/etc. only PRINT a URL — they are not clients.
- **Item-type CRUD → `src/commands/crud.rs`.** Definition-backed item modules (`map`, `plan`, `reflex`, `notebook`, …) delegate `list`/`show`/`create`/`update`/`delete`/`get_definition`/`update_definition` to `crud::*`, parameterized by op-name group, collection segment, role, and (for definitions) the part filename. A new pure definition-backed item type's handlers should be one-line delegations (see `map.rs` — fully delegated). Only write a bespoke handler when the type genuinely differs (extra create fields like `folderId`/`creationPayload`, cascade deletes, custom URL builders, per-type hints).
- **List rendering → `output::render_item_list`.** It appends the `SENSITIVITY LABEL` and `TAGS` columns automatically when present. Never re-add the old 4-way `(has_labels, has_tags)` match.
- **T-SQL statistics (warehouse ↔ sql-database) → `src/commands/tds_stats.rs`.** Pass a **lazy** `(server, database)` resolver closure (invoked only AFTER the dry-run guard, so `--dry-run` never opens a connection) + the backend label. Warehouse resolves via `warehouse::resolve_connection`; sql-database via `sql_database::query::resolve_sql_connection`.
- **On-demand job run+poll+cancel → `src/commands/item_job.rs` (`run_and_wait` + `RunSpec`).** Used by `copy-job`/`data-pipeline`/`spark-job-definition` `run`. The `jobId`+custom-render variants (`notebook`/`data-build-tool-job`/`dataflow`) are not yet unified.

**When you DO extract a new shared helper:** keep the dry-run guard BEFORE any network call (pass a lazy resolver/closure if resolution is needed), rebuild the canonical `"<group> <subcommand>"` op-name string INSIDE the helper (so the destructive dry-run marker + `agentNotice` still fire — see the guardrail section), and add `#[allow(clippy::too_many_arguments)]` (or a small config struct) rather than dropping parameters. File inventory of these helpers is in `.agents/RELEVANT-FILES.md`; the rationale is in `.agents/KEY-DECISIONS.md`.

## Pre-Commit Validation (MANDATORY)

Before committing ANY change, you MUST run the following validation steps in order and ensure they all pass with zero errors and zero warnings:

For a complete step-by-step checklist (including self-review, documentation checks, and safety verification), invoke the skill: `.agents/skills/dev-pr-checklist/SKILL.md`

```bash
# 1. Format check (must produce no diffs)
cargo fmt -- --check

# 2. Clippy with all tests and deny warnings (must produce zero warnings)
cargo clippy --tests -- -D warnings

# 3. Run tests (must all pass)
cargo test
```

**Local pre-commit hooks (prek):** The project uses [prek](https://prek.j178.dev) — a fast, Rust-native pre-commit runner configured in `prek.toml`. When installed (`cargo install prek && prek install`), it automatically enforces format and lint checks on every `git commit`. The hooks run: trailing-whitespace fix, EOF fixer, TOML/YAML validation, merge-conflict detection, large-file guard (500KB), gitleaks secret scanning, `cargo fmt -- --check`, and `cargo clippy --tests -- -D warnings`. Tests (`cargo test`) are NOT included in the hook (too slow for interactive commits) — run them manually before pushing.

**Rules:**
- Do NOT commit if any of these steps fail.
- If prek is available, always let the hooks run on commit. If they reject the commit, fix the issues before retrying. Do NOT bypass hooks with `--no-verify`.
- Fix all formatting issues (`cargo fmt` to auto-fix), clippy warnings, and test failures before committing.
- If you add new code, ensure it has no clippy pedantic+nursery warnings.
- If you modify existing tests or add new tests, verify they pass.
- Check for unused imports before committing. Clippy catches these (`unused_imports` lint), but proactively remove any `use` statements you added that are no longer needed after refactoring. Run `cargo clippy --tests -- -D warnings` and fix all `unused import` warnings — do not leave them for a follow-up commit.
- These steps mirror the CI pipeline — if they pass locally, CI will pass. The release workflow (`release.yml`) additionally runs a `validate` job (fmt + clippy + full `cargo test`, including the skills/context consistency gates and the CLI-invariant gate `no_subcommand_flag_collides_with_global`) that ALL build/publish jobs depend on, so a tagged release cannot produce artifacts unless the exact tagged commit passes the suite.

## Pre-Commit Self-Review (MANDATORY)

Before committing, you MUST perform a deep, thoughtful review of ALL changes you are about to commit. This is not a formality — it is a critical quality gate:

1. **Re-read every changed file** — Use `git diff --staged` (or `git diff` if not yet staged) and carefully review each hunk.
2. **Check for issues you may have introduced** — Look for:
   - Logic errors, off-by-one mistakes, or incorrect assumptions
   - Missing error handling or edge cases
   - Copy-paste errors (e.g., wrong variable names, leftover placeholder text)
   - Inconsistencies with existing code patterns and conventions
   - Dead code, unused imports, or debug artifacts left behind
   - Incomplete implementations (TODO comments without corresponding work)
   - Naming inconsistencies (does the new code match the codebase's naming style?)
3. **Verify correctness against the intent** — Does the code actually accomplish what was requested? Are there subtle misunderstandings?
4. **Fix any issues found** — Do NOT commit known problems. Fix them first, then re-run the pre-commit validation steps.

**Rules:**
- Treat this review as if you were reviewing someone else's code — be critical and objective.
- If you find even a minor issue, fix it before committing. Do not leave it for later.
- This step comes AFTER pre-commit validation passes but BEFORE the actual `git commit`.

## Pre-Push Validation (MANDATORY)

Before pushing changes to the remote, you MUST run the cross-target lint to catch platform-specific issues (Windows/macOS quirks, conditional compilation errors, and target-dependent lints):

```bash
./scripts/cross-check.sh
```

**Rules:**
- Do NOT push if the cross-check script fails.
- Fix any cross-compilation errors (e.g., `cfg(windows)` blocks, platform-specific imports, path handling) before pushing.
- You can target a single platform to iterate faster: `./scripts/cross-check.sh --target windows-x64`
- This catches issues that local clippy/tests miss: Windows-only code paths (`windows-sys`, `windows` crates), macOS Darwin targets, and ARM64 variants.
- **The script runs the EXACT CI lint (`cargo clippy --tests -- -D warnings`) per target, not just `cargo check`.** This is required to catch lints whose verdict is target-dependent — most notably `clippy::large_futures`, whose async-state-machine size differs by platform (OS handle/socket types, TLS backend state, `cfg(windows)` fields, wider path/`OsString` layouts). A future just under the threshold on Linux can exceed it on `windows-msvc`/`apple-darwin` and fail there only. See the "large future" note below.

### `clippy::large_futures` (target-dependent lint)

An `async fn` compiles to a state machine whose size = the live locals across `.await` points (including the sizes of nested futures). That size is **target-dependent**, so an oversized future can trip `clippy::large_futures` on `windows-msvc`/`apple-darwin` while the host Linux clippy passes.

- **Local early warning:** `clippy.toml` sets `future-size-threshold = 16000` (below clippy's 16384 default) so the host clippy (and the pre-commit hook) trips on oversized futures with ~384 bytes of headroom before they reach the Windows/macOS CI matrix. `clippy.toml` is repo-global, so it also applies in CI — keep it clean on ALL targets (verify with `./scripts/cross-check.sh`), not just the host.
- **The fix is to `Box::pin` the oversized future** to move it off the caller's stack frame onto the heap. Prefer boxing at the single **leaf** helper that builds the big future (e.g. the TDS/tiberius connect+execute path in `src/commands/tds_utils.rs` wraps its body in `Box::pin(async move { … }).await`) — that shrinks every transitive caller at once, rather than boxing many scattered call sites.
- If you lower the threshold further you MUST box every newly-flagged future on the strictest target (Windows), or CI will fail.

## Git History & Merge Strategy (MANDATORY)

Keep `main` history **linear**. NEVER create merge commits.

- **Integrate branches by rebasing, then fast-forwarding** — bring a branch's commits on top of the base (`git rebase main` on the branch, then fast-forward `main` to it). Never `git merge --no-ff`, never a merge commit.
- **Update a feature branch with `git rebase main`**, not `git merge main` into the branch. Do not create "Merge branch 'main' into …" commits.
- **Merge PRs with "Rebase and merge" or "Squash and merge"** — never "Create a merge commit". Keep the GitHub repo setting for merge commits disabled.
- **Never force-push shared branches** (`main`, release branches). Rebasing your own feature branch before it is merged is fine.
- Keep commits focused and Conventional-Commit style (see the Pre-Commit sections); a clean linear history is part of the deliverable.

## Agent Knowledge Architecture (MANDATORY READING)

fabio's agent-facing knowledge is organized as a layered information architecture, inspired by microsoft/skills-for-fabric's Agents→Skills→Common model but implemented the fabio way: **authored judgment lives in data files; every mechanical index is generated from the source of truth (`commands.json`), so nothing drifts from the CLI.** When adding knowledge for agents, put it in the correct layer — do NOT hand-write command lists into markdown.

### The layers (highest-level routing → deepest mechanics)

| Layer | Purpose | Where it lives | Served by | Generated? |
|-------|---------|----------------|-----------|------------|
| **L1 — Personas** (orchestrators) | Route a *role/broad task* to command groups + workflows + best-practices; decision gates, guardrails, negative routing | `src/commands/context/data/personas/*.json` | `fabio context persona <name>` | Authored (auto-registered by `build.rs`) |
| **L1 — Disambiguations** | Resolve an *overloaded term* to the concrete artifact + command group | `src/commands/context/data/disambiguations/*.json` | `fabio context disambiguate <term>` | Authored (auto-registered) |
| **L2 — Sub-skills** (intent-scoped) | Focused per-workload guidance (judgment + command index) for progressive disclosure | judgment: `src/commands/context/data/skills/*.json`; output: `.agents/skills/fabio-*/SKILL.md` | loaded as agent skills; `context agent --group` | **Generated** (`skillgen.rs`) from judgment + `commands.json` |
| **L3 — Mechanics** | The primitives sub-skills/personas point at | `data/{workflows,best_practices,examples,schemas}/*.json` + clap | `context {agent,describe,workflow,best-practices,examples,schema,skill,find}` | `commands.json` generated from clap; rest authored |
| **Root skill** | Cross-cutting entry point: install, auth, output envelope, global flags, safety, disambiguation quick-ref, routing to L1/L2 | `.agents/skills/fabio/SKILL.md` | loaded as the primary agent skill | Hand-authored |

### The "common" layer

skills-for-fabric's `common/*.md` shared references map to fabio's **best-practices** (`context best-practices <topic>`: throttling, pagination, lro, admin-apis, deploy-parameters, shortcuts, variable-libraries, migration-api-shims, etc.). Sub-skills deep-link to the relevant topics via their `shared_references` field (the generator renders each topic's own `summary`, so the link text is drift-free).

### Division of labor (the core rule)

- **Judgment** (when-to-use, gotchas, safety, routing, must/prefer/avoid, troubleshooting) → authored in JSON data files.
- **Mechanics** (command names, flags, types, mutability) → generated from `commands.json` (itself generated from clap).
- A sub-skill = authored judgment JSON **+** generated command index. Never hand-write the command table.

### Where to add new agent knowledge

| You want to… | Do this |
|--------------|---------|
| Route a new *role* (e.g. "ml-engineer") | Add `data/personas/<name>.json` |
| Resolve a new *ambiguous term* | Add `data/disambiguations/<term>.json` |
| Add a focused *workload sub-skill* | Add `data/skills/<family>.json`, then `cargo test generate_subskills -- --ignored` |
| Add a *multi-step recipe* | Add `data/workflows/<name>.json` |
| Add *cross-cutting operational guidance* | Add `data/best_practices/<topic>.json` (then reference it from the relevant sub-skills' `shared_references`) |
| Document a *response shape* | Add `data/examples/<group>_<cmd>.json` + register in `examples.rs` |
| Add an *item definition schema* | Add `data/schemas/<type>.json` |

All of `data/{personas,disambiguations,skills,workflows,best_practices}/` are auto-registered by `build.rs` (drop a file + rebuild). `examples/` and `schemas/` require an `include_str!` registration. After ANY command/subcommand/flag change, regenerate `commands.json` AND the sub-skills (their command index would otherwise drift) — see the one-liner below. All layers are searchable via `fabio context find`.

## Auto-Generated Files (MANDATORY)

The following files are auto-generated from the CLI source of truth. **NEVER edit them manually** — edits will be overwritten on regeneration and drift detection tests will fail in CI.
### Regeneration Commands

After adding, modifying, or removing commands/flags, run ALL of these:

```bash
# 1. Regenerate commands.json (the single source of truth for all agent-facing metadata)
cargo test generate_agent_schema -- --ignored

# 2. Verify drift detection passes (these run in cargo test / CI)
cargo test agent_schema_covers
```

### File Inventory

| File | Generated from | Drift test | When to regenerate |
|------|---------------|------------|-------------------|
| `src/commands/context/data/agent/commands.json` | clap metadata | `agent_schema_covers_all_groups`, `agent_schema_covers_all_subcommands` | New command/subcommand/flag added |
| `.agents/skills/fabio-*/SKILL.md` (14 intent-scoped sub-skills) | `data/skills/*.json` (authored judgment) + `commands.json` (command index) | `subskills_match_generated` | New command/subcommand added, or a `data/skills/*.json` family edited |
| (consistency invariant — no file) | every CLI group ↔ a skill family (generates a subcommand table) or the cross-cutting allowlist | `every_command_group_has_a_knowledge_home` | New command GROUP added — give it a `data/skills/<family>.json` `command_groups` entry (so all its subcommands land in a generated sub-skill table) |
| `docs/src/content/docs/reference/commands/*.md` | `commands.json` via `docs/scripts/generate-reference.mjs` | none needed — gitignored and rebuilt on every `npm run build`/`dev`/`check` | Automatically on each docs build; never commit or hand-edit (the directory is gitignored) |

### How Drift Detection Works

`agent_schema_covers_all_groups` and `agent_schema_covers_all_subcommands` are unit tests that run in the standard `cargo test` suite (and in CI). They compare the actual clap CLI surface against the committed `commands.json` and fail with a clear message (including the regeneration command) if any group or subcommand is missing.

`every_command_group_has_a_knowledge_home` (in `skillgen.rs`) is a third consistency gate that enforces coverage down to the SUBCOMMAND level: a generated sub-skill command index (name + description + mutates for every subcommand) is produced for each SKILL FAMILY from its `command_groups`, so a group covered by a family has ALL its subcommands in a table. Personas do NOT generate tables (they are additive routing), so the gate requires every command group to be in a skill family (`data/skills/*.json` `command_groups`) OR the explicit cross-cutting/meta/core-infra allowlist inside the test (`auth, catalog, completions, context, feedback, item, jobs, mcp, operation, profile, rest, upgrade` — documented in the root skill, not a workload family). It also asserts the allowlist itself contains only real groups. This keeps skills + context from silently drifting behind a newly-added command group before a release — the manual audit, automated. To fix a failure, add the group to the right family's `command_groups` (then `cargo test generate_subskills -- --ignored`), or, only for a genuinely cross-cutting/core group, add it to the allowlist and the root skill.

`no_subcommand_flag_collides_with_global` (in `cli.rs`) is a fourth CLI-invariant gate — not a drift/regeneration check, but it likewise runs in the standard `cargo test` suite and the release `validate` job. It walks the entire clap command tree (`Cli::command()`, on a 32 MB-stack thread since the 77-group derive overflows the default 2 MB test-thread stack) and fails if any NON-global subcommand flag's long or short name shadows a `global = true` flag. This prevents the class of bug where clap lets a local `--query`/`--force`/`--all` coexist with the global and the global silently captures or transforms the value (see the "No subcommand flag may shadow a global flag" decision). To fix a failure, rename the local flag or drop it and read the global.

The `generate_agent_schema` test (`#[ignore]`) writes a freshly generated `commands.json` to disk — run it manually whenever commands change. It merges clap-derived structural data with the semantic annotations already in the file, so existing `mutates`, `returns`, `async`, `destructive`, `auth_scope`, and `examples` values are preserved.

**Intent-scoped sub-skills** (`.agents/skills/fabio-<family>/SKILL.md`): generated by `cargo test generate_subskills -- --ignored` (in `src/commands/context/skillgen.rs`, which is a `#[cfg(test)]`-only module). Each sub-skill pairs authored judgment (a `data/skills/<family>.json` file: `family`, `title`, `description`, `command_groups`, `when_to_use`, `when_not_to_use`, `must`/`prefer`/`avoid`, `key_gotchas`, `troubleshooting` (array of `{symptom, fix}`), `safety`, `shared_references` (best-practice topic names — the cross-cutting "common" layer, rendered with each topic's own summary), `see_also`) with a command index derived from `commands.json`. The generated sections follow skills-for-fabric conventions: a MUST/PREFER/AVOID behavioral triad and a Troubleshooting symptom→fix table. The `subskills_match_generated` drift test fails in CI if the committed files are stale. **NEVER edit `.agents/skills/fabio-*/SKILL.md` by hand** — edit the `data/skills/*.json` family file and regenerate. To add a new family, drop a `data/skills/<family>.json` (auto-registered by `build.rs`) and regenerate. Regenerate after ANY command/subcommand change (the command index would otherwise drift).

### One-Liner (Regenerate Everything)

```bash
cargo test generate_agent_schema -- --ignored && cargo test generate_subskills -- --ignored
```

## Documentation Updates (MANDATORY)

When adding new features, commands, or discovering API behaviors, you MUST update the following documentation before committing:

1. **The `.agents/*.md` reference files** (NOT `AGENTS.md` itself — see **Agent Context Hygiene** below):
   - **Key Decisions**: Append significant architectural or design choices to `.agents/KEY-DECISIONS.md`. Do NOT add them to `AGENTS.md` directly — the section was extracted to reduce context size.
   - **Relevant Files**: Add new source files, test files, or config files to `.agents/RELEVANT-FILES.md`.
   - **API Behaviors Discovered**: Append to `.agents/API-BEHAVIORS-DISCOVERED.md` under the appropriate section heading. Do NOT add API behavior documentation to `AGENTS.md` directly — it was extracted to reduce context size.

2. **`src/commands/context/agent.rs`** — Update the machine-readable command schema so AI agents can discover the new commands (flags, types, mutability, examples).

   **Auto-generation (preferred)**: Run `cargo test generate_agent_schema -- --ignored` to regenerate `commands.json` from clap metadata. This extracts group names, subcommand names, flag names/types/required/descriptions directly from the CLI definition. Semantic annotations (`mutates`, `returns`, `destructive`) are auto-inferred from command naming conventions (e.g., `list*` → read-only + returns list, `delete*` → mutates + destructive + returns void). Only `async` (LRO) and `auth_scope` (per-group) cannot be inferred and must be added manually for new entries that need them.

   **Drift detection**: Two unit tests (`agent_schema_covers_all_groups`, `agent_schema_covers_all_subcommands`) will FAIL if `commands.json` is missing any group or subcommand present in the actual CLI. These tests run as part of `cargo test` and prevent drift from accumulating.

   **NEVER manually edit `commands.json`** — The file at `src/commands/context/data/agent/commands.json` is auto-generated. Manual edits will be overwritten on the next regeneration. All structural data (groups, subcommands, flags, types, descriptions) comes from clap derive annotations in the source code. Only semantic annotations (`mutates`, `returns`, `async`, `destructive`, `auth_scope`) are preserved across regenerations via merge logic.

   **Exact steps when adding a new command or subcommand:**

   ```bash
   # 1. Write the command code with proper clap derive annotations
   #    (doc comments become descriptions, arg types become flag types)

   # 2. Regenerate commands.json from the actual CLI surface
   cargo test generate_agent_schema -- --ignored

   # 3. Add semantic annotations to the NEW entries only.
   #    Open src/commands/context/data/agent/commands.json and find your new
   #    subcommand(s). Add these fields that clap cannot infer:
   #
   #    "mutates": true/false       — does it change state?
   #    "returns": "list|object|void" — what shape is the output?
   #    "async": true               — (optional) is it an LRO?
   #    "destructive": true         — (optional) does it delete data?
   #
   #    For new command GROUPS, also set:
   #    "auth_scope": "fabric|storage|arm|mixed"

   # 4. Verify drift detection passes
   cargo test agent_schema_covers

   # 5. Done — the MCP server, --format mcp, --group, describe, find
   #    all automatically pick up the new commands with zero extra work.
   ```

   **`src/commands/context/`** — If the new feature introduces an item type, add a schema file in `context/data/schemas/`. If it's part of a multi-step workflow, consider adding a workflow recipe in `context/data/workflows/`. If the new feature adds significant query/output patterns, add output examples in `context/data/examples/` so agents understand the response shapes (e.g., new KQL intelligence commands like `list-entities`, `diagnostics`, `deeplink` should have representative output examples).

   **Output examples format** — Each example is a JSON file in `src/commands/context/data/examples/` with the structure: `{"command": "fabio ...", "description": "...", "response": {...}, "notes": "...", "query_examples": [...]}`. After creating the file, it MUST be registered in `src/commands/context/examples.rs` in the `OUTPUT_EXAMPLES` constant using `include_str!()`. Without registration, the example won't be discoverable via `fabio context examples <group> <command>`.

   **Best-practices registration** — Each best-practice is a JSON file in `src/commands/context/data/best_practices/` with required fields: `topic`, `title`, `summary` (for search discoverability), and topic-specific content. **Auto-registered**: the `build.rs` script scans this directory at compile time and generates the registration code. Just drop a `.json` file and rebuild — no manual `include_str!()` wiring needed.

   **Workflow registration** — Each workflow recipe is a JSON file in `src/commands/context/data/workflows/` with required fields: `name`, `description` (for search discoverability), `steps` (array of step objects). **Auto-registered**: same as best-practices — drop a `.json` file and rebuild.

   **Persona registration** — Each orchestrator persona is a JSON file in `src/commands/context/data/personas/` with required fields: `name`, `description` (for search discoverability), `delegates_to` (the request-type → command-group/workflow routing table). **Auto-registered** by `build.rs` — drop a `.json` file and rebuild. Served via `fabio context persona <name>`. Personas are thin routers (Layer 1) that delegate to command groups + workflows + best-practices; they hold no implementation depth.

   **Disambiguation registration** — Each disambiguation table is a JSON file in `src/commands/context/data/disambiguations/` with required fields: `term`, `summary` (for search discoverability), `meanings` (array of `{context, artifact, description, command_group}`). **Auto-registered** by `build.rs` — drop a `.json` file and rebuild. Served via `fabio context disambiguate <term>` (term lookup normalizes spaces, hyphens, and underscores). Resolves overloaded Fabric terms (e.g. "materialized view") to the right artifact + command group.

   **Discoverability via `fabio context find`** — Best-practices, workflows, personas, and disambiguations are automatically searchable via `fabio context find "<query>"` once registered. The search indexes names, descriptions/summaries, and full JSON content. No additional wiring is needed beyond placing the file in the correct directory.

   **Sub-skill judgment is retrievable AND searchable** — a family's authored judgment (`data/skills/<family>.json`: `key_gotchas`, `troubleshooting`, `must`/`prefer`/`avoid`, `safety`) is served two ways: `fabio context skill <family>` returns the raw judgment JSON (like `context persona`), and `fabio context find` indexes it, so a keyword that appears only in a gotcha (e.g. `queriesMetadata`, `auto-sync`, "calculated column") routes an agent to `context skill <family>` instead of the gotcha being visible only by loading the whole generated sub-skill. When you add a gotcha to a `data/skills/*.json`, it becomes discoverable with no extra wiring (the runtime module `src/commands/context/skills.rs` embeds every family via `build.rs`).

   **Release-time coverage audit (the judgment gap has no mechanical gate)** — the drift tests keep the MECHANICAL surfaces honest (every command is in `commands.json` and a generated sub-skill table), but a change that introduces a new *behavior/gotcha* an agent must know can silently skip the authored `context` knowledge. That is a judgment gap with no syntactic signal, so it is a heuristic release-time step: `scripts/audit-context-coverage.py` (run by the `dev-release` skill, defaults to `--since <latest tag>`) reports NEW subcommands not referenced in any context data (split non-CRUD/high-signal vs CRUD/low-signal), NEW teaching errors (`with_hint`/`with_typed_hint` — each often encodes a gotcha agents should discover proactively), and NEW `API-BEHAVIORS-DISCOVERED.md` section headings. Triage the report and add the missing `key_gotchas`/`example`/`best_practice` before releasing; `--strict` exits non-zero when a non-CRUD new command has no context aid.

   **Agent skills naming convention** — Skills in `.agents/skills/` follow a prefix convention to signal their audience:
   - `dev-*` — Contributor-only skills for working on fabio's source code (e.g., `dev-pr-checklist`, `dev-release`). These are only relevant when an agent has the fabio repo open.
   - `fabio` / `fabio-*` — User-facing skills that teach agents how to USE the fabio CLI. These are distributed externally via `fabio aitools install` and installed into agent config directories.
   - When adding a new skill, choose the prefix based on audience: does it help someone contribute a PR (`dev-`), or does it help someone use fabio as a tool (`fabio-`)?

3. **README.md** — Update the user-facing documentation:
   - Add new commands to the command listing/examples.
   - Update feature descriptions if capabilities have expanded.
   - Update installation or usage instructions if relevant.
   - GitHub Actions examples and agent safety documentation live here.

4. **Documentation website (`docs/`)** — Keep the published site in sync:
   - **Command reference is automatic**: the per-group pages under `docs/src/content/docs/reference/commands/` are regenerated from `commands.json` on every build (`generate-reference.mjs`). After a command/flag change, just regenerate `commands.json` (see Auto-Generated Files) — the reference follows. Do NOT hand-edit those pages (gitignored).
   - **Hand-authored pages CAN drift**: the tutorial, how-to guides, explanation pages, and `reference/global-flags.md` are authored by hand. If a change affects a global flag, an install method, an auth flow, or a documented workflow, update the relevant page — the auto-generated reference will NOT cover it. (Example: adding a global flag requires editing both `global_flags()` in `agent.rs` AND `docs/.../reference/global-flags.md`.)
   - **Validate before committing**: run `npm run check` in `docs/` (type-check + internal link validation via `check-links.mjs`) so a link to a renamed/removed page or command group fails fast.
   - See the **Documentation Website (MANDATORY)** section for build/deploy details.

**Rules:**
- Documentation updates are part of the feature — do NOT commit code without corresponding doc updates.
- API behaviors discovered during implementation MUST be captured in `.agents/API-BEHAVIORS-DISCOVERED.md` (this is critical institutional knowledge for future development).
- The `context agent` schema must stay in sync with the actual CLI surface — agents rely on it for discovery.
- The `docs` data files must be updated when new item types or workflows are added — agents rely on them for understanding definition formats and best practices.
- Output examples in `context/data/examples/` SHOULD be added for commands with non-obvious response shapes (e.g., nested objects, aggregated multi-section results, URL outputs) so agents can parse responses correctly.

## Testing Requirements (MANDATORY)

All new features, improvements, and bug fixes MUST have corresponding tests. This is non-negotiable — code without tests is incomplete code. Do NOT submit or consider work done until both unit tests and E2E tests are written, passing, and validated live.

1. **Unit tests** — Add unit tests in the same source file (or a `#[cfg(test)]` module) for:
   - New helper functions, parsers, or data transformations.
   - Edge cases in business logic (error paths, boundary conditions).
   - Output formatting and serialization.

2. **E2E tests** — Add integration tests in `tests/e2e_*.rs` for:
   - New CLI commands (verify structured output, exit codes, `--dry-run` behavior).
   - API interactions (create/read/update/delete lifecycle).
   - Error handling (invalid inputs, permission errors, not-found responses).

3. **Live tenant validation** — You have access to a live Microsoft Fabric tenant for E2E testing:
   - **ALWAYS run your new feature live against the tenant** before considering the work done. Do not skip this step.
   - Use `cargo run -- <command> ...` to execute against the real Fabric APIs and verify the feature works end-to-end.
   - Use the test env vars (`FABIO_TEST_SOURCE_WORKSPACE`, `FABIO_TEST_CAPACITY_ID`, etc.) for workspace/item references.
   - If env vars are not set in your session, use the values from `tests/common/mod.rs` or ask the user.
   - If a feature requires additional Azure resources (VNets, storage accounts, etc.), use `az cli` to create them as part of test setup.
   - Document any API behaviors discovered during testing in the appropriate AGENTS.md section.
   - Clean up any test resources you create (delete items, profiles, etc.) after validation.

**Rules:**
- Do NOT commit new commands or features without corresponding unit AND E2E tests.
- Do NOT consider a feature complete until it has been validated live against the tenant (not just dry-run).
- E2E tests should cover at minimum: `--dry-run` validation, happy-path execution, and error cases (invalid ID, missing permissions).
- Follow existing test patterns in `tests/common/mod.rs` and existing `tests/e2e_*.rs` files.
- Tests must pass locally (`cargo test`) before committing.

## Skill Quality Evaluation (Promptfoo)

The fabio user-facing skill (`.agents/skills/fabio/SKILL.md`) is quality-tested via [promptfoo](https://promptfoo.dev) — an LLM eval framework that validates whether an agent given the skill instructions produces correct CLI commands.

**Config:** `tests/eval/promptfooconfig.yaml` (153 test cases across 16 categories)

**Run locally:**
```bash
AZURE_OPENAI_API_KEY=$(az cognitiveservices account keys list \
  --name foundry-imejiauseche-ai-caglobal-demos --resource-group rg-imejiauseche-ai-demos \
  --query "key1" -o tsv) \
  promptfoo eval -c tests/eval/promptfooconfig.yaml
promptfoo view   # interactive results browser
```

### When to Add New Eval Cases

Add promptfoo test cases whenever you:

1. **Add a new command or subcommand** — Add at least one test verifying the agent produces the correct invocation with required flags.
2. **Add a new critical API behavior** — If a new quirk could cause silent failures (e.g., PascalCase values, specific flag requirements, format limitations), add a test proving the skill teaches it correctly.
3. **Add a new workflow pattern** — Multi-step operations (e.g., "create eventhouse, then create KQL DB inside it") need sequencing tests that verify correct dependency order.
4. **Discover a routing ambiguity** — If a prompt could be confused with another platform (e.g., "create a warehouse" could mean Snowflake or Fabric), add a routing discrimination test.
5. **Add or change safety flags** — New destructive flags (`--hard-delete`, `--force`, `--allow-delete-types`) need tests verifying the agent uses them correctly and ideally warns about consequences.
6. **Fix a skill gap** — If you discover the skill caused an agent to produce wrong output, add a regression test BEFORE fixing the skill, then verify it passes after.

### Test Categories and Assertion Patterns

| Category | When to use | Key assertion types |
|----------|-------------|---------------------|
| **Basic CRUD** | New command groups | `icontains` for command + required flags |
| **PascalCase compliance** | New enum-valued flags | `contains` (case-sensitive) for exact values |
| **Routing discrimination** | Ambiguous terms | `llm-rubric` checking skill does NOT suggest fabio |
| **Intra-Fabric routing** | Overloaded terms / broad tasks routed to the right group, persona, or disambiguation | `llm-rubric` (outcome-focused: routes to correct command group/artifact) + `javascript` group-name checks. Test the routing *outcome*, not that the model cites the persona/disambiguate helper. |
| **Multi-turn sequencing** | Multi-step workflows | `javascript` with `indexOf()` comparisons for ordering |
| **Error recovery** | New error codes/hints | `llm-rubric` + `icontains` for suggested fix |
| **Agent safety** | Destructive operations | `icontains` for flag presence + optional `llm-rubric` for warnings |
| **Scope validation** | Tenant vs workspace | `not-icontains: "--workspace"` for tenant-scoped commands |
| **LRO awareness** | Async operations | `icontains: "--wait"` + `icontains: "--timeout"` |
| **Output format** | Projection/format flags | `javascript` checking `-o table` or `--query` patterns |

### Writing Good Test Cases

```yaml
# Template for a new command test:
- description: "Category: short description of what's being tested"
  vars:
    user_query: "Natural language request that an agent would receive"
  assert:
    # Hard gate: command must be present
    - type: icontains
      value: "fabio <group> <subcommand>"
    # Hard gate: required flags
    - type: icontains
      value: "--required-flag"
    # Semantic check for nuanced behavior
    - type: llm-rubric
      value: "Description of what constitutes a correct response"
      metric: descriptive-metric-name
```

**Best practices:**
- Use `icontains` for command names and flags (case-insensitive, simple)
- Use `javascript` for ordering checks (`indexOf` comparisons) and multi-condition logic
- Use `llm-rubric` only when string matching cannot capture correctness (semantic judgment)
- Use `not-icontains` sparingly — only for routing discrimination (negative tests)
- Keep rubric descriptions objective and measurable (avoid "should ideally" — either it must or it shouldn't)
- The prompt template tells the model to omit `--wrap-untrusted` for test clarity; don't assert its presence
- Accept that `fabio item list --type X` is equivalent to `fabio <type> list` — both are correct
- Accept both `upload` + `load-table` (two-step) and `upload-table` (one-step) for data loading

### Known Pitfall: `--wrap-untrusted` Breaking String Assertions

The SKILL.md instructs agents to **always** include `--wrap-untrusted` in every fabio command. This means models may emit `fabio --wrap-untrusted workspace list` instead of `fabio workspace list`. An `icontains: "fabio workspace list"` assertion will FAIL because the flag is inserted between `fabio` and the subcommand.

**The fix:** The prompt template in `promptfooconfig.yaml` explicitly tells the model to omit `--wrap-untrusted` for test clarity. This avoids the mismatch. If you still encounter this issue (e.g., a model ignores the prompt instruction), use `javascript` assertions that check for the subcommand portion only:

```yaml
# BAD — breaks when model inserts --wrap-untrusted:
- type: icontains
  value: "fabio workspace list"

# GOOD — matches regardless of flags between 'fabio' and subcommand:
- type: javascript
  value: |
    output.includes('workspace list')
```

This pattern is required for any assertion where the model might insert global flags (`--wrap-untrusted`, `--profile`, `--output`) before the subcommand.

### Maintaining Pass Rate

The eval should maintain a high pass rate on gpt-5-mini (the CI eval model). If a new test consistently fails:
1. First verify the SKILL.md actually teaches the behavior being tested
2. If the skill is correct but the model doesn't emit it (e.g., safety warnings), relax the assertion to test capability rather than style
3. If the skill is missing the information, update SKILL.md first, then verify the test passes
4. Never commit a test that you know fails — either fix the skill or relax the assertion

## Release Workflow (MANDATORY)

The release workflow is documented in a dedicated skill: `.agents/skills/dev-release/SKILL.md`

Invoke the release skill when cutting a new version. It covers: version bump, dependency freshness, documentation updates, full validation, changelog generation, tagging, and post-release dev version bump.

Automated: `./scripts/release.sh <version>` handles all steps end-to-end.

### Documentation website & the release

The docs website's command reference is **generated** from `commands.json`
(via `docs/scripts/generate-reference.mjs`) and **auto-deploys** to GitHub
Pages through `.github/workflows/docs.yml` on every push to `main` touching
`docs/**` or `commands.json` — there is no manual publish step. The website
therefore stays current automatically **provided `commands.json` is
regenerated whenever commands/subcommands/flags change** (see **Auto-Generated
Files (MANDATORY)**). The release skill's Step 3 re-runs the generators and
relies on the `agent_schema_covers_*` / `subskills_match_generated` drift tests
(run in Step 4's `cargo test`) to hard-fail a release whose reference would be
stale. Locally, the `docs-scripts-test` prek hook runs the deterministic
`npm --prefix docs test` (reference generator + link-checker unit tests)
whenever `docs/**` or `commands.json` is staged; the heavier `astro check` /
link-validation / full build run in the docs CI workflow.

### Configuration

- `cliff.toml` — git-cliff configuration (commit parsers, grouping, template)
- `.github/RELEASE_TEMPLATE.md` — Narrative structure template
 - `scripts/release.sh` — Automated release script (version bump, changelog, tag, push, publish notes)

## Agent Context Hygiene (MANDATORY)

`AGENTS.md` is **auto-loaded into every agent session** (Copilot CLI, Claude Code, etc.)
as static context. It is a shared, finite budget: when it grows too large it crowds out
the actual conversation and agents fail before doing any work. This is not hypothetical —
the `sync-fabric-api-specs` CI job once aborted with *"Static context is using 94% of
available input tokens"* because an append-only log had bloated this file. Treat
`AGENTS.md` size as a hard operational constraint, not documentation polish.

**Rules — keep `AGENTS.md` lean and free of duplication:**

1. **`AGENTS.md` holds durable *rules and policy* only** — the things an agent must know on
   *every* turn (conventions, mandatory workflows, safety guardrails, where knowledge
   lives). It is NOT a changelog, a decision log, an API reference, or a file inventory.

2. **Append-only logs live in `.agents/*.md`, never inline here.** There is exactly one
   home for each kind of growing content — add to the file, never restate it in `AGENTS.md`:
   - Architectural/design **decisions & bug-fix rationale** → `.agents/KEY-DECISIONS.md`
   - **API/runtime behaviors, quirks, undocumented details** → `.agents/API-BEHAVIORS-DISCOVERED.md`
   - **Source/test/config file inventory** → `.agents/RELEVANT-FILES.md`
   In `AGENTS.md`, reference these files by pointer (as the sections below do); do NOT paste
   their content back in.

3. **Single source of truth — no duplication across the reference files.** A given fact
   lives in exactly ONE place. A `KEY-DECISIONS.md` bullet captures the *decision and its
   rationale* and cross-references the API detail in `API-BEHAVIORS-DISCOVERED.md` rather
   than repeating it. Before adding a bullet, check whether the fact already exists
   elsewhere and link to it instead of copying. Mechanical facts (command lists, flags,
   schemas) belong in the generated `commands.json` / context data, never hand-copied into
   prose.

4. **Size budget.** Keep `AGENTS.md` under **~25k tokens (~100 KB)**. If an edit pushes it
   past that, it almost certainly belongs in a `.agents/*.md` reference file — extract it
   and leave a pointer. When adding a new *rule*, prefer a tight bullet over a paragraph.

5. **When in doubt, ask "is this a rule every agent needs, or a record of something we
   did?"** Rules stay; records go to `.agents/*.md`.

## Key Decisions

Significant architectural and design decisions, bug fixes, and their rationale are
maintained in a separate file to reduce the auto-loaded agent context size:

**File:** `.agents/KEY-DECISIONS.md`

Reference this file when working on a specific command group or feature — search for
the relevant decision by keyword or command group name. Do NOT load the entire file
into context.

When you make a significant architectural or design decision, append a bullet to
`.agents/KEY-DECISIONS.md` (NOT here). Keep the bullet focused on the decision and its
rationale; deep API/runtime behavior detail belongs in
`.agents/API-BEHAVIORS-DISCOVERED.md`.

## Relevant Files

The full list of source files, test files, and config files is maintained in:

**File:** `.agents/RELEVANT-FILES.md`

Reference this file when looking up specific source locations or adding new files to the documentation.

## Documentation Website (MANDATORY)

The user-facing documentation site lives in `docs/` — an [Astro](https://astro.build) + [Starlight](https://starlight.astro.build) static site organized with the [Diátaxis](https://diataxis.fr) framework (Tutorials / How-to guides / Explanation / Reference). It is published to GitHub Pages and served from the custom apex domain at `https://ismaelmejia.com/fabio/`. Full-text search is provided by Pagefind (built in). A **blog** is provided by the [`starlight-blog`](https://starlight-blog-docs.vercel.app) plugin (registered in `astro.config.mjs`, posts authored under `src/content/docs/blog/*.md`, served at `/blog/` + `blog/rss.xml`).

### Structure

```
docs/
├── astro.config.mjs        — Starlight config: sidebar, base path, plugins
├── package.json            — npm scripts + pinned deps (exact versions, no ^)
├── scripts/
│   ├── generate-reference.mjs   — generates reference/commands/*.md from commands.json
│   ├── check-links.mjs          — dependency-free internal link validator
│   └── *.test.mjs               — node:test unit tests for the scripts
├── public/                 — static assets (favicon, images) served at site root
└── src/
    ├── pages/index.astro   — hand-authored landing page (own <html>, not Starlight)
    ├── styles/             — landing + docs CSS
    └── content/docs/
        ├── getting-started.md          — Tutorial
        ├── guides/*.md                 — How-to guides
        ├── explanation/*.md            — Explanation
        ├── blog/*.md                   — Blog posts (starlight-blog plugin)
        └── reference/
            ├── index.md, global-flags.md  — hand-authored reference
            └── commands/*.md              — GENERATED (gitignored), never edit
```

### Local development

All commands run from `docs/`:

```bash
npm install                 # first time
npm run dev                 # generate reference + start dev server
npm run build               # generate reference + production build to dist/
npm run check               # generate reference + astro type-check + internal link check
npm run check:links         # internal link validation only (check-links.mjs)
npm test                    # node:test unit tests for the generator + link checker
```

Requires Node 22.12+ (CI uses Node 24). `npm run build`/`dev`/`check` all run `generate:reference` first, so the reference is always fresh.

### Generated vs. authored (critical)

- **Generated (never hand-edit)**: `src/content/docs/reference/commands/*.md` — one page per command group, produced by `generate-reference.mjs` from `src/commands/context/data/agent/commands.json`. The directory is **gitignored** and rebuilt on every build. To change the reference, change the CLI (then regenerate `commands.json`) — see **Auto-Generated Files (MANDATORY)**.
- **Authored (hand-maintained, CAN drift)**: the tutorial, guides, explanation pages, `reference/index.md`, `reference/global-flags.md`, the landing page, and styles. These must be updated by hand when the CLI surface they describe changes (e.g. a new global flag → update `reference/global-flags.md`).

### Build validation

`npm run check` (also run in CI) enforces two gates:
1. `astro check` — TypeScript/Astro diagnostics.
2. `check-links.mjs` — validates every internal link in authored pages and the landing page resolves to a real route (authored page, generated command group from `commands.json`, or public asset). No network calls, no server — deterministic and cross-platform.

### Deployment

`.github/workflows/docs.yml` builds on every push/PR that touches `docs/**`, `commands.json`, or the workflow, and deploys to GitHub Pages **only on push to `main`**:
- **build job** (`docs-build-<ref>`, `cancel-in-progress: true`): `npm ci` → `npm test` → `npm run check` → `npm run build` → upload Pages artifact. Runs on PRs too (validation without deploy).
- **deploy job** (`group: pages`, `cancel-in-progress: false`): serializes deploys and never cancels one mid-flight.
- **Base path & origin**: `astro.config.mjs` reads `SITE_URL`/`BASE_PATH` env vars (set explicitly in `docs.yml` to `https://ismaelmejia.com` + `/fabio`, because the site is served from the custom apex domain under the `/fabio` subpath). These override the `GITHUB_REPOSITORY`-derived fallback (`https://<owner>.github.io/<repo>`) used for forks/ad-hoc builds, and the localhost fallback used for local dev. Keep `SITE_URL`/`BASE_PATH` in the workflow in sync with the deployed domain so canonical URLs and the sitemap are correct.
- **One-time prerequisite**: the repo's **Settings → Pages → Source must be set to "GitHub Actions"** (not "Deploy from a branch") or the deploy job fails.

### When and how to keep it updated

| Change | Action |
|--------|--------|
| Add/modify/remove a command, subcommand, or flag | Regenerate `commands.json` (see Auto-Generated Files). The reference pages follow automatically on the next build — no docs edit needed. |
| Add/change a **global flag** | Update `global_flags()` in `agent.rs` AND `docs/.../reference/global-flags.md` (both are hand-maintained). |
| Change an install method, auth flow, or documented workflow | Update the relevant authored page (`getting-started.md`, a guide, or an explanation page). |
| Add a new authored page | Add it under the correct Diátaxis directory; sidebar auto-generates for `guides/` and `explanation/`. Run `npm run check` to validate links. |
| Add a **blog post** | Create `src/content/docs/blog/<slug>.md` with `title`/`date`/`authors`/`excerpt`/`tags` frontmatter (the docs schema is extended with `blogSchema` in `content.config.ts`). Add new authors to the `authors` map in `astro.config.mjs`. The `/blog/` + `/blog/tags/` index routes are plugin-generated — `check-links.mjs` recognizes them automatically when a post exists. |

### Best practices (MANDATORY)

- **Never hand-edit generated pages** (`reference/commands/*.md`) — they are gitignored and overwritten.
- **Pin exact dependency versions** in `docs/package.json` (no `^`/`~`), matching the repo-wide freshness policy. Validate against npm before bumping.
- **SHA-pin all GitHub Actions** in `docs.yml` with a trailing version comment (repo-wide rule).
- **Use relative links in authored Markdown** (e.g. `../guides/agents/`), not root-absolute links — Astro does not rebase root-absolute Markdown links, so absolute links break under the `/fabio` base path. `check-links.mjs` validates relative resolution.
- **Run `npm run check` before committing** any `docs/` change; it mirrors CI.
- **Add unit tests** for any new/changed logic in `docs/scripts/*.mjs` (`node:test`, colocated `*.test.mjs`).

A contributor-facing quickstart also lives in `docs/README.md`.

## Docker & Devcontainer

### Production Docker Image

Published to GHCR on every push to `main` and on version tags:

```
ghcr.io/iemejia/fabio:latest       # latest stable release
ghcr.io/iemejia/fabio:0.69.0       # release version
ghcr.io/iemejia/fabio:0.69         # major.minor
```

Multi-arch manifest: `linux/amd64` + `linux/arm64`.

**Dockerfile** (root): Multi-stage build — compiles in Alpine (native musl) builder stage, copies to `FROM scratch` runtime with only CA certs (~8MB). Binary is fully static (zero runtime dependencies).

### Devcontainer

Located in `.devcontainer/` for VS Code and GitHub Codespaces. Provides the full development environment:

**System packages** (in Dockerfile): `build-essential`, `cmake`, `pkg-config`, `libssl-dev`, `musl-tools`, `lld`, `clang`, `zig 0.16.0`

**Devcontainer features**: Rust (with cross targets), Git, GitHub CLI, Azure CLI

**Cargo tools** (installed via `postCreateCommand`): `git-cliff`, `cargo-zigbuild`, `cargo-xwin`, `cargo-audit`

**Cross-compilation targets** (for `./scripts/cross-check.sh`): `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`

**VS Code extensions**: rust-analyzer, Even Better TOML, CodeLLDB debugger, Dependi (crate version checker)

**MANDATORY: Keep devcontainer in sync** — When adding a new Cargo dependency that requires system libraries or build tools (e.g., a `-sys` crate needing `cmake`, `libfoo-dev`, or a new linker), you MUST also update `.devcontainer/Dockerfile` to install the required package. The devcontainer must always be able to fully build fabio from source without additional manual setup.

### Docker CI Workflow (`.github/workflows/docker.yml`)

| Trigger | Build | Push to GHCR |
|---------|-------|--------------|
| `.devcontainer/**` or workflow change | devcontainer image | Yes (on push to `main`) |

`GITHUB_TOKEN` for GHCR auth (no extra secrets).

The release workflow (`.github/workflows/release.yml`) handles tagged version images (`:latest`, `:X.Y.Z`, `:X.Y`) as a separate `docker` job that uses pre-built binaries from the build job (no compilation in Docker).

### Relevant Docker Files

- `Dockerfile`: Production image (copies pre-built static binaries into `FROM scratch`, used by release workflow)
- `.devcontainer/Dockerfile`: Dev environment base image (Ubuntu + system deps + musl-tools + zig)
- `.devcontainer/devcontainer.json`: Features, extensions, cargo tools, cross targets
- `.github/workflows/docker.yml`: Devcontainer build + GHCR publish workflow

## API Behaviors Discovered

Runtime behaviors, quirks, and undocumented API details are documented in a separate file to reduce context size:

**File:** `.agents/API-BEHAVIORS-DISCOVERED.md` (2019 lines)

Reference this file when working on specific command groups. Do NOT load the entire file into context — search for the relevant section by command group name (e.g., "Lakehouse API Behaviors Discovered", "Deploy Command Design & Behaviors").

When discovering new API behaviors during implementation, append them to `.agents/API-BEHAVIORS-DISCOVERED.md` under the appropriate section heading.
