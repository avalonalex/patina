mod completer;
mod highlighter;
mod validator;

use self::highlighter::SchemeHighlighter;
use self::validator::SchemeValidator;
pub use self::validator::needs_more_input;
use patina_interpreter::{TreeWalkInterpreter, format_interpreter_error};
use rustyline::error::ReadlineError;
use rustyline::history::FileHistory;
use rustyline::validate::ValidationResult;
use rustyline::{CompletionType, Config, EditMode, Editor};
use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation};

pub struct Repl {
    lines: Lines,
    interpreter: TreeWalkInterpreter,
    expr_counter: u32,
}

use rustyline::Context;
use rustyline::completion::{Completer, Pair};
use rustyline::highlight::CmdKind;
use rustyline::hint::{Hinter, HistoryHinter};

/// Helper that combines all REPL features (syntax highlighting, paren matching, history hints).
///
/// This is `pub` so the VM REPL can share the same rustyline helper.
pub struct SchemeHelper {
    highlighter: SchemeHighlighter,
    validator: SchemeValidator,
    hinter: HistoryHinter,
}

impl Default for SchemeHelper {
    fn default() -> Self {
        Self::new()
    }
}

impl SchemeHelper {
    pub fn new() -> Self {
        SchemeHelper {
            highlighter: SchemeHighlighter::new(),
            validator: SchemeValidator::new(),
            hinter: HistoryHinter::new(),
        }
    }

    /// The input that was still unfinished at the last line the editor
    /// accepted. The editor drops a partly-typed form when its input ends, and
    /// this is what a session reports in its place.
    pub fn take_pending_input(&self) -> Option<String> {
        self.validator.take_pending()
    }
}

impl rustyline::Helper for SchemeHelper {}

impl Completer for SchemeHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        _line: &str,
        _pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        // TODO: Implement auto-completion for Scheme symbols
        Ok((0, vec![]))
    }
}

impl Hinter for SchemeHelper {
    type Hint = String;

    fn hint(&self, line: &str, pos: usize, ctx: &Context<'_>) -> Option<String> {
        self.validator.saw_edit_buffer(line);
        self.hinter.hint(line, pos, ctx)
    }
}

impl rustyline::highlight::Highlighter for SchemeHelper {
    fn highlight<'l>(&self, line: &'l str, pos: usize) -> std::borrow::Cow<'l, str> {
        self.highlighter.highlight(line, pos)
    }

    fn highlight_char(&self, line: &str, pos: usize, kind: CmdKind) -> bool {
        self.highlighter.highlight_char(line, pos, kind)
    }
}

impl rustyline::validate::Validator for SchemeHelper {
    fn validate(
        &self,
        ctx: &mut rustyline::validate::ValidationContext,
    ) -> rustyline::Result<rustyline::validate::ValidationResult> {
        self.validator.validate(ctx)
    }
}

/// Where a session's lines come from.
///
/// At a terminal, the line editor: history, highlighting, editing a form over
/// several lines. Anything else — a pipe, an editor's inferior-Scheme buffer,
/// a program driving Patina — is read directly instead, because rustyline's
/// own path for input that is not a terminal walks the whole form again for
/// every line added to it, which costs time proportional to the square of the
/// form's length (#348). chibi, Gauche and Chez stay flat where that was
/// quadratic, by reading a datum from the port rather than deciding whether a
/// buffer is finished; reading the lines here is the same idea, and the
/// session's own reader already reads each line once (#341).
pub enum Lines {
    /// A terminal, with the line editor.
    Editing(Box<Editor<SchemeHelper, FileHistory>>),
    Piped(Box<PipedLines>),
}

/// Lines read straight from input that is not a terminal.
///
/// It follows what rustyline does there, so a session reads the same text: the
/// line ending is dropped, a `\x08` erases the grapheme before it — a letter
/// with its combining marks, not one character of one — and the form is taken
/// once the validator says it is finished.
///
/// The erasing is done to the form as it is built, walking back one grapheme
/// from its end, rather than reading the whole form again for every line, which
/// is what cost time proportional to its square.
pub struct PipedLines {
    validator: SchemeValidator,
}

