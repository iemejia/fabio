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
- Rust (edition 2024, rust-version 1.97.1), uses clap derive, tokio, reqwest, azure_identity, serde, serde_yaml, comfy-table, thiserror/anyhow
- Linting: clippy pedantic+nursery (zero warnings), rustfmt
- CI: GitHub Actions (cargo fmt, clippy, test, build release) on ubuntu/macos/windows
- Installable via `cargo install --git https://github.com/iemejia/fabio.git`
- **Dependency version freshness** — When introducing a new Cargo dependency or a new GitHub Action, always validate that you are using the most recent available and compatible version. Check crates.io for Rust crates and the action's repository releases/tags for GitHub Actions. Do NOT copy outdated versions from examples or memory — verify against the source of truth before adding. Additionally, reject any dependency with an incompatible license (GPL, LGPL, AGPL, SSPL, or any other copyleft license that would impose restrictions on the project). Only permissive licenses (MIT, Apache-2.0, BSD, ISC, Zlib, Unicode-3.0, etc.) are acceptable.
- **GitHub Actions pinning** — ALL GitHub Actions in `.github/workflows/*.yml` MUST be pinned to their full commit SHA with the version in a trailing comment. Format: `uses: owner/action@<40-char-sha> # v<major>` (or `# v<major>.<minor>.<patch>` for non-major tags). NEVER use floating tag references like `@v7` or `@stable`. This prevents supply-chain attacks where a tag is force-pushed to a compromised commit. When updating an action, always verify the new SHA matches the expected release tag from the action's repository.
- **Modern Rust idioms (MANDATORY)** — All code MUST leverage features available in the declared `rust-version` (currently 1.97.1). Do NOT write code using older patterns when a modern equivalent exists. When the MSRV is bumped, audit and migrate existing code. Key idioms to prefer:
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

### Standard guardrail stack for a NEW destructive command (MANDATORY checklist)

ANY new command/subcommand/operation that deletes data, overwrites content without backup, permanently replaces a definition, kills a running session/job, or is otherwise irreversible MUST ship with the SAME guardrail stack the existing destructive ops use (`item delete`, `lakehouse delete-directory`/`delete-table`/`delete-file`, `deploy apply --delete-orphans`, `warehouse queries-kill`, `data-agent delete --hard-delete`, `git relation delete`, `updateDefinition`, `reset`, `prune`, …). Before you consider the feature done, verify EVERY box:

- [ ] **`--dry-run` guard** — call `output::dry_run_guard(cli, "<group> <subcommand>", &preview)` and return early when it returns `true`, BEFORE any mutating network call. `preview` must describe exactly what would be affected (ids, paths, counts). Put the guard AFTER input-scope validation so a dry-run of an unsafe request still surfaces the validation error.
- [ ] **`--readonly` enforcement** — the mutation must route through a client method that calls `guard_readonly("<METHOD>", …)` (all `post`/`put`/`patch`/`delete` client helpers do). Never bypass the client for a raw mutating request.
- [ ] **`"destructive": true` in `commands.json`** — after `cargo test generate_agent_schema -- --ignored`, confirm the subcommand has `"destructive": true`. The generator only auto-infers this for `delete*`-named subcommands; for destructive ops with other names (`reset`, `kill`, `prune`, `update-definition`, `--hard-delete`, `--force*`, `--delete-orphans`) you MUST set it manually. Also set `"mutates": true`.
- [ ] **Blast-radius input guard** — if a malformed/empty/wildcard input could destroy far more than intended (e.g. an empty/root path recursively deleting an entire item — see `validate_delete_directory_path`; a glob matching everything; a missing filter deleting all rows), add a pure `validate_*` function that refuses the catastrophic case with a clear `FabioError::with_hint`, and unit-test it. Fail BEFORE any network call.
- [ ] **Safety-bypass flags** — if the operation is gated behind a bypass flag (`--force`, `--hard-delete`, `--allow-delete-types`, `--delete-orphans`, `--overwrite`, …), add the flag to `DANGEROUS_FLAGS` in `src/agent.rs` and surface it via `FabioError::with_hint()` so the `agentNotice` fires (rules 2–3 above).
- [ ] **Tests** — an e2e test asserting the `--dry-run` output (`dry_run: true`, `would_execute`, key `details`) AND a test for the blast-radius guard error. Do NOT rely solely on live happy-path.
- [ ] **Consistent verb** — destructive removal uses `delete` (never `remove`); see Key Decisions.

When you add or change a destructive command, re-read this checklist during the Pre-Commit Self-Review and confirm each box in your own review notes. A destructive command missing any box is INCOMPLETE.

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

**Current directory modules:** `context/` (7 files), `deploy/` (12 files), `lakehouse/` (10 files), `warehouse/` (7 files), `item/` (6 files), `ontology/` (5 files: `mod`, `crud`, `definitions`, `import`, `mcp`), `git/` (5 files: `mod`, `sync`, `connect`, `branch_out`, `relation`).

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
- These steps mirror the CI pipeline — if they pass locally, CI will pass. The release workflow (`release.yml`) additionally runs a `validate` job (fmt + clippy + full `cargo test`, including the skills/context consistency gates) that ALL build/publish jobs depend on, so a tagged release cannot produce artifacts unless the exact tagged commit passes the suite.

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
| **L3 — Mechanics** | The primitives sub-skills/personas point at | `data/{workflows,best_practices,examples,schemas}/*.json` + clap | `context {agent,describe,workflow,best-practices,examples,schema,find}` | `commands.json` generated from clap; rest authored |
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

The `generate_agent_schema` test (`#[ignore]`) writes a freshly generated `commands.json` to disk — run it manually whenever commands change. It merges clap-derived structural data with the semantic annotations already in the file, so existing `mutates`, `returns`, `async`, `destructive`, `auth_scope`, and `examples` values are preserved.

