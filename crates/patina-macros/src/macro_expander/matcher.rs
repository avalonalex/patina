//! PVREF-based pattern matching (Version 2)
//!
//! This module implements pattern matching for the PVREF-based macro system.
//! It takes compiled Pattern structures and matches them against input expressions,
//! building MatchEnv trees that properly represent nested ellipsis bindings.
//!
//! Key improvements over the original matcher:
//! - Uses PVREF encoding for O(1) variable binding
//! - Uses MatchEnv tree structure for nested ellipsis
//! - Implements Gauche's num_following optimization to avoid backtracking
//! - Proper level tracking for nested patterns
//!
//! Inspired by Gauche's pattern matching (macro.c:600+)
//! Reference: https://github.com/shirok/Gauche/blob/master/src/macro.c

use crate::macro_expander::pattern::Pattern;
use crate::macro_expander::utils::{
    collect_pattern_pvars, list_to_vec, pattern_to_string, pattern_to_string_with_names,
};
use patina_runtime::{MatchEnv, MatchValue, PVRef, Value};
use std::collections::HashSet;

/// Error type for pattern matching failures
#[derive(Debug, Clone, PartialEq)]
pub enum MatchError {
    /// Pattern requires more elements than input provides
    TooFewElements {
        pattern: String,
        expected: usize,
        actual: usize,
    },

    /// Input has more elements than pattern can match (no ellipsis to consume them)
    TooManyElements { expected: usize, actual: usize },

    /// Literal value doesn't match
    LiteralMismatch { expected: String, actual: String },

    /// Type mismatch (e.g., list pattern vs vector input)
    TypeMismatch { expected: String, actual: String },

    /// Vector pattern size doesn't match input vector size
    VectorSizeMismatch { expected: usize, actual: usize },

    /// Ellipsis pattern with inconsistent repetition counts
    InconsistentRepetition {
        var1: String,
        count1: usize,
        var2: String,
        count2: usize,
    },
}

impl std::fmt::Display for MatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MatchError::TooFewElements {
                pattern,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "Pattern matching failed: {} requires at least {} element(s), but input has only {}\n\
                     Hint: Check that your macro call provides enough arguments",
                    pattern, expected, actual
                )
            }
            MatchError::TooManyElements { expected, actual } => {
                write!(
                    f,
                    "Pattern matching failed: expected {} element(s), got {}\n\
                     Hint: Pattern has no ellipsis (...) to consume extra elements. \
                     Either add '...' to the pattern or remove extra arguments",
                    expected, actual
                )
            }
            MatchError::LiteralMismatch { expected, actual } => {
                write!(
                    f,
                    "Pattern matching failed: literal mismatch\n\
                     Expected: {}\n\
                     Got:      {}\n\
                     Hint: Literals in patterns must match exactly",
                    expected, actual
                )
            }
            MatchError::TypeMismatch { expected, actual } => {
                write!(
                    f,
                    "Pattern matching failed: type mismatch\n\
                     Expected: {}\n\
                     Got:      {}\n\
                     Hint: List patterns only match lists, vector patterns only match vectors",
                    expected, actual
                )
            }
            MatchError::VectorSizeMismatch { expected, actual } => {
                write!(
                    f,
                    "Pattern matching failed: vector size mismatch\n\
                     Expected: {} element(s)\n\
                     Got:      {} element(s)\n\
                     Hint: Vector patterns must match the exact number of elements (no ellipsis support yet)",
                    expected, actual
                )
            }
            MatchError::InconsistentRepetition {
                var1,
                count1,
                var2,
                count2,
            } => {
                write!(
                    f,
                    "Pattern matching failed: inconsistent repetition in ellipsis pattern\n\
                     Variable '{}' matched {} time(s)\n\
                     Variable '{}' matched {} time(s)\n\
                     Hint: All variables in the same ellipsis pattern must match the same number of times",
                    var1, count1, var2, count2
                )
            }
        }
    }
}

impl std::error::Error for MatchError {}

/// Pattern matcher for PVREF-based macro system
///
/// This implements the pattern matching phase of macro expansion.
/// It takes a compiled Pattern and an input expression, returning
/// a MatchEnv with all pattern variables bound.
///
/// Based on Gauche's pattern matching approach (macro.c:600+).
pub struct Matcher {
    /// Number of pattern variables (determines MatchEnv size)
    num_pvars: usize,

    /// Optional mapping from PVREF to variable names (for debug output)
    pvar_names: Option<std::collections::HashMap<PVRef, std::rc::Rc<str>>>,

