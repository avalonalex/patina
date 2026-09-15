//! Running a program that arrives a line at a time.
//!
//! A program on standard input is read a line at a time, and each form is
//! evaluated as soon as the line that ends it arrives: a producer that waits on
//! the program's output makes progress, and what the reader holds follows the
//! largest form rather than the whole stream (#333).
//!
//! Text read and not yet run stays in the input port's unread text, which the
//! program's own reads take from as well. A read inside the program therefore
//! continues right after the form being run, as it does in chibi and Gauche,
//! and reading the program resumes after whatever the read took.
//!
//! One source map serves the whole run, so an error located in a form read
//! earlier is placed, quoted and given its macro expansion chain as it is in a
//! file, while its line is among the last megabyte of text read. Older lines
//! are forgotten, so what the reader holds stays bounded.

use patina_core::{SharedHeap, TaggedValue};
use patina_frontend::{DatumScan, ReaderState, dialect};
use patina_interpreter::{
    ParseError, Parser, ProgramOutcome, SourceMap, format_parse_error_with_source,
    prune_freed_locations,
};
use patina_runtime::Port;
use std::cell::RefCell;
use std::rc::Rc;

/// How much of the program's text a run keeps for quoting in diagnostics.
const SOURCE_TEXT_KEPT: usize = 1 << 20;

/// Evaluate the program `input` delivers, each form as soon as the line that
/// ends it has arrived, and say how it went.
///
/// `eval_form` evaluates one datum against the run's source map, and renders
/// an error for printing. `keep_going` is the CLI's `-k`: without it the first
/// error ends the run, and with it an evaluation error is counted and the next
/// form runs. A read error, and input that ends inside a form, end the run
/// either way, since nothing after them can be read.
pub fn run_program_stream<E>(
    input: &Port,
    heap: &SharedHeap,
    source_name: &str,
    keep_going: bool,
    eval_form: E,
) -> ProgramOutcome
where
    E: FnMut(TaggedValue, &Rc<RefCell<SourceMap>>) -> Result<(), String>,
{
    let r6rs = dialect::allow_r6rs();
    let mut run = Run {
        input,
        heap,
        source_name: Rc::from(source_name),
        keep_going,
        eval_form,
        eval_errors: 0,
        source_map: Rc::new(RefCell::new(SourceMap::new())),
        r6rs,
        at: ReaderState::START,
        lines: 0,
        scan: DatumScan::new(r6rs),
        asked_at: 0,
    };
    loop {
        match input.pull_line() {
            Ok(Some(line)) => {
                if let Some(outcome) = run.line_arrived(&line) {
                    return outcome;
                }
            }
            Ok(None) => return run.finish(),
            Err(error) => return run.input_failed(&error),
        }
    }
}

/// What a run carries from one line to the next.
struct Run<'a, E> {
    input: &'a Port,
    heap: &'a SharedHeap,
    source_name: Rc<str>,
    keep_going: bool,
    eval_form: E,
    eval_errors: usize,
    source_map: Rc<RefCell<SourceMap>>,
    /// The dialect, resolved once for the run.
    r6rs: bool,
    /// Where the input's unread text begins in the program.
    at: ReaderState,
    /// Lines of the program the reader has seen.
    lines: u32,
    /// A scan of the unread text.
    scan: DatumScan,
    /// How long the unread text was when the parser last found nothing
    /// finished in it, or zero.
    asked_at: usize,
}

/// How much of a parser's text has been consumed from the input.
#[derive(Default)]
struct Consumed {
    bytes: usize,
    chars: usize,
}

