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
- **Windows-first compatibility** — All code must work on Windows. Use `Path::new().join()` (never hardcoded `/` for filesystem paths), `dirs::home_dir()` (never manual `HOME`/`USERPROFILE`), `.lines()` for text parsing (handles CRLF), no Unix-specific APIs. `.gitattributes` enforces LF line endings.
- **Throttling reduction** — Reduce the likelihood of API throttling by:
  - Use bulk and batch operations when available (e.g., `item bulk-create`, `item bulk-delete`, workspace role batch-assign, domain batch-assign).
  - Prefer list APIs over repeated single-resource requests (e.g., use a single list call + client-side filter rather than N individual show calls).
- CLI designed for AI agents first (structured output, no interactive prompts, explicit params)
- JSON output by default with `--output json|table|plain` flag
- Composable: manage inputs and produce outputs for piping
- Machine-readable error codes in structured JSON envelope
- Rust (edition 2024, rust-version 1.96), uses clap derive, tokio, reqwest, azure_identity, serde, serde_yaml, comfy-table, thiserror/anyhow
- Linting: clippy pedantic+nursery (zero warnings), rustfmt
- CI: GitHub Actions (cargo fmt, clippy, test, build release) on ubuntu/macos/windows
- Installable via `cargo install --git https://github.com/iemejia/fabio.git`
- **Dependency version freshness** — When introducing a new Cargo dependency or a new GitHub Action, always validate that you are using the most recent available and compatible version. Check crates.io for Rust crates and the action's repository releases/tags for GitHub Actions. Do NOT copy outdated versions from examples or memory — verify against the source of truth before adding. Additionally, reject any dependency with an incompatible license (GPL, LGPL, AGPL, SSPL, or any other copyleft license that would impose restrictions on the project). Only permissive licenses (MIT, Apache-2.0, BSD, ISC, Zlib, Unicode-3.0, etc.) are acceptable.
- **GitHub Actions pinning** — ALL GitHub Actions in `.github/workflows/*.yml` MUST be pinned to their full commit SHA with the version in a trailing comment. Format: `uses: owner/action@<40-char-sha> # v<major>` (or `# v<major>.<minor>.<patch>` for non-major tags). NEVER use floating tag references like `@v7` or `@stable`. This prevents supply-chain attacks where a tag is force-pushed to a compromised commit. When updating an action, always verify the new SHA matches the expected release tag from the action's repository.
- **Modern Rust idioms (MANDATORY)** — All code MUST leverage features available in the declared `rust-version` (currently 1.96). Do NOT write code using older patterns when a modern equivalent exists. When the MSRV is bumped, audit and migrate existing code. Key idioms to prefer:
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

3. **Dangerous flags must be in `DANGEROUS_FLAGS`** — If you add a new safety-bypass flag, add it to the `DANGEROUS_FLAGS` array in `src/agent.rs`. This ensures the agent safety notice fires when the flag is suggested in an error hint.

4. **Add `"destructive": true/false` to batch output** — For commands that produce a plan or summary of multiple actions (like `deploy plan/apply`), include a `"destructive"` boolean field in the structured output. Set to `true` when the operation includes deletions, overwrites, or other irreversible actions. Agents use this field to decide whether to ask the human for confirmation.

5. **Protected types require explicit opt-in** — Data-bearing item types (Lakehouse, Warehouse, SQLDatabase, Eventhouse, KQLDatabase) require `--allow-delete-types` for deletion. If you add support for a new data-bearing item type, add it to `PROTECTED_DELETE_TYPES` in `src/commands/deploy/mod.rs`.

6. **Warn on force/override modes** — When `--force-all`, `--force`, or similar override flags are active, emit a warning in the output explaining the irreversibility. This helps agents surface the risk to the human.

7. **Never add interactive prompts** — Fabio is non-interactive (Principle 1). Do NOT add `y/N` prompts or `--auto-approve` flags. Instead, use structured output signals (`"destructive": true`, warnings, `agentNotice`) that agents can programmatically evaluate.

### How agent safety notices work:

When ALL of the following conditions are true, the error output includes an `agentNotice` field:
1. The error has a `hint` field
2. The hint text contains a flag from `DANGEROUS_FLAGS` (e.g., `--force`, `--hard-delete`)
3. An AI agent is detected via environment variables (see `AGENT_ENV_VARS` in `src/agent.rs`)

The notice warns the agent: *"do not retry with the safety-bypass flag suggested above unless the user has explicitly approved it."*

### Example output with agent notice:

