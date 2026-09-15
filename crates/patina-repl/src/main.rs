use patina_core::TaggedValue;
use patina_interpreter::{
    Backend, Interpreter, InterpreterError, ProgramOutcome, SourceMap, TreeWalkInterpreter,
    format_backend_error_with_source, format_eval_error_with_source, format_interpreter_error,
};
use patina_repl::repl::needs_more_input;
use patina_repl::{Repl, make_editor, run_program_stream, run_repl_loop};
use patina_vm::tracer::StepTracer;
use patina_vm::{VmBackend, VmBackendError};
use std::cell::RefCell;
use std::env;
use std::fs;
use std::process;
use std::rc::Rc;

/// Parsed command-line options.
struct CliOptions {
    filename: Option<String>,
    use_tree_walker: bool,
    /// `-i`: take the interactive session even though standard input is not
    /// a terminal.
    interactive: bool,
    dump: bool,
    trace: bool,
    /// `-I` directories, in command-line order (first listed = searched first).
    prepend_paths: Vec<String>,
    /// `-A` directories, in command-line order.
    append_paths: Vec<String>,
    /// `-p` expressions, evaluated in order; each result is printed.
    eval_exprs: Vec<String>,
    /// `-k`: report each evaluation error and go on to the next top-level form
    /// instead of stopping at the first. It changes how much of a program runs,
    /// never what its exit status says.
    keep_going: bool,
}

fn parse_args(args: &[String]) -> CliOptions {
    let mut opts = CliOptions {
        filename: None,
        use_tree_walker: false,
        interactive: false,
        dump: false,
        trace: false,
        prepend_paths: Vec::new(),
        append_paths: Vec::new(),
        eval_exprs: Vec::new(),
        keep_going: false,
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
            "--interactive" | "-i" => opts.interactive = true,
            "--keep-going" | "-k" => opts.keep_going = true,
            // Set before constructing either backend: bootstrap itself must
            // not resolve through a user's environment or project directory.
            // SAFETY: argument parsing runs before any threads are started.
            "--isolated-libraries" => unsafe {
                std::env::set_var("PATINA_ISOLATED_LIBRARIES", "1")
            },
            // Sets the variable the frontend reads, rather than threading a
            // flag down to every Lexer construction site. The variable stays
            // the interface; this is the discoverable spelling of it.
            "--allow-r6rs" => unsafe { std::env::set_var("PATINA_ALLOW_R6RS", "1") },
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
    if !patina_runtime::LibraryRegistry::isolated_paths_enabled()
        && let Some(dir) = script.and_then(script_dir)
    {
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

    // Tracing is a VM instrument (`patina_vm::tracer`), so the two flags
    // together cannot both be honoured. Saying so beats running the other
    // backend and reporting nothing, which is what silently dropping one of
    // them amounted to.
    if opts.trace && opts.use_tree_walker {
        eprintln!("Error: --trace traces the VM and cannot be combined with --tree-walker");
        process::exit(1);
    }
    // -k decides how much of a program runs, so it needs a program that runs:
    // `-p` stops at its first error by design, and `--dump` runs nothing.
    if opts.keep_going && (!opts.eval_exprs.is_empty() || opts.dump) {
        eprintln!("Error: -k runs a program, and cannot be combined with -p or --dump");
        process::exit(1);
    }
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
    } else if opts.interactive || std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        // A terminal is a person, so it gets the session. `-i` says so for the
        // contexts where a person is at the other end of a pipe anyway — a
        // container without a tty, an editor's inferior-Scheme buffer.
        if opts.keep_going {
            // A session already reports each error and carries on, so the flag
            // could change nothing here.
            eprintln!("Error: -k runs a program, and a session already carries on past errors");
            process::exit(1);
        }
        if opts.use_tree_walker {
            run_repl_tree_walker(&opts);
        } else {
            run_repl_vm(&opts);
        }
    } else {
        run_stdin_program(&opts);
    }
}

/// Run a program read from standard input.
///
/// It is not read whole first: each form runs as soon as the line that ends it
/// arrives, so a producer that waits on the program's output makes progress,
/// and the reader holds the largest form rather than the stream (#333), though
/// the VM still keeps the compiled code of every form it has run (#338). It
/// gets a program's diagnostics and exit status, not a session's: a line
/// editor reading a pipe dropped an unfinished last form in silence and
/// exited 0 (#331).
///
/// Two things still separate this from `patina program.scm`, because a
/// redirect does not carry what a path carries:
///
/// - there is no directory to resolve libraries beside, so `-I`/`-A` and the
///   search path are all a program here has;
/// - the program's text and its own input are one stream, so a read from
///   standard input inside it continues right after the form being run, as it
///   does in chibi and Gauche.
fn run_stdin_program(opts: &CliOptions) -> ! {
    let outcome = if opts.trace {
        let (interp, tracer) = traced_vm();
        apply_library_paths(interp.backend(), opts, None);
        let outcome = stream_stdin(&interp, opts.keep_going);
        if !outcome.clean() {
            eprintln!("--- Trace: {} events recorded ---", tracer.borrow().len());
        }
        outcome
    } else if opts.use_tree_walker {
        let interp = TreeWalkInterpreter::new_tree_walker();
        apply_library_paths(interp.backend(), opts, None);
        stream_stdin(&interp, opts.keep_going)
    } else {
        let interp = Interpreter::new(VmBackend::new());
        apply_library_paths(interp.backend(), opts, None);
        stream_stdin(&interp, opts.keep_going)
    };
    process::exit(if outcome.clean() { 0 } else { 1 });
}

