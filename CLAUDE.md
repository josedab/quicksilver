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
├── lexer/            # Tokenization
├── parser/           # Recursive descent parser
├── ast/              # AST node definitions
├── bytecode/         # Bytecode compiler and opcodes
├── runtime/          # VM, value types, builtins
├── gc/               # Mark-and-sweep garbage collection
├── snapshot/         # Snapshot serialization
├── security/         # Capability-based security
├── sandbox/          # Sandbox configuration
├── concurrency/      # Channels, spawn, select
├── observability/    # Tracing, metrics, profiling
├── ai/               # AI-native runtime (JSDoc → tool schemas)
├── agent/            # AI agent execution sandbox
├── wasm/             # WebAssembly module parsing and execution
├── effects/          # Algebraic effects system
├── distributed/      # Distributed runtime primitives
├── debugger/         # Time-travel debugger with record/replay
├── hmr/              # Hot module reloading
├── event_loop/       # Async/await event loop
├── modules/          # ES Module loader
├── npm/              # CommonJS/npm compatibility
├── typescript/       # TypeScript type stripping
├── native/           # Native APIs (HTTP, timers, etc.)
├── edge/             # Edge computing (Cloudflare Workers compat)
├── workers/          # Worker threads & shared memory
├── playground/       # Web playground bridge
├── c_api/            # C FFI API
├── bindings/         # Multi-language SDK bindings
├── ffi/              # Foreign function interface
├── jit/              # JIT compilation (experimental)
├── plugins/          # Plugin system
├── profiler/         # Performance profiler
├── reactive/         # Reactive state management
├── diagnostics/      # Language diagnostics
├── durable/          # Durable objects
├── repl/             # Interactive REPL
├── test262/          # Test262 conformance harness
└── test_runner/      # Built-in test runner
```

### Module Implementation Status

| Module | LOC | Tests | Status | Notes |
|--------|----:|------:|--------|-------|
| `runtime` | 15,868 | 72 | ✅ Complete | Core VM, builtins, value types |
| `bytecode` | 4,704 | 14 | ✅ Complete | Compiler, opcodes, optimizer |
| `parser` | 2,903 | 8 | ✅ Complete | ES2020 recursive descent |
| `lexer` | 1,280 | 7 | ✅ Complete | Full tokenization |
| `ast` | 1,366 | 0 | ✅ Complete | Node definitions (tested via parser) |
| `native` | 3,338 | 63 | ✅ Complete | HTTP server/client, WebSocket, timers |
| `debugger` | 4,034 | 25 | ✅ Complete | Time-travel, DAP protocol, TUI |
| `modules` | 1,468 | 34 | ✅ Complete | ES Module loader |
| `typescript` | 2,590 | 18 | ⚠️ Partial | Type stripping only, no type checking |
| `wasm` | 2,122 | 23 | ✅ Complete | Full instruction set, WASI, JS bridge |
| `npm` | 1,760 | 29 | ⚠️ Partial | require(), core modules (path, util, process, os) |
| `event_loop` | 904 | 20 | ⚠️ Partial | Promise/A+, microtask queue, timers |
| `jit` | 928 | 13 | 🧪 Experimental | Basic compilation, not production-ready |
| `agent` | 886 | 16 | ✅ Complete | AI agent sandbox, tool schemas |
| `ai` | 479 | 3 | ✅ Complete | JSDoc → LLM tool schema generation |
| `c_api` | 929 | 12 | ✅ Complete | Full C FFI with runtime/value/callback APIs |
| `security` | 846 | 12 | ✅ Complete | Capability-based permissions |
| `sandbox` | 553 | 15 | ✅ Complete | Sandbox configuration and enforcement |
| `gc` | 470 | 6 | ✅ Complete | Mark-and-sweep GC |
| `snapshot` | 895 | 1 | ⚠️ Partial | Serialization framework, limited coverage |
| `edge` | 861 | 13 | ⚠️ Partial | Workers-compatible API surface |
| `hmr` | 924 | 7 | ⚠️ Partial | File watching, module graph tracking |
| `workers` | 578 | 25 | ✅ Complete | SharedArrayBuffer, Atomics, WorkerPool |
| `concurrency` | 469 | 4 | ⚠️ Partial | Channels work, limited task API |
| `distributed` | 786 | 7 | 🧪 Experimental | Cluster, actors (simulated) |
| `effects` | 478 | 4 | 🧪 Experimental | Algebraic effects framework |
| `plugins` | 900 | 17 | ⚠️ Partial | Plugin loading and lifecycle |
| `profiler` | 1,197 | 14 | ⚠️ Partial | CPU/memory profiling |
| `reactive` | 781 | 16 | ✅ Complete | Reactive state management |
| `observability` | 681 | 4 | ⚠️ Partial | OpenTelemetry-compatible metrics |
| `diagnostics` | 787 | 13 | ⚠️ Partial | Language server diagnostics |
| `durable` | 546 | 7 | 🧪 Experimental | Durable objects framework |
| `ffi` | 558 | 4 | ⚠️ Partial | Foreign function interface |
| `bindings` | 581 | 13 | ✅ Complete | C/Python/Go bindings |
| `playground` | 175 | 9 | ✅ Complete | Web playground evaluation bridge |
| `repl` | 911 | 2 | ⚠️ Partial | Basic REPL, limited completion |
| `test262` | 1,005 | 11 | ⚠️ Partial | Conformance micro-tests |
| `test_runner` | 970 | 17 | ✅ Complete | Built-in test framework |
| `wasi_target` | 738 | 13 | ⚠️ Partial | WASI target compilation |

**Legend**: ✅ Complete — ⚠️ Partial — 🧪 Experimental

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

#### Fully VM-Integrated
- **Effect System**: Algebraic effects with `perform` statement, effect registry, and handler chaining
- **Distributed Runtime**: Cluster computing, task submission, actor messaging, all accessible from VM
- **Time-Travel Debugger**: Record/replay with DAP protocol support, production-ready TUI, step forward/backward
- **Hot Module Reloading**: File watching, module graph tracking, hot updates with VM integration
- **Async/Await Event Loop**: Complete Promise/A+ compliant event loop with microtask queue and timers
- **Native TypeScript**: Type stripping transpiler for direct .ts file execution

### Not Yet Implemented
- Generators (`yield`) - tokens exist, no semantics
- ES Modules (`import`/`export`) - tokens exist, no module system
- `Proxy` traps - constructor only
- True weak references for `WeakMap`/`WeakSet`

## Debugging Tips

- The VM has a `MAX_STACK_SIZE` (10000) and `MAX_CALL_DEPTH` (1000)
- Check `self.stack` contents when debugging VM issues
- Use `cargo run -- -e "..."` for quick expression testing
- Compilation errors often indicate missing opcode handling
- Runtime "undefined" results often mean missing property/variable lookups