impl<E> Run<'_, E>
where
    E: FnMut(TaggedValue, &Rc<RefCell<SourceMap>>) -> Result<(), String>,
{
    /// A line has arrived on the end of the unread text: run what it finished.
    fn line_arrived(&mut self, line: &str) -> Option<ProgramOutcome> {
        let mut line = line;
        if self.lines == 0
            && let Some(rest) = line.strip_prefix('\u{feff}')
        {
            // A byte order mark is dropped at the start of a program, as the
            // lexer drops one at the start of a file; anywhere else it is a
            // character.
            self.input.consume_unread('\u{feff}'.len_utf8());
            line = rest;
        }
        self.lines = self.lines.saturating_add(1);
        {
            let mut map = self.source_map.borrow_mut();
            map.push_source_line(self.lines, line);
            map.forget_old_source_lines(SOURCE_TEXT_KEPT, self.at.line);
        }
        self.scan.feed(line);
        if self.scan.nothing_finished() {
            // Asking the parser reads everything since the unfinished datum
            // began, so while the scan shows nothing can have finished it is
            // asked again only once that text has doubled. That keeps the
            // total time linear, and bounds how long a mistake inside a form
            // that is still open waits to be reported.
            let unread = self.input.unread_len();
            if self.asked_at == 0 {
                self.asked_at = unread;
                return None;
            }
            if unread < 2 * self.asked_at {
                return None;
            }
        }
        self.run_unread(false)
    }

    /// Read and run the forms the unread text holds. At the end of the input
    /// a form still unfinished is reported rather than waited for.
    fn run_unread(&mut self, at_end: bool) -> Option<ProgramOutcome> {
        'text: loop {
            let text = self.input.unread_text();
            let at = ReaderState {
                offset: 0,
                ..self.at
            };
            let mut parser = match Parser::resuming(&text, at, self.heap.clone(), self.r6rs) {
                Ok(parser) => {
                    parser.recording_into(self.source_name.clone(), self.source_map.clone())
                }
                Err(error) if error.is_incomplete() && !at_end => {
                    self.wait(&text);
                    return None;
                }
                Err(error) => return Some(self.read_failed(&error)),
            };
            let mut consumed = Consumed::default();
            loop {
                // Drop SourceMap entries for slots the previous form's
                // evaluation freed, before this iteration's parse can reuse
                // them (§9.1).
                prune_freed_locations(self.heap, &self.source_map);
                match parser.parse_next() {
                    Ok(Some(datum)) => {
                        self.consume_to(&text, &mut consumed, parser.consumed_state());
                        let version = self.input.unread_version();
                        if let Err(message) = (self.eval_form)(datum, &self.source_map) {
                            eprintln!("Error: {}", message);
                            patina_runtime::exit_status::exit_if_interrupted();
                            self.eval_errors += 1;
                            patina_runtime::exit_status::note_error_reported();
                            if !self.keep_going {
                                return Some(self.outcome(false));
                            }
                        }
                        if self.input.unread_version() != version {
                            self.resync(&text[consumed.bytes..]);
                            continue 'text;
                        }
                    }
                    Ok(None) => {
                        self.consume_to(&text, &mut consumed, parser.end_state());
                        self.wait("");
                        return None;
                    }
                    Err(error) if error.is_incomplete() && !at_end => {
                        self.consume_to(&text, &mut consumed, parser.unfinished_start());
                        self.wait(&text[consumed.bytes..]);
                        return None;
                    }
                    Err(error) => return Some(self.read_failed(&error)),
                }
            }
        }
    }

    /// Consume the unread text up to `to`, a point `text`'s parser reached,
    /// and carry on from there.
    fn consume_to(&mut self, text: &str, consumed: &mut Consumed, to: ReaderState) {
        debug_assert!(to.offset >= consumed.chars);
        let rest = &text[consumed.bytes..];
        let bytes = rest
            .char_indices()
            .nth(to.offset - consumed.chars)
            .map_or(rest.len(), |(index, _)| index);
        self.input.consume_unread(bytes);
        consumed.bytes += bytes;
        consumed.chars = to.offset;
        self.at = to;
    }

    /// Wait for more input, with `unread` still to run.
    fn wait(&mut self, unread: &str) {
        self.scan = DatumScan::new(self.r6rs);
        self.scan.feed(unread);
        self.asked_at = unread.len();
    }

    /// The program read from its input while a form ran, taking text the
    /// reader had read but not yet run: `before` is what was unread then.
    fn resync(&mut self, before: &str) {
        let after = self.input.unread_text();
        match before.strip_suffix(after.as_str()) {
            // It took the start of what was there: carry on after that.
            Some(taken) => self.at = advance(self.at, taken),
            // It read past what had arrived, and left the rest of a line the
            // reader never saw the start of. Carry on from that, counted as
            // the next line: the lines the program read itself are not
            // counted.
            None => {
                self.at = ReaderState {
                    offset: 0,
                    line: self.lines.saturating_add(1),
                    column: 1,
                    fold_case: self.at.fold_case,
                };
                let mut map = self.source_map.borrow_mut();
                for line in after.split_inclusive('\n') {
                    self.lines = self.lines.saturating_add(1);
                    map.push_source_line(self.lines, line);
                }
            }
        }
    }

    fn finish(&mut self) -> ProgramOutcome {
        self.run_unread(true).unwrap_or_else(|| self.outcome(true))
    }

    fn read_failed(&self, error: &ParseError) -> ProgramOutcome {
        eprintln!(
            "Error: {}",
            format_parse_error_with_source(error, &self.source_map.borrow())
        );
        patina_runtime::exit_status::note_error_reported();
        self.outcome(false)
    }

    fn input_failed(&self, error: &std::io::Error) -> ProgramOutcome {
        eprintln!(
            "Error reading {} after line {}: {}",
            self.source_name, self.lines, error
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

/// `at` moved past `text`.
fn advance(at: ReaderState, text: &str) -> ReaderState {
    text.chars().fold(at, |at, c| {
        if c == '\n' {
            ReaderState {
                line: at.line.saturating_add(1),
                column: 1,
                ..at
            }
        } else {
            ReaderState {
                column: at.column.saturating_add(1),
                ..at
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where each form evaluated was, or `None` for one the parser records no
    /// position for.
    type Seen = Vec<Option<(u32, u32)>>;

    /// Run `text`, recording each form's position; `act` is what evaluating
    /// the form at a position does, and may read the input as a program does.
    fn run_with(
        text: &str,
        keep_going: bool,
        mut act: impl FnMut(&Port, Option<(u32, u32)>) -> Result<(), String>,
    ) -> (ProgramOutcome, Seen) {
        let heap = patina_core::new_shared_heap();
        let input = Port::new_input_string(text.to_string());
        let mut seen = Vec::new();
        let outcome = run_program_stream(&input, &heap, "<test>", keep_going, |datum, map| {
            let at = map.borrow().get(datum).map(|loc| (loc.line, loc.column));
            seen.push(at);
            act(&input, at)
        });
        (outcome, seen)
    }

    fn run(text: &str) -> (ProgramOutcome, Seen) {
        run_with(text, false, |_, _| Ok(()))
    }

    #[test]
    fn each_form_runs_with_its_own_position_in_the_program() {
        let (outcome, seen) = run("(a)\n(b)\n\n  (c) (d)\n");
        assert!(outcome.clean());
        assert_eq!(
            seen,
            [Some((1, 1)), Some((2, 1)), Some((4, 3)), Some((4, 7))]
        );
    }

    #[test]
    fn a_form_spanning_lines_runs_once_its_last_line_arrives() {
        assert_eq!(run("(a\n b)\n(c)\n").1, [Some((1, 1)), Some((3, 1))]);
        assert_eq!(run("(a \"x\ny\")\n(b)\n").1, [Some((1, 1)), Some((3, 1))]);
        assert_eq!(run("#| one\ntwo |#\n(a)\n").1, [Some((3, 1))]);
        assert_eq!(run("'\n\n(a)\n").1, [Some((1, 1))]);
    }

    /// A finished form runs before the next line is read, even when the
    /// line it ends on also starts a form that is still open.
    #[test]
    fn a_form_runs_before_the_line_after_it_is_read() {
        let mut unread = Vec::new();
        let (outcome, seen) = run_with("(a) (b\n c)\n", false, |input, _| {
            unread.push(input.unread_text());
            Ok(())
        });
        assert!(outcome.clean());
        assert_eq!(seen, [Some((1, 1)), Some((1, 5))]);
        assert_eq!(unread, [" (b\n", "\n"]);
    }

    #[test]
    fn input_ending_inside_a_form_runs_what_came_before_and_fails() {
        let (outcome, seen) = run_with("(a)\n(b\n", true, |_, _| Ok(()));
        assert_eq!(seen, [Some((1, 1))]);
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
        let (outcome, seen) = run_with("(a)\n(b)\n(c)\n", false, |_, at| match at {
            Some((2, _)) => Err("failed".to_string()),
            _ => Ok(()),
        });
        assert_eq!(seen, [Some((1, 1)), Some((2, 1))]);
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
        let (outcome, seen) = run_with("(a)\n(b)\n(c)\n", true, |_, at| match at {
            Some((2, _)) => Err("failed".to_string()),
            _ => Ok(()),
        });
        assert_eq!(seen, [Some((1, 1)), Some((2, 1)), Some((3, 1))]);
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
        let (outcome, seen) = run_with("(a))\n(b)\n", true, |_, _| Ok(()));
        assert_eq!(seen, [Some((1, 1))]);
        assert!(!outcome.read_to_end);
    }

    #[test]
    fn an_input_error_ends_the_run() {
        let heap = patina_core::new_shared_heap();
        let input = Port::new_input_string("(a)\n".to_string());
        input.close();
        let outcome = run_program_stream(&input, &heap, "<test>", true, |_, _| Ok(()));
        assert!(!outcome.read_to_end);
    }

    /// The program's reads take the text right after the form being run,
    /// and reading the program carries on after what they took.
    #[test]
    fn a_read_inside_the_program_continues_after_its_form() {
        // It takes part of the line: the next form is found after it.
        let (_, seen) = run_with("(r)  (x)\n", false, |input, at| {
            if at == Some((1, 1)) {
                assert_eq!(input.read_char().unwrap(), Some(' '));
            }
            Ok(())
        });
        assert_eq!(seen, [Some((1, 1)), Some((1, 6))]);

        // It reads past what had arrived and leaves the rest of a line: that
        // is read next, as the next line.
        let (_, seen) = run_with("(r)\n(z)\n", false, |input, at| {
            if at == Some((1, 1)) {
                input.take_pushback();
                input.set_pushback(" (after)\n".to_string());
            }
            Ok(())
        });
        assert_eq!(seen, [Some((1, 1)), Some((2, 2)), Some((3, 1))]);
    }

    /// A form read earlier is still quoted from the run's source map.
    #[test]
    fn the_text_of_earlier_lines_is_kept_for_diagnostics() {
        let heap = patina_core::new_shared_heap();
        let input = Port::new_input_string("(define (f)\n  1)\n(f)\n".to_string());
        let mut quoted = None;
        run_program_stream(&input, &heap, "<test>", false, |_, map| {
            quoted = map.borrow().get_line(1).map(str::to_string);
            Ok(())
        });
        assert_eq!(quoted.as_deref(), Some("(define (f)"));
    }

    #[test]
    fn a_byte_order_mark_is_dropped_only_at_the_start_of_the_program() {
        let (outcome, seen) = run("\u{feff}(a)\n\u{feff} (b)\n");
        assert!(outcome.clean());
        assert_eq!(seen, [Some((1, 1)), None, Some((2, 3))]);
    }

    #[test]
    fn fold_case_holds_for_the_rest_of_the_program() {
        let heap = patina_core::new_shared_heap();
        let input = Port::new_input_string("#!fold-case\n(A)\n(B\n C)\n".to_string());
        let mut written = Vec::new();
        let outcome = run_program_stream(&input, &heap, "<test>", false, |datum, _| {
            written.push(patina_core::debug_format::format_tagged(
                datum,
                &heap.borrow(),
            ));
            Ok(())
        });
        assert!(outcome.clean());
        assert_eq!(written, ["(a)", "(b c)"]);
    }
}
