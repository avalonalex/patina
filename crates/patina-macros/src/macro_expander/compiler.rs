//! Pattern and template compiler for PVREF-based macro system
//!
//! This module compiles Scheme syntax-rules patterns and templates into
//! efficient PVREF (Pattern Variable Reference) based representations.
//!
//! Inspired by Gauche Scheme's pattern compilation (macro.c:400-683)
//! by Shiro Kawai.
//!
//! Key concepts:
//! - Two-phase design: compile pattern once, match many times
//! - PVREF encoding for O(1) variable lookup
//! - Precomputed metadata (num_following, vars) for optimization
//!
//! Reference: https://github.com/shirok/Gauche/blob/master/src/macro.c

use super::pattern::Pattern;
use super::template::{Identifier, Template};
use crate::error::MacroError;
use patina_runtime::{Environment, PVRef, ScopeSet, Value};
use std::collections::HashMap;
use std::rc::Rc;

// Re-export CompiledMacro and CompiledRule from patina-core (via patina-runtime)
pub use patina_runtime::{CompiledMacro, CompiledRule};

/// Pattern and template compiler
///
/// Compiles Scheme S-expressions into PVREF-based Pattern2/Template2.
///
/// Based on Gauche's compile_rules (macro.c:604-683).
pub struct Compiler {
    /// Literal identifiers
    literals: Vec<Rc<str>>,

    /// Symbol used for ellipsis (usually "...")
    /// None means ellipsis is disabled (inside escape)
    ellipsis: Option<Rc<str>>,

    /// Lexical environment where the macro is being defined (for hygiene)
    ///
    /// Free variables in templates will capture this environment.
    /// This enables proper lexical scoping for macros following Gauche's approach.
    env: Option<Rc<Environment>>,

    /// Scope set at macro definition time (for scope-based hygiene)
    ///
    /// Free variables will carry this scope set so they resolve to
    /// definition-time bindings, not use-site bindings.
    definition_scopes: ScopeSet,

    // Per-rule compilation context
    /// Map from pattern variable name to PVREF
    pvars: HashMap<Rc<str>, PVRef>,

    /// Counter for assigning PVREF indices
    pvar_count: usize,

    /// Maximum ellipsis level seen so far
    max_level: usize,
}

impl Compiler {
    /// Create a new compiler
    ///
    /// # Arguments
    /// - `literals`: List of literal identifier names
    /// - `ellipsis`: Symbol to use for ellipsis (typically "...")
    pub fn new(literals: Vec<Rc<str>>, ellipsis: Option<Rc<str>>) -> Self {
        Self {
            literals,
            ellipsis: ellipsis.or_else(|| Some("...".into())),
            env: None,
            definition_scopes: ScopeSet::new(),
            pvars: HashMap::new(),
            pvar_count: 0,
            max_level: 0,
        }
    }

    /// Create a new compiler with environment capture (for hygiene)
    ///
    /// # Arguments
    /// - `literals`: List of literal identifier names
    /// - `ellipsis`: Symbol to use for ellipsis (typically "...")
    /// - `env`: Lexical environment where the macro is being defined
    pub fn with_env(
        literals: Vec<Rc<str>>,
        ellipsis: Option<Rc<str>>,
        env: Rc<Environment>,
    ) -> Self {
        Self {
            literals,
            ellipsis: ellipsis.or_else(|| Some("...".into())),
            env: Some(env),
            definition_scopes: ScopeSet::new(),
            pvars: HashMap::new(),
            pvar_count: 0,
            max_level: 0,
        }
    }

    /// Create a new compiler with environment and scope set (for scope-based hygiene)
    ///
    /// # Arguments
    /// - `literals`: List of literal identifier names
    /// - `ellipsis`: Symbol to use for ellipsis (typically "...")
    /// - `env`: Lexical environment where the macro is being defined
    /// - `scopes`: Scope set at macro definition time
    ///
    /// Free variables in templates will carry the scope set so they resolve to
    /// definition-time bindings, not use-site bindings.
    pub fn with_env_and_scopes(
        literals: Vec<Rc<str>>,
        ellipsis: Option<Rc<str>>,
        env: Rc<Environment>,
        scopes: ScopeSet,
    ) -> Self {
        Self {
            literals,
            ellipsis: ellipsis.or_else(|| Some("...".into())),
            env: Some(env),
            definition_scopes: scopes,
            pvars: HashMap::new(),
            pvar_count: 0,
            max_level: 0,
        }
    }

