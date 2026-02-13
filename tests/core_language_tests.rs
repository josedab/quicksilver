//! Integration tests for Quicksilver JavaScript runtime

mod common;
use common::run_js;
use quicksilver::Value;

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

