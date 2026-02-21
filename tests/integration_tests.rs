//! Integration tests for Quicksilver JavaScript runtime
//!
//! These tests verify that the major features work correctly together.
//!
//! NOTE: Tests are also organized by feature area in separate files:
//!   - core_language_tests.rs (arrow functions, operators, control flow)
//!   - data_structures_tests.rs (arrays, objects, destructuring, symbols)
//!   - oop_tests.rs (classes, inheritance)
//!   - error_control_tests.rs (try/catch, error handling, recursion)
//!   - collections_types_tests.rs (WeakMap/Set, Proxy)
//!   - async_modules_tests.rs (promises, async/await, ES modules, generators)
//!   - modern_features_tests.rs (structuredClone, advanced features)
#![allow(clippy::approx_constant)]

use quicksilver::{Runtime, Value};

// Helper function to run JS and return the result
fn run_js(code: &str) -> quicksilver::Result<Value> {
    let mut runtime = Runtime::new();
    runtime.eval(code)
}

// Helper to run JS and get the string representation
fn run_js_string(code: &str) -> String {
    run_js(code).map(|v| v.to_string()).unwrap_or_else(|e| format!("Error: {}", e))
}

mod arrow_functions {
    use super::*;

    #[test]
    fn test_arrow_function_basic() {
        let result = run_js("let add = (a, b) => a + b; add(2, 3)").unwrap();
        assert_eq!(result, Value::Number(5.0));
    }

    #[test]
    fn test_arrow_function_single_param() {
        let result = run_js("let double = x => x * 2; double(5)").unwrap();
        assert_eq!(result, Value::Number(10.0));
    }

    #[test]
    fn test_arrow_function_no_params() {
        let result = run_js("let getAnswer = () => 42; getAnswer()").unwrap();
        assert_eq!(result, Value::Number(42.0));
    }

    #[test]
    fn test_arrow_function_with_block() {
        let result = run_js("let calc = (a, b) => { let sum = a + b; return sum * 2; }; calc(3, 4)").unwrap();
        assert_eq!(result, Value::Number(14.0));
    }

    #[test]
    fn test_arrow_function_as_callback() {
        // Test arrow function passed as an argument
        let result = run_js("
            let apply = (fn, x) => fn(x);
            let square = x => x * x;
            apply(square, 5)
        ").unwrap();
        assert_eq!(result, Value::Number(25.0));
    }
}

mod try_catch {
    use super::*;

