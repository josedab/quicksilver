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

/// Benchmark: Destructuring patterns
fn bench_destructuring(c: &mut Criterion) {
    let mut group = c.benchmark_group("destructuring");

    group.bench_function("array_destructuring", |b| {
        let mut runtime = Runtime::new();
        b.iter(|| {
            runtime.eval(black_box("let [a, b, c] = [1, 2, 3]; a + b + c")).unwrap()
        })
    });

    group.bench_function("object_destructuring", |b| {
        let mut runtime = Runtime::new();
        b.iter(|| {
            runtime.eval(black_box("let {x, y, z} = {x: 1, y: 2, z: 3}; x + y + z")).unwrap()
        })
    });

    group.bench_function("nested_destructuring", |b| {
        let mut runtime = Runtime::new();
        b.iter(|| {
            runtime.eval(black_box("let {a: {b}} = {a: {b: 42}}; b")).unwrap()
        })
    });

    group.finish();
}

/// Benchmark: Closure creation and execution
fn bench_closures(c: &mut Criterion) {
    let mut group = c.benchmark_group("closures");

    group.bench_function("closure_creation", |b| {
        let mut runtime = Runtime::new();
        b.iter(|| {
            runtime.eval(black_box("let x = 10; let f = function() { return x; }; f")).unwrap()
        })
    });

    group.bench_function("closure_execution", |b| {
        let mut runtime = Runtime::new();
        runtime.eval("let x = 10; let f = function() { return x + 1; };").unwrap();
        b.iter(|| {
            runtime.eval(black_box("f()")).unwrap()
        })
    });

    group.bench_function("closure_chain", |b| {
        let mut runtime = Runtime::new();
        runtime.eval("function a(x) { return function() { return x * 2; }; }").unwrap();
        runtime.eval("let b = a(5);").unwrap();
        b.iter(|| {
            runtime.eval(black_box("b()")).unwrap()
        })
    });

    group.finish();
}

/// Benchmark: Promise creation and resolution
fn bench_promises(c: &mut Criterion) {
    let mut group = c.benchmark_group("promises");

    group.bench_function("promise_create_resolve", |b| {
        let mut runtime = Runtime::new();
        b.iter(|| {
            runtime.eval(black_box("new Promise(function(resolve) { resolve(42); })")).unwrap()
        })
    });

    group.bench_function("promise_then_chain", |b| {
        let mut runtime = Runtime::new();
        b.iter(|| {
            runtime.eval(black_box(
                "new Promise(function(resolve) { resolve(1); }).then(function(v) { return v + 1; })"
            )).unwrap()
        })
    });

    group.finish();
}

/// Benchmark: RegExp operations
fn bench_regex(c: &mut Criterion) {
    let mut group = c.benchmark_group("regex");

    group.bench_function("regex_test", |b| {
        let mut runtime = Runtime::new();
        b.iter(|| {
            runtime.eval(black_box("/^hello/.test('hello world')")).unwrap()
        })
    });

    group.bench_function("regex_match", |b| {
        let mut runtime = Runtime::new();
        b.iter(|| {
            runtime.eval(black_box("'hello world'.match(/\\w+/)")).unwrap()
        })
    });

    group.finish();
}

/// Benchmark: JSON parse + stringify roundtrips
fn bench_json_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("json_roundtrip");

    group.bench_function("small_json", |b| {
        let mut runtime = Runtime::new();
        b.iter(|| {
            runtime.eval(black_box("JSON.parse(JSON.stringify({a: 1, b: 2}))")).unwrap()
        })
    });

    group.bench_function("medium_json", |b| {
        let mut runtime = Runtime::new();
        b.iter(|| {
            runtime.eval(black_box(
                "JSON.parse(JSON.stringify({a: 1, b: [1, 2, 3], c: {d: 'hello', e: true}}))"
            )).unwrap()
        })
    });

    group.finish();
}

/// Benchmark: Map and Set operations
fn bench_map_set(c: &mut Criterion) {
    let mut group = c.benchmark_group("map_set");

    group.bench_function("map_set_get", |b| {
        let mut runtime = Runtime::new();
        b.iter(|| {
            runtime.eval(black_box(
                "let m = new Map(); m.set('a', 1); m.set('b', 2); m.get('a') + m.get('b')"
            )).unwrap()
        })
    });

    group.bench_function("set_add_has", |b| {
        let mut runtime = Runtime::new();
        b.iter(|| {
            runtime.eval(black_box(
                "let s = new Set(); s.add(1); s.add(2); s.add(3); s.has(2)"
            )).unwrap()
        })
    });

    group.finish();
}

/// Benchmark: Template literal interpolation
fn bench_template_literals(c: &mut Criterion) {
    let mut group = c.benchmark_group("template_literals");

    group.bench_function("simple_interpolation", |b| {
        let mut runtime = Runtime::new();
        runtime.eval("let name = 'World';").unwrap();
        b.iter(|| {
            runtime.eval(black_box("`Hello, ${name}!`")).unwrap()
        })
    });

    group.bench_function("complex_interpolation", |b| {
        let mut runtime = Runtime::new();
        runtime.eval("let a = 1; let b = 2; let c = 'test';").unwrap();
        b.iter(|| {
            runtime.eval(black_box("`a=${a}, b=${b}, sum=${a + b}, str=${c}`")).unwrap()
        })
    });

    group.finish();
}

