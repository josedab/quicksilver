//! Built-in functions and objects
//!
//! This module provides the standard JavaScript built-in functions and objects
//! like console, Math, JSON, etc.

use super::value::{ObjectKind, TypedArrayKind, Value};
use super::vm::VM;
use crate::concurrency::Channel;
use crate::error::Result;
use crate::security::{Capability, HostPattern, PathPattern, PermissionState, Sandbox};
use rustc_hash::FxHashMap as HashMap;
use std::cell::RefCell;
use std::rc::Rc;

/// Register all built-in globals
pub fn register_globals(vm: &mut VM) {
    register_console(vm);
    register_math(vm);
    register_json(vm);
    register_date(vm);
    register_map(vm);
    register_set(vm);
    register_weakmap(vm);
    register_weakset(vm);
    register_regexp(vm);
    register_proxy(vm);
    register_reflect(vm);
    register_promise(vm);
    register_timers(vm);
    register_fetch(vm);
    register_deno(vm);
    register_typed_arrays(vm);
    register_url(vm);
    register_global_functions(vm);
    register_error(vm);
    register_concurrency(vm);
    register_weakref(vm);
    register_performance(vm);
    register_encoding(vm);
    register_crypto(vm);
}

/// Format a value for console output (with BigInt 'n' suffix, etc.)
fn format_console_value(value: &Value) -> String {
    match value {
        Value::BigInt(n) => format!("{}n", n),
        _ => value.to_js_string(),
    }
}

/// Register console object
fn register_console(vm: &mut VM) {
    use std::cell::RefCell;
    use std::collections::HashMap as StdHashMap;
    use std::time::Instant;

    // Thread-local state for console methods
    thread_local! {
        static GROUP_DEPTH: RefCell<usize> = const { RefCell::new(0) };
        static TIMERS: RefCell<StdHashMap<String, Instant>> = RefCell::new(StdHashMap::default());
        static COUNTERS: RefCell<StdHashMap<String, usize>> = RefCell::new(StdHashMap::default());
    }

    fn get_indent() -> String {
        GROUP_DEPTH.with(|d| "  ".repeat(*d.borrow()))
    }

    // console.log
    vm.register_native("console_log", |args| {
        let indent = get_indent();
        let output: Vec<String> = args.iter().map(|v| format_console_value(v)).collect();
        println!("{}{}", indent, output.join(" "));
        Ok(Value::Undefined)
    });

    // console.warn
    vm.register_native("console_warn", |args| {
        let indent = get_indent();
        let output: Vec<String> = args.iter().map(|v| v.to_js_string()).collect();
        eprintln!("{}[WARN] {}", indent, output.join(" "));
        Ok(Value::Undefined)
    });

    // console.error
    vm.register_native("console_error", |args| {
        let indent = get_indent();
        let output: Vec<String> = args.iter().map(|v| v.to_js_string()).collect();
        eprintln!("{}[ERROR] {}", indent, output.join(" "));
        Ok(Value::Undefined)
    });

    // console.info (alias for log)
    vm.register_native("console_info", |args| {
        let indent = get_indent();
        let output: Vec<String> = args.iter().map(|v| v.to_js_string()).collect();
        println!("{}{}", indent, output.join(" "));
        Ok(Value::Undefined)
    });

    // console.debug
    vm.register_native("console_debug", |args| {
        let indent = get_indent();
        let output: Vec<String> = args.iter().map(|v| v.to_js_string()).collect();
        println!("{}[DEBUG] {}", indent, output.join(" "));
        Ok(Value::Undefined)
    });

    // console.table
    vm.register_native("console_table", |args| {
        let indent = get_indent();
        if let Some(data) = args.first() {
            match data {
                Value::Object(obj) => {
                    let obj_ref = obj.borrow();
                    match &obj_ref.kind {
                        super::value::ObjectKind::Array(arr) => {
                            // Print array as table
                            println!("{}┌─────────┬───────────────────────────────┐", indent);
                            println!("{}│ (index) │ Values                        │", indent);
                            println!("{}├─────────┼───────────────────────────────┤", indent);
                            for (i, val) in arr.iter().enumerate() {
                                println!("{}│ {:>7} │ {:<29} │", indent, i, val.to_js_string());
                            }
                            println!("{}└─────────┴───────────────────────────────┘", indent);
                        }
                        _ => {
                            // Print object as table
                            println!("{}┌───────────────────┬───────────────────────────────┐", indent);
                            println!("{}│ (key)             │ Values                        │", indent);
                            println!("{}├───────────────────┼───────────────────────────────┤", indent);
                            for (key, val) in &obj_ref.properties {
                                println!("{}│ {:>17} │ {:<29} │", indent, key, val.to_js_string());
                            }
                            println!("{}└───────────────────┴───────────────────────────────┘", indent);
                        }
                    }
                }
                _ => {
                    println!("{}{:?}", indent, data);
                }
            }
        }
        Ok(Value::Undefined)
    });

    // console.group
    vm.register_native("console_group", |args| {
        let indent = get_indent();
        let label = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        if !label.is_empty() {
            println!("{}▼ {}", indent, label);
        }
        GROUP_DEPTH.with(|d| *d.borrow_mut() += 1);
        Ok(Value::Undefined)
    });

    // console.groupCollapsed (same as group for CLI)
    vm.register_native("console_groupCollapsed", |args| {
        let indent = get_indent();
        let label = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        if !label.is_empty() {
            println!("{}▶ {}", indent, label);
        }
        GROUP_DEPTH.with(|d| *d.borrow_mut() += 1);
        Ok(Value::Undefined)
    });

    // console.groupEnd
    vm.register_native("console_groupEnd", |_args| {
        GROUP_DEPTH.with(|d| {
            let mut depth = d.borrow_mut();
            if *depth > 0 {
                *depth -= 1;
            }
        });
        Ok(Value::Undefined)
    });

    // console.time
    vm.register_native("console_time", |args| {
        let label = args.first().map(|v| v.to_js_string()).unwrap_or_else(|| "default".to_string());
        TIMERS.with(|t| t.borrow_mut().insert(label, Instant::now()));
        Ok(Value::Undefined)
    });

    // console.timeEnd
    vm.register_native("console_timeEnd", |args| {
        let indent = get_indent();
        let label = args.first().map(|v| v.to_js_string()).unwrap_or_else(|| "default".to_string());
        TIMERS.with(|t| {
            if let Some(start) = t.borrow_mut().remove(&label) {
                let elapsed = start.elapsed();
                println!("{}{}: {:.3}ms", indent, label, elapsed.as_secs_f64() * 1000.0);
            } else {
                println!("{}Timer '{}' does not exist", indent, label);
            }
        });
        Ok(Value::Undefined)
    });

    // console.timeLog
    vm.register_native("console_timeLog", |args| {
        let indent = get_indent();
        let label = args.first().map(|v| v.to_js_string()).unwrap_or_else(|| "default".to_string());
        TIMERS.with(|t| {
            if let Some(start) = t.borrow().get(&label) {
                let elapsed = start.elapsed();
                let extra: Vec<String> = args.iter().skip(1).map(|v| v.to_js_string()).collect();
                if extra.is_empty() {
                    println!("{}{}: {:.3}ms", indent, label, elapsed.as_secs_f64() * 1000.0);
                } else {
                    println!("{}{}: {:.3}ms {}", indent, label, elapsed.as_secs_f64() * 1000.0, extra.join(" "));
                }
            } else {
                println!("{}Timer '{}' does not exist", indent, label);
            }
        });
        Ok(Value::Undefined)
    });

    // console.count
    vm.register_native("console_count", |args| {
        let indent = get_indent();
        let label = args.first().map(|v| v.to_js_string()).unwrap_or_else(|| "default".to_string());
        COUNTERS.with(|c| {
            let mut counters = c.borrow_mut();
            let count = counters.entry(label.clone()).or_insert(0);
            *count += 1;
            println!("{}{}: {}", indent, label, count);
        });
        Ok(Value::Undefined)
    });

    // console.countReset
    vm.register_native("console_countReset", |args| {
        let label = args.first().map(|v| v.to_js_string()).unwrap_or_else(|| "default".to_string());
        COUNTERS.with(|c| c.borrow_mut().remove(&label));
        Ok(Value::Undefined)
    });

    // console.assert
    vm.register_native("console_assert", |args| {
        let indent = get_indent();
        let condition = args.first().map(|v| v.to_boolean()).unwrap_or(false);
        if !condition {
            let msg: Vec<String> = args.iter().skip(1).map(|v| v.to_js_string()).collect();
            if msg.is_empty() {
                eprintln!("{}Assertion failed", indent);
            } else {
                eprintln!("{}Assertion failed: {}", indent, msg.join(" "));
            }
        }
        Ok(Value::Undefined)
    });

    // console.clear
    vm.register_native("console_clear", |_args| {
        // ANSI escape code to clear screen
        print!("\x1B[2J\x1B[1;1H");
        Ok(Value::Undefined)
    });

    // console.dir (similar to log but with object inspection)
    vm.register_native("console_dir", |args| {
        let indent = get_indent();
        if let Some(val) = args.first() {
            println!("{}{:?}", indent, val);
        }
        Ok(Value::Undefined)
    });

    // console.trace
    vm.register_native("console_trace", |args| {
        let indent = get_indent();
        let msg: Vec<String> = args.iter().map(|v| v.to_js_string()).collect();
        if !msg.is_empty() {
            eprintln!("{}Trace: {}", indent, msg.join(" "));
        } else {
            eprintln!("{}Trace", indent);
        }
        eprintln!("{}  (stack trace not available)", indent);
        Ok(Value::Undefined)
    });

    // Create console object
    let console = Value::new_object();
    console.set_property("log", vm.get_global("console_log").unwrap_or(Value::Undefined));
    console.set_property("warn", vm.get_global("console_warn").unwrap_or(Value::Undefined));
    console.set_property("error", vm.get_global("console_error").unwrap_or(Value::Undefined));
    console.set_property("info", vm.get_global("console_info").unwrap_or(Value::Undefined));
    console.set_property("debug", vm.get_global("console_debug").unwrap_or(Value::Undefined));
    console.set_property("table", vm.get_global("console_table").unwrap_or(Value::Undefined));
    console.set_property("group", vm.get_global("console_group").unwrap_or(Value::Undefined));
    console.set_property("groupCollapsed", vm.get_global("console_groupCollapsed").unwrap_or(Value::Undefined));
    console.set_property("groupEnd", vm.get_global("console_groupEnd").unwrap_or(Value::Undefined));
    console.set_property("time", vm.get_global("console_time").unwrap_or(Value::Undefined));
    console.set_property("timeEnd", vm.get_global("console_timeEnd").unwrap_or(Value::Undefined));
    console.set_property("timeLog", vm.get_global("console_timeLog").unwrap_or(Value::Undefined));
    console.set_property("count", vm.get_global("console_count").unwrap_or(Value::Undefined));
    console.set_property("countReset", vm.get_global("console_countReset").unwrap_or(Value::Undefined));
    console.set_property("assert", vm.get_global("console_assert").unwrap_or(Value::Undefined));
    console.set_property("clear", vm.get_global("console_clear").unwrap_or(Value::Undefined));
    console.set_property("dir", vm.get_global("console_dir").unwrap_or(Value::Undefined));
    console.set_property("trace", vm.get_global("console_trace").unwrap_or(Value::Undefined));
    vm.set_global("console", console);
}

/// Register Math object
fn register_math(vm: &mut VM) {
    let math = Value::new_object();

    // Constants
    math.set_property("PI", Value::Number(std::f64::consts::PI));
    math.set_property("E", Value::Number(std::f64::consts::E));
    math.set_property("LN2", Value::Number(std::f64::consts::LN_2));
    math.set_property("LN10", Value::Number(std::f64::consts::LN_10));
    math.set_property("LOG2E", Value::Number(std::f64::consts::LOG2_E));
    math.set_property("LOG10E", Value::Number(std::f64::consts::LOG10_E));
    math.set_property("SQRT2", Value::Number(std::f64::consts::SQRT_2));
    math.set_property("SQRT1_2", Value::Number(std::f64::consts::FRAC_1_SQRT_2));

    // Math.abs
    vm.register_native("Math_abs", |args| {
        let n = args.first().map(|v| v.to_number()).unwrap_or(f64::NAN);
        Ok(Value::Number(n.abs()))
    });
    math.set_property("abs", vm.get_global("Math_abs").unwrap_or(Value::Undefined));

    // Math.floor
    vm.register_native("Math_floor", |args| {
        let n = args.first().map(|v| v.to_number()).unwrap_or(f64::NAN);
        Ok(Value::Number(n.floor()))
    });
    math.set_property(
        "floor",
        vm.get_global("Math_floor").unwrap_or(Value::Undefined),
    );

    // Math.ceil
    vm.register_native("Math_ceil", |args| {
        let n = args.first().map(|v| v.to_number()).unwrap_or(f64::NAN);
        Ok(Value::Number(n.ceil()))
    });
    math.set_property(
        "ceil",
        vm.get_global("Math_ceil").unwrap_or(Value::Undefined),
    );

    // Math.round
    vm.register_native("Math_round", |args| {
        let n = args.first().map(|v| v.to_number()).unwrap_or(f64::NAN);
        Ok(Value::Number(n.round()))
    });
    math.set_property(
        "round",
        vm.get_global("Math_round").unwrap_or(Value::Undefined),
    );

    // Math.sqrt
    vm.register_native("Math_sqrt", |args| {
        let n = args.first().map(|v| v.to_number()).unwrap_or(f64::NAN);
        Ok(Value::Number(n.sqrt()))
    });
    math.set_property(
        "sqrt",
        vm.get_global("Math_sqrt").unwrap_or(Value::Undefined),
    );

    // Math.pow
    vm.register_native("Math_pow", |args| {
        let base = args.first().map(|v| v.to_number()).unwrap_or(f64::NAN);
        let exp = args.get(1).map(|v| v.to_number()).unwrap_or(f64::NAN);
        Ok(Value::Number(base.powf(exp)))
    });
    math.set_property("pow", vm.get_global("Math_pow").unwrap_or(Value::Undefined));

    // Math.min
    vm.register_native("Math_min", |args| {
        if args.is_empty() {
            return Ok(Value::Number(f64::INFINITY));
        }
        let mut min = f64::INFINITY;
        for arg in args {
            let n = arg.to_number();
            if n.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            if n < min {
                min = n;
            }
        }
        Ok(Value::Number(min))
    });
    math.set_property("min", vm.get_global("Math_min").unwrap_or(Value::Undefined));

    // Math.max
    vm.register_native("Math_max", |args| {
        if args.is_empty() {
            return Ok(Value::Number(f64::NEG_INFINITY));
        }
        let mut max = f64::NEG_INFINITY;
        for arg in args {
            let n = arg.to_number();
            if n.is_nan() {
                return Ok(Value::Number(f64::NAN));
            }
            if n > max {
                max = n;
            }
        }
        Ok(Value::Number(max))
    });
    math.set_property("max", vm.get_global("Math_max").unwrap_or(Value::Undefined));

    // Math.random
    vm.register_native("Math_random", |_args| {
        Ok(Value::Number(rand::random::<f64>()))
    });
    math.set_property(
        "random",
        vm.get_global("Math_random").unwrap_or(Value::Undefined),
    );

    // Math.sin
    vm.register_native("Math_sin", |args| {
        let n = args.first().map(|v| v.to_number()).unwrap_or(f64::NAN);
        Ok(Value::Number(n.sin()))
    });
    math.set_property("sin", vm.get_global("Math_sin").unwrap_or(Value::Undefined));

    // Math.cos
    vm.register_native("Math_cos", |args| {
        let n = args.first().map(|v| v.to_number()).unwrap_or(f64::NAN);
        Ok(Value::Number(n.cos()))
    });
    math.set_property("cos", vm.get_global("Math_cos").unwrap_or(Value::Undefined));

    // Math.tan
    vm.register_native("Math_tan", |args| {
        let n = args.first().map(|v| v.to_number()).unwrap_or(f64::NAN);
        Ok(Value::Number(n.tan()))
    });
    math.set_property("tan", vm.get_global("Math_tan").unwrap_or(Value::Undefined));

    // Math.log
    vm.register_native("Math_log", |args| {
        let n = args.first().map(|v| v.to_number()).unwrap_or(f64::NAN);
        Ok(Value::Number(n.ln()))
    });
    math.set_property("log", vm.get_global("Math_log").unwrap_or(Value::Undefined));

    // Math.exp
    vm.register_native("Math_exp", |args| {
        let n = args.first().map(|v| v.to_number()).unwrap_or(f64::NAN);
        Ok(Value::Number(n.exp()))
    });
    math.set_property("exp", vm.get_global("Math_exp").unwrap_or(Value::Undefined));

    // Math.trunc
    vm.register_native("Math_trunc", |args| {
        let n = args.first().map(|v| v.to_number()).unwrap_or(f64::NAN);
        Ok(Value::Number(n.trunc()))
    });
    math.set_property(
        "trunc",
        vm.get_global("Math_trunc").unwrap_or(Value::Undefined),
    );

    // Math.sign
    vm.register_native("Math_sign", |args| {
        let n = args.first().map(|v| v.to_number()).unwrap_or(f64::NAN);
        Ok(Value::Number(n.signum()))
    });
    math.set_property(
        "sign",
        vm.get_global("Math_sign").unwrap_or(Value::Undefined),
    );

    vm.set_global("Math", math);
}

/// Register JSON object
fn register_json(vm: &mut VM) {
    let json = Value::new_object();

    // JSON.stringify
    vm.register_native("JSON_stringify", |args| {
        let value = args.first().cloned().unwrap_or(Value::Undefined);
        Ok(Value::String(stringify_value(&value)))
    });
    json.set_property(
        "stringify",
        vm.get_global("JSON_stringify").unwrap_or(Value::Undefined),
    );

    // JSON.parse
    vm.register_native("JSON_parse", |args| {
        let text = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        parse_json(&text)
    });
    json.set_property(
        "parse",
        vm.get_global("JSON_parse").unwrap_or(Value::Undefined),
    );

    vm.set_global("JSON", json);
}

/// Register Date object
fn register_date(vm: &mut VM) {
    use std::time::{SystemTime, UNIX_EPOCH};

    // Date.now() - returns current timestamp in milliseconds
    vm.register_native("Date_now", |_args| {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as f64)
            .unwrap_or(0.0);
        Ok(Value::Number(now))
    });

    // Date constructor - used when calling new Date()
    vm.register_native("Date_constructor", |args| {
        let timestamp = if args.is_empty() {
            // new Date() - current time
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as f64)
                .unwrap_or(0.0)
        } else if args.len() == 1 {
            // new Date(value) - parse timestamp or string
            match &args[0] {
                Value::Number(n) => *n,
                Value::String(s) => {
                    // Try to parse ISO date string
                    parse_date_string(s).unwrap_or(f64::NAN)
                }
                _ => args[0].to_number(),
            }
        } else {
            // new Date(year, month, day, hours, minutes, seconds, ms)
            let year = args.first().map(|v| v.to_number()).unwrap_or(0.0) as i32;
            let month = args.get(1).map(|v| v.to_number()).unwrap_or(0.0) as u32;
            let day = args.get(2).map(|v| v.to_number()).unwrap_or(1.0) as u32;
            let hours = args.get(3).map(|v| v.to_number()).unwrap_or(0.0) as u32;
            let minutes = args.get(4).map(|v| v.to_number()).unwrap_or(0.0) as u32;
            let seconds = args.get(5).map(|v| v.to_number()).unwrap_or(0.0) as u32;
            let ms = args.get(6).map(|v| v.to_number()).unwrap_or(0.0);

            // Convert to timestamp
            ymd_to_timestamp(year, month + 1, day, hours, minutes, seconds, ms)
        };

        Ok(create_date_object(timestamp))
    });

    // Date.parse
    vm.register_native("Date_parse", |args| {
        let text = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        let ts = parse_date_string(&text).unwrap_or(f64::NAN);
        Ok(Value::Number(ts))
    });

    // Date.UTC
    vm.register_native("Date_UTC", |args| {
        let year = args.first().map(|v| v.to_number()).unwrap_or(0.0) as i32;
        let month = args.get(1).map(|v| v.to_number()).unwrap_or(0.0) as u32;
        let day = args.get(2).map(|v| v.to_number()).unwrap_or(1.0) as u32;
        let hours = args.get(3).map(|v| v.to_number()).unwrap_or(0.0) as u32;
        let minutes = args.get(4).map(|v| v.to_number()).unwrap_or(0.0) as u32;
        let seconds = args.get(5).map(|v| v.to_number()).unwrap_or(0.0) as u32;
        let ms = args.get(6).map(|v| v.to_number()).unwrap_or(0.0);

        let ts = ymd_to_timestamp(year, month + 1, day, hours, minutes, seconds, ms);
        Ok(Value::Number(ts))
    });

    // Create Date object (acts as constructor)
    let date = Value::new_object();
    date.set_property("now", vm.get_global("Date_now").unwrap_or(Value::Undefined));
    date.set_property("parse", vm.get_global("Date_parse").unwrap_or(Value::Undefined));
    date.set_property("UTC", vm.get_global("Date_UTC").unwrap_or(Value::Undefined));

    vm.set_global("Date", date);
    vm.set_global("__Date_constructor", vm.get_global("Date_constructor").unwrap_or(Value::Undefined));
}

/// Create a Date object with methods
fn create_date_object(timestamp: f64) -> Value {
    use std::cell::RefCell;
    use std::rc::Rc;

    let obj = Rc::new(RefCell::new(super::value::Object {
        kind: super::value::ObjectKind::Date(timestamp),
        properties: std::collections::HashMap::default(),
        private_fields: HashMap::default(),
        prototype: None,
    }));

    let value = Value::Object(obj);

    // Add instance methods
    let ts = timestamp;

    // getTime
    value.set_property("getTime", create_date_method(ts, |ts| Value::Number(ts)));

    // getFullYear
    value.set_property("getFullYear", create_date_method(ts, |ts| {
        let (year, _, _) = timestamp_to_ymd(ts);
        Value::Number(year as f64)
    }));

    // getMonth (0-11)
    value.set_property("getMonth", create_date_method(ts, |ts| {
        let (_, month, _) = timestamp_to_ymd(ts);
        Value::Number((month - 1) as f64)
    }));

    // getDate (1-31)
    value.set_property("getDate", create_date_method(ts, |ts| {
        let (_, _, day) = timestamp_to_ymd(ts);
        Value::Number(day as f64)
    }));

    // getDay (0-6, Sunday = 0)
    value.set_property("getDay", create_date_method(ts, |ts| {
        let days_since_epoch = (ts / 86400000.0) as i64;
        // January 1, 1970 was a Thursday (4)
        let day = ((days_since_epoch + 4) % 7 + 7) % 7;
        Value::Number(day as f64)
    }));

    // getHours
    value.set_property("getHours", create_date_method(ts, |ts| {
        let secs = (ts / 1000.0) as i64;
        let hours = (secs % 86400) / 3600;
        Value::Number(hours as f64)
    }));

    // getMinutes
    value.set_property("getMinutes", create_date_method(ts, |ts| {
        let secs = (ts / 1000.0) as i64;
        let minutes = (secs % 3600) / 60;
        Value::Number(minutes as f64)
    }));

    // getSeconds
    value.set_property("getSeconds", create_date_method(ts, |ts| {
        let secs = (ts / 1000.0) as i64;
        let seconds = secs % 60;
        Value::Number(seconds as f64)
    }));

    // getMilliseconds
    value.set_property("getMilliseconds", create_date_method(ts, |ts| {
        let ms = ts % 1000.0;
        Value::Number(ms)
    }));

    // toISOString
    value.set_property("toISOString", create_date_method(ts, |ts| {
        let (year, month, day) = timestamp_to_ymd(ts);
        let secs = (ts / 1000.0) as i64;
        let hours = (secs % 86400) / 3600;
        let minutes = (secs % 3600) / 60;
        let seconds = secs % 60;
        let ms = (ts % 1000.0) as i32;
        Value::String(format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
            year, month, day, hours, minutes, seconds, ms))
    }));

    // toString
    value.set_property("toString", create_date_method(ts, |ts| {
        let (year, month, day) = timestamp_to_ymd(ts);
        let secs = (ts / 1000.0) as i64;
        let hours = (secs % 86400) / 3600;
        let minutes = (secs % 3600) / 60;
        let seconds = secs % 60;
        let month_name = match month {
            1 => "Jan", 2 => "Feb", 3 => "Mar", 4 => "Apr",
            5 => "May", 6 => "Jun", 7 => "Jul", 8 => "Aug",
            9 => "Sep", 10 => "Oct", 11 => "Nov", 12 => "Dec",
            _ => "???"
        };
        let days_since_epoch = (ts / 86400000.0) as i64;
        let day_of_week = ((days_since_epoch + 4) % 7 + 7) % 7;
        let day_name = match day_of_week {
            0 => "Sun", 1 => "Mon", 2 => "Tue", 3 => "Wed",
            4 => "Thu", 5 => "Fri", 6 => "Sat", _ => "???"
        };
        Value::String(format!("{} {} {:02} {} {:02}:{:02}:{:02} GMT+0000",
            day_name, month_name, day, year, hours, minutes, seconds))
    }));

    // valueOf (same as getTime)
    value.set_property("valueOf", create_date_method(ts, |ts| Value::Number(ts)));

    value
}