```json
{"error":{"code":"INVALID_INPUT","message":"Output directory is not empty: /tmp/export","hint":"Use --overwrite to replace existing content.","agentNotice":"Note for AI agents (Claude Code): do not retry with the safety-bypass flag suggested above unless the user has explicitly approved it. The flag bypasses a safety check and the operation may be irreversible."}}
```

### Example deploy output with destructive field:

```json
{"data":{"status":"dry_run","summary":{"create":1,"delete":3,"skip":2},"destructive":true,"warnings":["--force-all is active: ALL matched items will be overwritten regardless of content changes. This is irreversible."]}}
```

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

**Current directory modules:** `context/` (7 files), `deploy/` (12 files), `lakehouse/` (10 files).

**Scope:** This rule applies only to `src/commands/` source files. E2E test files (`tests/e2e_*.rs`) are NOT subject to the 1500-line limit — a single test file per command group is the preferred structure.

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
- These steps mirror the CI pipeline — if they pass locally, CI will pass. The CI release build is an additional artifact-packaging step, not a correctness gate.

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

Before pushing changes to the remote, you MUST run the cross-compilation check to catch platform-specific issues (Windows/macOS quirks, conditional compilation errors):

```bash
./scripts/cross-check.sh
```

**Rules:**
- Do NOT push if the cross-check script fails.
- Fix any cross-compilation errors (e.g., `cfg(windows)` blocks, platform-specific imports, path handling) before pushing.
- You can target a single platform to iterate faster: `./scripts/cross-check.sh --target windows-x64`
- This catches issues that local clippy/tests miss: Windows-only code paths (`windows-sys`, `windows` crates), macOS Darwin targets, and ARM64 variants.

## Auto-Generated Files (MANDATORY)

The following files are auto-generated from the CLI source of truth. **NEVER edit them manually** — edits will be overwritten on regeneration and drift detection tests will fail in CI.

### Regeneration Commands

After adding, modifying, or removing commands/flags, run ALL of these:

```bash
# 1. Regenerate commands.json (the single source of truth for all agent-facing metadata)
cargo test --bin fabio generate_agent_schema -- --include-ignored

# 2. Verify drift detection passes (these run in cargo test / CI)
cargo test --bin fabio agent_schema_covers
```

### File Inventory

| File | Generated from | Drift test | When to regenerate |
|------|---------------|------------|-------------------|
| `src/commands/context/data/agent/commands.json` | clap metadata | `agent_schema_covers_all_groups`, `agent_schema_covers_all_subcommands` | New command/subcommand/flag added |

### How Drift Detection Works

Each auto-generated file has a corresponding unit test that:
1. Regenerates the expected content in memory (same algorithm as the generator)
2. Reads the committed file from disk
3. Asserts they are identical
4. Fails with a message showing the exact regeneration command

These tests run as part of the standard `cargo test` suite and in CI. If a contributor adds a command but forgets to regenerate, CI will fail with a clear error message.

### One-Liner (Regenerate Everything)

```bash
cargo test --bin fabio generate_agent_schema -- --include-ignored
```

## Documentation Updates (MANDATORY)

When adding new features, commands, or discovering API behaviors, you MUST update the following documentation before committing:

1. **AGENTS.md** — Update these sections as applicable:
   - **Key Decisions**: Document significant architectural or design choices.
   - **Relevant Files**: Add new source files, test files, or config files.
   - **API Behaviors Discovered**: Append to `.agents/API-BEHAVIORS-DISCOVERED.md` under the appropriate section heading. Do NOT add API behavior documentation to AGENTS.md directly — it was extracted to reduce context size.

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

   **Discoverability via `fabio context find`** — Best-practices and workflows are automatically searchable via `fabio context find "<query>"` once registered. The search indexes topic names, descriptions/summaries, and full JSON content. No additional wiring is needed beyond placing the file in the correct directory.

   **Agent skills naming convention** — Skills in `.agents/skills/` follow a prefix convention to signal their audience:
   - `dev-*` — Contributor-only skills for working on fabio's source code (e.g., `dev-pr-checklist`, `dev-release`). These are only relevant when an agent has the fabio repo open.
   - `fabio` / `fabio-*` — User-facing skills that teach agents how to USE the fabio CLI. These are distributed externally via `fabio aitools install` and installed into agent config directories.
   - When adding a new skill, choose the prefix based on audience: does it help someone contribute a PR (`dev-`), or does it help someone use fabio as a tool (`fabio-`)?

