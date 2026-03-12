use patina_interpreter::{Backend, Interpreter, TreeWalkInterpreter, format_interpreter_error};
use patina_repl::{Repl, make_editor, run_repl_loop};
use patina_vm::VmBackend;
use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut filename: Option<String> = None;
    let mut use_vm = false;
    let mut vm_dump = false;
    let mut vm_trace = false;

    for arg in args.iter().skip(1) {
        if arg == "--help" || arg == "-h" {
            print_help();
            process::exit(0);
        } else if arg == "--vm" {
            use_vm = true;
        } else if arg == "--vm-dump" {
            use_vm = true;
            vm_dump = true;
        } else if arg == "--vm-trace" {
            use_vm = true;
            vm_trace = true;
        } else if !arg.starts_with('-') {
            filename = Some(arg.clone());
        } else {
            eprintln!("Unknown option: {}", arg);
            print_help();
            process::exit(1);
        }
    }

    if let Some(file) = filename {
        if vm_dump {
            dump_bytecode_file(&file);
        } else if vm_trace {
            run_script_vm_trace(&file);
        } else if use_vm {
            run_script_vm(&file);
        } else {
            run_script(&file);
        }
    } else if vm_dump {
        dump_bytecode_stdin();
    } else if use_vm {
        run_repl_vm();
    } else {
        run_repl();
    }
}

fn print_help() {
    eprintln!("Usage: patina [OPTIONS] [FILE]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --help     Show this help message");
    eprintln!("  --vm       Use the VM backend (experimental)");
    eprintln!("  --vm-dump  Compile to bytecode and disassemble (no execution)");
    eprintln!("  --vm-trace Execute with instruction-level tracing to stderr");
    eprintln!();
    eprintln!("If FILE is provided, run it as a script.");
    eprintln!("Otherwise, start an interactive REPL.");
    eprintln!();
    eprintln!("Features:");
    eprintln!("  - Full R7RS continuation support (call/cc, dynamic-wind)");
    eprintln!("  - Exception handling (guard, raise)");
}

fn run_script(filename: &str) {
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
    match interp.eval_program(&code) {
        Ok(_) => process::exit(0),
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
    }
}

fn run_repl() {
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

    let interp = Interpreter::new(VmBackend::new());
    let heap = interp.backend().global_env().heap().clone();

    println!("Patina Scheme (VM backend — experimental)");
    println!();
    println!("Features:");
    println!("  • Multi-line editing with auto-indentation");
    println!("  • Syntax highlighting");
    println!("  • (vm-compile <expr>) — disassemble bytecode without executing");
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

    run_repl_loop(&mut editor, "patina/vm> ", |line| {
        // Special form: (vm-compile <expr>) — compile and disassemble without executing.
        if let Some(rest) = line.strip_prefix("(vm-compile ") {
            let inner = rest.trim_end().strip_suffix(')').unwrap_or(rest.trim_end());
            return match interp.backend().disasm_source(inner) {
                Ok(()) => None,
                Err(e) => Some(format!("Error: {}", e)),
            };
        }

        match interp.eval_program(line) {
            Ok(result) => {
                if result != TaggedValue::UNSPECIFIED {
                    Some(format_tagged(result, &heap.borrow()))
                } else {
                    None
                }
            }
            Err(e) => Some(format!("Error: {}", e)),
        }
    });
}
