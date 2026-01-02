//! Integration tests for Quicksilver JavaScript runtime
//!
//! These tests verify that the major features work correctly together.

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
