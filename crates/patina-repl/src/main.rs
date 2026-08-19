use patina_interpreter::{
    Backend, Interpreter, TreeWalkInterpreter, format_backend_error_with_source,
    format_interpreter_error,
};
use patina_repl::{Repl, make_editor, run_repl_loop};
use patina_vm::{VmBackend, VmBackendError};
use std::env;
use std::fs;
use std::process;

/// Parsed command-line options.
struct CliOptions {
    filename: Option<String>,
    use_tree_walker: bool,
    dump: bool,
    trace: bool,
    /// `-I` directories, in command-line order (first listed = searched first).
    prepend_paths: Vec<String>,
    /// `-A` directories, in command-line order.
    append_paths: Vec<String>,
    /// `-p` expressions, evaluated in order; each result is printed.
    eval_exprs: Vec<String>,
}

fn parse_args(args: &[String]) -> CliOptions {
    let mut opts = CliOptions {
        filename: None,
        use_tree_walker: false,
        dump: false,
        trace: false,
        prepend_paths: Vec::new(),
        append_paths: Vec::new(),
        eval_exprs: Vec::new(),
    };

    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                print_help();
                process::exit(0);
            }
            "--version" => {
                println!("patina {}", env!("CARGO_PKG_VERSION"));
                process::exit(0);
            }
            "--tree-walker" => opts.use_tree_walker = true,
            // Sets the variable the frontend reads, rather than threading a
            // flag down to every Lexer construction site. The variable stays
            // the interface; this is the discoverable spelling of it.
            "--strict-r7rs" => unsafe { std::env::set_var("PATINA_STRICT_R7RS", "1") },
            "--dump" | "--vm-dump" => opts.dump = true,
            "--trace" | "--vm-trace" => opts.trace = true,
            // Accept --vm for backwards compatibility (it's now the default).
            "--vm" => {}
            "-A" => opts
                .append_paths
                .push(require_value(&mut iter, "-A", "a directory")),
            "-I" => opts
                .prepend_paths
                .push(require_value(&mut iter, "-I", "a directory")),
            "-p" => opts
                .eval_exprs
                .push(require_value(&mut iter, "-p", "an expression")),
            _ if !arg.starts_with('-') => opts.filename = Some(arg.clone()),
            _ => {
                eprintln!("Unknown option: {}", arg);
                print_help();
                process::exit(1);
            }
        }
    }
    opts
}

/// Take the value following a flag, or exit with a usage error.
fn require_value(iter: &mut std::slice::Iter<'_, String>, flag: &str, what: &str) -> String {
    iter.next().cloned().unwrap_or_else(|| {
        eprintln!("Error: {} requires {}", flag, what);
        process::exit(1);
    })
}

/// Uniform search-path surface over the two backends' inherent methods
/// (`add_library_search_path` / `prepend_library_search_path` are not on the
/// `Backend` trait).
trait LibraryPaths {
    fn prepend(&self, dir: std::path::PathBuf);
    fn append(&self, dir: std::path::PathBuf);
}

impl LibraryPaths for VmBackend {
    fn prepend(&self, dir: std::path::PathBuf) {
        self.prepend_library_search_path(dir);
    }
    fn append(&self, dir: std::path::PathBuf) {
        self.add_library_search_path(dir);
    }
}

impl LibraryPaths for patina_tree_walker::TreeWalker {
    fn prepend(&self, dir: std::path::PathBuf) {
        self.prepend_library_search_path(dir);
    }
    fn append(&self, dir: std::path::PathBuf) {
        self.add_library_search_path(dir);
    }
}

/// Apply `-I` / `-A` directories — and, for a script run, the script's own
/// directory — to a backend's library search path.
fn apply_library_paths(backend: &dyn LibraryPaths, opts: &CliOptions, script: Option<&str>) {
    // Prepend in reverse so the first -I listed is searched first.
    for dir in opts.prepend_paths.iter().rev() {
        backend.prepend(std::path::PathBuf::from(dir));
    }
    for dir in &opts.append_paths {
        backend.append(std::path::PathBuf::from(dir));
    }
    if let Some(dir) = script.and_then(script_dir) {
        backend.append(dir);
    }
}

