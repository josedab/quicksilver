//! Integration tests for Quicksilver JavaScript runtime

mod common;
use common::run_js;
use quicksilver::Value;

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