    /// Compile a complete macro definition
    ///
    /// # Arguments
    /// - `name`: Macro name
    /// - `rules`: List of (pattern, template) pairs as S-expressions
    ///
    /// # Returns
    /// Compiled macro with all rules in PVREF form
    pub fn compile_macro(
        &mut self,
        name: Rc<str>,
        rules: Vec<(Value, Value)>,
    ) -> Result<CompiledMacro, MacroError> {
        let mut compiled_rules = Vec::new();
        let mut max_pvars = 0;

        for (pat_form, tmpl_form) in rules {
            // Reset per-rule context
            self.pvars.clear();
            self.pvar_count = 0;
            self.max_level = 0;

            // R7RS: The first element of each pattern is the macro keyword placeholder
            // and should be ignored (treated as wildcard). This is true even if the
            // symbol appears in the literals list (e.g., when _ is in literals).
            let pattern = self.compile_rule_pattern(&pat_form, 0)?;
            let template = self.compile_template(&tmpl_form, 0)?;

            // Build reverse mapping: PVREF -> name (for debug output)
            let pvar_names: HashMap<PVRef, Rc<str>> = self
                .pvars
                .iter()
                .map(|(name, pvref)| (*pvref, name.clone()))
                .collect();

            // Validate the rule before adding it
            if let Err(e) = super::validator::validate_rule(&pattern, &template, &pvar_names) {
                return Err(MacroError::InvalidSyntax(format!(
                    "Macro '{}' validation failed: {}",
                    name, e
                )));
            }

            compiled_rules.push(CompiledRule {
                pattern,
                template,
                num_pvars: self.pvar_count,
                max_level: self.max_level,
                pvar_names,
            });

            max_pvars = max_pvars.max(self.pvar_count);
        }

        Ok(CompiledMacro {
            name,
            literals: self.literals.clone(),
            rules: compiled_rules,
            max_pvars,
            definition_scopes: self.definition_scopes.clone(),
        })
    }

    /// Compile a top-level syntax-rules pattern (a rule pattern)
    ///
    /// R7RS Section 4.3.2: "The first element of each clause is the keyword, which
    /// is ignored." This means the first element of the pattern list is a placeholder
    /// for the macro name and should always be treated as a wildcard, regardless of
    /// whether it appears in the literals list.
    ///
    /// For example, in:
    ///   (syntax-rules (_) ((_ x) x))
    /// The first `_` in the pattern `(_ x)` is the macro keyword placeholder and should
    /// match the macro name, NOT be treated as a literal that only matches `_`.
    fn compile_rule_pattern(&mut self, form: &Value, level: usize) -> Result<Pattern, MacroError> {
        match form {
            Value::Pair(_) => {
                let (items, tail) = self.collect_list_items(form)?;
                if items.is_empty() {
                    return Err(MacroError::InvalidSyntax(
                        "Pattern must have at least a macro keyword".to_string(),
                    ));
                }

                if let Some(tail_value) = tail {
                    // Dotted list pattern: (keyword p1 p2 . rest)
                    // First element is keyword (wildcard), rest are normal patterns
                    let mut patterns = vec![Pattern::Wildcard];
                    for item in items.iter().skip(1) {
                        patterns.push(self.compile_pattern(item, level)?);
                    }
                    let tail_pattern = Box::new(self.compile_pattern(&tail_value, level)?);
                    Ok(Pattern::DottedList {
                        patterns,
                        tail: tail_pattern,
                    })
                } else {
                    // Regular list pattern: (keyword p1 p2 ...)
                    // First element is keyword (wildcard), rest are compiled with ellipsis detection
                    self.compile_rule_list_pattern(&items, level)
                }
            }
            _ => Err(MacroError::InvalidSyntax(
                "Pattern must be a list starting with macro keyword".to_string(),
            )),
        }
    }

    /// Compile a rule list pattern where the first element is the macro keyword
    ///
    /// This is similar to compile_list_pattern but treats the first element as a wildcard.
    fn compile_rule_list_pattern(
        &mut self,
        items: &[Value],
        level: usize,
    ) -> Result<Pattern, MacroError> {
        let mut patterns = vec![Pattern::Wildcard]; // First element is always wildcard
        let mut i = 1; // Start from second element

        while i < items.len() {
            // Check for ellipsis
            if i + 1 < items.len() && self.is_ellipsis(&items[i + 1]) {
                // Found ellipsis pattern: (item ...)
                let num_following = items.len() - i - 2;
                let start_pvars = self.pvar_count;
                let subpattern = self.compile_pattern(&items[i], level + 1)?;
                let end_pvars = self.pvar_count;

                let mut vars = Vec::new();
                for idx in start_pvars..end_pvars {
                    vars.push(PVRef::new((level + 1) as u8, idx as u8));
                }

                self.max_level = self.max_level.max(level + 1);

                patterns.push(Pattern::Ellipsis {
                    subpattern: Box::new(subpattern),
                    level: (level + 1) as u8,
                    num_following,
                    vars,
                });

                i += 2; // Skip pattern and ellipsis
            } else {
                patterns.push(self.compile_pattern(&items[i], level)?);
                i += 1;
            }
        }

        Ok(Pattern::List(patterns))
    }

