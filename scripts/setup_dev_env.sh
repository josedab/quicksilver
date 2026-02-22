#!/usr/bin/env bash
set -euo pipefail

echo "🔧 Setting up Quicksilver development environment..."
echo ""

# Check Rust installation
if ! command -v rustc &> /dev/null; then
    echo "❌ Rust not found. Install from https://rustup.rs"
    exit 1
fi

RUST_VERSION=$(rustc --version | awk '{print $2}')
echo "✅ Rust $RUST_VERSION found"

# Verify minimum version
if [[ "$(printf '%s\n' "1.70.0" "$RUST_VERSION" | sort -V | head -n1)" != "1.70.0" ]]; then
    echo "❌ Rust 1.70+ required, found $RUST_VERSION"
    exit 1
fi

# Install recommended dev tools
echo ""
echo "📦 Installing development tools..."

install_tool() {
    local tool=$1
    local crate=${2:-$1}
    if command -v "$tool" &> /dev/null; then
        echo "  ✅ $tool already installed"
    else
        echo "  📥 Installing $crate..."
        cargo install "$crate" --quiet 2>/dev/null || echo "  ⚠️  Failed to install $crate (optional)"
    fi
}

install_tool "just" "just"
install_tool "cargo-deny" "cargo-deny"
install_tool "cargo-watch" "cargo-watch"

# Set up pre-commit hook
echo ""
echo "🔗 Setting up pre-commit hook..."
HOOK_PATH=".git/hooks/pre-commit"
if [ -d ".git" ]; then
    cat > "$HOOK_PATH" << 'HOOK'
#!/usr/bin/env bash
set -e

echo "Running pre-commit checks..."

# Check formatting
if ! cargo fmt --all -- --check 2>/dev/null; then
    echo "❌ Formatting check failed. Run 'cargo fmt' to fix."
    exit 1
fi

# Run clippy
if ! cargo clippy --all-targets --quiet 2>/dev/null; then
    echo "❌ Clippy check failed. Fix warnings before committing."
    exit 1
fi

# Run tests
if ! cargo test --quiet 2>/dev/null; then
    echo "❌ Tests failed. Run 'cargo test' for details."
    exit 1
fi

echo "✅ Pre-commit checks passed"
HOOK
    chmod +x "$HOOK_PATH"
    echo "  ✅ Pre-commit hook installed"
else
    echo "  ⚠️  Not in a git repository, skipping hook"
fi

# Verify build
echo ""
echo "🔨 Verifying build..."
if cargo check --quiet 2>/dev/null; then
    echo "  ✅ Build check passed"
else
    echo "  ❌ Build check failed — run 'cargo build' for details"
    exit 1
fi

echo ""
echo "🎉 Development environment ready!"
echo ""
echo "Quick start:"
echo "  cargo test          # Run tests"
echo "  cargo run -- repl   # Start REPL"
echo "  cargo check-all     # Clippy with strict warnings"
echo "  cargo test-quick    # Unit tests only (fast)"
echo ""
echo "Optional tools (if installed):"
echo "  just check          # Run fmt + clippy + test"
echo "  cargo watch -x test # Watch mode"
