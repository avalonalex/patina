//! Ellipsis escape handling for template compilation
//!
//! This module handles the special case of ellipsis escape in templates:
//! `(... template)` where the ellipsis symbol should be treated literally.

use super::Compiler;
use crate::error::MacroError;
use crate::macro_expander::Template;
use patina_core::TaggedValue;

impl Compiler {
    /// Compile a template with the ellipsis temporarily disabled
    ///
    /// Used for ellipsis escape: `(... template)`
    ///
    /// When the ellipsis is escaped it becomes a literal template value, so the
    /// inner `syntax-rules` of a nested macro definition receives it as data and
    /// can recognize it as its own ellipsis.
    pub(super) fn compile_with_escaped_ellipsis(
        &mut self,
        form: TaggedValue,
        level: usize,
    ) -> Result<Template, MacroError> {
        // Disable the ellipsis and record the spelling it had, then compile
        // the form as the ordinary template it is: with `self.ellipsis` gone
        // `is_ellipsis` answers `false` everywhere (`helpers.rs`), so
        // `compile_template`'s ellipsis branches are inert, and its
        // identifier branch picks the suspended spelling out by the field
        // set here. This used to be a parallel walker, and the two copies
        // had already drifted — the escaped one lacked the `'(... t)`
        // sub-case, and every symbol under an escape came out a bare
        // literal, which is what made references there neither hygienic nor
        // relinkable.
        //
        // The result is bound before the fields are restored, so an error
        // inside the escape restores them too.
        let saved_ellipsis = self.ellipsis.take();
        let saved_escaped = self.escaped_ellipsis.take();
        self.escaped_ellipsis = saved_ellipsis.clone();

        let result = self.compile_template(form, level);

        self.ellipsis = saved_ellipsis;
        self.escaped_ellipsis = saved_escaped;

        result
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
                let elements = self.heap.borrow().vector_slice(form).to_vec();
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
