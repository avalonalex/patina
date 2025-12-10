# PVREF-Based Macro System Redesign

**Status:** Design
**Date:** 2025-11-12
**Inspired By:** Gauche Scheme's macro.c implementation by Shiro Kawai
**Reference:** `~/Project/reference/Gauche/src/macro.c`

## Executive Summary

This document outlines the redesign of Patina's macro system using **PVREF (Pattern Variable Reference) encoding**, inspired by Gauche Scheme's elegant and robust implementation. Gauche's design has proven itself in production for over two decades, successfully implementing R7RS-compliant macros with excellent performance and correctness.

The new system will:

1. **Compile patterns once** into an efficient intermediate representation
2. Use **PVREF encoding** (level + index) instead of string-based HashMap lookups
3. Store matched values in **tree structures** for nested ellipsis support
4. Enable **testable macro expansion** for REPL and IDE integration
5. Support **double ellipsis** (`...  ...`) and **ellipsis escaping** (`(... template)`)

## Key Concepts from Gauche

### 1. PVREF (Pattern Variable Reference) Encoding

**PVREF** stands for **Pattern Variable Reference**. It is a compact representation encoding both:
- **Level**: Ellipsis nesting depth (0 = not in ellipsis, 1 = in one `...`, 2 = nested, etc.)
- **Index**: Unique identifier for this variable within the pattern (0, 1, 2, ...)

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct PVRef {
    level: u8,   // Ellipsis depth
    index: u8,   // Variable number
}

impl PVRef {
    pub fn new(level: u8, index: u8) -> Self {
        Self { level, index }
    }

    // Can pack into u16 for even more efficiency
    pub fn pack(self) -> u16 {
        ((self.level as u16) << 8) | (self.index as u16)
    }
}
```

**Example:**
```scheme
;; Pattern: (foo (bar x ...) y ...)
;; Variables:
;;   x -> PVRef { level: 1, index: 0 }
;;   y -> PVRef { level: 1, index: 1 }
;;   bar -> PVRef { level: 0, index: 2 }
;;   foo -> PVRef { level: 0, index: 3 }
```

### 2. Tree-Based Match Storage

Gauche uses a **tree structure** to store matched values for nested ellipsis:

```rust
#[derive(Clone, Debug)]
pub enum MatchValue {
    /// Single matched form (level 0)
    Leaf(Value),
    /// List of matched forms (level > 0)
    Branch(Vec<MatchValue>),
}
```

**Example from Gauche docs (macro.c:689-707):**
```scheme
;; Pattern: (a (b (c d ...) ...) ...)
;; Variables: a=level0, b=level1, c=level2, d=level3

;; Matched form: (1 (2 (3 4 5) (6)) (7 (8 9) (10 11 12)))

;; Bindings (tree structure):
a => Leaf(1)
b => Branch([Leaf(2), Leaf(7)])
c => Branch([Branch([Leaf(3), Leaf(6)]), Branch([Leaf(8), Leaf(10)])])
d => Branch([Branch([Branch([Leaf(4), Leaf(5)]), Branch([])]),
             Branch([Branch([Leaf(9)]), Branch([Leaf(11), Leaf(12)])])])
```

This structure naturally represents the nesting and allows efficient access via indices.

### 3. Two-Phase Design

Gauche separates macro processing into two phases:

**Phase 1: Compilation (at macro definition time)**
- Parse pattern and template
- Assign PVREFs to pattern variables
- Track ellipsis levels
- Convert free identifiers to hygienic identifiers
- Store compiled rules in `ScmSyntaxRules`

**Phase 2: Expansion (at macro use time)**
- Match input against compiled pattern
- Build tree of matched values
- Expand template using tree + indices
- Apply hygiene renaming

This separation enables:
- **Performance**: Compilation happens once, expansion happens many times
- **Testability**: Can inspect compiled patterns and templates
- **Debugging**: Can trace expansion step-by-step

### 4. Key Optimizations

**numFollowingItems** (Gauche's clever optimization from macro.c:133-145):
```rust
pub struct EllipsisPattern {
    subpattern: Box<Pattern>,
    level: u8,

    /// Number of items after this ellipsis (excluding final CDR)
    /// Example: From (x ... y z), x ... has numFollowingItems = 2
    /// Example: From (x ... . y), x ... has numFollowingItems = 0
    ///
    /// This optimization comes from Gauche's ScmSyntaxPattern.numFollowingItems
    /// (macro.c:138-145). It allows the matcher to know exactly how many items
    /// to consume for the ellipsis without backtracking.
    num_following: usize,

