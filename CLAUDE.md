# CLAUDE.md - Quicksilver Project Guide

This file provides guidance for AI assistants working with the Quicksilver codebase.

## Project Overview

Quicksilver is a memory-safe JavaScript runtime written in Rust. It implements a bytecode-compiled, stack-based JavaScript interpreter designed for embedded use cases, edge computing, and security-sensitive applications.

## Architecture

The project follows a standard interpreter pipeline:

```
Source Code → Lexer → Parser → AST → Compiler → Bytecode → VM → Result
```

### Module Structure

```
src/
├── main.rs           # CLI entry point
├── lib.rs            # Library exports
├── error.rs          # Error types (Error, Result)
├── lexer/
│   ├── mod.rs        # Lexer implementation (tokenization)
│   └── token.rs      # Token types and definitions
├── parser/
│   └── mod.rs        # Recursive descent parser
├── ast/
│   ├── mod.rs        # AST node definitions
│   ├── expr.rs       # Expression nodes
│   ├── stmt.rs       # Statement nodes
│   └── pattern.rs    # Destructuring patterns
├── bytecode/
│   ├── mod.rs        # Chunk and bytecode structures
│   ├── opcode.rs     # Opcode definitions
│   └── compiler.rs   # AST to bytecode compiler
├── runtime/
│   ├── mod.rs        # Runtime exports
│   ├── vm.rs         # Stack-based virtual machine
│   ├── value.rs      # JavaScript value types (Value, Object, ObjectKind)
│   └── builtins.rs   # Built-in objects (console, Math, JSON, Date, Map, Set)
├── gc/
│   └── mod.rs        # Mark-and-sweep garbage collection
├── snapshot/
│   └── mod.rs        # Snapshot serialization for instant cold starts
├── security/
│   └── mod.rs        # Capability-based security and sandboxing
├── concurrency/
│   └── mod.rs        # Structured concurrency (channels, spawn, select)
├── observability/
│   └── mod.rs        # Built-in tracing, metrics, and profiling
├── ai/
│   └── mod.rs        # AI-native runtime (JSDoc → tool schemas)
├── wasm/
│   └── mod.rs        # WebAssembly module parsing and integration
├── effects/
│   └── mod.rs        # Algebraic effects system
├── distributed/
│   └── mod.rs        # Distributed runtime primitives (cluster, actors)
├── debugger/
│   └── mod.rs        # Time-travel debugger with record/replay
└── hmr/
    └── mod.rs        # Hot module reloading
```

## Key Types

### Value (`src/runtime/value.rs`)
The core JavaScript value representation:
- `Value::Undefined`, `Value::Null`, `Value::Boolean(bool)`, `Value::Number(f64)`, `Value::String(String)`
- `Value::Object(Rc<RefCell<Object>>)` - reference-counted objects
- `Value::Symbol(u32)` - ES6 symbols

### ObjectKind (`src/runtime/value.rs`)
Discriminated union for object types:
- `Ordinary` - plain objects
- `Array(Vec<Value>)` - arrays
- `Function(Function)` - user-defined functions
- `NativeFunction { name, func }` - built-in functions
- `Class { name, constructor, prototype }` - ES6 classes
- `Date(f64)` - Date objects (timestamp in ms)
- `Map(Vec<(Value, Value)>)` - Map collections
- `Set(Vec<Value>)` - Set collections
- `Iterator { values, index }` - iterators for for...of
- `Error`, `Promise`, `SpreadMarker`

### Opcode (`src/bytecode/opcode.rs`)
Bytecode instructions executed by the VM. Key opcodes:
- Stack: `Push`, `Pop`, `Dup`
- Variables: `GetLocal`, `SetLocal`, `GetGlobal`, `SetGlobal`
- Objects: `GetProperty`, `SetProperty`, `CreateObject`, `CreateArray`
- Control: `Jump`, `JumpIfFalse`, `Call`, `Return`
- Operations: `Add`, `Sub`, `Mul`, `Div`, `Equal`, `Less`, etc.

## Common Tasks

### Adding a New Built-in Function