/// `-p` mode: apply the search-path flags, evaluate each expression in
/// order, print each non-unspecified result (write representation, like the
/// REPL), and exit.
fn run_eval_print<B: Backend + LibraryPaths>(interp: &Interpreter<B>, opts: &CliOptions) -> ! {
    use patina_tree_walker::eval::format_write_tagged;

    apply_library_paths(interp.backend(), opts, None);
    let heap = interp.backend().global_env().heap().clone();
    for expr in &opts.eval_exprs {
        match interp.eval_program(expr) {
            Ok(v) => {
                if v != patina_core::TaggedValue::UNSPECIFIED {
                    println!("{}", format_write_tagged(v, &heap));
                }
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                process::exit(1);
            }
        }
    }
    process::exit(0);
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let opts = parse_args(&args[1..]);

    if !opts.eval_exprs.is_empty() {
        if opts.filename.is_some() || opts.dump || opts.trace {
            eprintln!("Error: -p cannot be combined with a script file, --dump, or --trace");
            process::exit(1);
        }
        if opts.use_tree_walker {
            run_eval_print(&TreeWalkInterpreter::new_tree_walker(), &opts);
        } else {
            run_eval_print(&Interpreter::new(VmBackend::new()), &opts);
        }
    }

    if let Some(file) = &opts.filename {
        if opts.dump {
            dump_bytecode_file(file);
        } else if opts.trace {
            run_script_vm_trace(file, &opts);
        } else if opts.use_tree_walker {
            run_script_tree_walker(file, &opts);
        } else {
            run_script_vm(file, &opts);
        }
    } else if opts.dump {
        dump_bytecode_stdin();
    } else if opts.use_tree_walker {
        run_repl_tree_walker(&opts);
    } else {
        run_repl_vm(&opts);
    }
}

/// The directory containing the script being run, for program-relative
/// library resolution: a checked-out package runs without an install step
/// because its libraries resolve from beside the program.
fn script_dir(filename: &str) -> Option<std::path::PathBuf> {
    let path = std::path::Path::new(filename);
    let canonical = path.canonicalize().ok()?;
    canonical.parent().map(|p| p.to_path_buf())
}

fn print_help() {
    eprintln!("Usage: patina [OPTIONS] [FILE]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --help, -h     Show this help message");
    eprintln!("  --version      Print the version and exit");
    eprintln!("  --tree-walker  Use the tree-walking backend instead of the VM");
    eprintln!("  --strict-r7rs  Reject the R6RS syntax R7RS reserves: [ ], #vu8(,");
    eprintln!("                 (library ...), and versioned library names");
    eprintln!("  --dump         Compile to bytecode and disassemble (no execution)");
    eprintln!("  --trace        Execute with instruction-level tracing to stderr");
    eprintln!("  -I <dir>       Prepend a directory to the library search path");
    eprintln!("  -A <dir>       Append a directory to the library search path");
    eprintln!("  -p <expr>      Evaluate an expression, print its result, and exit");
    eprintln!("                 (repeatable; evaluated in order)");
    eprintln!();
    eprintln!("Environment:");
    eprintln!("  PATINA_LIBRARY_PATH  Colon-separated library directories, searched");
    eprintln!("                       before the built-in defaults");
    eprintln!("  PATINA_STRICT_R7RS   Same as --strict-r7rs when set to anything but 0");
    eprintln!();
    eprintln!("If FILE is provided, run it as a script.");
    eprintln!("Otherwise, start an interactive REPL.");
    eprintln!();
    eprintln!("The default backend is the register-based bytecode VM.");
    eprintln!("Use --tree-walker to switch to the CPS tree-walking interpreter.");
}

fn run_script_tree_walker(filename: &str, opts: &CliOptions) {
    let code = match fs::read_to_string(filename) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("Error reading file '{}': {}", filename, e);
            process::exit(1);
        }
    };

    let interp = TreeWalkInterpreter::new_tree_walker();
    apply_library_paths(interp.backend(), opts, Some(filename));
    let is_test_file = filename.contains("test") || code.contains("test-begin");

    if is_test_file {
        interp.eval_program_resilient_with_source_name(&code, filename);
        process::exit(0);
    } else {
        let (result, source_map) = interp.eval_program_with_source_name(&code, filename);
        match result {
            Ok(_) => process::exit(0),
            Err(e) => {
                eprintln!(
                    "Error: {}",
                    format_interpreter_error(&e, &source_map.borrow())
                );
                process::exit(1);
            }
        }
    }
}

fn dump_bytecode_file(filename: &str) {
    let code = match fs::read_to_string(filename) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("Error reading file '{}': {}", filename, e);
            process::exit(1);
        }
    };
    dump_bytecode(&code);
}

fn dump_bytecode_stdin() {
    use std::io::Read;
    let mut code = String::new();
    std::io::stdin()
        .read_to_string(&mut code)
        .unwrap_or_else(|e| {
            eprintln!("Error reading stdin: {}", e);
            process::exit(1);
        });
    dump_bytecode(&code);
}

