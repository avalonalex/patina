use patina_core::TaggedValue;
use patina_interpreter::{
    Backend, HasSourceLocation, Interpreter, ProgramOutcome, TreeWalkInterpreter,
    format_backend_error_with_source, format_error_with_source,
};
use patina_repl::repl::needs_more_input;
use patina_repl::{Repl, make_editor, run_program_stream, run_repl_loop};
use patina_vm::VmBackend;
use patina_vm::tracer::StepTracer;
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
                patina_runtime::exit_status::exit_if_interrupted();
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
        }
        let code = read_source_file(file);
        run(
            Program::Script {
                code: &code,
                filename: file,
            },
            &opts,
        );
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
        run(Program::Stdin, &opts);
    }
}

/// What a run evaluates as a program.
enum Program<'a> {
    /// A script's text, read whole, and the file it was read from.
    Script { code: &'a str, filename: &'a str },
    /// Standard input, run as it arrives.
    Stdin,
}

/// Run `program` on the backend the options select, and exit with its status:
/// 0 when it ran cleanly, 1 when it reported an error, with or without `-k`.
fn run(program: Program<'_>, opts: &CliOptions) -> ! {
    // A script's own directory is searched for its libraries.
    let script = match program {
        Program::Script { filename, .. } => Some(filename),
        Program::Stdin => None,
    };
    let clean = if opts.trace {
        // Tracing is a VM instrument, which is why `main` refuses `--trace`
        // with `--tree-walker` rather than quietly running the other backend.
        let (interp, tracer) = traced_vm();
        apply_library_paths(interp.backend(), opts, script);
        let clean = run_program(&interp, &program, opts.keep_going);
        if !clean {
            eprintln!("--- Trace: {} events recorded ---", tracer.borrow().len());
        }
        clean
    } else if opts.use_tree_walker {
        let interp = TreeWalkInterpreter::new_tree_walker();
        apply_library_paths(interp.backend(), opts, script);
        run_program(&interp, &program, opts.keep_going)
    } else {
        let interp = Interpreter::new(VmBackend::new());
        apply_library_paths(interp.backend(), opts, script);
        run_program(&interp, &program, opts.keep_going)
    };
    // An error that interrupted an `exit` stopped the program; the exit it
    // asked for decides the status, now that the trace count is out.
    patina_runtime::exit_status::exit_if_interrupted();
    process::exit(if clean { 0 } else { 1 });
}

/// Run `program` on `interp`, reporting each error as it arises, and say
/// whether it ran cleanly.
///
/// `keep_going` is `-k`, a recovery policy and not a verdict: every error is
/// reported and the next top-level form runs anyway, and a program that
/// reported one still did not run cleanly.
fn run_program<B: Backend>(interp: &Interpreter<B>, program: &Program<'_>, keep_going: bool) -> bool
where
    B::Error: HasSourceLocation,
{
    match *program {
        Program::Script { code, filename } if keep_going => interp
            .eval_program_resilient_with_source_name(code, filename)
            .1
            .clean(),
        Program::Script { code, filename } => {
            let (result, source_map) = interp.eval_program_with_source_name(code, filename);
            match result {
                Ok(_) => true,
                Err(e) => {
                    eprintln!(
                        "Error: {}",
                        format_backend_error_with_source(&e, &source_map.borrow())
                    );
                    false
                }
            }
        }
        Program::Stdin => stream_stdin(interp, keep_going).clean(),
    }
}

/// Run standard input as a program on `interp`.
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
fn stream_stdin<B: Backend>(interp: &Interpreter<B>, keep_going: bool) -> ProgramOutcome
where
    B::Error: HasSourceLocation,
{
    let backend = interp.backend();
    let heap = backend.global_env().heap().clone();
    let input = patina_runtime::Port::stdin();
    run_program_stream(&input, &heap, "<stdin>", keep_going, |datum, source_map| {
        backend
            .eval_with_source_map(datum, backend.global_env(), source_map)
            .map(|_| ())
            .map_err(|e| format_error_with_source(&e, &source_map.borrow()))
    })
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

        let (result, source_map) = interp.eval_program_with_source_name(line, "<repl>");
        match result {
            Ok(value) if value == TaggedValue::UNSPECIFIED => None,
            Ok(value) => Some(format_tagged(value, &heap.borrow())),
            Err(e) => Some(format!(
                "Error: {}",
                format_backend_error_with_source(&e, &source_map.borrow())
            )),
        }
    });
    if !clean {
        process::exit(1);
    }
}
