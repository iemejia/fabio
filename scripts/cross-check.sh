#!/usr/bin/env bash
# scripts/cross-check.sh — Validate lint + compilation across all CI targets from Linux.
#
# Runs `cargo clippy --tests -- -D warnings` per target (the EXACT command CI
# runs) so platform-specific lints are caught locally before pushing. This is
# what catches lints like `clippy::large_futures`, whose future-size verdict is
# target-dependent and can fire ONLY on windows-msvc / apple-darwin while the
# host Linux clippy passes. Uses:
#   - cargo-zigbuild: provides zig as the C cross-compiler for macOS and linux-arm64-musl
#   - cargo-xwin: provides Windows SDK headers for Windows MSVC targets
#   - OpenSSL stub directory: satisfies openssl-sys build script for Windows target
#
# Prerequisites (run with --setup to install automatically):
#   System:  lld, clang, zig, musl-tools, libssl-dev
#   Cargo:   cargo-xwin, cargo-zigbuild
#   Rustup:  targets for windows-msvc, apple-darwin, linux-musl
#
# Usage:
#   ./scripts/cross-check.sh                           # check all 5 targets
#   ./scripts/cross-check.sh --target windows-x64      # single target
#   ./scripts/cross-check.sh --setup                   # install all prerequisites
#
# Supported --target values:
#   linux-x64, linux-arm64, macos-x64, macos-arm64, windows-x64
#
# Note: windows-arm64 is excluded by default due to a known ring crate build
# issue with cargo-xwin on aarch64. Windows x64 covers all cfg(windows) Rust
# code paths — arm64-specific issues are link-time only.

set -euo pipefail

# ── Target definitions ──────────────────────────────────────────────────────
# Format: "name|rust_triple|tool"
# tool: native, zigbuild, xwin
TARGETS=(
  "linux-x64|x86_64-unknown-linux-musl|native"
  "linux-arm64|aarch64-unknown-linux-musl|zigbuild"
  "macos-x64|x86_64-apple-darwin|zigbuild"
  "macos-arm64|aarch64-apple-darwin|zigbuild"
  "windows-x64|x86_64-pc-windows-msvc|xwin"
)

# ── Defaults ────────────────────────────────────────────────────────────────
FILTER=""          # empty = all targets
DO_SETUP=false
FMT_CHECK=true
PASSED=0
FAILED=0
SKIPPED=0
RESULTS=()
OPENSSL_STUB=""

# ── Colors (disabled if not a terminal) ─────────────────────────────────────
if [ -t 1 ]; then
  GREEN='\033[0;32m'; RED='\033[0;31m'; YELLOW='\033[0;33m'
  BLUE='\033[0;34m'; BOLD='\033[1m'; RESET='\033[0m'
else
  GREEN=''; RED=''; YELLOW=''; BLUE=''; BOLD=''; RESET=''
fi

# ── Helpers ─────────────────────────────────────────────────────────────────
info()  { echo -e "${BLUE}[info]${RESET} $*"; }
ok()    { echo -e "${GREEN}[pass]${RESET} $*"; }
fail()  { echo -e "${RED}[FAIL]${RESET} $*"; }
warn()  { echo -e "${YELLOW}[warn]${RESET} $*"; }
header(){ echo -e "\n${BOLD}── $* ──${RESET}"; }

usage() {
  cat <<EOF
Usage: $(basename "$0") [OPTIONS]

Options:
  --target <name>   Only check one target (e.g., windows-x64, macos-arm64)
  --no-fmt          Skip cargo fmt check
  --setup           Install all prerequisites and exit
  -h, --help        Show this help

Targets: linux-x64, linux-arm64, macos-x64, macos-arm64, windows-x64
EOF
  exit 0
}

# ── Parse arguments ─────────────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
  case $1 in
    --target)  FILTER="$2"; shift 2 ;;
    --no-fmt)  FMT_CHECK=false; shift ;;
    --setup)   DO_SETUP=true; shift ;;
    -h|--help) usage ;;
    *) echo "Unknown option: $1"; usage ;;
  esac
done

# ── Setup mode ──────────────────────────────────────────────────────────────
do_setup() {
  header "Installing prerequisites"

  info "Installing system packages (lld, clang, musl-tools, libssl-dev)..."
  sudo apt update -qq && sudo apt install -y -qq lld clang musl-tools libssl-dev

  if ! command -v zig &>/dev/null; then
    info "Installing zig via snap..."
    sudo snap install zig --classic --beta
  else
    info "zig already installed: $(zig version)"
  fi

  info "Installing cargo-xwin and cargo-zigbuild..."
  cargo install cargo-xwin cargo-zigbuild

  info "Adding rustup targets..."
  rustup target add \
    x86_64-unknown-linux-musl \
    aarch64-unknown-linux-musl \
    x86_64-pc-windows-msvc \
    x86_64-apple-darwin \
    aarch64-apple-darwin

  echo ""
  ok "Setup complete. Run $(basename "$0") to validate all targets."
}

if $DO_SETUP; then
  do_setup
  exit 0
fi

# ── Prerequisite checks ────────────────────────────────────────────────────
check_prereqs() {
  local missing=0

  for cmd in lld clang musl-gcc; do
    if ! command -v "$cmd" &>/dev/null; then
      warn "Missing: $cmd (install via: sudo apt install lld clang musl-tools)"
      missing=1
    fi
  done

  if ! command -v zig &>/dev/null; then
    warn "Missing: zig (install via: sudo snap install zig --classic --beta)"
    missing=1
  fi

  for tool in cargo-xwin cargo-zigbuild; do
    if ! command -v "$tool" &>/dev/null; then
      warn "Missing cargo tool: $tool (install via: cargo install $tool)"
      missing=1
    fi
  done

  if [ $missing -ne 0 ]; then
    echo ""
    warn "Some prerequisites are missing. Run: $(basename "$0") --setup"
    exit 1
  fi
}

