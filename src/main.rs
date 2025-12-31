//! Quicksilver CLI
//!
//! A command-line interface for the Quicksilver JavaScript runtime.

use clap::{Parser, Subcommand};
use quicksilver::{Runtime, VERSION};
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
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
            eprintln!("Error reading file: {}", e);
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
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}

fn run_watch(path: &PathBuf, profile: bool, script_args: &[String]) {
    use quicksilver::hmr::FileWatcher;
    use std::time::Duration;

    println!("👀 Watching {} for changes...", path.display());
    println!("   Press Ctrl+C to stop\n");

    let watcher = FileWatcher::default();
    let canonical_path = path.canonicalize().unwrap_or_else(|_| path.clone());
    watcher.watch(&canonical_path);

    // Run once initially
    run_file(path, profile, script_args);

    // Watch for changes
    loop {
        std::thread::sleep(Duration::from_millis(500));

        let changes = watcher.poll();
        if !changes.is_empty() {
            println!("\n🔄 File changed, re-running...\n");
            run_file(path, profile, script_args);
        }
    }
}

fn set_script_args(runtime: &mut Runtime, path: &PathBuf, script_args: &[String]) {
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
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}

fn show_ast(input: &str) {
    let source = if std::path::Path::new(input).exists() {
        match fs::read_to_string(input) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading file: {}", e);
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
                eprintln!("Error reading file: {}", e);
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
    use rustc_hash::FxHashMap as HashMap;

    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading file: {}", e);
            std::process::exit(1);
        }
    };

    // Create debugger
    let mut debugger = TimeTravelDebugger::new();
    debugger.load_source(path.to_str().unwrap_or("unknown"), &source);

    // Compile the code
    let chunk = match quicksilver::bytecode::compile(&source) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Compilation error: {}", e);
            std::process::exit(1);
        }
    };

    // Simulate recording execution steps based on bytecode
    // In a full implementation, the VM would call debugger.record_step() during execution
    let mut line = 1u32;

    // Record initial state
    let locals: HashMap<String, quicksilver::Value> = HashMap::default();
    debugger.record_step(None, 0, 1, &[], &locals, "Program start");

    // Simulate some execution steps based on bytecode lines
    for i in 0..chunk.code.len().min(100) {
        if let Some(l) = chunk.lines.get(i) {
            if *l != line {
                line = *l;
                debugger.record_step(
                    None, // Opcode (would be filled in by actual VM execution)
                    i,
                    line,
                    &[],
                    &locals,
                    &format!("Executing line {}", line),
                );
            }
        }
    }

    // Add final step
    debugger.record_step(None, chunk.code.len(), line, &[], &locals, "Program end");

    println!("🕐 Quicksilver Time-Travel Debugger");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("File: {}", path.display());
    println!("Recorded {} execution steps", debugger.history_len());
    println!();

    // Run interactive debugger
    debugger.run_interactive();
}