    /// Compile a pattern at the given ellipsis level
    ///
    /// Based on Gauche's compile_rule1 (macro.c:400+).
    ///
    /// # Arguments
    /// - `form`: S-expression representing the pattern
    /// - `level`: Current ellipsis nesting level (0 = not in ellipsis)
    pub fn compile_pattern(&mut self, form: &Value, level: usize) -> Result<Pattern, MacroError> {
        // Handle all identifier types (Symbol, ScopedIdentifier, WrappedIdentifier)
        // This is needed for nested macro definitions where identifiers may be wrapped.
        if let Some(s) = Self::extract_symbol_name(form) {
            // R7RS: Check literals FIRST (including _ if it's in the literals list)
            if self.is_literal(s) {
                // Literal identifier (including _ if explicitly listed)
                // Use the original form to preserve identifier type
                Ok(Pattern::Literal(form.clone()))
            } else if s.as_ref() == "_" {
                // Underscore is wildcard only if NOT in literals
                Ok(Pattern::Wildcard)
            } else {
                // Pattern variable - assign PVREF
                let pvref = self.add_pvar(s.clone(), level)?;
                Ok(Pattern::Var(pvref))
            }
        } else {
            match form {
                // List - check for ellipsis and dotted tails
                Value::Pair(_) => {
                    let (items, tail) = self.collect_list_items(form)?;
                    if let Some(tail_value) = tail {
                        // Dotted list pattern
                        self.compile_dotted_pattern(&items, &tail_value, level)
                    } else {
                        // Regular list pattern
                        self.compile_list_pattern(&items, level)
                    }
                }

                Value::Null => Ok(Pattern::List(vec![])),

                // Vector
                Value::Vector(v) => {
                    let items = v.borrow();
                    let mut patterns = Vec::new();
                    for item in items.iter() {
                        patterns.push(self.compile_pattern(item, level)?);
                    }
                    Ok(Pattern::Vector(patterns))
                }

                // Literal value (boolean, number, string, character, etc.)
                other => Ok(Pattern::Literal(other.clone())),
            }
        }
    }

    /// Compile a list pattern, detecting ellipsis
    ///
    /// This is where the magic happens - we detect ellipsis patterns
    /// and precompute the num_following optimization.
    fn compile_list_pattern(
        &mut self,
        items: &[Value],
        level: usize,
    ) -> Result<Pattern, MacroError> {
        let mut patterns = Vec::new();
        let mut i = 0;

        while i < items.len() {
            // Check for ellipsis
            if i + 1 < items.len() && self.is_ellipsis(&items[i + 1]) {
                // Found ellipsis pattern: (item ...)

                // Count trailing items - Gauche's num_following optimization!
                // (macro.c:138-145)
                let num_following = items.len() - i - 2;

                // Track which pattern variables are introduced in subpattern
                let start_pvars = self.pvar_count;

                // Compile the subpattern at increased level
                let subpattern = self.compile_pattern(&items[i], level + 1)?;

                let end_pvars = self.pvar_count;

                // Collect PVREFs for variables introduced in this subpattern
                let mut vars = Vec::new();
                for idx in start_pvars..end_pvars {
                    vars.push(PVRef::new((level + 1) as u8, idx as u8));
                }

                // Update max level
                self.max_level = self.max_level.max(level + 1);

                patterns.push(Pattern::Ellipsis {
                    subpattern: Box::new(subpattern),
                    level: (level + 1) as u8,
                    num_following,
                    vars,
                });

                i += 2; // Skip pattern and ellipsis
            } else {
                patterns.push(self.compile_pattern(&items[i], level)?);
                i += 1;
            }
        }

        Ok(Pattern::List(patterns))
    }

    /// Compile a dotted list pattern: (a b . rest)
    /// Also handles patterns with ellipsis: (a b ... . rest)
    fn compile_dotted_pattern(
        &mut self,
        items: &[Value],
        tail: &Value,
        level: usize,
    ) -> Result<Pattern, MacroError> {
        let mut patterns = Vec::new();
        let mut i = 0;

        // Process items before the dot, checking for ellipsis
        while i < items.len() {
            // Check for ellipsis
            if i + 1 < items.len() && self.is_ellipsis(&items[i + 1]) {
                // Found ellipsis pattern: (item ...)

                // Count trailing items before the dot
                let num_following = items.len() - i - 2;

                // Track which pattern variables are introduced in subpattern
                let start_pvars = self.pvar_count;

                // Compile the subpattern at increased level
                let subpattern = self.compile_pattern(&items[i], level + 1)?;

                let end_pvars = self.pvar_count;

                // Collect PVREFs for variables introduced in this subpattern
                let mut vars = Vec::new();
                for idx in start_pvars..end_pvars {
                    vars.push(PVRef::new((level + 1) as u8, idx as u8));
                }

                // Update max level
                self.max_level = self.max_level.max(level + 1);

                patterns.push(Pattern::Ellipsis {
                    subpattern: Box::new(subpattern),
                    level: (level + 1) as u8,
                    num_following,
                    vars,
                });

                i += 2; // Skip pattern and ellipsis
            } else {
                patterns.push(self.compile_pattern(&items[i], level)?);
                i += 1;
            }
        }

        let tail_pattern = Box::new(self.compile_pattern(tail, level)?);

        Ok(Pattern::DottedList {
            patterns,
            tail: tail_pattern,
        })
    }

