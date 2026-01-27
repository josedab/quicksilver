# Quicksilver development tasks
# Install just: cargo install just

# Default: run all checks
default: check

# Run all checks (fmt, clippy, test)
check: fmt-check clippy test

# Format code
fmt:
    cargo fmt --all

# Check formatting without changing files
fmt-check:
    cargo fmt --all -- --check

# Run clippy lints
clippy:
    cargo clippy --all-targets -- -D warnings

# Run all tests
test:
    cargo test

# Run tests with output visible
test-verbose:
    cargo test -- --nocapture

# Run only unit tests
test-unit:
    cargo test --lib

# Run only integration tests
test-integration:
    cargo test --test '*'

# Build debug
build:
    cargo build

# Build release
build-release:
    cargo build --release

# Run a JavaScript file
run file:
    cargo run -- {{file}}

# Evaluate a JavaScript expression
eval expr:
    cargo run -- -e "{{expr}}"

# Start the REPL
repl:
    cargo run -- repl

# Run benchmarks
bench:
    cargo bench

# Generate and open documentation
docs:
    cargo doc --no-deps --open

# Fast syntax check (no codegen)
check-syntax:
    cargo check

# Clean build artifacts
clean:
    cargo clean