    vars: Vec<PVRef>,
}
```

This allows the matcher to know exactly how many items to consume for the ellipsis without backtracking!

## Current Patina Implementation - Problems

### Problem 1: No Ellipsis Level Tracking

```rust
// Current BindingValue enum - cannot distinguish nesting levels!
pub enum BindingValue {
    Single(Value),
    Multiple(Vec<Value>),  // ❌ Flat Vec - loses nesting structure!
}
```

**Issue:** Cannot handle nested ellipsis properly because we lose track of which level variables belong to.

**Example that fails:**
```scheme
;; Pattern: ((var init step ...) ...)
;; Input: ((i 0 (+ i 1)) (j 10))

;; Current system tries to match:
;;   var => Multiple([i, j])  ✓
;;   init => Multiple([0, 10])  ✓
;;   step => Multiple([???])  ❌ How to represent: i has 1 step, j has 0 steps?
```

### Problem 2: String-Based Lookups

```rust
// Current approach: HashMap<Rc<str>, BindingValue>
pub type Bindings = HashMap<Rc<str>, BindingValue>;

// ❌ Problems:
// 1. String comparisons are slower than integer indexing
// 2. No compile-time validation
// 3. Difficult to detect variable level mismatches
```

### Problem 3: Not Testable

Current design mixes parsing, matching, and expansion into one monolithic flow. Cannot easily test:
- "Does this pattern compile correctly?"
- "What PVREF was assigned to variable x?"
- "Show me the intermediate expansion before hygiene"

### Problem 4: Limited Error Messages

Without level tracking, we get vague errors:
```
"Pattern variables in ellipsis have different lengths"
```

Instead of:
```
"Pattern variable 'step' at level 2 has different length than 'var' at level 1
Expected: var (level 1) to have same length as step's outer dimension"
```

## Proposed Architecture

### File Header Template

When implementing the new macro system, each new file should include proper attribution:

```rust
//! PVREF-based macro system
//!
//! This implementation is inspired by Gauche Scheme's macro system,
//! particularly the PVREF (Pattern Variable Reference) encoding and
//! tree-based match storage for handling nested ellipsis patterns.
//!
//! Original design by Shiro Kawai in Gauche's macro.c.
//! Reference: https://github.com/shirok/Gauche
//!
//! Key concepts borrowed from Gauche:
//! - PVREF encoding: (level, index) for pattern variables
//! - Tree-based MatchValue storage for nested ellipsis
//! - numFollowingItems optimization to avoid backtracking
//! - Two-phase compilation (compile once, expand many times)
```

### Core Types

```rust
// ========== Pattern Variable Reference (PVREF) ==========
// PVREF = Pattern Variable Reference
// Inspired by Gauche's PVREF encoding (macro.c:297-300, macroP.h:133-139)

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct PVRef {
    level: u8,  // 0-255 ellipsis levels (more than enough!)
    index: u8,  // 0-255 variables per pattern (more than enough!)
}

impl PVRef {
    pub fn new(level: u8, index: u8) -> Self { /* ... */ }
    pub fn level(&self) -> usize { self.level as usize }
    pub fn index(&self) -> usize { self.index as usize }
}

// ========== Compiled Pattern ==========

#[derive(Clone, Debug)]
pub enum Pattern {
    Wildcard,
    Literal(Value),
    Var(PVRef),  // ✓ Uses PVREF instead of String!
    List(Vec<Pattern>),
    Vector(Vec<Pattern>),
    DottedList { patterns: Vec<Pattern>, tail: Box<Pattern> },

    Ellipsis {
        subpattern: Box<Pattern>,
        level: u8,
        num_following: usize,  // ✓ Gauche's optimization!
        vars: Vec<PVRef>,      // ✓ Precomputed variable list!
    },
}

// ========== Compiled Template ==========

#[derive(Clone, Debug)]
pub enum Template {
    Literal(Value),
    Symbol(Identifier),  // For hygiene
    Var(PVRef),          // ✓ Uses PVREF!
    List(Vec<Template>),
    Vector(Vec<Template>),
    DottedList { templates: Vec<Template>, tail: Box<Template> },

    Ellipsis {
        subtemplate: Box<Template>,
        level: u8,
        nesting: u8,  // ✓ Support for double ellipsis!
        vars: Vec<PVRef>,
    },
}

