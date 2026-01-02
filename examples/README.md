# Quicksilver Examples

This directory contains examples demonstrating Quicksilver's features.

## Running Examples

```bash
# Run a JavaScript example
cargo run -- examples/hello_world.js

# Run with interactive REPL
cargo run

# Run benchmarks
cargo bench
```

## Examples Overview

### Getting Started
| File | Description |
|------|-------------|
| `hello_world.js` | Basic "Hello World" and language tour |
| `fibonacci.js` | Recursive and iterative algorithms |
| `classes.js` | ES6 classes, inheritance, and OOP patterns |

### Core Features
| File | Description |
|------|-------------|
| `builtins_showcase.js` | All built-in objects: Math, Date, Map, Set, etc. |
| `async_patterns.js` | Async/await, promises, error handling |
| `security_sandbox.js` | Capability-based security model |

## Feature Demonstrations

### 1. Interactive REPL

```bash
cargo run
```

Features:
- Command history (persisted across sessions)
- Multi-line input support
- Special commands: `.help`, `.clear`, `.exit`, `.vars`
- Pretty-printed output with colors

### 2. Snapshot Isolation (Instant Cold Start)

```rust
use quicksilver::snapshot::Snapshot;

// Save runtime state
let snapshot = Snapshot::capture(&runtime);
snapshot.save("app.snapshot")?;

// Load in <10ms - instant cold start!
let runtime = Snapshot::load("app.snapshot")?;
```

### 3. Capability-Based Security

```rust
use quicksilver::security::{Sandbox, PathPattern};

let sandbox = Sandbox::new()
    .allow_read(&["./data"])       // Only ./data/*
    .allow_net(&["api.example.com"]) // Only this host
    .deny_write_all();             // No write access

runtime.execute_with_sandbox(code, sandbox)?;
```

### 4. Rust FFI Integration

```rust
use quicksilver::ffi::{FfiRegistry, ffi_fn};

let mut registry = FfiRegistry::new();

// Register a Rust function
registry.register_fn("myFunction", |args| {
    let x = f64::from_js_value(&args[0])?;
    Ok((x * 2.0).into_js_value())
});

// Call from JavaScript
// const result = __ffi.myFunction(21); // 42
```

### 5. Native APIs (Deno-style)

```javascript
// File System
const content = await Deno.readTextFile("./data.txt");
await Deno.writeTextFile("./output.txt", "Hello!");
for await (const entry of Deno.readDir("./")) {
  console.log(entry.name, entry.isFile);
}

// HTTP
const response = await fetch("https://api.example.com/data");
const data = await response.json();

// Process
const output = await Deno.run({ cmd: ["ls", "-la"] });
const home = Deno.env.get("HOME");
```

### 6. Structured Concurrency

```rust
use quicksilver::concurrency::{spawn, Channel};

// Go-style channels
let (tx, rx) = Channel::new(10);

spawn(async move {
    tx.send(42).await;
});

let value = rx.recv().await;
```

### 7. Time-Travel Debugging

```bash
cargo run -- debug examples/buggy.js
```

Commands:
- `step` / `s` - Step forward
- `back` / `b` - Step backward (time-travel!)
- `breakpoint <line>` - Set breakpoint
- `inspect <var>` - Inspect variable
- `history` - Show execution history

### 8. Hot Module Reloading

```javascript
if (module.hot) {
  module.hot.accept();
  module.hot.dispose((data) => {
    data.state = currentState; // Preserve state
  });
}
```

### 9. Built-in Observability

```rust
use quicksilver::observability::{Tracer, Counter};

// Tracing
let span = Tracer::span("http_request", &[
    ("method", "GET"),
    ("path", "/api/users")
]);

// Metrics
let counter = Counter::new("requests_total");
counter.inc();
```

### 10. AI-Native Runtime

```javascript
/**
 * Get weather for a location
 * @tool
 * @param {string} location - The city name
 * @returns {object} Weather data
 */
function getWeather(location) {
    return { temp: 72, conditions: "sunny" };
}

// Automatically generates LLM tool schema!
```

## Performance Benchmarks

```bash
cargo bench
```

Benchmarks include:
- Cold start time (<10ms target)
- Expression evaluation
- Function calls
- Object/array operations
- String manipulation
- Compilation speed

## Project Structure

```
quicksilver/
├── src/
│   ├── lib.rs          # Library exports
│   ├── runtime/        # VM, values, builtins
│   ├── bytecode/       # Compiler, opcodes
│   ├── parser/         # Parser
│   ├── lexer/          # Tokenizer
│   ├── security/       # Sandbox, capabilities
│   ├── native/         # FS, HTTP, Process APIs
│   ├── ffi/            # Rust FFI bridge
│   ├── concurrency/    # Channels, spawn
│   ├── snapshot/       # Serialization
│   ├── debugger/       # Time-travel debugger
│   ├── hmr/            # Hot module reloading
│   ├── observability/  # Tracing, metrics
│   ├── ai/             # AI tool generation
│   ├── wasm/           # WebAssembly support
│   └── effects/        # Algebraic effects
├── examples/           # This directory
├── benches/            # Performance benchmarks
└── tests/              # Integration tests
```