fn dump_bytecode(code: &str) {
    let backend = VmBackend::new();
    match backend.disasm_source(code) {
        Ok(()) => process::exit(0),
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
    }
}

fn run_script_vm_trace(filename: &str, opts: &CliOptions) {
    use patina_vm::tracer::StepTracer;
    use std::cell::RefCell;
    use std::rc::Rc;

    let code = match fs::read_to_string(filename) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("Error reading file '{}': {}", filename, e);
            process::exit(1);
        }
    };

    let backend = VmBackend::new();
    // Create a live-printing tracer, enabled AFTER bootstrap
    let mut tracer = StepTracer::new();
    tracer.print_live = true;
    let handle = Rc::new(RefCell::new(tracer));
    backend.set_tracer(Some(handle.clone()));
    let interp = Interpreter::new(backend);
    apply_library_paths(interp.backend(), opts, Some(filename));
    match interp.eval_program(&code) {
        Ok(_) => process::exit(0),
        Err(e) => {
            eprintln!("Error: {}", e);
            // Print trace summary on error
            let t = handle.borrow();
            eprintln!("--- Trace: {} events recorded ---", t.len());
            process::exit(1);
        }
    }
}

fn run_script_vm(filename: &str, opts: &CliOptions) {
    let code = match fs::read_to_string(filename) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("Error reading file '{}': {}", filename, e);
            process::exit(1);
        }
    };

    let interp = Interpreter::new(VmBackend::new());
    apply_library_paths(interp.backend(), opts, Some(filename));
    let is_test_file = filename.contains("test") || code.contains("test-begin");

    if is_test_file {
        eval_program_resilient_vm(&interp, &code, filename);
        process::exit(0);
    } else {
        let (result, source_map) = eval_program_vm(&interp, &code, filename);
        match result {
            Ok(_) => process::exit(0),
            Err(e) => {
                eprintln!(
                    "Error: {}",
                    format_backend_error_with_source(&e, &source_map.borrow())
                );
                process::exit(1);
            }
        }
    }
}

/// Evaluate a program with source map support for the VM backend.
fn eval_program_vm(
    interp: &Interpreter<VmBackend>,
    input: &str,
    source_name: &str,
) -> (
    Result<patina_core::TaggedValue, patina_interpreter::InterpreterError<VmBackendError>>,
    std::rc::Rc<std::cell::RefCell<patina_interpreter::SourceMap>>,
) {
    use patina_interpreter::{InterpreterError, ParseError, Parser, SourceMap};

    let mut result = patina_core::TaggedValue::UNSPECIFIED;
    let heap = interp.backend().global_env().heap();
    let source_map = std::rc::Rc::new(std::cell::RefCell::new(SourceMap::new()));
    let sname: std::rc::Rc<str> = std::rc::Rc::from(source_name);
    let mut parser =
        match Parser::new_with_source_map(input, heap.clone(), sname, source_map.clone()) {
            Ok(p) => p,
            Err(e) => return (Err(e.into()), source_map),
        };
    let global = interp.backend().global_env().clone();
    loop {
        // Drop SourceMap entries for slots the previous form's evaluation
        // freed, before this iteration's parse can reuse them (§9.1).
        patina_interpreter::prune_freed_locations(heap, &source_map);
        match parser.parse() {
            Ok(expr) => {
                match interp
                    .backend()
                    .eval_with_source_map(expr, &global, &source_map)
                    .map_err(InterpreterError::Backend)
                {
                    Ok(val) => result = val,
                    Err(e) => return (Err(e), source_map),
                }
            }
            Err(ParseError::UnexpectedEof) => break,
            Err(e) => return (Err(e.into()), source_map),
        }
    }
    (Ok(result), source_map)
}