3. **README.md** — Update the user-facing documentation:
   - Add new commands to the command listing/examples.
   - Update feature descriptions if capabilities have expanded.
   - Update installation or usage instructions if relevant.
   - GitHub Actions examples and agent safety documentation live here.

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

**Config:** `tests/eval/promptfooconfig.yaml` (106 test cases across 15 categories)

**Run locally:**
```bash
AZURE_OPENAI_API_KEY=$(az cognitiveservices account keys list \
  --name foundry-imejiauseche-ai-demos --resource-group rg-imejiauseche-ai-demos \
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

The eval should maintain **>95% pass rate** on gpt-4o-mini. If a new test consistently fails:
1. First verify the SKILL.md actually teaches the behavior being tested
2. If the skill is correct but the model doesn't emit it (e.g., safety warnings), relax the assertion to test capability rather than style
3. If the skill is missing the information, update SKILL.md first, then verify the test passes
4. Never commit a test that you know fails — either fix the skill or relax the assertion

## Release Workflow (MANDATORY)

The release workflow is documented in a dedicated skill: `.agents/skills/dev-release/SKILL.md`

Invoke the release skill when cutting a new version. It covers: version bump, dependency freshness, documentation updates, full validation, changelog generation, tagging, and post-release dev version bump.

Automated: `./scripts/release.sh <version>` handles all steps end-to-end.

### Configuration

- `cliff.toml` — git-cliff configuration (commit parsers, grouping, template)
- `.github/RELEASE_TEMPLATE.md` — Narrative structure template
- `scripts/release.sh` — Automated release script (version bump, changelog, tag, push, publish notes)

## Key Decisions
- JSON envelope always wraps output: lists get `{"data":[...],"count":N}`, objects get `{"data":{...}}`
- Errors on stderr as `{"error":{"code":"...","message":"..."}}` with non-zero exit
- `--query` supports full JMESPath expressions (see jmespath.org) — filter, project, slice, multiselect, pipe, functions (length, sort_by, etc.)
- `--quiet` suppresses all stdout; errors still go to stderr
- OneLake upload uses DFS create+append+flush 3-step pattern with `x-ms-content-md5` on flush (computes MD5 client-side, stores as file property for content-based matching)
- Notebook creation builds minimal .ipynb JSON, base64-encodes for Fabric API; `source` must be list of strings
- Item copy fetches definition from source via LRO, posts to destination workspace via LRO
- LRO polling: 2s default interval (respects `Retry-After` header, capped at 60s), 120s max wait, handles `Location`/`x-ms-operation-id` headers
- `post()` accepts `poll: bool` for LRO-aware operations
- Load-table requires PascalCase values (`"Overwrite"`, `"Csv"`) and `format` inside `formatOptions`
- **Load-table only supports Csv and Parquet**: The Fabric REST API `formatOptions` discriminated union only has `Csv` (with `header`/`delimiter`) and `Parquet` (format only). JSON is NOT supported — must convert to CSV/Parquet first. Sending CSV-specific fields (header, delimiter) with Parquet format causes API rejection.
- **SQL Database import**: Uses type inference with `Unknown` initial state → first non-empty observation sets the type, subsequent observations widen (Int→BigInt→Float→NVarChar, never narrows)
- **Server-side copy**: OneLake Blob API supports `PUT` with `x-ms-copy-source`; returns 202 with pending status. Poll via HEAD.
- **Atomic rename for same-item moves**: DFS `x-ms-rename-source` works within the same OneLake item (workspace + lakehouse). Works for both files and directories. Returns 201. Fails with 403 for cross-item/cross-workspace. Fallback: copy + delete.
- **Table file listing**: Must list from root (no `directory` param) to get real paths prefixed with item ID.
- **Recursive delete**: DFS `DELETE /{ws}/{lh}/Tables/{name}?recursive=true` works for directories.
- All destructive actions use consistent verb `delete` (not `remove`)
- Cross-workspace ops use `--source-workspace`/`--dest-workspace` with `visible_alias` short forms
- Auth relies on a multi-source credential chain: fabio cache (device code, browser PKCE, or service principal), environment variables, managed identity, Azure CLI, Azure Developer CLI
- `azure_identity`/`azure_core` with `default-features = false` (no OpenSSL dependency on Linux/macOS; OpenSSL for certificate auth via `client_certificate` feature)
- **Windows-first compatibility** — Token cache encrypted with DPAPI (`CryptProtectData`, user scope); WAM broker SSO via `--wam` flag
- `unsafe_code = "forbid"` in lints
- **KQL Queryset definition format**: Uses `RealTimeQueryset.json` (NOT `RawQueryset.kql`). JSON structure: `{"queryset":{"version":"1.0.0","dataSources":[{"id","clusterUri","type","databaseName"}],"tabs":[{"id","content","title","dataSourceId"}]}}`. The `content` field holds the KQL query text with `\n` for newlines.
- **KQL Queryset run**: Fetches definition via LRO, decodes `RealTimeQueryset.json`, selects tab by name or index, resolves data source (clusterUri + databaseName), executes via Kusto REST API. Tab selection is case-insensitive by title.
- **Deploy diff strategy**: Content hash vs live workspace (not git diff) — detects portal edits, works without git, idempotent convergence
- **Deploy parallelism**: Semaphore-bounded `tokio::spawn` per-item within type batch (default 8); sequential for DataPipeline; deletes always sequential
- **Deploy parameter format**: JSON (not YAML) — no extra crate dependency, agent-native consistency
- **Deploy plan staleness**: Workspace fingerprint = SHA256 of sorted `(id, type, name)` tuples; mismatch → error unless `--force`
- **Deploy logical ID resolution**: String replacement in base64 payloads; resolves items created earlier in same session
- **Deploy rename detection**: Two-pass matching — first by (type, name), then unmatched source items with logical IDs get candidates checked via `fetch_deployed_logical_id()` which reads `.platform` part from deployed item definition
- **Deploy creationPayload**: Separate `creationPayload.json` file in item directory; merged into creation body as `creationPayload` field; parameter substitution applied
- **Deploy post-hooks**: Opt-out via `--no-post-hooks`; hooks never fire during `--dry-run`; failures are non-fatal (reported in output, don't fail the deploy). SemanticModel → `POST /refreshes`, Environment → `POST /staging/publish`
- **Deploy empty definitions**: Items with no parts (Lakehouse, MLModel) omit `definition` field on create; skip `updateDefinition` on update
- **Deploy ordering**: 45 item types in `DEPLOY_ORDER`; deployed in dependency order (storage → compute → code → models → reactive → APIs → ML → graph → viz)
- **Deploy no state file**: Stateless — always queries live workspace. No `.tfstate` equivalent.
- **Deploy .platform in parts but excluded from hash**: `.platform` IS sent as a definition part (enables `?updateMetadata=true` for metadata propagation), but EXCLUDED from content hash (API rewrites `logicalId` in `.platform`, which would break idempotent skip detection)
- **Deploy workspace ID regex replacement**: Uses regex matching on `workspaceId`, `default_lakehouse_workspace_id`, `workspace` keys — not blanket string replacement. Skips shortcuts (handled separately with lakehouse GUID). Opt-out: `--no-workspace-id-replace`
- **Deploy config file (JSON + YAML)**: `--config <file> --env <name>` loads per-environment workspace/source/parameters; `serde_yaml` crate for YAML; CLI flags override config values
- **Deploy protected type deletion**: Lakehouse, Warehouse, SQLDatabase, Eventhouse, KQLDatabase require `--allow-delete-types` to be deleted by `--delete-orphans`
- **Deploy fabric-cicd full compatibility**: Source directory format, .platform file schema, definition parts, logical ID resolution, workspace ID replacement, creationPayload, .children/ discovery, .pbi/ exclusion, notebook ordering, Report byPath transform — all aligned with Microsoft's fabric-cicd Python library
- **Upgrade**: `fabio upgrade` downloads latest release from GitHub, verifies SHA256 checksum, extracts platform-appropriate archive (tar.gz on Unix, zip on Windows), atomically replaces running binary; supports `--check` (version query only), `--target-version` (pin specific version), `--force` (reinstall even if current), `--dry-run`
- **Context tenant LSP-inspired agent features**: Inspired by Language Server Protocol design, `context tenant` provides progressive disclosure for AI agents: `--summary-only` (cheap inventory probe, 2 API calls), `--resolve Type:Name` (fast name-to-ID lookup without graph), `--focus <id> --depth N` (ego-centric subgraph via BFS). All graph responses include a `meta` envelope (`scannedAt`, `scanDurationMs`, `etag` SHA-256 fingerprint, `partial`, `scope`) for freshness/drift detection. Edges carry `confidence` (high/medium/low) and `discoveryMethod` fields so agents can filter noise.
- **Item relations (beta)**: `fabio item list-upstream-relations`/`list-downstream-relations` call the new `GET /workspaces/{ws}/items/{id}/relations/{upstream|downstream}?beta=true` endpoints. Response is a graph fragment (`items`/`relations`/`workspaces`), not a paginated list — rendered via `render_object`, not `render_list_with_token`.
- **Lakehouse MLV execution definitions**: New CRUD group `fabio lakehouse {list,show,create,update,delete}-execution-definition(s)` at `/workspaces/{ws}/lakehouses/{id}/mlvexecutiondefinitions[/{defId}]`. Groups a `currentLakehouseExecutionContext`/`extendedLineageExecutionContext` (discriminated `All`/`Selected` unions) with optional Spark `environment` + `refreshMode` settings; referenced by materialized-lake-view refresh schedules via `executionData.mlvExecutionDefinitionId`.

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
- Fabric scope: `https://api.fabric.microsoft.com/.default`
- Storage scope: `https://storage.azure.com/.default`
- Spark rate limit on small capacity: LRO reports 430 `TooManyRequestsForCapacity` (non-standard code)
- Test env vars: `FABIO_TEST_SOURCE_WORKSPACE`, `FABIO_TEST_SOURCE_LAKEHOUSE`, `FABIO_TEST_DEST_WORKSPACE`, `FABIO_TEST_DEST_LAKEHOUSE`, `FABIO_TEST_NOTEBOOK_ID`, `FABIO_TEST_CAPACITY_ID`
- Fabric REST API specs (OpenAPI): `https://github.com/Azure/azure-rest-api-specs/` (look under `specification/fabric/`)