/// Run standard input as a program on `interp`, each form as it arrives.
fn stream_stdin<B: Backend + EvalSourceMapped>(
    interp: &Interpreter<B>,
    keep_going: bool,
) -> ProgramOutcome {
    let heap = interp.backend().global_env().heap().clone();
    let input = patina_runtime::Port::stdin();
    run_program_stream(&input, &heap, "<stdin>", keep_going, |datum, source_map| {
        interp.backend().eval_source_mapped(datum, source_map)
    })
}

/// Evaluate a datum with its source positions, rendering an error for
/// printing: both backends have `eval_with_source_map`, but `Backend` does
/// not, and each renders its own error type.
trait EvalSourceMapped {
    fn eval_source_mapped(
        &self,
        datum: TaggedValue,
        source_map: &Rc<RefCell<SourceMap>>,
    ) -> Result<(), String>;
}

impl EvalSourceMapped for VmBackend {
    fn eval_source_mapped(
        &self,
        datum: TaggedValue,
        source_map: &Rc<RefCell<SourceMap>>,
    ) -> Result<(), String> {
        let result = self.eval_with_source_map(datum, self.global_env(), source_map);
        result.map(|_| ()).map_err(|e| {
            format_backend_error_with_source(&InterpreterError::Backend(e), &source_map.borrow())
        })
    }
}

impl EvalSourceMapped for patina_tree_walker::TreeWalker {
    fn eval_source_mapped(
        &self,
        datum: TaggedValue,
        source_map: &Rc<RefCell<SourceMap>>,
    ) -> Result<(), String> {
        let result = self.eval_with_source_map(datum, self.global_env(), source_map);
        result
            .map(|_| ())
            .map_err(|e| format_eval_error_with_source(&e, &source_map.borrow()))
    }
}

/// A VM interpreter that traces each instruction to stderr as it runs, with
/// the tracer, whose count a traced run reports when the program fails. The
/// tracer is installed after the backend has bootstrapped, so the trace starts
/// with the program.
fn traced_vm() -> (Interpreter<VmBackend>, Rc<RefCell<StepTracer>>) {
    let backend = VmBackend::new();
    let mut tracer = StepTracer::new();
    tracer.print_live = true;
    let handle = Rc::new(RefCell::new(tracer));
    backend.set_tracer(Some(handle.clone()));
    (Interpreter::new(backend), handle)
}

/// Read a source file, or exit saying why.
fn read_source_file(filename: &str) -> String {
    match fs::read_to_string(filename) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("Error reading file '{}': {}", filename, e);
            process::exit(1);
        }
    }
}

/// Read all of standard input as source, or exit saying why.
fn read_stdin_source() -> String {
    use std::io::Read;

    let mut code = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut code) {
        eprintln!("Error reading stdin: {}", e);
        process::exit(1);
    }
    code
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
    eprintln!("  -i, --interactive  Start the REPL even when stdin is not a terminal");
    eprintln!("  --allow-r6rs   Also read the R6RS syntax R7RS reserves: [ ], #vu8(,");
    eprintln!("                 (library ...), and versioned library names");
    eprintln!("  --dump         Compile to bytecode and disassemble (no execution)");
    eprintln!("  --trace        Execute with instruction-level tracing to stderr");
    eprintln!("  -k, --keep-going  Report each error and run the next top-level form");
    eprintln!("                 anyway; the exit status still reports the failure");
    eprintln!("  -I <dir>       Prepend a directory to the library search path");
    eprintln!("  -A <dir>       Append a directory to the library search path");
    eprintln!("  --isolated-libraries  Use bundled roots and explicit -I/-A paths only");
    eprintln!(
        "                        Ignore user paths, ./lib, ./.patina/lib and script directory"
    );
    eprintln!("  -p <expr>      Evaluate an expression, print its result, and exit");
    eprintln!("                 (repeatable; evaluated in order)");
    eprintln!();
    eprintln!("Environment:");
    eprintln!("  PATINA_LIBRARY_PATH  Colon-separated library directories, searched");
    eprintln!("                       before the built-in defaults");
    eprintln!("  PATINA_ALLOW_R6RS    Same as --allow-r6rs when set to anything but 0");
    eprintln!("  PATINA_ISOLATED_LIBRARIES  Same as --isolated-libraries when set to 1");
    eprintln!();
    eprintln!("If FILE is provided, run it as a script. A program that reports an error");
    eprintln!("exits non-zero, with or without -k, even if it later calls (exit 0).");
    eprintln!("Otherwise, read a program from standard input, or start an");
    eprintln!("interactive REPL when standard input is a terminal or -i is given.");
    eprintln!("A program read from standard input runs each form as soon as the line");
    eprintln!("that ends it arrives, and shares that input with its own reads, which");
    eprintln!("continue right after the form being run. It cannot resolve libraries");
    eprintln!("beside itself; pass it as FILE if it must.");
    eprintln!();
    eprintln!("The default backend is the register-based bytecode VM.");
    eprintln!("Use --tree-walker to switch to the CPS tree-walking interpreter.");
}

