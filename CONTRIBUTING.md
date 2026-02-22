# Contributing to Quicksilver

Thank you for your interest in contributing to Quicksilver! This document provides guidelines and information for contributors.

## Code of Conduct

Please be respectful and constructive in all interactions. We welcome contributors of all experience levels.

## Getting Started

1. Fork the repository
2. Clone your fork:
   ```bash
   git clone https://github.com/YOUR_USERNAME/quicksilver.git
   cd quicksilver
   ```
3. Create a branch for your changes:
   ```bash
   git checkout -b feature/your-feature-name
   ```

## Development Setup

### Prerequisites

- Rust 1.70 or later (2021 edition)
- Cargo
- (Optional) [just](https://github.com/casey/just) task runner: `cargo install just`

### Automated Setup

Run the setup script to install dev tools and pre-commit hooks:
```bash
./scripts/setup_dev_env.sh
```

This will:
- Verify your Rust version
- Install `just`, `cargo-deny`, and `cargo-watch` (optional)
- Set up a pre-commit hook that runs `cargo fmt`, `cargo clippy`, and `cargo test`
- Verify the build works

### Quick Reference

```bash
# Core workflow
cargo check                   # Fast syntax check (~15-20s cold, ~3s warm)
cargo build                   # Debug build (~25s incremental)
cargo test                    # Run all 800+ tests (~10s)
cargo clippy                  # Lint check (must be zero warnings)
cargo fmt                     # Format code

# Running JavaScript
cargo run -- script.js        # Run a file
cargo run -- -e "1 + 2"       # Evaluate expression
cargo run -- repl             # Interactive REPL

# Cargo aliases (defined in .cargo/config.toml)
cargo check-all               # clippy with strict warnings
cargo test-quick              # Unit tests only (fast)
cargo dev                     # Start REPL

# Targeted testing
cargo test arrow_functions    # Run tests matching a name
cargo test --lib              # Unit tests only
cargo test --test core_language_tests  # Specific test file
cargo test -- --nocapture     # Show stdout from tests

# With just (optional, install via: cargo install just)
just check                    # fmt + clippy + test
just run examples/hello_world.js
just bench                    # Run benchmarks
```

### Building

```bash
cargo build
```

### Running Tests

```bash
cargo test
```

### Running Clippy

```bash
cargo clippy
```

### Formatting

```bash
cargo fmt
```

## Project Structure

```
src/
├── main.rs           # CLI entry point (REPL, run, debug, test commands)
├── lib.rs            # Library exports and module declarations
├── error.rs          # Error types (Error, Result, StackTrace)
├── prelude.rs        # Convenience re-exports for library users
├── lexer/            # Tokenization (source → tokens)
├── parser/           # Recursive descent parser (tokens → AST)
├── ast/              # AST node definitions (Expression, Statement)
├── bytecode/         # Bytecode compiler and opcodes (AST → Chunk)
├── runtime/          # VM, value types, builtins (Chunk → Result)
├── gc/               # Mark-and-sweep garbage collection
├── event_loop/       # Promise/A+ event loop, microtask queue
├── modules/          # ES Module loader (import/export, import maps)
├── npm/              # CommonJS/npm compatibility (require, path, util)
├── typescript/       # TypeScript type stripping
├── native/           # Native APIs (HTTP server/client, WebSocket, timers)
├── edge/             # Edge computing (Cloudflare Workers-compatible API)
├── wasm/             # WebAssembly module parsing and execution
├── workers/          # Worker threads, SharedArrayBuffer, Atomics
├── security/         # Capability-based permission system
├── sandbox/          # Sandbox configuration and enforcement
├── debugger/         # Time-travel debugger (DAP protocol, TUI, source maps)
├── diagnostics/      # Language diagnostics, error suggestions
├── profiler/         # CPU/memory profiling
├── repl/             # Interactive REPL with completion and highlighting
├── test_runner/      # Built-in JavaScript test framework
├── test262/          # Test262 conformance harness
├── c_api/            # C FFI API for embedding
├── ffi/              # Foreign function interface
├── bindings/         # Multi-language SDK bindings (C, Python, Go)
├── ai/               # AI-native runtime (JSDoc → tool schemas)
├── agent/            # AI agent execution sandbox
├── jit/              # JIT compilation (experimental)
├── effects/          # Algebraic effects system (experimental)
├── distributed/      # Distributed runtime primitives (experimental)
├── hmr/              # Hot module reloading
├── plugins/          # Plugin system
├── reactive/         # Reactive state management
├── concurrency/      # Channels, structured concurrency
├── snapshot/         # Snapshot serialization (cold starts)
├── observability/    # OpenTelemetry-compatible tracing/metrics
├── durable/          # Durable objects (experimental)
└── playground/       # Web playground bridge
```

### Pipeline Architecture

```
Source Code → Lexer → Parser → AST → Compiler → Bytecode → VM → Result
```

See `CLAUDE.md` for detailed type documentation and implementation guide.

## Making Changes

### Code Style

- Follow Rust idioms and conventions
- Use `cargo fmt` before committing
- Ensure `cargo clippy` passes without warnings
- Add doc comments (`///`) for public APIs
- Keep functions focused and well-named

### Testing

- Add tests for new features
- Ensure existing tests pass
- **Integration tests** go in `tests/` — organized by feature area:
  - `core_language_tests.rs` — arrow functions, operators, control flow
  - `data_structures_tests.rs` — arrays, objects, destructuring, symbols
  - `oop_tests.rs` — classes, inheritance
  - `error_control_tests.rs` — try/catch, error handling, recursion
  - `collections_types_tests.rs` — WeakMap/Set, Proxy
  - `async_modules_tests.rs` — promises, async/await, ES modules, generators
  - `modern_features_tests.rs` — structuredClone, advanced features
- **Unit tests** go in the same file as the code (in a `#[cfg(test)]` module) — use for pure functions, value conversions, individual opcode behavior
- **Doc tests** go in `///` doc comments — use for public API examples that should always compile

### Commit Messages

Use clear, descriptive commit messages:
- `feat: add support for Array.prototype.flatMap`
- `fix: correct precedence for nullish coalescing operator`
- `docs: update README with new features`
- `test: add integration tests for destructuring`
- `refactor: simplify bytecode emission for binary operators`

## Pull Request Process

1. Ensure all tests pass
2. Update documentation if needed
3. Add a clear PR description explaining your changes
4. Link any related issues

### PR Checklist

- [ ] Tests added/updated
- [ ] Documentation updated (if applicable)
- [ ] `cargo fmt` run
- [ ] `cargo clippy` passes
- [ ] `cargo test` passes
- [ ] Commit messages are clear

## Areas for Contribution

### Good First Issues

- Improving error messages
- Adding doc comments
- Writing tests for edge cases
- Small built-in function implementations

### Larger Projects

- Performance optimizations (VM dispatch, object property lookup)
- Improving garbage collection (generational GC)
- Expanding Test262 conformance coverage
- JIT compilation improvements
- WebAssembly interop enhancements

## Reporting Bugs

Please include:
- Quicksilver version
- Rust version
- Minimal reproduction case
- Expected vs actual behavior
- Error messages (if any)

## Feature Requests

Open an issue describing:
- The feature and its use case
- How it fits with Quicksilver's goals
- Any implementation ideas

## Questions?

Open a discussion or issue if you have questions about contributing.

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
