mod completer;
mod highlighter;
mod validator;

use self::highlighter::SchemeHighlighter;
use self::validator::SchemeValidator;
use patina_frontend::parser::Parser;
use patina_tree_walker::eval::Evaluator;
use rustyline::error::ReadlineError;
use rustyline::history::FileHistory;
use rustyline::{CompletionType, Config, EditMode, Editor};

pub struct Repl {
    editor: Editor<SchemeHelper, FileHistory>,
    evaluator: Evaluator,
}

use rustyline::Context;
use rustyline::completion::{Completer, Pair};
use rustyline::highlight::CmdKind;
use rustyline::hint::{Hinter, HistoryHinter};

/// Helper that combines all REPL features
struct SchemeHelper {
    highlighter: SchemeHighlighter,
    validator: SchemeValidator,
    hinter: HistoryHinter,
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

impl Repl {
    pub fn new() -> rustyline::Result<Self> {
        let config = Config::builder()
            .history_ignore_space(true)
            .completion_type(CompletionType::List)
            .edit_mode(EditMode::Emacs)
            .max_history_size(1000)?
            .build();

        let helper = SchemeHelper {
            highlighter: SchemeHighlighter::new(),
            validator: SchemeValidator::new(),
            hinter: HistoryHinter::new(),
        };

        let mut editor = Editor::with_config(config)?;
        editor.set_helper(Some(helper));

        // Load history from file
        let history_path = dirs::home_dir().map(|mut path| {
            path.push(".patina_history");
            path
        });

        if let Some(ref path) = history_path {
            let _ = editor.load_history(path);
        }

        Ok(Repl {
            editor,
            evaluator: Evaluator::new(),
        })
    }

    pub fn run(&mut self) -> rustyline::Result<()> {
        println!("Patina Scheme R7RS Interpreter");
        println!("Version 0.1.0");
        println!();
        println!("Features:");
        println!("  • Multi-line editing with auto-indentation");
        println!("  • Syntax highlighting");
        println!("  • Persistent history");
        println!("  • Parenthesis balancing");
        println!();
        println!("Commands:");
        println!("  (exit) or Ctrl+D to quit");
        println!("  Ctrl+C to cancel current input");
        println!();

        loop {
            // Flush stdout before readline to ensure display output is visible
            use std::io::Write;
            let _ = std::io::stdout().flush();

            let readline = self.editor.readline("patina> ");

            match readline {
                Ok(line) => {
                    let line = line.trim();

                    // Skip empty lines and comment-only lines
                    if line.is_empty() || line.starts_with(';') {
                        continue;
                    }

                    if line == "(exit)" || line == ",exit" || line == ",quit" {
                        println!("Goodbye!");
                        break;
                    }

                    // Add to history
                    let _ = self.editor.add_history_entry(line);

                    // Parse and evaluate
                    match Parser::new(line) {
                        Ok(mut parser) => match parser.parse() {
                            Ok(expr) => match self.evaluator.eval(&expr) {
                                Ok(result) => {
                                    // Don't print #<unspecified> values (from define, set!, etc.)
                                    if !matches!(result, patina_runtime::Value::Unspecified) {
                                        println!("{}", result);
                                    } else {
                                        // Force a flush by writing empty string to stderr
                                        // This works around rustyline stdout buffering issue
                                        eprint!("");
                                    }
                                }
                                Err(e) => eprintln!("Error: {}", e),
                            },
                            Err(e) => eprintln!("Parse error: {}", e),
                        },
                        Err(e) => eprintln!("Parser initialization error: {}", e),
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

        // Save history
        if let Some(mut path) = dirs::home_dir() {
            path.push(".patina_history");
            let _ = self.editor.save_history(&path);
        }

        Ok(())
    }
}
