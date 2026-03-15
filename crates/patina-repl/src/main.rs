use patina_interpreter::{
    Backend, Interpreter, TreeWalkInterpreter, format_backend_error_with_source,
    format_interpreter_error,
};
use patina_repl::{Repl, make_editor, run_repl_loop};
use patina_vm::{VmBackend, VmBackendError};
use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut filename: Option<String> = None;
    let mut use_tree_walker = false;
    let mut dump = false;
    let mut trace = false;

    for arg in args.iter().skip(1) {
        if arg == "--help" || arg == "-h" {
            print_help();
            process::exit(0);
        } else if arg == "--tree-walker" {
            use_tree_walker = true;
        } else if arg == "--dump" || arg == "--vm-dump" {
            dump = true;
        } else if arg == "--trace" || arg == "--vm-trace" {
            trace = true;
        } else if arg == "--vm" {
            // Accept --vm for backwards compatibility (it's now the default).
            // No-op.
        } else if !arg.starts_with('-') {
            filename = Some(arg.clone());
        } else {
            eprintln!("Unknown option: {}", arg);
            print_help();
            process::exit(1);
        }
    }

    if let Some(file) = filename {
        if dump {
            dump_bytecode_file(&file);
        } else if trace {
            run_script_vm_trace(&file);
        } else if use_tree_walker {
            run_script_tree_walker(&file);
        } else {
            run_script_vm(&file);
        }
    } else if dump {
        dump_bytecode_stdin();
    } else if use_tree_walker {
        run_repl_tree_walker();
    } else {
        run_repl_vm();
    }
}

fn print_help() {
    eprintln!("Usage: patina [OPTIONS] [FILE]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --help         Show this help message");
    eprintln!("  --tree-walker  Use the tree-walking backend instead of the VM");
    eprintln!("  --dump         Compile to bytecode and disassemble (no execution)");
    eprintln!("  --trace        Execute with instruction-level tracing to stderr");
    eprintln!();
    eprintln!("If FILE is provided, run it as a script.");
    eprintln!("Otherwise, start an interactive REPL.");
    eprintln!();
    eprintln!("The default backend is the register-based bytecode VM.");
    eprintln!("Use --tree-walker to switch to the CPS tree-walking interpreter.");
}

fn run_script_tree_walker(filename: &str) {
    let code = match fs::read_to_string(filename) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("Error reading file '{}': {}", filename, e);
            process::exit(1);
        }
    };

    let interp = TreeWalkInterpreter::new_tree_walker();
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

fn run_script_vm_trace(filename: &str) {
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

fn run_script_vm(filename: &str) {
    let code = match fs::read_to_string(filename) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("Error reading file '{}': {}", filename, e);
            process::exit(1);
        }
    };

    let interp = Interpreter::new(VmBackend::new());
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
    use patina_runtime::stdlib::test_increment_error;

    let mut result = patina_core::TaggedValue::UNSPECIFIED;
    let heap = interp.backend().global_env().heap();
    let source_map = std::rc::Rc::new(std::cell::RefCell::new(SourceMap::new()));
    let sname: std::rc::Rc<str> = std::rc::Rc::from(source_name);
    let mut parser =
        match Parser::new_with_source_map(input, heap.clone(), sname, source_map.clone()) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("Error: {}", e);
                test_increment_error();
                return result;
            }
        };
    let global = interp.backend().global_env().clone();
    loop {
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
                        test_increment_error();
                    }
                }
            }
            Err(ParseError::UnexpectedEof) => break,
            Err(e) => {
                eprintln!("Error: {}", e);
                test_increment_error();
            }
        }
    }
    result
}

fn run_repl_tree_walker() {
    match Repl::new() {
        Ok(mut repl) => {
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

fn run_repl_vm() {
    use patina_core::TaggedValue;
    use patina_core::debug_format::format_tagged;
    use patina_interpreter::{ParseError, Parser, SourceMap};

    let interp = Interpreter::new(VmBackend::new());
    let heap = interp.backend().global_env().heap().clone();

    println!("Patina Scheme R7RS Interpreter");
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