// ========== Match Storage ==========

#[derive(Clone, Debug)]
pub enum MatchValue {
    Leaf(Value),
    Branch(Vec<MatchValue>),
}

pub struct MatchEnv {
    vars: Vec<MatchValue>,  // ✓ Array indexed by PVRef.index!
}

impl MatchEnv {
    pub fn new(num_vars: usize) -> Self {
        Self {
            vars: vec![MatchValue::Leaf(Value::Null); num_vars],
        }
    }

    pub fn insert(&mut self, pvref: PVRef, value: Value) {
        self.vars[pvref.index()] = MatchValue::Leaf(value);
    }

    pub fn insert_branch(&mut self, pvref: PVRef, values: Vec<MatchValue>) {
        self.vars[pvref.index()] = MatchValue::Branch(values);
    }

    /// Get value at given indices path
    /// indices[0] is unused, indices[1..=level] are the path
    pub fn get(&self, pvref: PVRef, indices: &[usize]) -> Option<Value> {
        let mut val = &self.vars[pvref.index()];

        // Navigate tree using indices
        for i in 1..=pvref.level() {
            match val {
                MatchValue::Branch(items) => {
                    if indices[i] >= items.len() {
                        return None;  // Exhausted
                    }
                    val = &items[indices[i]];
                }
                MatchValue::Leaf(_) => return None,
            }
        }

        match val {
            MatchValue::Leaf(expr) => Some(expr.clone()),
            MatchValue::Branch(_) => None,
        }
    }
}

// ========== Compiled Macro ==========

#[derive(Clone, Debug)]
pub struct CompiledMacro {
    pub name: Symbol,
    pub rules: Vec<MacroRule>,
    pub max_pvars: usize,
}

#[derive(Clone, Debug)]
pub struct MacroRule {
    pub pattern: Pattern,
    pub template: Template,
    pub num_pvars: usize,
    pub max_level: usize,
}
```

### Compilation Pipeline

```rust
pub struct PatternCompiler {
    literals: Vec<Identifier>,
    ellipsis: Option<Symbol>,

    // Per-rule context
    pvars: HashMap<Symbol, PVRef>,  // Name -> PVREF mapping
    pvar_count: usize,
    max_level: usize,
}

impl PatternCompiler {
    pub fn compile(
        &mut self,
        name: Symbol,
        rules: Vec<(Value, Value)>,  // (pattern, template) pairs
    ) -> Result<CompiledMacro, MacroError> {
        let mut compiled_rules = Vec::new();
        let mut max_pvars = 0;

        for (pat_form, tmpl_form) in rules {
            // Reset per-rule context
            self.pvars.clear();
            self.pvar_count = 0;
            self.max_level = 0;

            let pattern = self.compile_pattern(&pat_form, 0)?;
            let template = self.compile_template(&tmpl_form, 0)?;

            compiled_rules.push(MacroRule {
                pattern,
                template,
                num_pvars: self.pvar_count,
                max_level: self.max_level,
            });

            max_pvars = max_pvars.max(self.pvar_count);
        }

        Ok(CompiledMacro {
            name,
            rules: compiled_rules,
            max_pvars,
        })
    }

    fn compile_pattern(&mut self, form: &Value, level: usize)
        -> Result<Pattern, MacroError>
    {
        match form {
            Value::Symbol(s) if s.as_ref() == "_" => {
                Ok(Pattern::Wildcard)
            }

            Value::Symbol(s) => {
                if self.is_literal(s) {
                    Ok(Pattern::Literal(/* ... */))
                } else {
                    // Pattern variable - assign PVREF
                    let pvref = self.add_pvar(s.clone(), level)?;
                    Ok(Pattern::Var(pvref))
                }
            }

            Value::Pair(_) => {
                let (items, tail) = collect_list_items(form)?;
                self.compile_list_pattern(&items, tail, level)
            }

            // ... other cases
        }
    }

