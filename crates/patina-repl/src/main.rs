use patina_interpreter::{TreeWalkInterpreter, format_interpreter_error};
use patina_repl::Repl;
use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    // Parse command-line options
    let mut filename: Option<String> = None;

    for arg in args.iter().skip(1) {
        if arg == "--help" || arg == "-h" {
            print_help();
            process::exit(0);
        } else if !arg.starts_with('-') {
            filename = Some(arg.clone());
        } else {
            eprintln!("Unknown option: {}", arg);
            print_help();
            process::exit(1);
        }
    }

    // Check if a file argument was provided
    if let Some(file) = filename {
        // Script mode: run the provided file
        run_script(&file);
    } else {
        // REPL mode: interactive shell
        run_repl();
    }
}

fn print_help() {
    eprintln!("Usage: patina [FILE]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --help    Show this help message");
    eprintln!();
    eprintln!("If FILE is provided, run it as a script.");
    eprintln!("Otherwise, start an interactive REPL.");
    eprintln!();
    eprintln!("Features:");
    eprintln!("  - Full R7RS continuation support (call/cc, dynamic-wind)");
    eprintln!("  - Exception handling (guard, raise)");
}

fn run_script(filename: &str) {
    // Read the file
    let code = match fs::read_to_string(filename) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("Error reading file '{}': {}", filename, e);
            process::exit(1);
        }
    };

    // Create interpreter and run the program
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Check if this is a test file by looking for common test patterns
    let is_test_file = filename.contains("test") || code.contains("test-begin");

    if is_test_file {
        interp.eval_program_resilient_with_source_name(&code, filename);
        process::exit(0);
    } else {
        let (result, source_map) = interp.eval_program_with_source_name(&code, filename);
        match result {
            Ok(_) => process::exit(0),
            Err(e) => {
                eprintln!("Error: {}", format_interpreter_error(&e, &source_map.borrow()));
                process::exit(1);
            }
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
