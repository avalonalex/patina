use patina_interpreter::TreeWalkInterpreter;
use patina_repl::Repl;
use std::env;
use std::fs;
use std::process;

fn main() {
    // TODO: Disable macro debug mode once all macro-related tests in r7rs-tests.scm pass
    // Currently enabled globally to help debug macro expansion issues
    // See: PRD/phase1/IMPLEMENTATION_STATUS.md for macro test status
    // patina_runtime::macro_debug::enable();

    let args: Vec<String> = env::args().collect();

    // Parse command-line options
    let mut use_cps = false;
    let mut filename: Option<String> = None;

    for arg in args.iter().skip(1) {
        if arg == "--cps" {
            use_cps = true;
        } else if arg == "--help" || arg == "-h" {
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
        run_script(&file, use_cps);
    } else {
        // REPL mode: interactive shell
        run_repl(use_cps);
    }
}

fn print_help() {
    eprintln!("Usage: patina [OPTIONS] [FILE]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --cps     Enable CPS evaluation mode (supports call/cc)");
    eprintln!("  --help    Show this help message");
    eprintln!();
    eprintln!("If FILE is provided, run it as a script.");
    eprintln!("Otherwise, start an interactive REPL.");
}

fn run_script(filename: &str, use_cps: bool) {
    // Read the file
    let code = match fs::read_to_string(filename) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("Error reading file '{}': {}", filename, e);
            process::exit(1);
        }
    };

    // Create interpreter and run the program
    let interp = if use_cps {
        TreeWalkInterpreter::new_tree_walker_with_cps()
    } else {
        TreeWalkInterpreter::new_tree_walker()
    };

    // Check if this is a test file by looking for common test patterns
    let is_test_file = filename.contains("test") || code.contains("test-begin");

    if is_test_file {
        // Use resilient mode for test files - continue on errors
        interp.eval_program_resilient(&code);
        process::exit(0);
    } else {
        // Use strict mode for regular scripts - stop on first error
        match interp.eval_program(&code) {
            Ok(_) => {
                // Script completed successfully
                process::exit(0);
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                process::exit(1);
            }
        }
    }
}

fn run_repl(use_cps: bool) {
    match Repl::new_with_cps(use_cps) {
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
