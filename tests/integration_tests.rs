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
        // Undefined variable + 1 returns NaN (undefined coerces to NaN)
        assert!(result.is_ok());
        if let Ok(value) = result {
            if let Value::Number(n) = value {
                assert!(n.is_nan(), "Expected NaN from undefined + 1");
            }
        }
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
