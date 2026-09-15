//! Running a program that arrives a line at a time.
//!
//! A program on standard input is read line by line, and each run of lines is
//! evaluated as soon as it holds only complete forms. A producer that waits on
//! the program's output therefore makes progress, and memory follows the
//! largest form rather than the whole stream (#333).
//!
//! Positions stay the program's: each piece is parsed from the line it starts
//! on. Each piece also gets its own source map, dropped once the piece has run,
//! so an error located in a form read earlier is reported by position without
//! the text of its line. The reader's `#!fold-case` state is the one thing a
//! piece passes to the next, as it passes along a file.

use crate::repl::needs_more_input;
use patina_core::{SharedHeap, TaggedValue};
use patina_frontend::DatumScan;
use patina_interpreter::{
    ParseError, Parser, ProgramOutcome, SourceMap, format_parse_error_with_source,
    prune_freed_locations,
};
use std::cell::RefCell;
use std::rc::Rc;

/// Evaluate the program `next_line` delivers, one run of complete lines at a
/// time, and say how it went.
///
/// `eval_form` evaluates one datum against the source map of the piece it was
/// read from, and renders an error for printing. `keep_going` is the CLI's
/// `-k`: without it the first error ends the run, and with it an evaluation
/// error is counted and the next form runs. A read error, and input that ends
/// inside a form, end the run either way, since nothing after them can be read.
pub fn run_program_stream<L, E>(
    mut next_line: L,
    heap: &SharedHeap,
    source_name: &str,
    keep_going: bool,
    eval_form: E,
) -> ProgramOutcome
where
    L: FnMut() -> std::io::Result<Option<String>>,
    E: FnMut(TaggedValue, &Rc<RefCell<SourceMap>>) -> Result<(), String>,
{
    let mut run = Run {
        heap,
        source_name: Rc::from(source_name),
        keep_going,
        eval_form,
        eval_errors: 0,
        fold_case: false,
    };
    let mut piece = String::new();
    let mut piece_first_line = 1;
    let mut lines_read: u32 = 0;
    let mut scan = DatumScan::new();
    let mut asked_at = 0;
    loop {
        let line = match next_line() {
            Ok(Some(line)) => line,
            Ok(None) => {
                // A piece is left at the end only when its last form never
                // finished. Evaluating it runs the forms before the cut, then
                // reports where the unfinished one began, as a file would.
                let ended = if piece.trim().is_empty() {
                    None
                } else {
                    run.piece(&piece, piece_first_line)
                };
                return ended.unwrap_or_else(|| run.outcome(true));
            }
            Err(e) => {
                eprintln!("Error reading stdin: {}", e);
                patina_runtime::exit_status::note_error_reported();
                return run.outcome(false);
            }
        };
        if piece.is_empty() {
            piece_first_line = lines_read + 1;
        }
        lines_read += 1;
        piece.push_str(&line);
        // The scan rules a finish out in time proportional to the line; only the
        // reader rules one in, and asking it reads the whole piece. So the reader
        // is asked when the scan cannot rule a finish out, and again whenever the
        // piece has doubled since it was last asked. That keeps the total time
        // linear in the piece, and bounds how long a mistake inside a form that
        // is still open waits to be reported.
        scan.feed(&line);
        if scan.certainly_unfinished() && piece.len() < 2 * asked_at {
            continue;
        }
        asked_at = piece.len();
        if needs_more_input(&piece) {
            continue;
        }
        let complete = std::mem::take(&mut piece);
        scan = DatumScan::new();
        asked_at = 0;
        if let Some(outcome) = run.piece(&complete, piece_first_line) {
            return outcome;
        }
    }
}

/// What a run carries from one piece to the next.
struct Run<'a, E> {
    heap: &'a SharedHeap,
    source_name: Rc<str>,
    keep_going: bool,
    eval_form: E,
    eval_errors: usize,
    /// Whether `#!fold-case` was in effect where the last piece ended.
    fold_case: bool,
}

