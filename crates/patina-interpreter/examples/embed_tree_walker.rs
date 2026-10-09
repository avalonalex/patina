#![allow(
    clippy::disallowed_methods,
    reason = "an embedding example evaluates from outside, as a host does; the workspace's \
              re-entry rule (#622) is for the evaluator's own crates"
)]

use patina_interpreter::{TreeWalkInterpreter, format_interpreter_error};

fn main() {
    let interpreter = TreeWalkInterpreter::new_tree_walker();
    interpreter.set_command_line("embedded.scm", ["hello".to_owned()]);
    let (result, sources) = interpreter.eval_program_with_source_name_owned(
        "(import (scheme base)) (define answer 42) (list 'answer answer)",
        "embedded.scm",
    );
    match result {
        Ok(value) => {
            let output = interpreter.display_tagged(&value);
            assert_eq!(output, "(answer 42)");
            println!("{output}");
        }
        Err(error) => panic!("{}", format_interpreter_error(&error, &sources.borrow())),
    }
}