/// Benchmark: Spread and rest operators
fn bench_spread_rest(c: &mut Criterion) {
    let mut group = c.benchmark_group("spread_rest");

    group.bench_function("array_spread", |b| {
        let mut runtime = Runtime::new();
        runtime.eval("let arr1 = [1, 2, 3]; let arr2 = [4, 5, 6];").unwrap();
        b.iter(|| {
            runtime.eval(black_box("[...arr1, ...arr2]")).unwrap()
        })
    });

    group.bench_function("function_rest", |b| {
        let mut runtime = Runtime::new();
        runtime.eval("function sum(...args) { let s = 0; for (let x of args) s += x; return s; }").unwrap();
        b.iter(|| {
            runtime.eval(black_box("sum(1, 2, 3, 4, 5)")).unwrap()
        })
    });

    group.finish();
}

/// Benchmark: Fibonacci comparison workload (standard cross-runtime benchmark)
fn bench_fibonacci_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("fibonacci_comparison");
    group.sample_size(50);

    for n in [5, 10, 15, 20] {
        group.bench_with_input(BenchmarkId::new("fib", n), &n, |b, &n| {
            let mut runtime = Runtime::new();
            runtime.eval("function fib(n) { return n <= 1 ? n : fib(n-1) + fib(n-2); }").unwrap();
            let code = format!("fib({})", n);
            b.iter(|| {
                runtime.eval(black_box(&code)).unwrap()
            })
        });
    }

    group.finish();
}

/// Benchmark: Property-heavy workload (simulates real-world object manipulation)
fn bench_property_intensive(c: &mut Criterion) {
    let mut group = c.benchmark_group("property_intensive");

    group.bench_function("object_create_access_100", |b| {
        let mut runtime = Runtime::new();
        b.iter(|| {
            runtime.eval(black_box(r#"
                let result = 0;
                for (let i = 0; i < 100; i++) {
                    let obj = { x: i, y: i * 2, z: i * 3 };
                    result += obj.x + obj.y + obj.z;
                }
                result
            "#)).unwrap()
        })
    });

    group.bench_function("array_map_filter_reduce", |b| {
        let mut runtime = Runtime::new();
        runtime.eval("let data = []; for (let i = 0; i < 50; i++) data.push(i);").unwrap();
        b.iter(|| {
            runtime.eval(black_box(
                "data.map(function(x) { return x * 2; }).filter(function(x) { return x > 10; }).reduce(function(a, b) { return a + b; }, 0)"
            )).unwrap()
        })
    });

    group.finish();
}

/// Benchmark: String-heavy workload
fn bench_string_intensive(c: &mut Criterion) {
    let mut group = c.benchmark_group("string_intensive");

    group.bench_function("string_build_100", |b| {
        let mut runtime = Runtime::new();
        b.iter(|| {
            runtime.eval(black_box(r#"
                let s = '';
                for (let i = 0; i < 100; i++) {
                    s += 'item' + i + ',';
                }
                s.length
            "#)).unwrap()
        })
    });

    group.bench_function("string_split_join", |b| {
        let mut runtime = Runtime::new();
        runtime.eval("let csv = 'a,b,c,d,e,f,g,h,i,j';").unwrap();
        b.iter(|| {
            runtime.eval(black_box("csv.split(',').join('-')")).unwrap()
        })
    });

    group.finish();
}

/// Benchmark: End-to-end real-world workloads
fn bench_real_world(c: &mut Criterion) {
    let mut group = c.benchmark_group("real_world");
    group.sample_size(50);

    group.bench_function("todo_app_simulation", |b| {
        let mut runtime = Runtime::new();
        b.iter(|| {
            runtime.eval(black_box(r#"
                let todos = [];
                for (let i = 0; i < 20; i++) {
                    todos.push({ id: i, text: 'Task ' + i, done: i % 3 === 0 });
                }
                let active = todos.filter(function(t) { return !t.done; });
                let completed = todos.filter(function(t) { return t.done; });
                let summary = {
                    total: todos.length,
                    active: active.length,
                    completed: completed.length
                };
                JSON.stringify(summary)
            "#)).unwrap()
        })
    });

    group.bench_function("class_hierarchy", |b| {
        let mut runtime = Runtime::new();
        b.iter(|| {
            runtime.eval(black_box(r#"
                class Shape {
                    constructor(type) { this.type = type; }
                    area() { return 0; }
                }
                class Circle extends Shape {
                    constructor(r) { super('circle'); this.r = r; }
                    area() { return 3.14159 * this.r * this.r; }
                }
                class Rect extends Shape {
                    constructor(w, h) { super('rect'); this.w = w; this.h = h; }
                    area() { return this.w * this.h; }
                }
                let shapes = [new Circle(5), new Rect(3, 4), new Circle(10), new Rect(7, 2)];
                let total = 0;
                for (let s of shapes) { total += s.area(); }
                total
            "#)).unwrap()
        })
    });

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
    bench_destructuring,
    bench_closures,
    bench_promises,
    bench_regex,
    bench_json_roundtrip,
    bench_map_set,
    bench_template_literals,
    bench_spread_rest,
    bench_fibonacci_comparison,
    bench_property_intensive,
    bench_string_intensive,
    bench_real_world,
);

criterion_main!(benches);
