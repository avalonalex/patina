//! PVREF-based template expansion (Version 2)
//!
//! This module implements template expansion for the PVREF-based macro system.
//! It takes compiled Template2 structures and MatchEnv trees from pattern matching,
//! expanding them into output expressions.
//!
//! Key improvements over the original expander:
//! - Uses PVREF encoding for O(1) variable lookup
//! - Uses MatchEnv tree navigation for nested ellipsis
//! - Supports double ellipsis (SRFI-149)
//! - Proper hygiene through Identifier renaming
//!
//! Inspired by Gauche's template expansion (macro.c:800+)
//! Reference: https://github.com/shirok/Gauche/blob/master/src/macro.c

use crate::macro_expander::template::{Identifier, Template};
use patina_runtime::{MatchEnv, MatchValue, PVRef, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Error type for template expansion failures
#[derive(Debug, Clone, PartialEq)]
pub enum ExpandError {
    /// Undefined pattern variable referenced in template
    UndefinedVariable { pvref: String },

    /// Variable used at wrong ellipsis level
    LevelMismatch {
        pvref: String,
        template_level: usize,
        actual_level: usize,
    },

    /// Ellipsis iteration with inconsistent repetition counts
    InconsistentRepetition { expected: usize, actual: usize },

    /// Invalid template structure
    InvalidTemplate { message: String },
}

impl std::fmt::Display for ExpandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExpandError::UndefinedVariable { pvref } => {
                write!(
                    f,
                    "Template expansion failed: undefined pattern variable: {}\n\
                     Hint: This variable was not bound during pattern matching. \
                     Check that the variable appears in the macro pattern",
                    pvref
                )
            }
            ExpandError::LevelMismatch {
                pvref,
                template_level,
                actual_level,
            } => {
                write!(
                    f,
                    "Template expansion failed: ellipsis level mismatch\n\
                     Variable:       {}\n\
                     Template level: {}\n\
                     Actual level:   {}\n\
                     Hint: Variable must be used at the same ellipsis nesting depth where it was bound",
                    pvref, template_level, actual_level
                )
            }
            ExpandError::InconsistentRepetition { expected, actual } => {
                write!(
                    f,
                    "Template expansion failed: inconsistent ellipsis repetition\n\
                     Expected: {} iteration(s)\n\
                     Got:      {} iteration(s)\n\
                     Hint: All variables in the same ellipsis template must have the same repetition count",
                    expected, actual
                )
            }
            ExpandError::InvalidTemplate { message } => {
                write!(
                    f,
                    "Template expansion failed: invalid template structure\n\
                     Error: {}\n\
                     Hint: Check the macro template syntax",
                    message
                )
            }
        }
    }
}

impl std::error::Error for ExpandError {}

/// Template expander for PVREF-based macro system
///
/// This implements the template expansion phase of macro expansion.
/// It takes a compiled Template2 and a MatchEnv with pattern variable bindings,
/// returning the expanded output expression.
///
/// Based on Gauche's template expansion approach (macro.c:800+).
pub struct Expander {
    /// Expansion-time environment (for checking if identifiers are macro names)
    /// We use the full runtime Environment directly to avoid duplication
    expansion_env: std::rc::Rc<patina_runtime::Environment>,

    /// Macro scope for this expansion (Racket-style hygiene)
    /// Used to distinguish use-site vs introduced identifiers
    macro_scope: patina_runtime::ScopeId,
}

impl Expander {
    /// Create a new expander with runtime environment and macro scope
    ///
    /// # Arguments
    /// * `expansion_env` - Runtime environment at expansion time
    /// * `macro_scope` - Fresh scope for this macro expansion (for Racket-style hygiene)
    pub fn new(
        expansion_env: std::rc::Rc<patina_runtime::Environment>,
        macro_scope: patina_runtime::ScopeId,
    ) -> Self {
        Self {
            expansion_env,
            macro_scope,
        }
    }

    /// Expand a template with the given match environment
    ///
    /// This is the main entry point for template expansion.
    ///
    /// Inspired by Gauche's expand_template (macro.c:800+)
    pub fn expand(&self, template: &Template, env: &MatchEnv) -> Result<Value, ExpandError> {
        self.expand_impl(template, env, &[])
    }