impl PipedLines {
    fn readline(&mut self) -> Result<String, ReadlineError> {
        use std::io::BufRead;

        let mut form = String::new();
        loop {
            let mut line = String::new();
            if std::io::stdin().lock().read_line(&mut line)? == 0 {
                return Err(ReadlineError::Eof);
            }
            let ended_with_newline = line.ends_with('\n');
            let mut ended_with_return = false;
            if ended_with_newline {
                line.pop();
                ended_with_return = line.ends_with('\r');
                if ended_with_return {
                    line.pop();
                }
            }
            for grapheme in UnicodeSegmentation::graphemes(line.as_str(), true) {
                if grapheme == "\u{8}" {
                    erase_last_grapheme(&mut form);
                } else {
                    form.push_str(grapheme);
                }
            }
            if !matches!(self.validator.judge(&form), ValidationResult::Incomplete) {
                return Ok(form);
            }
            // Unfinished: the line ending goes back, and the next line joins it.
            if ended_with_return {
                form.push('\r');
            }
            if ended_with_newline {
                form.push('\n');
            }
        }
    }
}

/// Drop the last grapheme of `form`, which is what a `\x08` erases.
fn erase_last_grapheme(form: &mut String) {
    let mut cursor = GraphemeCursor::new(form.len(), form.len(), true);
    if let Ok(Some(boundary)) = cursor.prev_boundary(form, 0) {
        form.truncate(boundary);
    }
}

impl Lines {
    fn readline(&mut self, prompt: &str) -> Result<String, ReadlineError> {
        match self {
            Lines::Editing(editor) => editor.readline(prompt),
            Lines::Piped(piped) => piped.readline(),
        }
    }

    /// Keep `line` for the history of a session a person is typing at. Input
    /// that is not a terminal leaves no history, as it leaves none in chibi,
    /// Gauche or Chez.
    fn remember(&mut self, line: &str) {
        if let Lines::Editing(editor) = self {
            let _ = editor.add_history_entry(line);
        }
    }

    /// What had been read of a form the input ended inside.
    fn take_pending_input(&mut self) -> Option<String> {
        match self {
            Lines::Editing(editor) => editor
                .helper()
                .and_then(|helper| helper.take_pending_input()),
            Lines::Piped(piped) => piped.validator.take_pending(),
        }
    }

    fn save_history(&mut self) {
        if let Lines::Editing(editor) = self
            && let Some(mut path) = dirs::home_dir()
        {
            path.push(".patina_history");
            let _ = editor.save_history(&path);
        }
    }
}

/// Where a session reads its lines from, with the Scheme helper (highlighting,
/// validation, hints) when that is a terminal. See [`Lines`].
///
/// Used by both the tree-walker REPL and the VM REPL.
pub fn session_lines() -> rustyline::Result<Lines> {
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        return Ok(Lines::Piped(Box::new(PipedLines {
            validator: SchemeValidator::new(),
        })));
    }

    let config = Config::builder()
        .history_ignore_space(true)
        .completion_type(CompletionType::List)
        .edit_mode(EditMode::Emacs)
        .max_history_size(1000)?
        .build();

    let mut editor = Editor::with_config(config)?;
    editor.set_helper(Some(SchemeHelper::new()));

    if let Some(mut path) = dirs::home_dir() {
        path.push(".patina_history");
        let _ = editor.load_history(&path);
    }

    Ok(Lines::Editing(Box::new(editor)))
}