/// Create a date method that captures the timestamp
fn create_date_method<F>(timestamp: f64, f: F) -> Value
where
    F: Fn(f64) -> Value + 'static,
{
    use std::cell::RefCell;
    use std::rc::Rc;

    let obj = Rc::new(RefCell::new(super::value::Object {
        kind: super::value::ObjectKind::NativeFunction {
            name: "date_method".to_string(),
            func: Rc::new(move |_args| Ok(f(timestamp))),
        },
        properties: std::collections::HashMap::default(),
        private_fields: HashMap::default(),
        prototype: None,
    }));
    Value::Object(obj)
}

/// Convert year/month/day to timestamp
fn ymd_to_timestamp(year: i32, month: u32, day: u32, hours: u32, minutes: u32, seconds: u32, ms: f64) -> f64 {
    // Adjust for years < 100
    let year = if year >= 0 && year <= 99 { year + 1900 } else { year };

    // Days from year 0 to this year
    let y = year - if month <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = month as i32;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day as i32 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;

    let total_secs = days as i64 * 86400 + hours as i64 * 3600 + minutes as i64 * 60 + seconds as i64;
    total_secs as f64 * 1000.0 + ms
}

/// Convert timestamp to year/month/day
fn timestamp_to_ymd(ts: f64) -> (i32, u32, u32) {
    let days = (ts / 86400000.0).floor() as i64;
    let remaining_days = days + 719468;

    let era = if remaining_days >= 0 { remaining_days } else { remaining_days - 146096 } / 146097;
    let doe = remaining_days - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };

    (year as i32, m as u32, d as u32)
}

/// Parse a date string to timestamp
fn parse_date_string(s: &str) -> Option<f64> {
    // Try ISO format: YYYY-MM-DDTHH:MM:SS.sssZ
    let s = s.trim();
    if s.len() >= 10 {
        let parts: Vec<&str> = s.split('T').collect();
        let date_parts: Vec<&str> = parts[0].split('-').collect();
        if date_parts.len() >= 3 {
            let year: i32 = date_parts[0].parse().ok()?;
            let month: u32 = date_parts[1].parse().ok()?;
            let day: u32 = date_parts[2].parse().ok()?;

            let (hours, minutes, seconds, ms) = if parts.len() > 1 {
                let time_str = parts[1].trim_end_matches('Z');
                let time_parts: Vec<&str> = time_str.split(':').collect();
                let h: u32 = time_parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
                let m: u32 = time_parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
                let (secs, millis) = if let Some(sec_str) = time_parts.get(2) {
                    let sec_parts: Vec<&str> = sec_str.split('.').collect();
                    let s: u32 = sec_parts[0].parse().ok().unwrap_or(0);
                    let ms: f64 = sec_parts.get(1).and_then(|ms| ms.parse().ok()).unwrap_or(0.0);
                    (s, ms)
                } else {
                    (0, 0.0)
                };
                (h, m, secs, millis)
            } else {
                (0, 0, 0, 0.0)
            };

            return Some(ymd_to_timestamp(year, month, day, hours, minutes, seconds, ms));
        }
    }
    None
}