    /// Compile a template at the given ellipsis level
    ///
    /// Based on Gauche's template compilation (macro.c:400+).
    pub fn compile_template(&mut self, form: &Value, level: usize) -> Result<Template, MacroError> {
        // Handle all identifier types (Symbol, ScopedIdentifier, WrappedIdentifier)
        // This is needed for nested macro definitions where identifiers may be wrapped.
        if let Some(s) = Self::extract_symbol_name(form) {
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
                return Ok(Template::Var(*pvref));
            }

            // Not a pattern variable - apply hygiene handling
            // Scope-based hygiene approach (Racket-style):
            // ALL non-pattern-variable identifiers in templates are tagged with definition scopes.
            // The scopes represent the lexical context at macro definition time.
            // At expansion time, the identifier will carry these scopes.
            // At lookup time, we find the binding with matching scope subset.
            //
            // This differs from the old marks-and-ribs approach which only tagged
            // identifiers that were bound at definition time ("free variables").
            // In scope-based hygiene, the scopes ARE the mechanism for hygiene -
            // we don't need to distinguish free vs introduced at compile time.

            if !self.definition_scopes.is_empty() {
                // Scope-based hygiene: tag with definition scopes
                return Ok(Template::Symbol(Identifier::with_scopes(
                    s.clone(),
                    self.definition_scopes.clone(),
                )));
            } else {
                // Fall back to marks-and-ribs hygiene (no scopes available)
                // Check if it's bound in the captured environment at compile time
                let should_capture = self.env.as_ref().is_some_and(|env| env.get(s).is_some());

                if should_capture {
                    // Free variable - use empty scopes (definition-time scopes)
                    return Ok(Template::Symbol(Identifier::with_scopes(
                        s.clone(),
                        ScopeSet::new(),
                    )));
                } else {
                    // Introduced identifier - will get expansion scope via flip-scope
                    return Ok(Template::Symbol(Identifier::new(s.clone())));
                }
            }
        }

