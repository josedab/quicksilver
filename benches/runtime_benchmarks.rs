//! Performance benchmarks for Quicksilver runtime
//!
//! Run with: cargo bench
//!
//! These benchmarks measure key performance characteristics:
//! - Cold start time (how fast the runtime initializes)
//! - Bytecode compilation speed
//! - VM execution throughput
//! - Object allocation and GC pressure
//! - Built-in function performance

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use quicksilver::Runtime;

/// Benchmark: Cold start time (runtime initialization)
fn bench_cold_start(c: &mut Criterion) {
    c.bench_function("cold_start", |b| {
        b.iter(|| {
            let runtime = Runtime::new();
            black_box(runtime)
        })
    });
}

/// Benchmark: Simple expression evaluation
fn bench_simple_eval(c: &mut Criterion) {
    let mut group = c.benchmark_group("eval");

    // Arithmetic
    group.bench_function("arithmetic", |b| {
        let mut runtime = Runtime::new();
        b.iter(|| {
            runtime.eval(black_box("1 + 2 * 3 - 4 / 2")).unwrap()
        })
    });

    // String concatenation
    group.bench_function("string_concat", |b| {
        let mut runtime = Runtime::new();
        b.iter(|| {
            runtime.eval(black_box("'hello' + ' ' + 'world'")).unwrap()
        })
    });

    // Boolean logic
    group.bench_function("boolean_logic", |b| {
        let mut runtime = Runtime::new();
        b.iter(|| {
            runtime.eval(black_box("true && false || !false")).unwrap()
        })
    });

    group.finish();
}

/// Benchmark: Function calls
fn bench_function_calls(c: &mut Criterion) {
    let mut group = c.benchmark_group("function_calls");

    // Simple function
    group.bench_function("simple_call", |b| {
        let mut runtime = Runtime::new();
        runtime.eval("function add(a, b) { return a + b; }").unwrap();
        b.iter(|| {
            runtime.eval(black_box("add(1, 2)")).unwrap()
        })
    });

    // Recursive function (fibonacci)
    group.bench_function("recursive_fib_10", |b| {
        let mut runtime = Runtime::new();
        runtime.eval("function fib(n) { return n <= 1 ? n : fib(n-1) + fib(n-2); }").unwrap();
        b.iter(|| {
            runtime.eval(black_box("fib(10)")).unwrap()
        })
    });

    // Higher-order function
    group.bench_function("higher_order", |b| {
        let mut runtime = Runtime::new();
        runtime.eval("function apply(f, x) { return f(x); }").unwrap();
        runtime.eval("function double(x) { return x * 2; }").unwrap();
        b.iter(|| {
            runtime.eval(black_box("apply(double, 21)")).unwrap()
        })
    });

    group.finish();
}

/// Benchmark: Loop performance
fn bench_loops(c: &mut Criterion) {
    let mut group = c.benchmark_group("loops");

    // While loop
    group.bench_function("while_1000", |b| {
        let mut runtime = Runtime::new();
        b.iter(|| {
            runtime.eval(black_box("let i = 0; let sum = 0; while (i < 1000) { sum += i; i++; } sum")).unwrap()
        })
    });

    // For loop
    group.bench_function("for_1000", |b| {
        let mut runtime = Runtime::new();
        b.iter(|| {
            runtime.eval(black_box("let sum = 0; for (let i = 0; i < 1000; i++) { sum += i; } sum")).unwrap()
        })
    });

    group.finish();
}

/// Benchmark: Object operations
fn bench_objects(c: &mut Criterion) {
    let mut group = c.benchmark_group("objects");

    // Object creation
    group.bench_function("create_object", |b| {
        let mut runtime = Runtime::new();
        b.iter(|| {
            runtime.eval(black_box("({ a: 1, b: 2, c: 3 })")).unwrap()
        })
    });

    // Property access
    group.bench_function("property_access", |b| {
        let mut runtime = Runtime::new();
        runtime.eval("const obj = { a: 1, b: 2, c: 3 };").unwrap();
        b.iter(|| {
            runtime.eval(black_box("obj.a + obj.b + obj.c")).unwrap()
        })
    });

    // Nested objects
    group.bench_function("nested_access", |b| {
        let mut runtime = Runtime::new();
        runtime.eval("const obj = { a: { b: { c: 42 } } };").unwrap();
        b.iter(|| {
            runtime.eval(black_box("obj.a.b.c")).unwrap()
        })
    });

    group.finish();
}

/// Benchmark: Array operations
fn bench_arrays(c: &mut Criterion) {
    let mut group = c.benchmark_group("arrays");

    // Array creation
    group.bench_function("create_array_100", |b| {
        let mut runtime = Runtime::new();
        b.iter(|| {
            runtime.eval(black_box("let arr = []; for (let i = 0; i < 100; i++) arr.push(i); arr")).unwrap()
        })
    });

    // Array access
    group.bench_function("array_access", |b| {
        let mut runtime = Runtime::new();
        runtime.eval("const arr = [1, 2, 3, 4, 5];").unwrap();
        b.iter(|| {
            runtime.eval(black_box("arr[0] + arr[2] + arr[4]")).unwrap()
        })
    });

    // Array iteration
    group.bench_function("array_sum_100", |b| {
        let mut runtime = Runtime::new();
        runtime.eval("const arr = []; for (let i = 0; i < 100; i++) arr.push(i);").unwrap();
        b.iter(|| {
            runtime.eval(black_box("let sum = 0; for (let x of arr) sum += x; sum")).unwrap()
        })
    });

    group.finish();
}