/// Register Map object
fn register_map(vm: &mut VM) {
    // Map constructor
    vm.register_native("Map_constructor", |args| {
        let entries: Vec<(Value, Value)> = if let Some(Value::Object(obj)) = args.first() {
            let obj = obj.borrow();
            if let super::value::ObjectKind::Array(arr) = &obj.kind {
                arr.iter()
                    .filter_map(|item| {
                        if let Value::Object(pair) = item {
                            let pair = pair.borrow();
                            if let super::value::ObjectKind::Array(kv) = &pair.kind {
                                if kv.len() >= 2 {
                                    return Some((kv[0].clone(), kv[1].clone()));
                                }
                            }
                        }
                        None
                    })
                    .collect()
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        Ok(create_map_object(entries))
    });

    let map = Value::new_object();
    vm.set_global("Map", map);
    vm.set_global("__Map_constructor", vm.get_global("Map_constructor").unwrap_or(Value::Undefined));
}

/// Create a Map object with methods
fn create_map_object(initial_entries: Vec<(Value, Value)>) -> Value {
    use std::cell::RefCell;
    use std::rc::Rc;

    let obj = Rc::new(RefCell::new(super::value::Object {
        kind: super::value::ObjectKind::Map(initial_entries),
        properties: std::collections::HashMap::default(),
        private_fields: HashMap::default(),
        prototype: None,
    }));

    let value = Value::Object(obj.clone());

    // get method
    let obj_ref = obj.clone();
    let get_fn = Rc::new(move |args: &[Value]| -> Result<Value> {
        let key = args.first().cloned().unwrap_or(Value::Undefined);
        let obj = obj_ref.borrow();
        if let super::value::ObjectKind::Map(entries) = &obj.kind {
            for (k, v) in entries {
                if k.strict_equals(&key) {
                    return Ok(v.clone());
                }
            }
        }
        Ok(Value::Undefined)
    });
    value.set_property("get", Value::Object(Rc::new(RefCell::new(super::value::Object {
        kind: super::value::ObjectKind::NativeFunction { name: "get".to_string(), func: get_fn },
        properties: std::collections::HashMap::default(),
        private_fields: HashMap::default(),
        prototype: None,
    }))));

    // set method
    let obj_ref = obj.clone();
    let set_fn = Rc::new(move |args: &[Value]| -> Result<Value> {
        let key = args.first().cloned().unwrap_or(Value::Undefined);
        let val = args.get(1).cloned().unwrap_or(Value::Undefined);
        let mut obj = obj_ref.borrow_mut();
        if let super::value::ObjectKind::Map(entries) = &mut obj.kind {
            for (k, v) in entries.iter_mut() {
                if k.strict_equals(&key) {
                    *v = val;
                    return Ok(Value::Undefined);
                }
            }
            entries.push((key, val));
        }
        Ok(Value::Undefined)
    });
    value.set_property("set", Value::Object(Rc::new(RefCell::new(super::value::Object {
        kind: super::value::ObjectKind::NativeFunction { name: "set".to_string(), func: set_fn },
        properties: std::collections::HashMap::default(),
        private_fields: HashMap::default(),
        prototype: None,
    }))));

    // has method
    let obj_ref = obj.clone();
    let has_fn = Rc::new(move |args: &[Value]| -> Result<Value> {
        let key = args.first().cloned().unwrap_or(Value::Undefined);
        let obj = obj_ref.borrow();
        if let super::value::ObjectKind::Map(entries) = &obj.kind {
            for (k, _) in entries {
                if k.strict_equals(&key) {
                    return Ok(Value::Boolean(true));
                }
            }
        }
        Ok(Value::Boolean(false))
    });
    value.set_property("has", Value::Object(Rc::new(RefCell::new(super::value::Object {
        kind: super::value::ObjectKind::NativeFunction { name: "has".to_string(), func: has_fn },
        properties: std::collections::HashMap::default(),
        private_fields: HashMap::default(),
        prototype: None,
    }))));

    // delete method
    let obj_ref = obj.clone();
    let delete_fn = Rc::new(move |args: &[Value]| -> Result<Value> {
        let key = args.first().cloned().unwrap_or(Value::Undefined);
        let mut obj = obj_ref.borrow_mut();
        if let super::value::ObjectKind::Map(entries) = &mut obj.kind {
            let len_before = entries.len();
            entries.retain(|(k, _)| !k.strict_equals(&key));
            return Ok(Value::Boolean(entries.len() < len_before));
        }
        Ok(Value::Boolean(false))
    });
    value.set_property("delete", Value::Object(Rc::new(RefCell::new(super::value::Object {
        kind: super::value::ObjectKind::NativeFunction { name: "delete".to_string(), func: delete_fn },
        properties: std::collections::HashMap::default(),
        private_fields: HashMap::default(),
        prototype: None,
    }))));

    // clear method
    let obj_ref = obj.clone();
    let clear_fn = Rc::new(move |_args: &[Value]| -> Result<Value> {
        let mut obj = obj_ref.borrow_mut();
        if let super::value::ObjectKind::Map(entries) = &mut obj.kind {
            entries.clear();
        }
        Ok(Value::Undefined)
    });
    value.set_property("clear", Value::Object(Rc::new(RefCell::new(super::value::Object {
        kind: super::value::ObjectKind::NativeFunction { name: "clear".to_string(), func: clear_fn },
        properties: std::collections::HashMap::default(),
        private_fields: HashMap::default(),
        prototype: None,
    }))));

    // size getter (implemented as property for simplicity)
    let obj_ref = obj.clone();
    let size_fn = Rc::new(move |_args: &[Value]| -> Result<Value> {
        let obj = obj_ref.borrow();
        if let super::value::ObjectKind::Map(entries) = &obj.kind {
            return Ok(Value::Number(entries.len() as f64));
        }
        Ok(Value::Number(0.0))
    });
    value.set_property("size", Value::Object(Rc::new(RefCell::new(super::value::Object {
        kind: super::value::ObjectKind::NativeFunction { name: "size".to_string(), func: size_fn },
        properties: std::collections::HashMap::default(),
        private_fields: HashMap::default(),
        prototype: None,
    }))));

    value
}

/// Register Set object
fn register_set(vm: &mut VM) {
    // Set constructor
    vm.register_native("Set_constructor", |args| {
        let items: Vec<Value> = if let Some(Value::Object(obj)) = args.first() {
            let obj = obj.borrow();
            if let super::value::ObjectKind::Array(arr) = &obj.kind {
                let mut result = Vec::new();
                for item in arr {
                    let mut found = false;
                    for existing in &result {
                        if item.strict_equals(existing) {
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        result.push(item.clone());
                    }
                }
                result
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        Ok(create_set_object(items))
    });

    let set = Value::new_object();
    vm.set_global("Set", set);
    vm.set_global("__Set_constructor", vm.get_global("Set_constructor").unwrap_or(Value::Undefined));
}

/// Create a Set object with methods
fn create_set_object(initial_items: Vec<Value>) -> Value {
    use std::cell::RefCell;
    use std::rc::Rc;

    let obj = Rc::new(RefCell::new(super::value::Object {
        kind: super::value::ObjectKind::Set(initial_items),
        properties: std::collections::HashMap::default(),
        private_fields: HashMap::default(),
        prototype: None,
    }));

    let value = Value::Object(obj.clone());

    // add method
    let obj_ref = obj.clone();
    let add_fn = Rc::new(move |args: &[Value]| -> Result<Value> {
        let item = args.first().cloned().unwrap_or(Value::Undefined);
        let mut obj = obj_ref.borrow_mut();
        if let super::value::ObjectKind::Set(items) = &mut obj.kind {
            for existing in items.iter() {
                if existing.strict_equals(&item) {
                    return Ok(Value::Undefined);
                }
            }
            items.push(item);
        }
        Ok(Value::Undefined)
    });
    value.set_property("add", Value::Object(Rc::new(RefCell::new(super::value::Object {
        kind: super::value::ObjectKind::NativeFunction { name: "add".to_string(), func: add_fn },
        properties: std::collections::HashMap::default(),
        private_fields: HashMap::default(),
        prototype: None,
    }))));

    // has method
    let obj_ref = obj.clone();
    let has_fn = Rc::new(move |args: &[Value]| -> Result<Value> {
        let item = args.first().cloned().unwrap_or(Value::Undefined);
        let obj = obj_ref.borrow();
        if let super::value::ObjectKind::Set(items) = &obj.kind {
            for existing in items {
                if existing.strict_equals(&item) {
                    return Ok(Value::Boolean(true));
                }
            }
        }
        Ok(Value::Boolean(false))
    });
    value.set_property("has", Value::Object(Rc::new(RefCell::new(super::value::Object {
        kind: super::value::ObjectKind::NativeFunction { name: "has".to_string(), func: has_fn },
        properties: std::collections::HashMap::default(),
        private_fields: HashMap::default(),
        prototype: None,
    }))));

    // delete method
    let obj_ref = obj.clone();
    let delete_fn = Rc::new(move |args: &[Value]| -> Result<Value> {
        let item = args.first().cloned().unwrap_or(Value::Undefined);
        let mut obj = obj_ref.borrow_mut();
        if let super::value::ObjectKind::Set(items) = &mut obj.kind {
            let len_before = items.len();
            items.retain(|v| !v.strict_equals(&item));
            return Ok(Value::Boolean(items.len() < len_before));
        }
        Ok(Value::Boolean(false))
    });
    value.set_property("delete", Value::Object(Rc::new(RefCell::new(super::value::Object {
        kind: super::value::ObjectKind::NativeFunction { name: "delete".to_string(), func: delete_fn },
        properties: std::collections::HashMap::default(),
        private_fields: HashMap::default(),
        prototype: None,
    }))));

    // clear method
    let obj_ref = obj.clone();
    let clear_fn = Rc::new(move |_args: &[Value]| -> Result<Value> {
        let mut obj = obj_ref.borrow_mut();
        if let super::value::ObjectKind::Set(items) = &mut obj.kind {
            items.clear();
        }
        Ok(Value::Undefined)
    });
    value.set_property("clear", Value::Object(Rc::new(RefCell::new(super::value::Object {
        kind: super::value::ObjectKind::NativeFunction { name: "clear".to_string(), func: clear_fn },
        properties: std::collections::HashMap::default(),
        private_fields: HashMap::default(),
        prototype: None,
    }))));

    // size getter
    let obj_ref = obj.clone();
    let size_fn = Rc::new(move |_args: &[Value]| -> Result<Value> {
        let obj = obj_ref.borrow();
        if let super::value::ObjectKind::Set(items) = &obj.kind {
            return Ok(Value::Number(items.len() as f64));
        }
        Ok(Value::Number(0.0))
    });
    value.set_property("size", Value::Object(Rc::new(RefCell::new(super::value::Object {
        kind: super::value::ObjectKind::NativeFunction { name: "size".to_string(), func: size_fn },
        properties: std::collections::HashMap::default(),
        private_fields: HashMap::default(),
        prototype: None,
    }))));

    value
}

/// Register Promise object
fn register_promise(vm: &mut VM) {
    use std::cell::RefCell;
    use std::rc::Rc;

    // Promise.resolve - creates a resolved promise
    vm.register_native("Promise_resolve", |args| {
        let value = args.first().cloned().unwrap_or(Value::Undefined);
        Ok(create_resolved_promise(value))
    });

    // Promise.reject - creates a rejected promise
    vm.register_native("Promise_reject", |args| {
        let reason = args.first().cloned().unwrap_or(Value::Undefined);
        Ok(create_rejected_promise(reason))
    });

    // Promise constructor
    vm.register_native("Promise_constructor", |args| {
        // The executor function is passed as first argument
        let _executor = args.first().cloned().unwrap_or(Value::Undefined);

        // Create a pending promise
        // In a full implementation, we'd call the executor with resolve/reject functions
        let obj = Rc::new(RefCell::new(super::value::Object {
            kind: super::value::ObjectKind::Promise {
                state: super::value::PromiseState::Pending,
                value: None,
                on_fulfilled: Vec::new(),
                on_rejected: Vec::new(),
            },
            properties: std::collections::HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        }));

        let promise = Value::Object(obj.clone());
        add_promise_methods(&promise, obj);
        Ok(promise)
    });

    // Promise.all - waits for all promises
    vm.register_native("Promise_all", |args| {
        let iterable = args.first().cloned().unwrap_or(Value::Undefined);

        // Extract array of values/promises
        let promises = if let Value::Object(obj) = iterable {
            let obj = obj.borrow();
            if let super::value::ObjectKind::Array(arr) = &obj.kind {
                arr.clone()
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        // Check if all are already resolved
        let mut results = Vec::new();
        let mut all_resolved = true;

        for p in &promises {
            if let Value::Object(obj) = p {
                let obj = obj.borrow();
                if let super::value::ObjectKind::Promise { state, value, .. } = &obj.kind {
                    match state {
                        super::value::PromiseState::Fulfilled => {
                            results.push(value.as_ref().map(|v| *v.clone()).unwrap_or(Value::Undefined));
                        }
                        super::value::PromiseState::Rejected => {
                            // Return rejected promise with the first rejection
                            return Ok(create_rejected_promise(
                                value.as_ref().map(|v| *v.clone()).unwrap_or(Value::Undefined)
                            ));
                        }
                        super::value::PromiseState::Pending => {
                            all_resolved = false;
                            results.push(Value::Undefined);
                        }
                    }
                } else {
                    // Non-promise value, treat as resolved
                    results.push(p.clone());
                }
            } else {
                // Non-object, treat as resolved value
                results.push(p.clone());
            }
        }

        if all_resolved {
            Ok(create_resolved_promise(Value::new_array(results)))
        } else {
            // Return pending promise (simplified - would need event loop for full impl)
            Ok(create_pending_promise())
        }
    });

    // Promise.race - resolves/rejects with first settled promise
    vm.register_native("Promise_race", |args| {
        let iterable = args.first().cloned().unwrap_or(Value::Undefined);

        let promises = if let Value::Object(obj) = iterable {
            let obj = obj.borrow();
            if let super::value::ObjectKind::Array(arr) = &obj.kind {
                arr.clone()
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        // Return first settled promise
        for p in &promises {
            if let Value::Object(obj) = p {
                let obj = obj.borrow();
                if let super::value::ObjectKind::Promise { state, value, .. } = &obj.kind {
                    match state {
                        super::value::PromiseState::Fulfilled => {
                            return Ok(create_resolved_promise(
                                value.as_ref().map(|v| *v.clone()).unwrap_or(Value::Undefined)
                            ));
                        }
                        super::value::PromiseState::Rejected => {
                            return Ok(create_rejected_promise(
                                value.as_ref().map(|v| *v.clone()).unwrap_or(Value::Undefined)
                            ));
                        }
                        _ => {}
                    }
                } else {
                    // Non-promise, immediately resolve with it
                    return Ok(create_resolved_promise(p.clone()));
                }
            } else {
                // Non-object, immediately resolve
                return Ok(create_resolved_promise(p.clone()));
            }
        }

        Ok(create_pending_promise())
    });

    // Promise.allSettled
    vm.register_native("Promise_allSettled", |args| {
        let iterable = args.first().cloned().unwrap_or(Value::Undefined);

        let promises = if let Value::Object(obj) = iterable {
            let obj = obj.borrow();
            if let super::value::ObjectKind::Array(arr) = &obj.kind {
                arr.clone()
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        let mut results = Vec::new();
        let mut all_settled = true;

        for p in &promises {
            if let Value::Object(obj) = p {
                let obj_ref = obj.borrow();
                if let super::value::ObjectKind::Promise { state, value, .. } = &obj_ref.kind {
                    match state {
                        super::value::PromiseState::Fulfilled => {
                            let result = Value::new_object();
                            result.set_property("status", Value::String("fulfilled".to_string()));
                            result.set_property("value", value.as_ref().map(|v| *v.clone()).unwrap_or(Value::Undefined));
                            results.push(result);
                        }
                        super::value::PromiseState::Rejected => {
                            let result = Value::new_object();
                            result.set_property("status", Value::String("rejected".to_string()));
                            result.set_property("reason", value.as_ref().map(|v| *v.clone()).unwrap_or(Value::Undefined));
                            results.push(result);
                        }
                        super::value::PromiseState::Pending => {
                            all_settled = false;
                        }
                    }
                } else {
                    let result = Value::new_object();
                    result.set_property("status", Value::String("fulfilled".to_string()));
                    result.set_property("value", p.clone());
                    results.push(result);
                }
            } else {
                let result = Value::new_object();
                result.set_property("status", Value::String("fulfilled".to_string()));
                result.set_property("value", p.clone());
                results.push(result);
            }
        }

        if all_settled {
            Ok(create_resolved_promise(Value::new_array(results)))
        } else {
            Ok(create_pending_promise())
        }
    });

    // Promise.any - resolves with first fulfilled, rejects if all reject
    vm.register_native("Promise_any", |args| {
        let iterable = args.first().cloned().unwrap_or(Value::Undefined);

        let promises = if let Value::Object(obj) = iterable {
            let obj = obj.borrow();
            if let super::value::ObjectKind::Array(arr) = &obj.kind {
                arr.clone()
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        if promises.is_empty() {
            // Empty iterable - reject with AggregateError
            let error = Value::new_object();
            error.set_property("name", Value::String("AggregateError".to_string()));
            error.set_property(
                "message",
                Value::String("All promises were rejected".to_string()),
            );
            error.set_property("errors", Value::new_array(vec![]));
            return Ok(create_rejected_promise(error));
        }

        let mut errors = Vec::new();
        let mut all_rejected = true;

        for p in &promises {
            if let Value::Object(obj) = p {
                let obj = obj.borrow();
                if let super::value::ObjectKind::Promise { state, value, .. } = &obj.kind {
                    match state {
                        super::value::PromiseState::Fulfilled => {
                            // First fulfilled promise - return immediately
                            return Ok(create_resolved_promise(
                                value.as_ref().map(|v| *v.clone()).unwrap_or(Value::Undefined),
                            ));
                        }
                        super::value::PromiseState::Rejected => {
                            errors.push(
                                value.as_ref().map(|v| *v.clone()).unwrap_or(Value::Undefined),
                            );
                        }
                        super::value::PromiseState::Pending => {
                            all_rejected = false;
                        }
                    }
                } else {
                    // Non-promise, treat as fulfilled
                    return Ok(create_resolved_promise(p.clone()));
                }
            } else {
                // Non-object, treat as fulfilled
                return Ok(create_resolved_promise(p.clone()));
            }
        }

        if all_rejected {
            // All rejected - create AggregateError
            let error = Value::new_object();
            error.set_property("name", Value::String("AggregateError".to_string()));
            error.set_property(
                "message",
                Value::String("All promises were rejected".to_string()),
            );
            error.set_property("errors", Value::new_array(errors));
            Ok(create_rejected_promise(error))
        } else {
            // Some still pending
            Ok(create_pending_promise())
        }
    });

    // Create Promise object
    let promise = Value::new_object();
    promise.set_property("resolve", vm.get_global("Promise_resolve").unwrap_or(Value::Undefined));
    promise.set_property("reject", vm.get_global("Promise_reject").unwrap_or(Value::Undefined));
    promise.set_property("all", vm.get_global("Promise_all").unwrap_or(Value::Undefined));
    promise.set_property("race", vm.get_global("Promise_race").unwrap_or(Value::Undefined));
    promise.set_property("allSettled", vm.get_global("Promise_allSettled").unwrap_or(Value::Undefined));
    promise.set_property("any", vm.get_global("Promise_any").unwrap_or(Value::Undefined));

    vm.set_global("Promise", promise);
    vm.set_global("__Promise_constructor", vm.get_global("Promise_constructor").unwrap_or(Value::Undefined));
}

/// Create a resolved promise
fn create_resolved_promise(value: Value) -> Value {
    use std::cell::RefCell;
    use std::rc::Rc;

    let obj = Rc::new(RefCell::new(super::value::Object {
        kind: super::value::ObjectKind::Promise {
            state: super::value::PromiseState::Fulfilled,
            value: Some(Box::new(value)),
            on_fulfilled: Vec::new(),
            on_rejected: Vec::new(),
        },
        properties: std::collections::HashMap::default(),
        private_fields: HashMap::default(),
        prototype: None,
    }));

    let promise = Value::Object(obj.clone());
    add_promise_methods(&promise, obj);
    promise
}

/// Create a rejected promise
fn create_rejected_promise(reason: Value) -> Value {
    use std::cell::RefCell;
    use std::rc::Rc;

    let obj = Rc::new(RefCell::new(super::value::Object {
        kind: super::value::ObjectKind::Promise {
            state: super::value::PromiseState::Rejected,
            value: Some(Box::new(reason)),
            on_fulfilled: Vec::new(),
            on_rejected: Vec::new(),
        },
        properties: std::collections::HashMap::default(),
        private_fields: HashMap::default(),
        prototype: None,
    }));

    let promise = Value::Object(obj.clone());
    add_promise_methods(&promise, obj);
    promise
}

/// Create a pending promise
fn create_pending_promise() -> Value {
    use std::cell::RefCell;
    use std::rc::Rc;

    let obj = Rc::new(RefCell::new(super::value::Object {
        kind: super::value::ObjectKind::Promise {
            state: super::value::PromiseState::Pending,
            value: None,
            on_fulfilled: Vec::new(),
            on_rejected: Vec::new(),
        },
        properties: std::collections::HashMap::default(),
        private_fields: HashMap::default(),
        prototype: None,
    }));

    let promise = Value::Object(obj.clone());
    add_promise_methods(&promise, obj);
    promise
}

/// Add .then(), .catch(), .finally() methods to a promise
fn add_promise_methods(promise: &Value, obj: std::rc::Rc<std::cell::RefCell<super::value::Object>>) {
    use std::cell::RefCell;
    use std::rc::Rc;

    // .then(onFulfilled, onRejected)
    let obj_ref = obj.clone();
    let then_fn = Rc::new(move |args: &[Value]| -> Result<Value> {
        let on_fulfilled = args.first().cloned();
        let on_rejected = args.get(1).cloned();

        let obj = obj_ref.borrow();
        if let super::value::ObjectKind::Promise { state, value, .. } = &obj.kind {
            match state {
                super::value::PromiseState::Fulfilled => {
                    if let Some(_callback) = on_fulfilled {
                        let result_value = value.as_ref().map(|v| *v.clone()).unwrap_or(Value::Undefined);
                        // In a full impl, we'd call the callback and return a new promise
                        // For now, return a resolved promise with the value
                        drop(obj);
                        return Ok(create_resolved_promise(result_value));
                    }
                    let result_value = value.as_ref().map(|v| *v.clone()).unwrap_or(Value::Undefined);
                    drop(obj);
                    Ok(create_resolved_promise(result_value))
                }
                super::value::PromiseState::Rejected => {
                    if let Some(_callback) = on_rejected {
                        let result_value = value.as_ref().map(|v| *v.clone()).unwrap_or(Value::Undefined);
                        drop(obj);
                        return Ok(create_resolved_promise(result_value));
                    }
                    let result_value = value.as_ref().map(|v| *v.clone()).unwrap_or(Value::Undefined);
                    drop(obj);
                    Ok(create_rejected_promise(result_value))
                }
                super::value::PromiseState::Pending => {
                    // Store callbacks for later (would need event loop)
                    drop(obj);
                    Ok(create_pending_promise())
                }
            }
        } else {
            drop(obj);
            Ok(Value::Undefined)
        }
    });
    promise.set_property("then", Value::Object(Rc::new(RefCell::new(super::value::Object {
        kind: super::value::ObjectKind::NativeFunction { name: "then".to_string(), func: then_fn },
        properties: std::collections::HashMap::default(),
        private_fields: HashMap::default(),
        prototype: None,
    }))));

    // .catch(onRejected) - shorthand for .then(undefined, onRejected)
    let obj_ref = obj.clone();
    let catch_fn = Rc::new(move |args: &[Value]| -> Result<Value> {
        let on_rejected = args.first().cloned();

        let obj = obj_ref.borrow();
        if let super::value::ObjectKind::Promise { state, value, .. } = &obj.kind {
            match state {
                super::value::PromiseState::Rejected => {
                    if on_rejected.is_some() {
                        let result_value = value.as_ref().map(|v| *v.clone()).unwrap_or(Value::Undefined);
                        drop(obj);
                        return Ok(create_resolved_promise(result_value));
                    }
                    let result_value = value.as_ref().map(|v| *v.clone()).unwrap_or(Value::Undefined);
                    drop(obj);
                    Ok(create_rejected_promise(result_value))
                }
                super::value::PromiseState::Fulfilled => {
                    let result_value = value.as_ref().map(|v| *v.clone()).unwrap_or(Value::Undefined);
                    drop(obj);
                    Ok(create_resolved_promise(result_value))
                }
                super::value::PromiseState::Pending => {
                    drop(obj);
                    Ok(create_pending_promise())
                }
            }
        } else {
            drop(obj);
            Ok(Value::Undefined)
        }
    });
    promise.set_property("catch", Value::Object(Rc::new(RefCell::new(super::value::Object {
        kind: super::value::ObjectKind::NativeFunction { name: "catch".to_string(), func: catch_fn },
        properties: std::collections::HashMap::default(),
        private_fields: HashMap::default(),
        prototype: None,
    }))));

    // .finally(onFinally)
    let obj_ref = obj.clone();
    let finally_fn = Rc::new(move |args: &[Value]| -> Result<Value> {
        let _on_finally = args.first().cloned();

        let obj = obj_ref.borrow();
        if let super::value::ObjectKind::Promise { state, value, .. } = &obj.kind {
            // Finally always returns same state but runs callback
            match state {
                super::value::PromiseState::Fulfilled => {
                    let result_value = value.as_ref().map(|v| *v.clone()).unwrap_or(Value::Undefined);
                    drop(obj);
                    Ok(create_resolved_promise(result_value))
                }
                super::value::PromiseState::Rejected => {
                    let result_value = value.as_ref().map(|v| *v.clone()).unwrap_or(Value::Undefined);
                    drop(obj);
                    Ok(create_rejected_promise(result_value))
                }
                super::value::PromiseState::Pending => {
                    drop(obj);
                    Ok(create_pending_promise())
                }
            }
        } else {
            drop(obj);
            Ok(Value::Undefined)
        }
    });
    promise.set_property("finally", Value::Object(Rc::new(RefCell::new(super::value::Object {
        kind: super::value::ObjectKind::NativeFunction { name: "finally".to_string(), func: finally_fn },
        properties: std::collections::HashMap::default(),
        private_fields: HashMap::default(),
        prototype: None,
    }))));
}

/// Pending timer registration request
#[derive(Clone)]
pub struct PendingTimer {
    pub callback: Value,
    pub delay: u64,
    pub args: Vec<Value>,
    pub repeating: bool,
    pub id: u64,
}

/// Pending timer cancellation request
#[derive(Clone)]
#[allow(dead_code)]
pub struct PendingCancel {
    pub id: u64,
}

thread_local! {
    /// Pending timer registrations (will be processed by VM after native call)
    pub static PENDING_TIMERS: RefCell<Vec<PendingTimer>> = RefCell::new(Vec::new());
    /// Pending timer cancellations
    pub static PENDING_CANCELS: RefCell<Vec<u64>> = RefCell::new(Vec::new());
    /// Pending microtask registrations
    pub static PENDING_MICROTASKS: RefCell<Vec<Value>> = RefCell::new(Vec::new());
}

/// Register setTimeout and setInterval
fn register_timers(vm: &mut VM) {
    use std::sync::atomic::{AtomicU64, Ordering};

    static TIMER_ID: AtomicU64 = AtomicU64::new(1);

    // setTimeout - schedule a callback to run after delay
    vm.register_native("setTimeout", |args| {
        let callback = args.first().cloned().unwrap_or(Value::Undefined);
        let delay = args.get(1).map(|v| v.to_number().max(0.0)).unwrap_or(0.0) as u64;
        let extra_args: Vec<Value> = args.iter().skip(2).cloned().collect();

        let id = TIMER_ID.fetch_add(1, Ordering::SeqCst);

        PENDING_TIMERS.with(|timers| {
            timers.borrow_mut().push(PendingTimer {
                callback,
                delay,
                args: extra_args,
                repeating: false,
                id,
            });
        });

        Ok(Value::Number(id as f64))
    });

    // clearTimeout
    vm.register_native("clearTimeout", |args| {
        let id = args.first().map(|v| v.to_number()).unwrap_or(0.0) as u64;
        PENDING_CANCELS.with(|cancels| {
            cancels.borrow_mut().push(id);
        });
        Ok(Value::Undefined)
    });

    // setInterval - schedule a repeating callback
    vm.register_native("setInterval", |args| {
        let callback = args.first().cloned().unwrap_or(Value::Undefined);
        let interval = args.get(1).map(|v| v.to_number().max(0.0)).unwrap_or(0.0) as u64;
        let extra_args: Vec<Value> = args.iter().skip(2).cloned().collect();

        let id = TIMER_ID.fetch_add(1, Ordering::SeqCst);

        PENDING_TIMERS.with(|timers| {
            timers.borrow_mut().push(PendingTimer {
                callback,
                delay: interval,
                args: extra_args,
                repeating: true,
                id,
            });
        });

        Ok(Value::Number(id as f64))
    });

    // clearInterval
    vm.register_native("clearInterval", |args| {
        let id = args.first().map(|v| v.to_number()).unwrap_or(0.0) as u64;
        PENDING_CANCELS.with(|cancels| {
            cancels.borrow_mut().push(id);
        });
        Ok(Value::Undefined)
    });

    // queueMicrotask - queue a microtask
    vm.register_native("queueMicrotask", |args| {
        let callback = args.first().cloned().unwrap_or(Value::Undefined);
        PENDING_MICROTASKS.with(|tasks| {
            tasks.borrow_mut().push(callback);
        });
        Ok(Value::Undefined)
    });
}

/// Register fetch API
fn register_fetch(vm: &mut VM) {
    use rustc_hash::FxHashMap as HashMap;

    // Get sandbox from VM for permission checks
    let sandbox = vm.get_sandbox();

    // Create Response class constructor
    vm.register_native("Response", |args| {
        let body = args.first().cloned().unwrap_or(Value::String("".to_string()));
        let status = args.get(1)
            .and_then(|opts| {
                if let Value::Object(obj) = opts {
                    obj.borrow().get_property("status").map(|v| v.to_number() as u16)
                } else {
                    None
                }
            })
            .unwrap_or(200);

        // Create Response object
        let response = Value::Object(Rc::new(RefCell::new(super::value::Object {
            kind: super::value::ObjectKind::Ordinary,
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        })));

        // Set response properties
        response.set_property("ok", Value::Boolean(status >= 200 && status < 300));
        response.set_property("status", Value::Number(status as f64));
        response.set_property("statusText", Value::String(
            match status {
                200 => "OK".to_string(),
                201 => "Created".to_string(),
                204 => "No Content".to_string(),
                400 => "Bad Request".to_string(),
                401 => "Unauthorized".to_string(),
                403 => "Forbidden".to_string(),
                404 => "Not Found".to_string(),
                500 => "Internal Server Error".to_string(),
                _ => "Unknown".to_string(),
            }
        ));
        response.set_property("_body", body);

        Ok(response)
    });

    // Response.prototype.json() - parse body as JSON
    vm.register_native("Response_json", |args| {
        let this = args.first().cloned().unwrap_or(Value::Undefined);
        if let Value::Object(obj) = &this {
            let body = obj.borrow().get_property("_body").unwrap_or(Value::Undefined);
            let body_str = body.to_js_string();

            // Parse JSON
            match parse_json(&body_str) {
                Ok(parsed) => {
                    // Return a resolved Promise with the parsed value
                    Ok(create_resolved_promise(parsed))
                }
                Err(_) => {
                    // Return a rejected Promise
                    Ok(create_rejected_promise(Value::String("Invalid JSON".to_string())))
                }
            }
        } else {
            Ok(create_rejected_promise(Value::String("Not a Response object".to_string())))
        }
    });

    // Response.prototype.text() - get body as text
    vm.register_native("Response_text", |args| {
        let this = args.first().cloned().unwrap_or(Value::Undefined);
        if let Value::Object(obj) = &this {
            let body = obj.borrow().get_property("_body").unwrap_or(Value::Undefined);
            Ok(create_resolved_promise(Value::String(body.to_js_string())))
        } else {
            Ok(create_rejected_promise(Value::String("Not a Response object".to_string())))
        }
    });

    // fetch() function - simplified synchronous implementation with sandbox support
    let sandbox_fetch = sandbox.clone();
    vm.register_native("fetch", move |args| {
        let url = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        let options = args.get(1).cloned();

        // Extract host from URL for permission check
        let host = extract_host_from_url(&url);

        // Check network permission if sandbox is enabled
        if let Some(ref sandbox_rc) = sandbox_fetch {
            let sandbox = sandbox_rc.borrow();
            let cap = Capability::Network(HostPattern::Exact(host.clone()));
            if sandbox.check(&cap) != PermissionState::Granted {
                return Err(crate::error::Error::type_error(
                    format!("Requires net access to {:?}, run again with the --allow-net flag", host)
                ));
            }
        }

        // Get method from options (default to GET)
        let method = options.as_ref()
            .and_then(|opts| {
                if let Value::Object(obj) = opts {
                    obj.borrow().get_property("method").map(|v| v.to_js_string())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "GET".to_string());

        // Get body from options
        let body = options.as_ref()
            .and_then(|opts| {
                if let Value::Object(obj) = opts {
                    obj.borrow().get_property("body")
                } else {
                    None
                }
            });

        // Get headers from options
        let _headers = options.as_ref()
            .and_then(|opts| {
                if let Value::Object(obj) = opts {
                    obj.borrow().get_property("headers")
                } else {
                    None
                }
            });

        // For now, we can't actually make HTTP requests without a runtime
        // Return a mock response based on the URL pattern
        let (status, response_body) = if url.starts_with("mock://") {
            // Mock responses for testing
            let path = &url[7..];
            match path {
                "success" => (200, r#"{"success": true}"#.to_string()),
                "error" => (500, r#"{"error": "Server error"}"#.to_string()),
                "notfound" => (404, r#"{"error": "Not found"}"#.to_string()),
                "echo" => {
                    let echo_body = body.map(|b| b.to_js_string()).unwrap_or_default();
                    (200, echo_body)
                }
                _ => (200, format!(r#"{{"url": "{}", "method": "{}"}}"#, url, method)),
            }
        } else {
            // For real URLs, return a placeholder (actual HTTP would need async runtime)
            (200, format!(r#"{{"url": "{}", "method": "{}", "note": "HTTP not yet implemented"}}"#, url, method))
        };

        // Create Response object
        let response = Value::Object(Rc::new(RefCell::new(super::value::Object {
            kind: super::value::ObjectKind::Ordinary,
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        })));

        response.set_property("ok", Value::Boolean(status >= 200 && status < 300));
        response.set_property("status", Value::Number(status as f64));
        response.set_property("statusText", Value::String(
            match status {
                200 => "OK".to_string(),
                404 => "Not Found".to_string(),
                500 => "Internal Server Error".to_string(),
                _ => "Unknown".to_string(),
            }
        ));
        response.set_property("_body", Value::String(response_body));
        response.set_property("url", Value::String(url));

        // Return a resolved Promise with the Response
        Ok(create_resolved_promise(response))
    });
}

fn stringify_value(value: &Value) -> String {
    match value {
        Value::Undefined => "undefined".to_string(),
        Value::Null => "null".to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Number(n) => {
            if n.is_nan() || n.is_infinite() {
                "null".to_string()
            } else {
                format!("{}", n)
            }
        }
        Value::BigInt(n) => n.to_string(),
        Value::String(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
        Value::Object(obj) => {
            let obj = obj.borrow();
            match &obj.kind {
                super::value::ObjectKind::Array(arr) => {
                    let elements: Vec<String> = arr.iter().map(stringify_value).collect();
                    format!("[{}]", elements.join(","))
                }
                _ => {
                    let pairs: Vec<String> = obj
                        .properties
                        .iter()
                        .map(|(k, v)| format!("\"{}\":{}", k, stringify_value(v)))
                        .collect();
                    format!("{{{}}}", pairs.join(","))
                }
            }
        }
        Value::Symbol(_) => "undefined".to_string(),
    }
}

fn parse_json(text: &str) -> Result<Value> {
    // Simplified JSON parser
    let text = text.trim();

    if text == "null" {
        return Ok(Value::Null);
    }
    if text == "true" {
        return Ok(Value::Boolean(true));
    }
    if text == "false" {
        return Ok(Value::Boolean(false));
    }

    // Try to parse as number
    if let Ok(n) = text.parse::<f64>() {
        return Ok(Value::Number(n));
    }

    // String
    if text.starts_with('"') && text.ends_with('"') {
        let inner = &text[1..text.len() - 1];
        return Ok(Value::String(
            inner.replace("\\\"", "\"").replace("\\\\", "\\"),
        ));
    }

    // Array
    if text.starts_with('[') && text.ends_with(']') {
        let inner = &text[1..text.len() - 1].trim();
        if inner.is_empty() {
            return Ok(Value::new_array(vec![]));
        }
        // Simplified: just split by comma (doesn't handle nested structures)
        let elements: Vec<Value> = inner
            .split(',')
            .map(|s| parse_json(s.trim()).unwrap_or(Value::Null))
            .collect();
        return Ok(Value::new_array(elements));
    }

    // Object
    if text.starts_with('{') && text.ends_with('}') {
        let obj = Value::new_object();
        let inner = &text[1..text.len() - 1].trim();
        if !inner.is_empty() {
            // Simplified parsing
            for pair in inner.split(',') {
                if let Some(colon_pos) = pair.find(':') {
                    let key = pair[..colon_pos].trim().trim_matches('"');
                    let value = parse_json(pair[colon_pos + 1..].trim()).unwrap_or(Value::Null);
                    obj.set_property(key, value);
                }
            }
        }
        return Ok(obj);
    }

    Ok(Value::Undefined)
}

/// Register Deno-style APIs with security sandbox enforcement
fn register_deno(vm: &mut VM) {
    use std::cell::RefCell;
    use rustc_hash::FxHashMap as HashMap;
    use std::path::PathBuf;
    use std::rc::Rc;

    // Get sandbox from VM for permission checks
    let sandbox = vm.get_sandbox();

    // Create Deno.readTextFile
    let sandbox_read = sandbox.clone();
    vm.register_native("Deno_readTextFile", move |args| {
        let path = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        let path_buf = PathBuf::from(&path);

        // Check permission if sandbox is enabled
        if let Some(ref sandbox_rc) = sandbox_read {
            let sandbox = sandbox_rc.borrow();
            let cap = Capability::FileRead(PathPattern::Exact(path_buf.clone()));
            if sandbox.check(&cap) != PermissionState::Granted {
                return Err(crate::error::Error::type_error(
                    format!("Requires read access to {:?}, run again with the --allow-read flag", path)
                ));
            }
        }

        // Actually read the file
        match std::fs::read_to_string(&path) {
            Ok(content) => Ok(create_resolved_promise(Value::String(content))),
            Err(e) => Ok(create_rejected_promise(Value::String(e.to_string()))),
        }
    });

    // Create Deno.writeTextFile
    let sandbox_write = sandbox.clone();
    vm.register_native("Deno_writeTextFile", move |args| {
        let path = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        let content = args.get(1).map(|v| v.to_js_string()).unwrap_or_default();
        let path_buf = PathBuf::from(&path);

        // Check permission if sandbox is enabled
        if let Some(ref sandbox_rc) = sandbox_write {
            let sandbox = sandbox_rc.borrow();
            let cap = Capability::FileWrite(PathPattern::Exact(path_buf.clone()));
            if sandbox.check(&cap) != PermissionState::Granted {
                return Err(crate::error::Error::type_error(
                    format!("Requires write access to {:?}, run again with the --allow-write flag", path)
                ));
            }
        }

        // Actually write the file
        match std::fs::write(&path, content) {
            Ok(()) => Ok(create_resolved_promise(Value::Undefined)),
            Err(e) => Ok(create_rejected_promise(Value::String(e.to_string()))),
        }
    });

    // Create Deno.readDir
    let sandbox_readdir = sandbox.clone();
    vm.register_native("Deno_readDir", move |args| {
        let path = args.first().map(|v| v.to_js_string()).unwrap_or(".".to_string());
        let path_buf = PathBuf::from(&path);

        // Check permission if sandbox is enabled
        if let Some(ref sandbox_rc) = sandbox_readdir {
            let sandbox = sandbox_rc.borrow();
            let cap = Capability::FileRead(PathPattern::Exact(path_buf.clone()));
            if sandbox.check(&cap) != PermissionState::Granted {
                return Err(crate::error::Error::type_error(
                    format!("Requires read access to {:?}, run again with the --allow-read flag", path)
                ));
            }
        }

        // Read directory and return array of entries
        match std::fs::read_dir(&path) {
            Ok(entries) => {
                let mut items = Vec::new();
                for entry in entries.flatten() {
                    let mut props = HashMap::default();
                    props.insert("name".to_string(), Value::String(entry.file_name().to_string_lossy().to_string()));
                    if let Ok(file_type) = entry.file_type() {
                        props.insert("isFile".to_string(), Value::Boolean(file_type.is_file()));
                        props.insert("isDirectory".to_string(), Value::Boolean(file_type.is_dir()));
                        props.insert("isSymlink".to_string(), Value::Boolean(file_type.is_symlink()));
                    }
                    items.push(Value::new_object_with_properties(props));
                }
                Ok(Value::new_array(items))
            }
            Err(e) => Ok(create_rejected_promise(Value::String(e.to_string()))),
        }
    });

    // Create Deno.stat
    let sandbox_stat = sandbox.clone();
    vm.register_native("Deno_stat", move |args| {
        let path = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        let path_buf = PathBuf::from(&path);

        // Check permission if sandbox is enabled
        if let Some(ref sandbox_rc) = sandbox_stat {
            let sandbox = sandbox_rc.borrow();
            let cap = Capability::FileRead(PathPattern::Exact(path_buf.clone()));
            if sandbox.check(&cap) != PermissionState::Granted {
                return Err(crate::error::Error::type_error(
                    format!("Requires read access to {:?}, run again with the --allow-read flag", path)
                ));
            }
        }

        match std::fs::metadata(&path) {
            Ok(metadata) => {
                let mut props = HashMap::default();
                props.insert("isFile".to_string(), Value::Boolean(metadata.is_file()));
                props.insert("isDirectory".to_string(), Value::Boolean(metadata.is_dir()));
                props.insert("isSymlink".to_string(), Value::Boolean(metadata.file_type().is_symlink()));
                props.insert("size".to_string(), Value::Number(metadata.len() as f64));
                props.insert("readonly".to_string(), Value::Boolean(metadata.permissions().readonly()));
                Ok(create_resolved_promise(Value::new_object_with_properties(props)))
            }
            Err(e) => Ok(create_rejected_promise(Value::String(e.to_string()))),
        }
    });

    // Create Deno.remove
    let sandbox_remove = sandbox.clone();
    vm.register_native("Deno_remove", move |args| {
        let path = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        let recursive = args.get(1).and_then(|opts| {
            if let Value::Object(obj) = opts {
                obj.borrow().get_property("recursive").map(|v| v.to_boolean())
            } else {
                None
            }
        }).unwrap_or(false);
        let path_buf = PathBuf::from(&path);

        // Check permission if sandbox is enabled
        if let Some(ref sandbox_rc) = sandbox_remove {
            let sandbox = sandbox_rc.borrow();
            let cap = Capability::FileWrite(PathPattern::Exact(path_buf.clone()));
            if sandbox.check(&cap) != PermissionState::Granted {
                return Err(crate::error::Error::type_error(
                    format!("Requires write access to {:?}, run again with the --allow-write flag", path)
                ));
            }
        }

        let result = if recursive {
            std::fs::remove_dir_all(&path)
        } else if path_buf.is_dir() {
            std::fs::remove_dir(&path)
        } else {
            std::fs::remove_file(&path)
        };

        match result {
            Ok(()) => Ok(create_resolved_promise(Value::Undefined)),
            Err(e) => Ok(create_rejected_promise(Value::String(e.to_string()))),
        }
    });

    // Create Deno.mkdir
    let sandbox_mkdir = sandbox.clone();
    vm.register_native("Deno_mkdir", move |args| {
        let path = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        let recursive = args.get(1).and_then(|opts| {
            if let Value::Object(obj) = opts {
                obj.borrow().get_property("recursive").map(|v| v.to_boolean())
            } else {
                None
            }
        }).unwrap_or(false);
        let path_buf = PathBuf::from(&path);

        // Check permission if sandbox is enabled
        if let Some(ref sandbox_rc) = sandbox_mkdir {
            let sandbox = sandbox_rc.borrow();
            let cap = Capability::FileWrite(PathPattern::Exact(path_buf.clone()));
            if sandbox.check(&cap) != PermissionState::Granted {
                return Err(crate::error::Error::type_error(
                    format!("Requires write access to {:?}, run again with the --allow-write flag", path)
                ));
            }
        }

        let result = if recursive {
            std::fs::create_dir_all(&path)
        } else {
            std::fs::create_dir(&path)
        };

        match result {
            Ok(()) => Ok(create_resolved_promise(Value::Undefined)),
            Err(e) => Ok(create_rejected_promise(Value::String(e.to_string()))),
        }
    });

    // Create Deno.run (subprocess execution)
    let sandbox_run = sandbox.clone();
    vm.register_native("Deno_run", move |args| {
        let opts = args.first().cloned().unwrap_or(Value::Undefined);

        // Get command array
        let cmd = if let Value::Object(obj) = &opts {
            obj.borrow().get_property("cmd").and_then(|v| {
                if let Value::Object(arr_obj) = &v {
                    let arr = arr_obj.borrow();
                    if let super::value::ObjectKind::Array(items) = &arr.kind {
                        Some(items.iter().map(|v| v.to_js_string()).collect::<Vec<_>>())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
        } else {
            None
        }.unwrap_or_default();

        if cmd.is_empty() {
            return Ok(create_rejected_promise(Value::String("cmd required".to_string())));
        }

        // Check permission if sandbox is enabled
        if let Some(ref sandbox_rc) = sandbox_run {
            let sandbox = sandbox_rc.borrow();
            if sandbox.check(&Capability::Subprocess) != PermissionState::Granted {
                return Err(crate::error::Error::type_error(
                    format!("Requires run access to {:?}, run again with the --allow-run flag", cmd[0])
                ));
            }
        }

        // Execute command
        match std::process::Command::new(&cmd[0])
            .args(&cmd[1..])
            .output()
        {
            Ok(output) => {
                let mut props = HashMap::default();
                props.insert("stdout".to_string(), Value::String(String::from_utf8_lossy(&output.stdout).to_string()));
                props.insert("stderr".to_string(), Value::String(String::from_utf8_lossy(&output.stderr).to_string()));
                props.insert("success".to_string(), Value::Boolean(output.status.success()));
                props.insert("code".to_string(), output.status.code().map(|c| Value::Number(c as f64)).unwrap_or(Value::Null));
                Ok(create_resolved_promise(Value::new_object_with_properties(props)))
            }
            Err(e) => Ok(create_rejected_promise(Value::String(e.to_string()))),
        }
    });

    // Create Deno.env.get
    let sandbox_env = sandbox.clone();
    vm.register_native("Deno_env_get", move |args| {
        let key = args.first().map(|v| v.to_js_string()).unwrap_or_default();

        // Check permission if sandbox is enabled
        if let Some(ref sandbox_rc) = sandbox_env {
            let sandbox = sandbox_rc.borrow();
            let cap = Capability::Env(crate::security::EnvPattern::Exact(key.clone()));
            if sandbox.check(&cap) != PermissionState::Granted {
                return Err(crate::error::Error::type_error(
                    format!("Requires env access to {:?}, run again with the --allow-env flag", key)
                ));
            }
        }

        match std::env::var(&key) {
            Ok(val) => Ok(Value::String(val)),
            Err(_) => Ok(Value::Undefined),
        }
    });

    // Create Deno.env.set
    let sandbox_env_set = sandbox.clone();
    vm.register_native("Deno_env_set", move |args| {
        let key = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        let value = args.get(1).map(|v| v.to_js_string()).unwrap_or_default();

        // Check permission if sandbox is enabled
        if let Some(ref sandbox_rc) = sandbox_env_set {
            let sandbox = sandbox_rc.borrow();
            let cap = Capability::Env(crate::security::EnvPattern::Exact(key.clone()));
            if sandbox.check(&cap) != PermissionState::Granted {
                return Err(crate::error::Error::type_error(
                    format!("Requires env access to {:?}, run again with the --allow-env flag", key)
                ));
            }
        }

        std::env::set_var(&key, &value);
        Ok(Value::Undefined)
    });

    // Create Deno.exit
    vm.register_native("Deno_exit", |args| {
        let code = args.first().map(|v| v.to_number() as i32).unwrap_or(0);
        std::process::exit(code);
    });

    // Create Deno.cwd
    vm.register_native("Deno_cwd", |_args| {
        match std::env::current_dir() {
            Ok(path) => Ok(Value::String(path.to_string_lossy().to_string())),
            Err(e) => Err(crate::error::Error::type_error(e.to_string())),
        }
    });

    // Create Deno.chdir
    vm.register_native("Deno_chdir", |args| {
        let path = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        match std::env::set_current_dir(&path) {
            Ok(()) => Ok(Value::Undefined),
            Err(e) => Err(crate::error::Error::type_error(e.to_string())),
        }
    });

    // Create Deno.stdin.readLine - read a line from stdin
    vm.register_native("Deno_stdin_readLine", |_args| {
        use std::io::BufRead;
        let stdin = std::io::stdin();
        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => Ok(Value::Null), // EOF
            Ok(_) => {
                // Remove trailing newline
                if line.ends_with('\n') {
                    line.pop();
                    if line.ends_with('\r') {
                        line.pop();
                    }
                }
                Ok(Value::String(line))
            }
            Err(e) => Err(crate::error::Error::type_error(e.to_string())),
        }
    });

    // Create Deno.stdin.read - read bytes from stdin
    vm.register_native("Deno_stdin_read", |args| {
        use std::io::Read;
        let size = args.first().map(|v| v.to_number() as usize).unwrap_or(1024);
        let stdin = std::io::stdin();
        let mut buffer = vec![0u8; size];
        match stdin.lock().read(&mut buffer) {
            Ok(0) => Ok(Value::Null), // EOF
            Ok(n) => {
                buffer.truncate(n);
                // Return as Uint8Array-like array
                let values: Vec<Value> = buffer.iter().map(|&b| Value::Number(b as f64)).collect();
                Ok(Value::new_array(values))
            }
            Err(e) => Err(crate::error::Error::type_error(e.to_string())),
        }
    });

    // Create Deno.stdout.write - write to stdout
    vm.register_native("Deno_stdout_write", |args| {
        use std::io::Write;
        let data = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        let stdout = std::io::stdout();
        match stdout.lock().write_all(data.as_bytes()) {
            Ok(()) => Ok(Value::Number(data.len() as f64)),
            Err(e) => Err(crate::error::Error::type_error(e.to_string())),
        }
    });

    // Create Deno.stdout.writeSync - synchronous write to stdout
    vm.register_native("Deno_stdout_writeSync", |args| {
        use std::io::Write;
        let data = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        let mut stdout = std::io::stdout();
        match stdout.write_all(data.as_bytes()) {
            Ok(()) => {
                let _ = stdout.flush();
                Ok(Value::Number(data.len() as f64))
            }
            Err(e) => Err(crate::error::Error::type_error(e.to_string())),
        }
    });

    // Create Deno.stderr.write - write to stderr
    vm.register_native("Deno_stderr_write", |args| {
        use std::io::Write;
        let data = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        let stderr = std::io::stderr();
        match stderr.lock().write_all(data.as_bytes()) {
            Ok(()) => Ok(Value::Number(data.len() as f64)),
            Err(e) => Err(crate::error::Error::type_error(e.to_string())),
        }
    });

    // Create Deno.stderr.writeSync - synchronous write to stderr
    vm.register_native("Deno_stderr_writeSync", |args| {
        use std::io::Write;
        let data = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        let mut stderr = std::io::stderr();
        match stderr.write_all(data.as_bytes()) {
            Ok(()) => {
                let _ = stderr.flush();
                Ok(Value::Number(data.len() as f64))
            }
            Err(e) => Err(crate::error::Error::type_error(e.to_string())),
        }
    });

    // Create the Deno global object with all methods
    let deno_obj = Value::Object(Rc::new(RefCell::new(super::value::Object {
        kind: super::value::ObjectKind::Ordinary,
        properties: HashMap::default(),
        private_fields: HashMap::default(),
        prototype: None,
    })));

    // Attach methods to Deno object
    deno_obj.set_property("readTextFile", vm.get_global("Deno_readTextFile").unwrap_or(Value::Undefined));
    deno_obj.set_property("writeTextFile", vm.get_global("Deno_writeTextFile").unwrap_or(Value::Undefined));
    deno_obj.set_property("readDir", vm.get_global("Deno_readDir").unwrap_or(Value::Undefined));
    deno_obj.set_property("stat", vm.get_global("Deno_stat").unwrap_or(Value::Undefined));
    deno_obj.set_property("remove", vm.get_global("Deno_remove").unwrap_or(Value::Undefined));
    deno_obj.set_property("mkdir", vm.get_global("Deno_mkdir").unwrap_or(Value::Undefined));
    deno_obj.set_property("run", vm.get_global("Deno_run").unwrap_or(Value::Undefined));
    deno_obj.set_property("exit", vm.get_global("Deno_exit").unwrap_or(Value::Undefined));
    deno_obj.set_property("cwd", vm.get_global("Deno_cwd").unwrap_or(Value::Undefined));
    deno_obj.set_property("chdir", vm.get_global("Deno_chdir").unwrap_or(Value::Undefined));

    // Create Deno.env object
    let env_obj = Value::Object(Rc::new(RefCell::new(super::value::Object {
        kind: super::value::ObjectKind::Ordinary,
        properties: HashMap::default(),
        private_fields: HashMap::default(),
        prototype: None,
    })));
    env_obj.set_property("get", vm.get_global("Deno_env_get").unwrap_or(Value::Undefined));
    env_obj.set_property("set", vm.get_global("Deno_env_set").unwrap_or(Value::Undefined));
    deno_obj.set_property("env", env_obj);

    // Create Deno.stdin object
    let stdin_obj = Value::Object(Rc::new(RefCell::new(super::value::Object {
        kind: super::value::ObjectKind::Ordinary,
        properties: HashMap::default(),
        private_fields: HashMap::default(),
        prototype: None,
    })));
    stdin_obj.set_property("readLine", vm.get_global("Deno_stdin_readLine").unwrap_or(Value::Undefined));
    stdin_obj.set_property("read", vm.get_global("Deno_stdin_read").unwrap_or(Value::Undefined));
    stdin_obj.set_property("rid", Value::Number(0.0)); // File descriptor 0
    deno_obj.set_property("stdin", stdin_obj);

    // Create Deno.stdout object
    let stdout_obj = Value::Object(Rc::new(RefCell::new(super::value::Object {
        kind: super::value::ObjectKind::Ordinary,
        properties: HashMap::default(),
        private_fields: HashMap::default(),
        prototype: None,
    })));
    stdout_obj.set_property("write", vm.get_global("Deno_stdout_write").unwrap_or(Value::Undefined));
    stdout_obj.set_property("writeSync", vm.get_global("Deno_stdout_writeSync").unwrap_or(Value::Undefined));
    stdout_obj.set_property("rid", Value::Number(1.0)); // File descriptor 1
    deno_obj.set_property("stdout", stdout_obj);

    // Create Deno.stderr object
    let stderr_obj = Value::Object(Rc::new(RefCell::new(super::value::Object {
        kind: super::value::ObjectKind::Ordinary,
        properties: HashMap::default(),
        private_fields: HashMap::default(),
        prototype: None,
    })));
    stderr_obj.set_property("write", vm.get_global("Deno_stderr_write").unwrap_or(Value::Undefined));
    stderr_obj.set_property("writeSync", vm.get_global("Deno_stderr_writeSync").unwrap_or(Value::Undefined));
    stderr_obj.set_property("rid", Value::Number(2.0)); // File descriptor 2
    deno_obj.set_property("stderr", stderr_obj);

    vm.set_global("Deno", deno_obj);
}

/// Check network permission using sandbox
#[allow(dead_code)]
fn check_network_permission(sandbox: &Option<Rc<RefCell<Sandbox>>>, host: &str) -> bool {
    if let Some(ref sandbox_rc) = sandbox {
        let sandbox = sandbox_rc.borrow();
        let cap = Capability::Network(HostPattern::Exact(host.to_string()));
        sandbox.check(&cap) == PermissionState::Granted
    } else {
        true // No sandbox = all permitted
    }
}

/// Extract host from URL for permission checking
fn extract_host_from_url(url: &str) -> String {
    // Simple URL host extraction
    let url = url.trim();

    // Handle common schemes
    let rest = if url.starts_with("https://") {
        &url[8..]
    } else if url.starts_with("http://") {
        &url[7..]
    } else if url.starts_with("mock://") {
        return "mock".to_string(); // Mock URLs don't need network permission
    } else {
        url
    };

    // Extract host (up to first / or : or end)
    let host_end = rest.find('/').unwrap_or(rest.len());
    let host_with_port = &rest[..host_end];

    // Strip port if present
    let host = host_with_port.split(':').next().unwrap_or(host_with_port);

    host.to_string()
}

/// Register global functions
fn register_global_functions(vm: &mut VM) {
    // parseInt
    vm.register_native("parseInt", |args| {
        let text = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        let radix = args.get(1).map(|v| v.to_number() as i32).unwrap_or(10);

        let text = text.trim();
        let (text, negative) = if let Some(stripped) = text.strip_prefix('-') {
            (stripped, true)
        } else if let Some(stripped) = text.strip_prefix('+') {
            (stripped, false)
        } else {
            (text, false)
        };

        let result = if radix == 16 && text.starts_with("0x") {
            i64::from_str_radix(&text[2..], 16)
        } else {
            i64::from_str_radix(text, radix as u32)
        };

        match result {
            Ok(n) => Ok(Value::Number(if negative { -(n as f64) } else { n as f64 })),
            Err(_) => Ok(Value::Number(f64::NAN)),
        }
    });

    // parseFloat
    vm.register_native("parseFloat", |args| {
        let text = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        match text.trim().parse::<f64>() {
            Ok(n) => Ok(Value::Number(n)),
            Err(_) => Ok(Value::Number(f64::NAN)),
        }
    });

    // Boolean constructor
    vm.register_native("Boolean", |args| {
        let val = args.first().cloned().unwrap_or(Value::Undefined);
        Ok(Value::Boolean(val.to_boolean()))
    });

    // Number constructor function (for calling Number(x))
    vm.register_native("Number_constructor", |args| {
        let val = args.first().cloned().unwrap_or(Value::Undefined);
        Ok(Value::Number(val.to_number()))
    });

    // Number static methods
    vm.register_native("Number_isNaN", |args| {
        // Unlike global isNaN, Number.isNaN doesn't coerce
        let val = args.first().cloned().unwrap_or(Value::Undefined);
        match val {
            Value::Number(n) => Ok(Value::Boolean(n.is_nan())),
            _ => Ok(Value::Boolean(false)),
        }
    });

    vm.register_native("Number_isFinite", |args| {
        // Unlike global isFinite, Number.isFinite doesn't coerce
        let val = args.first().cloned().unwrap_or(Value::Undefined);
        match val {
            Value::Number(n) => Ok(Value::Boolean(n.is_finite())),
            _ => Ok(Value::Boolean(false)),
        }
    });

    vm.register_native("Number_isInteger", |args| {
        let val = args.first().cloned().unwrap_or(Value::Undefined);
        match val {
            Value::Number(n) => Ok(Value::Boolean(n.is_finite() && n.fract() == 0.0)),
            _ => Ok(Value::Boolean(false)),
        }
    });

    vm.register_native("Number_isSafeInteger", |args| {
        let val = args.first().cloned().unwrap_or(Value::Undefined);
        const MAX_SAFE_INT: f64 = 9007199254740991.0; // 2^53 - 1
        match val {
            Value::Number(n) => {
                let is_safe = n.is_finite()
                    && n.fract() == 0.0
                    && n.abs() <= MAX_SAFE_INT;
                Ok(Value::Boolean(is_safe))
            }
            _ => Ok(Value::Boolean(false)),
        }
    });

    vm.register_native("Number_parseFloat", |args| {
        let text = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        match text.trim().parse::<f64>() {
            Ok(n) => Ok(Value::Number(n)),
            Err(_) => Ok(Value::Number(f64::NAN)),
        }
    });

    vm.register_native("Number_parseInt", |args| {
        let text = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        let radix = args.get(1).map(|v| v.to_number() as u32).unwrap_or(10);

        if radix != 0 && (radix < 2 || radix > 36) {
            return Ok(Value::Number(f64::NAN));
        }

        let text = text.trim();
        let (text, negative) = if text.starts_with('-') {
            (&text[1..], true)
        } else if text.starts_with('+') {
            (&text[1..], false)
        } else {
            (text, false)
        };

        let radix = if radix == 0 {
            if text.starts_with("0x") || text.starts_with("0X") {
                16
            } else {
                10
            }
        } else {
            radix
        };

        let text = if radix == 16 && (text.starts_with("0x") || text.starts_with("0X")) {
            &text[2..]
        } else {
            text
        };

        match i64::from_str_radix(text, radix) {
            Ok(n) => Ok(Value::Number(if negative { -(n as f64) } else { n as f64 })),
            Err(_) => Ok(Value::Number(f64::NAN)),
        }
    });

    // Create Number object with static methods and constants
    let number = Value::new_object();

    // Constants
    number.set_property("MAX_VALUE", Value::Number(f64::MAX));
    number.set_property("MIN_VALUE", Value::Number(f64::MIN_POSITIVE));
    number.set_property("NaN", Value::Number(f64::NAN));
    number.set_property("NEGATIVE_INFINITY", Value::Number(f64::NEG_INFINITY));
    number.set_property("POSITIVE_INFINITY", Value::Number(f64::INFINITY));
    number.set_property("EPSILON", Value::Number(f64::EPSILON));
    number.set_property("MAX_SAFE_INTEGER", Value::Number(9007199254740991.0)); // 2^53 - 1
    number.set_property("MIN_SAFE_INTEGER", Value::Number(-9007199254740991.0)); // -(2^53 - 1)

    // Static methods
    number.set_property("isNaN", vm.get_global("Number_isNaN").unwrap_or(Value::Undefined));
    number.set_property("isFinite", vm.get_global("Number_isFinite").unwrap_or(Value::Undefined));
    number.set_property("isInteger", vm.get_global("Number_isInteger").unwrap_or(Value::Undefined));
    number.set_property("isSafeInteger", vm.get_global("Number_isSafeInteger").unwrap_or(Value::Undefined));
    number.set_property("parseFloat", vm.get_global("Number_parseFloat").unwrap_or(Value::Undefined));
    number.set_property("parseInt", vm.get_global("Number_parseInt").unwrap_or(Value::Undefined));

    vm.set_global("Number", number);

    // String constructor
    vm.register_native("String", |args| {
        let val = args.first().cloned().unwrap_or(Value::Undefined);
        Ok(Value::String(val.to_js_string()))
    });

    // Symbol constructor and registry
    register_symbol(vm);

    // BigInt constructor
    vm.register_native("BigInt", |args| {
        use num_bigint::BigInt;
        let val = args.first().cloned().unwrap_or(Value::Undefined);
        match &val {
            Value::BigInt(n) => Ok(Value::BigInt(n.clone())),
            Value::Number(n) => {
                if n.fract() != 0.0 || n.is_infinite() || n.is_nan() {
                    Err(crate::error::Error::range_error("Cannot convert non-integer to BigInt"))
                } else {
                    Ok(Value::BigInt(BigInt::from(*n as i64)))
                }
            }
            Value::String(s) => {
                let s = s.trim();
                let s = s.strip_suffix('n').unwrap_or(s);
                match s.parse::<BigInt>() {
                    Ok(n) => Ok(Value::BigInt(n)),
                    Err(_) => Err(crate::error::Error::syntax_error(&format!("Cannot convert {} to BigInt", val.to_js_string()))),
                }
            }
            Value::Boolean(b) => Ok(Value::BigInt(BigInt::from(if *b { 1 } else { 0 }))),
            _ => Err(crate::error::Error::type_error(&format!("Cannot convert {} to BigInt", val.type_of()))),
        }
    });

    // isNaN
    vm.register_native("isNaN", |args| {
        let n = args.first().map(|v| v.to_number()).unwrap_or(f64::NAN);
        Ok(Value::Boolean(n.is_nan()))
    });

    // isFinite
    vm.register_native("isFinite", |args| {
        let n = args.first().map(|v| v.to_number()).unwrap_or(f64::NAN);
        Ok(Value::Boolean(n.is_finite()))
    });

    // encodeURIComponent (simplified)
    vm.register_native("encodeURIComponent", |args| {
        let text = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        let encoded: String = text
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || "-_.!~*'()".contains(c) {
                    c.to_string()
                } else {
                    format!("%{:02X}", c as u32)
                }
            })
            .collect();
        Ok(Value::String(encoded))
    });

    // decodeURIComponent (simplified)
    vm.register_native("decodeURIComponent", |args| {
        let text = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        let mut result = String::new();
        let mut chars = text.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '%' {
                let hex: String = chars.by_ref().take(2).collect();
                if let Ok(code) = u8::from_str_radix(&hex, 16) {
                    result.push(code as char);
                }
            } else {
                result.push(c);
            }
        }

        Ok(Value::String(result))
    });

    // Object.keys
    vm.register_native("Object_keys", |args| {
        let obj = args.first().cloned().unwrap_or(Value::Undefined);
        match obj {
            Value::Object(o) => {
                let o = o.borrow();
                let keys: Vec<Value> = o
                    .properties
                    .keys()
                    .map(|k| Value::String(k.clone()))
                    .collect();
                Ok(Value::new_array(keys))
            }
            _ => Ok(Value::new_array(vec![])),
        }
    });

    // Object.values
    vm.register_native("Object_values", |args| {
        let obj = args.first().cloned().unwrap_or(Value::Undefined);
        match obj {
            Value::Object(o) => {
                let o = o.borrow();
                let values: Vec<Value> = o.properties.values().cloned().collect();
                Ok(Value::new_array(values))
            }
            _ => Ok(Value::new_array(vec![])),
        }
    });

    // Object.entries
    vm.register_native("Object_entries", |args| {
        let obj = args.first().cloned().unwrap_or(Value::Undefined);
        match obj {
            Value::Object(o) => {
                let o = o.borrow();
                let entries: Vec<Value> = o
                    .properties
                    .iter()
                    .map(|(k, v)| {
                        Value::new_array(vec![Value::String(k.clone()), v.clone()])
                    })
                    .collect();
                Ok(Value::new_array(entries))
            }
            _ => Ok(Value::new_array(vec![])),
        }
    });

    // Object.assign
    vm.register_native("Object_assign", |args| {
        let target = args.first().cloned().unwrap_or(Value::new_object());
        if let Value::Object(target_obj) = &target {
            for source in args.iter().skip(1) {
                if let Value::Object(source_obj) = source {
                    let source_ref = source_obj.borrow();
                    let mut target_ref = target_obj.borrow_mut();
                    for (key, value) in &source_ref.properties {
                        target_ref.properties.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        Ok(target)
    });

    // Object.freeze (simple implementation - marks object but doesn't prevent writes in this impl)
    vm.register_native("Object_freeze", |args| {
        let obj = args.first().cloned().unwrap_or(Value::Undefined);
        Ok(obj)
    });

    // Object.create
    vm.register_native("Object_create", |args| {
        let proto = args.first().cloned();
        let obj = Value::new_object();
        if let Some(Value::Object(_)) = proto {
            // In a full impl we'd set the prototype
            // For now, just return a new empty object
        }
        Ok(obj)
    });

    // Object.defineProperty - simplified implementation (just sets property)
    vm.register_native("Object_defineProperty", |args| {
        let obj = args.first().cloned().unwrap_or(Value::Undefined);
        let prop = args.get(1).cloned().unwrap_or(Value::Undefined);
        let descriptor = args.get(2).cloned().unwrap_or(Value::Undefined);

        if let (Value::Object(target), Value::String(key)) = (&obj, &prop) {
            // Extract value from descriptor
            if let Value::Object(desc_obj) = &descriptor {
                let desc_ref = desc_obj.borrow();
                // Check for value property in descriptor
                if let Some(value) = desc_ref.get_property("value") {
                    target.borrow_mut().set_property(&key, value);
                }
                // Check for getter
                if let Some(getter) = desc_ref.get_property("get") {
                    // Store getter (simplified - would need proper get/set handling)
                    let getter_key = format!("__get_{}", key);
                    target.borrow_mut().set_property(&getter_key, getter);
                }
            }
        }
        Ok(obj)
    });

    // Object.getOwnPropertyDescriptor - simplified implementation
    vm.register_native("Object_getOwnPropertyDescriptor", |args| {
        let obj = args.first().cloned().unwrap_or(Value::Undefined);
        let prop = args.get(1).cloned().unwrap_or(Value::Undefined);

        if let (Value::Object(target), Value::String(key)) = (&obj, &prop) {
            let target_ref = target.borrow();
            if let Some(value) = target_ref.get_property(&key) {
                // Create a descriptor object
                let descriptor = Value::new_object();
                descriptor.set_property("value", value);
                descriptor.set_property("writable", Value::Boolean(true));
                descriptor.set_property("enumerable", Value::Boolean(true));
                descriptor.set_property("configurable", Value::Boolean(true));
                return Ok(descriptor);
            }
        }
        Ok(Value::Undefined)
    });

    // Object.getOwnPropertyDescriptors
    vm.register_native("Object_getOwnPropertyDescriptors", |args| {
        let obj = args.first().cloned().unwrap_or(Value::Undefined);

        if let Value::Object(target) = &obj {
            let target_ref = target.borrow();
            let result = Value::new_object();

            for (key, value) in &target_ref.properties {
                let descriptor = Value::new_object();
                descriptor.set_property("value", value.clone());
                descriptor.set_property("writable", Value::Boolean(true));
                descriptor.set_property("enumerable", Value::Boolean(true));
                descriptor.set_property("configurable", Value::Boolean(true));
                result.set_property(key, descriptor);
            }
            return Ok(result);
        }
        Ok(Value::new_object())
    });

    // Object.seal - simplified implementation
    vm.register_native("Object_seal", |args| {
        let obj = args.first().cloned().unwrap_or(Value::Undefined);
        // In a full implementation, this would mark properties as non-configurable
        // For now, just return the object
        Ok(obj)
    });

    // Object.is - SameValue comparison
    vm.register_native("Object_is", |args| {
        let val1 = args.first().cloned().unwrap_or(Value::Undefined);
        let val2 = args.get(1).cloned().unwrap_or(Value::Undefined);

        let result = match (&val1, &val2) {
            // Handle NaN (NaN === NaN in Object.is)
            (Value::Number(n1), Value::Number(n2)) => {
                if n1.is_nan() && n2.is_nan() {
                    true
                } else if *n1 == 0.0 && *n2 == 0.0 {
                    // Distinguish +0 from -0
                    n1.is_sign_positive() == n2.is_sign_positive()
                } else {
                    n1 == n2
                }
            }
            (Value::Undefined, Value::Undefined) => true,
            (Value::Null, Value::Null) => true,
            (Value::Boolean(b1), Value::Boolean(b2)) => b1 == b2,
            (Value::String(s1), Value::String(s2)) => s1 == s2,
            (Value::Object(o1), Value::Object(o2)) => std::rc::Rc::ptr_eq(o1, o2),
            (Value::Symbol(s1), Value::Symbol(s2)) => s1 == s2,
            (Value::BigInt(b1), Value::BigInt(b2)) => b1 == b2,
            _ => false,
        };
        Ok(Value::Boolean(result))
    });

    // Object.hasOwn (ES2022)
    vm.register_native("Object_hasOwn", |args| {
        let obj = args.first().cloned().unwrap_or(Value::Undefined);
        let prop = args.get(1).cloned().unwrap_or(Value::Undefined);

        if let (Value::Object(target), Value::String(key)) = (&obj, &prop) {
            let target_ref = target.borrow();
            return Ok(Value::Boolean(target_ref.properties.contains_key(&*key)));
        }
        Ok(Value::Boolean(false))
    });

    // Object.fromEntries
    vm.register_native("Object_fromEntries", |args| {
        let iterable = args.first().cloned().unwrap_or(Value::Undefined);
        let result = Value::new_object();

        if let Value::Object(obj) = &iterable {
            let obj_ref = obj.borrow();
            if let super::value::ObjectKind::Array(entries) = &obj_ref.kind {
                for entry in entries {
                    if let Value::Object(entry_obj) = entry {
                        let entry_ref = entry_obj.borrow();
                        if let super::value::ObjectKind::Array(pair) = &entry_ref.kind {
                            if pair.len() >= 2 {
                                let key = pair[0].to_js_string();
                                let value = pair[1].clone();
                                result.set_property(&key, value);
                            }
                        }
                    }
                }
            }
        }
        Ok(result)
    });

    // Array.isArray
    vm.register_native("Array_isArray", |args| {
        let obj = args.first().cloned().unwrap_or(Value::Undefined);
        match obj {
            Value::Object(o) => {
                let o = o.borrow();
                Ok(Value::Boolean(matches!(
                    o.kind,
                    super::value::ObjectKind::Array(_)
                )))
            }
            _ => Ok(Value::Boolean(false)),
        }
    });

    // Array.from
    vm.register_native("Array_from", |args| {
        let iterable = args.first().cloned().unwrap_or(Value::Undefined);
        let map_fn = args.get(1).cloned();

        match iterable {
            Value::String(s) => {
                let chars: Vec<Value> = s.chars().map(|c| Value::String(c.to_string())).collect();
                Ok(Value::new_array(chars))
            }
            Value::Object(o) => {
                let o = o.borrow();
                if let super::value::ObjectKind::Array(arr) = &o.kind {
                    if map_fn.is_some() {
                        // Map function support would need callback invocation
                        // For now, just clone
                        Ok(Value::new_array(arr.clone()))
                    } else {
                        Ok(Value::new_array(arr.clone()))
                    }
                } else {
                    // Convert object with length to array
                    if let Some(Value::Number(len)) = o.properties.get("length") {
                        let len = *len as usize;
                        let mut result = Vec::with_capacity(len);
                        for i in 0..len {
                            let val = o.properties.get(&i.to_string()).cloned().unwrap_or(Value::Undefined);
                            result.push(val);
                        }
                        Ok(Value::new_array(result))
                    } else {
                        Ok(Value::new_array(vec![]))
                    }
                }
            }
            _ => Ok(Value::new_array(vec![])),
        }
    });

    // Array.of
    vm.register_native("Array_of", |args| {
        Ok(Value::new_array(args.to_vec()))
    });

    // Create Object namespace
    let object = Value::new_object();
    object.set_property(
        "keys",
        vm.get_global("Object_keys").unwrap_or(Value::Undefined),
    );
    object.set_property(
        "values",
        vm.get_global("Object_values").unwrap_or(Value::Undefined),
    );
    object.set_property(
        "entries",
        vm.get_global("Object_entries").unwrap_or(Value::Undefined),
    );
    object.set_property(
        "assign",
        vm.get_global("Object_assign").unwrap_or(Value::Undefined),
    );
    object.set_property(
        "freeze",
        vm.get_global("Object_freeze").unwrap_or(Value::Undefined),
    );
    object.set_property(
        "create",
        vm.get_global("Object_create").unwrap_or(Value::Undefined),
    );
    object.set_property(
        "defineProperty",
        vm.get_global("Object_defineProperty").unwrap_or(Value::Undefined),
    );
    object.set_property(
        "getOwnPropertyDescriptor",
        vm.get_global("Object_getOwnPropertyDescriptor")
            .unwrap_or(Value::Undefined),
    );
    object.set_property(
        "getOwnPropertyDescriptors",
        vm.get_global("Object_getOwnPropertyDescriptors")
            .unwrap_or(Value::Undefined),
    );
    object.set_property(
        "seal",
        vm.get_global("Object_seal").unwrap_or(Value::Undefined),
    );
    object.set_property(
        "is",
        vm.get_global("Object_is").unwrap_or(Value::Undefined),
    );
    object.set_property(
        "hasOwn",
        vm.get_global("Object_hasOwn").unwrap_or(Value::Undefined),
    );
    object.set_property(
        "fromEntries",
        vm.get_global("Object_fromEntries").unwrap_or(Value::Undefined),
    );
    vm.set_global("Object", object);

    // Create Array namespace
    let array = Value::new_object();
    array.set_property(
        "isArray",
        vm.get_global("Array_isArray").unwrap_or(Value::Undefined),
    );
    array.set_property(
        "from",
        vm.get_global("Array_from").unwrap_or(Value::Undefined),
    );
    array.set_property(
        "of",
        vm.get_global("Array_of").unwrap_or(Value::Undefined),
    );
    vm.set_global("Array", array);

    // Infinity and NaN
    vm.set_global("Infinity", Value::Number(f64::INFINITY));
    vm.set_global("NaN", Value::Number(f64::NAN));
    vm.set_global("undefined", Value::Undefined);
}

/// Register WeakMap object
fn register_weakmap(vm: &mut VM) {
    use std::cell::RefCell;
    use rustc_hash::FxHashMap as HashMap;
    use std::rc::Rc;

    // WeakMap constructor
    vm.register_native("WeakMap_constructor", |_args| {
        // Create a new WeakMap - initial entries not supported for simplicity
        let weakmap = Value::Object(Rc::new(RefCell::new(super::value::Object {
            kind: super::value::ObjectKind::WeakMap(Vec::new()),
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        })));

        // Attach methods
        attach_weakmap_methods(&weakmap);

        Ok(weakmap)
    });

    // Create WeakMap constructor object
    let weakmap = Value::new_object();
    vm.set_global("WeakMap", weakmap);
    vm.set_global("__WeakMap_constructor", vm.get_global("WeakMap_constructor").unwrap_or(Value::Undefined));
}

/// Attach methods to a WeakMap instance
fn attach_weakmap_methods(weakmap: &Value) {
    use std::cell::RefCell;
    use rustc_hash::FxHashMap as HashMap;
    use std::rc::Rc;

    if let Value::Object(obj) = weakmap {
        let obj_clone = Rc::clone(obj);

        // WeakMap.prototype.get(key)
        let get_clone = Rc::clone(&obj_clone);
        let get_fn: super::value::NativeFn = Rc::new(move |args| {
            let key = args.first().cloned().unwrap_or(Value::Undefined);

            // WeakMap only accepts objects as keys
            if let Value::Object(key_obj) = &key {
                let mut obj_ref = get_clone.borrow_mut();
                if let super::value::ObjectKind::WeakMap(entries) = &mut obj_ref.kind {
                    // Clean up any dead references
                    entries.retain(|(k, _)| k.upgrade().is_some());

                    // Find the entry
                    for (weak_key, value) in entries.iter() {
                        if let Some(strong_key) = weak_key.upgrade() {
                            if Rc::ptr_eq(&strong_key, key_obj) {
                                return Ok(value.clone());
                            }
                        }
                    }
                }
            }
            Ok(Value::Undefined)
        });

        // WeakMap.prototype.set(key, value)
        let set_clone = Rc::clone(&obj_clone);
        let set_fn: super::value::NativeFn = Rc::new(move |args| {
            let key = args.first().cloned().unwrap_or(Value::Undefined);
            let value = args.get(1).cloned().unwrap_or(Value::Undefined);

            // WeakMap only accepts objects as keys
            if let Value::Object(key_obj) = &key {
                let mut obj_ref = set_clone.borrow_mut();
                if let super::value::ObjectKind::WeakMap(entries) = &mut obj_ref.kind {
                    // Clean up dead references
                    entries.retain(|(k, _)| k.upgrade().is_some());

                    // Check if key already exists
                    for (weak_key, existing_value) in entries.iter_mut() {
                        if let Some(strong_key) = weak_key.upgrade() {
                            if Rc::ptr_eq(&strong_key, key_obj) {
                                *existing_value = value.clone();
                                return Ok(Value::Object(set_clone.clone()));
                            }
                        }
                    }

                    // Add new entry
                    entries.push((Rc::downgrade(key_obj), value));
                }
                Ok(Value::Object(set_clone.clone()))
            } else {
                Err(crate::error::Error::type_error("Invalid value used as weak map key"))
            }
        });

        // WeakMap.prototype.has(key)
        let has_clone = Rc::clone(&obj_clone);
        let has_fn: super::value::NativeFn = Rc::new(move |args| {
            let key = args.first().cloned().unwrap_or(Value::Undefined);

            if let Value::Object(key_obj) = &key {
                let mut obj_ref = has_clone.borrow_mut();
                if let super::value::ObjectKind::WeakMap(entries) = &mut obj_ref.kind {
                    entries.retain(|(k, _)| k.upgrade().is_some());

                    for (weak_key, _) in entries.iter() {
                        if let Some(strong_key) = weak_key.upgrade() {
                            if Rc::ptr_eq(&strong_key, key_obj) {
                                return Ok(Value::Boolean(true));
                            }
                        }
                    }
                }
            }
            Ok(Value::Boolean(false))
        });

        // WeakMap.prototype.delete(key)
        let delete_clone = Rc::clone(&obj_clone);
        let delete_fn: super::value::NativeFn = Rc::new(move |args| {
            let key = args.first().cloned().unwrap_or(Value::Undefined);

            if let Value::Object(key_obj) = &key {
                let mut obj_ref = delete_clone.borrow_mut();
                if let super::value::ObjectKind::WeakMap(entries) = &mut obj_ref.kind {
                    let original_len = entries.len();
                    entries.retain(|(k, _)| {
                        if let Some(strong_key) = k.upgrade() {
                            !Rc::ptr_eq(&strong_key, key_obj)
                        } else {
                            false // Remove dead refs too
                        }
                    });
                    return Ok(Value::Boolean(entries.len() < original_len));
                }
            }
            Ok(Value::Boolean(false))
        });

        // Attach methods
        let mut obj_ref = obj.borrow_mut();
        obj_ref.set_property("get", Value::Object(Rc::new(RefCell::new(super::value::Object {
            kind: super::value::ObjectKind::NativeFunction { name: "get".to_string(), func: get_fn },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        }))));
        obj_ref.set_property("set", Value::Object(Rc::new(RefCell::new(super::value::Object {
            kind: super::value::ObjectKind::NativeFunction { name: "set".to_string(), func: set_fn },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        }))));
        obj_ref.set_property("has", Value::Object(Rc::new(RefCell::new(super::value::Object {
            kind: super::value::ObjectKind::NativeFunction { name: "has".to_string(), func: has_fn },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        }))));
        obj_ref.set_property("delete", Value::Object(Rc::new(RefCell::new(super::value::Object {
            kind: super::value::ObjectKind::NativeFunction { name: "delete".to_string(), func: delete_fn },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        }))));
    }
}

/// Register WeakSet object
fn register_weakset(vm: &mut VM) {
    use std::cell::RefCell;
    use rustc_hash::FxHashMap as HashMap;
    use std::rc::Rc;

    // WeakSet constructor
    vm.register_native("WeakSet_constructor", |_args| {
        let weakset = Value::Object(Rc::new(RefCell::new(super::value::Object {
            kind: super::value::ObjectKind::WeakSet(Vec::new()),
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        })));

        attach_weakset_methods(&weakset);

        Ok(weakset)
    });

    // Create WeakSet constructor object
    let weakset = Value::new_object();
    vm.set_global("WeakSet", weakset);
    vm.set_global("__WeakSet_constructor", vm.get_global("WeakSet_constructor").unwrap_or(Value::Undefined));
}

/// Attach methods to a WeakSet instance
fn attach_weakset_methods(weakset: &Value) {
    use std::cell::RefCell;
    use rustc_hash::FxHashMap as HashMap;
    use std::rc::Rc;

    if let Value::Object(obj) = weakset {
        let obj_clone = Rc::clone(obj);

        // WeakSet.prototype.add(value)
        let add_clone = Rc::clone(&obj_clone);
        let add_fn: super::value::NativeFn = Rc::new(move |args| {
            let value = args.first().cloned().unwrap_or(Value::Undefined);

            if let Value::Object(value_obj) = &value {
                let mut obj_ref = add_clone.borrow_mut();
                if let super::value::ObjectKind::WeakSet(items) = &mut obj_ref.kind {
                    // Clean up dead refs
                    items.retain(|w| w.upgrade().is_some());

                    // Check if already exists
                    let exists = items.iter().any(|w| {
                        if let Some(strong) = w.upgrade() {
                            Rc::ptr_eq(&strong, value_obj)
                        } else {
                            false
                        }
                    });

                    if !exists {
                        items.push(Rc::downgrade(value_obj));
                    }
                }
                Ok(Value::Object(add_clone.clone()))
            } else {
                Err(crate::error::Error::type_error("Invalid value used in weak set"))
            }
        });

        // WeakSet.prototype.has(value)
        let has_clone = Rc::clone(&obj_clone);
        let has_fn: super::value::NativeFn = Rc::new(move |args| {
            let value = args.first().cloned().unwrap_or(Value::Undefined);

            if let Value::Object(value_obj) = &value {
                let mut obj_ref = has_clone.borrow_mut();
                if let super::value::ObjectKind::WeakSet(items) = &mut obj_ref.kind {
                    items.retain(|w| w.upgrade().is_some());

                    for weak in items.iter() {
                        if let Some(strong) = weak.upgrade() {
                            if Rc::ptr_eq(&strong, value_obj) {
                                return Ok(Value::Boolean(true));
                            }
                        }
                    }
                }
            }
            Ok(Value::Boolean(false))
        });

        // WeakSet.prototype.delete(value)
        let delete_clone = Rc::clone(&obj_clone);
        let delete_fn: super::value::NativeFn = Rc::new(move |args| {
            let value = args.first().cloned().unwrap_or(Value::Undefined);

            if let Value::Object(value_obj) = &value {
                let mut obj_ref = delete_clone.borrow_mut();
                if let super::value::ObjectKind::WeakSet(items) = &mut obj_ref.kind {
                    let original_len = items.len();
                    items.retain(|w| {
                        if let Some(strong) = w.upgrade() {
                            !Rc::ptr_eq(&strong, value_obj)
                        } else {
                            false
                        }
                    });
                    return Ok(Value::Boolean(items.len() < original_len));
                }
            }
            Ok(Value::Boolean(false))
        });

        // Attach methods
        let mut obj_ref = obj.borrow_mut();
        obj_ref.set_property("add", Value::Object(Rc::new(RefCell::new(super::value::Object {
            kind: super::value::ObjectKind::NativeFunction { name: "add".to_string(), func: add_fn },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        }))));
        obj_ref.set_property("has", Value::Object(Rc::new(RefCell::new(super::value::Object {
            kind: super::value::ObjectKind::NativeFunction { name: "has".to_string(), func: has_fn },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        }))));
        obj_ref.set_property("delete", Value::Object(Rc::new(RefCell::new(super::value::Object {
            kind: super::value::ObjectKind::NativeFunction { name: "delete".to_string(), func: delete_fn },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        }))));
    }
}

/// Register RegExp object
fn register_regexp(vm: &mut VM) {
    // RegExp constructor
    vm.register_native("RegExp_constructor", |args| {
        let pattern = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        let flags = args.get(1).map(|v| v.to_js_string()).unwrap_or_default();

        create_regexp(&pattern, &flags)
    });

    // Create RegExp constructor object
    let regexp = Value::new_object();
    vm.set_global("RegExp", regexp);
    vm.set_global("__RegExp_constructor", vm.get_global("RegExp_constructor").unwrap_or(Value::Undefined));
}

/// Convert JavaScript regex flags to Rust regex pattern
fn js_to_rust_regex(pattern: &str, flags: &str) -> Result<regex::Regex> {
    use regex::RegexBuilder;

    let case_insensitive = flags.contains('i');
    let multiline = flags.contains('m');
    let dotall = flags.contains('s');

    RegexBuilder::new(pattern)
        .case_insensitive(case_insensitive)
        .multi_line(multiline)
        .dot_matches_new_line(dotall)
        .build()
        .map_err(|e| crate::error::Error::syntax_error(format!("Invalid regular expression: {}", e)))
}

/// Create a RegExp object
fn create_regexp(pattern: &str, flags: &str) -> Result<Value> {
    use std::cell::RefCell;
    use rustc_hash::FxHashMap as HashMap;
    use std::rc::Rc;

    let regex = js_to_rust_regex(pattern, flags)?;

    let regexp = Rc::new(RefCell::new(super::value::Object {
        kind: super::value::ObjectKind::RegExp {
            pattern: pattern.to_string(),
            flags: flags.to_string(),
            regex,
            last_index: 0,
        },
        properties: HashMap::default(),
        private_fields: HashMap::default(),
        prototype: None,
    }));

    // Attach properties
    {
        let mut obj_ref = regexp.borrow_mut();
        obj_ref.set_property("source", Value::String(pattern.to_string()));
        obj_ref.set_property("flags", Value::String(flags.to_string()));
        obj_ref.set_property("global", Value::Boolean(flags.contains('g')));
        obj_ref.set_property("ignoreCase", Value::Boolean(flags.contains('i')));
        obj_ref.set_property("multiline", Value::Boolean(flags.contains('m')));
        obj_ref.set_property("dotAll", Value::Boolean(flags.contains('s')));
        obj_ref.set_property("unicode", Value::Boolean(flags.contains('u')));
        obj_ref.set_property("sticky", Value::Boolean(flags.contains('y')));
        obj_ref.set_property("lastIndex", Value::Number(0.0));
    }

    // Attach methods
    attach_regexp_methods(&Value::Object(regexp.clone()));

    Ok(Value::Object(regexp))
}

/// Attach methods to a RegExp instance
fn attach_regexp_methods(regexp: &Value) {
    use std::cell::RefCell;
    use rustc_hash::FxHashMap as HashMap;
    use std::rc::Rc;

    if let Value::Object(obj) = regexp {
        let obj_clone = Rc::clone(obj);

        // RegExp.prototype.test(string)
        let test_clone = Rc::clone(&obj_clone);
        let test_fn: super::value::NativeFn = Rc::new(move |args| {
            let input = args.first().map(|v| v.to_js_string()).unwrap_or_default();

            let obj_ref = test_clone.borrow();
            if let super::value::ObjectKind::RegExp { regex, .. } = &obj_ref.kind {
                Ok(Value::Boolean(regex.is_match(&input)))
            } else {
                Err(crate::error::Error::type_error("Not a RegExp"))
            }
        });

        // RegExp.prototype.exec(string)
        let exec_clone = Rc::clone(&obj_clone);
        let exec_fn: super::value::NativeFn = Rc::new(move |args| {
            let input = args.first().map(|v| v.to_js_string()).unwrap_or_default();

            let obj_ref = exec_clone.borrow();
            if let super::value::ObjectKind::RegExp { regex, flags, .. } = &obj_ref.kind {
                let is_global = flags.contains('g');
                let last_index = if is_global {
                    obj_ref.get_property("lastIndex")
                        .map(|v| v.to_number() as usize)
                        .unwrap_or(0)
                } else {
                    0
                };

                // Search from last_index
                let search_string = if last_index < input.len() {
                    &input[last_index..]
                } else {
                    ""
                };

                if let Some(mat) = regex.find(search_string) {
                    // Create result array
                    let result = Value::new_array(vec![Value::String(mat.as_str().to_string())]);

                    // Set index property
                    result.set_property("index", Value::Number((last_index + mat.start()) as f64));
                    result.set_property("input", Value::String(input.clone()));

                    // Update lastIndex for global regexes
                    if is_global {
                        drop(obj_ref);
                        let mut obj_mut = exec_clone.borrow_mut();
                        obj_mut.set_property("lastIndex", Value::Number((last_index + mat.end()) as f64));
                    }

                    Ok(result)
                } else {
                    // No match - reset lastIndex for global
                    if is_global {
                        drop(obj_ref);
                        let mut obj_mut = exec_clone.borrow_mut();
                        obj_mut.set_property("lastIndex", Value::Number(0.0));
                    }
                    Ok(Value::Null)
                }
            } else {
                Err(crate::error::Error::type_error("Not a RegExp"))
            }
        });

        // RegExp.prototype.toString()
        let tostring_clone = Rc::clone(&obj_clone);
        let tostring_fn: super::value::NativeFn = Rc::new(move |_args| {
            let obj_ref = tostring_clone.borrow();
            if let super::value::ObjectKind::RegExp { pattern, flags, .. } = &obj_ref.kind {
                Ok(Value::String(format!("/{}/{}", pattern, flags)))
            } else {
                Err(crate::error::Error::type_error("Not a RegExp"))
            }
        });

        // Attach methods
        let mut obj_ref = obj.borrow_mut();
        obj_ref.set_property("test", Value::Object(Rc::new(RefCell::new(super::value::Object {
            kind: super::value::ObjectKind::NativeFunction { name: "test".to_string(), func: test_fn },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        }))));
        obj_ref.set_property("exec", Value::Object(Rc::new(RefCell::new(super::value::Object {
            kind: super::value::ObjectKind::NativeFunction { name: "exec".to_string(), func: exec_fn },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        }))));
        obj_ref.set_property("toString", Value::Object(Rc::new(RefCell::new(super::value::Object {
            kind: super::value::ObjectKind::NativeFunction { name: "toString".to_string(), func: tostring_fn },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        }))));
    }
}

/// Register Proxy constructor
fn register_proxy(vm: &mut VM) {
    use std::cell::RefCell;
    use rustc_hash::FxHashMap as HashMap;
    use std::rc::Rc;

    // Proxy constructor
    vm.register_native("Proxy_constructor", |args| {
        let target = args.first().cloned().unwrap_or(Value::Undefined);
        let handler = args.get(1).cloned().unwrap_or(Value::Undefined);

        // Both target and handler must be objects
        if !matches!(&target, Value::Object(_)) {
            return Err(crate::error::Error::type_error("Proxy target must be an object"));
        }
        if !matches!(&handler, Value::Object(_)) {
            return Err(crate::error::Error::type_error("Proxy handler must be an object"));
        }

        let proxy = Value::Object(Rc::new(RefCell::new(super::value::Object {
            kind: super::value::ObjectKind::Proxy {
                target: Box::new(target),
                handler: Box::new(handler),
                revoked: false,
            },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        })));

        Ok(proxy)
    });

    // Proxy.revocable(target, handler)
    vm.register_native("Proxy_revocable", |args| {
        let target = args.first().cloned().unwrap_or(Value::Undefined);
        let handler = args.get(1).cloned().unwrap_or(Value::Undefined);

        // Both target and handler must be objects
        if !matches!(&target, Value::Object(_)) {
            return Err(crate::error::Error::type_error("Proxy target must be an object"));
        }
        if !matches!(&handler, Value::Object(_)) {
            return Err(crate::error::Error::type_error("Proxy handler must be an object"));
        }

        let proxy_obj = Rc::new(RefCell::new(super::value::Object {
            kind: super::value::ObjectKind::Proxy {
                target: Box::new(target),
                handler: Box::new(handler),
                revoked: false,
            },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        }));

        let proxy = Value::Object(proxy_obj.clone());

        // Create revoke function
        let revoke_fn: super::value::NativeFn = Rc::new(move |_| {
            let mut obj_ref = proxy_obj.borrow_mut();
            if let super::value::ObjectKind::Proxy { revoked, .. } = &mut obj_ref.kind {
                *revoked = true;
            }
            Ok(Value::Undefined)
        });

        // Create result object { proxy, revoke }
        let result = Value::new_object();
        result.set_property("proxy", proxy);
        result.set_property("revoke", Value::Object(Rc::new(RefCell::new(super::value::Object {
            kind: super::value::ObjectKind::NativeFunction { name: "revoke".to_string(), func: revoke_fn },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        }))));

        Ok(result)
    });

    // Create Proxy constructor object
    let proxy = Value::new_object();
    proxy.set_property("revocable", vm.get_global("Proxy_revocable").unwrap_or(Value::Undefined));
    vm.set_global("Proxy", proxy);
    vm.set_global("__Proxy_constructor", vm.get_global("Proxy_constructor").unwrap_or(Value::Undefined));
}

/// Register Reflect object
fn register_reflect(vm: &mut VM) {
    // Reflect.get(target, propertyKey)
    vm.register_native("Reflect_get", |args| {
        let target = args.first().cloned().unwrap_or(Value::Undefined);
        let key = args.get(1).map(|v| v.to_js_string()).unwrap_or_default();

        if let Value::Object(obj) = &target {
            let obj_ref = obj.borrow();
            Ok(obj_ref.get_property(&key).unwrap_or(Value::Undefined))
        } else {
            Err(crate::error::Error::type_error("Reflect.get requires object target"))
        }
    });

    // Reflect.set(target, propertyKey, value)
    vm.register_native("Reflect_set", |args| {
        let target = args.first().cloned().unwrap_or(Value::Undefined);
        let key = args.get(1).map(|v| v.to_js_string()).unwrap_or_default();
        let value = args.get(2).cloned().unwrap_or(Value::Undefined);

        if let Value::Object(obj) = &target {
            let mut obj_ref = obj.borrow_mut();
            obj_ref.set_property(&key, value);
            Ok(Value::Boolean(true))
        } else {
            Ok(Value::Boolean(false))
        }
    });

    // Reflect.has(target, propertyKey)
    vm.register_native("Reflect_has", |args| {
        let target = args.first().cloned().unwrap_or(Value::Undefined);
        let key = args.get(1).map(|v| v.to_js_string()).unwrap_or_default();

        if let Value::Object(obj) = &target {
            let obj_ref = obj.borrow();
            Ok(Value::Boolean(obj_ref.get_property(&key).is_some()))
        } else {
            Ok(Value::Boolean(false))
        }
    });

    // Reflect.deleteProperty(target, propertyKey)
    vm.register_native("Reflect_deleteProperty", |args| {
        let target = args.first().cloned().unwrap_or(Value::Undefined);
        let key = args.get(1).map(|v| v.to_js_string()).unwrap_or_default();

        if let Value::Object(obj) = &target {
            let mut obj_ref = obj.borrow_mut();
            let existed = obj_ref.properties.remove(&key).is_some();
            Ok(Value::Boolean(existed))
        } else {
            Ok(Value::Boolean(false))
        }
    });

    // Reflect.ownKeys(target)
    vm.register_native("Reflect_ownKeys", |args| {
        let target = args.first().cloned().unwrap_or(Value::Undefined);

        if let Value::Object(obj) = &target {
            let obj_ref = obj.borrow();
            let keys: Vec<Value> = obj_ref.properties.keys()
                .map(|k| Value::String(k.clone()))
                .collect();
            Ok(Value::new_array(keys))
        } else {
            Err(crate::error::Error::type_error("Reflect.ownKeys requires object target"))
        }
    });

    // Reflect.getPrototypeOf(target)
    vm.register_native("Reflect_getPrototypeOf", |args| {
        let target = args.first().cloned().unwrap_or(Value::Undefined);

        if let Value::Object(obj) = &target {
            let obj_ref = obj.borrow();
            match &obj_ref.prototype {
                Some(proto) => Ok(Value::Object(proto.clone())),
                None => Ok(Value::Null),
            }
        } else {
            Err(crate::error::Error::type_error("Reflect.getPrototypeOf requires object target"))
        }
    });

    // Reflect.setPrototypeOf(target, prototype)
    vm.register_native("Reflect_setPrototypeOf", |args| {
        let target = args.first().cloned().unwrap_or(Value::Undefined);
        let proto = args.get(1).cloned();

        if let Value::Object(obj) = &target {
            let mut obj_ref = obj.borrow_mut();
            // Convert Value to Option<Rc<RefCell<Object>>>
            obj_ref.prototype = match proto {
                Some(Value::Object(o)) => Some(o),
                _ => None,
            };
            Ok(Value::Boolean(true))
        } else {
            Ok(Value::Boolean(false))
        }
    });

    // Reflect.apply(target, thisArgument, argumentsList)
    vm.register_native("Reflect_apply", |_args| {
        // Note: Full implementation requires VM access to call functions
        // For now, return undefined (proper implementation needs more integration)
        Ok(Value::Undefined)
    });

    // Reflect.construct(target, argumentsList)
    vm.register_native("Reflect_construct", |_args| {
        // Note: Full implementation requires VM access
        Ok(Value::Undefined)
    });

    // Create Reflect object
    let reflect = Value::new_object();
    reflect.set_property("get", vm.get_global("Reflect_get").unwrap_or(Value::Undefined));
    reflect.set_property("set", vm.get_global("Reflect_set").unwrap_or(Value::Undefined));
    reflect.set_property("has", vm.get_global("Reflect_has").unwrap_or(Value::Undefined));
    reflect.set_property("deleteProperty", vm.get_global("Reflect_deleteProperty").unwrap_or(Value::Undefined));
    reflect.set_property("ownKeys", vm.get_global("Reflect_ownKeys").unwrap_or(Value::Undefined));
    reflect.set_property("getPrototypeOf", vm.get_global("Reflect_getPrototypeOf").unwrap_or(Value::Undefined));
    reflect.set_property("setPrototypeOf", vm.get_global("Reflect_setPrototypeOf").unwrap_or(Value::Undefined));
    reflect.set_property("apply", vm.get_global("Reflect_apply").unwrap_or(Value::Undefined));
    reflect.set_property("construct", vm.get_global("Reflect_construct").unwrap_or(Value::Undefined));
    vm.set_global("Reflect", reflect);
}

/// Register ArrayBuffer and TypedArray builtins
fn register_typed_arrays(vm: &mut VM) {
    // ArrayBuffer constructor
    vm.register_native("ArrayBuffer_constructor", |args| {
        let byte_length = args.first()
            .map(|v| v.to_number() as usize)
            .unwrap_or(0);
        Ok(Value::new_array_buffer(byte_length))
    });

    // ArrayBuffer.isView
    vm.register_native("ArrayBuffer_isView", |args| {
        let is_view = args.first().map(|v| {
            if let Value::Object(obj) = v {
                let obj_ref = obj.borrow();
                matches!(obj_ref.kind, ObjectKind::TypedArray { .. } | ObjectKind::DataView { .. })
            } else {
                false
            }
        }).unwrap_or(false);
        Ok(Value::Boolean(is_view))
    });

    // ArrayBuffer.prototype.slice
    vm.register_native("ArrayBuffer_slice", |args| {
        let this = args.first().cloned().unwrap_or(Value::Undefined);
        if let Value::Object(obj) = &this {
            let obj_ref = obj.borrow();
            if let ObjectKind::ArrayBuffer(buffer) = &obj_ref.kind {
                let buf = buffer.borrow();
                let len = buf.len() as i64;

                let start = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
                let end = args.get(2).map(|v| v.to_number() as i64).unwrap_or(len);

                // Handle negative indices
                let start = if start < 0 { (len + start).max(0) } else { start.min(len) } as usize;
                let end = if end < 0 { (len + end).max(0) } else { end.min(len) } as usize;

                if start >= end {
                    return Ok(Value::new_array_buffer(0));
                }

                let new_buffer = Rc::new(RefCell::new(buf[start..end].to_vec()));
                return Ok(Value::Object(Rc::new(RefCell::new(super::value::Object {
                    kind: ObjectKind::ArrayBuffer(new_buffer),
                    properties: std::collections::HashMap::default(),
                    private_fields: HashMap::default(),
                    prototype: None,
                }))));
            }
        }
        Ok(Value::Undefined)
    });

    // Create ArrayBuffer constructor object
    let array_buffer = Value::new_object();
    array_buffer.set_property("isView", vm.get_global("ArrayBuffer_isView").unwrap_or(Value::Undefined));
    vm.set_global("ArrayBuffer", array_buffer);
    vm.set_global("__ArrayBuffer_constructor", vm.get_global("ArrayBuffer_constructor").unwrap_or(Value::Undefined));

    // Helper to create TypedArray constructor
    fn create_typed_array_constructor(vm: &mut VM, name: &str, kind: TypedArrayKind) {
        let constructor_name = format!("{}_constructor", name);
        let kind_clone = kind;

        vm.register_native(&constructor_name, move |args| {
            let arg = args.first().cloned().unwrap_or(Value::Undefined);

            match &arg {
                // new TypedArray(length)
                Value::Number(n) => {
                    let length = *n as usize;
                    Ok(Value::new_typed_array_with_length(kind_clone, length))
                }
                // new TypedArray(typedArray) - copy from another TypedArray
                Value::Object(obj) => {
                    let obj_ref = obj.borrow();
                    match &obj_ref.kind {
                        ObjectKind::TypedArray { buffer, kind: src_kind, byte_offset, length } => {
                            // Copy elements
                            let src_buf = buffer.borrow();
                            let new_len = *length;
                            let new_byte_len = new_len * kind_clone.bytes_per_element();
                            let mut new_buf = vec![0u8; new_byte_len];

                            // Copy element by element, converting types
                            for i in 0..new_len {
                                let src_elem_size = src_kind.bytes_per_element();
                                let src_offset = byte_offset + i * src_elem_size;
                                let value = if src_offset + src_elem_size <= src_buf.len() {
                                    match src_kind {
                                        TypedArrayKind::Int8 => src_buf[src_offset] as i8 as f64,
                                        TypedArrayKind::Uint8 | TypedArrayKind::Uint8Clamped => src_buf[src_offset] as f64,
                                        TypedArrayKind::Int16 => {
                                            i16::from_le_bytes([src_buf[src_offset], src_buf[src_offset + 1]]) as f64
                                        }
                                        TypedArrayKind::Uint16 => {
                                            u16::from_le_bytes([src_buf[src_offset], src_buf[src_offset + 1]]) as f64
                                        }
                                        TypedArrayKind::Int32 => {
                                            i32::from_le_bytes([src_buf[src_offset], src_buf[src_offset + 1], src_buf[src_offset + 2], src_buf[src_offset + 3]]) as f64
                                        }
                                        TypedArrayKind::Uint32 => {
                                            u32::from_le_bytes([src_buf[src_offset], src_buf[src_offset + 1], src_buf[src_offset + 2], src_buf[src_offset + 3]]) as f64
                                        }
                                        TypedArrayKind::Float32 => {
                                            f32::from_le_bytes([src_buf[src_offset], src_buf[src_offset + 1], src_buf[src_offset + 2], src_buf[src_offset + 3]]) as f64
                                        }
                                        TypedArrayKind::Float64 => {
                                            f64::from_le_bytes([
                                                src_buf[src_offset], src_buf[src_offset + 1], src_buf[src_offset + 2], src_buf[src_offset + 3],
                                                src_buf[src_offset + 4], src_buf[src_offset + 5], src_buf[src_offset + 6], src_buf[src_offset + 7],
                                            ])
                                        }
                                    }
                                } else {
                                    0.0
                                };

                                // Write to new buffer
                                let dst_elem_size = kind_clone.bytes_per_element();
                                let dst_offset = i * dst_elem_size;
                                match kind_clone {
                                    TypedArrayKind::Int8 => {
                                        new_buf[dst_offset] = value as i8 as u8;
                                    }
                                    TypedArrayKind::Uint8 => {
                                        new_buf[dst_offset] = value as u8;
                                    }
                                    TypedArrayKind::Uint8Clamped => {
                                        let clamped = if value < 0.0 { 0u8 }
                                            else if value > 255.0 { 255u8 }
                                            else { value.round() as u8 };
                                        new_buf[dst_offset] = clamped;
                                    }
                                    TypedArrayKind::Int16 => {
                                        let bytes = (value as i16).to_le_bytes();
                                        new_buf[dst_offset..dst_offset + 2].copy_from_slice(&bytes);
                                    }
                                    TypedArrayKind::Uint16 => {
                                        let bytes = (value as u16).to_le_bytes();
                                        new_buf[dst_offset..dst_offset + 2].copy_from_slice(&bytes);
                                    }
                                    TypedArrayKind::Int32 => {
                                        let bytes = (value as i32).to_le_bytes();
                                        new_buf[dst_offset..dst_offset + 4].copy_from_slice(&bytes);
                                    }
                                    TypedArrayKind::Uint32 => {
                                        let bytes = (value as u32).to_le_bytes();
                                        new_buf[dst_offset..dst_offset + 4].copy_from_slice(&bytes);
                                    }
                                    TypedArrayKind::Float32 => {
                                        let bytes = (value as f32).to_le_bytes();
                                        new_buf[dst_offset..dst_offset + 4].copy_from_slice(&bytes);
                                    }
                                    TypedArrayKind::Float64 => {
                                        let bytes = value.to_le_bytes();
                                        new_buf[dst_offset..dst_offset + 8].copy_from_slice(&bytes);
                                    }
                                }
                            }

                            let buffer = Rc::new(RefCell::new(new_buf));
                            Ok(Value::new_typed_array(buffer, kind_clone, 0, new_len))
                        }
                        // new TypedArray(array) - from regular array
                        ObjectKind::Array(arr) => {
                            let length = arr.len();
                            let byte_length = length * kind_clone.bytes_per_element();
                            let mut buf = vec![0u8; byte_length];

                            for (i, val) in arr.iter().enumerate() {
                                let num = val.to_number();
                                let elem_size = kind_clone.bytes_per_element();
                                let offset = i * elem_size;

                                match kind_clone {
                                    TypedArrayKind::Int8 => {
                                        buf[offset] = num as i8 as u8;
                                    }
                                    TypedArrayKind::Uint8 => {
                                        buf[offset] = num as u8;
                                    }
                                    TypedArrayKind::Uint8Clamped => {
                                        let clamped = if num < 0.0 { 0u8 }
                                            else if num > 255.0 { 255u8 }
                                            else { num.round() as u8 };
                                        buf[offset] = clamped;
                                    }
                                    TypedArrayKind::Int16 => {
                                        let bytes = (num as i16).to_le_bytes();
                                        buf[offset..offset + 2].copy_from_slice(&bytes);
                                    }
                                    TypedArrayKind::Uint16 => {
                                        let bytes = (num as u16).to_le_bytes();
                                        buf[offset..offset + 2].copy_from_slice(&bytes);
                                    }
                                    TypedArrayKind::Int32 => {
                                        let bytes = (num as i32).to_le_bytes();
                                        buf[offset..offset + 4].copy_from_slice(&bytes);
                                    }
                                    TypedArrayKind::Uint32 => {
                                        let bytes = (num as u32).to_le_bytes();
                                        buf[offset..offset + 4].copy_from_slice(&bytes);
                                    }
                                    TypedArrayKind::Float32 => {
                                        let bytes = (num as f32).to_le_bytes();
                                        buf[offset..offset + 4].copy_from_slice(&bytes);
                                    }
                                    TypedArrayKind::Float64 => {
                                        let bytes = num.to_le_bytes();
                                        buf[offset..offset + 8].copy_from_slice(&bytes);
                                    }
                                }
                            }

                            let buffer = Rc::new(RefCell::new(buf));
                            Ok(Value::new_typed_array(buffer, kind_clone, 0, length))
                        }
                        // new TypedArray(arrayBuffer, byteOffset?, length?)
                        ObjectKind::ArrayBuffer(buffer) => {
                            let buf_len = buffer.borrow().len();
                            let byte_offset = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
                            let elem_size = kind_clone.bytes_per_element();

                            let length = if let Some(len_arg) = args.get(2) {
                                len_arg.to_number() as usize
                            } else {
                                (buf_len - byte_offset) / elem_size
                            };

                            Ok(Value::new_typed_array(buffer.clone(), kind_clone, byte_offset, length))
                        }
                        _ => Ok(Value::new_typed_array_with_length(kind_clone, 0))
                    }
                }
                // new TypedArray() - empty
                _ => Ok(Value::new_typed_array_with_length(kind_clone, 0))
            }
        });

        // Create constructor object with BYTES_PER_ELEMENT
        let constructor = Value::new_object();
        constructor.set_property("BYTES_PER_ELEMENT", Value::Number(kind.bytes_per_element() as f64));
        vm.set_global(name, constructor);

        // Register the internal constructor for `new` calls
        let internal_name = format!("__{}_constructor", name);
        vm.set_global(&internal_name, vm.get_global(&constructor_name).unwrap_or(Value::Undefined));
    }

    // Register all TypedArray constructors
    create_typed_array_constructor(vm, "Int8Array", TypedArrayKind::Int8);
    create_typed_array_constructor(vm, "Uint8Array", TypedArrayKind::Uint8);
    create_typed_array_constructor(vm, "Uint8ClampedArray", TypedArrayKind::Uint8Clamped);
    create_typed_array_constructor(vm, "Int16Array", TypedArrayKind::Int16);
    create_typed_array_constructor(vm, "Uint16Array", TypedArrayKind::Uint16);
    create_typed_array_constructor(vm, "Int32Array", TypedArrayKind::Int32);
    create_typed_array_constructor(vm, "Uint32Array", TypedArrayKind::Uint32);
    create_typed_array_constructor(vm, "Float32Array", TypedArrayKind::Float32);
    create_typed_array_constructor(vm, "Float64Array", TypedArrayKind::Float64);

    // DataView constructor
    vm.register_native("DataView_constructor", |args| {
        let buffer_arg = args.first().cloned().unwrap_or(Value::Undefined);
        if let Value::Object(obj) = &buffer_arg {
            let obj_ref = obj.borrow();
            if let ObjectKind::ArrayBuffer(buffer) = &obj_ref.kind {
                let buf_len = buffer.borrow().len();
                let byte_offset = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
                let byte_length = args.get(2).map(|v| v.to_number() as usize).unwrap_or(buf_len - byte_offset);
                return Ok(Value::new_data_view(buffer.clone(), byte_offset, byte_length));
            }
        }
        Ok(Value::Undefined)
    });

    // DataView getter methods
    vm.register_native("DataView_getInt8", |args| {
        dataview_get(args, TypedArrayKind::Int8, false)
    });
    vm.register_native("DataView_getUint8", |args| {
        dataview_get(args, TypedArrayKind::Uint8, false)
    });
    vm.register_native("DataView_getInt16", |args| {
        let little_endian = args.get(2).map(|v| v.to_boolean()).unwrap_or(false);
        dataview_get(args, TypedArrayKind::Int16, little_endian)
    });
    vm.register_native("DataView_getUint16", |args| {
        let little_endian = args.get(2).map(|v| v.to_boolean()).unwrap_or(false);
        dataview_get(args, TypedArrayKind::Uint16, little_endian)
    });
    vm.register_native("DataView_getInt32", |args| {
        let little_endian = args.get(2).map(|v| v.to_boolean()).unwrap_or(false);
        dataview_get(args, TypedArrayKind::Int32, little_endian)
    });
    vm.register_native("DataView_getUint32", |args| {
        let little_endian = args.get(2).map(|v| v.to_boolean()).unwrap_or(false);
        dataview_get(args, TypedArrayKind::Uint32, little_endian)
    });
    vm.register_native("DataView_getFloat32", |args| {
        let little_endian = args.get(2).map(|v| v.to_boolean()).unwrap_or(false);
        dataview_get(args, TypedArrayKind::Float32, little_endian)
    });
    vm.register_native("DataView_getFloat64", |args| {
        let little_endian = args.get(2).map(|v| v.to_boolean()).unwrap_or(false);
        dataview_get(args, TypedArrayKind::Float64, little_endian)
    });

    // DataView setter methods
    vm.register_native("DataView_setInt8", |args| {
        dataview_set(args, TypedArrayKind::Int8, false)
    });
    vm.register_native("DataView_setUint8", |args| {
        dataview_set(args, TypedArrayKind::Uint8, false)
    });
    vm.register_native("DataView_setInt16", |args| {
        let little_endian = args.get(3).map(|v| v.to_boolean()).unwrap_or(false);
        dataview_set(args, TypedArrayKind::Int16, little_endian)
    });
    vm.register_native("DataView_setUint16", |args| {
        let little_endian = args.get(3).map(|v| v.to_boolean()).unwrap_or(false);
        dataview_set(args, TypedArrayKind::Uint16, little_endian)
    });
    vm.register_native("DataView_setInt32", |args| {
        let little_endian = args.get(3).map(|v| v.to_boolean()).unwrap_or(false);
        dataview_set(args, TypedArrayKind::Int32, little_endian)
    });
    vm.register_native("DataView_setUint32", |args| {
        let little_endian = args.get(3).map(|v| v.to_boolean()).unwrap_or(false);
        dataview_set(args, TypedArrayKind::Uint32, little_endian)
    });
    vm.register_native("DataView_setFloat32", |args| {
        let little_endian = args.get(3).map(|v| v.to_boolean()).unwrap_or(false);
        dataview_set(args, TypedArrayKind::Float32, little_endian)
    });
    vm.register_native("DataView_setFloat64", |args| {
        let little_endian = args.get(3).map(|v| v.to_boolean()).unwrap_or(false);
        dataview_set(args, TypedArrayKind::Float64, little_endian)
    });

    let dataview = Value::new_object();
    vm.set_global("DataView", dataview);
    vm.set_global("__DataView_constructor", vm.get_global("DataView_constructor").unwrap_or(Value::Undefined));
}

/// Helper function to read from DataView
fn dataview_get(args: &[Value], kind: TypedArrayKind, little_endian: bool) -> Result<Value> {
    let this = args.first().cloned().unwrap_or(Value::Undefined);
    let byte_offset_arg = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);

    if let Value::Object(obj) = &this {
        let obj_ref = obj.borrow();
        if let ObjectKind::DataView { buffer, byte_offset, byte_length } = &obj_ref.kind {
            let buf = buffer.borrow();
            let offset = byte_offset + byte_offset_arg;
            let elem_size = kind.bytes_per_element();

            if byte_offset_arg + elem_size > *byte_length {
                return Ok(Value::Undefined);
            }

            if offset + elem_size > buf.len() {
                return Ok(Value::Undefined);
            }

            let value = match kind {
                TypedArrayKind::Int8 => buf[offset] as i8 as f64,
                TypedArrayKind::Uint8 | TypedArrayKind::Uint8Clamped => buf[offset] as f64,
                TypedArrayKind::Int16 => {
                    let bytes = if little_endian {
                        [buf[offset], buf[offset + 1]]
                    } else {
                        [buf[offset + 1], buf[offset]]
                    };
                    i16::from_le_bytes(bytes) as f64
                }
                TypedArrayKind::Uint16 => {
                    let bytes = if little_endian {
                        [buf[offset], buf[offset + 1]]
                    } else {
                        [buf[offset + 1], buf[offset]]
                    };
                    u16::from_le_bytes(bytes) as f64
                }
                TypedArrayKind::Int32 => {
                    let bytes = if little_endian {
                        [buf[offset], buf[offset + 1], buf[offset + 2], buf[offset + 3]]
                    } else {
                        [buf[offset + 3], buf[offset + 2], buf[offset + 1], buf[offset]]
                    };
                    i32::from_le_bytes(bytes) as f64
                }
                TypedArrayKind::Uint32 => {
                    let bytes = if little_endian {
                        [buf[offset], buf[offset + 1], buf[offset + 2], buf[offset + 3]]
                    } else {
                        [buf[offset + 3], buf[offset + 2], buf[offset + 1], buf[offset]]
                    };
                    u32::from_le_bytes(bytes) as f64
                }
                TypedArrayKind::Float32 => {
                    let bytes = if little_endian {
                        [buf[offset], buf[offset + 1], buf[offset + 2], buf[offset + 3]]
                    } else {
                        [buf[offset + 3], buf[offset + 2], buf[offset + 1], buf[offset]]
                    };
                    f32::from_le_bytes(bytes) as f64
                }
                TypedArrayKind::Float64 => {
                    let bytes = if little_endian {
                        [buf[offset], buf[offset + 1], buf[offset + 2], buf[offset + 3],
                         buf[offset + 4], buf[offset + 5], buf[offset + 6], buf[offset + 7]]
                    } else {
                        [buf[offset + 7], buf[offset + 6], buf[offset + 5], buf[offset + 4],
                         buf[offset + 3], buf[offset + 2], buf[offset + 1], buf[offset]]
                    };
                    f64::from_le_bytes(bytes)
                }
            };
            return Ok(Value::Number(value));
        }
    }
    Ok(Value::Undefined)
}

/// Helper function to write to DataView
fn dataview_set(args: &[Value], kind: TypedArrayKind, little_endian: bool) -> Result<Value> {
    let this = args.first().cloned().unwrap_or(Value::Undefined);
    let byte_offset_arg = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let value = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);

    if let Value::Object(obj) = &this {
        let obj_ref = obj.borrow();
        if let ObjectKind::DataView { buffer, byte_offset, byte_length } = &obj_ref.kind {
            let mut buf = buffer.borrow_mut();
            let offset = byte_offset + byte_offset_arg;
            let elem_size = kind.bytes_per_element();

            if byte_offset_arg + elem_size > *byte_length {
                return Ok(Value::Undefined);
            }

            if offset + elem_size > buf.len() {
                return Ok(Value::Undefined);
            }

            match kind {
                TypedArrayKind::Int8 => {
                    buf[offset] = value as i8 as u8;
                }
                TypedArrayKind::Uint8 | TypedArrayKind::Uint8Clamped => {
                    buf[offset] = value as u8;
                }
                TypedArrayKind::Int16 => {
                    let bytes = (value as i16).to_le_bytes();
                    if little_endian {
                        buf[offset] = bytes[0];
                        buf[offset + 1] = bytes[1];
                    } else {
                        buf[offset] = bytes[1];
                        buf[offset + 1] = bytes[0];
                    }
                }
                TypedArrayKind::Uint16 => {
                    let bytes = (value as u16).to_le_bytes();
                    if little_endian {
                        buf[offset] = bytes[0];
                        buf[offset + 1] = bytes[1];
                    } else {
                        buf[offset] = bytes[1];
                        buf[offset + 1] = bytes[0];
                    }
                }
                TypedArrayKind::Int32 => {
                    let bytes = (value as i32).to_le_bytes();
                    if little_endian {
                        buf[offset..offset + 4].copy_from_slice(&bytes);
                    } else {
                        buf[offset] = bytes[3];
                        buf[offset + 1] = bytes[2];
                        buf[offset + 2] = bytes[1];
                        buf[offset + 3] = bytes[0];
                    }
                }
                TypedArrayKind::Uint32 => {
                    let bytes = (value as u32).to_le_bytes();
                    if little_endian {
                        buf[offset..offset + 4].copy_from_slice(&bytes);
                    } else {
                        buf[offset] = bytes[3];
                        buf[offset + 1] = bytes[2];
                        buf[offset + 2] = bytes[1];
                        buf[offset + 3] = bytes[0];
                    }
                }
                TypedArrayKind::Float32 => {
                    let bytes = (value as f32).to_le_bytes();
                    if little_endian {
                        buf[offset..offset + 4].copy_from_slice(&bytes);
                    } else {
                        buf[offset] = bytes[3];
                        buf[offset + 1] = bytes[2];
                        buf[offset + 2] = bytes[1];
                        buf[offset + 3] = bytes[0];
                    }
                }
                TypedArrayKind::Float64 => {
                    let bytes = value.to_le_bytes();
                    if little_endian {
                        buf[offset..offset + 8].copy_from_slice(&bytes);
                    } else {
                        for i in 0..8 {
                            buf[offset + i] = bytes[7 - i];
                        }
                    }
                }
            }
        }
    }
    Ok(Value::Undefined)
}

/// Register URL and URLSearchParams
fn register_url(vm: &mut VM) {
    use rustc_hash::FxHashMap as HashMap;

    // Helper function to parse a URL string
    fn parse_url(url_str: &str, base: Option<&str>) -> Option<UrlParts> {
        let url = if let Some(base_str) = base {
            // If base is provided, resolve relative URL
            if url_str.starts_with("//") || url_str.contains("://") {
                // Absolute URL, ignore base
                url_str.to_string()
            } else if let Some(base_parts) = parse_url(base_str, None) {
                // Relative URL
                if url_str.starts_with('/') {
                    // Absolute path
                    format!(
                        "{}//{}{}{}{}",
                        base_parts.protocol,
                        if !base_parts.username.is_empty() || !base_parts.password.is_empty() {
                            format!(
                                "{}{}@",
                                base_parts.username,
                                if !base_parts.password.is_empty() {
                                    format!(":{}", base_parts.password)
                                } else {
                                    String::new()
                                }
                            )
                        } else {
                            String::new()
                        },
                        base_parts.host,
                        url_str,
                        ""
                    )
                } else if url_str.starts_with('?') {
                    // Query-only
                    format!(
                        "{}//{}{}{}{}",
                        base_parts.protocol,
                        base_parts.host,
                        base_parts.pathname,
                        url_str,
                        ""
                    )
                } else if url_str.starts_with('#') {
                    // Hash-only
                    format!(
                        "{}//{}{}{}{}",
                        base_parts.protocol,
                        base_parts.host,
                        base_parts.pathname,
                        base_parts.search,
                        url_str
                    )
                } else {
                    // Relative path - resolve against base pathname
                    let base_path = base_parts.pathname;
                    let parent = if let Some(pos) = base_path.rfind('/') {
                        &base_path[..=pos]
                    } else {
                        "/"
                    };
                    format!(
                        "{}//{}{}{}",
                        base_parts.protocol,
                        base_parts.host,
                        parent,
                        url_str
                    )
                }
            } else {
                return None;
            }
        } else {
            url_str.to_string()
        };

        // Parse the URL
        let mut remaining = url.as_str();

        // Extract protocol
        let protocol = if let Some(pos) = remaining.find("://") {
            let proto = &remaining[..pos + 1]; // Include the colon
            remaining = &remaining[pos + 3..];
            proto.to_string()
        } else if remaining.starts_with("//") {
            remaining = &remaining[2..];
            "https:".to_string()
        } else {
            return None; // Invalid URL
        };

        // Extract username and password
        let (username, password, after_auth) = if let Some(at_pos) = remaining.find('@') {
            let auth = &remaining[..at_pos];
            let rest = &remaining[at_pos + 1..];
            if let Some(colon_pos) = auth.find(':') {
                (
                    auth[..colon_pos].to_string(),
                    auth[colon_pos + 1..].to_string(),
                    rest,
                )
            } else {
                (auth.to_string(), String::new(), rest)
            }
        } else {
            (String::new(), String::new(), remaining)
        };
        remaining = after_auth;

        // Extract hash
        let (without_hash, hash) = if let Some(pos) = remaining.find('#') {
            (&remaining[..pos], remaining[pos..].to_string())
        } else {
            (remaining, String::new())
        };
        remaining = without_hash;

        // Extract search/query
        let (without_search, search) = if let Some(pos) = remaining.find('?') {
            (&remaining[..pos], remaining[pos..].to_string())
        } else {
            (remaining, String::new())
        };
        remaining = without_search;

        // Extract pathname
        let (host_part, pathname) = if let Some(pos) = remaining.find('/') {
            (&remaining[..pos], remaining[pos..].to_string())
        } else {
            (remaining, "/".to_string())
        };

        // Extract hostname and port
        let (hostname, port) = if let Some(pos) = host_part.rfind(':') {
            let potential_port = &host_part[pos + 1..];
            if potential_port.chars().all(|c| c.is_ascii_digit()) {
                (host_part[..pos].to_string(), potential_port.to_string())
            } else {
                (host_part.to_string(), String::new())
            }
        } else {
            (host_part.to_string(), String::new())
        };

        let host = if port.is_empty() {
            hostname.clone()
        } else {
            format!("{}:{}", hostname, port)
        };

        // Build href
        let href = format!(
            "{}//{}{}{}{}{}/{}",
            protocol,
            if !username.is_empty() || !password.is_empty() {
                format!(
                    "{}{}@",
                    username,
                    if !password.is_empty() {
                        format!(":{}", password)
                    } else {
                        String::new()
                    }
                )
            } else {
                String::new()
            },
            host,
            if pathname == "/" { "" } else { &pathname },
            search,
            hash,
            ""
        ).trim_end_matches('/').to_string();

        Some(UrlParts {
            href,
            protocol,
            username,
            password,
            host,
            hostname,
            port,
            pathname,
            search,
            hash,
        })
    }

    #[derive(Clone)]
    struct UrlParts {
        href: String,
        protocol: String,
        username: String,
        password: String,
        host: String,
        hostname: String,
        port: String,
        pathname: String,
        search: String,
        hash: String,
    }

    // URL constructor
    vm.register_native("URL", |args| {
        let url_str = args
            .first()
            .map(|v| v.to_js_string())
            .unwrap_or_default();
        let base = args.get(1).map(|v| v.to_js_string());

        let parts = parse_url(&url_str, base.as_deref()).ok_or_else(|| {
            crate::error::Error::type_error(format!("Invalid URL: {}", url_str))
        })?;

        // Create URL object
        let url_obj = super::value::Object {
            kind: super::value::ObjectKind::URL {
                href: parts.href.clone(),
                protocol: parts.protocol.clone(),
                username: parts.username.clone(),
                password: parts.password.clone(),
                host: parts.host.clone(),
                hostname: parts.hostname.clone(),
                port: parts.port.clone(),
                pathname: parts.pathname.clone(),
                search: parts.search.clone(),
                hash: parts.hash.clone(),
            },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        };

        let obj_rc = Rc::new(RefCell::new(url_obj));

        // Add origin property (computed from protocol + host)
        let origin = format!(
            "//{}",
            parts.host
        );
        let origin = format!("{}{}", parts.protocol, origin);
        obj_rc.borrow_mut().set_property("origin", Value::String(origin));

        // Add searchParams property
        let search_params = parse_search_params(&parts.search);
        let params_obj = super::value::Object {
            kind: super::value::ObjectKind::URLSearchParams { params: search_params },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        };
        obj_rc.borrow_mut().set_property("searchParams", Value::Object(Rc::new(RefCell::new(params_obj))));

        Ok(Value::Object(obj_rc))
    });

    fn parse_search_params(search: &str) -> Vec<(String, String)> {
        use super::value::url_decode;
        let query = search.strip_prefix('?').unwrap_or(search);
        if query.is_empty() {
            return Vec::new();
        }
        query
            .split('&')
            .filter_map(|pair| {
                if pair.is_empty() {
                    return None;
                }
                let mut parts = pair.splitn(2, '=');
                let key = url_decode(parts.next()?);
                let value = url_decode(parts.next().unwrap_or(""));
                Some((key, value))
            })
            .collect()
    }

    // URLSearchParams constructor
    vm.register_native("URLSearchParams", |args| {
        let params: Vec<(String, String)> = if let Some(init) = args.first() {
            match init {
                Value::String(s) => parse_search_params(s),
                Value::Object(obj) => {
                    let obj_ref = obj.borrow();
                    match &obj_ref.kind {
                        super::value::ObjectKind::Array(arr) => {
                            // Array of [key, value] pairs
                            arr.iter()
                                .filter_map(|item| {
                                    if let Value::Object(pair_obj) = item {
                                        let pair_ref = pair_obj.borrow();
                                        if let super::value::ObjectKind::Array(pair) = &pair_ref.kind {
                                            let key = pair.first()?.to_js_string();
                                            let value = pair.get(1).map(|v| v.to_js_string()).unwrap_or_default();
                                            return Some((key, value));
                                        }
                                    }
                                    None
                                })
                                .collect()
                        }
                        super::value::ObjectKind::Ordinary => {
                            // Plain object
                            obj_ref
                                .properties
                                .iter()
                                .map(|(k, v)| (k.clone(), v.to_js_string()))
                                .collect()
                        }
                        super::value::ObjectKind::URLSearchParams { params } => {
                            // Copy from another URLSearchParams
                            params.clone()
                        }
                        _ => Vec::new(),
                    }
                }
                _ => Vec::new(),
            }
        } else {
            Vec::new()
        };

        let obj = super::value::Object {
            kind: super::value::ObjectKind::URLSearchParams { params },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        };

        Ok(Value::Object(Rc::new(RefCell::new(obj))))
    });

    // Register URL as a global
    let url_constructor = vm.get_global("URL").unwrap_or(Value::Undefined);
    vm.set_global("URL", url_constructor);

    // Register URLSearchParams as a global
    let params_constructor = vm.get_global("URLSearchParams").unwrap_or(Value::Undefined);
    vm.set_global("URLSearchParams", params_constructor);
}

/// Register Error constructor and error types
fn register_error(vm: &mut VM) {
    use super::value::Object;

    // Error.stackTraceLimit - global limit for stack trace frames
    vm.set_global("__Error_stackTraceLimit", Value::Number(10.0));

    // Error constructor - creates an Error object with message and stack
    vm.register_native("__Error_constructor", |args| {
        let message = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        // Support ES2022 error cause option
        let options = args.get(1);
        let cause = options.and_then(|o| o.get_property("cause"));

        // Create Error object with ObjectKind::Error
        let error_obj = Value::Object(Rc::new(RefCell::new(Object {
            kind: ObjectKind::Error {
                name: "Error".to_string(),
                message: message.clone(),
            },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        })));

        // Set message property explicitly for JavaScript access
        error_obj.set_property("message", Value::String(message.clone()));
        error_obj.set_property("name", Value::String("Error".to_string()));

        // Stack trace placeholder - will be populated by VM when thrown
        error_obj.set_property("stack", Value::String(format!("Error: {}", message)));

        // Set cause if provided (ES2022)
        if let Some(cause_val) = cause {
            error_obj.set_property("cause", cause_val);
        }

        Ok(error_obj)
    });

    // Error.captureStackTrace(targetObject, constructorOpt)
    // Sets the stack property on targetObject
    vm.register_native("Error_captureStackTrace", |args| {
        let target = args.first().cloned().unwrap_or(Value::Undefined);
        // constructorOpt is used to exclude frames - simplified implementation
        let _constructor_opt = args.get(1);

        if let Value::Object(_) = &target {
            // Create a stack trace string
            let stack = "    at <anonymous>".to_string();
            target.set_property("stack", Value::String(stack));
            Ok(Value::Undefined)
        } else {
            Err(crate::error::Error::type_error(
                "Error.captureStackTrace requires object target",
            ))
        }
    });

    // Error.prepareStackTrace - customizable stack trace formatting
    vm.register_native("Error_prepareStackTrace", |_args| {
        // Default implementation returns undefined
        // Can be overridden by user code
        Ok(Value::Undefined)
    });

    // Create Error constructor object
    let error_obj = Value::new_object();
    error_obj.set_property("name", Value::String("Error".to_string()));
    error_obj.set_property(
        "captureStackTrace",
        vm.get_global("Error_captureStackTrace").unwrap_or(Value::Undefined),
    );
    error_obj.set_property("stackTraceLimit", Value::Number(10.0));
    vm.set_global("Error", error_obj);

    // TypeError constructor
    vm.register_native("__TypeError_constructor", |args| {
        let message = args.first().map(|v| v.to_js_string()).unwrap_or_default();

        let error_obj = Value::Object(Rc::new(RefCell::new(Object {
            kind: ObjectKind::Error {
                name: "TypeError".to_string(),
                message: message.clone(),
            },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        })));

        error_obj.set_property("message", Value::String(message.clone()));
        error_obj.set_property("name", Value::String("TypeError".to_string()));
        error_obj.set_property("stack", Value::String(format!("TypeError: {}", message)));

        Ok(error_obj)
    });

    let type_error_obj = Value::new_object();
    type_error_obj.set_property("name", Value::String("TypeError".to_string()));
    vm.set_global("TypeError", type_error_obj);

    // ReferenceError constructor
    vm.register_native("__ReferenceError_constructor", |args| {
        let message = args.first().map(|v| v.to_js_string()).unwrap_or_default();

        let error_obj = Value::Object(Rc::new(RefCell::new(Object {
            kind: ObjectKind::Error {
                name: "ReferenceError".to_string(),
                message: message.clone(),
            },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        })));

        error_obj.set_property("message", Value::String(message.clone()));
        error_obj.set_property("name", Value::String("ReferenceError".to_string()));
        error_obj.set_property("stack", Value::String(format!("ReferenceError: {}", message)));

        Ok(error_obj)
    });

    let ref_error_obj = Value::new_object();
    ref_error_obj.set_property("name", Value::String("ReferenceError".to_string()));
    vm.set_global("ReferenceError", ref_error_obj);

    // RangeError constructor
    vm.register_native("__RangeError_constructor", |args| {
        let message = args.first().map(|v| v.to_js_string()).unwrap_or_default();

        let error_obj = Value::Object(Rc::new(RefCell::new(Object {
            kind: ObjectKind::Error {
                name: "RangeError".to_string(),
                message: message.clone(),
            },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        })));

        error_obj.set_property("message", Value::String(message.clone()));
        error_obj.set_property("name", Value::String("RangeError".to_string()));
        error_obj.set_property("stack", Value::String(format!("RangeError: {}", message)));

        Ok(error_obj)
    });

    let range_error_obj = Value::new_object();
    range_error_obj.set_property("name", Value::String("RangeError".to_string()));
    vm.set_global("RangeError", range_error_obj);

    // SyntaxError constructor
    vm.register_native("__SyntaxError_constructor", |args| {
        let message = args.first().map(|v| v.to_js_string()).unwrap_or_default();

        let error_obj = Value::Object(Rc::new(RefCell::new(Object {
            kind: ObjectKind::Error {
                name: "SyntaxError".to_string(),
                message: message.clone(),
            },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        })));

        error_obj.set_property("message", Value::String(message.clone()));
        error_obj.set_property("name", Value::String("SyntaxError".to_string()));
        error_obj.set_property("stack", Value::String(format!("SyntaxError: {}", message)));

        Ok(error_obj)
    });

    let syntax_error_obj = Value::new_object();
    syntax_error_obj.set_property("name", Value::String("SyntaxError".to_string()));
    vm.set_global("SyntaxError", syntax_error_obj);
}

/// Register Symbol constructor and well-known Symbols
fn register_symbol(vm: &mut VM) {
    use std::sync::atomic::{AtomicU64, Ordering};

    // Global symbol counter starting after well-known symbols
    static NEXT_SYMBOL_ID: AtomicU64 = AtomicU64::new(100);

    // Well-known Symbol IDs (0-99 reserved)
    const SYMBOL_ITERATOR: u64 = 1;
    const SYMBOL_TO_STRING_TAG: u64 = 2;
    const SYMBOL_TO_PRIMITIVE: u64 = 3;
    const SYMBOL_HAS_INSTANCE: u64 = 4;
    const SYMBOL_SPECIES: u64 = 5;
    const SYMBOL_IS_CONCAT_SPREADABLE: u64 = 6;
    const SYMBOL_MATCH: u64 = 7;
    const SYMBOL_REPLACE: u64 = 8;
    const SYMBOL_SEARCH: u64 = 9;
    const SYMBOL_SPLIT: u64 = 10;
    const SYMBOL_ASYNC_ITERATOR: u64 = 11;
    const SYMBOL_UNSCOPABLES: u64 = 12;

    // Shared registries for symbol tracking
    let symbol_registry: Rc<RefCell<HashMap<String, u64>>> = Rc::new(RefCell::new(HashMap::default()));
    let symbol_descriptions: Rc<RefCell<HashMap<u64, String>>> = Rc::new(RefCell::new(HashMap::default()));

    // Initialize well-known symbol descriptions
    {
        let mut descs = symbol_descriptions.borrow_mut();
        descs.insert(SYMBOL_ITERATOR, "Symbol.iterator".to_string());
        descs.insert(SYMBOL_TO_STRING_TAG, "Symbol.toStringTag".to_string());
        descs.insert(SYMBOL_TO_PRIMITIVE, "Symbol.toPrimitive".to_string());
        descs.insert(SYMBOL_HAS_INSTANCE, "Symbol.hasInstance".to_string());
        descs.insert(SYMBOL_SPECIES, "Symbol.species".to_string());
        descs.insert(SYMBOL_IS_CONCAT_SPREADABLE, "Symbol.isConcatSpreadable".to_string());
        descs.insert(SYMBOL_MATCH, "Symbol.match".to_string());
        descs.insert(SYMBOL_REPLACE, "Symbol.replace".to_string());
        descs.insert(SYMBOL_SEARCH, "Symbol.search".to_string());
        descs.insert(SYMBOL_SPLIT, "Symbol.split".to_string());
        descs.insert(SYMBOL_ASYNC_ITERATOR, "Symbol.asyncIterator".to_string());
        descs.insert(SYMBOL_UNSCOPABLES, "Symbol.unscopables".to_string());
    }

    // Symbol() constructor - creates a unique symbol
    let descs_clone = Rc::clone(&symbol_descriptions);
    vm.register_native("Symbol", move |args| {
        let description = args.first().map(|v| v.to_js_string());
        let id = NEXT_SYMBOL_ID.fetch_add(1, Ordering::SeqCst);

        if let Some(desc) = description {
            descs_clone.borrow_mut().insert(id, desc);
        }

        Ok(Value::Symbol(id))
    });

    // Symbol.for(key) - returns symbol from global registry
    let registry_clone = Rc::clone(&symbol_registry);
    let descs_clone2 = Rc::clone(&symbol_descriptions);
    vm.register_native("Symbol_for", move |args| {
        let key = args.first().map(|v| v.to_js_string()).unwrap_or_default();

        let mut registry = registry_clone.borrow_mut();
        if let Some(&id) = registry.get(&key) {
            Ok(Value::Symbol(id))
        } else {
            let id = NEXT_SYMBOL_ID.fetch_add(1, Ordering::SeqCst);
            registry.insert(key.clone(), id);
            descs_clone2.borrow_mut().insert(id, key);
            Ok(Value::Symbol(id))
        }
    });

    // Symbol.keyFor(symbol) - returns key for registered symbol
    let registry_clone2 = Rc::clone(&symbol_registry);
    vm.register_native("Symbol_keyFor", move |args| {
        match args.first() {
            Some(Value::Symbol(id)) => {
                let id = *id;
                let registry = registry_clone2.borrow();
                for (key, &sym_id) in registry.iter() {
                    if sym_id == id {
                        return Ok(Value::String(key.clone()));
                    }
                }
                Ok(Value::Undefined)
            }
            _ => Err(crate::error::Error::type_error("Symbol.keyFor requires a symbol argument")),
        }
    });

    // Symbol.prototype.toString
    let descs_clone3 = Rc::clone(&symbol_descriptions);
    vm.register_native("Symbol_toString", move |args| {
        match args.first() {
            Some(Value::Symbol(id)) => {
                let id = *id;
                let desc = descs_clone3.borrow().get(&id).cloned();
                match desc {
                    Some(d) => Ok(Value::String(format!("Symbol({})", d))),
                    None => Ok(Value::String("Symbol()".to_string())),
                }
            }
            _ => Err(crate::error::Error::type_error("Symbol.prototype.toString requires a symbol")),
        }
    });

    // Symbol.prototype.description (getter)
    let descs_clone4 = Rc::clone(&symbol_descriptions);
    vm.register_native("Symbol_description", move |args| {
        match args.first() {
            Some(Value::Symbol(id)) => {
                let id = *id;
                let desc = descs_clone4.borrow().get(&id).cloned();
                match desc {
                    Some(d) => Ok(Value::String(d)),
                    None => Ok(Value::Undefined),
                }
            }
            _ => Ok(Value::Undefined),
        }
    });

    // Create Symbol object with well-known symbols and static methods
    let symbol_obj = Value::new_object();

    // Add well-known symbols
    symbol_obj.set_property("iterator", Value::Symbol(SYMBOL_ITERATOR));
    symbol_obj.set_property("toStringTag", Value::Symbol(SYMBOL_TO_STRING_TAG));
    symbol_obj.set_property("toPrimitive", Value::Symbol(SYMBOL_TO_PRIMITIVE));
    symbol_obj.set_property("hasInstance", Value::Symbol(SYMBOL_HAS_INSTANCE));
    symbol_obj.set_property("species", Value::Symbol(SYMBOL_SPECIES));
    symbol_obj.set_property("isConcatSpreadable", Value::Symbol(SYMBOL_IS_CONCAT_SPREADABLE));
    symbol_obj.set_property("match", Value::Symbol(SYMBOL_MATCH));
    symbol_obj.set_property("replace", Value::Symbol(SYMBOL_REPLACE));
    symbol_obj.set_property("search", Value::Symbol(SYMBOL_SEARCH));
    symbol_obj.set_property("split", Value::Symbol(SYMBOL_SPLIT));
    symbol_obj.set_property("asyncIterator", Value::Symbol(SYMBOL_ASYNC_ITERATOR));
    symbol_obj.set_property("unscopables", Value::Symbol(SYMBOL_UNSCOPABLES));

    // Add static methods
    symbol_obj.set_property("for", vm.get_global("Symbol_for").unwrap_or(Value::Undefined));
    symbol_obj.set_property("keyFor", vm.get_global("Symbol_keyFor").unwrap_or(Value::Undefined));

    // The Symbol global is both a constructor and has properties
    // We need to make the Symbol function callable while also having properties
    // For now, we create a callable that also has the well-known symbols attached
    if let Value::Object(sym_fn) = vm.get_global("Symbol").unwrap_or(Value::Undefined) {
        let mut obj_ref = sym_fn.borrow_mut();
        obj_ref.set_property("iterator", Value::Symbol(SYMBOL_ITERATOR));
        obj_ref.set_property("toStringTag", Value::Symbol(SYMBOL_TO_STRING_TAG));
        obj_ref.set_property("toPrimitive", Value::Symbol(SYMBOL_TO_PRIMITIVE));
        obj_ref.set_property("hasInstance", Value::Symbol(SYMBOL_HAS_INSTANCE));
        obj_ref.set_property("species", Value::Symbol(SYMBOL_SPECIES));
        obj_ref.set_property("isConcatSpreadable", Value::Symbol(SYMBOL_IS_CONCAT_SPREADABLE));
        obj_ref.set_property("match", Value::Symbol(SYMBOL_MATCH));
        obj_ref.set_property("replace", Value::Symbol(SYMBOL_REPLACE));
        obj_ref.set_property("search", Value::Symbol(SYMBOL_SEARCH));
        obj_ref.set_property("split", Value::Symbol(SYMBOL_SPLIT));
        obj_ref.set_property("asyncIterator", Value::Symbol(SYMBOL_ASYNC_ITERATOR));
        obj_ref.set_property("unscopables", Value::Symbol(SYMBOL_UNSCOPABLES));
        obj_ref.set_property("for", vm.get_global("Symbol_for").unwrap_or(Value::Undefined));
        obj_ref.set_property("keyFor", vm.get_global("Symbol_keyFor").unwrap_or(Value::Undefined));
    }
}

/// Register concurrency primitives (Channel, spawn) for JavaScript
fn register_concurrency(vm: &mut VM) {
    use super::value::Object;
    use std::sync::Arc;

    // Channel.new(capacity?) - Create a new channel
    vm.register_native("Channel_new", |args| {
        let capacity = args
            .first()
            .map(|v| v.to_number() as usize)
            .unwrap_or(0);

        let channel = Arc::new(Channel::with_capacity(capacity));

        let channel_obj = Value::Object(Rc::new(RefCell::new(Object {
            kind: ObjectKind::Channel {
                channel,
                capacity,
            },
            properties: HashMap::default(),
            private_fields: HashMap::default(),
            prototype: None,
        })));

        Ok(channel_obj)
    });

    // channel.send(value) - Send a value through the channel
    vm.register_native("Channel_send", |args| {
        let channel_val = args.first().ok_or_else(|| {
            crate::error::Error::type_error("Channel.send requires a channel")
        })?;

        let value = args.get(1).cloned().unwrap_or(Value::Undefined);

        if let Value::Object(obj) = channel_val {
            let obj_ref = obj.borrow();
            if let ObjectKind::Channel { channel, .. } = &obj_ref.kind {
                match channel.send(value) {
                    Ok(()) => Ok(Value::Undefined),
                    Err(_) => Err(crate::error::Error::type_error("Channel is closed")),
                }
            } else {
                Err(crate::error::Error::type_error("Expected a Channel object"))
            }
        } else {
            Err(crate::error::Error::type_error("Expected a Channel object"))
        }
    });

    // channel.trySend(value) - Non-blocking send, returns boolean
    vm.register_native("Channel_trySend", |args| {
        let channel_val = args.first().ok_or_else(|| {
            crate::error::Error::type_error("Channel.trySend requires a channel")
        })?;

        let value = args.get(1).cloned().unwrap_or(Value::Undefined);

        if let Value::Object(obj) = channel_val {
            let obj_ref = obj.borrow();
            if let ObjectKind::Channel { channel, .. } = &obj_ref.kind {
                match channel.try_send(value) {
                    Ok(()) => Ok(Value::Boolean(true)),
                    Err(_) => Ok(Value::Boolean(false)),
                }
            } else {
                Err(crate::error::Error::type_error("Expected a Channel object"))
            }
        } else {
            Err(crate::error::Error::type_error("Expected a Channel object"))
        }
    });

    // channel.recv() - Receive from channel (blocking, returns Promise for async usage)
    vm.register_native("Channel_recv", |args| {
        let channel_val = args.first().ok_or_else(|| {
            crate::error::Error::type_error("Channel.recv requires a channel")
        })?;

        if let Value::Object(obj) = channel_val {
            let obj_ref = obj.borrow();
            if let ObjectKind::Channel { channel, .. } = &obj_ref.kind {
                // For now, do a blocking receive
                // In a full implementation, this would return a Promise
                match channel.recv() {
                    Ok(value) => Ok(value),
                    Err(_) => Ok(Value::Undefined), // Channel closed
                }
            } else {
                Err(crate::error::Error::type_error("Expected a Channel object"))
            }
        } else {
            Err(crate::error::Error::type_error("Expected a Channel object"))
        }
    });

    // channel.tryRecv() - Non-blocking receive, returns {value, ok} or {ok: false}
    vm.register_native("Channel_tryRecv", |args| {
        let channel_val = args.first().ok_or_else(|| {
            crate::error::Error::type_error("Channel.tryRecv requires a channel")
        })?;

        if let Value::Object(obj) = channel_val {
            let obj_ref = obj.borrow();
            if let ObjectKind::Channel { channel, .. } = &obj_ref.kind {
                match channel.try_recv() {
                    Ok(value) => {
                        let result = Value::new_object();
                        result.set_property("value", value);
                        result.set_property("ok", Value::Boolean(true));
                        Ok(result)
                    }
                    Err(_) => {
                        let result = Value::new_object();
                        result.set_property("ok", Value::Boolean(false));
                        Ok(result)
                    }
                }
            } else {
                Err(crate::error::Error::type_error("Expected a Channel object"))
            }
        } else {
            Err(crate::error::Error::type_error("Expected a Channel object"))
        }
    });

    // channel.close() - Close the channel
    vm.register_native("Channel_close", |args| {
        let channel_val = args.first().ok_or_else(|| {
            crate::error::Error::type_error("Channel.close requires a channel")
        })?;

        if let Value::Object(obj) = channel_val {
            let obj_ref = obj.borrow();
            if let ObjectKind::Channel { channel, .. } = &obj_ref.kind {
                channel.close();
                Ok(Value::Undefined)
            } else {
                Err(crate::error::Error::type_error("Expected a Channel object"))
            }
        } else {
            Err(crate::error::Error::type_error("Expected a Channel object"))
        }
    });

    // channel.isClosed() - Check if channel is closed
    vm.register_native("Channel_isClosed", |args| {
        let channel_val = args.first().ok_or_else(|| {
            crate::error::Error::type_error("Channel.isClosed requires a channel")
        })?;

        if let Value::Object(obj) = channel_val {
            let obj_ref = obj.borrow();
            if let ObjectKind::Channel { channel, .. } = &obj_ref.kind {
                Ok(Value::Boolean(channel.is_closed()))
            } else {
                Err(crate::error::Error::type_error("Expected a Channel object"))
            }
        } else {
            Err(crate::error::Error::type_error("Expected a Channel object"))
        }
    });

    // channel.length - Get number of items in channel
    vm.register_native("Channel_length", |args| {
        let channel_val = args.first().ok_or_else(|| {
            crate::error::Error::type_error("Channel.length requires a channel")
        })?;

        if let Value::Object(obj) = channel_val {
            let obj_ref = obj.borrow();
            if let ObjectKind::Channel { channel, .. } = &obj_ref.kind {
                Ok(Value::Number(channel.len() as f64))
            } else {
                Err(crate::error::Error::type_error("Expected a Channel object"))
            }
        } else {
            Err(crate::error::Error::type_error("Expected a Channel object"))
        }
    });

    // Create Channel constructor object
    let channel_obj = Value::new_object();
    channel_obj.set_property("new", vm.get_global("Channel_new").unwrap_or(Value::Undefined));
    vm.set_global("Channel", channel_obj);

    // spawn(fn) - Spawn a new task (simplified - runs synchronously for now)
    // In a full implementation, this would create an actual async task
    vm.register_native("spawn", |args| {
        // Get the function to spawn
        let func = args.first().ok_or_else(|| {
            crate::error::Error::type_error("spawn requires a function")
        })?;

        // For now, just validate it's a function
        match func {
            Value::Object(obj) => {
                let obj_ref = obj.borrow();
                match &obj_ref.kind {
                    ObjectKind::Function(_)
                    | ObjectKind::NativeFunction { .. }
                    | ObjectKind::BoundFunction { .. } => {
                        // Return a TaskHandle-like object
                        let task = Value::new_object();
                        task.set_property("id", Value::Number(1.0)); // Would be unique task ID
                        task.set_property("status", Value::String("pending".to_string()));
                        Ok(task)
                    }
                    _ => Err(crate::error::Error::type_error("spawn requires a function")),
                }
            }
            _ => Err(crate::error::Error::type_error("spawn requires a function")),
        }
    });
}

/// Register WeakRef and FinalizationRegistry (ES2021)
fn register_weakref(vm: &mut VM) {
    use std::sync::{Arc, Mutex};
    use super::value::Object;

    // Storage for weak references and their targets
    // In a real implementation, these would be tied to GC cycles
    let weak_refs: Arc<Mutex<Vec<(u64, std::rc::Weak<RefCell<Object>>)>>> = Arc::new(Mutex::new(Vec::new()));
    let ref_counter: Arc<Mutex<u64>> = Arc::new(Mutex::new(0));

    // WeakRef constructor
    let weak_refs_new = Arc::clone(&weak_refs);
    let ref_counter_new = Arc::clone(&ref_counter);
    vm.register_native("WeakRef_new", move |args| {
        let target = args.first().ok_or_else(|| {
            crate::error::Error::type_error("WeakRef requires a target object")
        })?;

        if let Value::Object(obj) = target {
            let weak = Rc::downgrade(obj);
            let mut counter = ref_counter_new.lock().unwrap();
            let id = *counter;
            *counter += 1;

            weak_refs_new.lock().unwrap().push((id, weak));

            // Create WeakRef object
            let weak_ref = Value::new_object();
            weak_ref.set_property("__weak_id__", Value::Number(id as f64));
            Ok(weak_ref)
        } else {
            Err(crate::error::Error::type_error("WeakRef target must be an object"))
        }
    });

    // WeakRef.prototype.deref()
    let weak_refs_deref = Arc::clone(&weak_refs);
    vm.register_native("WeakRef_deref", move |args| {
        let this = args.first().ok_or_else(|| {
            crate::error::Error::type_error("WeakRef.deref requires 'this'")
        })?;

        if let Some(id_val) = this.get_property("__weak_id__") {
            let id = id_val.to_number() as u64;
            let refs = weak_refs_deref.lock().unwrap();

            for (ref_id, weak) in refs.iter() {
                if *ref_id == id {
                    if let Some(strong) = weak.upgrade() {
                        return Ok(Value::Object(strong));
                    }
                }
            }
        }
        Ok(Value::Undefined)
    });

    // FinalizationRegistry constructor
    vm.register_native("FinalizationRegistry_new", |args| {
        let callback = args.first().cloned().unwrap_or(Value::Undefined);

        // Create registry object
        let registry = Value::new_object();
        registry.set_property("__callback__", callback);
        registry.set_property("__entries__", Value::new_array(vec![]));
        Ok(registry)
    });

    // FinalizationRegistry.prototype.register(target, heldValue, unregisterToken?)
    vm.register_native("FinalizationRegistry_register", |args| {
        let this = args.first().ok_or_else(|| {
            crate::error::Error::type_error("register requires 'this'")
        })?;
        let _target = args.get(1).cloned().unwrap_or(Value::Undefined);
        let held_value = args.get(2).cloned().unwrap_or(Value::Undefined);
        let _unregister_token = args.get(3).cloned();

        // Add entry to registry
        if let Some(entries) = this.get_property("__entries__") {
            if let Value::Object(arr_obj) = entries {
                let mut arr = arr_obj.borrow_mut();
                if let ObjectKind::Array(items) = &mut arr.kind {
                    let entry = Value::new_object();
                    entry.set_property("heldValue", held_value);
                    items.push(entry);
                }
            }
        }

        Ok(Value::Undefined)
    });

    // FinalizationRegistry.prototype.unregister(unregisterToken)
    vm.register_native("FinalizationRegistry_unregister", |_args| {
        // Simplified: always return false (no entry removed)
        Ok(Value::Boolean(false))
    });

    // Create WeakRef constructor
    let weakref_constructor = Value::new_object();
    weakref_constructor.set_property("prototype", Value::new_object());
    vm.set_global("WeakRef", vm.get_global("WeakRef_new").unwrap_or(weakref_constructor));

    // Create FinalizationRegistry constructor
    let registry_constructor = Value::new_object();
    registry_constructor.set_property("prototype", Value::new_object());
    vm.set_global("FinalizationRegistry", vm.get_global("FinalizationRegistry_new").unwrap_or(registry_constructor));

    // queueMicrotask - schedules a microtask (simplified: executes immediately)
    vm.register_native("queueMicrotask", |args| {
        let callback = args.first().ok_or_else(|| {
            crate::error::Error::type_error("queueMicrotask requires a callback")
        })?;

        // In a real implementation, this would queue to microtask queue
        // For now, we just validate it's a function
        match callback {
            Value::Object(obj) => {
                let obj_ref = obj.borrow();
                match &obj_ref.kind {
                    ObjectKind::Function(_) | ObjectKind::NativeFunction { .. } => {
                        // Would queue for later execution
                        Ok(Value::Undefined)
                    }
                    _ => Err(crate::error::Error::type_error("queueMicrotask requires a function")),
                }
            }
            _ => Err(crate::error::Error::type_error("queueMicrotask requires a function")),
        }
    });

    // structuredClone - deep clone a value
    vm.register_native("structuredClone", |args| {
        let value = args.first().cloned().unwrap_or(Value::Undefined);
        Ok(deep_clone(&value))
    });
}

/// Deep clone a JavaScript value
fn deep_clone(value: &Value) -> Value {
    match value {
        Value::Object(obj) => {
            let obj_ref = obj.borrow();
            match &obj_ref.kind {
                ObjectKind::Array(items) => {
                    let cloned_items: Vec<Value> = items.iter().map(deep_clone).collect();
                    Value::new_array(cloned_items)
                }
                ObjectKind::Ordinary => {
                    let cloned = Value::new_object();
                    for (key, val) in &obj_ref.properties {
                        cloned.set_property(key, deep_clone(val));
                    }
                    cloned
                }
                ObjectKind::Map(entries) => {
                    let cloned_entries: Vec<(Value, Value)> = entries
                        .iter()
                        .map(|(k, v)| (deep_clone(k), deep_clone(v)))
                        .collect();
                    Value::Object(Rc::new(RefCell::new(super::value::Object {
                        kind: ObjectKind::Map(cloned_entries),
                        properties: HashMap::default(),
                        private_fields: HashMap::default(),
                        prototype: None,
                    })))
                }
                ObjectKind::Set(items) => {
                    let cloned_items: Vec<Value> = items.iter().map(deep_clone).collect();
                    Value::Object(Rc::new(RefCell::new(super::value::Object {
                        kind: ObjectKind::Set(cloned_items),
                        properties: HashMap::default(),
                        private_fields: HashMap::default(),
                        prototype: None,
                    })))
                }
                ObjectKind::Date(ts) => {
                    Value::Object(Rc::new(RefCell::new(super::value::Object {
                        kind: ObjectKind::Date(*ts),
                        properties: HashMap::default(),
                        private_fields: HashMap::default(),
                        prototype: None,
                    })))
                }
                // Functions and other types cannot be cloned
                _ => value.clone(),
            }
        }
        // Primitives are value types, just clone
        _ => value.clone(),
    }
}

/// Register Performance API
fn register_performance(vm: &mut VM) {
    use std::time::Instant;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    // Store the start time for Performance.now()
    let start_time = Arc::new(Instant::now());

    // Performance.now() - returns milliseconds since page load
    let start_time_now = Arc::clone(&start_time);
    vm.register_native("Performance_now", move |_args| {
        let elapsed = start_time_now.elapsed();
        Ok(Value::Number(elapsed.as_secs_f64() * 1000.0))
    });

    // Calculate timeOrigin as a value (timestamp when performance started)
    let time_origin = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as f64;

    // Create performance object
    let perf_obj = Value::new_object();
    perf_obj.set_property("now", vm.get_global("Performance_now").unwrap_or(Value::Undefined));
    perf_obj.set_property("timeOrigin", Value::Number(time_origin));
    vm.set_global("performance", perf_obj);
}

/// Register TextEncoder/TextDecoder APIs
fn register_encoding(vm: &mut VM) {
    // TextEncoder.prototype.encode - encodes strings to UTF-8 bytes
    vm.register_native("TextEncoder_encode", |args| {
        let text = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        let bytes: Vec<Value> = text.as_bytes().iter().map(|&b| Value::Number(b as f64)).collect();

        // Return Uint8Array-like object
        let result = Value::new_array(bytes);
        Ok(result)
    });

    // TextEncoder.prototype.encodeInto(source, destination)
    vm.register_native("TextEncoder_encodeInto", |args| {
        let text = args.first().map(|v| v.to_js_string()).unwrap_or_default();
        let dest = args.get(1);

        let bytes = text.as_bytes();
        let read = bytes.len();
        let written = if let Some(Value::Object(_)) = dest {
            bytes.len() // In real impl, would write to dest
        } else {
            0
        };

        let result = Value::new_object();
        result.set_property("read", Value::Number(read as f64));
        result.set_property("written", Value::Number(written as f64));
        Ok(result)
    });

    // TextDecoder.prototype.decode - decodes UTF-8 bytes to string
    vm.register_native("TextDecoder_decode", |args| {
        let input = args.first();

        let bytes: Vec<u8> = match input {
            Some(Value::Object(obj)) => {
                let obj_ref = obj.borrow();
                match &obj_ref.kind {
                    ObjectKind::Array(items) => {
                        items.iter().map(|v| v.to_number() as u8).collect()
                    }
                    ObjectKind::ArrayBuffer(buf) => {
                        buf.borrow().clone()
                    }
                    ObjectKind::TypedArray { buffer, byte_offset, length, .. } => {
                        let buf = buffer.borrow();
                        buf[*byte_offset..*byte_offset + *length].to_vec()
                    }
                    _ => vec![],
                }
            }
            _ => vec![],
        };

        let text = String::from_utf8_lossy(&bytes).to_string();
        Ok(Value::String(text))
    });

    // Store references to the methods before creating constructors
    let encode_fn = vm.get_global("TextEncoder_encode").unwrap_or(Value::Undefined);
    let encode_into_fn = vm.get_global("TextEncoder_encodeInto").unwrap_or(Value::Undefined);
    let decode_fn = vm.get_global("TextDecoder_decode").unwrap_or(Value::Undefined);

    // Create TextEncoder constructor that attaches methods to instance
    let encode_fn_clone = encode_fn.clone();
    let encode_into_fn_clone = encode_into_fn.clone();
    vm.register_native("TextEncoder", move |_args| {
        let encoder = Value::new_object();
        encoder.set_property("encoding", Value::String("utf-8".to_string()));
        encoder.set_property("encode", encode_fn_clone.clone());
        encoder.set_property("encodeInto", encode_into_fn_clone.clone());
        Ok(encoder)
    });

    // Create TextDecoder constructor that attaches methods to instance
    let decode_fn_clone = decode_fn.clone();
    vm.register_native("TextDecoder", move |args| {
        let encoding = args.first().map(|v| v.to_js_string()).unwrap_or_else(|| "utf-8".to_string());
        let decoder = Value::new_object();
        decoder.set_property("encoding", Value::String(encoding));
        decoder.set_property("fatal", Value::Boolean(false));
        decoder.set_property("ignoreBOM", Value::Boolean(false));
        decoder.set_property("decode", decode_fn_clone.clone());
        Ok(decoder)
    });

    // Register globals
    vm.set_global("TextEncoder", vm.get_global("TextEncoder").unwrap_or(Value::Undefined));
    vm.set_global("TextDecoder", vm.get_global("TextDecoder").unwrap_or(Value::Undefined));
}

/// Register basic crypto API
fn register_crypto(vm: &mut VM) {
    use std::time::{SystemTime, UNIX_EPOCH};

    // crypto.getRandomValues(typedArray) - fill with random values
    vm.register_native("crypto_getRandomValues", |args| {
        let arr = args.first().ok_or_else(|| {
            crate::error::Error::type_error("getRandomValues requires a TypedArray")
        })?;

        if let Value::Object(obj) = arr {
            let mut obj_ref = obj.borrow_mut();
            match &mut obj_ref.kind {
                ObjectKind::Array(items) => {
                    // Simple PRNG based on time (not cryptographically secure)
                    let mut seed = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos() as u64;

                    for item in items.iter_mut() {
                        seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
                        *item = Value::Number((seed % 256) as f64);
                    }
                }
                ObjectKind::TypedArray { buffer, byte_offset, length, .. } => {
                    let mut buf = buffer.borrow_mut();
                    let mut seed = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos() as u64;

                    for i in 0..*length {
                        seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
                        buf[*byte_offset + i] = (seed % 256) as u8;
                    }
                }
                _ => {}
            }
            drop(obj_ref);
            Ok(arr.clone())
        } else {
            Err(crate::error::Error::type_error("getRandomValues requires a TypedArray"))
        }
    });

    // crypto.randomUUID() - generate a random UUID v4
    vm.register_native("crypto_randomUUID", |_args| {
        use std::time::{SystemTime, UNIX_EPOCH};

        let mut seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        let mut bytes = [0u8; 16];
        for byte in bytes.iter_mut() {
            seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
            *byte = (seed % 256) as u8;
        }

        // Set version (4) and variant (RFC4122)
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;

        let uuid = format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            bytes[0], bytes[1], bytes[2], bytes[3],
            bytes[4], bytes[5],
            bytes[6], bytes[7],
            bytes[8], bytes[9],
            bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
        );

        Ok(Value::String(uuid))
    });

    // Create crypto object
    let crypto_obj = Value::new_object();
    crypto_obj.set_property("getRandomValues", vm.get_global("crypto_getRandomValues").unwrap_or(Value::Undefined));
    crypto_obj.set_property("randomUUID", vm.get_global("crypto_randomUUID").unwrap_or(Value::Undefined));
    vm.set_global("crypto", crypto_obj);
}
