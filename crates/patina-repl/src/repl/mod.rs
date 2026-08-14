mod completer;
mod highlighter;
mod validator;

use self::highlighter::SchemeHighlighter;
use self::validator::SchemeValidator;
use patina_interpreter::{TreeWalkInterpreter, format_interpreter_error};
use rustyline::error::ReadlineError;
use rustyline::history::FileHistory;
use rustyline::{CompletionType, Config, EditMode, Editor};

pub struct Repl {
    editor: Editor<SchemeHelper, FileHistory>,
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

/// Build a shared rustyline editor with the Scheme helper (highlighting, validation, hints).
///
/// Used by both the tree-walker REPL and the VM REPL.
pub fn make_editor() -> rustyline::Result<Editor<SchemeHelper, FileHistory>> {
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

    Ok(editor)
}

/// Run a generic REPL loop using a shared rustyline editor.
///
/// `prompt` — the prompt string (e.g. `"patina> "` or `"patina/vm> "`).
/// `eval`   — called with each non-empty, non-comment line; returns:
///            - `None` to print nothing (e.g. for `#<unspecified>`)
///            - `Some(output)` to print a result or error
///            - returning `Err` (via the closure returning a sentinel) to stop is handled
///              by the closure itself; use the `bool` return to signal quit.
///
/// Returns when the user types `(exit)`, `,exit`, `,quit`, or Ctrl+D.
pub fn run_repl_loop<F>(editor: &mut Editor<SchemeHelper, FileHistory>, prompt: &str, mut eval: F)
where
    F: FnMut(&str) -> Option<String>,
{
    use std::io::Write;

    loop {
        let _ = std::io::stdout().flush();

        match editor.readline(prompt) {
            Ok(line) => {
                let line = line.trim();
                if line.is_empty() || line.starts_with(';') {
                    continue;
                }
                if line == "(exit)" || line == ",exit" || line == ",quit" {
                    println!("Goodbye!");
                    break;
                }

                let _ = editor.add_history_entry(line);

                if let Some(output) = eval(line) {
                    println!("{}", output);
                } else {
                    eprint!(""); // flush workaround
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("^C");
                continue;
            }
            Err(ReadlineError::Eof) => {
                println!("Goodbye!");
                break;
            }
            Err(err) => {
                eprintln!("Error: {:?}", err);
                break;
            }
        }
    }

    if let Some(mut path) = dirs::home_dir() {
        path.push(".patina_history");
        let _ = editor.save_history(&path);
    }
}

impl Repl {
    /// Create a new REPL with full continuation support.
    pub fn new() -> rustyline::Result<Self> {
        Ok(Repl {
            editor: make_editor()?,
            interpreter: TreeWalkInterpreter::new_tree_walker(),
            expr_counter: 0,
        })
    }

    /// The REPL's interpreter, for pre-run configuration (library search
    /// paths from the CLI's `-I`/`-A` flags).
    pub fn interpreter(&self) -> &TreeWalkInterpreter {
        &self.interpreter
    }

    pub fn run(&mut self) -> rustyline::Result<()> {
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

        run_repl_loop(&mut self.editor, "patina> ", |line| {
            *counter += 1;
            let source_name = format!("<repl-{}>", counter);
            let (eval_result, source_map) = interp.eval_str_with_source_name(line, &source_name);
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
        });

        Ok(())
    }
}