/// Benchmark: Class operations
fn bench_classes(c: &mut Criterion) {
    let mut group = c.benchmark_group("classes");

    // Class instantiation
    group.bench_function("instantiate", |b| {
        let mut runtime = Runtime::new();
        runtime.eval("class Point { constructor(x, y) { this.x = x; this.y = y; } }").unwrap();
        b.iter(|| {
            runtime.eval(black_box("new Point(1, 2)")).unwrap()
        })
    });

    // Method calls
    group.bench_function("method_call", |b| {
        let mut runtime = Runtime::new();
        runtime.eval("class Counter { constructor() { this.n = 0; } inc() { this.n++; return this.n; } }").unwrap();
        runtime.eval("const c = new Counter();").unwrap();
        b.iter(|| {
            runtime.eval(black_box("c.inc()")).unwrap()
        })
    });

    group.finish();
}

/// Benchmark: Built-in functions
fn bench_builtins(c: &mut Criterion) {
    let mut group = c.benchmark_group("builtins");

    // Math functions
    group.bench_function("math_sqrt", |b| {
        let mut runtime = Runtime::new();
        b.iter(|| {
            runtime.eval(black_box("Math.sqrt(42)")).unwrap()
        })
    });

    group.bench_function("math_random", |b| {
        let mut runtime = Runtime::new();
        b.iter(|| {
            runtime.eval(black_box("Math.random()")).unwrap()
        })
    });

    // JSON operations
    group.bench_function("json_stringify", |b| {
        let mut runtime = Runtime::new();
        runtime.eval("const obj = { a: 1, b: 'hello', c: [1, 2, 3] };").unwrap();
        b.iter(|| {
            runtime.eval(black_box("JSON.stringify(obj)")).unwrap()
        })
    });

    group.bench_function("json_parse", |b| {
        let mut runtime = Runtime::new();
        b.iter(|| {
            runtime.eval(black_box("JSON.parse('{\"a\":1,\"b\":2}')")).unwrap()
        })
    });

    group.finish();
}

/// Benchmark: String operations
fn bench_strings(c: &mut Criterion) {
    let mut group = c.benchmark_group("strings");

    // Template literals
    group.bench_function("template_literal", |b| {
        let mut runtime = Runtime::new();
        runtime.eval("const name = 'World';").unwrap();
        b.iter(|| {
            runtime.eval(black_box("`Hello, ${name}!`")).unwrap()
        })
    });

    // String methods
    group.bench_function("string_methods", |b| {
        let mut runtime = Runtime::new();
        runtime.eval("const s = 'hello world';").unwrap();
        b.iter(|| {
            runtime.eval(black_box("s.toUpperCase()")).unwrap()
        })
    });

    group.finish();
}

/// Benchmark: Error handling
fn bench_error_handling(c: &mut Criterion) {
    let mut group = c.benchmark_group("error_handling");

    // Try-catch (no error)
    group.bench_function("try_no_error", |b| {
        let mut runtime = Runtime::new();
        b.iter(|| {
            runtime.eval(black_box("try { 1 + 2 } catch (e) { 0 }")).unwrap()
        })
    });

    // Try-catch (with error)
    group.bench_function("try_with_error", |b| {
        let mut runtime = Runtime::new();
        b.iter(|| {
            runtime.eval(black_box("try { throw new Error('test'); } catch (e) { 42 }")).unwrap()
        })
    });

    group.finish();
}

/// Benchmark: Compilation throughput
fn bench_compilation(c: &mut Criterion) {
    let mut group = c.benchmark_group("compilation");

    // Small program
    let small_program = "function add(a, b) { return a + b; } add(1, 2);";
    group.throughput(Throughput::Bytes(small_program.len() as u64));
    group.bench_function("small_program", |b| {
        b.iter(|| {
            let mut runtime = Runtime::new();
            runtime.eval(black_box(small_program)).unwrap()
        })
    });

    // Medium program
    let medium_program = r#"
        class Calculator {
            constructor() { this.result = 0; }
            add(n) { this.result += n; return this; }
            sub(n) { this.result -= n; return this; }
            mul(n) { this.result *= n; return this; }
            div(n) { this.result /= n; return this; }
            get() { return this.result; }
        }
        const calc = new Calculator();
        calc.add(10).mul(2).sub(5).div(3).get();
    "#;
    group.throughput(Throughput::Bytes(medium_program.len() as u64));
    group.bench_function("medium_program", |b| {
        b.iter(|| {
            let mut runtime = Runtime::new();
            runtime.eval(black_box(medium_program)).unwrap()
        })
    });

    group.finish();
}

/// Benchmark: Scalability test
fn bench_scalability(c: &mut Criterion) {
    let mut group = c.benchmark_group("scalability");
    group.sample_size(50);

    for size in [10, 100, 1000].iter() {
        group.throughput(Throughput::Elements(*size as u64));
        group.bench_with_input(BenchmarkId::new("loop_iterations", size), size, |b, &size| {
            let mut runtime = Runtime::new();
            let code = format!("let sum = 0; for (let i = 0; i < {}; i++) sum += i; sum", size);
            b.iter(|| {
                runtime.eval(black_box(&code)).unwrap()
            })
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_cold_start,
    bench_simple_eval,
    bench_function_calls,
    bench_loops,
    bench_objects,
    bench_arrays,
    bench_classes,
    bench_builtins,
    bench_strings,
    bench_error_handling,
    bench_compilation,
    bench_scalability,
);

criterion_main!(benches);