**Intent-scoped sub-skills** (`.agents/skills/fabio-<family>/SKILL.md`): generated by `cargo test generate_subskills -- --ignored` (in `src/commands/context/skillgen.rs`, which is a `#[cfg(test)]`-only module). Each sub-skill pairs authored judgment (a `data/skills/<family>.json` file: `family`, `title`, `description`, `command_groups`, `when_to_use`, `when_not_to_use`, `must`/`prefer`/`avoid`, `key_gotchas`, `troubleshooting` (array of `{symptom, fix}`), `safety`, `shared_references` (best-practice topic names — the cross-cutting "common" layer, rendered with each topic's own summary), `see_also`) with a command index derived from `commands.json`. The generated sections follow skills-for-fabric conventions: a MUST/PREFER/AVOID behavioral triad and a Troubleshooting symptom→fix table. The `subskills_match_generated` drift test fails in CI if the committed files are stale. **NEVER edit `.agents/skills/fabio-*/SKILL.md` by hand** — edit the `data/skills/*.json` family file and regenerate. To add a new family, drop a `data/skills/<family>.json` (auto-registered by `build.rs`) and regenerate. Regenerate after ANY command/subcommand change (the command index would otherwise drift).

### One-Liner (Regenerate Everything)

```bash
cargo test generate_agent_schema -- --ignored && cargo test generate_subskills -- --ignored
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

   **Persona registration** — Each orchestrator persona is a JSON file in `src/commands/context/data/personas/` with required fields: `name`, `description` (for search discoverability), `delegates_to` (the request-type → command-group/workflow routing table). **Auto-registered** by `build.rs` — drop a `.json` file and rebuild. Served via `fabio context persona <name>`. Personas are thin routers (Layer 1) that delegate to command groups + workflows + best-practices; they hold no implementation depth.

   **Disambiguation registration** — Each disambiguation table is a JSON file in `src/commands/context/data/disambiguations/` with required fields: `term`, `summary` (for search discoverability), `meanings` (array of `{context, artifact, description, command_group}`). **Auto-registered** by `build.rs` — drop a `.json` file and rebuild. Served via `fabio context disambiguate <term>` (term lookup normalizes spaces, hyphens, and underscores). Resolves overloaded Fabric terms (e.g. "materialized view") to the right artifact + command group.

   **Discoverability via `fabio context find`** — Best-practices, workflows, personas, and disambiguations are automatically searchable via `fabio context find "<query>"` once registered. The search indexes names, descriptions/summaries, and full JSON content. No additional wiring is needed beyond placing the file in the correct directory.

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
- **CLI flag conventions**: `--workspace` always has `-w` shorthand and `env = "FABIO_WORKSPACE"`; `--capacity-id` always has `env = "FABIO_CAPACITY"`; cross-workspace flags (`--dest-workspace`, `--source-workspace`) are `long`-only (no env, no short). `semantic-model clone` uses `--target-workspace` with `visible_alias = "dest-workspace"` for backward compat. All `run` commands support `--wait`/`--timeout`/`--cancel-on-timeout` for LRO polling.
- Auth relies on a multi-source credential chain: static access token (`FABIO_ACCESS_TOKEN` env var, for Fabric Notebooks and pre-existing tokens), fabio cache (device code, browser PKCE, or service principal), environment variables, managed identity, Azure CLI, Azure Developer CLI
- **Interactive public-client app registration**: fabio's own multitenant public-client Entra app ("Fabio CLI") backs the interactive user flows (device code, browser PKCE, Windows WAM). The compiled-in default is `DEFAULT_PUBLIC_CLIENT_ID` in `src/token_cache.rs`, resolved at runtime via `public_client_id()` which honors the `FABIO_CLIENT_ID` env override (trimmed, non-empty) — lets users switch app registrations without recompiling (e.g. tenant loss/migration recovery). `scripts/create-fabio-app.sh` creates a compatible app (multitenant, `allowPublicClient`, loopback + native-client + WAM-broker redirect URIs) and can patch the source default in place. Distinct from service-principal auth, which takes its client ID from `--client-id`/`AZURE_CLIENT_ID` (see `scripts/setup-ci-auth.sh`). **Delegated permission model (minimal, not all ~200 Power BI scopes)**: fabio acquires tokens for SIX audiences, so the app carries one consented delegated permission per audience — (1) Power BI Service (`api.fabric.microsoft.com`): a curated COARSE set of 14 Fabric/Power BI scopes (`Workspace/Item/Capacity/Connection/Gateway/OneLake/Tenant.ReadWrite.All`, `Item.Execute/Reshare.All`, `Dataset/Report/PaginatedReport/Dashboard/Dataflow.ReadWrite.All`) — Fabric authorizes calls by the user's workspace/tenant RBAC role, not the granular scope claim, so a coarse set covers the whole CLI; (2) Azure Storage → `user_impersonation` (OneLake DFS/Blob); (3) Azure SQL DB → `user_impersonation` (TDS); (4) ARM → `user_impersonation` (capacity ops); (5) Azure Data Explorer → `user_impersonation` (KQL/Kusto); (6) Microsoft Graph → `User.Read` + `InformationProtectionPolicy.Read` (`label list`). Total 20 scopes. Scope GUIDs are resolved by NAME at runtime from each resource SP (portable across tenants/clouds), missing resource SPs are auto-provisioned, and unpublished allow-list names are reported. The non-Fabric audiences need their permission because fabio redeems the cached refresh token for each other audience non-interactively (`get_token_for_scope` in `src/token_cache.rs`), which requires a pre-consented delegated permission.
- `azure_identity`/`azure_core` with `default-features = false` (no OpenSSL on Linux/macOS); `client_certificate` feature Windows-only (vendored OpenSSL)
- **Fully static Linux binaries** — Built with musl (`x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`); zero runtime library dependencies; runs on any Linux kernel 2.6.39+; Docker image uses `FROM scratch`
- **Bundled CA roots** — `webpki-root-certs` crate pre-loads Mozilla CA certificates into every HTTP client via `http_client_builder()` in `src/client.rs`. Ensures HTTPS works on minimal Linux systems without `ca-certificates` installed (`rustls-platform-verifier` has no bundled fallback on Linux). All `reqwest::Client` construction MUST use `http_client_builder()` — never raw `Client::builder()`.
- **Windows-first compatibility** — Token cache encrypted with DPAPI (`CryptProtectData`, user scope); WAM broker SSO via `--wam` flag
- `unsafe_code = "forbid"` in lints
- **KQL Queryset definition format**: Uses `RealTimeQueryset.json` (NOT `RawQueryset.kql`). JSON structure: `{"queryset":{"version":"1.0.0","dataSources":[{"id","clusterUri","type","databaseName"}],"tabs":[{"id","content","title","dataSourceId"}]}}`. The `content` field holds the KQL query text with `\n` for newlines.
- **KQL Queryset run**: Fetches definition via LRO, decodes `RealTimeQueryset.json`, selects tab by name or index, resolves data source (clusterUri + databaseName), executes via Kusto REST API. Tab selection is case-insensitive by title.
- **Deploy diff strategy**: Content hash vs live workspace (not git diff) — detects portal edits, works without git, idempotent convergence
- **Deploy parallelism**: Semaphore-bounded `tokio::spawn` per-item within type batch (default 8); sequential for DataPipeline; deletes always sequential. Export also uses bounded parallelism (default 8) for `getDefinition` LRO calls. Cross-type parallelism via tier grouping (11 tiers, independent types run concurrently).
- **Deploy strategy**: `--strategy default|bulk|sequential`. Default: per-item parallel with content-hash skip (best for iterative CI/CD). Bulk: single `bulkImportDefinitions?beta=True` API call (faster for large initial deploys to empty workspaces; requires no Git integration). Sequential: concurrency=1 (debugging). All strategies share the same planning phase (parameter substitution, content-hash comparison, changeset building). Bulk falls back to per-item for renames and deletes.
- **Deploy parameter format**: JSON (not YAML) — no extra crate dependency, agent-native consistency. Supports fabric-cicd-compatible features: `find_replace`, `key_value_replace`, `spark_pool`, `semantic_model_binding`, dynamic variables (`$workspace.id`, `$items.Type.Name.id`, `$ENV:VAR`).
- **Deploy plan staleness**: Workspace fingerprint = SHA256 of sorted `(id, type, name)` tuples; mismatch → error unless `--force`
- **Deploy logical ID resolution**: String replacement in base64 payloads; resolves items created earlier in same session. Also resolves ExecutePipeline GUID references by matching activity names to pipeline names, and notebookId references by matching activity names to notebook names.
- **Deploy workspace ID replacement**: Replaces ALL workspace GUIDs found in `workspaceId`/`default_lakehouse_workspace_id` fields with the target workspace ID (not just `00000000-...` placeholders). Handles repos exported without Fabric Git Integration normalization.
- **Deploy notebook format detection**: Inferred from file name — `notebook-content.ipynb` → `format: "ipynb"`, `notebook-content.py` → no format (server auto-detects native `.py`). Explicit `definitionFormat` in `.platform` always takes precedence.
- **Deploy connection resolution**: `init-params --resolve-connections` scans pipeline definitions for connection GUIDs, queries tenant connections, and generates a parameters.json with pre-resolved (or TODO) mappings.
- **Deploy rename detection**: Two-pass matching — first by (type, name), then unmatched source items with logical IDs get candidates checked via `fetch_deployed_logical_id()` which reads `.platform` part from deployed item definition
- **Deploy creationPayload**: Separate `creationPayload.json` file in item directory; merged into creation body as `creationPayload` field; parameter substitution applied
- **Deploy post-hooks**: Opt-out via `--no-post-hooks`; hooks never fire during `--dry-run`; failures are non-fatal (reported in output, don't fail the deploy). SemanticModel → `POST /refreshes`, Environment → `POST /staging/publish`, VariableLibrary → `PATCH properties.activeValueSetName` (auto-activates value set matching `--env` name)
- **Deploy variable library value set activation**: When `--env` is specified and VariableLibrary items are deployed (create or update), fabio auto-activates the value set whose name matches the env name (e.g., `--env prod` activates "prod"). Aligns with fabric-cicd behavior. Non-fatal on failure (warns if value set doesn't exist).
- **Variable library definition format**: Three parts: `variables.json` (required), `settings.json` (required), `valueSets/<name>.json` (optional, one per alternate set). Value sets use `variableOverrides` array (not `values`). Path is plural `valueSets/` (forward slash). Active set is a workspace-level setting (not part of definition), managed via PATCH `properties.activeValueSetName`.
- **Deploy validate notebook-settings.json**: `deploy validate` warns when Notebook items lack `notebook-settings.json` — required since March 2026 for auto-binding of lakehouse dependencies after deployment/Git sync.
- **Deploy schedules export/apply**: `deploy export` fetches job schedules for schedulable items (Notebook, DataPipeline, SparkJobDefinition, etc.) and writes `schedules.metadata.json`. `deploy apply` creates schedules on deployed items from the metadata file (additive, non-fatal post-hook).
- **Deploy --post-run-item**: Triggers a named pipeline/notebook after deployment for data orchestration. Finds item by display name, determines job type, runs via Job Scheduler API. Non-fatal.
- **Workspace clone**: `workspace clone --source <WS> --dest <WS>` uses Bulk Export/Import Definitions APIs for fast workspace replication. Supports `--allow-pairing-by-name` for initial clones without logicalId matching, and `--item-types` for selective cloning.
- **Git branch-out**: `git branch-out --workspace <WS> --branch <feature-name>` automates the Fabric "Branch out to workspace" flow — creates workspace, assigns capacity, connects to new branch, initializes from branch content. Requires `--connection-id` for GitHub provider. Supports `--existing-workspace` for recycling feature workspaces.
- **Deploy empty definitions**: Items with no parts (Lakehouse, MLModel) omit `definition` field on create; skip `updateDefinition` on update
- **Deploy shell-only export**: Warehouse, SQLDatabase, MLExperiment, MLModel don't support `getDefinition` but are exported as `.platform`-only directories (metadata without definition parts). Aligns with fabric-cicd's `SHELL_ONLY_PUBLISH`. SQLEndpoint is always skipped (auto-provisioned by Fabric, not independently deployable).
- **Deploy ordering**: 45 item types in `DEPLOY_ORDER`; deployed in dependency order (storage → compute → code → models → reactive → APIs → ML → graph → viz)
- **Deploy no state file**: Stateless — always queries live workspace. No `.tfstate` equivalent.
- **Deploy .platform in parts but excluded from hash**: `.platform` IS sent as a definition part (enables `?updateMetadata=true` for metadata propagation), but EXCLUDED from content hash (API rewrites `logicalId` in `.platform`, which would break idempotent skip detection)
- **Deploy workspace ID regex replacement**: Uses regex matching on `workspaceId`, `default_lakehouse_workspace_id`, `workspace` keys — not blanket string replacement. Skips shortcuts (handled separately with lakehouse GUID). Opt-out: `--no-workspace-id-replace`
- **Deploy config file (JSON + YAML)**: `--config <file> --env <name>` loads per-environment workspace/source/parameters; `serde_yaml` crate for YAML; CLI flags override config values
- **Deploy protected type deletion**: Lakehouse, Warehouse, SQLDatabase, Eventhouse, KQLDatabase require `--allow-delete-types` to be deleted by `--delete-orphans`
- **Deploy fabric-cicd full compatibility**: Source directory format, .platform file schema, definition parts, logical ID resolution, workspace ID replacement, creationPayload, .children/ discovery, .pbi/ exclusion, notebook ordering, Report byPath transform — all aligned with Microsoft's fabric-cicd Python library
- **Upgrade**: `fabio upgrade` downloads latest release from GitHub, verifies SHA256 checksum, extracts platform-appropriate archive (tar.gz on Unix, zip on Windows), atomically replaces running binary; supports `--check` (version query only), `--target-version` (pin specific version), `--force` (reinstall even if current), `--dry-run`
- **Context tenant LSP-inspired agent features**: Inspired by Language Server Protocol design, `context tenant` provides progressive disclosure for AI agents: `--summary-only` (cheap inventory probe, 2 API calls), `--resolve Type:Name` (fast name-to-ID lookup without graph), `--focus <id> --depth N` (ego-centric subgraph via BFS). All graph responses include a `meta` envelope (`scannedAt`, `scanDurationMs`, `etag` SHA-256 fingerprint, `partial`, `scope`) for freshness/drift detection. Edges carry `confidence` (high/medium/low) and `discoveryMethod` fields so agents can filter noise.
- **Context information-architecture layers (personas + disambiguations)**: Inspired by microsoft/skills-for-fabric's Agents→Skills→Common decomposition, fabio adds two authored-knowledge layers on top of its runtime mechanics. **Personas** (`fabio context persona <name>`: data-engineer, data-scientist, app-developer, bi-developer, rti-engineer, migration-engineer, fabric-admin) are Layer-1 orchestrators — thin routers mapping a request type to command groups + workflows + best-practices, with decision gates, guardrails (must/prefer/avoid), and negative routing. **Disambiguations** (`fabio context disambiguate <term>`: materialized-view, dataflow, semantic-model, sql-endpoint, mirroring, model) resolve overloaded Fabric terms to the concrete artifact + command group. Both are JSON data files auto-registered by `build.rs` (like workflows/best-practices), searchable via `context find`, and drift-free because command indexes come from `commands.json`. Migration is data-only for now (workflows synapse/databricks/hdinsight/pipeline + best-practice `migration-api-shims`); a `fabio migrate assess` command is a deferred epic.
- **Intent-scoped sub-skills (Layer 2, generated)**: Fourteen `fabio-<family>` sub-skills (lakehouse, warehouse-sql, data-engineering, dataflows, data-science, mirroring, rti-kql, bi, ontology, geospatial, deploy-cicd, admin, migration, app-dev) at `.agents/skills/fabio-*/SKILL.md` are GENERATED from authored judgment (`src/commands/context/data/skills/<family>.json`) + a command index pulled from `commands.json`. This realizes the division of labor: prose carries judgment (when-to-use, gotchas, safety, routing); the command table is mechanically derived (drift-free). The generator lives in `src/commands/context/skillgen.rs` (a `#[cfg(test)]`-only module — it is a build/test-time tool, not runtime, mirroring `generate_agent_schema`). Regenerate with `cargo test generate_subskills -- --ignored`; the `subskills_match_generated` drift test fails in CI if committed files are stale. Every workload command group is in a skill family (enforced down to the subcommand level by the `every_command_group_has_a_knowledge_home` test — a group with no family generates no command table, so a new group with no family/allowlist home fails `cargo test` before release; personas are additive routing, not a coverage substitute). The root `fabio` skill remains the comprehensive single-file entry point and now routes to sub-skills for progressive disclosure (load only the relevant sub-skill to keep context lean).
- **Item relations (beta)**: `fabio item list-upstream-relations`/`list-downstream-relations` call the new `GET /workspaces/{ws}/items/{id}/relations/{upstream|downstream}?beta=true` endpoints. Response is a graph fragment (`items`/`relations`/`workspaces`), not a paginated list — rendered via `render_object`, not `render_list_with_token`.
 - **Lakehouse MLV execution definitions**: New CRUD group `fabio lakehouse {list,show,create,update,delete}-execution-definition(s)` at `/workspaces/{ws}/lakehouses/{id}/mlvexecutiondefinitions[/{defId}]`. Groups a `currentLakehouseExecutionContext`/`extendedLineageExecutionContext` (discriminated `All`/`Selected` unions) with optional Spark `environment` + `refreshMode` settings; referenced by materialized-lake-view refresh schedules via `executionData.mlvExecutionDefinitionId`.
 - **User data function invoke (portal-URL action)**: `fabio user-data-function invoke --url <public-function-url> [--parameter name=value]... [--body <json>]` invokes a *published* function via its per-function public REST endpoint. Fabric exposes no public API to invoke a function or discover its URL (the URL is copied from the portal, public access enabled), so `--url` is supplied directly — mirroring `data-agent query --published-url`. fabio SSRF-guards the URL (`validate_trusted_url`, `*.fabric.microsoft.com` HTTPS only), attaches the Fabric bearer token, POSTs the parameter body, and renders the `{functionName, invocationId, status, output, errors}` response. Pure `build_invoke_body` (parameters→JSON, `--body` override) is unit-tested; plumbing is live-validated (auth + POST reach Fabric, clean 404 surfaced). Happy-path invocation needs a published function with public access (not REST-provisionable) — documented in API-BEHAVIORS.
 - **ML experiment run tracking (MLflow action)**: `fabio ml-experiment {list-runs,get-run,get-metric-history}` read MLflow run data from the per-workspace Fabric-hosted MLflow tracking server at `/workspaces/{ws}/mlflow/api/2.0/mlflow/...` (standard Fabric token; the experiment item GUID is the MLflow `experiment_id`). `list-runs` → `runs/search` (supports `--filter`/`--order-by` MLflow expressions + `--limit`), `get-run` → `runs/get`, `get-metric-history` → `metrics/get-history`. This closes the ML-experiment Tier-B action gap (runs live on the MLflow surface, not the Fabric item API). Pure helpers (`mlflow_base`, `build_search_body`) are unit-tested; the e2e test seeds a run entirely over the MLflow REST API (`runs/create`+`log-metric`+`update`) so the full loop is live-validated without a notebook.
 - **Report/paginated-report export-to-file (Power BI action)**: `fabio report export` and `fabio paginated-report export` render a (paginated) report to a file via the Power BI `exportToFile` async flow (`POST .../reports/{id}/ExportTo` → poll `.../exports/{jobId}` → download `.../file`), implemented in the shared `src/commands/powerbi_export.rs` module (with the new `client::get_powerbi_bytes` helper). Formats: PDF/PPTX (both), PNG (Power BI reports), IMAGE/XLSX/DOCX/CSV/XML/MHTML/ACCESSIBLEPDF (paginated); paginated `--parameter name=value` maps to `paginatedReportConfiguration.parameterValues`. The format validator + body builder are pure and unit-tested; plumbing is live-validated (a bogus id yields a clean `PowerBIEntityNotFound`). This was the one clear "CRUD-only item that exists to produce an output" gap found in the item-action audit — most other CRUD-only groups are complete because the public Fabric REST API offers only CRUD for them (their actions live on Power BI/MLflow/Kusto/portal surfaces).
  - **Paginated-report create/update-definition body (definition MUST omit `format`)**: `fabio paginated-report create`/`update-definition` send `definition: { parts: [...] }` with **NO `format` field**. Sending `definition.format: "PaginatedReportDefinition"` (the previous behavior, and what the docs example implies) is rejected by the Fabric API with `InvalidDefinitionFormat` — this mirrors `report create`, which also omits `format`. The single RDL part path MUST equal `<displayName>.rdl` (any other path → `MissingDefinitionParts`): `create` synthesizes it from `--name`, `update-definition` resolves it from a GET of the item's current display name (a `.platform` part is optional). This corrected an earlier WRONG "create is blocked server-side on this capacity" conclusion — it was a fabio bug, not a tenant/region limitation; live-validated end-to-end (create → byte-identical CSV export vs a portal-authored original → update-definition round-trip). Pure helpers `single_rdl_part`/`definition_object` in `src/commands/paginated_report.rs` are unit-tested (regression guard: `definition_object` must never emit `format`). See `.agents/API-BEHAVIORS-DISCOVERED.md` "Dashboard/Datamart/Paginated Report API Behaviors Discovered".
 - **Data agent MCP consumption endpoint**: `fabio data-agent mcp-url --workspace <ws> --id <id>` prints the canonical Model Context Protocol runtime/consumption endpoint for a data agent — `{fabricBase}/mcp/workspaces/{ws}/dataagents/{id}/agent` — which external MCP clients (Claude, Copilot Studio, Azure AI Foundry) use to query a *published* agent. This is distinct from `data-agent query`, which uses the older OpenAI-Assistants endpoint (`.../aiassistant/openai`). Published state is detected reliably via `GET /dataAgents/{id}/settings` (200 = published, 404 `DataAgentNotPublished` = draft); when unpublished, the command reports `published: false` and hints to run `data-agent publish`. The URL builder (`build_mcp_url`) is pure and unit-tested; the base URL honors `FABIO_FABRIC_API_ENDPOINT` via the new `client::fabric_base_url()` accessor. Both Fabric agent item types — DataAgent and OperationsAgent — are now fully covered against their public REST APIs (8 and 7 operations respectively), plus fabio's richer sub-operations.
 - **Operations agent start/stop/status (RTI AI monitoring)**: `fabio operations-agent {start,stop,status}` manage a Real-Time Intelligence operations agent's activation. Fabric exposes no dedicated start/stop endpoint — activation is the top-level `shouldRun` boolean inside the agent's `Configurations.json` definition. `start`/`stop` are implemented as a read-modify-write (`getDefinition` → decode `Configurations.json` part → flip `shouldRun` → `updateDefinition` with just that part) followed by a re-read to report the persisted value; `status` reads `shouldRun` back and reports `running`/`stopped`. **Fabric silently coerces `shouldRun` back to `false` for an agent with no configured data source + playbook** (verified live), so `start` reports `requestedShouldRun` vs the actual `shouldRun` and adds a `note` when activation was refused rather than claiming false success. While running, Fabric evaluates the agent's rule queries every 5 minutes. The `shouldRun` read/flip helpers in `src/commands/operations_agent.rs` are pure and unit-tested. operations-agent is routed via the `rti-kql` sub-skill and the `app-developer` persona. Playbook generation, Copilot-chat configuration, the activity log, and Teams/Investigator actions are portal/Copilot-only surfaces with no public REST API, so they are out of scope for the CLI.
 - **Data agent SQL sources & preview runtime (Advanced NL2SQL)**: A data agent's four SQL source types — Lakehouse, Data Warehouse, SQL Database, Mirrored Database — are added and configured through the existing datasource commands (`add-datasource --artifact-type Lakehouse|Warehouse|SQLDatabase|MirroredDatabase`, `select-tables` for schema scope, `update-datasource --instructions/--description`, and the fewshots commands for example queries). The runtime that drives their built-in NL2SQL tool is toggled by `fabio data-agent update-config --enable-preview-runtime|--disable-preview-runtime`: standard runtime = GA NL2SQL (single-pass, production-stable), preview runtime = Advanced NL2SQL (multi-step reasoning — better example following, filter-value substitution, ambiguity clarification). Both runtimes consume the SAME per-source config, so switching needs no reconfiguration. On the wire the flag is the NESTED settings boolean `experimental.enableExperimentalFeatures` in `PATCH .../staging/settings` (a top-level `enableExperimentalFeatures` is silently ignored); fabio does a read-modify-write to preserve sibling `experimental` keys (e.g. `mcpServers`). `get-config`/`update-config` surface it as a top-level `previewRuntime` boolean. This matches the Fabric Python SDK's `update_configuration(enable_preview_runtime=...)`. A published agent's runtime is fixed at publish time (republish to change). The pure body builder (`build_settings_body`) and reader (`preview_runtime_enabled`) in `src/commands/dataagent/config.rs` are unit-tested; the full enable→disable toggle is live-validated in `tests/e2e_dataagent.rs` (`dataagent_preview_runtime_toggle_lifecycle` and `dataagent_advanced_management_lifecycle`). NOTE: the `--enable-preview-runtime`/`--disable-preview-runtime` flags previously existed on `update-config` but were accepted and silently dropped (never written to the API) — this wires them to the real settings field.
 - **Data agent consumption: multi-turn query, answer-file download, evaluate batch primitive, and constructed published-URL fallback**: A gap analysis against the Python `fabric-data-agent-sdk` (v0.1.27a0) confirmed fabio's public *management*-plane surface is already 1:1 with the SDK (settings/datasources/elements/fewshots/publish/reset). The SDK's extra capabilities are data-plane/LLM/notebook features; the reachable ones are now in fabio's `query`/`evaluate` (`src/commands/dataagent/query.rs`, `evaluate.rs`). (1) **`query` returns `threadId`** and gained `--thread-id` (reuse an existing thread) + `--keep-thread` (retain it) for MULTI-TURN follow-ups over the OpenAI Assistants protocol fabio already speaks — live-validated that a second CLI invocation on the same thread recalls prior context. (2) **`query --download-files <dir>`** saves files the answer attaches (generated CSVs/charts) via the OpenAI files API (`GET {base}/files/{id}/content`) on the published endpoint; the response gains a `files` array. File-id extraction scans assistant-message text annotations (`file_path`/`file_citation`), `image_file` content items, and message `attachments`; filenames are sanitized to a safe basename (path-traversal-proof, Windows + Unix). (3) **`--stage` is no longer a silent no-op**: only `production`/`published` is accepted; `sandbox`/`staging` (draft) fails fast with a publish-first hint, because the draft agent lives on the internal workload host with no public endpoint. (4) **`data-agent evaluate --questions <file>`** is a batch primitive (mirrors the SDK's `evaluate_data_agent` shape: questions + optional expected + `--repeats`) that runs each question on its own thread and emits the answers (`--show-steps` for run steps); it is NOT an LLM judge — when a question has an `expected` answer it adds only a NAIVE `match.exact`/`match.contains` string signal, leaving semantic grading to the calling agent. Questions file is JSON (array of strings or `{question,expected}` objects) or CSV/TSV with a `question` column. (5) **Constructed published-URL fallback**: `GET /dataAgents/{id}/settings` does NOT return a `publishedUrl`, so `query`/`evaluate` now build the canonical `{fabricBase}/workspaces/{ws}/dataagents/{id}/aiassistant/openai` (lowercase `dataagents`) when no `--published-url` is given — previously `query` errored unless the user supplied the URL manually. Pure helpers (`validate_query_stage`, `extract_answer`, `extract_file_ids`, `sanitize_filename`, `build_published_url` in `query.rs`; `parse_questions`, `compute_match`, `normalize` in `evaluate.rs`) are unit-tested; the full published-agent lifecycle (publish → query+threadId → multi-turn → evaluate) is live-validated in `dataagent_query_multiturn_and_evaluate_lifecycle`. Out of scope (require an LLM judge or the internal workload host, not the public API): the SDK's OpenAI *Responses* API client, few-shot LLM validation, and M365 Copilot Agent Store publishing (`publish --to-m365` remains correctly reported as unsupported — the SDK implements it against a non-public `metaosapppackage` workload endpoint).
 - **Data agent LLM-powered features (bring-your-own judge model) + confirmed non-public surfaces**: Two previously-out-of-scope SDK features that need an external judge model are now implemented, with the model supplied by the caller via `--llm-endpoint`/`--llm-key`/`--llm-model` (+ `--llm-api-version`; env fallbacks `FABIO_LLM_ENDPOINT`/`FABIO_LLM_KEY`/`FABIO_LLM_MODEL`/`FABIO_LLM_API_VERSION`). A shared minimal OpenAI-compatible client lives in `src/llm.rs` (`LlmClient`/`LlmConfig`): it auto-detects flavor from the endpoint host — `*.azure.com` → Azure OpenAI (`{endpoint}/openai/deployments/{model}/chat/completions?api-version=…`, `api-key` header, model in URL) vs anything else → OpenAI-compatible (`{endpoint}/chat/completions`, `Authorization: Bearer`, model in body). `complete_json` parses JSON leniently (strips code fences, extracts the first balanced brace span). (1) **`data-agent validate-fewshots`** (`validate.rs`) reads a data source's few-shots and asks the judge to flag duplicates/conflicts/ambiguity/low-quality/incorrect queries → `{issueCount, overallQuality, issues:[{fewshotIds,type,severity,explanation,suggestion}]}`; read-only; requires `--llm-*`. (2) **`data-agent evaluate --llm-*`** upgrades the batch primitive with an optional LLM critic: each answer gets a `grade` (`{correct,score,rationale}`) and the summary adds `gradedRuns`/`passedRuns`/`passRate`. Grading is **non-fatal** — a judge error (e.g. Azure content-filter 400, live-observed) is recorded as a per-answer `grade.error` and the eval continues. Pure builders (`build_chat_url`/`build_chat_body`/`extract_content`/`extract_json`/`is_azure_endpoint` in `llm.rs`; `build_validation_prompt` in `validate.rs`; `build_grade_prompt` in `evaluate.rs`) are unit-tested; both features are live-validated end-to-end against Azure OpenAI (`gpt-5-mini`) + the tenant in `dataagent_llm_validate_and_grade_lifecycle` (skips if `FABIO_LLM_*` unset). **Confirmed non-public (live 404 evidence, cannot be implemented from an external CLI)**: the SDK's OpenAI *Responses* API (`POST {published}/responses` → 404) and M365 Copilot Store publishing (every candidate public path for `metaosapppackage` → 404) both live only on the internal Fabric workload host, discovered via `synapse.ml.fabric.service_discovery` inside a notebook. `publish --to-m365` stays `unsupported` with a sharper message; Responses adds nothing over fabio's existing thread-based multi-turn.
- **Hint type classification for semantic drift prevention**: Error hints include a `hintType` field (`auth_fix`, `retry_safe`, `syntax_fix`, `semantic_correction`, `safety_bypass`) that classifies the hint's semantic impact on the operation. Agents use this to decide whether a hint-driven retry is safe to execute automatically (`auth_fix`/`retry_safe`/`syntax_fix`) or requires user confirmation/post-action verification (`semantic_correction`/`safety_bypass`). An optional `verifyAfter` field provides a read-only verification command the agent should run after a successful retry. Inference logic in `render_error()` auto-classifies the ~391 existing `with_hint()` call sites based on error code and hint content patterns; new code uses explicit `with_typed_hint()`.
- **Sensitivity labels**: All 50 item-type create commands support `--sensitivity-label <uuid>`. All list commands dynamically show a SENSITIVITY LABEL column when items have labels. Label UUIDs are returned inline by the Fabric API (no `--include` needed). `fabio label list` resolves UUIDs to names via Microsoft Graph (requires M365 E5 + InformationProtection.Read). PATCH does NOT support label changes — only create-time or admin bulk operations. See `.agents/API-BEHAVIORS-DISCOVERED.md` section "Sensitivity Labels API Behaviors Discovered" for full details.
- **Workspace inbound External Data Shares bypass policy (Preview)**: `fabio workspace get-inbound-external-data-shares-policy`/`set-inbound-external-data-shares-policy --default-action Allow|Deny [--if-match <etag>]` at `/workspaces/{ws}/networking/communicationPolicy/inbound/externalDataShares`. First fabio endpoint to use response `ETag`/request `If-Match` optimistic concurrency for a Fabric REST object (previously only used for OneLake file properties). New `FabricClient::get_with_etag()`/`put_with_if_match()` helpers merge the `ETag` response header into the JSON body as an `etag` field so it round-trips through the CLI without a separate flag.
- **Connection `gatewayId` is now a base response property**: Any connection's response (not just gateway-specific connectivity types) may include `gatewayId`. `fabio connection list` shows a dynamic `GATEWAY ID` column when present.
- **Gateway member-count range fields**: `gateway create`/`gateway update` gained `--max-member-gateway-count`/`--min-member-gateway-count` (mutually exclusive with the legacy `--member-count` fixed value, via clap `conflicts_with_all`/`requires`), mirroring the Fabric API's new `maxMemberGatewayCount`/`minMemberGatewayCount` range pair that supersedes `numberOfMemberGateways`. `create` still defaults to a fixed count of 1 when none of the three flags are given (backward compatible); `update` applies no default (partial PATCH). See `.agents/API-BEHAVIORS-DISCOVERED.md` "Gateway Lifecycle API Behaviors Discovered" for the full mutual-exclusivity/error-code details.
- **Git Workspace Relations (Preview)**: New `fabio git relation list|create|delete` commands implement the Fabric REST `WorkspaceRelations` API (`GET/POST /workspaces/{id}/git/workspaceRelations`, `DELETE .../{relationId}`) for managing base/branch links between workspaces as an independent resource (previously only implicit via `git branch-out`). Implemented in `src/commands/git/relation.rs` (the `git` command is a directory module — `mod.rs` enum/dispatch + `sync.rs` + `connect.rs` + `branch_out.rs` + `relation.rs`). See `.agents/API-BEHAVIORS-DISCOVERED.md` "Git Workspace Relations API Behaviors Discovered (Preview)" for API semantics and error codes.
 - **Microsoft Fabric MCP server parity (OneLake shortcuts/directories)**: Audited fabio against Microsoft's official Fabric MCP server (`microsoft/mcp` → `servers/Fabric.Mcp.Server`, v1.2.0 Jul 2026). fabio was already at/beyond parity for its data-access roles (`onelake-security`, a superset), catalog search (`catalog search`), OneLake table API (`lakehouse iceberg-*`), OneLake settings (`workspace modify-diagnostics`/`modify-immutability-policy`/`reset-shortcut-cache`), and Dataflow Gen2 M execution (`dataflow execute-query --mashup`). The genuine gaps closed: (1) **`lakehouse list-shortcuts`** (`GET /workspaces/{ws}/items/{id}/shortcuts`, `--parent-path`, paginated) — mirrors the MCP's client-side hiding of DW-managed shortcuts (internal OneLake→OneLake refs under `Tables/…`, hidden unless `--include-managed`; heuristic in `is_managed_shortcut`). (2) **Typed shortcut-target creation**: `create-shortcut` gained typed flags (`--connection-id`, `--location`, `--subpath`, `--bucket`, `--target-workspace`/`--target-item`/`--target-path`, `--environment-domain`, `--delta-lake-folder`, `--update-sensitivity`) covering all 9 Fabric target types (OneLake, AdlsGen2, AmazonS3, AzureBlobStorage, GoogleCloudStorage, S3Compatible, Dataverse, ExternalDataShare, OneDriveSharePoint) in ONE command (vs the MCP's 9 separate typed tools); `--target-type` is normalized/validated (aliases + any case) and an unknown type errors with the enumerated valid values; the raw `--target` JSON remains an escape hatch. Exact target field shapes were taken from the MCP's `ShortcutModels.cs`. Pure builders (`normalize_target_type`, `build_shortcut_target`, `is_managed_shortcut`) in `src/commands/lakehouse/shortcuts.rs` are unit-tested; the typed OneLake create + list lifecycle is live-validated (`lakehouse_typed_onelake_shortcut_and_list`). (3) **`lakehouse delete-directory --path`** (recursive DFS delete, `--dry-run`-guarded, destructive) — wires up the existing `delete_onelake_directory` client method for arbitrary directories (previously only reachable via `delete-table`). Because a recursive delete of an empty/root path (`""`, `/`, `.`, `..`) would erase the ENTIRE lakehouse, `validate_delete_directory_path` refuses those outright (concrete subdirs, including the `Files`/`Tables` roots, are allowed); it is also `destructive:true` in `commands.json` and blocked by `--readonly` via the client `guard_readonly`. NOT added: the MCP's `onelake_list_items_dfs` (a DFS-transport duplicate of `fabio item list`, which already enumerates items via the Fabric REST API — no new capability). See `.agents/API-BEHAVIORS-DISCOVERED.md` "OneLake Shortcuts API Behaviors Discovered".
 - **Item definition part requirements + offline validation (determinism for agents)**: A single source of truth — `src/commands/context/data/agent/definition_requirements.json`, loaded by the top-level `src/definition_spec.rs` module — captures each item type's CANONICAL definition part paths (what `getDefinition`/Git export/`deploy` round-trip), `definitionFormat`, accepted aliases, and authoring notes (ground-truthed live). It powers three drift-free capabilities: (1) **`fabio item validate-definition`** — an OFFLINE validator (no API call) that checks a definition envelope (`--file`/`--definition`) or a folder of parts (`--dir`) before create/update-definition. Universal envelope rules are ERRORS (`MISSING_PARTS`, `INVALID_PAYLOAD_TYPE` — only `InlineBase64`, `INVALID_BASE64`, `INVALID_JSON_PART`, `DUPLICATE_PART`, …); per-type canonical-part gaps are WARNINGS (Fabric tolerates alias filenames), promoted to failures with `--strict`. Zero false positives verified against real exported CopyJob/Dataflow/DataPipeline/SparkJobDefinition/Notebook folders. (2) **Enriched definition-authoring hints** (`definition_spec::definition_input_hint`) that enumerate the required part path(s) + `definitionFormat`, show the envelope shape, and point at `fabio context schema <Type>` and the offline validator. (3) **`fabio context schema <Type>`** now merges an authoritative `definition_requirements` block (canonical parts never drift from Fabric), and serves spec-backed responses for types that lacked a hand-authored schema (KQLDashboard, Map, OperationsAgent, …). `fabio context find` now also indexes item schemas + output examples (previously it did not, so an agent searching "notebook definition format" was never routed to `context schema`). **Fabric alignment fixes**: `copy-job update-definition` now emits the canonical `copyjob-content.json` (was `CopyJobV1.json`); `dataflow update-definition` (previously a NO-OP that wrapped input as a single ignored `dataflow.json` part) now passes a full multi-part envelope through verbatim via the shared `definition_spec::build_update_definition_body`, so the reliable pattern is `get-definition` → edit → `update-definition --file <envelope.json>`. `SparkJobDefinition` keeps the canonical `SparkJobDefinitionV1.json` FILENAME (format `SparkJobDefinitionV2`). See `.agents/API-BEHAVIORS-DISCOVERED.md` "Item Definition Part Requirements & Offline Validation".
 - **Ontology MCP consumption endpoint**: `fabio ontology mcp-url --workspace <ws> --id <id>` prints the canonical Model Context Protocol server URL for consuming a Fabric ontology (preview) item as an MCP server — `{fabricBase}/mcp/dataPlane/workspaces/{ws}/items/{id}/ontologyEndpoint` — which external MCP clients (VS Code agent mode, Claude, Copilot Studio) connect to over HTTP transport with Fabric auth. NOTE the URL shape DIFFERS from the data-agent one (`/mcp/dataPlane/.../items/.../ontologyEndpoint` vs `/mcp/workspaces/.../dataagents/.../agent`) — an agent cannot guess it, so fabio constructs it. Handler lives in `src/commands/ontology/mcp.rs` (the `ontology` command is a directory module — `mod.rs` enum/dispatch + `crud.rs` + `definitions.rs` + `import.rs` + `mcp.rs`). Pure `build_mcp_url` is unit-tested; the command does a lightweight existence check (`exists` field + prerequisite `note`, or a `hint` when not found). Live-verified end-to-end: the constructed URL is a real, working MCP server — an `initialize` handshake returns `serverInfo: "Microsoft Fabric Ontology" v1.0.0` with a `tools` capability. This is distinct from grounding a fabio data-agent ON an ontology (`data-agent add-datasource --artifact-type Ontology`); mcp-url exposes the ontology ITSELF over MCP. Requires an F2+/P1 capacity and the Ontology-item preview tenant setting. See `.agents/API-BEHAVIORS-DISCOVERED.md` "Ontology MCP Server (Preview)".
 - **Ontology entity-type listing (MCP-tool parity, offline)**: `fabio ontology list-entity-types --workspace <ws> --id <id> [--entity-name <name>] [--include-properties]` is the pure-fabio equivalent of the ontology MCP server's `list_ontology_entity_types` tool — it reproduces that tool's `{"values":[...]}` output BYTE-FOR-BYTE (verified live for all param combinations), computed OFFLINE from `getDefinition`'s `EntityTypes/*/definition.json` parts (no MCP session). The reshape reorders fields to the tool's key order (relies on serde_json `preserve_order`), strips `$schema` + null property fields, and defaults `documents`/`mappings`/`resourceLinks` to `[]`. The ONLY field it cannot reproduce is the server-assigned `etag` (a per-entity concurrency token absent from the definition) — omitted. Handler + pure reshape helpers in `src/commands/ontology/entity_types.rs` are unit-tested against the exact live MCP output; a live e2e (`ontology_list_entity_types_matches_mcp_shape`) validates it end-to-end. Established via a live `tools/list` comparison of BOTH Fabric MCP servers: the data-agent MCP server's single tool (`DataAgent_<name>(userQuestion)`) is already covered by `fabio data-agent query`, and the ontology server's other tool (`search_ontology`, NL query) is now covered too (see the MCP-client decision below). See `.agents/API-BEHAVIORS-DISCOVERED.md` "Ontology MCP tools vs. pure fabio".
 - **MCP client (fabio consuming external MCP servers) + `ontology search`**: fabio gained its first MCP-CLIENT capability — `src/mcp_client.rs`, a GENERIC Model Context Protocol client over the streamable-HTTP transport (`McpClient::connect` runs the `initialize` handshake + `notifications/initialized`, then `list_tools`/`call_tool`; handles both `application/json` and `text/event-stream`/SSE responses; echoes an `Mcp-Session-Id` if the server assigns one — Fabric's servers are stateless and omit it). It is the counterpart of `fabio mcp serve` (fabio as an MCP *server*) and is NOT ontology-specific: it takes an endpoint URL + an optional `Authorization` header. First consumer: **`fabio ontology search --workspace <ws> --id <id> --prompt "<q>" [--raw]`**, which drives the ontology MCP server's `search_ontology` tool (NL query over the ontology data estate — the one Fabric-IQ-reasoning capability with no offline equivalent). It builds the ontology MCP URL (same as `mcp-url`), HTTPS+trusted-host-validates it (`validate_trusted_url`) before sending the Fabric bearer token (`require_auth`), confirms the tool exists via `list_tools`, then calls `search_ontology` with `{naturalLanguageQuery, naturalLanguageResponse=!--raw}`; output is `{"query","answer","isError"}`, `--dry-run` prints the plan without any network call. Pure parsers in `mcp_client.rs` (`parse_rpc_response` for JSON + SSE, `sse_data_blocks`, `ToolResult::text`) are unit-tested; a deterministic `--dry-run` e2e + a live e2e (`ontology_search_drives_mcp_client`) validate the client end-to-end. Live-verified: fabio's `ontology search` is BYTE-IDENTICAL to a raw MCP `tools/call` (side-by-side curl). A successful NL *answer* additionally needs the ontology bound to data AND the capacity's Fabric IQ reasoning provisioned (server-side) — without it the tool returns `isError:true` "could not be processed", which fabio faithfully surfaces (exiting non-zero). See `.agents/API-BEHAVIORS-DISCOVERED.md` "MCP client (fabio consuming external MCP servers)".
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
 - Power BI Desktop project (PBIP) format docs (authoritative, coding-agent-facing): `https://learn.microsoft.com/en-us/power-bi/developer/projects/projects-overview` (+ `projects-report` and `projects-dataset`). Defines the plain-text folder layout Power BI Desktop / Fabric Git Integration produce and that coding agents can generate/edit: a PBIP root has `<name>.Report/`, `<name>.SemanticModel/`, and a `<name>.pbip` pointer file. **Report** (`.Report/`): required `definition.pbir` (`$schema`+`version`+`datasetReference` with `byPath` XOR `byConnection`) plus EITHER `report.json` (PBIR-Legacy, version 1.0) OR a `definition/` folder (PBIR enhanced, version 4.0+: `definition/report.json`, `definition/version.json`, `definition/pages/pages.json`, `definition/pages/<page>/page.json`, `definition/pages/<page>/visuals/<visual>/visual.json`, optional `bookmarks/`, `reportExtensions.json` for report-level measures). Every PBIR file carries its own `$schema` (schemas under `microsoft/json-schemas/fabric/item/report/definition/**`). **Semantic model** (`.SemanticModel/`): required `definition.pbism` plus EITHER `model.bim` (TMSL, version 1.0) OR a `definition/` folder (TMDL, version 4.0+). Deploy into Fabric requires `byConnection` (a report's `byPath` must be rewired to the deployed model id); `.pbi/` files (localSettings/cache.abf) are git-ignored user state. PBIR is the format DESIGNED for programmatic generation by agents (per-file JSON with schemas); when PBIR reaches GA it becomes the only report format. fabio uses this to power `report validate` (offline PBIR/PBIR-Legacy structural + `$schema` checks) and `report create --definition <folder>` (create a full PBIR report from a generated folder).
 - Analysis Services references (authoritative — the specs Fabric semantic models inherit): `https://learn.microsoft.com/en-us/analysis-services/analysis-services-references`. Fabric/Power BI Premium tabular models are Analysis Services tabular models, so they share: **TMSL** (Tabular Model Scripting Language — the JSON `model.bim` object/command syntax, compat level 1200+; fabio's `semantic-model create --file model.bim`), **TMDL** (Tabular Model Definition Language — the newer per-object folder format; `definition/` folder, `--file *.tmdl`), **TOM/AMO** (.NET client libraries — NOT usable from fabio's Rust/REST surface), **XMLA** (the SOAP protocol under all AS clients; Fabric exposes XMLA endpoints but fabio uses the Power BI `executeQueries` REST API instead), **DAX** (`semantic-model query --dax`), **MDX** (multidimensional — not Fabric tabular), **Power Query M** (`dataflow execute-query --mashup`), and **Schema Rowsets** (the `TMSCHEMA_*`/`DISCOVER_*` DMVs — exposed as DAX `INFO.VIEW.*` functions and surfaced by `semantic-model list-tables/list-columns/list-measures/list-relationships`). What is REST-accessible to fabio: DAX (incl. `INFO.VIEW.*` introspection) and enhanced refresh via the Power BI API; TMSL command execution and TOM programmatic editing require an XMLA/AS client (out of scope for a REST CLI). See `.agents/API-BEHAVIORS-DISCOVERED.md` "Analysis Services specs → fabio surface".
 - Fabric item-definition JSON schemas (authoritative): `https://github.com/microsoft/json-schemas/tree/main/fabric` — Microsoft's published JSON Schemas for Fabric item definitions and the git-integration `.platform` file. Structure: `fabric/item/<type>/definition/**/<version>/schema.json` (per-item, versioned, e.g. `report/definitionProperties/{1.0.0,2.0.0}`, `semanticModel/definitionProperties/1.0.0`, `map/definition/2.1.0`, `ontology/{entityType,relationshipType,dataBinding}/1.0.0`, `variableLibrary/definition/{variables,settings,valueSet}/1.0.0`, `operationsAgents/definition/1.0.0`), `fabric/gitIntegration/platformProperties/{2.0.0,2.1.0}/schema.json` (the `.platform` file), and `fabric/common/`. When fabio generates or emits a `$schema`-bearing definition part, it MUST reference the LATEST matching published version and conform to that schema's `required`/`properties`/`enum`. Served publicly at `https://developer.microsoft.com/json-schemas/fabric/...` (the URL fabio puts in `$schema`). Item types WITHOUT a published schema here (Notebook, DataPipeline, Eventstream, Reflex, KQL queryset/dashboard, SparkJobDefinition, Dataflow, MirroredDatabase, etc.) have no conformance obligation.
 - **JSON-schema conformance (definition `$schema` fields)**: Every definition part fabio synthesizes that MS marks `$schema`-required MUST include it. Verified/fixed: report `definition.pbir` (`--dataset` binding) emits `report/definitionProperties/1.0.0` (the 6-field `byConnection`/bind-by-id shape; Fabric normalizes stored form to 2.0.0); semantic-model `definition.pbism` emits `semanticModel/definitionProperties/1.0.0` + `version`; ontology `entityType`/`relationshipType` and operations-agent already emit `1.0.0`. The `.platform` file is emitted at `platformProperties/2.1.0` and now round-trips `metadata.sensitivityLabelId` (2.1.0's added field): `parse_platform_file` reads it and `deploy apply` applies it on create as a fallback when no `governance.metadata.json` sidecar label is present (closes the interop gap where Git-integration/fabric-cicd repos carry the label in `.platform`, which fabio previously dropped). Pure builders `build_dataset_pbir` (report.rs), `build_pbism` (semantic_model/crud.rs), `build_platform_json` (deploy/platform.rs) are unit-tested against the MS `required`/version constraints; all three fixes are live-validated (report + semantic-model create round-trip the `$schema`; `deploy export` emits 2.1.0 `.platform` and `deploy plan` re-parses it cleanly). See `.agents/API-BEHAVIORS-DISCOVERED.md` "Fabric JSON Schema Conformance".
 - **Power BI Project (PBIP/PBIR) report authoring for coding agents**: fabio treats the documented PBIP/PBIR plain-text format (`https://learn.microsoft.com/power-bi/developer/projects/projects-overview`) as the agent-native way to author Power BI reports. Two commands close the biggest gaps: **`report validate --source <path>`** (OFFLINE structural + `$schema` validation of a `.Report` folder, a `definition.pbir`, or a PBIP root — required PBIR files, JSON validity, byPath-vs-byConnection, version/format compatibility; machine-readable `code`s; non-zero exit on invalid), and **`report create --definition <folder>`** (creates a FULL multi-page PBIR report from a generated folder — gathers all files recursively, validates first, optional `--dataset` rebinds `definition.pbir` to a concrete model by connection). PBIR logic lives in `src/commands/report_pbir.rs` (sibling module of `report.rs` via `#[path]`, since report.rs is near the 1500-line limit); pure helpers `validate_report_folder`/`validate`/`gather_report_parts`/`rebind_pbir_part` are unit-tested and the full export→validate→create-from-folder→render loop is live-validated. **`deploy` .platform synthesis (raw Desktop PBIP)**: `deploy plan/apply/validate` now discover a raw Power BI Desktop PBIP folder that has NO Git-integration `.platform` sidecar — `synthesize_platform_metadata` (in `deploy/platform.rs`) infers the item from the folder-name suffix (`<name>.Report` → Report + required `definition.pbir`; `<name>.SemanticModel` → SemanticModel + required `definition.pbism`), but ONLY when the suffix maps to one of those two PBIP types AND the type's entry-point definition file exists (so an arbitrary folder is never misclassified — it recurses as a plain folder instead). The synthesized item has `logical_id: None`, so rename tracking is unavailable (the plan warns "no logicalId"); items match deployed items by `(type, name)`. Report→model binding still works because it goes through the existing byConnection name-resolver (`resolve_report_byconnection_model_id`): the model deploys first and is registered in the name→id map, and the report's `initial catalog=<model_name>` (v2 PBIR) rebinds to the newly created model's id. Live-validated: export SalesReport + sales_semantic_model → strip every `.platform` → `deploy plan` discovers/types both → `deploy apply` to a fresh workspace creates both AND the report's `semanticmodelid` rebinds to the newly created model. `.pbi/` Desktop user state is excluded from parts (existing `read_parts_recursive` skip). Roadmap: PBIR scaffolding from a compact page/visual spec. **Full JSON-Schema (per-property) conformance validation was investigated and closed as "won't implement"**: real Fabric reports declare body-schema `$schema` versions that are NOT published in `microsoft/json-schemas` (e.g. a live-exported `visual.json` declares `visualContainer/2.11.0` while upstream maxes at `2.9.0` — a 404), so offline body conformance can never match a real file, and only the stable `definitionProperties`/`semanticModel` schemas (which `definition.pbir`/`definition.pbism` declare and which real files DO conform to) could be validated — adding only strictness over the existing structural checks, not worth promoting `jsonschema` to a runtime dependency. See `.agents/API-BEHAVIORS-DISCOVERED.md` "PBIR body-schema conformance is INFEASIBLE offline". (Folder-based `semantic-model` TMDL ingestion outside deploy is now DONE — `semantic-model create --definition <folder>`.) See `.agents/API-BEHAVIORS-DISCOVERED.md` "Power BI Project (PBIP) / PBIR Report Support".
 - **Semantic-model schema introspection (Analysis Services "Schema Rowsets" over DAX)**: Fabric semantic models are Analysis Services tabular models, so their metadata is queryable via the DAX `INFO.VIEW.*` functions (the modern, readable form of the `TMSCHEMA_*` schema rowsets/DMVs) through the same Power BI `executeQueries` REST endpoint fabio already uses for DAX. New commands `semantic-model list-tables|list-columns|list-measures|list-relationships` run `EVALUATE INFO.VIEW.{TABLES,COLUMNS,MEASURES,RELATIONSHIPS}()` and return readable model metadata (StorageMode incl. Direct Lake, DataType, SummarizeBy, FormatString, Cardinality, CrossFilteringBehavior, LineageTag, …) WITHOUT fetching/parsing the TMDL/TMSL definition — the agent-native way to understand a model before writing DAX or wiring a report. The raw `INFO.TABLES()`/`INFO.COLUMNS()` variants are rejected by `executeQueries` (HTTP 400) — only `INFO.VIEW.*` works. DAX-bracketed keys (`[Name]`) are stripped for agent-friendly output (`strip_bracket_keys`, unit-tested); the create→introspect→delete loop is live-validated (`semantic_model_schema_introspection_lifecycle`). Enhanced/granular refresh: `semantic-model refresh` gained `--objects` (JSON `[{table,partition?}]`), `--commit-mode` (transactional|partialBatch), `--max-parallelism`, `--retry-count` — the TMSL `refresh` command's granular options mapped onto the Power BI enhanced-refresh API (registers as `ViaEnhancedApi`); pure `build_refresh_body`/`parse_refresh_objects`/`normalize_commit_mode` are unit-tested, live-verified on a Direct Lake table. The enhanced-refresh lifecycle is completed by `refresh-details --refresh-id` (object-level status) and `cancel-refresh --refresh-id` (destructive, cancels an in-progress job). Scheduled (automatic) refresh: `get-refresh-schedule`/`update-refresh-schedule` (Power BI `refreshSchedule`) with typed flags (`--enabled/--days/--times/--local-time-zone-id/--notify-option`); two API constraints are enforced client-side — times must be on the full/half hour (`HH:00`/`HH:30`) and disabling must be sent alone (the API rejects changing other settings while disabling). Dataset gateway binding: `get-bound-gateway-datasources` (read-only) + `bind-to-gateway --gateway-id` (mutation, dry-run-guarded) mirror the `bind-connection`/`unbind-connection` pair for on-premises/VNet gateways; `bind-to-gateway`'s happy path needs a real gateway (a no-op on cloud/Direct Lake models). Deliberately NOT added (probed, not cleanly reachable/valuable here): `Default.DiscoverGateways` (401, needs gateway-admin), `executeQueries` impersonation (needs RLS roles), `queryScaleOut` (Premium, `StorageModeNotSupported` on Direct Lake) — all reachable via `rest call --api powerbi`. See `.agents/API-BEHAVIORS-DISCOVERED.md` "Analysis Services specs → fabio surface".
- **Warehouse execution plan capture**: `warehouse plan` / `sql-database plan` / `sql-endpoint plan` — uses `SET SHOWPLAN_XML ON` via TDS to capture estimated execution plans without executing the query. Returns plan XML in structured JSON (`{"statementCount": N, "plans": [{"statementIndex": i, "planXml": "<ShowPlanXML...>"}]}`). Safe for DDL/DML (not executed). Works on Warehouse, Lakehouse SQL Endpoint, and SQL Database.
- **Warehouse query insights**: `warehouse queries-running|queries-frequent|queries-long-running|queries-history|queries-kill` — TDS queries against `sys.dm_exec_requests` and `queryinsights.*` schema views. `queries-kill` executes `KILL <session_id>` (mutating, guarded by `dry_run_guard`). Note: `sys.dm_exec_requests` on Fabric does NOT have `login_name` column (it's in `sys.dm_exec_sessions`).
- **Warehouse statistics management**: `warehouse statistics-list|statistics-show|statistics-create|statistics-update|statistics-delete` — TDS-based CRUD for user-defined statistics. `statistics-list` queries `sys.stats` + `sys.stats_columns` + `sys.tables` (works on both Warehouse and SQL endpoints). `statistics-show` uses `DBCC SHOW_STATISTICS` with auto-lookup of owning table via `sys.stats`. Note: `sys.dm_db_stats_properties` is NOT supported on Lakehouse SQL endpoints — removed from list query.
- **Warehouse module directory structure**: Refactored from single `warehouse.rs` (1357 lines) into `warehouse/` directory module: `mod.rs` (enum + dispatch + shared helpers), `crud.rs`, `query.rs`, `admin.rs`, `restore_points.rs`, `insights.rs`, `statistics.rs`.
- **SQL Database insights and statistics**: Same query monitoring and statistics CRUD as warehouse — `sql-database queries-running|queries-history|queries-kill|statistics-list|statistics-show|statistics-create|statistics-update|statistics-delete`. Uses `resolve_sql_connection()` for different TDS connection resolution (host+port vs connection string).
- **SQL Endpoint insights**: `sql-endpoint queries-running|queries-frequent|queries-long-running|queries-history` — read-only query monitoring (no kill, endpoints are read-only). Same `queryinsights.*` views as warehouse.
- **Lakehouse plan and insights**: `lakehouse plan|queries-running|queries-frequent|queries-long-running|queries-history` — direct discoverability for lakehouse users (previously only accessible via `fabio warehouse <cmd> --id <lakehouse_id>` workaround). Resolves connection from lakehouse `sqlEndpointProperties.connectionString`.
- **KQL Database query monitoring**: `kql-database queries-running|journal|queries-completed` — uses `.show running queries`, `.show journal`, `.show queries` management commands via Kusto REST mgmt endpoint (`/v1/rest/mgmt`). Reuses existing `kql_utils::execute_kql` infrastructure which auto-routes `.show` commands to mgmt endpoint.
- **Documentation website**: `docs/` is an Astro + Starlight site organized with the Diátaxis framework. Its browser-searchable command reference is generated at build time from `src/commands/context/data/agent/commands.json`, and `.github/workflows/docs.yml` publishes it to GitHub Pages.


## Relevant Files

The full list of source files, test files, and config files is maintained in:

**File:** `.agents/RELEVANT-FILES.md`

Reference this file when looking up specific source locations or adding new files to the documentation.

## Documentation Website (MANDATORY)

The user-facing documentation site lives in `docs/` — an [Astro](https://astro.build) + [Starlight](https://starlight.astro.build) static site organized with the [Diátaxis](https://diataxis.fr) framework (Tutorials / How-to guides / Explanation / Reference). It is published to GitHub Pages and served from the custom apex domain at `https://ismaelmejia.com/fabio/`. Full-text search is provided by Pagefind (built in).

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
ghcr.io/iemejia/fabio:0.52.0       # release version
ghcr.io/iemejia/fabio:0.52         # major.minor
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
