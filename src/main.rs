//! Quicksilver CLI
//!
//! A command-line interface for the Quicksilver JavaScript runtime.

use clap::{Parser, Subcommand};
use quicksilver::{Runtime, VERSION};
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Parser)]
#[command(name = "quicksilver")]
#[command(author, version, about = "A memory-safe JavaScript runtime written in Rust", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// JavaScript file to execute
    #[arg(value_name = "FILE")]
    file: Option<PathBuf>,

    /// Evaluate a string of JavaScript
    #[arg(short, long, value_name = "CODE")]
    eval: Option<String>,

    /// Watch file for changes and re-run on save
    #[arg(short, long)]
    watch: bool,

    /// Enable profiling and show execution statistics
    #[arg(short, long)]
    profile: bool,

    /// Verbose output (-v for info, -vv for debug, -vvv for trace)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Arguments to pass to the script (after --)
    #[arg(last = true)]
    script_args: Vec<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start a REPL (Read-Eval-Print-Loop)
    Repl {
        /// Use fancy REPL with readline support
        #[arg(long, default_value_t = true)]
        fancy: bool,
    },

    /// Run a JavaScript file
    Run {
        /// The file to run
        file: PathBuf,
    },

    /// Evaluate JavaScript code
    Eval {
        /// The code to evaluate
        code: String,
    },

    /// Parse and display AST
    Ast {
        /// The file or code to parse
        input: String,
    },

    /// Compile and display bytecode
    Bytecode {
        /// The file or code to compile
        input: String,
    },

    /// Debug a JavaScript file with Time-Travel Debugger
    Debug {
        /// The file to debug
        file: PathBuf,
    },

    /// Run JavaScript tests
    Test {
        /// Test file or directory
        path: PathBuf,
        /// Filter tests by name pattern
        #[arg(short, long)]
        filter: Option<String>,
        /// Verbose output
        #[arg(long)]
        verbose: bool,
    },
}

fn main() {
    let cli = Cli::parse();

    // Set up logging based on verbosity
    setup_logging(cli.verbose);

    // Handle subcommands
    if let Some(command) = cli.command {
        match command {
            Commands::Repl { fancy } => {
                if fancy {
                    run_fancy_repl();
                } else {
                    run_repl();
                }
            }
            Commands::Run { file } => run_file(&file, cli.profile, &cli.script_args),
            Commands::Eval { code } => eval_code(&code),
            Commands::Ast { input } => show_ast(&input),
            Commands::Bytecode { input } => show_bytecode(&input),
            Commands::Debug { file } => run_debug(&file),
            Commands::Test { path, filter, verbose } => run_tests(&path, filter, verbose),
        }
        return;
    }

    // Handle direct arguments
    if let Some(code) = cli.eval {
        eval_code(&code);
        return;
    }

    if let Some(file) = cli.file {
        if cli.watch {
            run_watch(&file, cli.profile, &cli.script_args);
        } else {
            run_file(&file, cli.profile, &cli.script_args);
        }
        return;
    }

    // Default to fancy REPL
    run_fancy_repl();
}

fn setup_logging(verbosity: u8) {
    match verbosity {
        0 => {} // Default: errors only
        1 => eprintln!("[INFO] Verbose logging enabled"),
        2 => eprintln!("[DEBUG] Debug logging enabled"),
        _ => eprintln!("[TRACE] Trace logging enabled"),
    }
}

