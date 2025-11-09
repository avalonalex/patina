// Module declarations
mod application;
mod debug;
mod error;
mod primitives;
mod special_forms;

// Re-export error type for public API
pub use error::EvalError;

use crate::env::Environment;
use crate::value::Value;
use debug::DebugConfig;
use std::rc::Rc;

pub struct Evaluator {
    global_env: Rc<Environment>,
    pub(crate) debug: Rc<DebugConfig>,
}

impl Evaluator {
    pub fn new() -> Self {
        let global_env = Rc::new(Environment::new());
        Self::install_primitives(&global_env);
        let evaluator = Evaluator {
            global_env,
            debug: Rc::new(DebugConfig::new()),
        };

        // Load bootstrap library
        evaluator.load_bootstrap();

        evaluator
    }

    fn load_bootstrap(&self) {
        // Embed bootstrap.scm at compile time
        const BOOTSTRAP: &str = include_str!("../../lib/bootstrap.scm");

        // Parse and evaluate all expressions in bootstrap
        // Silently ignore any errors (shouldn't happen in bootstrap)
        let mut parser = match crate::parser::Parser::new(BOOTSTRAP) {
            Ok(p) => p,
            Err(_) => return, // Bootstrap failed to parse
        };

        // Parse and eval all expressions
        loop {
            match parser.parse() {
                Ok(expr) => {
                    // Evaluate, ignore result and errors
                    let _ = self.eval(&expr);
                }
                Err(crate::parser::ParseError::UnexpectedEof) => break,
                Err(_) => break, // Stop on other errors
            }
        }
    }

    pub fn eval(&self, expr: &Value) -> Result<Value, EvalError> {
        self.eval_in_env(expr, &self.global_env)
    }

    fn eval_in_env(&self, expr: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
        // Debug trace entry
        if self.debug.is_enabled(debug::DebugStage::Eval) {
            eprintln!("[EVAL]{} Evaluating: {}", self.debug.current_indent(), expr);
            self.debug.indent();
        }

        let result = match expr {
            // Self-evaluating
            Value::Boolean(_)
            | Value::Integer(_)
            | Value::BigInteger(_)
            | Value::Rational(_)
            | Value::Real(_)
            | Value::Complex(_, _)
            | Value::Character(_)
            | Value::String(_)
            | Value::Vector(_)
            | Value::Bytevector(_) => Ok(expr.clone()),

            // Variable lookup
            Value::Symbol(name) => {
                if self.debug.is_enabled(debug::DebugStage::Env) {
                    eprintln!("[ENV]{} Lookup: '{}'", self.debug.current_indent(), name);
                }

                // First try looking up in current environment
                if let Some(value) = env.get(name) {
                    return Ok(value);
                }

                // If it's a gensym and not found, try looking it up in the global environment
                // This handles hygienic macro expansion: gensyms reference bindings from
                // where the macro was defined, not where it's used
                if crate::macro_system::hygiene::is_gensym(name.as_ref()) {
                    // Extract original name from gensym (format: ##name#counter)
                    if let Some(original_name) = extract_original_from_gensym(name.as_ref()) {
                        if let Some(value) = self.global_env.get(&Rc::from(original_name)) {
                            return Ok(value);
                        }
                    }
                }

                Err(EvalError::UndefinedVariable(name.to_string()))
            }

            // Empty list
            Value::Null => Ok(Value::Null),

            // Lists (procedure calls or special forms)
            Value::Pair(_) => self.eval_list(expr, env),

            _ => Ok(expr.clone()),
        };

        // Debug trace exit
        if self.debug.is_enabled(debug::DebugStage::Eval) {
            self.debug.dedent();
            match &result {
                Ok(val) => eprintln!("[EVAL]{} => {}", self.debug.current_indent(), val),
                Err(e) => eprintln!("[EVAL]{} => ERROR: {}", self.debug.current_indent(), e),
            }
        }

        result
    }

    fn eval_list(&self, expr: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
        let (car, cdr) = self.extract_pair(expr)?;

        // Check for special forms
        if let Value::Symbol(ref sym) = car {
            match sym.as_ref() {
                "quote" => return self.eval_quote(&cdr),
                "if" => return self.eval_if(&cdr, env),
                "define" => return self.eval_define(&cdr, env),
                "define-syntax" => return self.eval_define_syntax(&cdr, env),
                "set!" => return self.eval_set(&cdr, env),
                "lambda" => return self.eval_lambda(&cdr, env),
                "begin" => return self.eval_begin(&cdr, env),
                "cond" => return self.eval_cond(&cdr, env),
                "let" => return self.eval_let(&cdr, env),
                "let*" => return self.eval_let_star(&cdr, env),
                "letrec" => return self.eval_letrec(&cdr, env),
                "letrec*" => return self.eval_letrec_star(&cdr, env),
                "let-values" => return self.eval_let_values(&cdr, env),
                "let*-values" => return self.eval_let_star_values(&cdr, env),
                "and" => return self.eval_and(&cdr, env),
                "or" => return self.eval_or(&cdr, env),
                "case" => return self.eval_case(&cdr, env),
                "apply" => return self.eval_apply(&cdr, env),
                _ => {}
            }

            // Check if this symbol is bound to a macro
            if let Some(Value::Macro(macro_def)) = env.get(sym) {
                // Debug trace: macro expansion entry
                if self.debug.is_enabled(debug::DebugStage::Expand) {
                    eprintln!(
                        "[MACRO]{} Expanding macro '{}': {}",
                        self.debug.current_indent(),
                        sym,
                        expr
                    );
                    self.debug.indent();
                }

                // Expand the macro with the WHOLE form (unevaluated!)
                // The pattern includes the keyword, so we pass the whole expr
                let expanded = self.expand_macro(&macro_def, expr)?;

                // Debug trace: show expanded form
                if self.debug.is_enabled(debug::DebugStage::Expand) {
                    eprintln!(
                        "[MACRO]{} Expanded to: {}",
                        self.debug.current_indent(),
                        expanded
                    );
                    self.debug.dedent();
                }

                // Evaluate the expanded form
                return self.eval_in_env(&expanded, env);
            }
        }

        // Regular procedure call
        let proc = self.eval_in_env(&car, env)?;
        let args = self.eval_arguments(&cdr, env)?;
        self.apply(proc, args)
    }
}

impl Default for Evaluator {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract the original identifier name from a gensym
///
/// Gensyms have format: ##name#counter
/// This extracts "name" from that format.
fn extract_original_from_gensym(gensym: &str) -> Option<String> {
    if !gensym.starts_with("##") {
        return None;
    }

    // Skip "##" prefix
    let without_prefix = &gensym[2..];

    // Find the last '#' which separates name from counter
    without_prefix
        .rfind('#')
        .map(|last_hash| without_prefix[..last_hash].to_string())
}
