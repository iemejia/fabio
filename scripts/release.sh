#!/usr/bin/env bash
# scripts/release.sh — Guided release workflow for fabio
# Usage: ./scripts/release.sh <version>
# Example: ./scripts/release.sh 0.23.0
set -euo pipefail

VERSION="${1:-}"
if [[ -z "$VERSION" ]]; then
  echo "Usage: $0 <version>"
  echo "Example: $0 0.23.0"
  exit 1
fi

TAG="v${VERSION}"

# ── Sanity checks ───────────────────────────────────────────────────────────

if ! command -v git-cliff &>/dev/null; then
  echo "ERROR: git-cliff is not installed. Run: cargo install git-cliff"
  exit 1
fi

if ! command -v gh &>/dev/null; then
  echo "ERROR: gh (GitHub CLI) is not installed."
  exit 1
fi

if git rev-parse "$TAG" &>/dev/null; then
  echo "ERROR: Tag $TAG already exists."
  exit 1
fi

if [[ -n "$(git status --porcelain)" ]]; then
  echo "ERROR: Working tree is dirty. Commit or stash changes first."
  exit 1
fi

echo "==> Preparing release $TAG"
echo ""

# ── Step 1: Bump version in Cargo.toml ──────────────────────────────────────

CURRENT_VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
echo "  [1/8] Version bump"
echo "        Current: $CURRENT_VERSION"
echo "        New:     $VERSION"
echo ""

sed -i "s/^version = \"$CURRENT_VERSION\"/version = \"$VERSION\"/" Cargo.toml

# Regenerate Cargo.lock with the new version
cargo check --quiet

# ── Step 2: Validate dependency freshness ───────────────────────────────────

echo "  [2/8] Checking dependency freshness..."

# Show what cargo update would change (without actually updating)
UPDATES=$(cargo update --dry-run 2>&1 || true)
if echo "$UPDATES" | grep -q "Updating"; then
  echo ""
  echo "  WARNING: Some dependencies have newer compatible versions available:"
  echo "$UPDATES" | grep "Updating" | sed 's/^/        /'
  echo ""
  echo "  Run 'cargo update' to update them, or press Enter to continue without updating."
  echo "  (If you update, the script will re-validate after.)"
  read -r -p "  Update dependencies now? [y/N] " REPLY
  if [[ "$REPLY" =~ ^[Yy]$ ]]; then
    cargo update
    echo "  Dependencies updated."
  fi
  echo ""
else
  echo "        All dependencies are up to date."
  echo ""
fi

# ── Step 3: Update version references in docs ───────────────────────────────

echo "  [3/8] Updating version references in documentation..."

sed -i "s|ghcr.io/iemejia/fabio:$CURRENT_VERSION|ghcr.io/iemejia/fabio:$VERSION|g" README.md
sed -i "s|ghcr.io/iemejia/fabio:$CURRENT_VERSION|ghcr.io/iemejia/fabio:$VERSION|g" AGENTS.md

MAJOR_MINOR=$(echo "$VERSION" | cut -d. -f1,2)
OLD_MAJOR_MINOR=$(echo "$CURRENT_VERSION" | cut -d. -f1,2)
if [[ "$MAJOR_MINOR" != "$OLD_MAJOR_MINOR" ]]; then
  sed -i "s|ghcr.io/iemejia/fabio:$OLD_MAJOR_MINOR|ghcr.io/iemejia/fabio:$MAJOR_MINOR|g" README.md
  sed -i "s|ghcr.io/iemejia/fabio:$OLD_MAJOR_MINOR|ghcr.io/iemejia/fabio:$MAJOR_MINOR|g" AGENTS.md
fi
echo "        Done."
echo ""

# ── Step 4: Run full validation ─────────────────────────────────────────────

echo "  [4/8] Running full validation..."
echo ""

echo "        cargo fmt -- --check"
if ! cargo fmt -- --check; then
  echo "ERROR: Formatting check failed. Run 'cargo fmt' to fix."
  exit 1
fi

echo "        cargo clippy --tests -- -D warnings"
if ! cargo clippy --tests -- -D warnings; then
  echo "ERROR: Clippy found warnings. Fix them before releasing."
  exit 1
fi

echo "        cargo test"
if ! cargo test; then
  echo "ERROR: Tests failed. Fix them before releasing."
  exit 1
fi

echo "        cargo audit --deny warnings"
cargo audit --deny warnings

echo "        Audit website dependencies and test glob security"
npm --prefix docs ci --ignore-scripts
npm --prefix docs audit --audit-level=low
npm --prefix docs test

echo "        ./scripts/cross-check.sh"
if [[ -x "./scripts/cross-check.sh" ]]; then
  if ! ./scripts/cross-check.sh; then
    echo "ERROR: Cross-compilation check failed. Fix platform issues before releasing."
    exit 1
  fi
else
  echo "        (skipped — cross-check.sh not found or not executable)"
fi

echo ""
echo "        All validations passed."
echo ""

# ── Step 5: Commit Cargo.toml + Cargo.lock + docs together ─────────────────

echo "  [5/8] Committing version bump..."

git add Cargo.toml Cargo.lock README.md AGENTS.md
git commit -m "chore: bump version to $VERSION"

echo "        Committed."
echo ""

# ── Step 6: Generate release notes ──────────────────────────────────────────

echo "  [6/8] Generating release notes..."
echo ""

NOTES_FILE="/tmp/fabio-release-notes-${VERSION}.md"

echo "  Changelog preview (unreleased commits):"
echo "  ---"
git cliff --unreleased --tag "$TAG"
echo "  ---"
echo ""

git cliff --unreleased --tag "$TAG" > "$NOTES_FILE"
echo "  Raw changelog written to: $NOTES_FILE"
echo "  Edit this file with curated release notes, then press Enter to continue..."
read -r

# ── Step 7: Tag and push ────────────────────────────────────────────────────

echo "  [7/8] Tagging $TAG and pushing..."

git tag "$TAG"
git push
git push origin "$TAG"

echo ""
echo "  Release workflow triggered. Binaries will be available in ~10-15 min."
echo "  Docker image will follow in ~45 min."
echo ""

# ── Step 8: Publish release notes ───────────────────────────────────────────

echo "  [8/8] Waiting for GitHub Release to be created..."

for _ in $(seq 1 60); do
  if gh release view "$TAG" &>/dev/null 2>&1; then
    echo "  Release $TAG found. Publishing release notes..."
    gh release edit "$TAG" --notes-file "$NOTES_FILE"
    echo ""
    echo "==> Done! Release $TAG published successfully."
    echo "    https://github.com/iemejia/fabio/releases/tag/$TAG"
    exit 0
  fi
  sleep 30
done

echo ""
echo "  WARNING: Timed out waiting for release to appear."
echo "  Run manually when ready: gh release edit $TAG --notes-file $NOTES_FILE"
exit 1
