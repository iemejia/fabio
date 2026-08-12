You are working on fabio, a Rust CLI for Microsoft Fabric.

The Microsoft Fabric REST API specs (github.com/microsoft/fabric-rest-api-specs) have been updated.

Here are the commits since our last sync:

${SPEC_COMMITS}

Here is a detailed summary of file changes:

${SPEC_CHANGES_SUMMARY}

The full diff is available at /tmp/spec-changes.diff and the full spec repo is at ./fabric-rest-api-specs/.

## Goal: Complete Parity

Your goal is **complete implementation of every API change**. Every modification in the spec diff must be reflected in fabio. Do not skip minor changes, optional fields, or cosmetic updates. Obviously prioritize high-impact changes first (new endpoints, breaking schema changes, new required fields), but then continue through every remaining modification until fabio covers 100% of what the spec diff introduced.

## Systematic Analysis

Before making changes, perform a complete inventory:

1. **Read the full diff** at /tmp/spec-changes.diff. Do NOT rely solely on the summary — read the actual diff to catch every field addition, removal, enum change, description update, and schema modification.

2. **Build a change manifest** — For each modified spec file, enumerate:
   - New endpoints (HTTP method + path)
   - New request body fields (required and optional)
   - New response body fields
   - New query parameters
   - New/changed enum values
   - Changed field types or constraints
   - Deprecated or removed fields
   - New error codes or status codes
   - Changed descriptions that reveal behavioral semantics
   - New API versions or preview flags

3. **Map each change to fabio** — For every item in your manifest, identify the corresponding fabio source file in `src/commands/` or `src/client.rs`. If no corresponding code exists yet, it means a new subcommand or handler is needed.

4. **Implement ALL changes** — Do not prioritize. Work through your manifest item by item until every change is implemented.

## Implementation Checklist (apply to EVERY change)

For each spec change, complete ALL applicable steps:

### New endpoint
- [ ] Add subcommand in the appropriate `src/commands/<group>/` module
- [ ] Add clap derive struct with all flags (required and optional)
- [ ] Implement request builder with correct HTTP method, path, query params, headers
- [ ] Handle response deserialization
- [ ] Add examples to `commands.json` (1-3 practical examples)
- [ ] Add output example in `data/examples/` if response shape is non-standard
- [ ] Add unit test for request serialization
- [ ] Add E2E test with `--dry-run`

### New/changed request field
- [ ] Add the field to the request struct (with correct `serde` attributes)
- [ ] Add corresponding clap flag (with help text from spec description)
- [ ] Mark required fields as non-optional in clap; optional fields use `Option<T>`
- [ ] Update any existing tests that serialize this request
- [ ] If it's a PascalCase enum value, use `clap::ValueEnum` with explicit string mappings

### New/changed response field
- [ ] Add the field to the response struct
- [ ] Update output formatting (table display, JSON serialization)
- [ ] Update output examples if the shape changed materially
- [ ] Update JMESPath documentation if the field is commonly queried

### New enum value
- [ ] Add variant to the Rust enum
- [ ] Update `clap::ValueEnum` or `possible_values`
- [ ] Update `commands.json` flag description to list valid values
- [ ] Add test case exercising the new variant

### Removed/deprecated field
- [ ] Remove from request struct (or mark `#[deprecated]` if still accepted)
- [ ] Remove from clap flags
- [ ] Document in `.agents/API-BEHAVIORS-DISCOVERED.md` under appropriate section
- [ ] Update tests

### Changed description/semantics
- [ ] Update clap help text
- [ ] Update `commands.json` description/notes
- [ ] If behavior changed, document in `.agents/API-BEHAVIORS-DISCOVERED.md`

### New query parameter requirement (beta/preview flags)
- [ ] Add to URL construction in the handler
- [ ] Document in AGENTS.md API behaviors section

## Agent Knowledge Enrichment

fabio's agent discoverability is entirely runtime-based. There are NO separate documentation files to maintain (no COMMANDS.md, no EXAMPLES.md). All agent-facing knowledge lives in:

- `src/commands/context/data/agent/commands.json` — Auto-generated command schema with manually-preserved `examples` arrays
- `src/commands/context/data/examples/*.json` — Output shape examples (registered in `examples.rs`)
- `src/commands/context/data/schemas/*.json` — Item definition schemas (registered in `schemas.rs`)
- `src/commands/context/data/workflows/*.json` — Multi-step workflow recipes (registered in `workflows.rs`)
- `src/commands/context/data/best_practices/*.json` — Operational guidance (registered in `best_practices.rs`)

### commands.json — Command Examples (MANDATORY for new commands)

After adding a new subcommand, add practical CLI examples to its `examples` field in `commands.json`. These are preserved across regeneration. Add 1-3 examples per subcommand showing non-obvious flag usage:

```json
"my-new-command": {
  "description": "...",
  "flags": {...},
  "mutates": true,
  "returns": "object",
  "examples": [
    "fabio <group> my-new-command --workspace $WS --id $ID --some-flag Value"
  ]
}
```

After adding examples, regenerate to pick up structural changes:
```bash
cargo test --bin fabio generate_agent_schema -- --include-ignored
```

### Output Shape Examples (MANDATORY for non-obvious responses)

If the new command returns non-trivial JSON (nested objects, aggregated results, non-standard envelope), add a file in `data/examples/` and register it in `examples.rs`:

```json
{
  "command": "fabio <group> <cmd> --workspace $WS ...",
  "description": "What this shows",
  "response": {"data": {...}},
  "notes": "Important agent-relevant notes",
  "query_examples": [
    {"query": "data.id", "description": "Extract the ID"}
  ]
}
```

Standard CRUD responses (create returns object with id/displayName, list returns `{"data":[...],"count":N}`, delete returns `{"status":"deleted","id":"..."}`) do NOT need output examples — agents already know these patterns.

### Item Definition Schemas (for new item types)

If the spec introduces a new item type, add `data/schemas/<type>.json` and register in `schemas.rs`. Structure: `type`, `description`, `create_command`, `definition_format`, `definition_parts`, `creation_body_template`, `flags`, `notes`, `related_commands`.

### Workflow Recipes (for new multi-step flows)

If spec changes reveal a new multi-step workflow (create-configure-publish sequence, new integration between item types), add `data/workflows/<name>.json` and register in `workflows.rs`. Structure: `name`, `description`, `prerequisites`, `steps` (numbered with `command` + `description`), `tips`.

### Best Practices (for new operational patterns)

If the spec reveals new gotchas (required field ordering, beta/preview flags, new throttling behavior, PascalCase requirements, non-standard response keys), update `data/best_practices/<topic>.json`.

## Tests

### Unit tests
When the spec provides example request/response JSON, add unit tests in the relevant `src/commands/*.rs` `#[cfg(test)]` module that verify serialization/deserialization against those examples. Cover edge cases (optional fields, enum variants, default values).

### E2E tests
Add or update integration tests in `tests/e2e_*.rs` that exercise new or changed endpoints. Include `--dry-run` tests verifying the constructed request body matches the spec format.

## AGENTS.md API Behaviors

Document ALL non-obvious API behaviors discovered from the spec in `.agents/API-BEHAVIORS-DISCOVERED.md`:
- Required vs optional fields that differ from intuition
- Non-standard response keys (not `"value"`)
- PascalCase vs camelCase requirements
- Query parameter requirements (`?beta=true`, `?preview=true`)
- Discriminated union patterns in request bodies
- Fields the server auto-adds or strips
- Error codes and their meaning
- New constraints or validation rules
- Rate limiting or throttling changes
- Breaking changes in existing endpoints

## README.md

Update only if the spec reveals major new capabilities that change the project's feature description (new item type categories, new authentication methods, new deployment capabilities).

## Completeness Verification

Before finishing, verify completeness:

1. **Re-read the diff** — Go through /tmp/spec-changes.diff one more time and check each hunk against your changes. Every addition/removal in the spec must have a corresponding fabio change.
2. **Count changes** — Your PR body must include a count: "X new endpoints, Y new fields, Z enum changes, W deprecations implemented."
3. **No TODOs** — Do not leave `// TODO: implement this from spec` comments. Implement it now or explain in the PR body why it's blocked.

## Final Steps

1. Run the mandatory pre-commit validation defined in AGENTS.md (section "Pre-Commit Validation (MANDATORY)"). All steps must pass with zero errors and zero warnings.
2. Regenerate `commands.json`:
   ```bash
   cargo test --bin fabio generate_agent_schema -- --include-ignored
   ```
3. **Write three files** that describe the actual work you did. The CI workflow reads these to build the commit and PR — if you omit them (or make them generic), the PR falls back to an unhelpful title and message. Base each on the changes you actually implemented, NOT on a template.

   - **`/tmp/commit-title.txt`** — one line, used verbatim as BOTH the commit subject and the PR title. Write a **Conventional Commit subject** (repo convention — see AGENTS.md; the exact format is validated by CI, which falls back to a generic title if yours is missing or invalid). The one thing that matters here: summarize WHAT actually changed with concrete nouns/counts — e.g. `feat(eventhouse): add scale-compute endpoint and minReplicas field` — NOT a generic `feat: sync with latest Fabric REST API spec changes`.
   - **`/tmp/pr-body.md`** — the PR description (Markdown), readable without opening the diff: a one-paragraph summary; **spec changes processed** grouped by category (endpoints, fields, enums, deprecations, semantic changes) each mapped to the **fabio file/command** touched; a **count summary** ("X new endpoints, Y new fields, Z enum changes, W deprecations"); the upstream **spec commits** (`${SPEC_COMMITS}`); and anything that could NOT be implemented and why.
   - **`/tmp/commit-body.txt`** — the commit body below the subject (plain text, `- ` bullets, ~72-col wrap). Same substance as the PR body, condensed. Optional: if omitted, the commit uses the subject plus the spec-commit list.

## Tool Usage Rules

- You have read-only access to git (status, diff, log, show, rev-parse, ls-files, blame, branch).
- **Under NO circumstance may you run `git add`, `git commit`, or `git push`.** The CI workflow that invokes you handles all staging, committing, branch creation, and PR submission. Your job is to edit files and write /tmp/pr-body.md — nothing more.
- You may run `cargo fmt`, `cargo check`, `cargo clippy`, `cargo build`, and `cargo test` to validate your changes.
- Use `read`, `write`, and `edit` tools for file modifications.

**Aim for 100% coverage of the spec diff.** Do not skip changes because they seem minor. A missing optional field today becomes a user-reported bug tomorrow.

Follow all constraints and preferences defined in AGENTS.md, in particular the pre-commit validation rules and Windows-first compatibility requirements.