    /// Names that are shadowed by local bindings at the macro use site.
    /// When a literal identifier (like `=>` in cond) is in this set,
    /// it should NOT match as a literal (R7RS 4.3.2).
    ///
    /// This is compile-time shadowing info from the desugarer's `shadowed_names`.
    shadowed_names: std::collections::HashSet<std::rc::Rc<str>>,

    /// Literal identifiers from the macro definition (e.g., ["else", "=>"] for cond)
    /// Used with shadowed_names to check if literals are shadowed.
    literals: Vec<std::rc::Rc<str>>,
}

impl Matcher {
    /// Create a new matcher
    ///
    /// # Arguments
    /// * `num_pvars` - Total number of pattern variables in the pattern
    pub fn new(num_pvars: usize) -> Self {
        Self {
            num_pvars,
            pvar_names: None,
            shadowed_names: std::collections::HashSet::new(),
            literals: Vec::new(),
        }
    }

    /// Create a new matcher with pattern variable names for debug output
    ///
    /// # Arguments
    /// * `num_pvars` - Total number of pattern variables in the pattern
    /// * `pvar_names` - Mapping from PVREF to variable names
    pub fn new_with_names(
        num_pvars: usize,
        pvar_names: std::collections::HashMap<PVRef, std::rc::Rc<str>>,
    ) -> Self {
        Self {
            num_pvars,
            pvar_names: Some(pvar_names),
            shadowed_names: std::collections::HashSet::new(),
            literals: Vec::new(),
        }
    }

    /// Create a new matcher with hygiene support for literal shadowing
    ///
    /// # Arguments
    /// * `num_pvars` - Total number of pattern variables in the pattern
    /// * `pvar_names` - Mapping from PVREF to variable names
    /// * `shadowed_names` - Names shadowed by local bindings at macro use site
    /// * `literals` - Literal identifiers from the macro definition
    pub fn new_with_hygiene(
        num_pvars: usize,
        pvar_names: std::collections::HashMap<PVRef, std::rc::Rc<str>>,
        shadowed_names: std::collections::HashSet<std::rc::Rc<str>>,
        literals: Vec<std::rc::Rc<str>>,
    ) -> Self {
        Self {
            num_pvars,
            pvar_names: Some(pvar_names),
            shadowed_names,
            literals,
        }
    }

    /// Match a pattern against an input expression
    ///
    /// This is the main entry point for pattern matching.
    /// Returns a MatchEnv with all pattern variables bound on success.
    ///
    /// Inspired by Gauche's match_pattern (macro.c:600+)
    pub fn match_pattern(&self, pattern: &Pattern, input: &Value) -> Result<MatchEnv, MatchError> {
        if patina_runtime::macro_debug::is_enabled() {
            // Use names if available, otherwise fall back to generic representation
            let pattern_str = if let Some(ref names) = self.pvar_names {
                pattern_to_string_with_names(pattern, names)
            } else {
                pattern_to_string(pattern)
            };
            println!("[MACRO]     Pattern: {}", pattern_str);
            println!("[MACRO]     Input: {}", input);
        }

        let mut env = MatchEnv::new(self.num_pvars);
        let result = self.match_impl(pattern, input, &mut env, 0);

        if patina_runtime::macro_debug::is_enabled() {
            match &result {
                Ok(_) => {
                    println!("[MACRO]     Match: SUCCESS");
                    println!("[MACRO]     Bindings:");
                    self.print_bindings(&env);
                }
                Err(e) => {
                    println!("[MACRO]     Match: FAILED ({})", e);
                }
            }
        }

        result?;
        Ok(env)
    }

