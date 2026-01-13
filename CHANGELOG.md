# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2024-01-12

### Added

#### Core JavaScript Runtime
- Bytecode-compiled, stack-based JavaScript interpreter
- Lexer with full ES2020 token support
- Recursive descent parser for JavaScript syntax
- Bytecode compiler with optimization passes
- Stack-based virtual machine with 10,000 stack limit and 1,000 call depth limit

#### Language Features
- Variables: `let`, `const`, `var` with proper scoping
- Data types: numbers, strings, booleans, null, undefined, objects, arrays, symbols
- Operators: arithmetic, comparison, logical, bitwise, spread (`...`), optional chaining (`?.`), nullish coalescing (`??`)
- Control flow: `if/else`, `while`, `for`, `for...in`, `for...of`, `switch`, `break`, `continue`
- Functions: declarations, expressions, arrow functions, default parameters, rest parameters
- Classes: constructors, instance properties, `instanceof` operator
- Error handling: `try/catch/finally`, `throw` with Error types
- Destructuring: array and object patterns with defaults
- Template literals with expression interpolation

#### Built-in Objects
- `console`: log, warn, error, info, debug, group, groupEnd, time, timeEnd, count, table, assert, trace, clear
- `Math`: All standard methods (sin, cos, abs, min, max, random, pow, sqrt, floor, ceil, round, etc.)
- `JSON`: parse, stringify with reviver/replacer support
- `Date`: Full constructor and getter/setter support, formatting methods
- `Array`: map, filter, reduce, forEach, find, findIndex, includes, slice, splice, sort, reverse, flat, flatMap, join, concat, push, pop, shift, unshift, etc.
- `String`: charAt, substring, slice, indexOf, toUpperCase, toLowerCase, trim, split, replace, match, repeat, startsWith, endsWith, includes, padStart, padEnd, etc.
- `Object`: keys, values, entries, assign, create, defineProperty, freeze, seal
- `Map` and `Set`: Full collection support
- `RegExp`: Pattern matching with test, exec, match, replace, split
- Typed Arrays: Int8Array, Uint8Array, Int16Array, Uint16Array, Int32Array, Uint32Array, Float32Array, Float64Array
- `URL`: URL parsing and manipulation
- `TextEncoder`/`TextDecoder`: UTF-8 encoding support
- Timers: setTimeout, setInterval, clearTimeout, clearInterval

#### Runtime Infrastructure
- Mark-and-sweep garbage collection with configurable thresholds
- Snapshot serialization for instant cold starts
- Capability-based security sandbox with fine-grained permissions
- Structured concurrency with Go-style channels
- Observability: OpenTelemetry-compatible tracing, metrics (Counter, Gauge, Histogram)
- AI-native runtime: JSDoc to LLM tool schema generation (OpenAI and Anthropic formats)
- WebAssembly module parsing (execution pending)
- Algebraic effects system framework
- Time-travel debugger framework
- Hot module reloading framework

#### CLI
- `quicksilver <file.js>` - Run JavaScript files
- `quicksilver -e "<code>"` - Evaluate expressions
- `quicksilver repl` - Interactive REPL with readline support
- `quicksilver compile` - Compile to bytecode
- `quicksilver snapshot` - Create/load snapshots

#### Developer Experience
- Comprehensive error messages with source locations and stack traces
- 217 tests (178 unit + 39 integration)
- 8 example programs demonstrating features

### Known Limitations

The following features are recognized by the lexer but not yet fully implemented:
- Generators (`yield`)
- ES Modules (`import`/`export`)
- Full `async`/`await` with event loop
- `Proxy` traps (constructor exists)
- True weak reference semantics for `WeakMap`/`WeakSet`

### Infrastructure

- Rust 2021 edition
- MIT License
- Optimized release builds with LTO

[0.1.0]: https://github.com/anthropics/quicksilver/releases/tag/v0.1.0
