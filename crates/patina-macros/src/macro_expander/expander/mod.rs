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
//! - Returns TaggedValue directly
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
use patina_core::{SharedHeap, TaggedValue};
use patina_runtime::MatchEnv;

/// Template expander for PVREF-based macro system
///
/// This implements the template expansion phase of macro expansion.
/// It takes a compiled Template and a MatchEnv with pattern variable bindings,
/// returning the expanded output expression as TaggedValue.
///
/// Based on Gauche's template expansion approach (macro.c:800+).
///
/// ## TaggedValue Output
///
/// The Expander returns TaggedValue directly, eliminating the need for
/// post-expansion Value → TaggedValue conversion. This requires a SharedHeap
/// to allocate heap objects (pairs, vectors, etc.).
///
/// ## MatchEnv Integration
///
/// The Expander retrieves TaggedValue from MatchEnv and uses them directly
/// in the expansion. When constructing output, it allocates on the SharedHeap.
pub struct Expander {
    /// Macro scope for this expansion (Racket-style hygiene)
    /// Used to distinguish use-site vs introduced identifiers
    macro_scope: patina_runtime::ScopeId,

    /// Shared heap for TaggedValue allocation (REQUIRED for TaggedValue output)
    /// Used to allocate pairs, vectors, and other heap objects during expansion
    shared_heap: Option<SharedHeap>,
}

impl Expander {
    /// Create a new expander with macro scope
    ///
    /// # Arguments
    /// * `macro_scope` - Fresh scope for this macro expansion (for Racket-style hygiene)
    pub fn new(macro_scope: patina_runtime::ScopeId) -> Self {
        Self {
            macro_scope,
            shared_heap: None,
        }
    }

    /// Create a new expander with macro scope and SharedHeap
    ///
    /// Create a new expander with macro scope and SharedHeap for heap allocation.
    ///
    /// # Arguments
    /// * `macro_scope` - Fresh scope for this macro expansion (for Racket-style hygiene)
    /// * `shared_heap` - Shared heap for allocating expanded output
    pub fn new_with_heap(macro_scope: patina_runtime::ScopeId, shared_heap: SharedHeap) -> Self {
        Self {
            macro_scope,
            shared_heap: Some(shared_heap),
        }
    }

    /// Get reference to the shared heap (if available)
    pub fn shared_heap(&self) -> Option<&SharedHeap> {
        self.shared_heap.as_ref()
    }

    /// Get the shared heap, panicking if not available
    ///
    /// TaggedValue output requires a shared heap. This method is used internally
    /// to ensure we have one when allocating.
    fn heap(&self) -> &SharedHeap {
        self.shared_heap
            .as_ref()
            .expect("Expander requires SharedHeap for TaggedValue output")
    }

    /// Expand a template with the given match environment
    ///
    /// This is the main entry point for template expansion.
    /// Returns TaggedValue directly, avoiding post-expansion conversion.
    ///
    /// Inspired by Gauche's expand_template (macro.c:800+)
    pub fn expand(&self, template: &Template, env: &MatchEnv) -> Result<TaggedValue, ExpandError> {
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
    ) -> Result<TaggedValue, ExpandError> {
        match template {
            Template::Literal(tv) => {
                // Literal values are inserted as-is (already TaggedValue from compilation)
                Ok(*tv)
            }

            Template::Symbol(id) => {
                // Symbols are renamed for hygiene
                Ok(self.rename_identifier_tagged(id))
            }

            Template::Var(pvref) => {
                // Look up pattern variable in match environment
                // Use indices to navigate the tree
                // MatchEnv.get() returns Option<TaggedValue> directly!
                match env.get(*pvref, indices) {
                    Some(tv) => {
                        if inside_quote {
                            Ok(tv)
                        } else {
                            Ok(self.mark_substituted_tagged(tv))
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
                let ctx = ellipsis::EllipsisContext {
                    subtemplate,
                    env,
                    indices,
                    vars,
                    level: *level,
                    inside_quote,
                };
                self.expand_ellipsis(&ctx, *nesting)
            }
        }
    }

    /// Convert a Scheme list (TaggedValue) to a Vec<TaggedValue>
    pub(super) fn list_to_vec_tagged(
        &self,
        tv: TaggedValue,
    ) -> Result<Vec<TaggedValue>, ExpandError> {
        let heap = self.heap().borrow();
        let mut result = Vec::new();
        let mut current = tv;

        loop {
            if current == TaggedValue::NULL {
                break;
            }
            if current.is_pair() {
                let (car, cdr) = heap.get_pair(current);
                result.push(car);
                current = cdr;
            } else {
                return Err(ExpandError::InvalidTemplate {
                    message: "Expected proper list".to_string(),
                });
            }
        }

        Ok(result)
    }

    /// Convert a Vec<TaggedValue> to a Scheme list (TaggedValue)
    pub(super) fn vec_to_list_tagged(&self, values: Vec<TaggedValue>) -> TaggedValue {
        self.heap().borrow_mut().list_from_iter(values)
    }
}

impl Default for Expander {
    fn default() -> Self {
        Self {
            macro_scope: patina_runtime::ScopeId::fresh(),
            shared_heap: None,
        }
    }
}
