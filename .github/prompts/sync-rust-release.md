You are working on fabio, a Rust CLI for Microsoft Fabric.

A new stable release of the Rust language is available. The project's minimum
supported Rust version (MSRV) has just been bumped from **${OLD_VERSION}** to
**${NEW_VERSION}**: the `rust-version` field in `Cargo.toml` and the MSRV
references in `README.md`, `AGENTS.md`, and `.github/copilot-instructions.md`
have ALREADY been updated for you (mechanically). `Cargo.lock` has been
refreshed.

Your job is the part that requires judgement: make the codebase build cleanly on
Rust ${NEW_VERSION} and take advantage of what the newer toolchain enables —
exactly the kind of migration a human maintainer does when bumping the MSRV.

## Goal

1. **Fix every new clippy lint** introduced by the newer toolchain.
2. **Adopt modern idioms** the new MSRV makes available, per the "Modern Rust
   idioms (MANDATORY)" section of `AGENTS.md`.
3. Leave the tree passing `cargo fmt`, `cargo clippy --tests -- -D warnings`, and
   `cargo test` with zero warnings and zero failures.

## Steps

1. **Run clippy and read every diagnostic.**
   ```bash
   cargo clippy --tests -- -D warnings
   ```
   Newer clippy versions add lints and tighten existing ones, so a codebase that
   was clean on ${OLD_VERSION} may now emit warnings. Fix ALL of them. Do not
   `#[allow(...)]` a lint to silence it unless it is a genuine false positive —
   prefer the idiomatic rewrite the lint suggests.

2. **Map each fix to the mandated modern idiom.** The `AGENTS.md` "Modern Rust
   idioms (MANDATORY)" list is the source of truth for the preferred form. Common
   examples (apply the one the lint points at):
   - `.ok().is_some_and(..)` on a `Result` → `Result::is_ok_and(..)`
   - `format!("literal with no args")` → the plain string literal / `.to_string()`
   - `opt.map_or(true, ..)` → `Option::is_none_or(..)`
   - `opt.map_or(false, ..)` / `matches!(opt, Some(x) if ..)` → `Option::is_some_and(..)`
   - raw `&s[..n]` on user/API text → `str::floor_char_boundary()`
   - nested `if let` + `if` → let chains
   - `Duration::from_secs(N * 60)` → `Duration::from_mins(N)`
   Read the actual clippy `help:` line for the exact suggested rewrite.

3. **Proactively audit for newly-stabilized idioms.** Beyond what clippy flags,
   the new toolchain may stabilize APIs or language features (e.g. new let-chain
   forms, new `std` helpers, new `const fn` capabilities) that let you simplify
   existing code the AGENTS.md idioms list calls for. Apply obvious, low-risk
   simplifications. Do NOT undertake large speculative refactors — keep the diff
   focused on the toolchain bump.

4. **If a new idiom becomes broadly relevant**, add it as a bullet to the
   "Modern Rust idioms (MANDATORY)" list in `AGENTS.md` so future contributions
   use it. Only do this for a genuinely new, generally-applicable idiom — not for
   a one-off fix.

5. **Validate.** Run the mandatory pre-commit validation from `AGENTS.md`:
   ```bash
   cargo fmt -- --check
   cargo clippy --tests -- -D warnings
   cargo test
   ```
   All must pass with zero errors, zero warnings, zero failures. Fix anything
   that fails. Respect the Windows-first compatibility rules in `AGENTS.md`
   (no Unix-only APIs, use `Path::join`, `.lines()` for CRLF, etc.).

## Write three files describing the work

The CI workflow reads these to build the commit and PR. Base each on the changes
you ACTUALLY made, not on this template. If you omit them, the PR falls back to a
generic title/message.

- **`/tmp/commit-title.txt`** — one line, used verbatim as BOTH the commit subject
  and the PR title. Must be a **Conventional Commit subject** (validated by CI).
  Summarize what changed with concrete nouns — e.g.
  `chore: bump MSRV to Rust ${NEW_VERSION} and migrate to is_ok_and`.
  A pure version bump with no code migration can be
  `chore: bump MSRV to Rust ${NEW_VERSION}`.
- **`/tmp/pr-body.md`** — the PR description (Markdown), readable without opening
  the diff: a one-paragraph summary (bumped ${OLD_VERSION} → ${NEW_VERSION}); a
  bulleted list of each lint/idiom migration grouped by kind, mapped to the
  source files touched; and a note that `cargo fmt` / `cargo clippy --tests -D
  warnings` / `cargo test` all pass. If no code migration was needed (only the
  MSRV bump), say so explicitly.
- **`/tmp/commit-body.txt`** — the commit body below the subject (plain text,
  `- ` bullets, ~72-col wrap). Same substance as the PR body, condensed.

## Tool usage rules

- You have read-only access to git (status, diff, log, show, rev-parse, ls-files,
  blame, branch).
- **Under NO circumstance may you run `git add`, `git commit`, or `git push`.**
  The CI workflow handles all staging, committing, branch creation, and PR
  submission. Your job is to edit files and write the three `/tmp` files.
- You may run `cargo fmt`, `cargo check`, `cargo clippy`, `cargo build`, and
  `cargo test` to validate your changes.
- Use `read`, `write`, and `edit` for file modifications.

Follow all constraints and preferences defined in `AGENTS.md`, in particular the
pre-commit validation rules, the modern-idiom mandate, and Windows-first
compatibility.