fn run_fancy_repl() {
    use quicksilver::repl::Repl;

    match Repl::new() {
        Ok(mut repl) => {
            if let Err(e) = repl.run() {
                eprintln!("REPL error: {}", e);
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("Failed to start REPL: {}", e);
            eprintln!("Falling back to basic REPL...");
            run_repl();
        }
    }
}

fn run_repl() {
    println!("Quicksilver {} - JavaScript Runtime", VERSION);
    println!("Type .help for help, .exit to quit\n");

    let mut runtime = Runtime::new();
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut multiline_buffer = String::new();
    let mut in_multiline = false;

    loop {
        if in_multiline {
            print!("... ");
        } else {
            print!("> ");
        }
        stdout.flush().unwrap();

        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(_) => {}
            Err(e) => {
                eprintln!("Error reading input: {}", e);
                continue;
            }
        }

        let trimmed = line.trim();

        // Handle REPL commands (only when not in multiline mode)
        if !in_multiline && trimmed.starts_with('.') {
            match trimmed {
                ".exit" | ".quit" => break,
                ".help" => {
                    println!("REPL Commands:");
                    println!("  .exit, .quit  - Exit the REPL");
                    println!("  .help         - Show this help");
                    println!("  .clear        - Clear the screen");
                    println!("  .version      - Show version info");
                    println!("  .ast <code>   - Show AST for code");
                    println!("  .bc <code>    - Show bytecode for code");
                    println!();
                    println!("Multi-line input:");
                    println!("  Use {{ to start a block, }} to end");
                    println!("  Incomplete statements continue on next line");
                    continue;
                }
                ".clear" => {
                    print!("\x1b[2J\x1b[H");
                    stdout.flush().unwrap();
                    continue;
                }
                ".version" => {
                    println!("Quicksilver {}", VERSION);
                    println!("A memory-safe JavaScript runtime written in Rust");
                    continue;
                }
                _ if trimmed.starts_with(".ast ") => {
                    let code = &trimmed[5..];
                    show_ast(code);
                    continue;
                }
                _ if trimmed.starts_with(".bc ") => {
                    let code = &trimmed[4..];
                    show_bytecode(code);
                    continue;
                }
                _ => {
                    println!("Unknown command: {}", trimmed);
                    println!("Type .help for help");
                    continue;
                }
            }
        }

        if in_multiline {
            multiline_buffer.push_str(&line);
        } else if trimmed.is_empty() {
            continue;
        } else {
            multiline_buffer = line.clone();
        }

        // Check if input is complete
        if !is_input_complete(&multiline_buffer) {
            in_multiline = true;
            continue;
        }

        // Reset multiline state
        in_multiline = false;
        let code = std::mem::take(&mut multiline_buffer);
        let code = code.trim();

        if code.is_empty() {
            continue;
        }

        match runtime.eval(code) {
            Ok(value) => {
                if !value.is_undefined() {
                    println!("{}", format_value(&value));
                }
            }
            Err(e) => {
                eprintln!("\x1b[31m{}\x1b[0m", e);
            }
        }
    }

    println!("\nGoodbye!");
}

/// Check if the input is syntactically complete
fn is_input_complete(input: &str) -> bool {
    let trimmed = input.trim();

    // Empty input is complete
    if trimmed.is_empty() {
        return true;
    }

    // Count braces, brackets, and parens
    let mut brace_count = 0i32;
    let mut bracket_count = 0i32;
    let mut paren_count = 0i32;
    let mut in_string = false;
    let mut string_char = ' ';
    let mut in_template = false;
    let mut escape = false;

    for ch in trimmed.chars() {
        if escape {
            escape = false;
            continue;
        }

        if ch == '\\' {
            escape = true;
            continue;
        }

        if in_string {
            if ch == string_char {
                in_string = false;
            }
            continue;
        }

        if in_template {
            if ch == '`' {
                in_template = false;
            }
            continue;
        }

        match ch {
            '"' | '\'' => {
                in_string = true;
                string_char = ch;
            }
            '`' => {
                in_template = true;
            }
            '{' => brace_count += 1,
            '}' => brace_count -= 1,
            '[' => bracket_count += 1,
            ']' => bracket_count -= 1,
            '(' => paren_count += 1,
            ')' => paren_count -= 1,
            _ => {}
        }
    }

    // Input is complete if all delimiters are balanced and we're not in a string
    !in_string && !in_template && brace_count <= 0 && bracket_count <= 0 && paren_count <= 0
}

/// Format a value for REPL display
fn format_value(value: &quicksilver::Value) -> String {
    use quicksilver::Value;

    match value {
        Value::Undefined => "\x1b[90mundefined\x1b[0m".to_string(),
        Value::Null => "\x1b[1mnull\x1b[0m".to_string(),
        Value::Boolean(b) => format!("\x1b[33m{}\x1b[0m", b),
        Value::Number(n) => format!("\x1b[33m{}\x1b[0m", n),
        Value::String(s) => format!("\x1b[32m'{}'\x1b[0m", s),
        Value::Symbol(id) => format!("\x1b[35mSymbol({})\x1b[0m", id),
        Value::BigInt(n) => format!("\x1b[33m{}n\x1b[0m", n),
        Value::Object(_) => format!("{}", value),
    }
}

