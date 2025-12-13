//! List, vector, and dotted list expansion
//!
//! This module handles expansion of list, vector, and dotted list templates.

use super::Expander;
use super::ellipsis::EllipsisContext;
use super::error::ExpandError;
use crate::macro_expander::Template;
use patina_runtime::{MatchEnv, Value};
use std::cell::RefCell;
use std::rc::Rc;

impl Expander {
    /// Expand a list template
    pub(super) fn expand_list(
        &self,
        templates: &[Template],
        env: &MatchEnv,
        indices: &[usize],
        inside_quote: bool,
    ) -> Result<Value, ExpandError> {
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
                    match expanded {
                        Value::Null => {} // Empty list - nothing to add
                        Value::Pair(_) => {
                            // Convert list to vec and append all elements
                            let items = self.list_to_vec(&expanded)?;
                            result.extend(items);
                        }
                        _ => {
                            return Err(ExpandError::InvalidTemplate {
                                message: "Ellipsis must expand to list".to_string(),
                            });
                        }
                    }
                }
            } else {
                // Regular template - expands to single element
                let value = self.expand_impl(template, env, indices, in_quote_ctx)?;
                result.push(value);
            }
        }

        // Convert vec back to Scheme list
        Ok(self.vec_to_list(result))
    }

    /// Check if a template list represents a (quote ...) form
    pub(super) fn is_quote_template(&self, templates: &[Template]) -> bool {
        if templates.len() >= 2 {
            match &templates[0] {
                Template::Symbol(id) => id.name().as_ref() == "quote",
                Template::Literal(Value::Symbol(s)) => s.as_ref() == "quote",
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
    ) -> Result<Value, ExpandError> {
        let mut result = Vec::new();

        for template in templates {
            let value = self.expand_impl(template, env, indices, inside_quote)?;
            result.push(value);
        }

        Ok(Value::Vector(Rc::new(RefCell::new(result))))
    }

    /// Expand a dotted list template: (t1 t2 . rest)
    pub(super) fn expand_dotted_list(
        &self,
        templates: &[Template],
        tail: &Template,
        env: &MatchEnv,
        indices: &[usize],
        inside_quote: bool,
    ) -> Result<Value, ExpandError> {
        // Expand the fixed part
        let mut items = Vec::new();
        for template in templates {
            let value = self.expand_impl(template, env, indices, inside_quote)?;
            items.push(value);
        }

        // Expand the tail
        let tail_value = self.expand_impl(tail, env, indices, inside_quote)?;

        // Build dotted list
        let mut result = tail_value;
        for item in items.into_iter().rev() {
            result = Value::Pair(Rc::new(RefCell::new((item, result))));
        }

        Ok(result)
    }
}
