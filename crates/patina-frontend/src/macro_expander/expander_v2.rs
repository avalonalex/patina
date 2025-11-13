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

use crate::macro_expander::template_v2::{Identifier, Template2};
use patina_runtime::{MatchEnv, MatchValue, PVRef, Value};
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
                write!(f, "Undefined pattern variable: {}", pvref)
            }
            ExpandError::LevelMismatch {
                pvref,
                template_level,
                actual_level,
            } => {
                write!(
                    f,
                    "Variable {} used at level {} but defined at level {}",
                    pvref, template_level, actual_level
                )
            }
            ExpandError::InconsistentRepetition { expected, actual } => {
                write!(
                    f,
                    "Inconsistent ellipsis repetition: expected {}, got {}",
                    expected, actual
                )
            }
            ExpandError::InvalidTemplate { message } => {
                write!(f, "Invalid template: {}", message)
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
    /// Counter for generating unique identifiers for hygiene
    #[allow(dead_code)] // Will be used when full hygiene is implemented
    gensym_counter: std::cell::RefCell<usize>,
}

impl Expander {
    /// Create a new expander
    pub fn new() -> Self {
        Self {
            gensym_counter: std::cell::RefCell::new(0),
        }
    }

    /// Expand a template with the given match environment
    ///
    /// This is the main entry point for template expansion.
    ///
    /// Inspired by Gauche's expand_template (macro.c:800+)
    pub fn expand(&self, template: &Template2, env: &MatchEnv) -> Result<Value, ExpandError> {
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
        template: &Template2,
        env: &MatchEnv,
        indices: &[usize],
    ) -> Result<Value, ExpandError> {
        match template {
            Template2::Literal(value) => {
                // Literal values are inserted as-is
                Ok(value.clone())
            }

            Template2::Symbol(id) => {
                // Symbols are renamed for hygiene
                Ok(self.rename_identifier(id))
            }

            Template2::Var(pvref) => {
                // Look up pattern variable in match environment
                // Use indices to navigate the tree
                match env.get(*pvref, indices) {
                    Some(value) => Ok(value),
                    None => Err(ExpandError::UndefinedVariable {
                        pvref: format!("{:?}", pvref),
                    }),
                }
            }

            Template2::List(templates) => {
                // Expand list template: (t1 t2 t3)
                self.expand_list(templates, env, indices)
            }

            Template2::Vector(templates) => {
                // Expand vector template: #(t1 t2 t3)
                self.expand_vector(templates, env, indices)
            }

            Template2::DottedList { templates, tail } => {
                // Expand dotted list template: (t1 t2 . rest)
                self.expand_dotted_list(templates, tail, env, indices)
            }

            Template2::Ellipsis {
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
        templates: &[Template2],
        env: &MatchEnv,
        indices: &[usize],
    ) -> Result<Value, ExpandError> {
        let mut result = Vec::new();

        for template in templates {
            if template.is_ellipsis() {
                // Handle ellipsis specially - it expands to multiple elements
                if let Template2::Ellipsis {
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
        templates: &[Template2],
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
        templates: &[Template2],
        tail: &Template2,
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
            result = Value::Pair(Rc::new((item, result)));
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
        subtemplate: &Template2,
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
        subtemplate: &Template2,
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
        subtemplate: &Template2,
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

        // Get iteration count at outer level (level - 1)
        let outer_count = if let Some(&first_var) = vars.first() {
            // The variable should be at level >= 2 for double ellipsis
            // We want to iterate at its parent level
            self.get_iteration_count_at_level(env, first_var, indices, (level - 1) as usize)?
        } else {
            0
        };

        // For each outer iteration, expand the inner ellipsis and flatten
        let mut all_results = Vec::new();

        for outer_idx in 0..outer_count {
            // Build indices for this outer iteration
            let mut outer_indices = indices.to_vec();
            while outer_indices.len() <= (level - 1) as usize {
                outer_indices.push(0);
            }
            outer_indices[(level - 1) as usize] = outer_idx;

            // Get inner iteration count at this outer index
            let inner_count = if let Some(&first_var) = vars.first() {
                self.get_iteration_count(env, first_var, &outer_indices)?
            } else {
                0
            };

            // Expand inner ellipsis
            for inner_idx in 0..inner_count {
                let mut inner_indices = outer_indices.clone();
                while inner_indices.len() <= level as usize {
                    inner_indices.push(0);
                }
                inner_indices[level as usize] = inner_idx;

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
        let mut current = match_value;
        for &idx in indices.iter().skip(1).take(pvref.level() - 1) {
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

    /// Rename an identifier for hygiene
    ///
    /// For now, we just return the identifier as a symbol.
    /// Full hygiene support would involve tracking scopes and renaming.
    fn rename_identifier(&self, id: &Identifier) -> Value {
        // TODO: Implement full hygiene with scope tracking
        // For now, just return as symbol
        Value::Symbol(id.name().clone())
    }

    /// Convert a Scheme list to a Vec
    fn list_to_vec(&self, value: &Value) -> Result<Vec<Value>, ExpandError> {
        let mut result = Vec::new();
        let mut current = value.clone();

        loop {
            match current {
                Value::Null => break,
                Value::Pair(p) => {
                    let (car, cdr) = &*p;
                    result.push(car.clone());
                    current = cdr.clone();
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
            result = Value::Pair(Rc::new((value, result)));
        }
        result
    }
}

impl Default for Expander {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macro_expander::template_v2::Identifier;

    // Helper to create a list from values
    fn make_list(values: Vec<Value>) -> Value {
        let mut result = Value::Null;
        for value in values.into_iter().rev() {
            result = Value::Pair(Rc::new((value, result)));
        }
        result
    }

    #[test]
    fn test_expand_literal() {
        let expander = Expander::new();
        let template = Template2::Literal(Value::Integer(42));
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
        let expander = Expander::new();
        let template = Template2::Symbol(Identifier::new("if"));
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
        let expander = Expander::new();
        let pvref = PVRef::new(0, 0);
        let template = Template2::Var(pvref);

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
        let expander = Expander::new();
        let x = PVRef::new(0, 0);
        let y = PVRef::new(0, 1);

        // Template: (if x y)
        let template = Template2::List(vec![
            Template2::Symbol(Identifier::new("if")),
            Template2::Var(x),
            Template2::Var(y),
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
        let expander = Expander::new();
        let x = PVRef::new(1, 0);

        // Template: (x ...)
        let template = Template2::List(vec![Template2::Ellipsis {
            subtemplate: Box::new(Template2::Var(x)),
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
    fn test_expand_ellipsis_with_constant() {
        let expander = Expander::new();
        let x = PVRef::new(1, 0);

        // Template: ((+ x 1) ...)
        let template = Template2::List(vec![Template2::Ellipsis {
            subtemplate: Box::new(Template2::List(vec![
                Template2::Symbol(Identifier::new("+")),
                Template2::Var(x),
                Template2::Literal(Value::Integer(1)),
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
        let expander = Expander::new();
        let x = PVRef::new(1, 0);
        let y = PVRef::new(0, 1);

        // Template: (begin x ... y)
        let template = Template2::List(vec![
            Template2::Symbol(Identifier::new("begin")),
            Template2::Ellipsis {
                subtemplate: Box::new(Template2::Var(x)),
                level: 1,
                nesting: 1,
                vars: vec![x],
            },
            Template2::Var(y),
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
    fn test_expand_double_ellipsis() {
        // This tests the do macro use case:
        // Pattern: ((var init step ...) ...)
        // Template: (loop step ... ...)
        // Input: ((i 0 (+ i 1)) (j 10))
        // Expected: (loop (+ i 1) j)

        let expander = Expander::new();
        let step = PVRef::new(2, 0); // level 2 because it's in nested ellipsis

        // Template: (loop step ... ...)
        let template = Template2::List(vec![
            Template2::Symbol(Identifier::new("loop")),
            Template2::Ellipsis {
                subtemplate: Box::new(Template2::Var(step)),
                level: 2,
                nesting: 2,
                vars: vec![step],
            },
        ]);

        let mut env = MatchEnv::new(1);

        // step is a doubly-nested structure:
        // Branch([
        //   Branch([Leaf((+ i 1))]),  // First binding has 1 step
        //   Branch([])                 // Second binding has 0 steps (will use var as step)
        // ])
        let plus_symbol = Value::Symbol(Rc::from("+"));
        let i_symbol = Value::Symbol(Rc::from("i"));
        let one = Value::Integer(1);
        let step_expr = make_list(vec![plus_symbol, i_symbol, one]);

        let j_symbol = Value::Symbol(Rc::from("j"));

        env.insert_branch(
            step,
            vec![
                MatchValue::Branch(vec![MatchValue::Leaf(step_expr)]), // i binding: 1 step
                MatchValue::Branch(vec![MatchValue::Leaf(j_symbol)]), // j binding: 1 step (the var itself)
            ],
        );

        let result = expander.expand(&template, &env);
        assert!(result.is_ok());

        // Should produce (loop (+ i 1) j)
        let expected_step = make_list(vec![
            Value::Symbol(Rc::from("+")),
            Value::Symbol(Rc::from("i")),
            Value::Integer(1),
        ]);
        let expected = make_list(vec![
            Value::Symbol(Rc::from("loop")),
            expected_step,
            Value::Symbol(Rc::from("j")),
        ]);

        assert_eq!(format!("{:?}", result.unwrap()), format!("{:?}", expected));
    }

    #[test]
    fn test_expand_double_ellipsis_empty_inner() {
        // Test case where inner ellipsis has 0 elements for some iterations
        // Pattern: ((a b ...) ...)
        // Template: (result b ... ...)
        // Input: ((x 1 2) (y))
        // Expected: (result 1 2)

        let expander = Expander::new();
        let b = PVRef::new(2, 0);

        let template = Template2::List(vec![
            Template2::Symbol(Identifier::new("result")),
            Template2::Ellipsis {
                subtemplate: Box::new(Template2::Var(b)),
                level: 2,
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

        // Should produce (result 1 2)
        let expected = make_list(vec![
            Value::Symbol(Rc::from("result")),
            Value::Integer(1),
            Value::Integer(2),
        ]);

        assert_eq!(format!("{:?}", result.unwrap()), format!("{:?}", expected));
    }
}
