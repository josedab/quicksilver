//! Integration tests for Quicksilver JavaScript runtime

mod common;
use common::run_js;
use quicksilver::Value;

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

