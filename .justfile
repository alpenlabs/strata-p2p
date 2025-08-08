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

# Show a friendly help menu
help:
	@echo ""
	@echo "Available recipes:"
	@echo "  lint               - Run all lints (clippy matrix, fmt check, codespell, taplo, actionlint, zizmor) in parallel"
	@echo "  test-matrix        - Run feature matrix tests (nextest) in parallel"
	@echo "  clippy-matrix      - Run feature matrix clippy in parallel"
	@echo "  unit               - Run unit tests with coverage (llvm-cov + nextest)"
	@echo "  docs               - Build docs"
	@echo "  fmt-check          - Check formatting (rustfmt)"
	@echo "  fmt-fix            - Auto-format (rustfmt)"
	@echo "  codespell          - Lint spellings"
	@echo "  taplo-lint         - Lint TOML files"
	@echo "  taplo-fmt-check    - Check TOML formatting"
	@echo "  audit              - Run cargo-audit (supply-chain)"
	@echo "  actionlint         - Lint GitHub Actions workflows"
	@echo "  zizmor             - Run zizmor (stdout)"
	@echo "  zizmor-sarif       - Run zizmor (write results.sarif)"
	@echo "  ci-all             - Run everything in parallel groups (lint + test-matrix + unit + docs)"
	@echo ""
	@echo "Helpers:"
	@echo "  clippy-features FEATURES='' - Run clippy for a specific feature combo"
	@echo "  test-features   FEATURES='' - Run nextest for a specific feature combo"
	@echo ""

# ----------------------------
# Core linting & formatting
# ----------------------------

# Clippy on specific features (no-default-features)
# Use RUSTFLAGS=-D warnings to fail on warnings, matching CI.
[group('lint')]
clippy-features FEATURES:
	if [ -z "{{FEATURES}}" ]; then \
		RUSTFLAGS="-D warnings" cargo clippy --workspace --lib --locked --examples --tests --benches --all-targets --no-default-features ; \
	else \
		RUSTFLAGS="-D warnings" cargo clippy --workspace --lib --locked --examples --tests --benches --all-targets --no-default-features --features "{{FEATURES}}" ; \
	fi

# Non-BYOS clippy matrix entries
[group('lint')]
clippy-nonbyos-base:
	just clippy-features ""

[group('lint')]
clippy-nonbyos-gossipsub:
	just clippy-features "gossipsub"

[group('lint')]
clippy-nonbyos-request-response:
	just clippy-features "request-response"

[group('lint')]
clippy-nonbyos-quic:
	just clippy-features "quic"

[group('lint')]
clippy-nonbyos-kad:
	just clippy-features "kad"

[group('lint')]
clippy-nonbyos-gossipsub-request-response:
	just clippy-features "gossipsub request-response"

[group('lint')]
clippy-nonbyos-gossipsub-quic:
	just clippy-features "gossipsub quic"

[group('lint')]
clippy-nonbyos-gossipsub-kad:
	just clippy-features "gossipsub kad"

[group('lint')]
clippy-nonbyos-request-response-quic:
	just clippy-features "request-response quic"

[group('lint')]
clippy-nonbyos-request-response-kad:
	just clippy-features "request-response kad"

[group('lint')]
clippy-nonbyos-quic-kad:
	just clippy-features "quic kad"

[group('lint')]
clippy-nonbyos-gossipsub-request-response-quic:
	just clippy-features "gossipsub request-response quic"

[group('lint')]
clippy-nonbyos-gossipsub-request-response-kad:
	just clippy-features "gossipsub request-response kad"

[group('lint')]
clippy-nonbyos-gossipsub-quic-kad:
	just clippy-features "gossipsub quic kad"

[group('lint')]
clippy-nonbyos-request-response-quic-kad:
	just clippy-features "request-response quic kad"

[group('lint')]
clippy-nonbyos-all:
	just clippy-features "gossipsub request-response quic kad"

# BYOS clippy matrix entries (no kad)
[group('lint')]
clippy-byos-base:
	just clippy-features "byos"

[group('lint')]
clippy-byos-gossipsub:
	just clippy-features "byos gossipsub"

[group('lint')]
clippy-byos-request-response:
	just clippy-features "byos request-response"

[group('lint')]
clippy-byos-quic:
	just clippy-features "byos quic"

[group('lint')]
clippy-byos-gossipsub-request-response:
	just clippy-features "byos gossipsub request-response"

[group('lint')]
clippy-byos-gossipsub-quic:
	just clippy-features "byos gossipsub quic"

[group('lint')]
clippy-byos-request-response-quic:
	just clippy-features "byos request-response quic"

[group('lint')]
clippy-byos-all:
	just clippy-features "byos gossipsub request-response quic"

