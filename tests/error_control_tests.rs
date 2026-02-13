//! Integration tests for Quicksilver JavaScript runtime

mod common;
use common::run_js;
use quicksilver::Value;

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