        match form {
            // List - check for ellipsis and ellipsis escape
            Value::Pair(_) => {
                let (items, tail) = self.collect_list_items(form)?;

                // Check for quote form: (quote datum)
                // R7RS requires pattern variables to be expanded inside quotes,
                // but literal data without pattern variables should stay literal.
                // For example: (syntax-rules () ((m x) '(x))) should expand (m 5) to '(5)
                // But: (syntax-rules () ((m x) 'ok)) should expand (m 5) to 'ok (not renamed)
                // Note: Check for identifier types to support nested macros
                if items.len() == 2
                    && Self::extract_symbol_name(&items[0])
                        .map(|s| s.as_ref() == "quote")
                        .unwrap_or(false)
                    && tail.is_none()
                {
                    // Check for ellipsis escape inside quote: '(... template)
                    // R7RS: (... template) produces template with ... treated literally
                    // but pattern variables are still substituted.
                    // So '(... ...) should produce '...
                    // And '(... (x ...)) where x is a pvar should produce '(val ...)
                    if let Value::Pair(_) = &items[1]
                        && let Ok((inner_items, None)) = self.collect_list_items(&items[1])
                        && inner_items.len() == 2
                        && self.is_ellipsis(&inner_items[0])
                    {
                        // Ellipsis escape inside quote: '(... template) → '(compiled_template_with_no_ellipsis)
                        // Compile the inner template with ellipsis disabled, using quote compilation
                        // so that symbols become Literal values, not ScopedIdentifiers
                        let inner_template =
                            self.compile_quote_template_escaped(&inner_items[1], level)?;

                        // Wrap in a quote template
                        let quote_symbol = Template::Literal(Value::Symbol("quote".into()));
                        return Ok(Template::List(vec![quote_symbol, inner_template]));
                    }

                    // Check if the quoted datum contains pattern variables
                    if self.contains_pattern_vars(&items[1]) {
                        // Has pattern variables - compile normally so they expand
                        // Fall through to normal list compilation
                    } else {
                        // No pattern variables - treat as literal (no hygiene renaming)
                        return Ok(Template::Literal(form.clone()));
                    }
                }

                // Check for ellipsis escape: (... template)
                if items.len() == 2 && self.is_ellipsis(&items[0]) {
                    // Ellipsis escape - compile inner template with ellipsis disabled
                    return self.compile_with_escaped_ellipsis(&items[1], level);
                }

                if let Some(tail_value) = tail {
                    // Dotted list template
                    self.compile_dotted_template(&items, &tail_value, level)
                } else {
                    // Regular list template
                    self.compile_list_template(&items, level)
                }
            }

            Value::Null => Ok(Template::List(vec![])),

            // Vector
            Value::Vector(v) => {
                let items = v.borrow();
                let mut templates = Vec::new();
                for item in items.iter() {
                    templates.push(self.compile_template(item, level)?);
                }
                Ok(Template::Vector(templates))
            }

            // Literal value
            other => Ok(Template::Literal(other.clone())),
        }
    }

    /// Compile a list template, detecting ellipsis and double ellipsis
    fn compile_list_template(
        &mut self,
        items: &[Value],
        level: usize,
    ) -> Result<Template, MacroError> {
        let mut templates = Vec::new();
        let mut i = 0;

        while i < items.len() {
            // Check for ellipsis
            if i + 1 < items.len() && self.is_ellipsis(&items[i + 1]) {
                // Found ellipsis in template

                // Count consecutive ellipses for double ellipsis support (SRFI-149)
                let mut nesting = 0u8;
                let mut j = i + 1;
                while j < items.len() && self.is_ellipsis(&items[j]) {
                    nesting += 1;
                    j += 1;
                }

                // Compile the base template at deepest level
                let subtemplate = self.compile_template(&items[i], level + nesting as usize)?;

                // Collect variables that should iterate
                let vars = self.collect_template_vars(&subtemplate, level + 1);

                if vars.is_empty() {
                    return Err(MacroError::InvalidSyntax(
                        "Ellipsis in template contains no pattern variables".to_string(),
                    ));
                }

                // Verify variables are at appropriate levels for nesting
                self.verify_ellipsis_nesting(&vars, level, nesting as usize)?;

                templates.push(Template::Ellipsis {
                    subtemplate: Box::new(subtemplate),
                    level: (level + 1) as u8,
                    nesting,
                    vars,
                });

                i = j; // Skip past all ellipses
            } else {
                templates.push(self.compile_template(&items[i], level)?);
                i += 1;
            }
        }

        Ok(Template::List(templates))
    }

    /// Compile a dotted list template: (a b . rest)
    fn compile_dotted_template(
        &mut self,
        items: &[Value],
        tail: &Value,
        level: usize,
    ) -> Result<Template, MacroError> {
        let mut templates = Vec::new();
        for item in items {
            templates.push(self.compile_template(item, level)?);
        }

        let tail_template = Box::new(self.compile_template(tail, level)?);

        Ok(Template::DottedList {
            templates,
            tail: tail_template,
        })
    }

    /// Compile template with ellipsis temporarily disabled
    ///
    /// Used for ellipsis escape: (... template)
    ///
    /// When ellipsis is escaped, `...` symbols should become literal Symbol values,
    /// not ScopedIdentifier/WrappedIdentifier values. This is crucial for nested
    /// macro definitions where the inner `syntax-rules` needs to recognize `...`
    /// as the ellipsis symbol.
    fn compile_with_escaped_ellipsis(
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
                    // Non-ellipsis, non-pvar symbol - use normal hygiene handling
                    if !self.definition_scopes.is_empty() {
                        Ok(Template::Symbol(Identifier::with_scopes(
                            s.clone(),
                            self.definition_scopes.clone(),
                        )))
                    } else {
                        // Fall back to marks-and-ribs hygiene
                        let should_capture =
                            self.env.as_ref().is_some_and(|env| env.get(s).is_some());

                        if should_capture {
                            Ok(Template::Symbol(Identifier::with_scopes(
                                s.clone(),
                                ScopeSet::new(),
                            )))
                        } else {
                            Ok(Template::Symbol(Identifier::new(s.clone())))
                        }
                    }
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
    fn compile_quote_template_escaped(
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

    /// Add a pattern variable and assign it a PVREF
    ///
    /// # Arguments
    /// - `name`: Variable name
    /// - `level`: Ellipsis nesting level
    ///
    /// # Returns
    /// The assigned PVREF
    fn add_pvar(&mut self, name: Rc<str>, level: usize) -> Result<PVRef, MacroError> {
        if self.pvars.contains_key(&name) {
            return Err(MacroError::InvalidSyntax(format!(
                "Duplicate pattern variable: {}",
                name
            )));
        }

        if level > 255 {
            return Err(MacroError::InvalidSyntax(
                "Ellipsis nesting too deep (max 255 levels)".to_string(),
            ));
        }

        if self.pvar_count >= 255 {
            return Err(MacroError::InvalidSyntax(
                "Too many pattern variables (max 255)".to_string(),
            ));
        }

        let pvref = PVRef::new(level as u8, self.pvar_count as u8);
        self.pvars.insert(name, pvref);
        self.pvar_count += 1;

        Ok(pvref)
    }

    /// Check if a symbol is a literal identifier
    fn is_literal(&self, sym: &Rc<str>) -> bool {
        self.literals.contains(sym)
    }

    /// Check if a value is the ellipsis symbol
    ///
    /// Recognizes both plain Symbol and Identifier (with marks/scopes)
    /// to support nested macro definitions where ellipsis may be wrapped.
    fn is_ellipsis(&self, form: &Value) -> bool {
        match &self.ellipsis {
            None => false, // Ellipsis disabled
            Some(elli) => match form {
                Value::Symbol(s) => s == elli,
                Value::Identifier { name, .. } => name == elli,
                _ => false,
            },
        }
    }

    /// Extract symbol name from any identifier type
    ///
    /// Returns Some(name) for Symbol or Identifier.
    /// Returns None for other value types.
    fn extract_symbol_name(form: &Value) -> Option<&Rc<str>> {
        match form {
            Value::Symbol(s) => Some(s),
            Value::Identifier { name, .. } => Some(name),
            _ => None,
        }
    }

    /// Collect items from a list Value
    ///
    /// Returns (items, tail) where tail is Some(value) for improper lists
    fn collect_list_items(&self, expr: &Value) -> Result<(Vec<Value>, Option<Value>), MacroError> {
        let mut items = Vec::new();
        let mut current = expr.clone();

        loop {
            match current {
                Value::Null => return Ok((items, None)),
                Value::Pair(pair) => {
                    let borrowed = pair.borrow();
                    items.push(borrowed.0.clone());
                    current = borrowed.1.clone();
                }
                _ => {
                    // Improper list: (a b . c)
                    return Ok((items, Some(current)));
                }
            }
        }
    }

    /// Collect all pattern variables from a template
    ///
    /// Used to determine which variables need to be iterated during ellipsis expansion.
    fn collect_template_vars(&self, tmpl: &Template, min_level: usize) -> Vec<PVRef> {
        let mut vars = Vec::new();
        Self::collect_vars_rec(tmpl, min_level, &mut vars);
        vars.sort_by_key(|pv| (pv.level(), pv.index()));
        vars.dedup();
        vars
    }

    /// Recursively collect variables from template
    fn collect_vars_rec(tmpl: &Template, min_level: usize, acc: &mut Vec<PVRef>) {
        match tmpl {
            Template::Var(pvref) if pvref.level() >= min_level => {
                acc.push(*pvref);
            }
            Template::List(items) | Template::Vector(items) => {
                for item in items {
                    Self::collect_vars_rec(item, min_level, acc);
                }
            }
            Template::Ellipsis { subtemplate, .. } => {
                Self::collect_vars_rec(subtemplate, min_level, acc);
            }
            Template::DottedList { templates, tail } => {
                for t in templates {
                    Self::collect_vars_rec(t, min_level, acc);
                }
                Self::collect_vars_rec(tail, min_level, acc);
            }
            _ => {}
        }
    }

    /// Verify that template variables are at valid levels for ellipsis nesting
    fn verify_ellipsis_nesting(
        &self,
        vars: &[PVRef],
        base_level: usize,
        nesting: usize,
    ) -> Result<(), MacroError> {
        let innermost_level = base_level + nesting;

        // At least one variable must be at the right level for innermost ellipsis
        let has_valid_var = vars
            .iter()
            .any(|pv| pv.level() > base_level && pv.level() <= innermost_level);

        if !has_valid_var {
            return Err(MacroError::InvalidSyntax(format!(
                "Invalid ellipsis nesting: no variables at levels {}-{} (nesting={})",
                base_level + 1,
                innermost_level,
                nesting
            )));
        }

        Ok(())
    }

    /// Check if a value contains any pattern variables
    /// Used to determine if a quoted expression needs template expansion
    fn contains_pattern_vars(&self, value: &Value) -> bool {
        // Handle all identifier types (Symbol, ScopedIdentifier, WrappedIdentifier)
        if let Some(s) = Self::extract_symbol_name(value) {
            return self.pvars.contains_key(s);
        }

        match value {
            Value::Pair(_) => {
                let items = match self.collect_list_items(value) {
                    Ok((items, _)) => items,
                    Err(_) => return false,
                };
                items.iter().any(|item| self.contains_pattern_vars(item))
            }
            Value::Vector(v) => v
                .borrow()
                .iter()
                .any(|item| self.contains_pattern_vars(item)),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Helper to create a symbol
    fn sym(s: &str) -> Value {
        Value::Symbol(s.into())
    }

    /// Helper to create a list
    fn list(items: Vec<Value>) -> Value {
        items.into_iter().rev().fold(Value::Null, |acc, val| {
            Value::Pair(Rc::new(RefCell::new((val, acc))))
        })
    }

    #[test]
    fn test_compile_simple_pattern() {
        // Pattern: (when test body)
        let mut compiler = Compiler::new(vec![], Some("...".into()));

        let pattern_form = list(vec![sym("when"), sym("test"), sym("body")]);
        let pattern = compiler.compile_pattern(&pattern_form, 0).unwrap();

        match pattern {
            Pattern::List(patterns) => {
                assert_eq!(patterns.len(), 3);
                assert!(matches!(&patterns[0], Pattern::Var(_)));
                assert!(matches!(&patterns[1], Pattern::Var(_)));
                assert!(matches!(&patterns[2], Pattern::Var(_)));
            }
            _ => panic!("Expected Pattern2::List"),
        }

        // Should have 3 pattern variables
        assert_eq!(compiler.pvar_count, 3);
    }

    #[test]
    fn test_compile_pattern_with_ellipsis() {
        // Pattern: (when test body ...)
        let mut compiler = Compiler::new(vec![], Some("...".into()));

        let pattern_form = list(vec![sym("when"), sym("test"), sym("body"), sym("...")]);
        let pattern = compiler.compile_pattern(&pattern_form, 0).unwrap();

        match pattern {
            Pattern::List(patterns) => {
                assert_eq!(patterns.len(), 3);
                // First two are normal vars
                assert!(matches!(&patterns[0], Pattern::Var(_)));
                assert!(matches!(&patterns[1], Pattern::Var(_)));
                // Third is ellipsis
                match &patterns[2] {
                    Pattern::Ellipsis {
                        subpattern,
                        level,
                        num_following,
                        vars,
                    } => {
                        assert_eq!(*level, 1);
                        assert_eq!(*num_following, 0); // No items after ellipsis
                        assert_eq!(vars.len(), 1);
                        assert!(matches!(**subpattern, Pattern::Var(_)));
                    }
                    _ => panic!("Expected Pattern2::Ellipsis"),
                }
            }
            _ => panic!("Expected Pattern2::List"),
        }
    }

    #[test]
    fn test_compile_pattern_ellipsis_with_following() {
        // Pattern: (do bindings ... (test result))
        // The ellipsis should have num_following = 1
        let mut compiler = Compiler::new(vec![], Some("...".into()));

        let pattern_form = list(vec![
            sym("do"),
            sym("bindings"),
            sym("..."),
            list(vec![sym("test"), sym("result")]),
        ]);
        let pattern = compiler.compile_pattern(&pattern_form, 0).unwrap();

        match pattern {
            Pattern::List(patterns) => {
                assert_eq!(patterns.len(), 3); // do, bindings..., (test result)
                match &patterns[1] {
                    Pattern::Ellipsis { num_following, .. } => {
                        assert_eq!(*num_following, 1); // One item follows
                    }
                    _ => panic!("Expected ellipsis"),
                }
            }
            _ => panic!("Expected list"),
        }
    }

    #[test]
    fn test_compile_simple_template() {
        // First compile a pattern to establish variables
        let mut compiler = Compiler::new(vec![], Some("...".into()));
        let _pattern = compiler
            .compile_pattern(&list(vec![sym("when"), sym("test"), sym("body")]), 0)
            .unwrap();

        // Now compile template: (if test body)
        let template_form = list(vec![sym("if"), sym("test"), sym("body")]);
        let template = compiler.compile_template(&template_form, 0).unwrap();

        match template {
            Template::List(templates) => {
                assert_eq!(templates.len(), 3);
                // "if" is introduced symbol
                assert!(matches!(&templates[0], Template::Symbol(_)));
                // "test" and "body" are pattern variables
                assert!(matches!(&templates[1], Template::Var(_)));
                assert!(matches!(&templates[2], Template::Var(_)));
            }
            _ => panic!("Expected Template2::List"),
        }
    }

    #[test]
    fn test_compile_template_with_ellipsis() {
        // Pattern: (begin body ...)
        let mut compiler = Compiler::new(vec![], Some("...".into()));
        let _pattern = compiler
            .compile_pattern(&list(vec![sym("begin"), sym("body"), sym("...")]), 0)
            .unwrap();

        // Template: (lambda () body ...)
        let template_form = list(vec![sym("lambda"), Value::Null, sym("body"), sym("...")]);
        let template = compiler.compile_template(&template_form, 0).unwrap();

        match template {
            Template::List(templates) => {
                assert_eq!(templates.len(), 3); // lambda, (), (body ...)
                match &templates[2] {
                    Template::Ellipsis {
                        subtemplate,
                        level,
                        nesting,
                        vars,
                    } => {
                        assert_eq!(*level, 1);
                        assert_eq!(*nesting, 1);
                        assert_eq!(vars.len(), 1);
                        assert!(matches!(**subtemplate, Template::Var(_)));
                    }
                    _ => panic!("Expected ellipsis"),
                }
            }
            _ => panic!("Expected list"),
        }
    }

    #[test]
    fn test_compile_with_literals() {
        // Pattern with literal "else"
        let mut compiler = Compiler::new(vec!["else".into()], Some("...".into()));

        let pattern_form = list(vec![sym("cond"), sym("else"), sym("body")]);
        let pattern = compiler.compile_pattern(&pattern_form, 0).unwrap();

        match pattern {
            Pattern::List(patterns) => {
                assert_eq!(patterns.len(), 3);
                // "cond" is a variable
                assert!(matches!(&patterns[0], Pattern::Var(_)));
                // "else" is a literal
                assert!(matches!(&patterns[1], Pattern::Literal(_)));
                // "body" is a variable
                assert!(matches!(&patterns[2], Pattern::Var(_)));
            }
            _ => panic!("Expected list"),
        }
    }

    #[test]
    fn test_compile_full_macro() {
        // Compile a complete when macro
        let mut compiler = Compiler::new(vec![], Some("...".into()));

        let pattern = list(vec![sym("when"), sym("test"), sym("body"), sym("...")]);
        let template = list(vec![
            sym("if"),
            sym("test"),
            list(vec![sym("begin"), sym("body"), sym("...")]),
        ]);

        let compiled = compiler
            .compile_macro("when".into(), vec![(pattern, template)])
            .unwrap();

        assert_eq!(compiled.name.as_ref(), "when");
        assert_eq!(compiled.rules.len(), 1);
        // R7RS: First element "when" is the macro keyword placeholder (compiled as wildcard)
        // Only "test" and "body" are actual pattern variables
        assert_eq!(compiled.rules[0].num_pvars, 2); // test and body (not when!)
        assert_eq!(compiled.rules[0].max_level, 1); // body is at level 1
    }

    #[test]
    fn test_error_duplicate_pattern_var() {
        // Pattern: (test test) - duplicate variable
        let mut compiler = Compiler::new(vec![], Some("...".into()));

        let pattern_form = list(vec![sym("test"), sym("test")]);
        let result = compiler.compile_pattern(&pattern_form, 0);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Duplicate pattern variable")
        );
    }

    #[test]
    fn test_var_level_validation() {
        // Pattern: (foo x ...)
        // This establishes 'foo' at level 0 and 'x' at level 1
        let mut compiler = Compiler::new(vec![], Some("...".into()));
        let pattern_form = list(vec![sym("foo"), sym("x"), sym("...")]);
        let _pattern = compiler.compile_pattern(&pattern_form, 0).unwrap();

        // Using 'foo' (level 0) at level 0 in template should work
        let template1 = sym("foo");
        assert!(compiler.compile_template(&template1, 0).is_ok());

        // Using 'x' (level 1) in an ellipsis (level 1) should work
        let template2 = list(vec![sym("x"), sym("...")]);
        assert!(compiler.compile_template(&template2, 0).is_ok());

        // Using 'x' (level 1) at level 0 should fail - wrong level
        let template3 = sym("x");
        let result = compiler.compile_template(&template3, 0);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("at level 1 used at level 0")
        );
    }

    #[test]
    fn test_underscore_as_wildcard() {
        // When _ is NOT in literals, it should be Wildcard
        let mut compiler = Compiler::new(vec![], Some("...".into()));

        let pattern_form = list(vec![sym("foo"), sym("_"), sym("bar")]);
        let pattern = compiler.compile_pattern(&pattern_form, 0).unwrap();

        match pattern {
            Pattern::List(patterns) => {
                assert_eq!(patterns.len(), 3);
                // "foo" is a pattern variable
                assert!(matches!(&patterns[0], Pattern::Var(_)));
                // "_" is a wildcard
                assert!(matches!(&patterns[1], Pattern::Wildcard));
                // "bar" is a pattern variable
                assert!(matches!(&patterns[2], Pattern::Var(_)));
            }
            _ => panic!("Expected list"),
        }
    }

    #[test]
    fn test_underscore_as_literal() {
        // When _ IS in literals, it should be Literal, not Wildcard
        let mut compiler = Compiler::new(vec!["_".into()], Some("...".into()));

        let pattern_form = list(vec![sym("foo"), sym("_"), sym("bar")]);
        let pattern = compiler.compile_pattern(&pattern_form, 0).unwrap();

        match pattern {
            Pattern::List(patterns) => {
                assert_eq!(patterns.len(), 3);
                // "foo" is a pattern variable
                assert!(matches!(&patterns[0], Pattern::Var(_)));
                // "_" is a LITERAL (not wildcard!)
                assert!(
                    matches!(&patterns[1], Pattern::Literal(Value::Symbol(s)) if s.as_ref() == "_"),
                    "Expected Literal(_), got {:?}",
                    patterns[1]
                );
                // "bar" is a pattern variable
                assert!(matches!(&patterns[2], Pattern::Var(_)));
            }
            _ => panic!("Expected list"),
        }
    }
}