    /// Internal matching implementation
    ///
    /// # Arguments
    /// * `pattern` - The pattern to match
    /// * `input` - The input expression
    /// * `env` - The match environment being built
    /// * `level` - Current ellipsis nesting level
    ///
    /// Based on Gauche's match_rec (macro.c:620+)
    fn match_impl(
        &self,
        pattern: &Pattern,
        input: &Value,
        env: &mut MatchEnv,
        level: usize,
    ) -> Result<(), MatchError> {
        match pattern {
            Pattern::Wildcard => {
                // Wildcard matches anything, binds nothing
                Ok(())
            }

            Pattern::Literal(lit) => {
                // Literal must match exactly, BUT we also need to check hygiene:
                // If the literal is an identifier that's shadowed at the use site,
                // it should NOT match (R7RS 4.3.2).
                //
                // Example: (let ((=> #f)) (cond (#t => 'ok)))
                // Here `=>` is shadowed, so it shouldn't match cond's `=>` literal.
                if self.is_literal_shadowed(lit, input) {
                    return Err(MatchError::LiteralMismatch {
                        expected: format!("{:?}", lit),
                        actual: format!("{:?} (shadowed)", input),
                    });
                }

                // Check for match
                // For identifiers (Symbol and Identifier types), compare by name only.
                // This enables recursive macro expansion where literals like `else`
                // may be transformed from Symbol to Identifier with scopes during
                // pattern variable substitution, but should still match.
                if Self::values_match_as_literal(lit, input) {
                    Ok(())
                } else {
                    Err(MatchError::LiteralMismatch {
                        expected: format!("{:?}", lit),
                        actual: format!("{:?}", input),
                    })
                }
            }

            Pattern::Var(pvref) => {
                // Bind pattern variable
                // At level 0, this is a simple leaf binding
                env.insert(*pvref, input.clone());
                Ok(())
            }

            Pattern::List(patterns) => {
                // Match list pattern
                self.match_list(patterns, input, env, level)
            }

            Pattern::Vector(patterns) => {
                // Match vector pattern
                self.match_vector(patterns, input, env, level)
            }

            Pattern::DottedList { patterns, tail } => {
                // Match dotted list: (p1 p2 . rest)
                self.match_dotted_list(patterns, tail, input, env, level)
            }

            Pattern::Ellipsis {
                subpattern,
                level: _pattern_level,
                num_following,
                vars,
            } => {
                // Match ellipsis pattern: (p ...)
                // This is the most complex case
                self.match_ellipsis(subpattern, *num_following, vars, input, env, level)
            }
        }
    }

    /// Match a list pattern against a list value
    ///
    /// Handles both proper lists and lists with ellipsis patterns.
    ///
    /// # Ellipsis Pattern Matching Algorithm
    ///
    /// This implements Gauche's `num_following` optimization (macro.c:138-145) to avoid
    /// backtracking. The key insight is:
    ///
    /// 1. **Pre-computation**: During pattern compilation, each ellipsis pattern stores
    ///    `num_following` - the count of non-ellipsis patterns that follow it
    ///
    /// 2. **Greedy consumption**: The ellipsis consumes `remaining_input - num_following`
    ///    elements, leaving exactly enough for the trailing patterns
    ///
    /// 3. **Branch collection**: For nested ellipsis support, we collect ALL pattern
    ///    variables from the subpattern tree (not just direct children), creating a
    ///    "branch" for each variable that stores values from each iteration
    ///
    /// # Example
    ///
    /// Pattern: `(a b ... c)` with `num_following = 1`
    /// Input: `(1 2 3 4 5)`
    ///
    /// - `a` matches `1` (non-ellipsis, consumes 1)
    /// - `b ...` consumes `4 - 1 = 3` elements: `(2 3 4)` → `b` bound to branch `[2, 3, 4]`
    /// - `c` matches `5` (trailing element)
    ///
    /// # MatchEnv Branches
    ///
    /// A "branch" in `MatchEnv` represents multiple values bound to a pattern variable
    /// during ellipsis iteration. For pattern `(x ...)` matching `(1 2 3)`:
    /// - `x` gets branch `[Leaf(1), Leaf(2), Leaf(3)]`
    ///
    /// For nested ellipsis `((x ...) ...)` matching `((1 2) (3))`:
    /// - `x` gets branch `[Branch([1, 2]), Branch([3])]`
    fn match_list(
        &self,
        patterns: &[Pattern],
        input: &Value,
        env: &mut MatchEnv,
        level: usize,
    ) -> Result<(), MatchError> {
        // Input must be a list
        if !matches!(input, Value::Pair(_) | Value::Null) {
            return Err(MatchError::TypeMismatch {
                expected: "list".to_string(),
                actual: format!("{}", input),
            });
        }

        // Convert input to Vec for easier processing
        let input_list = self.value_to_vec(input)?;

        // Check if we have enough elements (accounting for ellipsis patterns)
        let min_required = self.count_min_required(patterns);
        if input_list.len() < min_required {
            return Err(MatchError::TooFewElements {
                pattern: "list pattern".to_string(),
                expected: min_required,
                actual: input_list.len(),
            });
        }

        // Match patterns against input
        let mut input_idx = 0;
        let mut has_ellipsis = false;

        for pattern in patterns {
            if pattern.is_ellipsis() {
                has_ellipsis = true;
                // Handle ellipsis pattern
                if let Pattern::Ellipsis {
                    subpattern,
                    num_following,
                    ..
                } = pattern
                {
                    // Calculate how many elements to consume
                    // Use Gauche's optimization: leave num_following elements for patterns after this
                    let remaining_input = input_list.len() - input_idx;
                    let to_consume = remaining_input.saturating_sub(*num_following);

                    // Collect ALL variables from subpattern (recursively)
                    // This is critical for nested ellipsis support
                    let all_vars = self.collect_all_pvars(subpattern);

                    // Initialize branches for ALL variables
                    let mut branches: std::collections::HashMap<PVRef, Vec<MatchValue>> = all_vars
                        .iter()
                        .copied()
                        .map(|pvref| (pvref, Vec::new()))
                        .collect();

                    // Match subpattern against each consumed element
                    for i in 0..to_consume {
                        let elem = &input_list[input_idx + i];

                        // Create a temporary environment for this iteration
                        let mut temp_env = MatchEnv::new(self.num_pvars);
                        self.match_impl(subpattern, elem, &mut temp_env, level + 1)?;

                        // Extract matched values for ALL variables (not just direct ones)
                        // This is the key fix for nested ellipsis
                        for &pvref in &all_vars {
                            if let Some(match_value) = temp_env.get_raw(pvref) {
                                branches.entry(pvref).or_default().push(match_value.clone());
                            }
                        }
                    }

                    // Install branches into environment
                    for (pvref, values) in branches {
                        env.insert_branch(pvref, values);
                    }

                    input_idx += to_consume;
                }
            } else {
                // Regular pattern
                if input_idx >= input_list.len() {
                    return Err(MatchError::TooFewElements {
                        pattern: format!("{}", pattern),
                        expected: input_idx + 1,
                        actual: input_list.len(),
                    });
                }
                self.match_impl(pattern, &input_list[input_idx], env, level)?;
                input_idx += 1;
            }
        }

        // Check for unconsumed input elements
        // If the pattern has no ellipsis, all input elements must be consumed.
        // This matches Gauche's behavior (macro.c:901): return SCM_NULLP(form);
        if !has_ellipsis && input_idx < input_list.len() {
            return Err(MatchError::TooManyElements {
                expected: input_idx,
                actual: input_list.len(),
            });
        }

        Ok(())
    }