/// Evaluate a program resiliently (continue on errors) with source map support.
fn eval_program_resilient_vm(
    interp: &Interpreter<VmBackend>,
    input: &str,
    source_name: &str,
) -> patina_core::TaggedValue {
    use patina_interpreter::{ParseError, Parser, SourceMap};

    let mut result = patina_core::TaggedValue::UNSPECIFIED;
    let heap = interp.backend().global_env().heap();
    let source_map = std::rc::Rc::new(std::cell::RefCell::new(SourceMap::new()));
    let sname: std::rc::Rc<str> = std::rc::Rc::from(source_name);
    let mut parser =
        match Parser::new_with_source_map(input, heap.clone(), sname, source_map.clone()) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Error: {}", e);
                return result;
            }
        };
    let global = interp.backend().global_env().clone();
    loop {
        // Drop SourceMap entries for slots the previous form's evaluation
        // freed, before this iteration's parse can reuse them (§9.1).
        patina_interpreter::prune_freed_locations(heap, &source_map);
        match parser.parse() {
            Ok(expr) => {
                match interp
                    .backend()
                    .eval_with_source_map(expr, &global, &source_map)
                {
                    Ok(val) => result = val,
                    Err(e) => {
                        if let Some(loc) = e.source_location() {
                            let mut parts = vec![format!("Error: {}", e)];
                            parts.push(format!("  at {}", loc));
                            if let Some(ctx) = source_map.borrow().format_context(loc) {
                                parts.push(ctx);
                            }
                            eprintln!("{}", parts.join("\n"));
                        } else {
                            eprintln!("Error: {}", e);
                        }
                    }
                }
            }
            Err(ParseError::UnexpectedEof) => break,
            Err(e) => {
                eprintln!("Error: {}", e);
            }
        }
    }
    result
}

fn run_repl_tree_walker(opts: &CliOptions) {
    match Repl::new() {
        Ok(mut repl) => {
            apply_library_paths(repl.interpreter().backend(), opts, None);
            if let Err(e) = repl.run() {
                eprintln!("REPL error: {}", e);
                process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("Failed to initialize REPL: {}", e);
            process::exit(1);
        }
    }
}

fn run_repl_vm(opts: &CliOptions) {
    use patina_core::TaggedValue;
    use patina_core::debug_format::format_tagged;
    use patina_interpreter::{ParseError, Parser, SourceMap};

    let interp = Interpreter::new(VmBackend::new());
    apply_library_paths(interp.backend(), opts, None);
    let heap = interp.backend().global_env().heap().clone();

    println!("Patina Scheme R7RS Interpreter");
    println!("Version {}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("Features:");
    println!("  - Full R7RS continuation support (call/cc, dynamic-wind)");
    println!("  - Exception handling (guard, raise)");
    println!("  - Multi-line editing with auto-indentation");
    println!("  - Syntax highlighting");
    println!("  - (vm-compile <expr>) -- disassemble bytecode without executing");
    println!();
    println!("Commands:");
    println!("  (exit) or Ctrl+D to quit");
    println!("  Ctrl+C to cancel current input");
    println!();

    let mut editor = match make_editor() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Failed to initialize editor: {}", e);
            process::exit(1);
        }
    };

    run_repl_loop(&mut editor, "patina> ", |line| {
        // Special form: (vm-compile <expr>) -- compile and disassemble without executing.
        if let Some(rest) = line.strip_prefix("(vm-compile ") {
            let inner = rest.trim_end().strip_suffix(')').unwrap_or(rest.trim_end());
            return match interp.backend().disasm_source(inner) {
                Ok(()) => None,
                Err(e) => Some(format!("Error: {}", e)),
            };
        }

        // Parse with source map for better error reporting.
        let source_map = std::rc::Rc::new(std::cell::RefCell::new(SourceMap::new()));
        let sname: std::rc::Rc<str> = std::rc::Rc::from("<repl>");
        let mut parser =
            match Parser::new_with_source_map(line, heap.clone(), sname, source_map.clone()) {
                Ok(p) => p,
                Err(e) => return Some(format!("Error: {}", e)),
            };
        let global = interp.backend().global_env().clone();
        let mut result = TaggedValue::UNSPECIFIED;
        loop {
            // Drop SourceMap entries for slots the previous form's evaluation
            // freed, before this iteration's parse can reuse them (§9.1).
            patina_interpreter::prune_freed_locations(&heap, &source_map);
            match parser.parse() {
                Ok(expr) => {
                    match interp
                        .backend()
                        .eval_with_source_map(expr, &global, &source_map)
                    {
                        Ok(val) => result = val,
                        Err(e) => {
                            if let Some(loc) = e.source_location() {
                                let mut parts = vec![format!("Error: {}", e)];
                                parts.push(format!("  at {}", loc));
                                if let Some(ctx) = source_map.borrow().format_context(loc) {
                                    parts.push(ctx);
                                }
                                return Some(parts.join("\n"));
                            }
                            return Some(format!("Error: {}", e));
                        }
                    }
                }
                Err(ParseError::UnexpectedEof) => break,
                Err(e) => return Some(format!("Error: {}", e)),
            }
        }
        if result != TaggedValue::UNSPECIFIED {
            Some(format_tagged(result, &heap.borrow()))
        } else {
            None
        }
    });
}