# ── OpenSSL stub directory (Windows target only) ────────────────────────────
# The Windows target needs OpenSSL headers/libs for client_certificate feature.
# Since clippy is check-based (no linking), stub .lib files suffice.
# Linux/macOS targets do NOT use OpenSSL.
setup_openssl_stub() {
  OPENSSL_STUB=$(mktemp -d)
  mkdir -p "$OPENSSL_STUB/include/openssl" "$OPENSSL_STUB/lib"

  # Symlink system OpenSSL headers (satisfies openssl-sys build script)
  if [ -d /usr/include/openssl ]; then
    ln -sf /usr/include/openssl/* "$OPENSSL_STUB/include/openssl/"
  fi

  # Copy platform-specific headers on Debian/Ubuntu
  local arch_dir="/usr/include/$(dpkg-architecture -qDEB_HOST_MULTIARCH 2>/dev/null || echo x86_64-linux-gnu)/openssl"
  if [ -d "$arch_dir" ]; then
    cp "$arch_dir"/*.h "$OPENSSL_STUB/include/openssl/" 2>/dev/null || true
  fi

  # Create empty .lib stubs for Windows (clippy is check-based, no linking)
  touch "$OPENSSL_STUB/lib/libssl.lib" "$OPENSSL_STUB/lib/libcrypto.lib"
  touch "$OPENSSL_STUB/lib/ssl.lib" "$OPENSSL_STUB/lib/crypto.lib"

  info "OpenSSL stub directory: $OPENSSL_STUB"
}

cleanup_openssl_stub() {
  if [ -n "$OPENSSL_STUB" ] && [ -d "$OPENSSL_STUB" ]; then
    rm -rf "$OPENSSL_STUB"
  fi
}
trap cleanup_openssl_stub EXIT

# ── Check functions ─────────────────────────────────────────────────────────
# Each runs the exact lint CI runs (`cargo clippy --tests -- -D warnings`) for
# the given target, so platform-specific lints (e.g. clippy::large_futures) are
# caught before pushing — not just type/borrow errors.
run_native() {
  local triple=$1
  cargo clippy --tests --target "$triple" -- -D warnings 2>&1
}

run_zigbuild() {
  local triple=$1
  # cargo-zigbuild clippy: zig provides the C cross-compiler for build scripts
  cargo-zigbuild clippy --tests --target "$triple" -- -D warnings 2>&1
}

run_xwin() {
  local triple=$1
  # cargo xwin clippy: xwin provides Windows SDK; OpenSSL stub satisfies
  # the openssl-sys build script for the client_certificate feature
  OPENSSL_NO_VENDOR=1 OPENSSL_DIR="$OPENSSL_STUB" \
    cargo xwin clippy --tests --target "$triple" -- -D warnings 2>&1
}

# ── Main ────────────────────────────────────────────────────────────────────
START_TIME=$SECONDS

header "Fabio cross-target lint (clippy -D warnings)"
check_prereqs
setup_openssl_stub

# Format check (once, target-independent)
if $FMT_CHECK; then
  header "cargo fmt --check"
  if cargo fmt -- --check 2>&1; then
    ok "Formatting OK"
  else
    fail "Formatting errors found (run: cargo fmt)"
    FAILED=$((FAILED + 1))
    RESULTS+=("fmt|FAIL")
  fi
fi

# Per-target checks
for entry in "${TARGETS[@]}"; do
  IFS='|' read -r name triple tool <<< "$entry"

  # Filter if --target was given
  if [ -n "$FILTER" ] && [ "$name" != "$FILTER" ]; then
    SKIPPED=$((SKIPPED + 1))
    continue
  fi

  header "$name ($triple) [$tool]"
  target_start=$SECONDS

  set +e
  case $tool in
    native)   output=$(run_native "$triple" 2>&1) ; rc=$? ;;
    zigbuild) output=$(run_zigbuild "$triple" 2>&1) ; rc=$? ;;
    xwin)     output=$(run_xwin "$triple" 2>&1) ; rc=$? ;;
  esac
  set -e

  elapsed=$((SECONDS - target_start))

  if [ $rc -eq 0 ]; then
    ok "$name passed (${elapsed}s)"
    PASSED=$((PASSED + 1))
    RESULTS+=("$name|PASS|${elapsed}s")
  else
    fail "$name failed (${elapsed}s)"
    echo "$output" | tail -30
    FAILED=$((FAILED + 1))
    RESULTS+=("$name|FAIL|${elapsed}s")
  fi
done

# ── Summary ─────────────────────────────────────────────────────────────────
TOTAL_TIME=$((SECONDS - START_TIME))

header "Results"
printf "  %-20s %s\n" "TARGET" "STATUS"
printf "  %-20s %s\n" "------" "------"
for r in "${RESULTS[@]}"; do
  IFS='|' read -r rname rstatus rtime <<< "$r"
  if [ "$rstatus" = "PASS" ]; then
    printf "  %-20s ${GREEN}%s${RESET}  %s\n" "$rname" "$rstatus" "$rtime"
  else
    printf "  %-20s ${RED}%s${RESET}  %s\n" "$rname" "$rstatus" "${rtime:-}"
  fi
done

echo ""
info "Passed: $PASSED  Failed: $FAILED  Skipped: $SKIPPED  Total: ${TOTAL_TIME}s"

if [ $FAILED -gt 0 ]; then
  exit 1
fi
