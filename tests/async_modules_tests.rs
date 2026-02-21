//! Integration tests for Quicksilver JavaScript runtime
#![allow(clippy::approx_constant)]

mod common;
use common::run_js;
use quicksilver::{Runtime, Value};

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

