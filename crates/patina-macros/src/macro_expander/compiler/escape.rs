//! Ellipsis escape handling for template compilation
//!
//! This module handles the special case of ellipsis escape in templates:
//! `(... template)` where the ellipsis symbol should be treated literally.

use super::Compiler;
use crate::error::MacroError;
use crate::macro_expander::{Identifier, Template};
use patina_core::TaggedValue;
use std::rc::Rc;

impl Compiler {
    /// Compile template with ellipsis temporarily disabled
    ///
    /// Used for ellipsis escape: (... template)
    ///
    /// When ellipsis is escaped, `...` symbols should become literal Symbol values,
    /// not ScopedIdentifier/WrappedIdentifier values. This is crucial for nested
    /// macro definitions where the inner `syntax-rules` needs to recognize `...`
    /// as the ellipsis symbol.
    pub(super) fn compile_with_escaped_ellipsis(
        &mut self,
        form: TaggedValue,
        level: usize,
    ) -> Result<Template, MacroError> {
        // Save current ellipsis setting
        let saved_ellipsis = self.ellipsis.take();

        // Compile with special handling for ellipsis symbols
        let result = self.compile_template_escaped(form, level, &saved_ellipsis);

        // Restore ellipsis setting
        self.ellipsis = saved_ellipsis;

        result
    }

    /// Compile a template inside an ellipsis escape context
    ///
    /// This is similar to compile_template but:
    /// 1. Ellipsis is disabled (not treated as an operator)
    /// 2. The ellipsis symbol itself becomes a literal Symbol value
    fn compile_template_escaped(
        &mut self,
        form: TaggedValue,
        level: usize,
        escaped_ellipsis: &Option<Rc<str>>,
    ) -> Result<Template, MacroError> {
        // Check for symbol/identifier
        if let Some(s) = self.extract_symbol_name(form) {
            // Check if it's the ellipsis symbol that was escaped
            if escaped_ellipsis.as_ref() == Some(&s) {
                // The nested macro's ellipsis — emitted as an identifier
                // carrying this macro's scopes, like any template symbol,
                // not as a bare literal: a bare `...` inside a scope that
                // binds `...` is that variable (R7RS 4.3.2, see
                // `EllipsisBinding`), whereas this one came from *here*.
                return Ok(Template::Symbol(Identifier::with_scopes(
                    s,
                    self.definition_scopes.clone(),
                )));
            }

            // Check if it's a pattern variable
            if let Some(pvref) = self.lookup_pvar(form) {
                // Verify level is valid
                if pvref.level() > level {
                    return Err(MacroError::InvalidSyntax(format!(
                        "Pattern variable {} at level {} used at level {}",
                        s,
                        pvref.level(),
                        level
                    )));
                }
                Ok(Template::Var(pvref))
            } else {
                // In ellipsis escape context: produce plain Symbol values.
                Ok(self.make_literal_template(form))
            }
        } else {
            // Check for pair
            let is_pair = form.is_pair();
            if is_pair {
                let (items, tail) = self.collect_list_items(form)?;

                // Check for quote form
                if items.len() == 2
                    && self
                        .extract_symbol_name(items[0])
                        .map(|s| s.as_ref() == "quote")
                        .unwrap_or(false)
                    && tail.is_none()
                {
                    // Check if quoted datum contains pattern variables
                    if !self.contains_pattern_vars(items[1]) {
                        // No pattern variables - keep as literal
                        return Ok(self.make_literal_template(form));
                    }
                    // Has pattern variables - compile recursively (fall through)
                }

                if let Some(tail_value) = tail {
                    // Dotted list template
                    let mut templates = Vec::new();
                    for item in items {
                        templates.push(self.compile_template_escaped(
                            item,
                            level,
                            escaped_ellipsis,
                        )?);
                    }
                    let tail_template = Box::new(self.compile_template_escaped(
                        tail_value,
                        level,
                        escaped_ellipsis,
                    )?);
                    Ok(Template::DottedList {
                        templates,
                        tail: tail_template,
                    })
                } else {
                    // Regular list template
                    let mut templates = Vec::new();
                    for item in items {
                        templates.push(self.compile_template_escaped(
                            item,
                            level,
                            escaped_ellipsis,
                        )?);
                    }
                    Ok(Template::List(templates))
                }
            } else if form == TaggedValue::NULL {
                Ok(Template::List(vec![]))
            } else if form.is_vector() {
                let heap = self.heap.borrow();
                let len = heap.vector_len(form);
                let elements: Vec<TaggedValue> =
                    (0..len).map(|i| heap.vector_ref(form, i)).collect();
                drop(heap);
                let mut templates = Vec::new();
                for item in elements {
                    templates.push(self.compile_template_escaped(item, level, escaped_ellipsis)?);
                }
                Ok(Template::Vector(templates))
            } else {
                // Literal value
                Ok(self.make_literal_template(form))
            }
        }
    }

    /// Compile a template inside an escaped quote (for ellipsis escape inside quotes)
    ///
    /// This produces Literal templates for non-pattern-variable symbols,
    /// ensuring they stay as plain Symbol values rather than ScopedIdentifiers.
    /// Pattern variables are still expanded normally.
    pub(super) fn compile_quote_template_escaped(
        &mut self,
        form: TaggedValue,
        _level: usize,
    ) -> Result<Template, MacroError> {
        // Check for symbol
        if self.extract_symbol_name(form).is_some() {
            // Check if it's a pattern variable
            if let Some(pvref) = self.lookup_pvar(form) {
                Ok(Template::Var(pvref))
            } else {
                // Non-pvar symbol: produce literal Symbol value
                Ok(self.make_literal_template(form))
            }
        } else {
            let is_pair = form.is_pair();
            if is_pair {
                let (items, tail) = self.collect_list_items(form)?;
                if tail.is_some() {
                    return Err(MacroError::InvalidSyntax(
                        "Dotted list in ellipsis-escaped quote not supported".to_string(),
                    ));
                }
                // Compile each item recursively
                let mut templates = Vec::new();
                for item in items {
                    templates.push(self.compile_quote_template_escaped(item, _level)?);
                }
                Ok(Template::List(templates))
            } else if form == TaggedValue::NULL {
                Ok(Template::List(vec![]))
            } else if form.is_vector() {
                let heap = self.heap.borrow();
                let len = heap.vector_len(form);
                let elements: Vec<TaggedValue> =
                    (0..len).map(|i| heap.vector_ref(form, i)).collect();
                drop(heap);
                let mut templates = Vec::new();
                for item in elements {
                    templates.push(self.compile_quote_template_escaped(item, _level)?);
                }
                Ok(Template::Vector(templates))
            } else {
                // All other values are literal
                Ok(self.make_literal_template(form))
            }
        }
    }
}
