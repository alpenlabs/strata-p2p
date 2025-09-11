# Root Justfile for strata-p2p
#
# Goals:
# - Valid just syntax with tabs for bodies
# - Use [group(...)] and [parallel] attributes
# - Mirror common CI tasks: clippy, tests, docs, fmt, codespell, taplo, audit, actionlint, zizmor
# - Default recipe prints `just --list`

# Export common environment to commands
set export := true
RUST_BACKTRACE := "1"
CARGO_TERM_COLOR := "always"

# Default target: show available recipes
default:
	just --list

# ----------------------------
# Core linting & formatting
# ----------------------------

# Helper to ensure cargo-hack is installed
ensure-cargo-hack:
	@if ! cargo hack --version > /dev/null 2>&1; then \
		echo "Error: cargo-hack is not installed. Please install it by running: cargo install cargo-hack"; \
		exit 1; \
	fi

# Helper to ensure cargo-nextest is installed
ensure-cargo-nextest:
	@if ! cargo nextest --version > /dev/null 2>&1; then \
		echo "Installing cargo-nextest..."; \
		cargo install cargo-nextest --locked; \
	fi


# Run feature matrix clippy (cargo-hack)
[group('lint')]
clippy-matrix: ensure-cargo-hack
	RUSTFLAGS="-D warnings" cargo hack --feature-powerset clippy --workspace --lib --locked --examples --tests --benches --all-targets 

# Format check / fix
[group('lint')]
fmt-check:
	cargo fmt --all --check

fmt-fix:
	cargo fmt --all

# Codespell
[group('lint')]
codespell:
	codespell

# Taplo lint and format check
[group('lint')]
taplo-lint:
	taplo lint

[group('lint')]
taplo-fmt-check:
	taplo fmt --check

# Actionlint for GitHub Actions workflows
[group('lint')]
actionlint:
	actionlint -color

# zizmor (local)
[group('lint')]
zizmor:
	uvx zizmor --config=zizmor.yml .

# zizmor writing SARIF (matches cron workflow behavior)
zizmor-sarif:
	uvx zizmor --format sarif . > results.sarif

# Cargo audit (supply-chain)
[group('lint')]
audit:
	cargo install --locked cargo-audit >/dev/null 2>&1 || true
	cargo audit

# Lint umbrella (run dependencies in parallel)
[parallel]
lint: clippy-matrix fmt-check codespell taplo-lint taplo-fmt-check actionlint zizmor

# ----------------------------
# Tests & Docs
# ----------------------------

# Run feature matrix tests (nextest)
[group('tests')]
test-matrix: ensure-cargo-hack ensure-cargo-nextest
	RUST_LOG="INFO" RUST_BACKTRACE=1 cargo hack --feature-powerset nextest run --workspace 


# Unit tests with coverage (similar to unit.yml)
[group('tests')]
unit:
	cargo install --locked cargo-llvm-cov >/dev/null 2>&1 || true
	cargo llvm-cov --workspace nextest --profile ci --no-capture --lcov --output-path lcov.info

# Docs (similar to docs.yml)
[group('docs')]
docs:
	RUSTDOCFLAGS="--show-type-layout --enable-index-page -Zunstable-options -A rustdoc::private-doc-tests -D warnings" \
	cargo doc --workspace --no-deps

# --------------------------------
# CI mega-group to run everything
# --------------------------------

# Run all CI-like tasks in parallel groups
[parallel]
ci-all: lint test-matrix unit docs