    /// Internal expansion implementation with indices
    ///
    /// # Arguments
    /// * `template` - The template to expand
    /// * `env` - The match environment with variable bindings
    /// * `indices` - Current ellipsis indices for navigation
    ///
    /// Based on Gauche's expand_rec (macro.c:820+)
    fn expand_impl(
        &self,
        template: &Template,
        env: &MatchEnv,
        indices: &[usize],
    ) -> Result<Value, ExpandError> {
        match template {
            Template::Literal(value) => {
                // Literal values are inserted as-is
                Ok(value.clone())
            }

            Template::Symbol(id) => {
                // Symbols are renamed for hygiene
                Ok(self.rename_identifier(id))
            }

            Template::Var(pvref) => {
                // Look up pattern variable in match environment
                // Use indices to navigate the tree
                match env.get(*pvref, indices) {
                    Some(value) => Ok(value),
                    None => Err(ExpandError::UndefinedVariable {
                        pvref: format!("{:?}", pvref),
                    }),
                }
            }

            Template::List(templates) => {
                // Expand list template: (t1 t2 t3)
                self.expand_list(templates, env, indices)
            }

            Template::Vector(templates) => {
                // Expand vector template: #(t1 t2 t3)
                self.expand_vector(templates, env, indices)
            }

            Template::DottedList { templates, tail } => {
                // Expand dotted list template: (t1 t2 . rest)
                self.expand_dotted_list(templates, tail, env, indices)
            }

            Template::Ellipsis {
                subtemplate,
                level,
                nesting,
                vars,
            } => {
                // Expand ellipsis template: (t ...)
                self.expand_ellipsis(subtemplate, *level, *nesting, vars, env, indices)
            }
        }
    }