    fn compile_list_pattern(
        &mut self,
        items: &[Value],
        tail: Option<Value>,
        level: usize
    ) -> Result<Pattern, MacroError> {
        let mut patterns = Vec::new();
        let mut i = 0;

        while i < items.len() {
            // Check for ellipsis
            if i + 1 < items.len() && self.is_ellipsis(&items[i + 1]) {
                // Count trailing items (key optimization!)
                let num_following = items.len() - i - 2;

                // Track variables in subpattern
                let start_pvars = self.pvar_count;
                let subpattern = self.compile_pattern(&items[i], level + 1)?;
                let end_pvars = self.pvar_count;

                // Collect PVRefs
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

                i += 2;  // Skip pattern and ellipsis
            } else {
                patterns.push(self.compile_pattern(&items[i], level)?);
                i += 1;
            }
        }

        if let Some(tail_value) = tail {
            Ok(Pattern::DottedList {
                patterns,
                tail: Box::new(self.compile_pattern(&tail_value, level)?),
            })
        } else {
            Ok(Pattern::List(patterns))
        }
    }

    fn add_pvar(&mut self, name: Symbol, level: usize)
        -> Result<PVRef, MacroError>
    {
        if self.pvars.contains_key(&name) {
            return Err(MacroError::DuplicatePatternVar(name));
        }

        let pvref = PVRef::new(level as u8, self.pvar_count as u8);
        self.pvars.insert(name, pvref);
        self.pvar_count += 1;

        Ok(pvref)
    }
}
```

### Pattern Matching

```rust
// Pattern matching algorithm based on Gauche's match_synrule (macro.c:763-900)
pub struct PatternMatcher {
    // Temporary storage for building trees
    accum: Vec<Vec<MatchValue>>,
}

impl PatternMatcher {
    pub fn match_pattern(
        &mut self,
        pattern: &Pattern,
        form: &Value,
        env: &mut MatchEnv,
    ) -> Result<(), MatchError> {
        match pattern {
            Pattern::Var(pvref) => {
                env.insert(*pvref, form.clone());
                Ok(())
            }

            Pattern::Ellipsis {
                subpattern,
                num_following,
                vars,
                ..
            } => {
                // This is handled in match_list
                unreachable!()
            }

            // ... other cases
        }
    }

    fn match_list(
        &mut self,
        patterns: &[Pattern],
        items: &[Value],
        env: &mut MatchEnv,
    ) -> Result<(), MatchError> {
        let mut pat_idx = 0;
        let mut form_idx = 0;

        while pat_idx < patterns.len() {
            match &patterns[pat_idx] {
                Pattern::Ellipsis {
                    subpattern,
                    num_following,
                    vars,
                    ..
                } => {
                    // Calculate items to match using Gauche's numFollowingItems optimization
                    // This avoids backtracking by knowing exactly how many items
                    // the ellipsis should consume (macro.c:138-145)
                    let available = items.len() - form_idx;
                    if available < *num_following {
                        return Err(MatchError::TooFewItems);
                    }
                    let limit = available - num_following;

                    // Accumulate matches
                    let mut matches = Vec::new();
                    for _ in 0..limit {
                        let mut sub_env = MatchEnv::new(vars.len());
                        self.match_pattern(
                            subpattern,
                            &items[form_idx],
                            &mut sub_env,
                        )?;
                        matches.push(sub_env);
                        form_idx += 1;
                    }

                    // Store matches in tree structure
                    for pvref in vars {
                        let values: Vec<MatchValue> = matches
                            .iter()
                            .map(|m| m.vars[pvref.index()].clone())
                            .collect();
                        env.insert_branch(*pvref, values);
                    }

                    pat_idx += 1;
                }
                _ => {
                    if form_idx >= items.len() {
                        return Err(MatchError::TooFewItems);
                    }
                    self.match_pattern(&patterns[pat_idx], &items[form_idx], env)?;
                    pat_idx += 1;
                    form_idx += 1;
                }
            }
        }

        if form_idx != items.len() {
            return Err(MatchError::TooManyItems);
        }

        Ok(())
    }
}
```

### Template Expansion

```rust
// Template expansion algorithm based on Gauche's expand_synrule (macro.c:901+)
// Uses tree navigation with indices to handle nested ellipsis
pub struct TemplateExpander {
    indices: Vec<usize>,  // Current index at each ellipsis level
}

impl TemplateExpander {
    pub fn new(max_level: usize) -> Self {
        Self {
            indices: vec![0; max_level + 1],
        }
    }

    pub fn expand(
        &mut self,
        template: &Template,
        env: &MatchEnv,
    ) -> Result<Value, ExpandError> {
        self.expand_rec(template, env, 0)
    }