# Run all clippy jobs in parallel
[group('lint')]
[parallel]
clippy-matrix: \
	clippy-nonbyos-base \
	clippy-nonbyos-gossipsub \
	clippy-nonbyos-request-response \
	clippy-nonbyos-quic \
	clippy-nonbyos-kad \
	clippy-nonbyos-gossipsub-request-response \
	clippy-nonbyos-gossipsub-quic \
	clippy-nonbyos-gossipsub-kad \
	clippy-nonbyos-request-response-quic \
	clippy-nonbyos-request-response-kad \
	clippy-nonbyos-quic-kad \
	clippy-nonbyos-gossipsub-request-response-quic \
	clippy-nonbyos-gossipsub-request-response-kad \
	clippy-nonbyos-gossipsub-quic-kad \
	clippy-nonbyos-request-response-quic-kad \
	clippy-nonbyos-all \
	clippy-byos-base \
	clippy-byos-gossipsub \
	clippy-byos-request-response \
	clippy-byos-quic \
	clippy-byos-gossipsub-request-response \
	clippy-byos-gossipsub-quic \
	clippy-byos-request-response-quic \
	clippy-byos-all

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

# Nextest on specific features (no-default-features)
[group('tests')]
test-features FEATURES:
	if [ -z "{{FEATURES}}" ]; then \
		cargo nextest run --workspace --no-capture --no-default-features ; \
	else \
		cargo nextest run --workspace --no-capture --no-default-features --features "{{FEATURES}}" ; \
	fi

# Non-BYOS test matrix entries
[group('tests')]
test-nonbyos-base:
	just test-features ""

[group('tests')]
test-nonbyos-gossipsub:
	just test-features "gossipsub"

[group('tests')]
test-nonbyos-request-response:
	just test-features "request-response"

[group('tests')]
test-nonbyos-quic:
	just test-features "quic"

[group('tests')]
test-nonbyos-kad:
	just test-features "kad"

[group('tests')]
test-nonbyos-gossipsub-request-response:
	just test-features "gossipsub request-response"

[group('tests')]
test-nonbyos-gossipsub-quic:
	just test-features "gossipsub quic"

[group('tests')]
test-nonbyos-gossipsub-kad:
	just test-features "gossipsub kad"

[group('tests')]
test-nonbyos-request-response-quic:
	just test-features "request-response quic"

[group('tests')]
test-nonbyos-request-response-kad:
	just test-features "request-response kad"

[group('tests')]
test-nonbyos-quic-kad:
	just test-features "quic kad"

[group('tests')]
test-nonbyos-gossipsub-request-response-quic:
	just test-features "gossipsub request-response quic"

[group('tests')]
test-nonbyos-gossipsub-request-response-kad:
	just test-features "gossipsub request-response kad"

[group('tests')]
test-nonbyos-gossipsub-quic-kad:
	just test-features "gossipsub quic kad"

[group('tests')]
test-nonbyos-request-response-quic-kad:
	just test-features "request-response quic kad"

[group('tests')]
test-nonbyos-all:
	just test-features "gossipsub request-response quic kad"

# BYOS test matrix entries (no kad)
[group('tests')]
test-byos-base:
	just test-features "byos"

[group('tests')]
test-byos-gossipsub:
	just test-features "byos gossipsub"

[group('tests')]
test-byos-request-response:
	just test-features "byos request-response"

[group('tests')]
test-byos-quic:
	just test-features "byos quic"

[group('tests')]
test-byos-gossipsub-request-response:
	just test-features "byos gossipsub request-response"

[group('tests')]
test-byos-gossipsub-quic:
	just test-features "byos gossipsub quic"

[group('tests')]
test-byos-request-response-quic:
	just test-features "byos request-response quic"

[group('tests')]
test-byos-all:
	just test-features "byos gossipsub request-response quic"

# Run all test jobs in parallel
[group('tests')]
[parallel]
test-matrix: \
	test-nonbyos-base \
	test-nonbyos-gossipsub \
	test-nonbyos-request-response \
	test-nonbyos-quic \
	test-nonbyos-kad \
	test-nonbyos-gossipsub-request-response \
	test-nonbyos-gossipsub-quic \
	test-nonbyos-gossipsub-kad \
	test-nonbyos-request-response-quic \
	test-nonbyos-request-response-kad \
	test-nonbyos-quic-kad \
	test-nonbyos-gossipsub-request-response-quic \
	test-nonbyos-gossipsub-request-response-kad \
	test-nonbyos-gossipsub-quic-kad \
	test-nonbyos-request-response-quic-kad \
	test-nonbyos-all \
	test-byos-base \
	test-byos-gossipsub \
	test-byos-request-response \
	test-byos-quic \
	test-byos-gossipsub-request-response \
	test-byos-gossipsub-quic \
	test-byos-request-response-quic \
	test-byos-all

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
