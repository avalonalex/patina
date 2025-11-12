use patina_interpreter::Interpreter;
use patina_repl::Repl;
use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    // Check if a file argument was provided
    if args.len() > 1 {
        // Script mode: run the provided file
        let filename = &args[1];
        run_script(filename);
    } else {
        // REPL mode: interactive shell
        run_repl();
    }
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
    let interp = Interpreter::new();
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