    fn expand_rec(
        &mut self,
        template: &Template,
        env: &MatchEnv,
        level: usize,
    ) -> Result<Value, ExpandError> {
        match template {
            Template::Var(pvref) => {
                // Navigate tree using current indices at each level
                // Based on Gauche's get_pvref_value (macro.c:730-750)
                env.get(*pvref, &self.indices)
                    .ok_or(ExpandError::Exhausted)
            }

            Template::Ellipsis { subtemplate, .. } => {
                let mut result = Vec::new();

                // Iterate at this level
                self.indices[level + 1] = 0;
                loop {
                    match self.expand_rec(subtemplate, env, level + 1) {
                        Ok(expr) => result.push(expr),
                        Err(ExpandError::Exhausted) => break,
                        Err(e) => return Err(e),
                    }
                    self.indices[level + 1] += 1;
                }

                Ok(list_from_vec(result))
            }

            // ... other cases
        }
    }
}
```

## Migration Plan

### Phase 1: Test Framework ✓ (DONE)

- [x] Create `macro_expansion.rs` test file
- [x] Add pattern parsing tests
- [x] Add template parsing tests
- [x] Add integration tests for simple macros
- [x] Add ignored tests for future features

### Phase 2: Core PVREF Implementation

1. **Add PVREF types** to `patina-runtime`:
   ```rust
   // crates/patina-runtime/src/pvref.rs
   pub struct PVRef { level: u8, index: u8 }
   pub enum MatchValue { Leaf(Value), Branch(Vec<MatchValue>) }
   pub struct MatchEnv { vars: Vec<MatchValue> }
   ```

2. **Update Pattern enum** in `patina-frontend/macro_expander/mod.rs`:
   - Change `Pattern::Variable(Rc<str>)` to `Pattern::Var(PVRef)`
   - Add `num_following` to `Pattern::Ellipsis`
   - Add `vars: Vec<PVRef>` to `Pattern::Ellipsis`

3. **Update Template enum**:
   - Change `Template::Variable(Rc<str>)` to `Template::Var(PVRef)`
   - Add `nesting: u8` to `Template::Ellipsis` for double ellipsis support
   - Add `vars: Vec<PVRef>` to `Template::Ellipsis`

### Phase 3: Pattern Compiler

1. Create `PatternCompiler` struct with PVREF assignment
2. Implement `compile_pattern` with level tracking
3. Implement `compile_template` with level validation
4. Add tests for PVREF assignment correctness

### Phase 4: Pattern Matching with Trees

1. Implement `MatchEnv` with tree storage
2. Update `match_pattern` to use `MatchEnv`
3. Implement tree construction for nested ellipsis
4. Add tests for complex nested patterns

### Phase 5: Template Expansion

1. Implement `TemplateExpander` with indices
2. Update `expand_template` to use tree navigation
3. Handle exhaustion correctly
4. Add tests for expansion at multiple levels

### Phase 6: Advanced Features

1. **Double ellipsis support**:
   - Detect consecutive `...` in parser
   - Set `nesting` field correctly
   - Implement nested iteration in expander

2. **Ellipsis escaping**:
   - Already supported in parser (`EllipsisEscape`)
   - Ensure it works with new system

3. **Better error messages**:
   - Include PVREF info in errors
   - Show expected vs actual levels
   - Provide variable names in errors

### Phase 7: Integration & Migration

1. Update `expand_macro` to use new types
2. Ensure hygiene still works
3. Run all existing tests
4. Fix any regressions
5. Update documentation

## Benefits

### Performance

- **Faster lookups**: Array indexing instead of HashMap
- **Less allocation**: Compile pattern once, reuse many times
- **Better cache locality**: Compact PVREFs fit in CPU cache

### Correctness

- **Proper nesting**: Tree structure naturally represents ellipsis levels
- **Level validation**: Compile-time checks for variable level mismatches
- **No backtracking**: `num_following` optimization

### Testability

- **Inspect compiled patterns**: See exact PVREF assignments
- **Debug expansion**: Trace indices through tree
- **Unit test matching**: Test pattern matching separately from expansion

### Maintainability

- **Clear separation**: Compile vs expand phases
- **Type safety**: PVRef instead of strings
- **Better errors**: Level-aware error messages

## Testing Strategy

### Unit Tests

Each phase gets comprehensive unit tests:

```rust
#[test]
fn test_pvref_assignment() {
    // Pattern: (foo x (bar y ...) z ...)
    let compiler = PatternCompiler::new(...);
    let pattern = compiler.compile_pattern(...)?;

    // Verify PVREF assignments
    assert_eq!(x.pvref, PVRef::new(0, 0));
    assert_eq!(y.pvref, PVRef::new(1, 1));
    assert_eq!(z.pvref, PVRef::new(1, 2));
}

