//! PVREF-based template expansion
//!
//! This module implements template expansion for the PVREF-based macro system.
//! It takes compiled Template structures and MatchEnv trees from pattern matching,
//! expanding them into output expressions.
//!
//! Key features:
//! - Uses PVREF encoding for O(1) variable lookup
//! - Uses MatchEnv tree navigation for nested ellipsis
//! - Supports double ellipsis (SRFI-149)
//! - Proper hygiene through Identifier renaming
//!
//! Inspired by Gauche's template expansion (macro.c:800+)
//! Reference: https://github.com/shirok/Gauche/blob/master/src/macro.c
//!
//! # Module Organization
//!
//! - `mod.rs` - Expander struct, constructors, and core expansion logic
//! - `error.rs` - ExpandError enum
//! - `list.rs` - List, vector, and dotted list expansion
//! - `ellipsis.rs` - Ellipsis expansion (single and double)
//! - `hygiene.rs` - Identifier renaming and hygiene support
//! - `tests.rs` - Unit tests

mod ellipsis;
pub mod error;
mod hygiene;
mod list;
#[cfg(test)]
mod tests;

pub use error::ExpandError;

use crate::macro_expander::template::Template;
use crate::macro_expander::utils::{
    list_to_vec as utils_list_to_vec, vec_to_list as utils_vec_to_list,
};
use patina_runtime::{MatchEnv, Value};
use std::rc::Rc;

/// Template expander for PVREF-based macro system
///
/// This implements the template expansion phase of macro expansion.
/// It takes a compiled Template and a MatchEnv with pattern variable bindings,
/// returning the expanded output expression.
///
/// Based on Gauche's template expansion approach (macro.c:800+).
pub struct Expander {
    /// Expansion-time environment (for checking if identifiers are macro names)
    /// We use the full runtime Environment directly to avoid duplication
    expansion_env: Rc<patina_runtime::Environment>,

    /// Macro scope for this expansion (Racket-style hygiene)
    /// Used to distinguish use-site vs introduced identifiers
    macro_scope: patina_runtime::ScopeId,
}

impl Expander {
    /// Create a new expander with runtime environment and macro scope
    ///
    /// # Arguments
    /// * `expansion_env` - Runtime environment at expansion time
    /// * `macro_scope` - Fresh scope for this macro expansion (for Racket-style hygiene)
    pub fn new(
        expansion_env: Rc<patina_runtime::Environment>,
        macro_scope: patina_runtime::ScopeId,
    ) -> Self {
        Self {
            expansion_env,
            macro_scope,
        }
    }

    /// Expand a template with the given match environment
    ///
    /// This is the main entry point for template expansion.
    ///
    /// Inspired by Gauche's expand_template (macro.c:800+)
    pub fn expand(&self, template: &Template, env: &MatchEnv) -> Result<Value, ExpandError> {
        self.expand_impl(template, env, &[], false)
    }

    /// Internal expansion implementation with indices
    ///
    /// # Arguments
    /// * `template` - The template to expand
    /// * `env` - The match environment with variable bindings
    /// * `indices` - Current ellipsis indices for navigation
    /// * `inside_quote` - Whether we're inside a (quote ...) form
    ///
    /// Based on Gauche's expand_rec (macro.c:820+)
    pub(super) fn expand_impl(
        &self,
        template: &Template,
        env: &MatchEnv,
        indices: &[usize],
        inside_quote: bool,
    ) -> Result<Value, ExpandError> {
        match template {
            Template::Literal(value) => {
                // Literal values are inserted as-is
                Ok(value.clone())
            }

            Template::Symbol(id) => {
                // Symbols are renamed for hygiene
                Ok(self.rename_identifier(id))
            }

            Template::Var(pvref) => {
                // Look up pattern variable in match environment
                // Use indices to navigate the tree
                match env.get(*pvref, indices) {
                    Some(value) => {
                        // IMPORTANT: Mark substituted symbols with macro scope for nested macro hygiene.
                        //
                        // When a macro template generates a `define-syntax`, pattern variable
                        // substitution can produce symbols that would be re-interpreted as
                        // pattern variables in the inner macro. For example:
                        //
                        //   (define-syntax foo
                        //     (syntax-rules ()
                        //       ((foo bar y)
                        //        (define-syntax bar
                        //          (syntax-rules ()
                        //            ((bar x) 'y))))))
                        //
                        // When (foo bar x) is called, 'y' substitutes to 'x'. Without marking,
                        // the inner macro sees ((bar x) 'x) and treats x in 'x as a pattern
                        // variable, wrongly returning the argument instead of the symbol 'x.
                        //
                        // By converting Symbol to Identifier with macro_scope, we mark it as
                        // "came from outer expansion". The inner macro compiler can then treat
                        // identifiers with scopes as literals rather than pattern variables.
                        //
                        // HOWEVER: When inside a (quote ...) form, we should NOT mark the values.
                        // Quoted data is self-quoting and should remain as plain symbols so that
                        // memv/memq comparisons work correctly. For example, in the case macro:
                        //   (case key ((datum ...) result))
                        // expands to:
                        //   (memv temp '(datum ...))
                        // The datum values should stay as symbols, not become identifiers with scopes.
                        if inside_quote {
                            Ok(value)
                        } else {
                            Ok(self.mark_substituted_value(value))
                        }
                    }
                    None => Err(ExpandError::UndefinedVariable {
                        pvref: format!("{:?}", pvref),
                    }),
                }
            }

            Template::List(templates) => {
                // Expand list template: (t1 t2 t3)
                self.expand_list(templates, env, indices, inside_quote)
            }

            Template::Vector(templates) => {
                // Expand vector template: #(t1 t2 t3)
                self.expand_vector(templates, env, indices, inside_quote)
            }

            Template::DottedList { templates, tail } => {
                // Expand dotted list template: (t1 t2 . rest)
                self.expand_dotted_list(templates, tail, env, indices, inside_quote)
            }

            Template::Ellipsis {
                subtemplate,
                level,
                nesting,
                vars,
            } => {
                // Expand ellipsis template: (t ...)
                self.expand_ellipsis(
                    subtemplate,
                    *level,
                    *nesting,
                    vars,
                    env,
                    indices,
                    inside_quote,
                )
            }
        }
    }

    /// Convert a Scheme list to a Vec
    pub(super) fn list_to_vec(&self, value: &Value) -> Result<Vec<Value>, ExpandError> {
        utils_list_to_vec(value).map_err(|msg| ExpandError::InvalidTemplate { message: msg })
    }

    /// Convert a Vec to a Scheme list
    pub(super) fn vec_to_list(&self, values: Vec<Value>) -> Value {
        utils_vec_to_list(values)
    }
}

impl Default for Expander {
    fn default() -> Self {
        // Create an empty runtime environment for tests
        use patina_runtime::Environment;
        Self::new(
            Rc::new(Environment::new()),
            patina_runtime::ScopeId::fresh(),
        )
    }
}
