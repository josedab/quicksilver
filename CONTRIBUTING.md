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
├── main.rs           # CLI entry point
├── lib.rs            # Library exports
├── error.rs          # Error types
├── lexer/            # Tokenization
├── parser/           # AST generation
├── ast/              # AST node definitions
├── bytecode/         # Bytecode compiler
├── runtime/          # VM and value types
├── gc/               # Garbage collection
└── ...               # Feature modules
```

See `CLAUDE.md` for detailed architecture documentation.

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
- Integration tests go in `tests/`
- Unit tests go in the same file as the code (in a `#[cfg(test)]` module)

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

- Implementing generators (`yield`)
- Adding ES Module support
- Improving garbage collection
- Performance optimizations

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
