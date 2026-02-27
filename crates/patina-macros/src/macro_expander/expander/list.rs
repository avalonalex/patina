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

            if template.is_ellipsis() {
                // Handle ellipsis specially - it expands to multiple elements
                if let Template::Ellipsis {
                    subtemplate,
                    level,
                    nesting,
                    vars,
                } = template
                {
                    let ctx = EllipsisContext {
                        subtemplate,
                        env,
                        indices,
                        vars,
                        level: *level,
                        inside_quote: in_quote_ctx,
                    };
                    let expanded = self.expand_ellipsis(&ctx, *nesting)?;

                    // Expanded ellipsis should be a list - splice it in
                    if expanded == TaggedValue::NULL {
                        // Empty list - nothing to add
                    } else if expanded.is_pair() {
                        // Convert list to vec and append all elements
                        let items = self.list_to_vec_tagged(expanded)?;
                        result.extend(items);
                    } else {
                        return Err(ExpandError::InvalidTemplate {
                            message: "Ellipsis must expand to list".to_string(),
                        });
                    }
                }
            } else {
                // Regular template - expands to single element
                let value = self.expand_impl(template, env, indices, in_quote_ctx)?;
                result.push(value);
            }
        }

        // Convert vec back to Scheme list
        Ok(self.vec_to_list_tagged(result))
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
        let mut result = Vec::new();

        for template in templates {
            let value = self.expand_impl(template, env, indices, inside_quote)?;
            result.push(value);
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
        // Expand the fixed part
        let mut items = Vec::new();
        for template in templates {
            let value = self.expand_impl(template, env, indices, inside_quote)?;
            items.push(value);
        }

        // Expand the tail
        let tail_value = self.expand_impl(tail, env, indices, inside_quote)?;

        // Build dotted list (from back to front)
        let mut result = tail_value;
        let mut heap = self.heap().borrow_mut();
        for item in items.into_iter().rev() {
            result = heap.alloc_pair(item, result);
        }

        Ok(result)
    }
}