## Relevant Files

The full list of source files, test files, and config files is maintained in:

**File:** `.agents/RELEVANT-FILES.md`

Reference this file when looking up specific source locations or adding new files to the documentation.

## Docker & Devcontainer

### Production Docker Image

Published to GHCR on every push to `main` and on version tags:

```
ghcr.io/iemejia/fabio:latest       # latest stable release
ghcr.io/iemejia/fabio:0.33.0       # release version
ghcr.io/iemejia/fabio:0.33         # major.minor
```

Multi-arch manifest: `linux/amd64` + `linux/arm64`.

**Dockerfile** (root): Multi-stage build — compiles release binary in Ubuntu builder stage, copies to minimal runtime image (~52MB) with only `ca-certificates`.

### Devcontainer

Located in `.devcontainer/` for VS Code and GitHub Codespaces. Provides the full development environment:

**System packages** (in Dockerfile): `build-essential`, `pkg-config`, `libssl-dev`, `lld`, `clang`, `zig 0.16.0`

**Devcontainer features**: Rust (with cross targets), Git, GitHub CLI, Azure CLI

**Cargo tools** (installed via `postCreateCommand`): `git-cliff`, `cargo-zigbuild`, `cargo-xwin`, `cargo-audit`

**Cross-compilation targets** (for `./scripts/cross-check.sh`): `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`