1. Open `src/runtime/builtins.rs`
2. Register the native function:
```rust
vm.register_native("functionName", |args| {
    // Implementation
    Ok(Value::...)
});
```
3. Attach to a global object:
```rust
object.set_property("methodName", vm.get_global("functionName").unwrap_or(Value::Undefined));
```

### Adding a New Object Type

1. Add variant to `ObjectKind` in `src/runtime/value.rs`
2. Update the `Display` impl for `ObjectKind` (in the `fmt::Debug` impl for `Value`)
3. Handle in VM operations as needed (`src/runtime/vm.rs`)

### Adding a New Opcode

1. Add to `Opcode` enum in `src/bytecode/opcode.rs`
2. Handle in compiler (`src/bytecode/compiler.rs`)
3. Handle in VM execution loop (`src/runtime/vm.rs`, `run()` method)

### Modifying the Parser

The parser is in `src/parser/mod.rs`. Key methods:
- `parse_statement()` - entry for statements
- `parse_expression()` - entry for expressions
- `parse_primary_expression()` - literals, identifiers, etc.
- `parse_function_params()` - function parameter parsing

## Build & Test Commands

```bash
# Build
cargo build
cargo build --release

# Run a JavaScript file
cargo run -- script.js

# Run with expression
cargo run -- -e "console.log('hello')"

# Run tests
cargo test

# Run clippy
cargo clippy

# Format code
cargo fmt
```

## Testing JavaScript Features

Create test files in the project root (e.g., `test_feature.js`) and run:
```bash
cargo run -- test_feature.js
```

## Code Conventions

- Use `Result<T>` (from `crate::error`) for fallible operations
- Use `Rc<RefCell<...>>` for shared mutable state (GC handles memory)
- Native functions take `&[Value]` and return `Result<Value>`
- Match exhaustively on `ObjectKind` when handling objects
- Prefer `Value::new_object()`, `Value::new_array()` constructors

## Current JavaScript Feature Support

### Implemented
- Variables: `let`, `const`, `var`
- Functions: declarations, expressions, arrow functions, default params, rest params
- Classes: constructors, properties, `instanceof`
- Control flow: `if/else`, `while`, `for`, `for...in`, `for...of`, `switch`
- Operators: arithmetic, comparison, logical, bitwise, spread (`...`), optional chaining (`?.`), nullish coalescing (`??`)
- Destructuring: arrays and objects
- Template literals: `` `string ${expr}` ``
- Error handling: `try/catch/finally`, `throw`
- Built-ins: `console`, `Math`, `JSON`, `Date`, `Map`, `Set`, `Array`, `Object`

### Advanced Features (Next-Gen Modules)

#### Fully Functional
- **Garbage Collection**: Mark-and-sweep GC with configurable thresholds
- **Snapshot Isolation**: Instant cold starts via bytecode serialization
- **Capability Security**: Fine-grained permission control with Sandbox
- **Structured Concurrency**: Go-style channels with sender/receiver
- **Observability**: OpenTelemetry-compatible tracing, metrics (Counter, Gauge, Histogram)
- **AI-Native Runtime**: JSDoc → LLM tool schema generation (OpenAI/Anthropic formats)
- **WebAssembly**: Complete WASM module parsing (Type, Import, Function, Table, Memory, Export, Code sections)

#### Framework Implemented (Not Yet VM-Integrated)
- **Effect System**: Algebraic effects infrastructure (needs `perform` statement in VM)
- **Distributed Runtime**: Cluster primitives and actor model (needs network layer)
- **Time-Travel Debugger**: Record/replay framework (needs VM integration)
- **Hot Module Reloading**: Module tracking and updates (needs VM integration)

### Not Yet Implemented
- Generators (`yield`) - tokens exist, no semantics
- ES Modules (`import`/`export`) - tokens exist, no module system
- Full `async`/`await` - parser only, no event loop
- `Proxy` traps - constructor only
- True weak references for `WeakMap`/`WeakSet`

## Debugging Tips

- The VM has a `MAX_STACK_SIZE` (10000) and `MAX_CALL_DEPTH` (1000)
- Check `self.stack` contents when debugging VM issues
- Use `cargo run -- -e "..."` for quick expression testing
- Compilation errors often indicate missing opcode handling
- Runtime "undefined" results often mean missing property/variable lookups