    /// Expand a list template
    fn expand_list(
        &self,
        templates: &[Template],
        env: &MatchEnv,
        indices: &[usize],
    ) -> Result<Value, ExpandError> {
        let mut result = Vec::new();

        for template in templates {
            if template.is_ellipsis() {
                // Handle ellipsis specially - it expands to multiple elements
                if let Template::Ellipsis {
                    subtemplate,
                    level,
                    nesting,
                    vars,
                } = template
                {
                    let expanded =
                        self.expand_ellipsis(subtemplate, *level, *nesting, vars, env, indices)?;

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
                let value = self.expand_impl(template, env, indices)?;
                result.push(value);
            }
        }

        // Convert vec back to Scheme list
        Ok(self.vec_to_list(result))
    }

    /// Expand a vector template
    fn expand_vector(
        &self,
        templates: &[Template],
        env: &MatchEnv,
        indices: &[usize],
    ) -> Result<Value, ExpandError> {
        let mut result = Vec::new();

        for template in templates {
            let value = self.expand_impl(template, env, indices)?;
            result.push(value);
        }

        Ok(Value::Vector(Rc::new(std::cell::RefCell::new(result))))
    }

    /// Expand a dotted list template: (t1 t2 . rest)
    fn expand_dotted_list(
        &self,
        templates: &[Template],
        tail: &Template,
        env: &MatchEnv,
        indices: &[usize],
    ) -> Result<Value, ExpandError> {
        // Expand the fixed part
        let mut items = Vec::new();
        for template in templates {
            let value = self.expand_impl(template, env, indices)?;
            items.push(value);
        }

        // Expand the tail
        let tail_value = self.expand_impl(tail, env, indices)?;

        // Build dotted list
        let mut result = tail_value;
        for item in items.into_iter().rev() {
            result = Value::Pair(Rc::new(RefCell::new((item, result))));
        }

        Ok(result)
    }

    /// Expand an ellipsis template
    ///
    /// This is the most complex case, handling iteration over pattern variable bindings.
    ///
    /// Based on Gauche's ellipsis expansion (macro.c:850+)
    fn expand_ellipsis(
        &self,
        subtemplate: &Template,
        level: u8,
        nesting: u8,
        vars: &[PVRef],
        env: &MatchEnv,
        indices: &[usize],
    ) -> Result<Value, ExpandError> {
        if nesting == 1 {
            // Single ellipsis - standard iteration
            self.expand_single_ellipsis(subtemplate, level, vars, env, indices)
        } else if nesting == 2 {
            // Double ellipsis (SRFI-149)
            // Template: x ... ...
            // This expands x at each level, then flattens one level
            self.expand_double_ellipsis(subtemplate, level, vars, env, indices)
        } else {
            // Triple+ ellipsis not supported
            Err(ExpandError::InvalidTemplate {
                message: format!("Ellipsis nesting level {} not supported (max 2)", nesting),
            })
        }
    }

    /// Expand a single ellipsis template (nesting = 1)
    fn expand_single_ellipsis(
        &self,
        subtemplate: &Template,
        level: u8,
        vars: &[PVRef],
        env: &MatchEnv,
        indices: &[usize],
    ) -> Result<Value, ExpandError> {
        // Determine iteration count from the first variable
        let iteration_count = if let Some(&first_var) = vars.first() {
            self.get_iteration_count(env, first_var, indices)?
        } else {
            // No variables - ellipsis matches empty
            0
        };

        // Validate that all variables have the same iteration count
        for &var in vars.iter().skip(1) {
            let count = self.get_iteration_count(env, var, indices)?;
            if count != iteration_count {
                return Err(ExpandError::InconsistentRepetition {
                    expected: iteration_count,
                    actual: count,
                });
            }
        }

        // Expand subtemplate for each iteration
        let mut result = Vec::new();
        for i in 0..iteration_count {
            // Build new indices array with this iteration index
            let mut new_indices = indices.to_vec();
            // Ensure indices is large enough
            while new_indices.len() <= level as usize {
                new_indices.push(0);
            }
            new_indices[level as usize] = i;

            let value = self.expand_impl(subtemplate, env, &new_indices)?;
            result.push(value);
        }

        // Convert result vec to Scheme list
        Ok(self.vec_to_list(result))
    }

    /// Expand a double ellipsis template (nesting = 2)
    ///
    /// Double ellipsis `x ... ...` means:
    /// 1. Expand x at the inner level (level) for each iteration
    /// 2. Then expand at the outer level (level-1) and flatten
    ///
    /// Example from SRFI-46:
    /// Pattern: ((a b ...) ...)
    /// Template: ((a b ... ...) ...)
    /// Input: ((1 2 3) (4 5))
    /// Output: ((1 2 3) (4 5))
    ///
    /// For the do macro:
    /// Pattern: ((var init step ...) ...)
    /// Template: (loop step ... ...)
    /// Input: ((i 0 (+ i 1)) (j 10))
    /// step values: Branch([Branch([Leaf((+ i 1))]), Branch([])])
    /// Output: (loop (+ i 1) j)
    fn expand_double_ellipsis(
        &self,
        subtemplate: &Template,
        level: u8,
        vars: &[PVRef],
        env: &MatchEnv,
        indices: &[usize],
    ) -> Result<Value, ExpandError> {
        // For double ellipsis, we iterate at the outer level (level - 1)
        // and for each iteration, we expand the inner ellipsis

        if level == 0 {
            return Err(ExpandError::InvalidTemplate {
                message: "Double ellipsis requires level >= 1".to_string(),
            });
        }

        // Get iteration count at outer level
        // For double ellipsis, we iterate first at the ellipsis level (which is the variable's parent level)
        let outer_count = if let Some(&first_var) = vars.first() {
            // The variable should be at level >= 2 for double ellipsis
            // We want to iterate at the ellipsis level (the variable's parent level)
            self.get_iteration_count_at_level(env, first_var, indices, (level - 1) as usize)?
        } else {
            0
        };

        // For each outer iteration, expand the inner ellipsis and flatten
        let mut all_results = Vec::new();

        for outer_idx in 0..outer_count {
            // Build indices for this outer iteration
            let mut outer_indices = indices.to_vec();
            // Ensure outer_indices is long enough for the variable's level
            // For a level-2 variable, we need indices[0] and indices[1]
            let max_var_level = vars.iter().map(|v| v.level()).max().unwrap_or(0);
            while outer_indices.len() <= max_var_level {
                outer_indices.push(0);
            }
            // Set the index at the ellipsis level (outer iteration)
            outer_indices[level as usize] = outer_idx;

            // Get inner iteration count at this outer index
            let inner_count = if let Some(&first_var) = vars.first() {
                self.get_iteration_count(env, first_var, &outer_indices)?
            } else {
                0
            };

            // Expand inner ellipsis
            for inner_idx in 0..inner_count {
                let mut inner_indices = outer_indices.clone();
                // For double ellipsis with level-2 variables, we need indices[2]
                // So ensure indices has at least (level + 1) elements
                // Actually, we need enough space for the VARIABLE's level, not just ellipsis level
                // For step ... ... where step is level 2, we need indices[0], [1], [2]
                let max_var_level = vars.iter().map(|v| v.level()).max().unwrap_or(0);
                while inner_indices.len() <= max_var_level {
                    inner_indices.push(0);
                }
                // Set the index at the variable's level (inner iteration)
                // For double ellipsis with level=1 and var level=2, we set indices[2]
                let var_level = vars
                    .first()
                    .map(|v| v.level())
                    .unwrap_or((level + 1) as usize);
                inner_indices[var_level] = inner_idx;

                let value = self.expand_impl(subtemplate, env, &inner_indices)?;
                all_results.push(value);
            }
        }

        // Convert flattened results to list
        Ok(self.vec_to_list(all_results))
    }

    /// Get iteration count at a specific level
    fn get_iteration_count_at_level(
        &self,
        env: &MatchEnv,
        pvref: PVRef,
        indices: &[usize],
        target_level: usize,
    ) -> Result<usize, ExpandError> {
        let match_value = env
            .get_raw(pvref)
            .ok_or_else(|| ExpandError::UndefinedVariable {
                pvref: format!("{:?}", pvref),
            })?;

        // Navigate to target level
        let mut current = match_value;
        for (depth, &idx) in indices.iter().enumerate().skip(1) {
            if depth > target_level {
                break;
            }
            match current {
                MatchValue::Branch(items) => {
                    current = items
                        .get(idx)
                        .ok_or_else(|| ExpandError::UndefinedVariable {
                            pvref: format!("{:?}", pvref),
                        })?;
                }
                MatchValue::Leaf(_) => {
                    return Err(ExpandError::LevelMismatch {
                        pvref: format!("{:?}", pvref),
                        template_level: target_level,
                        actual_level: depth - 1,
                    });
                }
            }
        }

        // Current should be a Branch at target_level
        match current {
            MatchValue::Branch(items) => Ok(items.len()),
            MatchValue::Leaf(_) => Err(ExpandError::LevelMismatch {
                pvref: format!("{:?}", pvref),
                template_level: target_level,
                actual_level: 0,
            }),
        }
    }

    /// Get the iteration count for a variable at the current ellipsis level
    ///
    /// This looks at the variable's bindings and determines how many times
    /// we need to iterate at this level.
    fn get_iteration_count(
        &self,
        env: &MatchEnv,
        pvref: PVRef,
        indices: &[usize],
    ) -> Result<usize, ExpandError> {
        // Get the raw match value
        let match_value = env
            .get_raw(pvref)
            .ok_or_else(|| ExpandError::UndefinedVariable {
                pvref: format!("{:?}", pvref),
            })?;

        // Navigate to the appropriate level using indices
        // For a level-N variable, we need to navigate N-1 times using indices[1] through indices[N-1]
        // Example: level-2 variable with indices=[_, 0] navigates once using indices[1]=0
        let mut current = match_value;
        for level_idx in 1..pvref.level() {
            if level_idx >= indices.len() {
                return Err(ExpandError::UndefinedVariable {
                    pvref: format!("{:?}", pvref),
                });
            }
            let idx = indices[level_idx];
            match current {
                MatchValue::Branch(items) => {
                    current = items
                        .get(idx)
                        .ok_or_else(|| ExpandError::UndefinedVariable {
                            pvref: format!("{:?}", pvref),
                        })?;
                }
                MatchValue::Leaf(_) => {
                    return Err(ExpandError::LevelMismatch {
                        pvref: format!("{:?}", pvref),
                        template_level: pvref.level(),
                        actual_level: 0,
                    });
                }
            }
        }

        // Now current should be at the level just above where we're iterating
        // It should be a Branch
        match current {
            MatchValue::Branch(items) => Ok(items.len()),
            MatchValue::Leaf(_) => Err(ExpandError::LevelMismatch {
                pvref: format!("{:?}", pvref),
                template_level: pvref.level(),
                actual_level: 0,
            }),
        }
    }

    /// Rename an identifier for hygiene using Racket-style scope sets
    ///
    /// All identifiers become `Identifier` with appropriate scopes:
    /// - Free variables: use definition scopes (for binding resolution)
    /// - Introduced identifiers: empty scopes (macro_scope will be added by flip on output)
    ///
    /// The actual hygiene discrimination happens via flip-scope:
    /// 1. Before expansion: flip macro_scope on INPUT (adds to use-site identifiers)
    /// 2. Template symbols get their definition_scopes here
    /// 3. After expansion: flip macro_scope on OUTPUT
    ///    - Use-site (from pattern vars): macro_scope removed (was added, then flipped off)
    ///    - Introduced (from template): macro_scope added (wasn't there, then flipped on)
    fn rename_identifier(&self, id: &Identifier) -> Value {
        let name = id.name();

        // Special forms and macros are never renamed
        if is_special_form(name.as_ref()) || self.is_macro(name) {
            return Value::Symbol(name.clone());
        }

        // Get scopes for the identifier
        let scopes = if let Some(def_scopes) = id.definition_scopes() {
            // FREE VARIABLE - use definition-time scopes
            if patina_runtime::macro_debug::is_enabled() {
                println!(
                    "[SCOPE-SETS] Free variable '{}' with scopes {}",
                    name, def_scopes
                );
            }
            def_scopes.clone()
        } else {
            // INTRODUCED IDENTIFIER - empty scopes for now
            // The macro_scope will be added when we flip on the output
            if patina_runtime::macro_debug::is_enabled() {
                println!(
                    "[SCOPE-SETS] Introduced '{}' (will get macro scope {} on output flip)",
                    name, self.macro_scope
                );
            }
            patina_runtime::ScopeSet::new()
        };

        // Return Identifier with scopes (Racket-style hygiene)
        Value::Identifier {
            name: name.clone(),
            scopes,
        }
    }

    /// Check if a name is bound to a macro in the expansion environment
    fn is_macro(&self, name: &Rc<str>) -> bool {
        use patina_runtime::Value;
        matches!(self.expansion_env.get(name), Some(Value::Macro { .. }))
    }

    /// Convert a Scheme list to a Vec
    fn list_to_vec(&self, value: &Value) -> Result<Vec<Value>, ExpandError> {
        let mut result = Vec::new();
        let mut current = value.clone();

        loop {
            match current {
                Value::Null => break,
                Value::Pair(p) => {
                    let borrowed = p.borrow();
                    result.push(borrowed.0.clone());
                    current = borrowed.1.clone();
                }
                _ => {
                    return Err(ExpandError::InvalidTemplate {
                        message: format!("Expected list, got {}", value),
                    });
                }
            }
        }

        Ok(result)
    }

    /// Convert a Vec to a Scheme list
    fn vec_to_list(&self, values: Vec<Value>) -> Value {
        let mut result = Value::Null;
        for value in values.into_iter().rev() {
            result = Value::Pair(Rc::new(RefCell::new((value, result))));
        }
        result
    }
}

impl Default for Expander {
    fn default() -> Self {
        // Create an empty runtime environment for tests
        use patina_runtime::Environment;
        Self::new(
            std::rc::Rc::new(Environment::new()),
            patina_runtime::ScopeId::fresh(),
        )
    }
}

/// Check if a symbol is a special form keyword
///
/// Special forms are part of the language syntax and should not be renamed.
fn is_special_form(name: &str) -> bool {
    matches!(
        name,
        // Core special forms
        "quote"
            | "if"
            | "define"
            | "set!"
            | "lambda"
            | "begin"
            | "apply"
            | "call-with-values"
            // Macro-related special forms
            | "define-syntax"
            | "let-syntax"
            | "letrec-syntax"
            | "syntax-rules"
            // Parameter-related special forms
            | "parameterize"
            // Derived special forms (could be macros but are special forms for now)
            | "cond"
            | "case"
            | "let"
            | "let*"
            | "letrec"
            | "letrec*"
            | "and"
            | "or"
            | "do"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macro_expander::template::Identifier;

    // Helper to create a list from values
    fn make_list(values: Vec<Value>) -> Value {
        let mut result = Value::Null;
        for value in values.into_iter().rev() {
            result = Value::Pair(Rc::new(RefCell::new((value, result))));
        }
        result
    }

    #[test]
    fn test_expand_literal() {
        let expander = Expander::default();
        let template = Template::Literal(Value::Integer(42));
        let env = MatchEnv::new(0);

        let result = expander.expand(&template, &env);
        assert!(result.is_ok());
        assert_eq!(
            format!("{:?}", result.unwrap()),
            format!("{:?}", Value::Integer(42))
        );
    }

    #[test]
    fn test_expand_symbol() {
        let expander = Expander::default();
        let template = Template::Symbol(Identifier::new("if"));
        let env = MatchEnv::new(0);

        let result = expander.expand(&template, &env);
        assert!(result.is_ok());
        if let Value::Symbol(s) = result.unwrap() {
            assert_eq!(&*s, "if");
        } else {
            panic!("Expected symbol");
        }
    }

    #[test]
    fn test_expand_var() {
        let expander = Expander::default();
        let pvref = PVRef::new(0, 0);
        let template = Template::Var(pvref);

        let mut env = MatchEnv::new(1);
        env.insert(pvref, Value::Integer(42));

        let result = expander.expand(&template, &env);
        assert!(result.is_ok());
        assert_eq!(
            format!("{:?}", result.unwrap()),
            format!("{:?}", Value::Integer(42))
        );
    }

    #[test]
    fn test_expand_simple_list() {
        let expander = Expander::default();
        let x = PVRef::new(0, 0);
        let y = PVRef::new(0, 1);

        // Template: (if x y)
        let template = Template::List(vec![
            Template::Symbol(Identifier::new("if")),
            Template::Var(x),
            Template::Var(y),
        ]);

        let mut env = MatchEnv::new(2);
        env.insert(x, Value::Integer(1));
        env.insert(y, Value::Integer(2));

        let result = expander.expand(&template, &env);
        assert!(result.is_ok());

        // Should produce (if 1 2)
        let expected = make_list(vec![
            Value::Symbol(Rc::from("if")),
            Value::Integer(1),
            Value::Integer(2),
        ]);
        assert_eq!(format!("{:?}", result.unwrap()), format!("{:?}", expected));
    }

    #[test]
    fn test_expand_ellipsis_simple() {
        let expander = Expander::default();
        let x = PVRef::new(1, 0);

        // Template: (x ...)
        let template = Template::List(vec![Template::Ellipsis {
            subtemplate: Box::new(Template::Var(x)),
            level: 1,
            nesting: 1,
            vars: vec![x],
        }]);

        let mut env = MatchEnv::new(1);
        // x has 3 values: 1, 2, 3
        env.insert_branch(
            x,
            vec![
                MatchValue::Leaf(Value::Integer(1)),
                MatchValue::Leaf(Value::Integer(2)),
                MatchValue::Leaf(Value::Integer(3)),
            ],
        );

        let result = expander.expand(&template, &env);
        assert!(result.is_ok());

        // Should produce (1 2 3)
        let expected = make_list(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
        ]);
        assert_eq!(format!("{:?}", result.unwrap()), format!("{:?}", expected));
    }

    #[test]
    #[ignore] // TODO: Update test to expect hygiene-renamed identifiers
    fn test_expand_ellipsis_with_constant() {
        let expander = Expander::default();
        let x = PVRef::new(1, 0);

        // Template: ((+ x 1) ...)
        let template = Template::List(vec![Template::Ellipsis {
            subtemplate: Box::new(Template::List(vec![
                Template::Symbol(Identifier::new("+")),
                Template::Var(x),
                Template::Literal(Value::Integer(1)),
            ])),
            level: 1,
            nesting: 1,
            vars: vec![x],
        }]);

        let mut env = MatchEnv::new(1);
        // x has 2 values: 10, 20
        env.insert_branch(
            x,
            vec![
                MatchValue::Leaf(Value::Integer(10)),
                MatchValue::Leaf(Value::Integer(20)),
            ],
        );

        let result = expander.expand(&template, &env);
        assert!(result.is_ok());

        // Should produce ((+ 10 1) (+ 20 1))
        let elem1 = make_list(vec![
            Value::Symbol(Rc::from("+")),
            Value::Integer(10),
            Value::Integer(1),
        ]);
        let elem2 = make_list(vec![
            Value::Symbol(Rc::from("+")),
            Value::Integer(20),
            Value::Integer(1),
        ]);
        let expected = make_list(vec![elem1, elem2]);
        assert_eq!(format!("{:?}", result.unwrap()), format!("{:?}", expected));
    }

    #[test]
    fn test_expand_ellipsis_with_following() {
        let expander = Expander::default();
        let x = PVRef::new(1, 0);
        let y = PVRef::new(0, 1);

        // Template: (begin x ... y)
        let template = Template::List(vec![
            Template::Symbol(Identifier::new("begin")),
            Template::Ellipsis {
                subtemplate: Box::new(Template::Var(x)),
                level: 1,
                nesting: 1,
                vars: vec![x],
            },
            Template::Var(y),
        ]);

        let mut env = MatchEnv::new(2);
        // x has 2 values: 1, 2
        env.insert_branch(
            x,
            vec![
                MatchValue::Leaf(Value::Integer(1)),
                MatchValue::Leaf(Value::Integer(2)),
            ],
        );
        // y is a single value: 99
        env.insert(y, Value::Integer(99));

        let result = expander.expand(&template, &env);
        assert!(result.is_ok());

        // Should produce (begin 1 2 99)
        let expected = make_list(vec![
            Value::Symbol(Rc::from("begin")),
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(99),
        ]);
        assert_eq!(format!("{:?}", result.unwrap()), format!("{:?}", expected));
    }

    #[test]
    #[ignore] // TODO: Update test to expect hygiene-renamed identifiers
    fn test_expand_double_ellipsis() {
        // This tests the do macro use case with BOTH bindings having steps:
        // Pattern: ((var init step ...) ...)
        // Template: (loop step ... ...)
        // Input: ((i 0 (+ i 1)) (j 10 (- j 1)))
        // Expected: (loop (+ i 1) (- j 1))

        let expander = Expander::default();
        let step = PVRef::new(2, 0); // level 2 because it's in nested ellipsis

        // Template: (loop step ... ...)
        let template = Template::List(vec![
            Template::Symbol(Identifier::new("loop")),
            Template::Ellipsis {
                subtemplate: Box::new(Template::Var(step)),
                level: 1, // ellipsis level is 1 for double ellipsis with level-2 variables
                nesting: 2,
                vars: vec![step],
            },
        ]);

        let mut env = MatchEnv::new(1);

        // step is a doubly-nested structure:
        // Branch([
        //   Branch([Leaf((+ i 1))]),    // First binding has 1 step
        //   Branch([Leaf((- j 1))])     // Second binding has 1 step
        // ])
        let step_i = make_list(vec![
            Value::Symbol(Rc::from("+")),
            Value::Symbol(Rc::from("i")),
            Value::Integer(1),
        ]);
        let step_j = make_list(vec![
            Value::Symbol(Rc::from("-")),
            Value::Symbol(Rc::from("j")),
            Value::Integer(1),
        ]);

        env.insert_branch(
            step,
            vec![
                MatchValue::Branch(vec![MatchValue::Leaf(step_i.clone())]), // i binding: 1 step
                MatchValue::Branch(vec![MatchValue::Leaf(step_j.clone())]), // j binding: 1 step
            ],
        );

        let result = expander.expand(&template, &env);
        assert!(result.is_ok());

        // Should produce (loop (+ i 1) (- j 1))
        let expected = make_list(vec![Value::Symbol(Rc::from("loop")), step_i, step_j]);

        assert_eq!(format!("{:?}", result.unwrap()), format!("{:?}", expected));
    }

    #[test]
    #[ignore] // TODO: Update test to expect hygiene-renamed identifiers
    fn test_expand_double_ellipsis_empty_inner() {
        // Test case where inner ellipsis has 0 elements for some iterations
        // Pattern: ((a b ...) ...)
        // Template: (result b ... ...)
        // Input: ((x 1 2) (y))
        // Expected: (result 1 2) - empty inner branch contributes nothing

        let expander = Expander::default();
        let b = PVRef::new(2, 0);

        let template = Template::List(vec![
            Template::Symbol(Identifier::new("result")),
            Template::Ellipsis {
                subtemplate: Box::new(Template::Var(b)),
                level: 1, // ellipsis level is 1 for double ellipsis with level-2 variables
                nesting: 2,
                vars: vec![b],
            },
        ]);

        let mut env = MatchEnv::new(1);
        env.insert_branch(
            b,
            vec![
                MatchValue::Branch(vec![
                    MatchValue::Leaf(Value::Integer(1)),
                    MatchValue::Leaf(Value::Integer(2)),
                ]), // First group: 2 elements
                MatchValue::Branch(vec![]), // Second group: 0 elements
            ],
        );

        let result = expander.expand(&template, &env);
        assert!(result.is_ok());

        // Should produce (result 1 2) - second group contributes nothing
        let expected = make_list(vec![
            Value::Symbol(Rc::from("result")),
            Value::Integer(1),
            Value::Integer(2),
        ]);

        assert_eq!(format!("{:?}", result.unwrap()), format!("{:?}", expected));
    }

    // === Error Condition Tests ===

    #[test]
    fn test_error_undefined_variable() {
        // Template references a variable that's not in the environment
        let expander = Expander::default();
        let x = PVRef::new(0, 5); // Out of bounds!
        let template = Template::Var(x);

        let env = MatchEnv::new(1); // Only has space for 1 var (index 0)

        let result = expander.expand(&template, &env);
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), ExpandError::UndefinedVariable { .. }),
            "Expected UndefinedVariable error"
        );
    }

    #[test]
    fn test_error_inconsistent_repetition() {
        // Two variables in same ellipsis with different repetition counts
        let expander = Expander::default();
        let x = PVRef::new(1, 0);
        let y = PVRef::new(1, 1);

        // Template: ((x y) ...)
        let template = Template::List(vec![Template::Ellipsis {
            subtemplate: Box::new(Template::List(vec![Template::Var(x), Template::Var(y)])),
            level: 1,
            nesting: 1,
            vars: vec![x, y],
        }]);

        let mut env = MatchEnv::new(2);
        // x has 3 values
        env.insert_branch(
            x,
            vec![
                MatchValue::Leaf(Value::Integer(1)),
                MatchValue::Leaf(Value::Integer(2)),
                MatchValue::Leaf(Value::Integer(3)),
            ],
        );
        // y has only 2 values - INCONSISTENT!
        env.insert_branch(
            y,
            vec![
                MatchValue::Leaf(Value::Integer(10)),
                MatchValue::Leaf(Value::Integer(20)),
            ],
        );

        let result = expander.expand(&template, &env);
        assert!(result.is_err());
        assert!(
            matches!(
                result.unwrap_err(),
                ExpandError::InconsistentRepetition { .. }
            ),
            "Expected InconsistentRepetition error"
        );
    }

    #[test]
    fn test_error_triple_ellipsis_not_supported() {
        // Triple ellipsis (nesting = 3) should be rejected
        let expander = Expander::default();
        let x = PVRef::new(3, 0);

        let template = Template::List(vec![Template::Ellipsis {
            subtemplate: Box::new(Template::Var(x)),
            level: 1,
            nesting: 3, // Triple ellipsis!
            vars: vec![x],
        }]);

        let env = MatchEnv::new(1);

        let result = expander.expand(&template, &env);
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), ExpandError::InvalidTemplate { .. }),
            "Expected InvalidTemplate error for triple ellipsis"
        );
    }

    #[test]
    fn test_error_double_ellipsis_level_zero() {
        // Double ellipsis with level 0 is invalid
        let expander = Expander::default();
        let x = PVRef::new(1, 0);

        let template = Template::List(vec![Template::Ellipsis {
            subtemplate: Box::new(Template::Var(x)),
            level: 0, // Level 0 with double ellipsis is invalid!
            nesting: 2,
            vars: vec![x],
        }]);

        let env = MatchEnv::new(1);

        let result = expander.expand(&template, &env);
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), ExpandError::InvalidTemplate { .. }),
            "Expected InvalidTemplate error for level-0 double ellipsis"
        );
    }

    #[test]
    fn test_error_ellipsis_expands_to_non_list() {
        // This tests that ellipsis expansion produces a list
        // Actually, this is hard to trigger with the current implementation
        // because ellipsis always produces lists. Keeping this as a placeholder
        // in case we add validation later.
    }

    #[test]
    fn test_error_level_mismatch_leaf_instead_of_branch() {
        // Try to expand an ellipsis when the variable is a Leaf instead of Branch
        let expander = Expander::default();
        let x = PVRef::new(1, 0);

        // Template: (x ...)
        let template = Template::List(vec![Template::Ellipsis {
            subtemplate: Box::new(Template::Var(x)),
            level: 1,
            nesting: 1,
            vars: vec![x],
        }]);

        let mut env = MatchEnv::new(1);
        // Insert x as a Leaf instead of Branch - level mismatch!
        env.insert(x, Value::Integer(42));

        let result = expander.expand(&template, &env);
        assert!(result.is_err());
        // This will likely be UndefinedVariable or LevelMismatch
        assert!(
            matches!(
                result.unwrap_err(),
                ExpandError::LevelMismatch { .. } | ExpandError::UndefinedVariable { .. }
            ),
            "Expected LevelMismatch or UndefinedVariable error"
        );
    }

    #[test]
    fn test_error_undefined_in_nested_context() {
        // Variable undefined in nested ellipsis context (out of bounds)
        let expander = Expander::default();
        let x = PVRef::new(1, 5); // Out of bounds!

        let template = Template::List(vec![Template::Ellipsis {
            subtemplate: Box::new(Template::Var(x)),
            level: 1,
            nesting: 1,
            vars: vec![x],
        }]);

        // Create env with limited space
        let env = MatchEnv::new(1); // Only has space for index 0

        let result = expander.expand(&template, &env);
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), ExpandError::UndefinedVariable { .. }),
            "Expected UndefinedVariable error"
        );
    }
}
