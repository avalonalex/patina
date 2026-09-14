use patina_frontend::Parser;
use rustyline::validate::{ValidationContext, ValidationResult, Validator};

pub struct SchemeValidator;

impl SchemeValidator {
    pub fn new() -> Self {
        SchemeValidator
    }
}

/// Whether the line so far stops part-way through a datum, so the editor
/// should take another one.
///
/// The reader answers this, rather than a paren count: counting cannot know
/// that the parenthesis in `#\(` is a character and not an opener, that the
/// one in `#|(|#` is inside a comment, or that `'` and `#;` are waiting for a
/// datum of their own. A counter got each of those wrong in both directions —
/// holding a complete line open forever, and evaluating an unfinished one.
///
/// Anything else is handed on as complete, including input that is plainly
/// wrong: a stray `)` is a mistake to report now, not a reason to sit and
/// wait for input that cannot fix it.
fn needs_more_input(input: &str) -> bool {
    match Parser::new(input) {
        // A constructor failure is the first token failing to lex, which for
        // an unterminated string or block comment means the same thing.
        Err(e) => e.is_incomplete(),
        Ok(mut parser) => loop {
            match parser.parse_next() {
                Ok(Some(_)) => continue,
                Ok(None) => return false,
                Err(e) => return e.is_incomplete(),
            }
        },
    }
}

impl Validator for SchemeValidator {
    fn validate(&self, ctx: &mut ValidationContext) -> rustyline::Result<ValidationResult> {
        if needs_more_input(ctx.input()) {
            Ok(ValidationResult::Incomplete)
        } else {
            Ok(ValidationResult::Valid(None))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::needs_more_input;

    #[test]
    fn takes_another_line_only_while_a_datum_is_unfinished() {
        for input in [
            "(+ 1",
            "(let ((x 1))",
            "'",
            "`(a ,",
            "#(1 2",
            "#u8(1",
            "#;",
            "(display \"unterminated",
            "(+ 1 #| unterminated",
            "(a |unterminated",
            "(1 . 2",
        ] {
            assert!(needs_more_input(input), "{input:?}");
        }
    }

    #[test]
    fn a_complete_line_is_evaluated() {
        for input in [
            "",
            "   ",
            "42",
            "(+ 1 2)",
            "(let ((x 1)) x)",
            "; just a comment",
            "#| block |#",
            "#;(dropped) 1",
            "(display \"a (paren) in a string\")",
            "(+ 1 #| a ( comment |# 2)",
            "(write #\\()",
            "(write #\\))",
            "(a |bar ( identifier|)",
            "(1 . 2)",
        ] {
            assert!(!needs_more_input(input), "{input:?}");
        }
    }

    /// Input that cannot be fixed by typing more is reported, not waited on.
    #[test]
    fn a_line_that_is_simply_wrong_is_not_held_open() {
        for input in [
            "(display 1))",
            ")",
            "(1 . 2 3)",
            "#u8(300)",
            // Square brackets are R6RS, off by default: holding the line open
            // would wait for a `]` the reader would refuse anyway.
            "[vector 1",
        ] {
            assert!(!needs_more_input(input), "{input:?}");
        }
    }
}
