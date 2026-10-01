use patina_interpreter::{VmInterpreter, format_interpreter_error};

fn main() {
    let interpreter = VmInterpreter::new_vm();
    interpreter.set_command_line("embedded.scm", ["hello".to_owned()]);
    let (result, sources) = interpreter.eval_program_with_source_name(
        "(import (scheme base)) (define answer 42) (list 'answer answer)",
        "embedded.scm",
    );
    match result {
        Ok(value) => {
            let output = interpreter.display_tagged(value);
            assert_eq!(output, "(answer 42)");
            println!("{output}");
        }
        Err(error) => panic!("{}", format_interpreter_error(&error, &sources.borrow())),
    }
}