fn run_script_tree_walker(filename: &str, opts: &CliOptions) -> ! {
    let code = read_source_file(filename);
    let interp = TreeWalkInterpreter::new_tree_walker();
    apply_library_paths(interp.backend(), opts, Some(filename));
    if opts.keep_going {
        // -k is a recovery policy, not a verdict: every error is reported and
        // the next top-level form runs anyway, and the status still says the
        // program failed.
        let (_, outcome) = interp.eval_program_resilient_with_source_name(&code, filename);
        process::exit(if outcome.clean() { 0 } else { 1 });
    }

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

fn dump_bytecode_file(filename: &str) -> ! {
    dump_bytecode(&read_source_file(filename));
}

fn dump_bytecode_stdin() -> ! {
    dump_bytecode(&read_stdin_source());
}

fn dump_bytecode(code: &str) -> ! {
    let backend = VmBackend::new();
    match backend.disasm_source(code) {
        Ok(()) => process::exit(0),
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
    }
}

/// Run a script on the VM under `--trace`. Tracing is a VM instrument, which
/// is why `main` refuses `--trace` with `--tree-walker` rather than quietly
/// running the other backend.
fn run_script_vm_trace(filename: &str, opts: &CliOptions) -> ! {
    let code = read_source_file(filename);
    let (interp, tracer) = traced_vm();
    apply_library_paths(interp.backend(), opts, Some(filename));
    // -k reaches tracing too, so a file gets the same status traced and
    // untraced.
    if opts.keep_going {
        let outcome = eval_program_keep_going_vm(&interp, &code, filename);
        if !outcome.clean() {
            eprintln!("--- Trace: {} events recorded ---", tracer.borrow().len());
        }
        process::exit(if outcome.clean() { 0 } else { 1 });
    }
    let (result, source_map) = eval_program_vm(&interp, &code, filename);
    match result {
        Ok(_) => process::exit(0),
        Err(e) => {
            eprintln!(
                "Error: {}",
                format_backend_error_with_source(&e, &source_map.borrow())
            );
            eprintln!("--- Trace: {} events recorded ---", tracer.borrow().len());
            process::exit(1);
        }
    }
}

/// The VM's counterpart to [`run_script_tree_walker`].
fn run_script_vm(filename: &str, opts: &CliOptions) -> ! {
    let code = read_source_file(filename);
    let interp = Interpreter::new(VmBackend::new());
    apply_library_paths(interp.backend(), opts, Some(filename));
    if opts.keep_going {
        // As in `run_script_tree_walker`: carrying on past an error does not
        // stop the status from reporting it.
        let outcome = eval_program_keep_going_vm(&interp, &code, filename);
        process::exit(if outcome.clean() { 0 } else { 1 });
    }

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

/// Evaluate a program with source map support for the VM backend.
fn eval_program_vm(
    interp: &Interpreter<VmBackend>,
    input: &str,
    source_name: &str,
) -> (
    Result<patina_core::TaggedValue, patina_interpreter::InterpreterError<VmBackendError>>,
    std::rc::Rc<std::cell::RefCell<patina_interpreter::SourceMap>>,
) {
    use patina_interpreter::{InterpreterError, Parser, SourceMap};

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
        match parser.parse_next() {
            Ok(Some(expr)) => {
                match interp
                    .backend()
                    .eval_with_source_map(expr, &global, &source_map)
                    .map_err(InterpreterError::Backend)
                {
                    Ok(val) => result = val,
                    Err(e) => return (Err(e), source_map),
                }
            }
            Ok(None) => break,
            Err(e) => return (Err(e.into()), source_map),
        }
    }
    (Ok(result), source_map)
}

/// Evaluate a whole program under `-k`: report each error and go on to the
/// next top-level form, then say how it went.
///
/// The VM's counterpart to `Interpreter::eval_program_resilient_with_source_name`.
/// Like it, each error is recorded process-wide, so a later `(exit 0)` cannot
/// report success.
fn eval_program_keep_going_vm(
    interp: &Interpreter<VmBackend>,
    input: &str,
    source_name: &str,
) -> patina_interpreter::ProgramOutcome {
    use patina_interpreter::{
        InterpreterError, Parser, ProgramOutcome, SourceMap, format_backend_error_with_source,
    };

    let heap = interp.backend().global_env().heap();
    let mut eval_errors = 0usize;
    let source_map = std::rc::Rc::new(std::cell::RefCell::new(SourceMap::new()));
    let sname: std::rc::Rc<str> = std::rc::Rc::from(source_name);
    let mut parser =
        match Parser::new_with_source_map(input, heap.clone(), sname, source_map.clone()) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Error: {}", e);
                patina_runtime::exit_status::note_error_reported();
                return ProgramOutcome {
                    eval_errors,
                    read_to_end: false,
                };
            }
        };
    let global = interp.backend().global_env().clone();
    loop {
        // Drop SourceMap entries for slots the previous form's evaluation
        // freed, before this iteration's parse can reuse them (§9.1).
        patina_interpreter::prune_freed_locations(heap, &source_map);
        match parser.parse_next() {
            Ok(Some(expr)) => {
                if let Err(e) = interp
                    .backend()
                    .eval_with_source_map(expr, &global, &source_map)
                {
                    eval_errors += 1;
                    patina_runtime::exit_status::note_error_reported();
                    eprintln!(
                        "Error: {}",
                        format_backend_error_with_source(
                            &InterpreterError::Backend(e),
                            &source_map.borrow()
                        )
                    );
                }
            }
            Ok(None) => {
                return ProgramOutcome {
                    eval_errors,
                    read_to_end: true,
                };
            }
            Err(e) => {
                // The parser leaves the offending token where it was, so
                // reading on would report it again, without end.
                eprintln!(
                    "Error: {}",
                    patina_interpreter::format_parse_error_with_source(&e, &source_map.borrow())
                );
                patina_runtime::exit_status::note_error_reported();
                return ProgramOutcome {
                    eval_errors,
                    read_to_end: false,
                };
            }
        }
    }
}