    /// Recursively collect all pattern variables from a pattern
    ///
    /// This is essential for nested ellipsis support. We need to extract
    /// bindings for ALL variables in the subpattern tree, not just the
    /// direct children of the ellipsis.
    ///
    /// Inspired by Gauche's enter_subpattern (macro.c:766+)
    fn collect_all_pvars(&self, pattern: &Pattern) -> HashSet<PVRef> {
        collect_pattern_pvars(pattern)
    }

    /// Match a vector pattern against a vector value
    fn match_vector(
        &self,
        patterns: &[Pattern],
        input: &Value,
        env: &mut MatchEnv,
        level: usize,
    ) -> Result<(), MatchError> {
        // Input must be a vector
        let input_vec = match input {
            Value::Vector(v) => v.borrow().clone(),
            _ => {
                return Err(MatchError::TypeMismatch {
                    expected: "vector".to_string(),
                    actual: format!("{}", input),
                });
            }
        };

        // For now, require exact size match (no ellipsis in vectors yet)
        if patterns.len() != input_vec.len() {
            return Err(MatchError::VectorSizeMismatch {
                expected: patterns.len(),
                actual: input_vec.len(),
            });
        }

        // Match each pattern
        for (pattern, input_elem) in patterns.iter().zip(input_vec.iter()) {
            self.match_impl(pattern, input_elem, env, level)?;
        }

        Ok(())
    }

