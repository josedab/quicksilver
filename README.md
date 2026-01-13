# Quicksilver

A memory-safe JavaScript runtime written in Rust.

Quicksilver is a JavaScript engine designed for embedded use cases, edge computing, and security-sensitive applications. Unlike V8 and SpiderMonkey, which are massive C++ codebases with ongoing memory safety vulnerabilities, Quicksilver prioritizes security and embeddability while maintaining competitive performance.

## Features

- **Memory Safety**: Pure Rust implementation eliminates common security vulnerability classes
- **Small Footprint**: Designed for embedding in resource-constrained environments
- **Fast Cold Starts**: Snapshot serialization enables instant cold starts
- **ES2020 Support**: Modern JavaScript features including classes, arrow functions, destructuring, and more
- **Capability-Based Security**: Fine-grained sandboxing for untrusted code
- **AI-Native**: JSDoc to LLM tool schema generation

## Supported JavaScript Features

### Core Language
- Variables: `let`, `const`, `var` with proper scoping
- Data types: numbers, strings, booleans, null, undefined, objects, arrays, symbols
- Operators: arithmetic, comparison, logical, bitwise, spread (`...`), optional chaining (`?.`), nullish coalescing (`??`)
- Control flow: `if/else`, `while`, `for`, `for...in`, `for...of`, `switch`, `break`, `continue`
- Functions: declarations, expressions, arrow functions, default parameters, rest parameters
- Classes: constructors, instance properties, `instanceof`
- Error handling: `try/catch/finally`, `throw` with Error types
- Destructuring: array and object patterns with defaults
- Template literals with expression interpolation

### Built-in Objects
- `console`: log, warn, error, info, debug, group, time, timeEnd, count, table, assert, trace
- `Math`: All standard methods (sin, cos, abs, min, max, random, pow, sqrt, floor, ceil, round, etc.)
- `JSON`: parse, stringify with full options
- `Date`: Constructor, getters, setters, formatting
- `Array`: map, filter, reduce, forEach, find, includes, slice, splice, sort, flat, flatMap, etc.
- `String`: charAt, substring, indexOf, toUpperCase, toLowerCase, trim, split, replace, match, etc.
- `Object`: keys, values, entries, assign, create, defineProperty, freeze, seal
- `Map` and `Set`: Full collection support
- `RegExp`: Pattern matching with test, exec, match, replace, split
- Typed Arrays: Int8Array, Uint8Array, Float32Array, Float64Array, etc.
- `URL`: URL parsing and manipulation
- `TextEncoder`/`TextDecoder`: UTF-8 encoding
- Timers: setTimeout, setInterval

### Runtime Features
- Mark-and-sweep garbage collection
- Snapshot serialization for instant cold starts
- Capability-based security sandbox
- Structured concurrency with channels
- Observability (tracing, metrics)
- WebAssembly module parsing
- Time-Travel Debugger with record/replay
- Hot Module Reloading (HMR) with state preservation

### Not Yet Implemented
- Generators (`yield`)
- ES Modules (`import`/`export`)
- Full `async`/`await` with event loop
- `Proxy` traps

## Installation

### From Source

```bash
git clone https://github.com/anthropics/quicksilver.git
cd quicksilver
cargo build --release
```

## Usage

### Command Line

Run a JavaScript file:
```bash
quicksilver script.js
```

Evaluate an expression:
```bash
quicksilver -e "console.log('Hello, World!')"
```

### As a Library

Add to your `Cargo.toml`:
```toml
[dependencies]
quicksilver = "0.1.0"
```

Use in your Rust code:
```rust
use quicksilver::{Runtime, Value};

fn main() -> quicksilver::Result<()> {
    let mut runtime = Runtime::new();

    // Evaluate expressions
    let result = runtime.eval("1 + 2 * 3")?;
    println!("Result: {:?}", result);

    // Run JavaScript code
    runtime.eval("
        function greet(name) {
            return 'Hello, ' + name + '!';
        }
        console.log(greet('World'));
    ")?;

    Ok(())
}
```

## Examples

### Arrow Functions
```javascript
let add = (a, b) => a + b;
let square = x => x * x;
console.log(add(2, 3));    // 5
console.log(square(4));    // 16
```

### Classes
```javascript
class Person {
    constructor(name, age) {
        this.name = name;
        this.age = age;
    }
}

let person = new Person('Alice', 30);
console.log(person.name);  // Alice
```

### Destructuring
```javascript
let arr = [1, 2, 3];
let [a, b, c] = arr;
console.log(a, b, c);  // 1 2 3

let obj = {x: 10, y: 20};
let {x, y} = obj;
console.log(x, y);  // 10 20
```

### Error Handling
```javascript
try {
    throw 'Something went wrong';
} catch (e) {
    console.log('Caught:', e);
} finally {
    console.log('Cleanup');
}
```

## Architecture

Quicksilver uses a bytecode compilation and interpretation approach:

1. **Lexer** (`src/lexer/`): Tokenizes JavaScript source code
2. **Parser** (`src/parser/`): Builds an Abstract Syntax Tree (AST)
3. **Compiler** (`src/bytecode/`): Compiles AST to bytecode
4. **VM** (`src/runtime/`): Executes bytecode with a stack-based interpreter
5. **GC** (`src/gc/`): Garbage collection for memory management

## Development

### Building
```bash
cargo build
```

### Testing
```bash
cargo test
```

### Running Clippy
```bash
cargo clippy
```

## License

MIT License - see LICENSE file for details.
