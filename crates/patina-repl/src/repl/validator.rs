use patina_frontend::Parser;
use rustyline::validate::{ValidationContext, ValidationResult, Validator};
use std::cell::RefCell;

#[derive(Default)]
pub struct SchemeValidator {
    /// The input last judged unfinished, kept until it is finished or taken.
    /// The editor throws a partly-typed form away when its input ends, so
    /// this is the only record a session has of one to report.
    pending: RefCell<Option<String>>,
}

impl SchemeValidator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Take the input that was still unfinished at the last line the editor
    /// accepted, if any.
    pub fn take_pending(&self) -> Option<String> {
        self.pending.borrow_mut().take()
    }

    /// Note the editor's buffer while a terminal line is being edited.
    ///
    /// At a terminal the editor asks for a hint on every change to the line,
    /// and ends input only on an empty one, so by the end of input whatever
    /// was recorded has been erased: an empty buffer forgets it. Input that is
    /// not a terminal is never edited, and keeps its record to the end.
    pub fn saw_edit_buffer(&self, buffer: &str) {
        if buffer.is_empty() {
            *self.pending.borrow_mut() = None;
        }
    }

    /// Decide whether `input` is finished, remembering it when it is not.
    fn judge(&self, input: &str) -> ValidationResult {
        if needs_more_input(input) {
            *self.pending.borrow_mut() = Some(input.to_string());
            ValidationResult::Incomplete
        } else {
            *self.pending.borrow_mut() = None;
            ValidationResult::Valid(None)
        }
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
pub fn needs_more_input(input: &str) -> bool {
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
        Ok(self.judge(ctx.input()))
    }
}

#[cfg(test)]
mod tests {
    use super::{SchemeValidator, needs_more_input};
    use rustyline::validate::ValidationResult;

    /// The editor drops a partly-typed form when its input ends, so the
    /// validator keeps the last unfinished input it judged until that input is
    /// finished or taken.
    #[test]
    fn the_last_unfinished_input_is_kept_until_it_is_finished_or_taken() {
        let validator = SchemeValidator::new();
        assert!(matches!(
            validator.judge("(define y (+ 1"),
            ValidationResult::Incomplete
        ));
        assert_eq!(validator.take_pending().as_deref(), Some("(define y (+ 1"));
        assert_eq!(validator.take_pending(), None, "taking it clears it");

        assert!(matches!(
            validator.judge("(+ 1"),
            ValidationResult::Incomplete
        ));
        assert!(matches!(
            validator.judge("(+ 1 2)"),
            ValidationResult::Valid(None)
        ));
        assert_eq!(
            validator.take_pending(),
            None,
            "a finished form leaves nothing"
        );
    }

    /// At a terminal, erasing a form that was being typed forgets it: input
    /// can end there only on an empty line, and must not report the form.
    #[test]
    fn an_erased_form_is_forgotten() {
        let validator = SchemeValidator::new();
        assert!(matches!(
            validator.judge("(define y"),
            ValidationResult::Incomplete
        ));
        validator.saw_edit_buffer("(define y\n  (+ 1");
        validator.saw_edit_buffer("(de");
        assert!(validator.pending.borrow().is_some(), "still being typed");
        validator.saw_edit_buffer("");
        assert_eq!(validator.take_pending(), None);
    }

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
