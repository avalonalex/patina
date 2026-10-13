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
//! - Reads and allocates through the context it is given
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

use crate::macro_expander::Template;
use patina_core::{Cx, LiteralId, Value};
use patina_runtime::MatchEnv;

/// Template expander for PVREF-based macro system
///
/// This implements the template expansion phase of macro expansion.
/// It takes a compiled Template and a MatchEnv with pattern variable bindings,
/// returning the expanded output expression.
///
/// Based on Gauche's template expansion approach (macro.c:800+).
///
/// ## Values In, Values Out
///
/// The Expander reads the values its MatchEnv binds and allocates the
/// expansion through the context it is given, the one the input was matched
/// in: what it answers is a `Value<'gc>` of that context.
pub struct Expander<'gc> {
    /// Macro scope for this expansion (Racket-style hygiene)
    /// Used to distinguish use-site vs introduced identifiers
    macro_scope: patina_runtime::ScopeId,

    /// The macro object being expanded, which holds the literals its
    /// templates name (#718). Unset, a literal template finds none.
    mac: Value<'gc>,

    /// How deep a substituted value may nest: what is left of
    /// [`MAX_FORM_DEPTH`] below the use being expanded (#617).
    ///
    /// [`MAX_FORM_DEPTH`]: patina_core::walk::MAX_FORM_DEPTH
    depth_limit: usize,
}

impl<'gc> Expander<'gc> {
    /// Create a new expander with macro scope
    ///
    /// # Arguments
    /// * `macro_scope` - Fresh scope for this macro expansion (for Racket-style hygiene)
    pub fn new(macro_scope: patina_runtime::ScopeId) -> Self {
        Self {
            macro_scope,
            mac: Value::UNSPECIFIED,
            depth_limit: patina_core::walk::MAX_FORM_DEPTH,
        }
    }

    /// Expanding the templates of the macro object `mac`, whose literals
    /// they name by index (#718).
    pub fn for_macro(mut self, mac: Value<'gc>) -> Self {
        self.mac = mac;
        self
    }

    /// Expanding a use `depth` forms deep: a value substituted into the
    /// expansion may nest only what is left of [`MAX_FORM_DEPTH`] below it.
    /// One that nests deeper would take the expansion past the limit where it
    /// is desugared, and is refused here, before every level of it is marked
    /// at every level above (#617).
    ///
    /// [`MAX_FORM_DEPTH`]: patina_core::walk::MAX_FORM_DEPTH
    pub fn at_depth(mut self, depth: usize) -> Self {
        self.depth_limit = patina_core::walk::MAX_FORM_DEPTH.saturating_sub(depth);
        self
    }

    /// The literal `id` of the macro being expanded.
    pub(super) fn literal(&self, cx: &Cx<'gc>, id: LiteralId) -> Value<'gc> {
        cx.macro_literal(self.mac, id)
            .expect("a template's literal is its macro object's")
    }

    /// Expand a template with the given match environment
    ///
    /// This is the main entry point for template expansion.
    ///
    /// Inspired by Gauche's expand_template (macro.c:800+)
    pub fn expand(
        &self,
        cx: &mut Cx<'gc>,
        template: &Template,
        env: &MatchEnv<'gc>,
    ) -> Result<Value<'gc>, ExpandError> {
        self.expand_impl(cx, template, env, &[], false)
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
        cx: &mut Cx<'gc>,
        template: &Template,
        env: &MatchEnv<'gc>,
        indices: &[usize],
        inside_quote: bool,
    ) -> Result<Value<'gc>, ExpandError> {
        // As deep as the template: grown as it goes (#617).
        patina_core::walk::ensure_sufficient_stack_if(template.is_nested(), || {
            self.expand_node(cx, template, env, indices, inside_quote)
        })
    }

    /// [`Self::expand_impl`] for one node of the template.
    fn expand_node(
        &self,
        cx: &mut Cx<'gc>,
        template: &Template,
        env: &MatchEnv<'gc>,
        indices: &[usize],
        inside_quote: bool,
    ) -> Result<Value<'gc>, ExpandError> {
        match template {
            Template::Literal(id) => {
                // Literal values are inserted as-is, from the macro object.
                Ok(self.literal(cx, *id))
            }

            Template::Symbol(id) => {
                // Symbols are renamed for hygiene
                Ok(self.rename_identifier_tagged(cx, id))
            }

            Template::Var(pvref) => {
                // Look up pattern variable in match environment
                // Use indices to navigate the tree
                match env.get(*pvref, indices) {
                    Some(tv) => {
                        if inside_quote {
                            Ok(tv)
                        } else {
                            self.mark_substituted_tagged(cx, tv)
                        }
                    }
                    None => Err(ExpandError::UndefinedVariable {
                        pvref: format!("{:?}", pvref),
                    }),
                }
            }

            Template::List(templates) => {
                // Expand list template: (t1 t2 t3)
                self.expand_list(cx, templates, env, indices, inside_quote)
            }

            Template::Vector(templates) => {
                // Expand vector template: #(t1 t2 t3)
                self.expand_vector(cx, templates, env, indices, inside_quote)
            }

            Template::DottedList { templates, tail } => {
                // Expand dotted list template: (t1 t2 . rest)
                self.expand_dotted_list(cx, templates, tail, env, indices, inside_quote)
            }

            Template::Ellipsis {
                subtemplate,
                level,
                nesting,
                vars,
            } => {
                // Expand ellipsis template: (t ...)
                let ctx = ellipsis::EllipsisContext {
                    subtemplate,
                    env,
                    indices,
                    vars,
                    level: *level,
                    inside_quote,
                };
                self.expand_ellipsis(cx, &ctx, *nesting)
            }
        }
    }

    /// Convert a Scheme list to a Vec
    pub(super) fn list_to_vec_tagged(
        &self,
        cx: &Cx<'gc>,
        tv: Value<'gc>,
    ) -> Result<Vec<Value<'gc>>, ExpandError> {
        // `Heap::list_to_vec` refuses a circular list as well as an improper
        // one, where a plain walk followed a circular one forever (#459).
        cx.list_to_vec(tv)
            .ok_or_else(|| ExpandError::InvalidTemplate {
                message: "Expected proper list".to_string(),
            })
    }
}

impl Default for Expander<'_> {
    fn default() -> Self {
        Self::new(patina_runtime::ScopeId::fresh())
    }
}
