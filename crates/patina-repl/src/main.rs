use patina_interpreter::{Backend, Interpreter, TreeWalkInterpreter, format_interpreter_error};
use patina_repl::Repl;
use patina_vm::VmBackend;
use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut filename: Option<String> = None;
    let mut use_vm = false;

    for arg in args.iter().skip(1) {
        if arg == "--help" || arg == "-h" {
            print_help();
            process::exit(0);
        } else if arg == "--vm" {
            use_vm = true;
        } else if !arg.starts_with('-') {
            filename = Some(arg.clone());
        } else {
            eprintln!("Unknown option: {}", arg);
            print_help();
            process::exit(1);
        }
    }

    if let Some(file) = filename {
        if use_vm {
            run_script_vm(&file);
        } else {
            run_script(&file);
        }
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
    eprintln!("  --help    Show this help message");
    eprintln!("  --vm      Use the VM backend (experimental)");
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
    use std::io::{self, BufRead, Write};

    let interp = Interpreter::new(VmBackend::new());
    let heap = interp.backend().global_env().heap().clone();

    println!("Patina Scheme (VM backend — experimental)");
    println!("Type (exit) or Ctrl+D to quit.");
    println!();

    let stdin = io::stdin();
    loop {
        print!("patina/vm> ");
        let _ = io::stdout().flush();

        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => {
                println!("\nGoodbye!");
                break;
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("Read error: {}", e);
                break;
            }
        }

        let line = line.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if line == "(exit)" || line == ",exit" || line == ",quit" {
            println!("Goodbye!");
            break;
        }

        match interp.eval_program(line) {
            Ok(result) => {
                if result != TaggedValue::UNSPECIFIED {
                    println!("{}", format_tagged(result, &heap.borrow()));
                }
            }
            Err(e) => eprintln!("Error: {}", e),
        }
    }
}