impl<E> Run<'_, E>
where
    E: FnMut(TaggedValue, &Rc<RefCell<SourceMap>>) -> Result<(), String>,
{
    /// Evaluate one piece, returning the run's outcome if the piece ended it.
    fn piece(&mut self, text: &str, first_line: u32) -> Option<ProgramOutcome> {
        let source_map = Rc::new(RefCell::new(SourceMap::new()));
        let parser = Parser::new_with_source_map_at_line(
            text,
            self.heap.clone(),
            self.source_name.clone(),
            source_map.clone(),
            first_line,
            self.fold_case,
        );
        let mut parser = match parser {
            Ok(parser) => parser,
            Err(e) => return Some(self.read_failed(&e, &source_map)),
        };
        loop {
            // Drop SourceMap entries for slots the previous form's evaluation
            // freed, before this iteration's parse can reuse them (§9.1).
            prune_freed_locations(self.heap, &source_map);
            match parser.parse_next() {
                Ok(Some(datum)) => {
                    if let Err(message) = (self.eval_form)(datum, &source_map) {
                        eprintln!("Error: {}", message);
                        self.eval_errors += 1;
                        patina_runtime::exit_status::note_error_reported();
                        if !self.keep_going {
                            return Some(self.outcome(false));
                        }
                    }
                }
                Ok(None) => {
                    self.fold_case = parser.folds_case();
                    return None;
                }
                Err(e) => return Some(self.read_failed(&e, &source_map)),
            }
        }
    }

    fn read_failed(&self, error: &ParseError, source_map: &RefCell<SourceMap>) -> ProgramOutcome {
        eprintln!(
            "Error: {}",
            format_parse_error_with_source(error, &source_map.borrow())
        );
        patina_runtime::exit_status::note_error_reported();
        self.outcome(false)
    }

    fn outcome(&self, read_to_end: bool) -> ProgramOutcome {
        ProgramOutcome {
            eval_errors: self.eval_errors,
            read_to_end,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// Lines as a stream delivers them.
    fn lines(text: &str) -> impl FnMut() -> std::io::Result<Option<String>> {
        let mut rest: VecDeque<String> = text.split_inclusive('\n').map(str::to_string).collect();
        move || Ok(rest.pop_front())
    }

    /// Run `text`, recording the line of each form evaluated; a form on a line
    /// in `fail_on` fails.
    fn run(text: &str, keep_going: bool, fail_on: &[u32]) -> (ProgramOutcome, Vec<u32>) {
        let heap = patina_core::new_shared_heap();
        let mut seen = Vec::new();
        let outcome = run_program_stream(lines(text), &heap, "<test>", keep_going, |datum, map| {
            let line = map.borrow().get(datum).map_or(0, |loc| loc.line);
            seen.push(line);
            if fail_on.contains(&line) {
                Err(format!("failed at line {line}"))
            } else {
                Ok(())
            }
        });
        (outcome, seen)
    }

    #[test]
    fn each_form_runs_with_the_program_s_own_line_number() {
        let (outcome, seen) = run("(a)\n(b)\n\n(c) (d)\n", false, &[]);
        assert!(outcome.clean());
        assert_eq!(seen, [1, 2, 4, 4]);
    }

    #[test]
    fn a_form_spanning_lines_runs_once_complete_and_keeps_its_first_line() {
        assert_eq!(run("(a\n b)\n(c)\n", false, &[]).1, [1, 3]);
        assert_eq!(run("(a \"x\ny\")\n(b)\n", false, &[]).1, [1, 3]);
        assert_eq!(run("#| one\ntwo |#\n(a)\n", false, &[]).1, [3]);
    }

    #[test]
    fn input_ending_inside_a_form_runs_what_came_before_and_fails() {
        let (outcome, seen) = run("(a)\n(b\n", true, &[]);
        assert_eq!(seen, [1]);
        assert_eq!(
            outcome,
            ProgramOutcome {
                eval_errors: 0,
                read_to_end: false
            }
        );
    }

    #[test]
    fn without_keep_going_the_first_error_ends_the_run() {
        let (outcome, seen) = run("(a)\n(b)\n(c)\n", false, &[2]);
        assert_eq!(seen, [1, 2]);
        assert_eq!(
            outcome,
            ProgramOutcome {
                eval_errors: 1,
                read_to_end: false
            }
        );
    }

    #[test]
    fn keep_going_counts_an_error_and_runs_the_next_form() {
        let (outcome, seen) = run("(a)\n(b)\n(c)\n", true, &[2]);
        assert_eq!(seen, [1, 2, 3]);
        assert_eq!(
            outcome,
            ProgramOutcome {
                eval_errors: 1,
                read_to_end: true
            }
        );
        assert!(!outcome.clean());
    }

    #[test]
    fn a_read_error_ends_the_run_even_under_keep_going() {
        let (outcome, seen) = run("(a))\n(b)\n", true, &[]);
        assert_eq!(seen, [1]);
        assert!(!outcome.read_to_end);
    }

    #[test]
    fn an_input_error_ends_the_run() {
        let heap = patina_core::new_shared_heap();
        let failing = || Err(std::io::Error::other("broken pipe"));
        let outcome = run_program_stream(failing, &heap, "<test>", true, |_, _| Ok(()));
        assert!(!outcome.read_to_end);
    }
}
