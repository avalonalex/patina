//! Ellipsis escape handling for template compilation
//!
//! This module handles the special case of ellipsis escape in templates:
//! `(... template)` where the ellipsis symbol should be treated literally.

use super::Compiler;
use crate::error::MacroError;
use crate::macro_expander::Template;
use patina_core::TaggedValue;
use std::rc::Rc;

impl Compiler {
    /// Compile template with ellipsis temporarily disabled
    ///
    /// Used for ellipsis escape: (... template)
    ///
    /// When the ellipsis is escaped it becomes a literal template value, so the
    /// inner `syntax-rules` of a nested macro definition receives it as data and
    /// can recognize it as its own ellipsis.
    ///
    /// That literal is an *identifier* carrying this macro's definition scopes,
    /// not a bare symbol. The scopes are not what the later compiler reads —
    /// they are usually empty, since the macro escaping an ellipsis is usually
    /// top level — being an identifier at all is: that is what marks the token
    /// as introduced here rather than written wherever the generated macro is
    /// compiled, which decides whether `...` still names the ellipsis under
    /// R7RS 4.3.2. Emitting a bare symbol made `(let ((... 'dots)) (def-first
    /// f))` generate a macro with no ellipsis.
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
                // Produce a literal Symbol value so nested macros can use it —
                // carrying this macro's definition scopes, so that it stays an
                // ellipsis where it lands.
                //
                // `(... ...)` exists to hand an ellipsis to a `syntax-rules`
                // this macro generates, and that macro may be expanded
                // somewhere `...` is bound as a variable, where the default
                // ellipsis is not the ellipsis (R7RS 4.3.2). Emitted bare, the
                // token would arrive with no scopes and be read against the
                // *use* site's bindings, so `(let ((... 'dots)) (def-first f))`
                // would silently produce a macro with no ellipsis at all.
                // Stamped with the scopes it was written in, it keeps naming
                // what it named here. Bare is still right when this macro has
                // no scopes to give.
                // Stamped even when this macro is top level and its scope set
                // is empty: what the later compiler reads is not the scopes
                // themselves but that the token is an *identifier* at all,
                // which is what says it was introduced here rather than
                // written at the site it lands in. An empty set then correctly
                // means "top level", where `...` is the ellipsis.
                let stamped = {
                    let mut heap = self.heap.borrow_mut();
                    heap.alloc_identifier(s.clone(), self.definition_scopes.clone())
                };
                return Ok(self.make_literal_template(stamped));
            }

            // Anything else is a symbol like any other in a template — a
            // pattern variable, or a reference that resolves where this
            // macro was written — and `compile_template` already knows how
            // to compile one. The escape only changes what `...` means. It
            // used to emit every non-pattern-variable symbol here as a bare
            // literal, so nothing under `(... …)` was hygienic or could be
            // relinked: a library-private `helper` inside an escape was
            // unbound at the use site, and a program's own `tag` captured
            // the one a generated macro's template meant.
            self.compile_template(form, level)
        } else {
            // Check for pair
            let is_pair = form.is_pair();
            if is_pair {
                let (items, tail) = self.collect_list_items(form)?;

                // Check for quote form — outside a quasiquote, as in
                // `compile_template`.
                if self.quasiquote_depth == 0
                    && items.len() == 2
                    && self
                        .extract_symbol_name(items[0])
                        .map(|s| s.as_ref() == "quote")
                        .unwrap_or(false)
                    && tail.is_none()
                {
                    // Check if quoted datum contains pattern variables
                    if !self.contains_pattern_vars(items[1]) {
                        // No pattern variables: the datum is a literal, the
                        // `quote` in front of it a reference — the same
                        // split `compile_template` makes.
                        let head = self.compile_template(items[0], level)?;
                        return Ok(Template::List(vec![
                            head,
                            self.make_literal_template(items[1]),
                        ]));
                    }
                    // Has pattern variables - compile recursively (fall through)
                }

                let step = self.quasiquote_step(&items, tail);
                self.quasiquote_depth = self.quasiquote_depth.wrapping_add_signed(step);
                let compiled = if let Some(tail_value) = tail {
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
                };
                self.quasiquote_depth = self.quasiquote_depth.wrapping_add_signed(-step);
                compiled
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