fn run_file(path: &PathBuf, profile: bool, script_args: &[String]) {
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading '{}': {}", path.display(), e);
            std::process::exit(1);
        }
    };

    // Canonicalize the path for proper module resolution
    let canonical_path = path.canonicalize().unwrap_or_else(|_| path.clone());

    let mut runtime = Runtime::new();

    // Set up script arguments as process.argv
    set_script_args(&mut runtime, path, script_args);

    let start = if profile { Some(Instant::now()) } else { None };

    match runtime.eval_file(&canonical_path, &source) {
        Ok(_) => {
            if let Some(start) = start {
                let elapsed = start.elapsed();
                eprintln!();
                eprintln!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                eprintln!("📊 Execution Profile");
                eprintln!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                eprintln!("  Total time: {:?}", elapsed);
                eprintln!("  File: {}", path.display());
                eprintln!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
            }
        }
        Err(e) => {
            eprintln!("{}", e.with_source_context(&source));
            std::process::exit(1);
        }
    }
}

fn run_watch(path: &PathBuf, profile: bool, script_args: &[String]) {
    use quicksilver::hmr::HmrRuntime;
    use std::time::Duration;

    println!("🔥 Hot Module Reloading enabled");
    println!("👀 Watching {} for changes...", path.display());
    println!("   Press Ctrl+C to stop\n");

    let canonical_path = path.canonicalize().unwrap_or_else(|_| path.clone());

    // Create HMR runtime and register the module
    let hmr = HmrRuntime::new();
    let module_id = hmr.register_module(&canonical_path);

    // Track module state for HMR
    let mut module_version = 1u64;
    let mut preserved_globals: std::collections::HashMap<String, quicksilver::Value> =
        std::collections::HashMap::new();

    // Run initially
    println!("📦 Loading module v{}...", module_version);
    run_file_with_hmr(path, profile, script_args, &module_id, &mut preserved_globals, true);

    // Watch for changes
    loop {
        std::thread::sleep(Duration::from_millis(500));

        let changes = hmr.check_for_updates();
        if !changes.is_empty() {
            module_version += 1;
            println!("\n🔄 Hot update detected (v{})...", module_version);

            // Apply pending updates
            let results = hmr.apply_pending_updates();
            for result in &results {
                if result.success {
                    println!("   ✓ Module {} updated successfully ({:?})",
                        result.module_id, result.duration);
                    if !result.affected_modules.is_empty() {
                        println!("   ↳ Affected modules: {:?}",
                            result.affected_modules.iter().map(|m| m.to_string()).collect::<Vec<_>>());
                    }
                } else if let Some(ref err) = result.error {
                    println!("   ✗ Update failed: {}", err);
                    println!("   ↳ Performing full reload...");
                }
            }

            // Re-run with state preservation attempt
            run_file_with_hmr(path, profile, script_args, &module_id, &mut preserved_globals, false);
        }
    }
}

