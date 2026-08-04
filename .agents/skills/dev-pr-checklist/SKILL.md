---
name: dev-pr-checklist
description: "Pre-commit and pre-push validation for fabio contributions. Run this skill before committing or creating a PR to ensure code quality, formatting, test coverage, and documentation are all correct. Invoke when: ready to commit, preparing a PR, reviewing changes before push."
---

# PR Checklist for Fabio

Run this checklist before every commit. Each step must pass before proceeding to the next.

**Note:** The project uses [prek](https://prek.j178.dev) pre-commit hooks (`prek.toml`). When installed (`cargo install prek && prek install`), Steps 1-2 run automatically on `git commit`. Tests (Step 3) are NOT in hooks — always run them manually. Do NOT bypass hooks with `--no-verify`.

## Step 1: Format

```bash
cargo fmt -- --check
```

If it fails, fix with `cargo fmt` and re-check.

## Step 2: Lint

```bash
cargo clippy --tests -- -D warnings
```

Fix ALL warnings. Common issues:
- Unused imports — remove them, don't leave for later
- `case_sensitive_file_extension_comparisons` — use `Path::extension()` instead of `ends_with(".json")`
- `too_many_lines` — split the function or add `#[allow(clippy::too_many_lines)]`
- `doc_markdown` — wrap identifiers in backticks in doc comments

## Step 3: Test

```bash
cargo test
```

All tests must pass. If you added new code, verify it has tests.

> If you added or renamed a subcommand flag, the `no_subcommand_flag_collides_with_global`
> test (in `cli.rs`) will FAIL if the flag shadows a global flag (`--query`,
> `--output`/`-o`, `--json`, `--quiet`, `--force`, `--dry-run`, `--limit`,
> `--all`, `--continuation-token`, `--profile`, `--verbose`/`-v`, `--lro-timeout`,
> `--readonly`, `--wrap-untrusted`, `--enable-commands`, `--disable-commands`).
> Rename the local flag or read the global instead — never let a subcommand
> redefine a global (it silently captures/transforms the value).

## Step 4: Regenerate auto-generated files (if commands changed)

Only needed if you added, modified, or removed commands/flags:

```bash
cargo test generate_agent_schema -- --ignored
cargo test agent_schema_covers
```

Skipping this leaves `commands.json` stale: the new subcommand won't be exposed as an MCP tool and can't be referenced by `--enable-commands` / `--disable-commands` policy patterns. The `agent_schema_covers_*` drift tests fail CI if you forget.

## Step 5: Self-review

Run `git diff --staged` (or `git diff` if not yet staged) and review every hunk:

- Logic errors, off-by-one mistakes, incorrect assumptions
- Missing error handling or edge cases
- Copy-paste errors (wrong variable names, leftover placeholder text)
- Inconsistencies with existing code patterns
- Dead code, unused imports, debug artifacts (`println!`, `dbg!`, `eprintln!`)
- TODO comments without corresponding implementation
- Naming inconsistencies with the codebase style

**RULE:** If you find any issue, fix it and restart from Step 1. Do NOT commit known problems.

## Step 6: Check documentation updates

If you added new features or commands, verify:

- [ ] AGENTS.md updated (Progress > Done, Key Decisions, Relevant Files, API Behaviors)
- [ ] `commands.json` regenerated (Step 4)
- [ ] Best-practice or workflow added if applicable (just drop a `.json` file in `src/commands/context/data/best_practices/` or `workflows/`)
- [ ] Output examples added for non-obvious response shapes (`src/commands/context/data/examples/`)
- [ ] README.md updated if user-facing behavior changed

## Step 7: Check irreversible operation safety

If your change adds or modifies a destructive operation (deletes data, overwrites
without backup, replaces a definition, kills a session/job, or is otherwise
irreversible), verify the FULL guardrail stack (see AGENTS.md → "Standard
guardrail stack for a NEW destructive command"):

- [ ] `--dry-run` guard via `output::dry_run_guard(cli, "<group> <cmd>", &preview)`, returning early before any mutating call
- [ ] `--readonly` enforced (mutation routes through a client `post`/`put`/`patch`/`delete` helper that calls `guard_readonly`)
- [ ] `"destructive": true` (and `"mutates": true`) confirmed in `commands.json` after `generate_agent_schema` — set manually for non-`delete*`-named ops (`reset`, `kill`, `prune`, `update-definition`, `--hard-delete`, `--force*`)
- [ ] Blast-radius input guard for catastrophic inputs (empty/root path, match-all glob, missing filter) — a pure `validate_*` fn that fails before any network call, with a unit test
- [ ] `FabioError::with_hint()` used when suggesting safety-bypass flags
- [ ] New safety-bypass flags added to `DANGEROUS_FLAGS` in `src/agent.rs`
- [ ] `"destructive": true/false` included in batch/plan output if applicable
- [ ] Protected types added to `PROTECTED_DELETE_TYPES` if new data-bearing item type
- [ ] e2e test for the `--dry-run` output AND for the blast-radius guard error
- [ ] Removal verb is `delete` (never `remove`)

## Step 8: Commit

```bash
git add <files>
git status  # verify only intended files staged
git commit -m "<type>: <description>"
```

Commit message format: imperative mood, concise subject (50 chars), body if needed.
Types: `feat`, `fix`, `docs`, `refactor`, `test`, `chore`.
Include `Assisted-by:` trailer for AI attribution.

## Step 9: Pre-push validation

Before pushing, run the cross-compilation check:

```bash
./scripts/cross-check.sh
```

This catches Windows/macOS/ARM64 compilation issues that local tests miss.
Iterate faster with: `./scripts/cross-check.sh --target windows-x64`

## Quick Reference

| Step | Command | Fix |
|------|---------|-----|
| Format | `cargo fmt -- --check` | `cargo fmt` |
| Lint | `cargo clippy --tests -- -D warnings` | Fix each warning |
| Test | `cargo test` | Fix failing tests |
| Regen | `cargo test generate_agent_schema -- --ignored` | Only if commands changed |
| Cross | `./scripts/cross-check.sh` | Fix platform-specific code |