    /// Match a dotted list pattern: (p1 p2 . rest)
    fn match_dotted_list(
        &self,
        patterns: &[Pattern],
        tail: &Pattern,
        input: &Value,
        env: &mut MatchEnv,
        level: usize,
    ) -> Result<(), MatchError> {
        // Convert the input to a vector for easier processing
        // For dotted lists, we'll handle the tail separately
        let mut input_list = Vec::new();
        let mut current = input.clone();

        // Collect elements into a vec, keeping track of the tail
        while let Value::Pair(p) = current {
            let borrowed = p.borrow();
            input_list.push(borrowed.0.clone());
            current = borrowed.1.clone();
            drop(borrowed);
        }

        // current now holds the tail (could be Value::Null for proper lists,
        // or some other value for improper lists)

        // Match patterns against input elements
        let mut input_idx = 0;

        for pattern in patterns {
            if pattern.is_ellipsis() {
                // Handle ellipsis pattern
                if let Pattern::Ellipsis {
                    subpattern,
                    num_following,
                    ..
                } = pattern
                {
                    // Calculate how many elements to consume
                    // Leave num_following elements for patterns after this
                    let remaining_input = input_list.len() - input_idx;
                    let to_consume = remaining_input.saturating_sub(*num_following);

                    // Collect ALL variables from subpattern
                    let all_vars = self.collect_all_pvars(subpattern);

                    // Initialize branches for ALL variables
                    let mut branches: std::collections::HashMap<PVRef, Vec<MatchValue>> =
                        all_vars.iter().map(|pvref| (*pvref, Vec::new())).collect();

                    // Match subpattern against each consumed element
                    for i in 0..to_consume {
                        let elem = &input_list[input_idx + i];

                        // Create a temporary environment for this iteration
                        let mut temp_env = MatchEnv::new(self.num_pvars);
                        self.match_impl(subpattern, elem, &mut temp_env, level + 1)?;

                        // Extract matched values for ALL variables
                        for pvref in &all_vars {
                            if let Some(match_value) = temp_env.get_raw(*pvref) {
                                branches
                                    .entry(*pvref)
                                    .or_default()
                                    .push(match_value.clone());
                            }
                        }
                    }

                    // Install branches into environment
                    for (pvref, values) in branches {
                        env.insert_branch(pvref, values);
                    }

                    input_idx += to_consume;
                }
            } else {
                // Regular pattern
                if input_idx >= input_list.len() {
                    return Err(MatchError::TooFewElements {
                        pattern: "dotted list".to_string(),
                        expected: input_idx + 1,
                        actual: input_list.len(),
                    });
                }
                self.match_impl(pattern, &input_list[input_idx], env, level)?;
                input_idx += 1;
            }
        }

        // For dotted patterns, the tail captures remaining elements
        // Reconstruct the remaining list for the tail pattern
        let mut remaining = current; // This is the final cdr (could be Null or improper tail)

        // Build up the remaining list from unconsumed elements
        use std::cell::RefCell;
        use std::rc::Rc;
        for i in (input_idx..input_list.len()).rev() {
            remaining = Value::Pair(Rc::new(RefCell::new((input_list[i].clone(), remaining))));
        }

        // Match the tail against the remaining elements
        self.match_impl(tail, &remaining, env, level)?;

        Ok(())
    }

    /// Match an ellipsis pattern
    ///
    /// This implements Gauche's clever num_following optimization (macro.c:138-145)
    /// to avoid backtracking.
    fn match_ellipsis(
        &self,
        _subpattern: &Pattern,
        _num_following: usize,
        _vars: &[PVRef],
        input: &Value,
        _env: &mut MatchEnv,
        _level: usize,
    ) -> Result<(), MatchError> {
        // This is handled inline in match_list
        // If we get here, we're in a context where ellipsis isn't supported yet
        Err(MatchError::TypeMismatch {
            expected: "ellipsis in list context".to_string(),
            actual: format!("{}", input),
        })
    }

    /// Convert a Scheme list Value to a Vec for easier processing
    fn value_to_vec(&self, value: &Value) -> Result<Vec<Value>, MatchError> {
        list_to_vec(value).map_err(|_| MatchError::TypeMismatch {
            expected: "proper list".to_string(),
            actual: format!("{}", value),
        })
    }

    /// Count minimum required elements in a pattern list
    ///
    /// This accounts for ellipsis patterns which can match zero elements.
    fn count_min_required(&self, patterns: &[Pattern]) -> usize {
        let mut count = 0;
        for pattern in patterns {
            if pattern.is_ellipsis() {
                // Ellipsis can match zero, but need to account for num_following
                if let Pattern::Ellipsis { num_following, .. } = pattern {
                    // The items after this ellipsis are required
                    count += num_following;
                    break; // No more patterns after this (they're accounted for in num_following)
                }
            } else {
                count += 1;
            }
        }
        count
    }

    /// Convert a pattern to a readable string for debugging
    /// Check if a literal identifier is shadowed at the macro use site.
    ///
    /// R7RS 4.3.2: A literal identifier matches an input identifier if both have
    /// the same binding, or both are unbound and have the same name.
    ///
    /// If the literal is shadowed by a local binding at the use site, it should
    /// NOT match, allowing the clause to fall through to other patterns.
    fn is_literal_shadowed(&self, lit: &Value, input: &Value) -> bool {
        // If no shadowed names, nothing can be shadowed
        if self.shadowed_names.is_empty() {
            return false;
        }

        // Extract the literal name from the pattern
        let lit_name = match lit {
            Value::Symbol(s) => s.clone(),
            Value::Identifier(id) => id.name.clone(),
            _ => return false, // Non-identifier literals can't be shadowed
        };

        // Only check literals that are in the macro's literals list
        if !self
            .literals
            .iter()
            .any(|l| l.as_ref() == lit_name.as_ref())
        {
            return false;
        }

        // Extract the input name
        let input_name = match input {
            Value::Symbol(s) => s.clone(),
            Value::Identifier(id) => id.name.clone(),
            _ => return false, // Input is not an identifier
        };

        // Names must match for shadowing to be relevant
        if lit_name.as_ref() != input_name.as_ref() {
            return false;
        }

        // Check if the input name is in the shadowed_names set
        // This is compile-time shadowing from lambda parameters, let bindings, etc.
        self.shadowed_names.contains(&input_name)
    }