/// Run a generic REPL loop using a shared rustyline editor.
///
/// `prompt` — the prompt string (e.g. `"patina> "` or `"patina/vm> "`).
/// `eval`   — called with each non-empty, non-comment line; returns:
///            - `None` to print nothing (e.g. for `#<unspecified>`)
///            - `Some(output)` to print a result or error
///
/// Returns whether the session ended cleanly: `true` after `(exit)`, `,exit`,
/// `,quit` or the end of input, and `false` when input ended part-way through
/// a form or the editor failed.
pub fn run_repl_loop<F>(lines: &mut Lines, prompt: &str, mut eval: F) -> bool
where
    F: FnMut(&str) -> Option<String>,
{
    use std::io::Write;

    let clean = loop {
        let _ = std::io::stdout().flush();

        match lines.readline(prompt) {
            Ok(line) => {
                let line = line.trim();
                if line.is_empty() || line.starts_with(';') {
                    continue;
                }
                if line == "(exit)" || line == ",exit" || line == ",quit" {
                    println!("Goodbye!");
                    break true;
                }

                lines.remember(line);

                if let Some(output) = eval(line) {
                    println!("{}", output);
                    // An error that interrupted an `exit` still ends the
                    // session (`patina_runtime::exit_status`).
                    patina_runtime::exit_status::exit_if_interrupted();
                } else {
                    eprint!(""); // flush workaround
                }
            }
            Err(ReadlineError::Interrupted) => {
                // Ctrl+C abandons whatever was being typed.
                lines.take_pending_input();
                println!("^C");
                continue;
            }
            Err(ReadlineError::Eof) => {
                // Input that is not a terminal ended part-way through a form: the
                // editor has thrown the form away, but the validator kept what of
                // it had been accepted. (At a terminal input ends only on an empty
                // line, and the validator forgets what was erased.) A session cut
                // off inside a form has not ended cleanly: run what arrived, which
                // reports where the unfinished form began, as a file would.
                let pending = lines.take_pending_input();
                match pending {
                    Some(pending) => {
                        if let Some(output) = eval(&pending) {
                            eprintln!("{}", output);
                            patina_runtime::exit_status::exit_if_interrupted();
                        }
                        break false;
                    }
                    None => {
                        println!("Goodbye!");
                        break true;
                    }
                }
            }
            Err(err) => {
                eprintln!("Error: {:?}", err);
                break false;
            }
        }
    };

    lines.save_history();
    clean
}

impl Repl {
    /// Create a new REPL with full continuation support.
    pub fn new() -> rustyline::Result<Self> {
        Ok(Repl {
            lines: session_lines()?,
            interpreter: TreeWalkInterpreter::new_tree_walker(),
            expr_counter: 0,
        })
    }

    /// The REPL's interpreter, for pre-run configuration (library search
    /// paths from the CLI's `-I`/`-A` flags).
    pub fn interpreter(&self) -> &TreeWalkInterpreter {
        &self.interpreter
    }

    /// Run the session, and say whether it ended cleanly: `false` means its
    /// input ended part-way through a form, which has been reported, or the
    /// editor failed.
    pub fn run(&mut self) -> bool {
        println!("Patina Scheme R7RS Interpreter");
        println!("Version {}", env!("CARGO_PKG_VERSION"));
        println!();
        println!("Features:");
        println!("  • Full R7RS continuation support (call/cc, dynamic-wind)");
        println!("  • Exception handling (guard, raise)");
        println!("  • Multi-line editing with auto-indentation");
        println!("  • Syntax highlighting");
        println!();
        println!("Commands:");
        println!("  (exit) or Ctrl+D to quit");
        println!("  Ctrl+C to cancel current input");
        println!();

        let interp = &self.interpreter;
        let counter = &mut self.expr_counter;

        run_repl_loop(&mut self.lines, "patina> ", |line| {
            *counter += 1;
            let source_name = format!("<repl-{}>", counter);
            // Every form on the line, as the VM REPL does: reading only the
            // first left `(define a 1) (define b 2)` with `b` unbound, and
            // dropped a trailing datum the line cut short.
            let (eval_result, source_map) =
                interp.eval_program_with_source_name(line, &source_name);
            match eval_result {
                Ok(result) => {
                    if result != patina_core::TaggedValue::UNSPECIFIED {
                        Some(interp.display_tagged(result))
                    } else {
                        None
                    }
                }
                Err(e) => Some(format!(
                    "Error: {}",
                    format_interpreter_error(&e, &source_map.borrow())
                )),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::erase_last_grapheme;

    /// A `\x08` erases the grapheme before it, which is what rustyline does
    /// with a pipe: a letter and the marks on it go together.
    #[test]
    fn erasing_takes_the_whole_grapheme() {
        let mut form = String::from("ab");
        erase_last_grapheme(&mut form);
        assert_eq!(form, "a");

        let mut form = String::from("xe\u{301}");
        erase_last_grapheme(&mut form);
        assert_eq!(form, "x", "the letter goes with its combining mark");

        let mut form = String::from("a\n");
        erase_last_grapheme(&mut form);
        assert_eq!(form, "a", "a line ending is a grapheme of its own");

        let mut form = String::new();
        erase_last_grapheme(&mut form);
        assert_eq!(form, "", "nothing to erase");
    }
}