#[test]
fn test_tree_storage() {
    // Pattern: ((a b ...) ...)
    // Input: ((1 2 3) (4 5))
    let env = match_pattern(...)?;

    // Verify tree structure
    assert_eq!(env.get(a_pvref, &[0, 0]), Some(1));
    assert_eq!(env.get(a_pvref, &[0, 1]), Some(4));
    assert_eq!(env.get(b_pvref, &[0, 0, 0]), Some(2));
    assert_eq!(env.get(b_pvref, &[0, 0, 1]), Some(3));
    assert_eq!(env.get(b_pvref, &[0, 1, 0]), Some(5));
}
```

### Integration Tests

Test full macro expansion:

```rust
#[test]
fn test_do_macro_variable_steps() {
    let interp = Interpreter::new();

    interp.eval_str(r#"
        (do ((i 0 (+ i 1))
             (sum 0 (+ sum i)))
            ((> i 5) sum))
    "#)?;

    assert_eq!(result, Value::Integer(15));
}
```

### Regression Tests

All existing tests must continue to pass:
- 61 frontend tests (lexer, parser, macros)
- 285 R7RS compliance tests
- All integration tests

## Open Questions

1. **Memory management**: Should we use `Rc<Pattern>` or `Box<Pattern>`?
   - Patterns are rarely shared, so `Box` is probably better
   - But templates in macro definitions might be cloned?

2. **Error recovery**: How to handle partial matches for better error messages?
   - Could track "deepest match" to show where matching failed

3. **Optimization**: Should we cache compiled macros?
   - Yes, store in `Value::Macro` variant

4. **Backward compatibility**: How to migrate existing macros?
   - Old system can coexist temporarily
   - Gradually migrate bootstrap.scm macros

## Acknowledgments

This redesign is heavily inspired by **Gauche Scheme's macro implementation** by **Shiro Kawai**.
Gauche has successfully implemented R7RS-compliant macros for over two decades with excellent
performance and correctness. We are grateful for the open-source reference implementation that
made this design possible.

### Key Concepts from Gauche

- **PVREF Encoding**: Pattern Variable Reference using (level, index) tupling
- **Tree-Based Match Storage**: MatchVar structure with branch/sprout/root fields
- **numFollowingItems Optimization**: Avoiding backtracking in ellipsis matching
- **Two-Phase Design**: Compile-time pattern analysis vs runtime expansion

### Specific Code References

- **PVREF definition**: `macro.c:297-300`, `macroP.h:133-139`
- **Pattern compilation**: `compile_rule1()` in `macro.c:400+`
- **Match storage**: `MatchVar` structure in `macro.c:709-726`
- **Tree navigation**: `get_pvref_value()` in `macro.c:730-750`
- **Pattern matching**: `match_synrule()` in `macro.c:763-900`
- **numFollowingItems**: `ScmSyntaxPattern` in `macro.c:133-145`

Gauche is distributed under the BSD license:
```
Copyright (c) 2000-2025 Shiro Kawai <shiro@acm.org>
```

## References

- **Gauche macro.c**: `~/Project/reference/Gauche/src/macro.c`
- **Gauche macroP.h**: `~/Project/reference/Gauche/src/gauche/priv/macroP.h`
- **Gauche GitHub**: https://github.com/shirok/Gauche
- **R7RS spec**: Section 4.3 (Macros)
- **SRFI-46**: Basic Syntax-rules Extensions (ellipsis escaping)
- **SRFI-149**: Basic Syntax-rules Extensions (double ellipsis)

## Timeline Estimate

- **Phase 1**: ✓ Done (test framework)
- **Phase 2**: 2-3 hours (PVREF types)
- **Phase 3**: 3-4 hours (pattern compiler)
- **Phase 4**: 4-5 hours (pattern matching)
- **Phase 5**: 3-4 hours (template expansion)
- **Phase 6**: 2-3 hours (advanced features)
- **Phase 7**: 2-3 hours (integration)

**Total**: ~16-22 hours of focused implementation time

## Success Criteria

✓ All existing tests pass
✓ New macro_expansion tests pass (including currently ignored ones)
✓ `do` macro works with variable steps
✓ Double ellipsis examples work
✓ Better error messages with level information
✓ Performance improvements measurable in benchmark
✓ Code is cleaner and more maintainable