    /// Check if two values match as literals.
    ///
    /// For identifier types (Symbol and Identifier), we compare by name only.
    /// This is necessary because during recursive macro expansion, a literal
    /// identifier may be transformed from a Symbol to an Identifier with scopes
    /// when it passes through pattern variable substitution.
    ///
    /// Example: In `(cond (#f 1) (else 2))`:
    /// 1. First expansion: `else` is captured by pattern var `clause`
    /// 2. During substitution, `else` becomes Identifier with macro_scope
    /// 3. Recursive expansion: `(cond (else 2))` should still match the `else` literal
    ///    Compare two values as literals for pattern matching.
    ///
    /// This implements R7RS literal matching semantics:
    /// - Symbols match Symbols by name
    /// - Symbols match Identifiers by name (for compatibility during macro expansion)
    /// - Identifiers with scopes match using bound-identifier=? semantics:
    ///   they must have the same name AND the same scopes
    ///
    /// The bound-identifier=? check is crucial for nested macro hygiene:
    /// when a macro generates another macro, identifiers introduced by
    /// the outer template should only match input with the same binding.
    fn values_match_as_literal(pattern_lit: &Value, input: &Value) -> bool {
        match (pattern_lit, input) {
            // Symbol vs Symbol: compare by name
            (Value::Symbol(pat_name), Value::Symbol(inp_name)) => pat_name == inp_name,

            // Symbol vs Identifier: compare by name (Symbol acts as "any binding")
            (Value::Symbol(pat_name), Value::Identifier(inp_id)) => {
                pat_name.as_ref() == inp_id.name.as_ref()
            }

            // Identifier vs Symbol: compare by name
            (Value::Identifier(pat_id), Value::Symbol(inp_name)) => {
                pat_id.name.as_ref() == inp_name.as_ref()
            }

            // Identifier vs Identifier: bound-identifier=? semantics
            //
            // R7RS 4.3.2: A literal identifier matches an input identifier if both have
            // the same binding, or both are unbound and have the same name.
            //
            // In our scope-set based system:
            // 1. Empty pattern scopes = substituted from outer expansion = matches anything
            // 2. Otherwise, pattern.scopes must be a SUBSET of input.scopes
            //    - This handles the case where input passes through additional expansions
            //    - The subset relationship means they come from the same binding context
            //
            // Note: Shadowing is checked separately in is_literal_shadowed() before we get here.
            // If the input identifier is shadowed by a local binding, that check will fail
            // the match before reaching this function.
            //
            // Example:
            //   (let-syntax ((m (syntax-rules ()
            //     ((m _) (let-syntax ((n (syntax-rules (k) ((n k) 'match) ((n y) 'no))))
            //              (n k))))))
            //     (m x))
            //   => 'match
            // The literal `k` in `n`'s pattern has scopes {S1, S2}.
            // The input `k` in `(n k)` has scopes {S1, S2, S3} after flip.
            // Since {S1, S2} ⊆ {S1, S2, S3}, they match.
            (Value::Identifier(pat_id), Value::Identifier(inp_id)) => {
                // Pattern with empty scopes = substituted from outer expansion
                // It matches ANY identifier (regardless of name or scopes)
                if pat_id.scopes.is_empty() {
                    return true;
                }
                // Otherwise, bound-identifier=? check using subset relationship:
                // Same name AND pattern's scopes are a subset of input's scopes
                pat_id.name.as_ref() == inp_id.name.as_ref()
                    && pat_id.scopes.is_subset_of(&inp_id.scopes)
            }

            // For non-identifier types, use exact comparison via Debug format
            _ => format!("{:?}", pattern_lit) == format!("{:?}", input),
        }
    }

    // Note: pattern_to_string_with_names and pattern_to_string are now in utils.rs
    // and imported at the top of this file

    /// Print bindings from match environment
    fn print_bindings(&self, env: &MatchEnv) {
        // If we have pvar_names, iterate through them and print in order
        // Otherwise, fall back to printing all bindings at levels 0 and 1
        if let Some(ref names) = self.pvar_names {
            // Collect and sort PVREFs by (level, index) for consistent ordering
            let mut sorted_pvrefs: Vec<_> = names.keys().collect();
            sorted_pvrefs.sort_by_key(|pv| (pv.level(), pv.index()));

            for pv in sorted_pvrefs {
                if let Some(value) = env.get_raw(*pv) {
                    let name = &names[pv];
                    println!(
                        "[MACRO]       {} = {}",
                        name,
                        Self::match_value_to_string(value)
                    );
                }
            }
        } else {
            // Fallback: print all bindings at levels 0 and 1
            for level in 0..=1 {
                for i in 0..self.num_pvars {
                    let pv = PVRef::new(level, i as u8);
                    if let Some(value) = env.get_raw(pv) {
                        println!(
                            "[MACRO]       var#{}@L{} = {}",
                            i,
                            level,
                            Self::match_value_to_string(value)
                        );
                    }
                }
            }
        }
    }