fn run_repl_tree_walker(opts: &CliOptions) {
    match Repl::new() {
        Ok(mut repl) => {
            apply_library_paths(repl.interpreter().backend(), opts, None);
            if !repl.run() {
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
    use patina_core::debug_format::format_tagged;
    use patina_interpreter::{Parser, format_parse_error_with_source};

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

    let clean = run_repl_loop(&mut editor, "patina> ", |line| {
        // Special form: (vm-compile <expr>) -- compile and disassemble without executing.
        // Only finished input takes it: a session cut off part-way through one
        // is reported as the unfinished form it is.
        if let Some(rest) = line.strip_prefix("(vm-compile ")
            && !needs_more_input(line)
        {
            let inner = rest.trim_end().strip_suffix(')').unwrap_or(rest.trim_end());
            return match interp.backend().disasm_source(inner) {
                Ok(()) => None,
                Err(e) => Some(format!("Error: {}", e)),
            };
        }

        // Parse with source map for better error reporting.
        let source_map = Rc::new(RefCell::new(SourceMap::new()));
        let sname: Rc<str> = Rc::from("<repl>");
        let mut parser =
            match Parser::new_with_source_map(line, heap.clone(), sname, source_map.clone()) {
                Ok(p) => p,
                Err(e) => {
                    return Some(format!(
                        "Error: {}",
                        format_parse_error_with_source(&e, &source_map.borrow())
                    ));
                }
            };
        let global = interp.backend().global_env().clone();
        let mut result = TaggedValue::UNSPECIFIED;
        loop {
            // Drop SourceMap entries for slots the previous form's evaluation
            // freed, before this iteration's parse can reuse them (§9.1).
            patina_interpreter::prune_freed_locations(&heap, &source_map);
            match parser.parse_next() {
                Ok(Some(expr)) => {
                    match interp
                        .backend()
                        .eval_with_source_map(expr, &global, &source_map)
                    {
                        Ok(val) => result = val,
                        Err(e) => {
                            return Some(format!(
                                "Error: {}",
                                format_backend_error_with_source(
                                    &InterpreterError::Backend(e),
                                    &source_map.borrow()
                                )
                            ));
                        }
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    return Some(format!(
                        "Error: {}",
                        format_parse_error_with_source(&e, &source_map.borrow())
                    ));
                }
            }
        }
        if result != TaggedValue::UNSPECIFIED {
            Some(format_tagged(result, &heap.borrow()))
        } else {
            None
        }
    });
    if !clean {
        process::exit(1);
    }
}