**VS Code extensions**: rust-analyzer, Even Better TOML, CodeLLDB debugger, Dependi (crate version checker)

### Docker CI Workflow (`.github/workflows/docker.yml`)

| Trigger | Build | Push to GHCR |
|---------|-------|--------------|
| Pull request | amd64 + arm64 | No (validation only) |
| Push to `main` | amd64 + arm64 | No (validation only) |

Uses GitHub Actions cache (`type=gha`) for Docker layer caching. QEMU for arm64 cross-build. `GITHUB_TOKEN` for GHCR auth (no extra secrets).

The release workflow (`.github/workflows/release.yml`) handles tagged version images (`:latest`, `:X.Y.Z`, `:X.Y`) as a separate `docker` job that runs after binaries are published.

### Relevant Docker Files

- `Dockerfile`: Production multi-stage image (builder + minimal runtime)
- `.devcontainer/Dockerfile`: Dev environment base image (Ubuntu + system deps + zig)
- `.devcontainer/devcontainer.json`: Features, extensions, cargo tools, cross targets
- `.github/workflows/docker.yml`: Build validation + GHCR publish workflow

## API Behaviors Discovered

Runtime behaviors, quirks, and undocumented API details are documented in a separate file to reduce context size:

**File:** `.agents/API-BEHAVIORS-DISCOVERED.md` (2019 lines)

Reference this file when working on specific command groups. Do NOT load the entire file into context — search for the relevant section by command group name (e.g., "Lakehouse API Behaviors Discovered", "Deploy Command Design & Behaviors").

When discovering new API behaviors during implementation, append them to `.agents/API-BEHAVIORS-DISCOVERED.md` under the appropriate section heading.
