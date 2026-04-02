# Faultline development commands
# Install just: cargo install just

# Default recipe: run all CI checks locally
default: ci

# Format all code
fmt:
    cargo fmt --all

# Check formatting (CI mode)
fmt-check:
    cargo fmt --all -- --check

# Run clippy lints
lint:
    cargo clippy --all-targets -- -D warnings

# Run all tests
test:
    cargo test

# Run tests for a specific package
test-pkg pkg:
    cargo test -p {{pkg}}

# Build in release mode
build:
    cargo build --release

# Run cargo-deny license/advisory checks
deny:
    cargo deny check

# Run full CI pipeline locally (matches GitHub Actions order)
ci: fmt-check lint test build deny

# Run CI in Docker (matches GitHub Actions exactly)
ci-docker:
    docker compose --profile ci run --rm rust-ci cargo fmt --all -- --check
    docker compose --profile ci run --rm rust-ci cargo clippy --all-targets -- -D warnings
    docker compose --profile ci run --rm rust-ci cargo test
    docker compose --profile ci run --rm rust-ci cargo build --release
    docker compose --profile ci run --rm rust-ci cargo deny check

# Run a single simulation
run-single scenario="scenarios/tutorial_symmetric.toml":
    cargo run -p faultline-cli -- {{scenario}} --single-run

# Run Monte Carlo batch
run-mc scenario="scenarios/tutorial_symmetric.toml" runs="1000":
    cargo run -p faultline-cli -- {{scenario}} -n {{runs}}

# Validate a scenario file
validate scenario:
    cargo run -p faultline-cli -- {{scenario}} --validate

# Clean build artifacts
clean:
    cargo clean
