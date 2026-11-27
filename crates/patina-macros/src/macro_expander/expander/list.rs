//! List, vector, and dotted list expansion
//!
//! This module handles expansion of list, vector, and dotted list templates.

use super::Expander;
use super::ellipsis::EllipsisContext;
use super::error::ExpandError;
use crate::macro_expander::Template;
use patina_core::{Cx, Value};
use patina_runtime::MatchEnv;

impl<'gc> Expander<'gc> {
    /// Expand a list template
    pub(super) fn expand_list(
        &self,
        cx: &mut Cx<'gc>,
        templates: &[Template],
        env: &MatchEnv<'gc>,
        indices: &[usize],
        inside_quote: bool,
    ) -> Result<Value<'gc>, ExpandError> {
        let mut result = Vec::new();

        // Check if this list starts with 'quote' - if so, we're entering a quoted context
        // for all subsequent elements (the second element in (quote datum))
        let is_quote_form = self.is_quote_template(cx, templates);

        for (i, template) in templates.iter().enumerate() {
            // Inside (quote datum), the datum (index 1) should use inside_quote=true
            let in_quote_ctx = inside_quote || (is_quote_form && i >= 1);
            self.expand_element_into(cx, template, env, indices, in_quote_ctx, &mut result)?;
        }
        self.locate_head(cx, templates, &mut result, inside_quote);

        // Convert vec back to Scheme list
        Ok(cx.list_from_iter(result))
    }

    /// Locate a list this template builds at the use, through its head
    /// (decision 16 on #782, #792): the head's form span becomes the use's
    /// location, or none where the use has none, never the template's own
    /// list. Whatever the head's origin — written in the template or
    /// substituted from the use — the head's own span is untouched, so that
    /// a reference the template introduces still reports the template's
    /// text. A head that is no identifier leaves the list with no location.
    ///
    /// The head a template symbol was just instantiated as is this
    /// context's, and written in place; any other — substituted, or a
    /// literal the macro object holds — another context may have made, and
    /// the list is given a copy. Quoted data is skipped: its identifiers
    /// become symbols, and nothing reads where it was.
    fn locate_head(
        &self,
        cx: &mut Cx<'gc>,
        templates: &[Template],
        elements: &mut [Value<'gc>],
        inside_quote: bool,
    ) {
        let Some(head) = elements.first_mut() else {
            return;
        };
        if inside_quote || !cx.is_identifier(*head) {
            return;
        }
        let form = self.use_location.as_ref();
        if matches!(templates.first(), Some(Template::Symbol(_))) {
            cx.record_form(*head, form);
        } else if let Some(copy) = cx.alloc_identifier_formed(*head, form) {
            *head = copy;
        }
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
        cx: &mut Cx<'gc>,
        template: &Template,
        env: &MatchEnv<'gc>,
        indices: &[usize],
        inside_quote: bool,
        out: &mut Vec<Value<'gc>>,
    ) -> Result<(), ExpandError> {
        let Template::Ellipsis {
            subtemplate,
            level,
            nesting,
            vars,
        } = template
        else {
            out.push(self.expand_impl(cx, template, env, indices, inside_quote)?);
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
        let expanded = self.expand_ellipsis(cx, &ctx, *nesting)?;
        if expanded.is_pair() {
            out.extend(self.list_to_vec_tagged(cx, expanded)?);
            Ok(())
        } else if expanded.is_null() {
            // Zero repetitions contribute nothing.
            Ok(())
        } else {
            Err(ExpandError::InvalidTemplate {
                message: "Ellipsis must expand to list".to_string(),
            })
        }
    }

    /// Check if a template list represents a (quote ...) form
    pub(super) fn is_quote_template(&self, cx: &Cx<'gc>, templates: &[Template]) -> bool {
        if templates.len() >= 2 {
            match &templates[0] {
                Template::Symbol(id) => id.name().as_ref() == "quote",
                Template::Literal(id) => {
                    let literal = self.literal(cx, *id);
                    cx.get_symbol_name(literal).as_deref() == Some("quote")
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
        cx: &mut Cx<'gc>,
        templates: &[Template],
        env: &MatchEnv<'gc>,
        indices: &[usize],
        inside_quote: bool,
    ) -> Result<Value<'gc>, ExpandError> {
        // Ellipsis splicing is shared with expand_list / expand_dotted_list:
        // `#(x ...)` must produce `#(x1 x2)`, not `#((x1 x2))`.
        let mut result = Vec::new();
        for template in templates {
            self.expand_element_into(cx, template, env, indices, inside_quote, &mut result)?;
        }

        // Allocate vector on heap
        Ok(cx.alloc_vector(result))
    }

    /// Expand a dotted list template: (t1 t2 . rest)
    pub(super) fn expand_dotted_list(
        &self,
        cx: &mut Cx<'gc>,
        templates: &[Template],
        tail: &Template,
        env: &MatchEnv<'gc>,
        indices: &[usize],
        inside_quote: bool,
    ) -> Result<Value<'gc>, ExpandError> {
        // Expand the part before the dot. Ellipsis splicing lives in
        // expand_element_into, shared with expand_list.
        let mut items = Vec::new();
        for template in templates {
            self.expand_element_into(cx, template, env, indices, inside_quote, &mut items)?;
        }
        self.locate_head(cx, templates, &mut items, inside_quote);

        // Expand the tail
        let tail_value = self.expand_impl(cx, tail, env, indices, inside_quote)?;

        // Build dotted list
        Ok(cx.list_from_iter_with_tail(items, tail_value))
    }
}
