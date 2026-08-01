---
name: dev-release
description: "Step-by-step release workflow for fabio. Invoke when cutting a new release version. Covers: version bump, dependency freshness, documentation updates, validation, changelog, tagging, and post-release dev version bump."
---

# Release Workflow for Fabio

Complete ALL steps in order. Do NOT skip any step.

## Step 1: Bump the Version Number

```bash
# Check current version (should be X.Y.Z-dev during development)
grep '^version' Cargo.toml | head -1

# Update to release version (remove -dev suffix)
sed -i 's/^version = ".*"/version = "0.25.0"/' Cargo.toml
```

Run `cargo check` or `cargo build` to regenerate `Cargo.lock`.

## Step 2: Validate Dependency Freshness and Security

```bash
# Check for outdated dependencies
cargo outdated --root-deps-only

# Update to latest compatible versions
cargo update

# Security audit — MUST pass with zero vulnerabilities
cargo audit
```

**Rules:**
- Update any dependency with a newer compatible version (within semver range).
- `cargo audit` MUST report zero vulnerabilities. If a CVE exists, upgrade the affected crate. If no patched version is available, evaluate the risk and add to `audit.toml` ignore list ONLY if the vulnerability is unexploitable in fabio's usage.
- For major bumps, check changelog for breaking changes.
- Reject copyleft licenses (GPL, LGPL, AGPL, SSPL). Only permissive (MIT, Apache-2.0, BSD, ISC, Zlib, Unicode-3.0).
- Run full pre-commit validation after updating dependencies.
- Check GitHub Actions versions in `.github/workflows/*.yml` — update to latest SHA-pinned versions.

## Step 3: Update Version References in Documentation

1. **README.md** — Docker image version in usage examples.
2. **AGENTS.md** — Docker & Devcontainer section version examples.

### Ensure the docs website reflects the release's CLI surface

The documentation website's command reference is generated from
`commands.json` (via `docs/scripts/generate-reference.mjs`) and auto-deploys
to GitHub Pages through `.github/workflows/docs.yml` on every push to `main`
that touches `docs/**` or `commands.json`. So the site updates itself — the
only release-time obligation is to make sure the generated metadata is current
with any commands/subcommands/flags added this cycle:

```bash
# Regenerate the source of truth for the website reference AND the sub-skills.
cargo test generate_agent_schema -- --ignored && cargo test generate_subskills -- --ignored
git status  # commands.json / .agents/skills/fabio-*/SKILL.md should be clean (no drift)
```

If `git status` shows changes here, a feature landed without regenerating —
commit them. Step 4's `cargo test` will otherwise FAIL the release: the
`agent_schema_covers_all_groups` / `agent_schema_covers_all_subcommands` /
`subskills_match_generated` drift tests are the safety net that blocks a
release whose website reference would be stale. A fourth gate,
`every_command_group_has_a_knowledge_home`, blocks a release where a command
GROUP was added without a skill family (`data/skills/*.json` `command_groups`) or
the cross-cutting allowlist — i.e. some subcommands would have no generated
sub-skill command table, so skills + context would be inconsistent with the CLI
down to the subcommand level. If it fails, add the group to the right family and
regenerate. Optionally sanity-check the
site build locally with `npm --prefix docs run check` (needs `npm ci` in
`docs/` first).

## Step 4: Run Full Validation

```bash
cargo fmt -- --check
cargo clippy --tests -- -D warnings
cargo test
cargo audit
./scripts/cross-check.sh
```

ALL must pass with zero errors, zero warnings, and zero vulnerabilities.

> This is also enforced automatically: the `release.yml` workflow runs a
> `validate` job (fmt + clippy + full `cargo test`, including the
> skills/context consistency gates such as `every_command_group_has_a_knowledge_home`
> and `subskills_match_generated`) that ALL build/publish jobs `needs:`. A tag
> whose commit fails the suite produces NO artifacts — the release hard-stops.
> Running Step 4 locally first just avoids a failed tag build.

## Step 5: Commit Cargo.toml AND Cargo.lock Together

```bash
git add Cargo.toml Cargo.lock README.md AGENTS.md
git status  # verify only intended files
git commit -m "chore: bump version to 0.25.0"
```

**Rules:**
- NEVER tag without `Cargo.lock` reflecting the exact dependency tree.
- `git status` must be clean before tagging.

## Step 6: Generate Release Notes

```bash
# Preview unreleased changes (before tagging):
git cliff --unreleased

# For the latest tag (after tagging):
git cliff --latest

# Between two specific tags:
git cliff v0.24.0..v0.25.0
```

Follow the template in `.github/RELEASE_TEMPLATE.md`:
1. Lead with impact (most user-visible features first)
2. Group related commits into single feature descriptions
3. Include command usage examples for new features
4. Stats at the end (commit count, lines changed, test coverage)

**Rules:**
- ALWAYS run `git cliff` first — do NOT rely on memory.
- Cover ALL features/fixes from the raw changelog.
- New item types and headline features go FIRST.
- CI/CD-only changes go at the end.
- Include `Full Changelog` comparison link.

## Step 7: Tag and Trigger the Release

```bash
git tag v0.25.0
git push
git push origin v0.25.0
```

CI builds 6 binaries + Docker image automatically.

## Step 8: Publish Release Notes

```bash
gh release edit v0.25.0 --notes-file release-notes.md
# or: gh release create v0.25.0 --notes-file release-notes.md --title "v0.25.0"
```

## Step 9: Post-Release — Bump to Next Dev Version

```bash
sed -i 's/^version = ".*"/version = "0.26.0-dev"/' Cargo.toml
cargo check
git add Cargo.toml Cargo.lock
git commit -m "chore: bump version to 0.26.0-dev"
git push
```

**Version lifecycle:** `0.25.0-dev` (dev) → `0.25.0` (release tag) → `0.26.0-dev` (next cycle).

## Automated Release Script

```bash
./scripts/release.sh 0.25.0
```

Automates ALL steps. Pauses for:
- Dependency update decision
- Release notes editing

Aborts on any validation failure.

## Configuration

- `cliff.toml` — git-cliff config
- `.github/RELEASE_TEMPLATE.md` — Narrative template
- `scripts/release.sh` — Automated release script
