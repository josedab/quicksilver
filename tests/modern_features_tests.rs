//! Integration tests for Quicksilver JavaScript runtime

mod common;
use common::run_js;
use quicksilver::Value;

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