fn run_file_with_hmr(
    path: &PathBuf,
    profile: bool,
    script_args: &[String],
    _module_id: &quicksilver::hmr::ModuleId,
    preserved_globals: &mut std::collections::HashMap<String, quicksilver::Value>,
    is_initial: bool,
) {
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading '{}': {}", path.display(), e);
            return;
        }
    };

    let canonical_path = path.canonicalize().unwrap_or_else(|_| path.clone());
    let mut runtime = Runtime::new();

    // Set up HMR API (module.hot object)
    setup_module_hot(&mut runtime, path);

    // Restore preserved globals if this is a hot reload
    if !is_initial && !preserved_globals.is_empty() {
        for (name, value) in preserved_globals.iter() {
            runtime.set_global(name, value.clone());
        }
        println!("   ↳ Restored {} preserved globals", preserved_globals.len());
    }

    // Set up script arguments
    set_script_args(&mut runtime, path, script_args);

    let start = if profile { Some(Instant::now()) } else { None };

    match runtime.eval_file(&canonical_path, &source) {
        Ok(_) => {
            if let Some(start) = start {
                let elapsed = start.elapsed();
                eprintln!();
                eprintln!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                eprintln!("📊 Execution Profile");
                eprintln!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                eprintln!("  Total time: {:?}", elapsed);
                eprintln!("  File: {}", path.display());
                eprintln!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
            }

            // Try to preserve globals marked for HMR
            // Look for __hmr_preserve__ array in globals
            if let Some(quicksilver::Value::Object(obj)) = runtime.get_global("__hmr_preserve__") {
                let borrowed = obj.borrow();
                if let quicksilver::ObjectKind::Array(items) = &borrowed.kind {
                    preserved_globals.clear();
                    for item in items {
                        if let quicksilver::Value::String(name) = item {
                            if let Some(value) = runtime.get_global(name) {
                                preserved_globals.insert(name.clone(), value);
                            }
                        }
                    }
                    if !preserved_globals.is_empty() {
                        println!("   ↳ Marked {} globals for preservation", preserved_globals.len());
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("❌ {}", e);
        }
    }
}

fn setup_module_hot(runtime: &mut Runtime, path: &Path) {
    let module_path = path.display().to_string();

    // Set up HMR API via JavaScript
    // Note: Methods don't set this.* due to RefCell borrow issues with method calls
    // State is tracked externally through the HmrRuntime
    let setup_code = format!(r#"
        var __hmr_preserve__ = [];

        var module = {{
            id: '{}',
            hot: {{
                data: null,
                accept: function() {{
                    // HMR accepts this module for hot updates
                }},
                decline: function() {{
                    // HMR declines updates for this module
                }},
                dispose: function(cb) {{
                    // Dispose handler (placeholder)
                }},
                invalidate: function() {{
                    // Force reload
                }},
                status: function() {{
                    return 'idle';
                }}
            }}
        }};

        function hmrPreserve(name) {{
            __hmr_preserve__.push(name);
        }}
    "#, module_path.replace('"', "\\\""));

    if let Err(e) = runtime.eval(&setup_code) {
        eprintln!("HMR setup error: {}", e);
    }
}

fn set_script_args(runtime: &mut Runtime, path: &Path, script_args: &[String]) {
    // Create process.argv array: [runtime, script, ...args]
    let args_js = format!(
        "globalThis.process = globalThis.process || {{}};
         globalThis.process.argv = ['quicksilver', '{}', {}];",
        path.display(),
        script_args
            .iter()
            .map(|s| format!("'{}'", s.replace('\'', "\\'")))
            .collect::<Vec<_>>()
            .join(", ")
    );

    // Execute the setup code
    let _ = runtime.eval(&args_js);
}

fn eval_code(code: &str) {
    let mut runtime = Runtime::new();
    match runtime.eval(code) {
        Ok(value) => {
            if !value.is_undefined() {
                println!("{}", value);
            }
        }
        Err(e) => {
            eprintln!("{}", e.with_source_context(code));
            std::process::exit(1);
        }
    }
}

fn show_ast(input: &str) {
    let source = if std::path::Path::new(input).exists() {
        match fs::read_to_string(input) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading '{}': {}", input, e);
                return;
            }
        }
    } else {
        input.to_string()
    };

    match quicksilver::parser::parse(&source) {
        Ok(program) => {
            println!("{:#?}", program);
        }
        Err(e) => {
            eprintln!("{}", e);
        }
    }
}

fn show_bytecode(input: &str) {
    let source = if std::path::Path::new(input).exists() {
        match fs::read_to_string(input) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading '{}': {}", input, e);
                return;
            }
        }
    } else {
        input.to_string()
    };

    match quicksilver::bytecode::compile(&source) {
        Ok(chunk) => {
            println!("{}", chunk.disassemble("main"));
        }
        Err(e) => {
            eprintln!("{}", e);
        }
    }
}

fn run_debug(path: &PathBuf) {
    use quicksilver::debugger::TimeTravelDebugger;

    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading '{}': {}", path.display(), e);
            std::process::exit(1);
        }
    };

    // Create runtime
    let mut runtime = quicksilver::Runtime::new();

    // Create and attach debugger
    let debugger = TimeTravelDebugger::new();
    runtime.attach_debugger(debugger);
    runtime.set_source(path.to_str().unwrap_or("unknown"), &source);

    println!("🕐 Quicksilver Time-Travel Debugger");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("File: {}", path.display());
    println!("Running with debugger attached...");
    println!();

    // Execute the code - the VM will record all execution steps
    match runtime.eval(&source) {
        Ok(value) => {
            println!();
            println!("Program completed with result: {}", value.to_js_string());
        }
        Err(e) => {
            println!();
            println!("Program error: {}", e);
        }
    }

    // Get debugger back and show stats
    if let Some(debugger_ref) = runtime.get_debugger() {
        let debugger = debugger_ref.borrow();
        println!();
        println!("Recorded {} execution steps", debugger.history_len());
        println!();
        drop(debugger);

        // Run interactive debugger for replay
        runtime.run_debugger_interactive();
    }
}

fn run_tests(path: &PathBuf, filter: Option<String>, verbose: bool) {
    use quicksilver::test_runner::{TestConfig, TestRunner};

    let config = TestConfig {
        filter,
        verbose,
        ..TestConfig::default()
    };

    let mut runner = TestRunner::new(config);

    match runner.run_file(path) {
        Ok(report) => {
            print!("{}", report);
            if report.failed > 0 {
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("Test runner error: {}", e);
            std::process::exit(1);
        }
    }
}