    /// Convert a match value to a readable string
    fn match_value_to_string(mv: &MatchValue) -> String {
        match mv {
            MatchValue::Leaf(v) => format!("{}", v),
            MatchValue::Branch(values) => {
                let items: Vec<String> = values.iter().map(Self::match_value_to_string).collect();
                format!("[{}]", items.join(", "))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macro_expander::utils::vec_to_list;
    use std::rc::Rc;

    // Use vec_to_list from utils as make_list alias for test readability
    fn make_list(values: Vec<Value>) -> Value {
        vec_to_list(values)
    }

    #[test]
    fn test_match_wildcard() {
        let matcher = Matcher::new(0);
        let pattern = Pattern::Wildcard;
        let input = Value::Integer(42);

        let result = matcher.match_pattern(&pattern, &input);
        assert!(result.is_ok());
    }

    #[test]
    fn test_match_literal_success() {
        let matcher = Matcher::new(0);
        let pattern = Pattern::Literal(Value::Integer(42));
        let input = Value::Integer(42);

        let result = matcher.match_pattern(&pattern, &input);
        assert!(result.is_ok());
    }

    #[test]
    fn test_match_literal_failure() {
        let matcher = Matcher::new(0);
        let pattern = Pattern::Literal(Value::Integer(42));
        let input = Value::Integer(43);

        let result = matcher.match_pattern(&pattern, &input);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MatchError::LiteralMismatch { .. }
        ));
    }

    #[test]
    fn test_match_var() {
        let matcher = Matcher::new(1);
        let pvref = PVRef::new(0, 0);
        let pattern = Pattern::Var(pvref);
        let input = Value::Integer(42);

        let result = matcher.match_pattern(&pattern, &input);
        assert!(result.is_ok());

        let env = result.unwrap();
        let bound = env.get(pvref, &[]);
        assert_eq!(
            format!("{:?}", bound),
            format!("{:?}", Some(Value::Integer(42)))
        );
    }

    #[test]
    fn test_match_simple_list() {
        // Pattern: (x y)
        let matcher = Matcher::new(2);
        let x = PVRef::new(0, 0);
        let y = PVRef::new(0, 1);
        let pattern = Pattern::List(vec![Pattern::Var(x), Pattern::Var(y)]);

        let input = make_list(vec![Value::Integer(1), Value::Integer(2)]);

        let result = matcher.match_pattern(&pattern, &input);
        assert!(result.is_ok());

        let env = result.unwrap();
        assert!(env.get(x, &[]).is_some());
        assert!(env.get(y, &[]).is_some());
    }

    #[test]
    fn test_match_list_too_few_elements() {
        // Pattern: (x y z)
        let matcher = Matcher::new(3);
        let pattern = Pattern::List(vec![
            Pattern::Var(PVRef::new(0, 0)),
            Pattern::Var(PVRef::new(0, 1)),
            Pattern::Var(PVRef::new(0, 2)),
        ]);

        let input = make_list(vec![Value::Integer(1), Value::Integer(2)]);

        let result = matcher.match_pattern(&pattern, &input);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MatchError::TooFewElements { .. }
        ));
    }

    #[test]
    fn test_match_ellipsis_simple() {
        // Pattern: (x ...)
        let matcher = Matcher::new(1);
        let x = PVRef::new(1, 0);
        let pattern = Pattern::List(vec![Pattern::Ellipsis {
            subpattern: Box::new(Pattern::Var(x)),
            level: 1,
            num_following: 0,
            vars: vec![x],
        }]);

        let input = make_list(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
        ]);

        let result = matcher.match_pattern(&pattern, &input);
        assert!(result.is_ok());

        let env = result.unwrap();
        let bound_raw = env.get_raw(x);
        assert!(bound_raw.is_some());
        // Should be a Branch with 3 elements
        if let Some(MatchValue::Branch(items)) = bound_raw {
            assert_eq!(items.len(), 3);
        } else {
            panic!("Expected Branch, got {:?}", bound_raw);
        }
    }

    #[test]
    fn test_match_ellipsis_with_following() {
        // Pattern: (x ... y)
        let matcher = Matcher::new(2);
        let x = PVRef::new(1, 0);
        let y = PVRef::new(0, 1);
        let pattern = Pattern::List(vec![
            Pattern::Ellipsis {
                subpattern: Box::new(Pattern::Var(x)),
                level: 1,
                num_following: 1,
                vars: vec![x],
            },
            Pattern::Var(y),
        ]);

        let input = make_list(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
            Value::Integer(99),
        ]);

        let result = matcher.match_pattern(&pattern, &input);
        assert!(result.is_ok());

        let env = result.unwrap();

        // x should match first 3 elements
        let x_bound = env.get_raw(x);
        if let Some(MatchValue::Branch(items)) = x_bound {
            assert_eq!(items.len(), 3);
        } else {
            panic!("Expected Branch for x, got {:?}", x_bound);
        }

        // y should match last element (99)
        let y_bound = env.get(y, &[]);
        assert_eq!(
            format!("{:?}", y_bound),
            format!("{:?}", Some(Value::Integer(99)))
        );
    }

    // === Error Condition Tests ===

    #[test]
    fn test_error_too_many_elements() {
        // Pattern: (x y) without ellipsis
        // Input: (1 2 3) - too many elements
        let matcher = Matcher::new(2);
        let pattern = Pattern::List(vec![
            Pattern::Var(PVRef::new(0, 0)),
            Pattern::Var(PVRef::new(0, 1)),
        ]);

        let input = make_list(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3), // extra!
        ]);

        let result = matcher.match_pattern(&pattern, &input);
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), MatchError::TooManyElements { .. }),
            "Expected TooManyElements error"
        );
    }

    #[test]
    fn test_error_type_mismatch_list_vs_vector() {
        // Pattern: (x y) - expects list
        // Input: #(1 2) - vector
        let matcher = Matcher::new(2);
        let pattern = Pattern::List(vec![
            Pattern::Var(PVRef::new(0, 0)),
            Pattern::Var(PVRef::new(0, 1)),
        ]);

        let input = Value::Vector(Rc::new(std::cell::RefCell::new(vec![
            Value::Integer(1),
            Value::Integer(2),
        ])));

        let result = matcher.match_pattern(&pattern, &input);
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), MatchError::TypeMismatch { .. }),
            "Expected TypeMismatch error"
        );
    }

    #[test]
    fn test_error_type_mismatch_vector_vs_list() {
        // Pattern: #(x y) - expects vector
        // Input: (1 2) - list
        let matcher = Matcher::new(2);
        let pattern = Pattern::Vector(vec![
            Pattern::Var(PVRef::new(0, 0)),
            Pattern::Var(PVRef::new(0, 1)),
        ]);

        let input = make_list(vec![Value::Integer(1), Value::Integer(2)]);

        let result = matcher.match_pattern(&pattern, &input);
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), MatchError::TypeMismatch { .. }),
            "Expected TypeMismatch error"
        );
    }

    #[test]
    fn test_error_vector_size_mismatch() {
        // Pattern: #(x y z) - expects 3 elements
        // Input: #(1 2) - only 2 elements
        let matcher = Matcher::new(3);
        let pattern = Pattern::Vector(vec![
            Pattern::Var(PVRef::new(0, 0)),
            Pattern::Var(PVRef::new(0, 1)),
            Pattern::Var(PVRef::new(0, 2)),
        ]);

        let input = Value::Vector(Rc::new(std::cell::RefCell::new(vec![
            Value::Integer(1),
            Value::Integer(2),
        ])));

        let result = matcher.match_pattern(&pattern, &input);
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), MatchError::VectorSizeMismatch { .. }),
            "Expected VectorSizeMismatch error"
        );
    }

    #[test]
    fn test_error_type_mismatch_list_vs_atom() {
        // Pattern: (x y) - expects list
        // Input: 42 - atom
        let matcher = Matcher::new(2);
        let pattern = Pattern::List(vec![
            Pattern::Var(PVRef::new(0, 0)),
            Pattern::Var(PVRef::new(0, 1)),
        ]);

        let input = Value::Integer(42);

        let result = matcher.match_pattern(&pattern, &input);
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), MatchError::TypeMismatch { .. }),
            "Expected TypeMismatch error"
        );
    }

    #[test]
    fn test_error_dotted_list_too_few() {
        // Pattern: (x y . rest) - expects at least 2 elements
        // Input: (1) - only 1 element
        let matcher = Matcher::new(3);
        let pattern = Pattern::DottedList {
            patterns: vec![
                Pattern::Var(PVRef::new(0, 0)),
                Pattern::Var(PVRef::new(0, 1)),
            ],
            tail: Box::new(Pattern::Var(PVRef::new(0, 2))),
        };

        let input = make_list(vec![Value::Integer(1)]);

        let result = matcher.match_pattern(&pattern, &input);
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), MatchError::TooFewElements { .. }),
            "Expected TooFewElements error"
        );
    }
}
