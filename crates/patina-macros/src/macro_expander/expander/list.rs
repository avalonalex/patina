//! List, vector, and dotted list expansion
//!
//! This module handles expansion of list, vector, and dotted list templates.
//! All methods return TaggedValue directly.

use super::Expander;
use super::ellipsis::EllipsisContext;
use super::error::ExpandError;
use crate::macro_expander::Template;
use patina_core::TaggedValue;
use patina_runtime::MatchEnv;

impl Expander {
    /// Expand a list template
    pub(super) fn expand_list(
        &self,
        templates: &[Template],
        env: &MatchEnv,
        indices: &[usize],
        inside_quote: bool,
    ) -> Result<TaggedValue, ExpandError> {
        let mut result = Vec::new();

        // Check if this list starts with 'quote' - if so, we're entering a quoted context
        // for all subsequent elements (the second element in (quote datum))
        let is_quote_form = self.is_quote_template(templates);

        for (i, template) in templates.iter().enumerate() {
            // Inside (quote datum), the datum (index 1) should use inside_quote=true
            let in_quote_ctx = inside_quote || (is_quote_form && i >= 1);
            self.expand_element_into(template, env, indices, in_quote_ctx, &mut result)?;
        }

        // Convert vec back to Scheme list
        Ok(self.vec_to_list_tagged(result))
    }

    /// Expand one template into `out`.
    ///
    /// An ellipsis contributes each of its repetitions, *spliced* into `out`;
    /// every other template contributes exactly one value. Splicing is the whole
    /// point of the shared helper: `(x ... . t)` must produce `(x1 x2 . t)`, and
    /// getting that wrong is silent rather than loud.
    ///
    /// The quote context is the caller's to decide. `expand_list` computes a
    /// per-element flag so the datum of a `(quote datum)` form expands quoted;
    /// the dotted-list path has no such notion and passes its inherited context
    /// through unchanged.
    fn expand_element_into(
        &self,
        template: &Template,
        env: &MatchEnv,
        indices: &[usize],
        inside_quote: bool,
        out: &mut Vec<TaggedValue>,
    ) -> Result<(), ExpandError> {
        let Template::Ellipsis {
            subtemplate,
            level,
            nesting,
            vars,
        } = template
        else {
            out.push(self.expand_impl(template, env, indices, inside_quote)?);
            return Ok(());
        };

        let ctx = EllipsisContext {
            subtemplate,
            env,
            indices,
            vars,
            level: *level,
            inside_quote,
        };
        let expanded = self.expand_ellipsis(&ctx, *nesting)?;
        if expanded.is_pair() {
            out.extend(self.list_to_vec_tagged(expanded)?);
            Ok(())
        } else if expanded == TaggedValue::NULL {
            // Zero repetitions contribute nothing.
            Ok(())
        } else {
            Err(ExpandError::InvalidTemplate {
                message: "Ellipsis must expand to list".to_string(),
            })
        }
    }

    /// Check if a template list represents a (quote ...) form
    pub(super) fn is_quote_template(&self, templates: &[Template]) -> bool {
        if templates.len() >= 2 {
            match &templates[0] {
                Template::Symbol(id) => id.name().as_ref() == "quote",
                Template::Literal(tv) => {
                    let heap = self.heap().borrow();
                    heap.get_symbol_name(*tv) == Some("quote")
                }
                _ => false,
            }
        } else {
            false
        }
    }

    /// Expand a vector template
    pub(super) fn expand_vector(
        &self,
        templates: &[Template],
        env: &MatchEnv,
        indices: &[usize],
        inside_quote: bool,
    ) -> Result<TaggedValue, ExpandError> {
        // Ellipsis splicing is shared with expand_list / expand_dotted_list:
        // `#(x ...)` must produce `#(x1 x2)`, not `#((x1 x2))`.
        let mut result = Vec::new();
        for template in templates {
            self.expand_element_into(template, env, indices, inside_quote, &mut result)?;
        }

        // Allocate vector on heap
        let mut heap = self.heap().borrow_mut();
        Ok(heap.alloc_vector(result))
    }

    /// Expand a dotted list template: (t1 t2 . rest)
    pub(super) fn expand_dotted_list(
        &self,
        templates: &[Template],
        tail: &Template,
        env: &MatchEnv,
        indices: &[usize],
        inside_quote: bool,
    ) -> Result<TaggedValue, ExpandError> {
        // Expand the part before the dot. Ellipsis splicing lives in
        // expand_element_into, shared with expand_list.
        let mut items = Vec::new();
        for template in templates {
            self.expand_element_into(template, env, indices, inside_quote, &mut items)?;
        }

        // Expand the tail
        let tail_value = self.expand_impl(tail, env, indices, inside_quote)?;

        // Build dotted list
        Ok(self
            .heap()
            .borrow_mut()
            .list_from_iter_with_tail(items, tail_value))
    }
}
