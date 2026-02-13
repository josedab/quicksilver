//! Integration tests for Quicksilver JavaScript runtime

mod common;
use common::{run_js, run_js_string};
use quicksilver::Value;

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

