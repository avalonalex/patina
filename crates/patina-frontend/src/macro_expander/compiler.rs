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
use crate::error::FrontendError;
use patina_runtime::{PVRef, Value};
use std::collections::HashMap;
use std::rc::Rc;

/// Compiled macro rule
///
/// Contains both pattern and template in PVREF-based representation,
/// along with metadata for efficient matching and expansion.
#[derive(Clone, Debug)]
pub struct CompiledRule {
    /// Compiled pattern
    pub pattern: Pattern,

    /// Compiled template
    pub template: Template,

    /// Number of pattern variables in this rule
    pub num_pvars: usize,

    /// Maximum ellipsis nesting level in this rule
    pub max_level: usize,

    /// Mapping from pattern variable names to their PVREFs (for debug output)
    pub pvar_names: HashMap<PVRef, Rc<str>>,
}

/// Compiled macro definition
///
/// Contains all rules for a syntax-rules macro in compiled form.
#[derive(Clone, Debug)]
pub struct CompiledMacro {
    /// Macro name (for error messages)
    pub name: Rc<str>,

    /// Literal identifiers (e.g., "else" in cond)
    pub literals: Vec<Rc<str>>,

    /// Compiled rules (tried in order, first-match-wins)
    pub rules: Vec<CompiledRule>,

    /// Maximum number of pattern variables in any rule
    pub max_pvars: usize,
}

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
    ) -> Result<CompiledMacro, FrontendError> {
        let mut compiled_rules = Vec::new();
        let mut max_pvars = 0;

        for (pat_form, tmpl_form) in rules {
            // Reset per-rule context
            self.pvars.clear();
            self.pvar_count = 0;
            self.max_level = 0;

            let pattern = self.compile_pattern(&pat_form, 0)?;
            let template = self.compile_template(&tmpl_form, 0)?;

            // Build reverse mapping: PVREF -> name (for debug output)
            let pvar_names: HashMap<PVRef, Rc<str>> = self
                .pvars
                .iter()
                .map(|(name, pvref)| (*pvref, name.clone()))
                .collect();

            // Validate the rule before adding it
            if let Err(e) = super::validator::validate_rule(&pattern, &template, &pvar_names) {
                return Err(FrontendError::MacroError(format!(
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
        })
    }

    /// Compile a pattern at the given ellipsis level
    ///
    /// Based on Gauche's compile_rule1 (macro.c:400+).
    ///
    /// # Arguments
    /// - `form`: S-expression representing the pattern
    /// - `level`: Current ellipsis nesting level (0 = not in ellipsis)
    pub fn compile_pattern(
        &mut self,
        form: &Value,
        level: usize,
    ) -> Result<Pattern, FrontendError> {
        match form {
            // Underscore wildcard
            Value::Symbol(s) if s.as_ref() == "_" => Ok(Pattern::Wildcard),

            // Symbol - could be literal or pattern variable
            Value::Symbol(s) => {
                if self.is_literal(s) {
                    // Literal identifier
                    Ok(Pattern::Literal(form.clone()))
                } else {
                    // Pattern variable - assign PVREF
                    let pvref = self.add_pvar(s.clone(), level)?;
                    Ok(Pattern::Var(pvref))
                }
            }

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

    /// Compile a list pattern, detecting ellipsis
    ///
    /// This is where the magic happens - we detect ellipsis patterns
    /// and precompute the num_following optimization.
    fn compile_list_pattern(
        &mut self,
        items: &[Value],
        level: usize,
    ) -> Result<Pattern, FrontendError> {
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
    ) -> Result<Pattern, FrontendError> {
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
    pub fn compile_template(
        &mut self,
        form: &Value,
        level: usize,
    ) -> Result<Template, FrontendError> {
        match form {
            Value::Symbol(s) => {
                // Check if it's a pattern variable
                if let Some(pvref) = self.pvars.get(s) {
                    // Verify level is valid
                    if pvref.level() > level {
                        return Err(FrontendError::InvalidSyntax(format!(
                            "Pattern variable {} at level {} used at level {}",
                            s,
                            pvref.level(),
                            level
                        )));
                    }
                    Ok(Template::Var(*pvref))
                } else {
                    // Introduced symbol (will be renamed for hygiene)
                    Ok(Template::Symbol(Identifier::new(s.clone())))
                }
            }

            // List - check for ellipsis and ellipsis escape
            Value::Pair(_) => {
                let (items, tail) = self.collect_list_items(form)?;

                // Check for ellipsis escape: (... template)
                if items.len() == 2
                    && self.ellipsis.is_some()
                    && matches!(&items[0], Value::Symbol(s) if Some(s) == self.ellipsis.as_ref())
                {
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
    ) -> Result<Template, FrontendError> {
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
                    return Err(FrontendError::InvalidSyntax(
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
    ) -> Result<Template, FrontendError> {
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
    fn compile_with_escaped_ellipsis(
        &mut self,
        form: &Value,
        level: usize,
    ) -> Result<Template, FrontendError> {
        // Save current ellipsis setting
        let saved_ellipsis = self.ellipsis.take();

        // Compile with ellipsis disabled
        let result = self.compile_template(form, level);

        // Restore ellipsis setting
        self.ellipsis = saved_ellipsis;

        result
    }

    /// Add a pattern variable and assign it a PVREF
    ///
    /// # Arguments
    /// - `name`: Variable name
    /// - `level`: Ellipsis nesting level
    ///
    /// # Returns
    /// The assigned PVREF
    fn add_pvar(&mut self, name: Rc<str>, level: usize) -> Result<PVRef, FrontendError> {
        if self.pvars.contains_key(&name) {
            return Err(FrontendError::InvalidSyntax(format!(
                "Duplicate pattern variable: {}",
                name
            )));
        }

        if level > 255 {
            return Err(FrontendError::InvalidSyntax(
                "Ellipsis nesting too deep (max 255 levels)".to_string(),
            ));
        }

        if self.pvar_count >= 255 {
            return Err(FrontendError::InvalidSyntax(
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
    fn is_ellipsis(&self, form: &Value) -> bool {
        match (&self.ellipsis, form) {
            (None, _) => false, // Ellipsis disabled
            (Some(elli), Value::Symbol(s)) => s == elli,
            _ => false,
        }
    }

    /// Collect items from a list Value
    ///
    /// Returns (items, tail) where tail is Some(value) for improper lists
    fn collect_list_items(
        &self,
        expr: &Value,
    ) -> Result<(Vec<Value>, Option<Value>), FrontendError> {
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
    ) -> Result<(), FrontendError> {
        let innermost_level = base_level + nesting;

        // At least one variable must be at the right level for innermost ellipsis
        let has_valid_var = vars
            .iter()
            .any(|pv| pv.level() > base_level && pv.level() <= innermost_level);

        if !has_valid_var {
            return Err(FrontendError::InvalidSyntax(format!(
                "Invalid ellipsis nesting: no variables at levels {}-{} (nesting={})",
                base_level + 1,
                innermost_level,
                nesting
            )));
        }

        Ok(())
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
        assert_eq!(compiled.rules[0].num_pvars, 3); // when, test and body
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
}
