//! Ellipsis escape handling for template compilation
//!
//! This module handles the special case of ellipsis escape in templates:
//! `(... template)` where the ellipsis symbol should be treated literally.

use super::Compiler;
use crate::error::MacroError;
use crate::macro_expander::Template;
use patina_runtime::Value;
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
        form: &Value,
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
    ///
    /// This ensures that nested macro definitions work correctly - the `...`
    /// symbol in the inner syntax-rules will be a plain Symbol that the
    /// macro compiler can recognize.
    fn compile_template_escaped(
        &mut self,
        form: &Value,
        level: usize,
        escaped_ellipsis: &Option<Rc<str>>,
    ) -> Result<Template, MacroError> {
        match form {
            Value::Symbol(s) => {
                // Check if it's the ellipsis symbol that was escaped
                if escaped_ellipsis.as_ref() == Some(s) {
                    // Produce literal Symbol value so nested macros can use it
                    return Ok(Template::Literal(form.clone()));
                }

                // Check if it's a pattern variable
                if let Some(pvref) = self.pvars.get(s) {
                    // Verify level is valid
                    if pvref.level() > level {
                        return Err(MacroError::InvalidSyntax(format!(
                            "Pattern variable {} at level {} used at level {}",
                            s,
                            pvref.level(),
                            level
                        )));
                    }
                    Ok(Template::Var(*pvref))
                } else {
                    // In ellipsis escape context: produce plain Symbol values.
                    // The escaped template is meant to be compiled fresh as a new macro,
                    // so introduced identifiers should NOT carry definition scopes.
                    // They will become pattern variables in the inner macro when compiled.
                    Ok(Template::Literal(form.clone()))
                }
            }

            Value::Pair(_) => {
                let (items, tail) = self.collect_list_items(form)?;

                // Check for quote form
                if items.len() == 2
                    && matches!(&items[0], Value::Symbol(s) if s.as_ref() == "quote")
                    && tail.is_none()
                {
                    // Check if quoted datum contains pattern variables
                    if self.contains_pattern_vars(&items[1]) {
                        // Has pattern variables - compile recursively
                    } else {
                        // No pattern variables - keep as literal
                        return Ok(Template::Literal(form.clone()));
                    }
                }

                if let Some(tail_value) = tail {
                    // Dotted list template
                    let mut templates = Vec::new();
                    for item in items {
                        templates.push(self.compile_template_escaped(
                            &item,
                            level,
                            escaped_ellipsis,
                        )?);
                    }
                    let tail_template = Box::new(self.compile_template_escaped(
                        &tail_value,
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
                            &item,
                            level,
                            escaped_ellipsis,
                        )?);
                    }
                    Ok(Template::List(templates))
                }
            }

            Value::Null => Ok(Template::List(vec![])),

            Value::Vector(v) => {
                let items = v.borrow();
                let mut templates = Vec::new();
                for item in items.iter() {
                    templates.push(self.compile_template_escaped(item, level, escaped_ellipsis)?);
                }
                Ok(Template::Vector(templates))
            }

            // Literal value
            other => Ok(Template::Literal(other.clone())),
        }
    }

    /// Compile a template inside an escaped quote (for ellipsis escape inside quotes)
    ///
    /// This produces Literal templates for non-pattern-variable symbols,
    /// ensuring they stay as plain Symbol values rather than ScopedIdentifiers.
    /// Pattern variables are still expanded normally.
    pub(super) fn compile_quote_template_escaped(
        &mut self,
        form: &Value,
        _level: usize,
    ) -> Result<Template, MacroError> {
        match form {
            Value::Symbol(s) => {
                // Check if it's a pattern variable
                if let Some(pvref) = self.pvars.get(s) {
                    Ok(Template::Var(*pvref))
                } else {
                    // Non-pvar symbol: produce literal Symbol value
                    Ok(Template::Literal(form.clone()))
                }
            }
            Value::Pair(_) => {
                let (items, tail) = self.collect_list_items(form)?;
                if let Some(_tail_value) = tail {
                    // Dotted list - not supported in escaped quote for now
                    return Err(MacroError::InvalidSyntax(
                        "Dotted list in ellipsis-escaped quote not supported".to_string(),
                    ));
                }
                // Compile each item recursively
                let mut templates = Vec::new();
                for item in items {
                    templates.push(self.compile_quote_template_escaped(&item, _level)?);
                }
                Ok(Template::List(templates))
            }
            Value::Null => Ok(Template::List(vec![])),
            Value::Vector(v) => {
                let items = v.borrow();
                let mut templates = Vec::new();
                for item in items.iter() {
                    templates.push(self.compile_quote_template_escaped(item, _level)?);
                }
                Ok(Template::Vector(templates))
            }
            // All other values are literal
            other => Ok(Template::Literal(other.clone())),
        }
    }
}