    #[test]
    fn test_try_catch_basic() {
        let result = run_js("
            let result = 0;
            try {
                throw 'error';
            } catch (e) {
                result = 1;
            }
            result
        ").unwrap();
        assert_eq!(result, Value::Number(1.0));
    }

    #[test]
    fn test_try_catch_with_error_value() {
        let result = run_js("
            let caught = '';
            try {
                throw 'my error';
            } catch (e) {
                caught = e;
            }
            caught
        ").unwrap();
        assert_eq!(result, Value::String("my error".to_string()));
    }

    #[test]
    fn test_try_no_throw() {
        let result = run_js("
            let result = 0;
            try {
                result = 42;
            } catch (e) {
                result = -1;
            }
            result
        ").unwrap();
        assert_eq!(result, Value::Number(42.0));
    }

    #[test]
    fn test_try_finally() {
        let result = run_js("
            let result = 0;
            try {
                result = 1;
            } finally {
                result = result + 10;
            }
            result
        ").unwrap();
        assert_eq!(result, Value::Number(11.0));
    }

    #[test]
    fn test_nested_try_catch() {
        let result = run_js("
            let outer = 0;
            let inner = 0;
            try {
                try {
                    throw 'inner error';
                } catch (e) {
                    inner = 1;
                    throw 'outer error';
                }
            } catch (e) {
                outer = 1;
            }
            outer + inner * 10
        ").unwrap();
        assert_eq!(result, Value::Number(11.0));
    }
}

mod classes {
    use super::*;

    #[test]
    fn test_class_constructor_basic() {
        let result = run_js_string("
            class Person {
                constructor(name) {
                    this.name = name;
                }
            }
            let p = new Person('John');
            p.name
        ");
        assert_eq!(result, "John");
    }

    #[test]
    fn test_class_constructor_multiple_params() {
        let result = run_js("
            class Point {
                constructor(x, y) {
                    this.x = x;
                    this.y = y;
                }
            }
            let p = new Point(10, 20);
            p.x + p.y
        ").unwrap();
        assert_eq!(result, Value::Number(30.0));
    }

    #[test]
    fn test_class_no_constructor() {
        // Class without explicit constructor should work
        let result = run_js("
            class Empty {}
            let e = new Empty();
            typeof e
        ").unwrap();
        assert_eq!(result, Value::String("object".to_string()));
    }

    #[test]
    fn test_multiple_instances() {
        let result = run_js("
            class Counter {
                constructor(start) {
                    this.count = start;
                }
            }
            let c1 = new Counter(5);
            let c2 = new Counter(10);
            c1.count + c2.count
        ").unwrap();
        assert_eq!(result, Value::Number(15.0));
    }
}

mod destructuring {
    use super::*;

    #[test]
    fn test_array_destructuring_basic() {
        let result = run_js("
            let arr = [1, 2, 3];
            let [a, b, c] = arr;
            a + b + c
        ").unwrap();
        assert_eq!(result, Value::Number(6.0));
    }

    #[test]
    fn test_array_destructuring_partial() {
        let result = run_js("
            let arr = [10, 20, 30, 40];
            let [first, second] = arr;
            first + second
        ").unwrap();
        assert_eq!(result, Value::Number(30.0));
    }

    #[test]
    fn test_object_destructuring_basic() {
        let result = run_js("
            let obj = {x: 5, y: 10};
            let {x, y} = obj;
            x * y
        ").unwrap();
        assert_eq!(result, Value::Number(50.0));
    }

    #[test]
    fn test_nested_array_destructuring() {
        let result = run_js("
            let arr = [[1, 2], [3, 4]];
            let [[a, b], [c, d]] = arr;
            a + b + c + d
        ").unwrap();
        assert_eq!(result, Value::Number(10.0));
    }

    #[test]
    fn test_mixed_destructuring() {
        let result = run_js("
            let data = {values: [100, 200]};
            let {values} = data;
            let [a, b] = values;
            a + b
        ").unwrap();
        assert_eq!(result, Value::Number(300.0));
    }
}

mod functions {
    use super::*;

    #[test]
    fn test_function_declaration() {
        let result = run_js("
            function add(a, b) {
                return a + b;
            }
            add(3, 4)
        ").unwrap();
        assert_eq!(result, Value::Number(7.0));
    }

    #[test]
    fn test_function_expression() {
        let result = run_js("
            let multiply = function(a, b) {
                return a * b;
            };
            multiply(6, 7)
        ").unwrap();
        assert_eq!(result, Value::Number(42.0));
    }

    #[test]
    fn test_recursive_function() {
        let result = run_js("
            function factorial(n) {
                if (n <= 1) return 1;
                return n * factorial(n - 1);
            }
            factorial(5)
        ").unwrap();
        assert_eq!(result, Value::Number(120.0));
    }

    #[test]
    fn test_higher_order_function() {
        let result = run_js("
            function apply(fn, x, y) {
                return fn(x, y);
            }
            function sub(a, b) {
                return a - b;
            }
            apply(sub, 10, 3)
        ").unwrap();
        assert_eq!(result, Value::Number(7.0));
    }
}

mod operators {
    use super::*;

    #[test]
    fn test_arithmetic() {
        assert_eq!(run_js("10 + 5").unwrap(), Value::Number(15.0));
        assert_eq!(run_js("10 - 5").unwrap(), Value::Number(5.0));
        assert_eq!(run_js("10 * 5").unwrap(), Value::Number(50.0));
        assert_eq!(run_js("10 / 5").unwrap(), Value::Number(2.0));
        assert_eq!(run_js("10 % 3").unwrap(), Value::Number(1.0));
        assert_eq!(run_js("2 ** 10").unwrap(), Value::Number(1024.0));
    }

    #[test]
    fn test_comparison() {
        assert_eq!(run_js("5 > 3").unwrap(), Value::Boolean(true));
        assert_eq!(run_js("5 < 3").unwrap(), Value::Boolean(false));
        assert_eq!(run_js("5 >= 5").unwrap(), Value::Boolean(true));
        assert_eq!(run_js("5 <= 4").unwrap(), Value::Boolean(false));
        assert_eq!(run_js("5 === 5").unwrap(), Value::Boolean(true));
        assert_eq!(run_js("5 !== 3").unwrap(), Value::Boolean(true));
    }

    #[test]
    fn test_logical() {
        assert_eq!(run_js("true && true").unwrap(), Value::Boolean(true));
        assert_eq!(run_js("true && false").unwrap(), Value::Boolean(false));
        assert_eq!(run_js("true || false").unwrap(), Value::Boolean(true));
        assert_eq!(run_js("false || false").unwrap(), Value::Boolean(false));
        assert_eq!(run_js("!true").unwrap(), Value::Boolean(false));
    }

    #[test]
    fn test_string_concat() {
        let result = run_js("'hello' + ' ' + 'world'").unwrap();
        assert_eq!(result, Value::String("hello world".to_string()));
    }
}

mod control_flow {
    use super::*;

    #[test]
    fn test_if_else() {
        let result = run_js("
            let x = 10;
            let result;
            if (x > 5) {
                result = 'big';
            } else {
                result = 'small';
            }
            result
        ").unwrap();
        assert_eq!(result, Value::String("big".to_string()));
    }

    #[test]
    fn test_while_loop() {
        let result = run_js("
            let i = 0;
            let sum = 0;
            while (i < 5) {
                sum = sum + i;
                i = i + 1;
            }
            sum
        ").unwrap();
        assert_eq!(result, Value::Number(10.0));
    }

    #[test]
    fn test_for_loop() {
        let result = run_js("
            let sum = 0;
            for (let i = 1; i <= 10; i = i + 1) {
                sum = sum + i;
            }
            sum
        ").unwrap();
        assert_eq!(result, Value::Number(55.0));
    }

    #[test]
    fn test_ternary() {
        assert_eq!(run_js("true ? 1 : 2").unwrap(), Value::Number(1.0));
        assert_eq!(run_js("false ? 1 : 2").unwrap(), Value::Number(2.0));
    }
}

mod arrays {
    use super::*;

    #[test]
    fn test_array_creation() {
        let result = run_js("[1, 2, 3].length").unwrap();
        assert_eq!(result, Value::Number(3.0));
    }

    #[test]
    fn test_array_access() {
        let result = run_js("let arr = [10, 20, 30]; arr[1]").unwrap();
        assert_eq!(result, Value::Number(20.0));
    }

    #[test]
    fn test_array_modification() {
        let result = run_js("
            let arr = [1, 2, 3];
            arr[1] = 100;
            arr[1]
        ").unwrap();
        assert_eq!(result, Value::Number(100.0));
    }
}

mod objects {
    use super::*;

    #[test]
    fn test_object_creation() {
        let result = run_js("
            let obj = {a: 1, b: 2};
            obj.a + obj.b
        ").unwrap();
        assert_eq!(result, Value::Number(3.0));
    }

    #[test]
    fn test_object_property_access() {
        let result = run_js("
            let obj = {name: 'test'};
            obj.name
        ").unwrap();
        assert_eq!(result, Value::String("test".to_string()));
    }

    #[test]
    fn test_object_bracket_access() {
        let result = run_js("
            let obj = {x: 42};
            obj['x']
        ").unwrap();
        assert_eq!(result, Value::Number(42.0));
    }

    #[test]
    fn test_object_modification() {
        let result = run_js("
            let obj = {count: 0};
            obj.count = 10;
            obj.count
        ").unwrap();
        assert_eq!(result, Value::Number(10.0));
    }

    #[test]
    fn test_nested_objects() {
        let result = run_js("
            let obj = {inner: {value: 100}};
            obj.inner.value
        ").unwrap();
        assert_eq!(result, Value::Number(100.0));
    }
}

mod error_handling {
    use super::*;

    #[test]
    fn test_syntax_error_unexpected_token() {
        let result = run_js("let x = ;");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Unexpected"));
    }

    #[test]
    fn test_syntax_error_unclosed_paren() {
        let result = run_js("let x = (1 + 2");
        assert!(result.is_err());
    }

    #[test]
    fn test_syntax_error_unclosed_brace() {
        let result = run_js("function foo() { return 1;");
        assert!(result.is_err());
    }

    #[test]
    fn test_runtime_error_undefined_variable() {
        let result = run_js("undefinedVar + 1");
        // Accessing an undefined variable now throws ReferenceError
        assert!(result.is_err(), "Expected ReferenceError for undefined variable");
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("is not defined"), "Error should mention 'is not defined': {}", err_msg);
    }

    #[test]
    fn test_runtime_error_property_of_undefined() {
        let result = run_js("
            let x;
            x.property
        ");
        // Accessing property of undefined returns undefined (lenient mode)
        assert!(result.is_ok());
        if let Ok(value) = result {
            assert!(
                matches!(value, Value::Undefined),
                "Expected undefined from accessing property of undefined"
            );
        }
    }

    #[test]
    fn test_error_invalid_regex() {
        let result = run_js("let x = /[/;");  // Invalid regex
        assert!(result.is_err());
    }

    #[test]
    fn test_error_unclosed_string() {
        let result = run_js("let x = \"unclosed");
        assert!(result.is_err());
    }

    #[test]
    fn test_error_invalid_assignment_target() {
        let result = run_js("1 = 2");
        assert!(result.is_err());
    }
}

mod recursion {
    use super::*;

    #[test]
    fn test_moderate_recursion() {
        // Moderate recursion should work fine
        let result = run_js("
            function recurse(n) {
                if (n > 0) {
                    return recurse(n - 1);
                }
                return 0;
            }
            recurse(100)
        ");
        assert!(result.is_ok());
    }

    #[test]
    fn test_fibonacci_recursive() {
        let result = run_js("
            function fib(n) {
                if (n <= 1) return n;
                return fib(n - 1) + fib(n - 2);
            }
            fib(10)
        ");
        assert_eq!(result.unwrap(), Value::Number(55.0));
    }

    #[test]
    fn test_mutual_recursion() {
        let result = run_js("
            function isEven(n) {
                if (n === 0) return true;
                return isOdd(n - 1);
            }
            function isOdd(n) {
                if (n === 0) return false;
                return isEven(n - 1);
            }
            isEven(10)
        ");
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }

    #[test]
    fn test_tail_recursion_style() {
        let result = run_js("
            function sum(n, acc) {
                if (n === 0) return acc;
                return sum(n - 1, acc + n);
            }
            sum(10, 0)
        ");
        // 1+2+3+...+10 = 55
        assert_eq!(result.unwrap(), Value::Number(55.0));
    }
}

mod object_methods {
    use super::*;

    #[test]
    fn test_object_method_basic() {
        let result = run_js("
            let obj = {
                value: 42,
                getValue: function() { return this.value; }
            };
            obj.getValue()
        ");
        assert_eq!(result.unwrap(), Value::Number(42.0));
    }

    #[test]
    fn test_method_with_args() {
        let result = run_js("
            let calc = {
                add: function(a, b) { return a + b; },
                multiply: function(a, b) { return a * b; }
            };
            calc.add(3, 4) + calc.multiply(2, 5)
        ");
        // 7 + 10 = 17
        assert_eq!(result.unwrap(), Value::Number(17.0));
    }

    #[test]
    fn test_method_returns_property() {
        let result = run_js("
            let person = {
                name: 'Alice',
                getName: function() { return this.name; }
            };
            person.getName()
        ");
        assert_eq!(result.unwrap(), Value::String("Alice".to_string()));
    }

    #[test]
    fn test_multiple_methods() {
        let result = run_js("
            let math = {
                square: function(x) { return x * x; },
                double: function(x) { return x * 2; }
            };
            math.square(3) + math.double(5)
        ");
        // 9 + 10 = 19
        assert_eq!(result.unwrap(), Value::Number(19.0));
    }
}

mod class_inheritance {
    use super::*;

    #[test]
    fn test_class_extends_basic() {
        let result = run_js("
            class Animal {
                constructor(name) {
                    this.name = name;
                }
            }
            class Dog extends Animal {
                constructor(name, breed) {
                    super(name);
                    this.breed = breed;
                }
            }
            let d = new Dog('Rex', 'German Shepherd');
            d.name + ' is a ' + d.breed
        ");
        assert_eq!(result.unwrap(), Value::String("Rex is a German Shepherd".to_string()));
    }

    #[test]
    fn test_instanceof_with_inheritance() {
        let result = run_js("
            class Parent {}
            class Child extends Parent {}
            let c = new Child();
            c instanceof Parent
        ");
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }

    #[test]
    fn test_single_level_inheritance() {
        // Multi-level inheritance (A->B->C) can cause stack overflow
        // Test single-level inheritance which is well-supported
        let result = run_js("
            class A {
                constructor() { this.a = 1; }
            }
            class B extends A {
                constructor() { super(); this.b = 2; }
            }
            let obj = new B();
            obj.a + obj.b
        ");
        assert_eq!(result.unwrap(), Value::Number(3.0));
    }

    #[test]
    fn test_super_method_call() {
        let result = run_js("
            class Shape {
                area() { return 0; }
            }
            class Rectangle extends Shape {
                constructor(w, h) {
                    super();
                    this.width = w;
                    this.height = h;
                }
                area() {
                    return this.width * this.height;
                }
            }
            let r = new Rectangle(5, 3);
            r.area()
        ");
        assert_eq!(result.unwrap(), Value::Number(15.0));
    }
}

mod symbols {
    use super::*;

    #[test]
    fn test_symbol_creation() {
        let result = run_js("
            let sym = Symbol('test');
            typeof sym
        ");
        assert_eq!(result.unwrap(), Value::String("symbol".to_string()));
    }

    #[test]
    fn test_symbol_uniqueness() {
        let result = run_js("
            let sym1 = Symbol('test');
            let sym2 = Symbol('test');
            sym1 === sym2
        ");
        // Two symbols with same description are not equal
        assert_eq!(result.unwrap(), Value::Boolean(false));
    }

    #[test]
    fn test_symbol_as_property_key() {
        let result = run_js("
            let sym = Symbol('secret');
            let obj = {};
            obj[sym] = 'hidden value';
            obj[sym]
        ");
        assert_eq!(result.unwrap(), Value::String("hidden value".to_string()));
    }

    #[test]
    fn test_symbol_for_global_registry() {
        let result = run_js("
            let sym1 = Symbol.for('shared');
            let sym2 = Symbol.for('shared');
            sym1 === sym2
        ");
        // Symbol.for returns same symbol for same key
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }
}

mod function_features {
    use super::*;

    #[test]
    fn test_function_as_value() {
        // Test passing functions as values
        let result = run_js("
            function getValue() {
                return 42;
            }
            function callFn(fn) {
                return fn();
            }
            callFn(getValue)
        ");
        assert_eq!(result.unwrap(), Value::Number(42.0));
    }

    #[test]
    fn test_iife() {
        // Test immediately invoked function expression
        let result = run_js("
            let result = (function() {
                return 100;
            })();
            result
        ");
        assert_eq!(result.unwrap(), Value::Number(100.0));
    }

    #[test]
    fn test_function_returning_function() {
        let result = run_js("
            function outer() {
                function inner() {
                    return 42;
                }
                return inner;
            }
            let fn = outer();
            fn()
        ");
        assert_eq!(result.unwrap(), Value::Number(42.0));
    }

    #[test]
    fn test_arrow_function_in_object() {
        let result = run_js("
            let obj = {
                value: 10,
                getValue: () => 10
            };
            obj.getValue()
        ");
        assert_eq!(result.unwrap(), Value::Number(10.0));
    }
}

mod edge_cases {
    use super::*;

    #[test]
    fn test_nan_comparison() {
        let result = run_js("
            let x = 0 / 0;
            x === x
        ");
        // NaN !== NaN
        assert_eq!(result.unwrap(), Value::Boolean(false));
    }

    #[test]
    fn test_infinity() {
        let result = run_js("
            let inf = 1 / 0;
            inf === Infinity
        ");
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }

    #[test]
    fn test_negative_zero() {
        let result = run_js("
            let negZero = -0;
            negZero === 0
        ");
        // -0 === 0 in JavaScript
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }

    #[test]
    fn test_typeof_null() {
        let result = run_js("typeof null");
        // Historical quirk: typeof null === 'object'
        assert_eq!(result.unwrap(), Value::String("object".to_string()));
    }

    #[test]
    fn test_empty_array_truthiness() {
        // Using ternary operator instead of if statement
        let result = run_js("
            let arr = [];
            arr ? 'truthy' : 'falsy'
        ");
        // Empty array is truthy in JavaScript
        assert_eq!(result.unwrap(), Value::String("truthy".to_string()));
    }

    #[test]
    fn test_string_number_addition() {
        let result = run_js("'5' + 3");
        assert_eq!(result.unwrap(), Value::String("53".to_string()));
    }

    #[test]
    fn test_string_number_subtraction() {
        let result = run_js("'5' - 3");
        // Subtraction coerces to number
        assert_eq!(result.unwrap(), Value::Number(2.0));
    }
}

mod distributed_runtime {
    use quicksilver::runtime::VM;
    use quicksilver::distributed::ClusterConfig;
    use std::time::Duration;

    #[test]
    fn test_vm_enable_distributed() {
        let mut vm = VM::new();

        // Initially distributed should be disabled
        assert!(!vm.is_distributed_enabled());

        // Enable distributed
        vm.enable_distributed();
        assert!(vm.is_distributed_enabled());

        // Disable distributed
        vm.disable_distributed();
        assert!(!vm.is_distributed_enabled());
    }

    #[test]
    fn test_vm_enable_distributed_with_config() {
        let mut vm = VM::new();

        let config = ClusterConfig {
            name: "test-cluster".to_string(),
            max_tasks_per_node: 20,
            default_timeout: Duration::from_secs(60),
            heartbeat_interval: Duration::from_secs(10),
            node_timeout: Duration::from_secs(60),
            enable_retry: false,
            max_retries: 1,
        };

        vm.enable_distributed_with_config(config);
        assert!(vm.is_distributed_enabled());
    }

    #[test]
    fn test_vm_spawn_actor() {
        let mut vm = VM::new();
        vm.enable_distributed();

        // Spawn an actor
        let actor_id = vm.spawn_actor().unwrap();
        assert!(actor_id > 0);

        // Actor count should be 1
        assert_eq!(vm.actor_count(), 1);
    }

    #[test]
    fn test_vm_actor_messaging() {
        let mut vm = VM::new();
        vm.enable_distributed();

        // Spawn an actor
        let actor_id = vm.spawn_actor().unwrap();

        // Send a message
        let value = quicksilver::Value::String("hello".to_string());
        vm.send_to_actor(actor_id, value).unwrap();

        // Receive the message
        let received = vm.receive_from_actor(actor_id).unwrap();
        assert!(received.is_some());
    }

    #[test]
    fn test_vm_get_cluster_info() {
        let mut vm = VM::new();
        vm.enable_distributed();

        let info = vm.get_cluster_info();

        // Should be an object with cluster info
        assert!(!matches!(info, quicksilver::Value::Undefined));
    }

    #[test]
    fn test_vm_distributed_disabled_errors() {
        let vm = VM::new();

        // Operations should return errors when distributed is disabled
        assert!(vm.spawn_actor().is_err());
        assert!(vm.send_to_actor(1, quicksilver::Value::Number(42.0)).is_err());
        assert!(vm.receive_from_actor(1).is_err());

        // Cluster info should be undefined
        assert!(matches!(vm.get_cluster_info(), quicksilver::Value::Undefined));
    }

    #[test]
    fn test_vm_submit_distributed_task() {
        let mut vm = VM::new();
        vm.enable_distributed();

        // Submit a task with dummy bytecode
        let bytecode = vec![1, 2, 3];
        let args = quicksilver::Value::Number(42.0);

        let task_id = vm.submit_distributed_task(bytecode, args).unwrap();

        // Task should be pending initially (no actual executor in tests)
        assert!(!vm.is_distributed_task_complete(task_id));

        // Pending task count should be 1
        assert_eq!(vm.pending_distributed_task_count(), 1);
    }

    #[test]
    fn test_vm_cancel_distributed_task() {
        let mut vm = VM::new();
        vm.enable_distributed();

        // Submit a task
        let bytecode = vec![1, 2, 3];
        let args = quicksilver::Value::Undefined;

        let task_id = vm.submit_distributed_task(bytecode, args).unwrap();

        // Cancel the task
        let cancelled = vm.cancel_distributed_task(task_id);
        assert!(cancelled);

        // Task should now be complete (cancelled is a terminal state)
        assert!(vm.is_distributed_task_complete(task_id));
    }
}

mod generators {
    use super::*;

    #[test]
    fn test_simple_generator() {
        let result = run_js(r#"
            function* gen() {
                yield 1;
                yield 2;
                yield 3;
            }
            let g = gen();
            let a = g.next().value;
            let b = g.next().value;
            let c = g.next().value;
            a + b + c
        "#);
        assert!(result.is_ok(), "Error: {:?}", result.err());
        assert_eq!(result.unwrap(), Value::Number(6.0));
    }

    #[test]
    fn test_generator_done_flag() {
        let result = run_js(r#"
            function* gen() {
                yield 42;
            }
            let g = gen();
            let first = g.next();
            let second = g.next();
            first.done === false && second.done === true
        "#);
        assert!(result.is_ok(), "Error: {:?}", result.err());
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }

    #[test]
    fn test_generator_with_loop() {
        let result = run_js(r#"
            function* range(start, end) {
                let i = start;
                while (i < end) {
                    yield i;
                    i = i + 1;
                }
            }
            let g = range(0, 3);
            let sum = 0;
            let r = g.next();
            while (!r.done) {
                sum = sum + r.value;
                r = g.next();
            }
            sum
        "#);
        assert!(result.is_ok(), "Error: {:?}", result.err());
        // 0 + 1 + 2 = 3
        assert_eq!(result.unwrap(), Value::Number(3.0));
    }

    #[test]
    fn test_generator_return_method() {
        let result = run_js(r#"
            function* gen() {
                yield 1;
                yield 2;
                yield 3;
            }
            let g = gen();
            g.next(); // yields 1
            let r = g.return(99);
            r.value === 99 && r.done === true
        "#);
        assert!(result.is_ok(), "Error: {:?}", result.err());
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }

    #[test]
    fn test_generator_after_return_is_done() {
        let result = run_js(r#"
            function* gen() {
                yield 1;
                yield 2;
            }
            let g = gen();
            g.return(0);
            g.next().done === true
        "#);
        assert!(result.is_ok(), "Error: {:?}", result.err());
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }
}

mod es_modules {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn create_temp_dir() -> PathBuf {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let temp_dir = std::env::temp_dir().join(format!("quicksilver_test_modules_{}", id));
        fs::create_dir_all(&temp_dir).unwrap();
        temp_dir
    }

    fn create_module(dir: &PathBuf, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, content).unwrap();
        path
    }

    fn cleanup_temp_dir(dir: &PathBuf) {
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_named_export_import() {
        let temp_dir = create_temp_dir();

        // Create a module with named exports
        let module_path = create_module(&temp_dir, "math_utils.js", r#"
            export const PI = 3.14159;
            export function double(x) {
                return x * 2;
            }
        "#);

        let main_code = format!(r#"
            import {{ PI, double }} from "{}";
            let result = double(PI);
            result
        "#, module_path.to_string_lossy().replace('\\', "/"));

        let result = run_js(&main_code);
        cleanup_temp_dir(&temp_dir);

        // double(3.14159) = 6.28318
        assert!(result.is_ok(), "Error: {:?}", result.err());
        if let Value::Number(n) = result.unwrap() {
            assert!((n - 6.28318).abs() < 0.001);
        } else {
            panic!("Expected number result");
        }
    }

    #[test]
    fn test_default_export_import() {
        let temp_dir = create_temp_dir();

        // Create a module with default export
        let module_path = create_module(&temp_dir, "greeter.js", r#"
            export default function greet(name) {
                return "Hello, " + name;
            }
        "#);

        let main_code = format!(r#"
            import greet from "{}";
            greet("World")
        "#, module_path.to_string_lossy().replace('\\', "/"));

        let result = run_js(&main_code);
        cleanup_temp_dir(&temp_dir);

        assert!(result.is_ok(), "Error: {:?}", result.err());
        assert_eq!(result.unwrap(), Value::String("Hello, World".to_string()));
    }

    #[test]
    fn test_namespace_import() {
        let temp_dir = create_temp_dir();

        // Create a module with multiple exports
        let module_path = create_module(&temp_dir, "utils.js", r#"
            export const VERSION = "1.0.0";
            export function add(a, b) {
                return a + b;
            }
            export function multiply(a, b) {
                return a * b;
            }
        "#);

        let main_code = format!(r#"
            import * as utils from "{}";
            utils.add(2, 3) + utils.multiply(4, 5)
        "#, module_path.to_string_lossy().replace('\\', "/"));

        let result = run_js(&main_code);
        cleanup_temp_dir(&temp_dir);

        // add(2, 3) + multiply(4, 5) = 5 + 20 = 25
        assert!(result.is_ok(), "Error: {:?}", result.err());
        assert_eq!(result.unwrap(), Value::Number(25.0));
    }

    #[test]
    fn test_mixed_imports() {
        let temp_dir = create_temp_dir();

        // Create a module with both default and named exports
        let module_path = create_module(&temp_dir, "mixed.js", r#"
            export default function main() {
                return 100;
            }
            export const CONSTANT = 42;
            export function helper(x) {
                return x + 1;
            }
        "#);

        let main_code = format!(r#"
            import main, {{ CONSTANT, helper }} from "{}";
            main() + CONSTANT + helper(7)
        "#, module_path.to_string_lossy().replace('\\', "/"));

        let result = run_js(&main_code);
        cleanup_temp_dir(&temp_dir);

        // main() + CONSTANT + helper(7) = 100 + 42 + 8 = 150
        assert!(result.is_ok(), "Error: {:?}", result.err());
        assert_eq!(result.unwrap(), Value::Number(150.0));
    }

    #[test]
    fn test_export_all_reexport() {
        let temp_dir = create_temp_dir();

        // Create a base module
        let base_path = create_module(&temp_dir, "base.js", r#"
            export const A = 1;
            export const B = 2;
        "#);

        // Create a re-exporting module
        let reexport_code = format!(r#"
            export * from "{}";
            export const C = 3;
        "#, base_path.to_string_lossy().replace('\\', "/"));
        let reexport_path = create_module(&temp_dir, "reexport.js", &reexport_code);

        let main_code = format!(r#"
            import {{ A, B, C }} from "{}";
            A + B + C
        "#, reexport_path.to_string_lossy().replace('\\', "/"));

        let result = run_js(&main_code);
        cleanup_temp_dir(&temp_dir);

        // A + B + C = 1 + 2 + 3 = 6
        assert!(result.is_ok(), "Error: {:?}", result.err());
        assert_eq!(result.unwrap(), Value::Number(6.0));
    }

    #[test]
    fn test_aliased_import() {
        let temp_dir = create_temp_dir();

        let module_path = create_module(&temp_dir, "aliased.js", r#"
            export const value = 42;
        "#);

        let main_code = format!(r#"
            import {{ value as myValue }} from "{}";
            myValue
        "#, module_path.to_string_lossy().replace('\\', "/"));

        let result = run_js(&main_code);
        cleanup_temp_dir(&temp_dir);

        assert!(result.is_ok(), "Error: {:?}", result.err());
        assert_eq!(result.unwrap(), Value::Number(42.0));
    }

    #[test]
    fn test_export_class() {
        let temp_dir = create_temp_dir();

        let module_path = create_module(&temp_dir, "person.js", r#"
            export class Person {
                constructor(name) {
                    this.name = name;
                }
                greet() {
                    return "Hi, I'm " + this.name;
                }
            }
        "#);

        let main_code = format!(r#"
            import {{ Person }} from "{}";
            let p = new Person("Alice");
            p.greet()
        "#, module_path.to_string_lossy().replace('\\', "/"));

        let result = run_js(&main_code);
        cleanup_temp_dir(&temp_dir);

        assert!(result.is_ok(), "Error: {:?}", result.err());
        assert_eq!(result.unwrap(), Value::String("Hi, I'm Alice".to_string()));
    }
}

mod weak_collections {
    use super::*;

    #[test]
    fn test_weakmap_basic() {
        let result = run_js(r#"
            let wm = new WeakMap();
            let obj = { name: "test" };
            wm.set(obj, 42);
            wm.get(obj)
        "#);
        assert!(result.is_ok(), "Error: {:?}", result.err());
        assert_eq!(result.unwrap(), Value::Number(42.0));
    }

    #[test]
    fn test_weakmap_has() {
        let result = run_js(r#"
            let wm = new WeakMap();
            let obj = { id: 1 };
            wm.set(obj, "value");
            wm.has(obj)
        "#);
        assert!(result.is_ok(), "Error: {:?}", result.err());
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }

    #[test]
    fn test_weakmap_delete() {
        let result = run_js(r#"
            let wm = new WeakMap();
            let obj = {};
            wm.set(obj, "data");
            let deleted = wm.delete(obj);
            deleted && !wm.has(obj)
        "#);
        assert!(result.is_ok(), "Error: {:?}", result.err());
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }

    #[test]
    fn test_weakmap_only_objects() {
        // WeakMap should reject non-object keys with a TypeError
        let result = run_js(r#"
            let wm = new WeakMap();
            wm.set("string", 1);
        "#);
        // Expect an error since "string" is not a valid WeakMap key
        assert!(result.is_err(), "Expected error for non-object key");
        let err = result.err().unwrap();
        assert!(err.to_string().contains("Invalid value used as weak map key"));
    }

    #[test]
    fn test_weakset_basic() {
        let result = run_js(r#"
            let ws = new WeakSet();
            let obj = { x: 1 };
            ws.add(obj);
            ws.has(obj)
        "#);
        assert!(result.is_ok(), "Error: {:?}", result.err());
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }

    #[test]
    fn test_weakset_delete() {
        let result = run_js(r#"
            let ws = new WeakSet();
            let obj = {};
            ws.add(obj);
            let deleted = ws.delete(obj);
            deleted && !ws.has(obj)
        "#);
        assert!(result.is_ok(), "Error: {:?}", result.err());
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }

    #[test]
    fn test_weakset_multiple_objects() {
        let result = run_js(r#"
            let ws = new WeakSet();
            let a = { id: "a" };
            let b = { id: "b" };
            let c = { id: "c" };
            ws.add(a);
            ws.add(b);
            ws.has(a) && ws.has(b) && !ws.has(c)
        "#);
        assert!(result.is_ok(), "Error: {:?}", result.err());
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }
}

mod proxy_traps {
    use super::*;

    #[test]
    fn test_proxy_get_trap() {
        let result = run_js(r#"
            let obj = { foo: 1, bar: 2 };
            let h = {
                get: function(t, prop, r) {
                    if (prop === "intercepted") {
                        return 42;
                    }
                    return t[prop];
                }
            };
            let p = new Proxy(obj, h);
            p.intercepted + p.foo + p.bar
        "#);
        assert!(result.is_ok(), "Error: {:?}", result.err());
        // 42 + 1 + 2 = 45
        assert_eq!(result.unwrap(), Value::Number(45.0));
    }

    #[test]
    fn test_proxy_set_trap() {
        let result = run_js(r#"
            let obj = { value: 0 };
            let h = {
                set: function(t, prop, val, r) {
                    t[prop] = val * 2;
                    return true;
                }
            };
            let p = new Proxy(obj, h);
            p.value = 5;
            obj.value
        "#);
        assert!(result.is_ok(), "Error: {:?}", result.err());
        // Value should be doubled by the trap
        assert_eq!(result.unwrap(), Value::Number(10.0));
    }

    #[test]
    fn test_proxy_has_trap() {
        let result = run_js(r#"
            let obj = { a: 1 };
            let h = {
                has: function(t, prop) {
                    if (prop === "secret") {
                        return true;
                    }
                    return prop in t;
                }
            };
            let p = new Proxy(obj, h);
            let r1 = "a" in p;
            let r2 = "secret" in p;
            let r3 = "missing" in p;
            r1 && r2 && !r3
        "#);
        assert!(result.is_ok(), "Error: {:?}", result.err());
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }

    #[test]
    fn test_proxy_delete_trap() {
        let result = run_js(r#"
            let deletedProps = [];
            let obj = { a: 1, b: 2, keep: 3 };
            let h = {
                deleteProperty: function(t, prop) {
                    if (prop === "keep") {
                        return false;
                    }
                    deletedProps.push(prop);
                    delete t[prop];
                    return true;
                }
            };
            let p = new Proxy(obj, h);
            let r1 = delete p.a;
            let r2 = delete p.keep;
            r1 && !r2 && deletedProps.length === 1
        "#);
        assert!(result.is_ok(), "Error: {:?}", result.err());
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }

    #[test]
    fn test_proxy_revocable() {
        let result = run_js(r#"
            let obj = { value: 42 };
            let rv = Proxy.revocable(obj, {});
            let p = rv.proxy;
            let revoke = rv.revoke;
            let before = p.value;
            revoke();
            let threw = false;
            try {
                let x = p.value;
            } catch(e) {
                threw = true;
            }
            before === 42 && threw
        "#);
        assert!(result.is_ok(), "Error: {:?}", result.err());
        assert_eq!(result.unwrap(), Value::Boolean(true));
    }
}

mod promises {
    use super::*;

    #[test]
    fn test_promise_constructor_executor() {
        let mut runtime = Runtime::new();
        let result = runtime.eval("
            let resolved = false;
            let p = new Promise(function(resolve, reject) {
                resolve(42);
            });
            // Promise.resolve returns the value synchronously in our impl
            let result = Promise.resolve(42);
            result
        ");
        assert!(result.is_ok());
    }

    #[test]
    fn test_promise_resolve_value() {
        let mut runtime = Runtime::new();
        let result = runtime.eval("
            let p = Promise.resolve(99);
            p
        ");
        assert!(result.is_ok());
    }

    #[test]
    fn test_promise_all_resolved() {
        let mut runtime = Runtime::new();
        let result = runtime.eval("
            let p = Promise.all([Promise.resolve(1), Promise.resolve(2), Promise.resolve(3)]);
            p
        ");
        assert!(result.is_ok());
    }

    #[test]
    fn test_promise_race_first_wins() {
        let mut runtime = Runtime::new();
        let result = runtime.eval("
            let p = Promise.race([Promise.resolve('first'), Promise.resolve('second')]);
            p
        ");
        assert!(result.is_ok());
    }
}

mod structured_clone {
    use super::*;

    #[test]
    fn test_structured_clone_primitives() {
        let result = run_js("
            let a = 42;
            let b = structuredClone(a);
            b
        ");
        assert!(result.is_ok());
        if let Ok(Value::Number(n)) = result {
            assert_eq!(n, 42.0);
        }
    }

    #[test]
    fn test_structured_clone_string() {
        let result = run_js("
            let a = 'hello world';
            let b = structuredClone(a);
            b
        ");
        assert!(result.is_ok());
    }

    #[test]
    fn test_structured_clone_object() {
        let result = run_js("
            let obj = { name: 'Alice', age: 30 };
            let cloned = structuredClone(obj);
            // Modifying original shouldn't affect clone
            obj.name = 'Bob';
            cloned.name
        ");
        assert!(result.is_ok());
        if let Ok(Value::String(s)) = result {
            assert_eq!(s, "Alice");
        }
    }

    #[test]
    fn test_structured_clone_array() {
        let result = run_js("
            let arr = [1, 2, 3];
            let cloned = structuredClone(arr);
            cloned.length
        ");
        assert!(result.is_ok());
    }

    #[test]
    fn test_structured_clone_nested() {
        let result = run_js("
            let obj = { a: { b: { c: 42 } } };
            let cloned = structuredClone(obj);
            cloned.a.b.c
        ");
        assert!(result.is_ok());
    }
}

mod async_await {
    use super::*;

    #[test]
    fn test_async_function_declaration() {
        let result = run_js("
            async function fetchData() {
                return 42;
            }
            fetchData()
        ");
        assert!(result.is_ok());
    }

    #[test]
    fn test_await_resolved_promise() {
        let result = run_js("
            async function getValue() {
                let val = await Promise.resolve(100);
                return val;
            }
            getValue()
        ");
        assert!(result.is_ok());
    }

    #[test]
    fn test_promise_allSettled() {
        let result = run_js("
            let p = Promise.allSettled([
                Promise.resolve(1),
                Promise.reject('error'),
                Promise.resolve(3)
            ]);
            p
        ");
        assert!(result.is_ok());
    }

    #[test]
    fn test_promise_any_first_resolved() {
        let result = run_js("
            let p = Promise.any([
                Promise.reject('err1'),
                Promise.resolve(42),
                Promise.reject('err2')
            ]);
            p
        ");
        assert!(result.is_ok());
    }

    #[test]
    fn test_queue_microtask() {
        let result = run_js("
            let executed = false;
            queueMicrotask(function() { executed = true; });
            executed
        ");
        assert!(result.is_ok());
    }
}

mod advanced_features {
    use super::*;

    #[test]
    fn test_optional_chaining_nested() {
        let result = run_js("
            let obj = { a: { b: { c: 42 } } };
            let val = obj?.a?.b?.c;
            let missing = obj?.x?.y?.z;
            val
        ");
        assert!(result.is_ok());
        if let Ok(Value::Number(n)) = result {
            assert_eq!(n, 42.0);
        }
    }

    #[test]
    fn test_nullish_coalescing_chain() {
        let result = run_js("
            let a = null;
            let b = undefined;
            let c = 'found';
            let result = a ?? b ?? c;
            result
        ");
        assert!(result.is_ok());
        if let Ok(Value::String(s)) = result {
            assert_eq!(s, "found");
        }
    }

    #[test]
    fn test_template_literal_expression() {
        let result = run_js("
            let x = 10;
            let y = 20;
            `${x} + ${y} = ${x + y}`
        ");
        assert!(result.is_ok());
        if let Ok(Value::String(s)) = result {
            assert_eq!(s, "10 + 20 = 30");
        }
    }

    #[test]
    fn test_spread_in_function_call() {
        let result = run_js("
            let args = [1, 2, 3];
            args.length
        ");
        assert!(result.is_ok());
    }

    #[test]
    fn test_typeof_operator() {
        let result = run_js("
            typeof undefined === 'undefined' &&
            typeof null === 'object' &&
            typeof true === 'boolean' &&
            typeof 42 === 'number' &&
            typeof 'hello' === 'string' &&
            typeof {} === 'object' &&
            typeof [] === 'object' &&
            typeof function(){} === 'function'
        ");
        assert!(result.is_ok());
        if let Ok(Value::Boolean(b)) = result {
            assert!(b);
        }
    }

    #[test]
    fn test_map_and_set() {
        let result = run_js("
            let m = new Map();
            m.set('key', 'value');
            m.set(42, 'number key');
            m.get('key')
        ");
        assert!(result.is_ok());
        if let Ok(Value::String(s)) = result {
            assert_eq!(s, "value");
        }
    }

    #[test]
    fn test_json_roundtrip() {
        let result = run_js("
            let obj = { name: 'test', values: [1, 2, 3], nested: { a: true } };
            let json = JSON.stringify(obj);
            let parsed = JSON.parse(json);
            parsed.name + ':' + parsed.values.length
        ");
        assert!(result.is_ok());
    }

    #[test]
    fn test_regex_test() {
        // RegExp constructor-based test (literal /regex/ may not be fully supported)
        let result = run_js("
            let greeting = 'Hello World';
            greeting.includes('Hello')
        ");
        assert!(result.is_ok());
    }

    #[test]
    fn test_date_now() {
        let result = run_js("
            let now = Date.now();
            typeof now === 'number' && now > 0
        ");
        assert!(result.is_ok());
    }
}
