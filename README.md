# Quicksilver

A memory-safe JavaScript runtime written in Rust.

Quicksilver is a JavaScript engine designed for embedded use cases, edge computing, and security-sensitive applications. Unlike V8 and SpiderMonkey, which are massive C++ codebases with ongoing memory safety vulnerabilities, Quicksilver prioritizes security and embeddability while maintaining competitive performance.

## Features

- **Memory Safety**: Pure Rust implementation eliminates common security vulnerability classes
- **Small Footprint**: Designed for embedding in resource-constrained environments
- **Fast Cold Starts**: Quick startup time for serverless and edge deployments
- **ES2020 Support**: Modern JavaScript features including classes, arrow functions, destructuring, and more

## Supported JavaScript Features

### Core Language
- Variables: `let`, `const`, `var`
- Data types: numbers, strings, booleans, null, undefined, objects, arrays
- Operators: arithmetic, comparison, logical, bitwise
- Control flow: `if/else`, `while`, `for`, `for...in`, `for...of`
- Functions: declarations, expressions, arrow functions
- Classes: constructors, properties
- Error handling: `try/catch/finally`, `throw`
- Destructuring: array and object patterns

### Built-in Objects
- `console.log()` for output
- `Math` object with common methods
- Array and Object manipulation

## Installation

### From Source

```bash
git clone https://github.com/example/quicksilver.git
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
