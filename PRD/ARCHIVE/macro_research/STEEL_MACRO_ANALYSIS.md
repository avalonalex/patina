# Steel-Scheme Macro Analysis - syntax-rules Implementation

**Date:** 2025-11-08
**Repository:** `~/Project/reference/steel`
**Status:** Analysis Complete

---

## Executive Summary

Steel takes a **native Rust implementation** approach similar to vonuvoli, but with **full `syntax-rules` support**:
- ✅ Implements `define-syntax` and `syntax-rules` (unlike vonuvoli)
- ✅ Full pattern matching with ellipsis support
- ✅ Template expansion with identifier mangling for hygiene
- ✅ Written entirely in Rust (no meta-circular bootstrap like chibi)
- ✅ Compile-time macro expansion (separate phase from evaluation)

**Key insight:** This is **exactly what Patina needs** - a pure Rust implementation of R7RS macros that doesn't require bootstrapping from Scheme code.

---

## Architecture Overview

### Three-File Modular Design

**Steel's macro system is split across three files:**

1. **`expander.rs`** (1568 lines) - Macro data structures and pattern matching
2. **`macro_template.rs`** (151 lines) - Template validation
3. **`expand_visitor.rs`** (728 lines) - AST visitor for macro expansion

### Key Differences from Chibi and Vonuvoli

| Aspect | Chibi | Vonuvoli | Steel |
|--------|-------|----------|-------|
| **Macro support** | ✅ Full (`define-syntax`) | ❌ None (Unsupported) | ✅ Full (`define-syntax`) |
| **Implementation** | Scheme + C (meta-circular) | Rust (built-in only) | Rust (native) |
| **Pattern matching** | Generated Scheme code | N/A | Native Rust |
| **Template expansion** | Scheme procedures | N/A | Native Rust |
| **Hygiene** | Explicit renaming + closures | N/A | Identifier mangling |
| **Bootstrap** | Required (init-7.scm) | Not applicable | Not required |
| **R7RS compliance** | Full | Partial (no macros) | Full |

---

## Data Structures

### 1. SteelMacro

**Location:** `expander.rs:146-283`

```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SteelMacro {
    name: InternedString,
    special_forms: Vec<InternedString>,  // Literals (e.g., "else")
    pub(crate) cases: Vec<MacroCase>,    // Pattern-template pairs
    mangled: bool,
    pub(crate) location: Span,
    pub(crate) special_mangled: bool,
}
```

**Corresponds to:**
```scheme
(define-syntax when
  (syntax-rules ()
    ((when test body ...)
     (if test (begin body ...)))))
```

**Fields:**
- `name`: `"when"`
- `special_forms`: `[]` (no literals)
- `cases`: Single `MacroCase` with pattern and template

### 2. MacroCase

**Location:** `expander.rs:312-491`

```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MacroCase {
    args: PatternList,       // Pattern to match
    pub(crate) body: ExprKind,  // Template to expand
}
```

**Example for `when`:**
- `args`: `[Syntax("when"), Single("test"), Many(Single("body"))]`
- `body`: `If(test, Begin([body, ...]), void)`

### 3. MacroPattern Enum

**Location:** `expander.rs:493-530`

```rust
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub enum MacroPattern {
    Rest(Box<MacroPattern>),           // Improper list tail
    Single(InternedString),            // Pattern variable
    Syntax(InternedString),            // Literal keyword
    Many(Box<MacroPattern>),           // Ellipsis (zero-or-more)
    Nested(PatternList, bool),         // Nested list/vector
    CharacterLiteral(char),            // Constant matching
    BytesLiteral(Vec<u8>),
    NumberLiteral(NumberLiteral),
    StringLiteral(Arc<String>),
    BooleanLiteral(bool),
    QuotedExpr(Box<Quote>),
    Quote(InternedString),
    Keyword(InternedString),           // Keyword args (Steel extension)
}
```

**Comparison with R7RS patterns:**
- `Single` → pattern variable
- `Syntax` → literal identifier
- `Many` → ellipsis pattern (`...`)
- `Nested` → sublist pattern
- Rest → constants (numbers, strings, booleans, etc.)

---

## Pattern Matching Algorithm

### Core Function: `match_list_pattern`

**Location:** `expander.rs:843-923`

```rust
fn match_list_pattern(patterns: &[MacroPattern], list: &[ExprKind], improper: bool) -> bool {
    // 1. Split off rest pattern if present
    let (proper_patterns, rest_pattern) = match patterns.split_last() {
        Some((MacroPattern::Rest(pat), rest)) => (rest, Some(&**pat)),
        _ => (patterns, None),
    };

    // 2. Check if ellipsis is present
    let has_ellipsis = patterns.iter().any(|pat| pat.is_many());
    let multi_arity = has_ellipsis || rest_pattern.is_some();

    // 3. Validate length
    let len_matches = if multi_arity {
        proper_list.len() + 1 >= proper_patterns.len()  // ≥ for ellipsis
    } else {
        proper_list.len() == proper_patterns.len()      // Exact match
    };

    if !len_matches {
        return false;
    }

    // 4. Match each pattern element
    let expected_many_captures = proper_list.len() + 1 - proper_patterns.len();

    for pat in proper_patterns {
        match &pat {
            MacroPattern::Many(subpat) if has_ellipsis => {
                // Match zero-or-more repetitions
                for _ in 0..expected_many_captures {
                    let Some((_, item)) = exprs_iter.next() else {
                        return false;
                    };
                    if !match_single_pattern(subpat, item) {
                        return false;
                    }
                }
                continue;
            }
            _ => {}
        }

        let Some((_, expr)) = exprs_iter.next() else {
            return false;
        };

        if !match_single_pattern(pat, expr) {
            return false;
        }
    }

    // 5. Handle rest pattern
    if let Some((pat, exprs)) = tail_to_match {
        // ... rest pattern matching logic
    }

    true
}
```

**Key features:**
- **Length validation:** Strict for exact patterns, relaxed for ellipsis
- **Ellipsis handling:** Calculates expected captures upfront
- **Rest patterns:** Supports improper lists (Scheme extension)
- **Recursive matching:** Uses `match_single_pattern` for nested structures

### Pattern Matching for Single Elements

**Location:** `expander.rs:935-1112`

```rust
fn match_single_pattern(pattern: &MacroPattern, expr: &ExprKind) -> bool {
    match pattern {
        MacroPattern::Single(_) => true,  // Always matches, binds variable

        MacroPattern::Syntax(v) => match expr {
            ExprKind::Atom(Atom {
                syn: SyntaxObject {
                    ty: TokenType::Identifier(s),
                    ..
                },
            }) if s == v => true,  // Literal match
            _ => false,
        },

        MacroPattern::NumberLiteral(n) => {
            // Compare numeric values
            match expr {
                ExprKind::Atom(Atom {
                    syn: SyntaxObject {
                        ty: TokenType::Number(t),
                        ..
                    },
                }) => n.into_steelval()? == t.clone().into_steelval()?,
                _ => false,
            }
        },

        MacroPattern::Nested(patterns, is_vec) => {
            match expr {
                ExprKind::List(l) =>
                    !is_vec && match_list_pattern(&patterns.args, l, l.improper),
                ExprKind::Vector(v) =>
                    *is_vec && !v.bytes && match_list_pattern(&patterns.args, &v.args, false),
                _ => false,
            }
        }

        // ... other literal matching
    }
}
```

---

## Binding Collection

### Core Function: `collect_bindings`

**Location:** `expander.rs:1120-1275`

```rust
fn collect_bindings(
    patterns: &[MacroPattern],
    list: &[ExprKind],
    bindings: &mut FxHashMap<InternedString, ExprKind>,
    binding_kind: &mut FxHashMap<InternedString, BindingKind>,
    improper: bool,
) -> Result<()> {
    let mut expr_iter = list.iter().enumerate();
    let expected_many_captures = list.len() + 1 - patterns.len();

    for pattern in patterns {
        match pattern {
            // Single variable: bind one expression
            MacroPattern::Single(s) => {
                if let Some((_, e)) = expr_iter.next() {
                    bindings.insert(*s, e.clone());
                } else {
                    stop!(ArityMismatch => "macro invocation expected a value, found none");
                }
            }

            // Syntax keyword: check match
            MacroPattern::Syntax(s) => {
                let (_, e) = expr_iter.next().ok_or_else(error_func)?;
                if let ExprKind::Atom(Atom {
                    syn: SyntaxObject {
                        ty: TokenType::Identifier(syn),
                        ..
                    },
                }) = e {
                    if s != syn {
                        stop!(BadSyntax => "macro expansion expected keyword, found: {}", syn)
                    }
                }
            }

            // Ellipsis: collect multiple bindings
            MacroPattern::Many(pat) => {
                let mut nested_bindings = FxHashMap::default();
                let mut list_bindings = FxHashMap::default();

                for _ in 0..expected_many_captures {
                    if let Some((_, expr)) = expr_iter.next() {
                        collect_bindings(
                            std::slice::from_ref(pat),
                            std::slice::from_ref(expr),
                            &mut nested_bindings,
                            binding_kind,
                            false,
                        )?;
                    }

                    for (ident, captured) in nested_bindings.drain() {
                        list_bindings.entry(ident).or_insert(vec![]).push(captured);
                    }
                }

                for (ident, captured_list) in list_bindings {
                    bindings.insert(ident, List::new(captured_list).into());
                    binding_kind.insert(ident, BindingKind::Many);
                }
            }

            // Nested patterns: recursive descent
            MacroPattern::Nested(children, is_vec) => {
                let (_, child) = expr_iter.next()
                    .ok_or_else(throw!(ArityMismatch => "Macro expected a pattern"))?;

                match child {
                    ExprKind::List(l) => {
                        if !is_vec {
                            collect_bindings(
                                &children.args,
                                l,
                                bindings,
                                binding_kind,
                                l.improper,
                            )?;
                        }
                    }
                    // ... vector and quote handling
                }
            }

            // Literals: skip (no binding)
            _ => {
                expr_iter.next();
            }
        }
    }

    Ok(())
}
```

**Key features:**
- **Single bindings:** Directly insert into hashmap
- **Ellipsis bindings:** Collect into Vec, then wrap in List
- **BindingKind tracking:** Distinguishes Single vs Many for expansion
- **Nested structures:** Recursive calls for sublists

---

## Template Expansion

### Expansion Entry Point

**Location:** `expander.rs:431-490`

```rust
fn expand(&self, expr: List, span: Span) -> Result<ExprKind> {
    thread_local! {
        static BINDINGS: RefCell<FxHashMap<InternedString, ExprKind>> = RefCell::new(FxHashMap::default());
        static BINDINGS_KIND: RefCell<FxHashMap<InternedString, BindingKind>> = RefCell::new(FxHashMap::default());
        static FALLBACK_BINDINGS: RefCell<FxHashMap<InternedString, ExprKind>> = RefCell::new(FxHashMap::default());
    }

    BINDINGS.with(|bindings| {
        BINDINGS_KIND.with(|binding_kind| {
            FALLBACK_BINDINGS.with(|fallback_bindings| {
                let mut bindings = bindings.borrow_mut();
                let mut binding_kind = binding_kind.borrow_mut();
                let mut fallback_bindings = fallback_bindings.borrow_mut();

                bindings.clear();
                binding_kind.clear();
                fallback_bindings.clear();

                // Collect bindings from pattern matching
                collect_bindings(
                    &self.args.args[1..],
                    &expr[1..],
                    &mut bindings,
                    &mut binding_kind,
                    expr.improper,
                )?;

                let mut body = self.body.clone();

                // Replace identifiers with bindings
                replace_identifiers(
                    &mut body,
                    &mut bindings,
                    &mut binding_kind,
                    &mut fallback_bindings,
                    span,
                )?;

                Ok(body)
            })
        })
    })
}
```

**Performance optimization:** Thread-local storage to avoid allocations on every expansion

### Template Validation

**Location:** `macro_template.rs:38-150`

```rust
pub fn verify(mut self, expr: &ExprKind) -> Result<(), SteelErr> {
    let _ = self.visit(expr);
    self.result
}

fn visit_atom(&mut self, atom: &Atom) -> ControlFlow<()> {
    // 1. Check for bare ellipses
    if atom.syn.ty == TokenType::Ellipses {
        self.result = steelerr![BadSyntax => "ellipses are not a valid identifier in templates"; atom.syn.span];
        return ControlFlow::Break(());
    }

    let Some(ident) = atom.ident() else {
        return ControlFlow::Continue(());
    };

    // 2. Check if identifier is a pattern variable
    let Some(pattern_depth) = self.bindings.get(ident).copied() else {
        return ControlFlow::Continue(());  // Not a pattern var, OK
    };

    // 3. Verify ellipsis depth matches pattern depth
    if pattern_depth > self.depth {
        let missing = pattern_depth - self.depth;
        let name = if missing > 1 { "ellipses" } else { "ellipsis" };

        self.result = steelerr![BadSyntax => format!(
            "missing {}: pattern variable needs at least {} levels of repetition, found {}",
            name, pattern_depth, self.depth
        ); atom.syn.span];

        return ControlFlow::Break(());
    }

    ControlFlow::Continue(())
}
```

**Validation ensures:**
- No bare ellipses in templates (must be quoted or escaped)
- Pattern variables used with correct ellipsis depth
- Example error: `(syntax-rules () ((m x) (x ...)))` fails because `x` has depth 0 but used with depth 1

---

## Hygiene Strategy

### Identifier Mangling

**Location:** `expander.rs:537-556`

```rust
fn mangle(&mut self, special_forms: &[InternedString]) {
    use MacroPattern::*;
    match self {
        Single(s) => {
            let special = special_forms.contains(s) || *s == InternedString::from_static("_");

            if !special {
                // Mangle non-literal, non-underscore identifiers
                *self = Single(("##".to_string() + s.resolve()).into())
            }
        }
        Nested(v, _) => {
            v.args.iter_mut().for_each(|x| x.mangle(special_forms));
        }
        Many(pat) => pat.mangle(special_forms),
        Rest(v) => {
            v.mangle(special_forms);
        }
        _ => {}
    }
}
```

**How it works:**
1. **Pattern variables** are prefixed with `##` during parsing
2. **Literals** (special forms like `else`) are NOT mangled
3. **Underscore** `_` is NOT mangled (wildcard pattern)
4. **Example:**
   ```scheme
   (define-syntax when
     (syntax-rules ()
       ((when test body ...)
        (if test (begin body ...)))))
   ```
   → Pattern variables `test` and `body` become `##test` and `##body`
   → Literals `if` and `begin` remain unchanged

**Compare with Chibi's approach:**
- **Chibi:** Uses `rename` function to create fresh identifiers in macro environment
- **Steel:** Uses string mangling (`##` prefix) to avoid capture
- **Both achieve:** Macro-inserted identifiers don't capture user variables

### Renaming Identifiers in Templates

**Location:** Referenced in `expander.rs:393` via `RenameIdentifiersVisitor`

```rust
RenameIdentifiersVisitor::new(&args_str, special_forms).rename_identifiers(&mut body);
```

**Purpose:** Ensure identifiers in template that match pattern variables are renamed, while preserving literals

---

## Macro Expansion Process

### Expansion Visitor

**Location:** `expand_visitor.rs:192-459`

```rust
impl<'a> VisitorMutRef for Expander<'a> {
    fn visit(&mut self, expr: &mut ExprKind) -> Self::Output {
        // Depth limit to prevent infinite expansion
        if self.depth > 512 {
            stop!(Generic => "macro expansion depth reached!");
        }

        match expr {
            ExprKind::List(l) => {
                match l.first() {
                    // Check if first element is a macro name
                    Some(ExprKind::Atom(Atom {
                        syn: SyntaxObject {
                            ty: TokenType::Identifier(s),
                            span: sp,
                            ..
                        },
                    })) => {
                        // Look up macro in overlay or main map
                        if let Some(m) = self
                            .overlay
                            .as_ref()
                            .and_then(|x| x.get(s))
                            .or_else(|| self.map.get(s))
                        {
                            // Check if macro is shadowed by local binding
                            if !self.in_scope_values.contains(s) && self.source_id.is_none()
                                || self.source_id == m.location.source_id()
                            {
                                let span = *sp;

                                // Expand macro
                                let mut expanded = m.expand(
                                    List::new_maybe_improper(
                                        std::mem::take(&mut l.args),
                                        l.improper,
                                    ),
                                    span,
                                )?;
                                self.changed = true;

                                self.depth += 1;

                                // Recursively expand result
                                self.visit(&mut expanded)?;

                                self.depth -= 1;

                                *expr = expanded;

                                return Ok(());
                            }
                        }
                    }
                    _ => {}
                }

                // Not a macro, visit subexpressions
                for expr in l.args.iter_mut() {
                    self.visit(expr)?;
                }

                Ok(())
            }

            // ... other expression types
        }
    }
}
```

**Key features:**
- **Shadowing respect:** Macros don't expand if local variable has same name
- **Recursive expansion:** Expanded output is recursively expanded
- **Depth tracking:** Prevents infinite expansion loops
- **In-place mutation:** Uses `&mut ExprKind` for efficiency

### Scope Tracking

**Location:** `expand_visitor.rs:381-397`

```rust
fn visit_lambda_function(
    &mut self,
    lambda_function: &mut super::ast::LambdaFunction,
) -> Self::Output {
    // Push new scope layer
    self.in_scope_values.push_layer();

    // Add lambda parameters to scope
    for value in &lambda_function.args {
        if let Some(ident) = value.atom_identifier() {
            self.in_scope_values.define(*ident);
        }
    }

    // Visit lambda body (macros shadowed by params won't expand)
    self.visit(&mut lambda_function.body)?;

    // Pop scope layer
    self.in_scope_values.pop_layer();

    Ok(())
}
```

**Prevents:** Macro expansion when identifier is shadowed by lambda parameter

---

## Comparison with R7RS Examples

### Example 1: `when` macro

**R7RS definition:**
```scheme
(define-syntax when
  (syntax-rules ()
    ((when test result1 result2 ...)
     (if test (begin result1 result2 ...)))))
```

**Steel's representation:**
```rust
SteelMacro {
    name: "when",
    special_forms: [],
    cases: vec![
        MacroCase {
            args: PatternList::new(vec![
                MacroPattern::Syntax("when"),
                MacroPattern::Single("test"),       // Mangled to "##test"
                MacroPattern::Many(
                    MacroPattern::Single("result1")  // Mangled to "##result1"
                ),
                MacroPattern::Many(
                    MacroPattern::Single("result2")  // Mangled to "##result2"
                ),
            ]),
            body: ExprKind::If(If::new(
                ExprKind::ident("##test"),
                ExprKind::Begin(Begin::new(
                    vec![
                        ExprKind::ident("##result1"),
                        ExprKind::Ellipses,
                        ExprKind::ident("##result2"),
                        ExprKind::Ellipses,
                    ],
                    ...
                )),
                ExprKind::void(),
                ...
            ))
        }
    ],
}
```

**Expansion of `(when #t 1 2 3)`:**

1. **Pattern matching:**
   - `test` binds to `#t`
   - `result1` binds to `[1]` (single-element list, BindingKind::Many)
   - `result2` binds to `[2, 3]` (two-element list, BindingKind::Many)

2. **Binding collection:**
   ```rust
   bindings: {
       "##test": #t,
       "##result1": (list 1),
       "##result2": (list 2 3),
   }
   binding_kind: {
       "##result1": Many,
       "##result2": Many,
   }
   ```

3. **Template expansion:**
   ```scheme
   (if #t (begin 1 2 3))
   ```

### Example 2: Hygiene test

**Input:**
```scheme
(let ((if #t))
  (when if (set! if 'now))
  if)
```

**Expansion:**
1. Pattern matches: `test` → `if` (user's variable)
2. Template expands to: `(if ##test (begin (set! ##test 'now)))`
3. Since `##test` is mangled, it refers to user's `if` variable
4. The `if` special form in template is NOT mangled (literal)
5. Result: Macro's `if` doesn't capture user's `if`

---

## Advantages of Steel's Approach

### 1. No Bootstrap Required
- **Chibi:** Requires loading `init-7.scm` with hundreds of lines of Scheme
- **Steel:** Macros work immediately, no Scheme code needed
- **Patina benefit:** Can implement macros without circular dependencies

### 2. Native Performance
- **Chibi:** Macro expansion runs interpreted Scheme code
- **Steel:** Macro expansion is compiled Rust code
- **Patina benefit:** Faster compilation times

### 3. Type Safety
- **Chibi:** Runtime errors in macro expansion
- **Steel:** Compile-time type checking in Rust
- **Patina benefit:** Easier to debug macro implementation

### 4. Clear Separation of Concerns
- **Pattern matching:** `match_list_pattern` + `match_single_pattern`
- **Binding collection:** `collect_bindings`
- **Template expansion:** `replace_identifiers`
- **Hygiene:** `mangle` + `RenameIdentifiersVisitor`

### 5. Comprehensive Testing
- **Unit tests:** 330+ lines of tests in `expander.rs`
- **Test coverage:** Pattern matching, binding collection, expansion
- **Patina benefit:** Can reuse testing strategy

---

## Disadvantages of Steel's Approach

### 1. More Rust Code
- **Chibi:** ~250 lines of Scheme (init-7.scm)
- **Steel:** ~2400 lines of Rust (3 files)
- **Tradeoff:** More code to write, but easier to understand

### 2. Less Flexible
- **Chibi:** Can modify macro system by editing init-7.scm
- **Steel:** Must modify Rust code and recompile
- **Patina impact:** Minimal (we want stable macro system)

### 3. Hygiene Strategy Differences
- **Chibi:** Full syntactic closures with environment tracking
- **Steel:** Identifier mangling (simpler but less powerful)
- **R7RS compliance:** Both work, chibi's is more sophisticated

---

## Implementation Roadmap for Patina

### Phase 1: Data Structures (Week 2)

**Create Rust enums in `src/value/mod.rs` or new `src/macro_system/mod.rs`:**

```rust
pub enum Pattern {
    Wildcard,                           // _
    Literal(Value),                     // Constants
    Variable(Rc<str>),                  // Pattern variables
    List(Vec<Pattern>),                 // (p1 p2 p3)
    Ellipsis {
        pattern: Box<Pattern>,
        tail: Vec<Pattern>,
    },                                  // (p1 ... p2)
}

pub enum Template {
    Variable(Rc<str>),                  // Pattern variables
    Literal(Value),                     // Constants
    List(Vec<Template>),                // (t1 t2 t3)
    Ellipsis {
        template: Box<Template>,
        tail: Vec<Template>,
    },                                  // (t1 ... t2)
}

pub struct MacroRule {
    pattern: Pattern,
    template: Template,
}

pub struct Macro {
    name: Rc<str>,
    literals: Vec<Rc<str>>,
    rules: Vec<MacroRule>,
    env: Rc<Environment>,              // For hygiene
}
```

**Steel equivalent:**
- `Pattern` ≈ `MacroPattern`
- `Template` ≈ `ExprKind` (steel reuses AST)
- `Macro` ≈ `SteelMacro`

**Add to Value enum:**
```rust
pub enum Value {
    // ... existing variants ...
    Macro(Rc<Macro>),
}
```

### Phase 2: Pattern Matching (Week 3)

**Implement pattern matching following steel's algorithm:**

```rust
// src/macro_system/pattern.rs

pub fn match_pattern(
    pattern: &Pattern,
    expr: &Value,
    literals: &[Rc<str>],
) -> Option<HashMap<Rc<str>, Value>> {
    let mut bindings = HashMap::new();
    if match_pattern_helper(pattern, expr, literals, &mut bindings) {
        Some(bindings)
    } else {
        None
    }
}

fn match_pattern_helper(
    pattern: &Pattern,
    expr: &Value,
    literals: &[Rc<str>],
    bindings: &mut HashMap<Rc<str>, Value>,
) -> bool {
    match pattern {
        Pattern::Wildcard => true,

        Pattern::Literal(lit) => values_equal(lit, expr),

        Pattern::Variable(name) => {
            if literals.contains(name) {
                // Literal match: check if expr is same identifier
                match expr {
                    Value::Symbol(sym) => sym == name,
                    _ => false,
                }
            } else {
                // Pattern variable: bind
                bindings.insert(name.clone(), expr.clone());
                true
            }
        }

        Pattern::List(patterns) => {
            match expr {
                Value::Pair(_) => {
                    let exprs = collect_list_items(expr)?;
                    match_list_patterns(patterns, &exprs, literals, bindings)
                }
                _ => false,
            }
        }

        Pattern::Ellipsis { pattern, tail } => {
            // Complex ellipsis matching (see steel's algorithm)
            match_ellipsis_pattern(pattern, tail, expr, literals, bindings)
        }
    }
}
```

**Reuse steel's logic:**
- `match_list_pattern` → `match_list_patterns`
- `match_single_pattern` → `match_pattern_helper`
- `collect_bindings` → part of `match_pattern_helper`

### Phase 3: Template Expansion (Week 4)

**Implement template expansion:**

```rust
// src/macro_system/template.rs

pub fn expand_template(
    template: &Template,
    bindings: &HashMap<Rc<str>, Value>,
) -> Result<Value, EvalError> {
    match template {
        Template::Literal(lit) => Ok(lit.clone()),

        Template::Variable(name) => {
            bindings.get(name)
                .cloned()
                .ok_or_else(|| EvalError::MacroExpansion(
                    format!("Unbound pattern variable: {}", name)
                ))
        }

        Template::List(templates) => {
            let expanded: Result<Vec<_>, _> = templates
                .iter()
                .map(|t| expand_template(t, bindings))
                .collect();

            Ok(list_from_vec(expanded?))
        }

        Template::Ellipsis { template, tail } => {
            // Find pattern variables in template
            let vars = find_pattern_vars(template);

            // Get binding for first variable (all should have same length)
            let first_var = vars.iter().next()
                .ok_or_else(|| EvalError::MacroExpansion(
                    "No pattern variables in ellipsis template".to_string()
                ))?;

            let list = bindings.get(first_var)
                .ok_or_else(|| EvalError::MacroExpansion(
                    format!("Unbound pattern variable: {}", first_var)
                ))?;

            // List must be a list (from ellipsis pattern match)
            let items = collect_list_items(list)?;

            // Expand template for each item
            let mut expanded = Vec::new();
            for i in 0..items.len() {
                // Create temporary bindings for this iteration
                let mut iter_bindings = bindings.clone();
                for var in &vars {
                    let var_list = bindings.get(var).unwrap();
                    let var_items = collect_list_items(var_list)?;
                    iter_bindings.insert(var.clone(), var_items[i].clone());
                }

                expanded.push(expand_template(template, &iter_bindings)?);
            }

            // Expand tail
            let tail_expanded: Result<Vec<_>, _> = tail
                .iter()
                .map(|t| expand_template(t, bindings))
                .collect();

            expanded.extend(tail_expanded?);

            Ok(list_from_vec(expanded))
        }
    }
}
```

**Differences from steel:**
- **Steel:** Reuses `ExprKind` for templates, uses `replace_identifiers`
- **Patina:** Separate `Template` type, cleaner separation
- **Both:** Same core algorithm for ellipsis expansion

### Phase 4: Integration (Week 5)

**1. Add `define-syntax` special form:**

```rust
// src/eval/special_forms.rs

pub(super) fn eval_define_syntax(
    &self,
    args: Vec<Value>,
    env: &Rc<Environment>,
) -> Result<Value, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::ArityMismatch {
            expected: "2".to_string(),
            got: args.len(),
        });
    }

    let name = match &args[0] {
        Value::Symbol(s) => s.clone(),
        _ => return Err(EvalError::TypeError(
            "define-syntax: name must be a symbol".to_string()
        )),
    };

    // Parse syntax-rules
    let macro_def = parse_syntax_rules(&args[1], env)?;

    // Store in macro environment
    self.macro_env.define(name, Value::Macro(Rc::new(macro_def)));

    Ok(Value::Unspecified)
}
```

**2. Parse `syntax-rules`:**

```rust
fn parse_syntax_rules(
    expr: &Value,
    env: &Rc<Environment>,
) -> Result<Macro, EvalError> {
    // Expect: (syntax-rules (literals...) (pattern template) ...)
    let items = collect_list_items(expr)?;

    if items.len() < 2 {
        return Err(EvalError::SyntaxError(
            "syntax-rules: expected (syntax-rules (literals...) rules...)".to_string()
        ));
    }

    // Check first element is 'syntax-rules
    match &items[0] {
        Value::Symbol(s) if s.as_ref() == "syntax-rules" => {}
        _ => return Err(EvalError::SyntaxError(
            "syntax-rules: expected syntax-rules keyword".to_string()
        )),
    }

    // Parse literals
    let literals = parse_literals(&items[1])?;

    // Parse rules
    let rules: Result<Vec<_>, _> = items[2..]
        .iter()
        .map(|rule| parse_macro_rule(rule))
        .collect();

    Ok(Macro {
        name: "anonymous".into(),  // Will be set by define-syntax
        literals,
        rules: rules?,
        env: env.clone(),
    })
}
```

**3. Macro expansion in evaluator:**

```rust
// src/eval/mod.rs

fn eval_in_env(&self, expr: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
    // ... existing code ...

    // Lists: check for macro expansion first
    Value::Pair(_) => {
        let items = collect_list_items(expr)?;
        if items.is_empty() {
            return Err(EvalError::SyntaxError("Cannot evaluate empty list".to_string()));
        }

        // Check if first element is a macro
        if let Value::Symbol(name) = &items[0] {
            if let Some(Value::Macro(m)) = self.macro_env.get(name) {
                // Expand macro
                let expanded = expand_macro(m, &items[1..])?;

                // Recursively evaluate expansion
                return self.eval_in_env(&expanded, env);
            }
        }

        // Not a macro, proceed with normal evaluation
        self.eval_list(expr, env)
    }
}

fn expand_macro(m: &Macro, args: &[Value]) -> Result<Value, EvalError> {
    // Try each rule in order
    for rule in &m.rules {
        if let Some(bindings) = match_pattern(&rule.pattern, &list_from_vec(args.to_vec()), &m.literals) {
            return expand_template(&rule.template, &bindings);
        }
    }

    Err(EvalError::MacroExpansion(
        format!("No matching pattern for macro {}", m.name)
    ))
}
```

### Phase 5: Hygiene (Week 6)

**Steel's hygiene strategy (simplified):**

```rust
// src/macro_system/hygiene.rs

pub fn mangle_pattern_vars(pattern: &mut Pattern, literals: &[Rc<str>]) {
    match pattern {
        Pattern::Variable(name) => {
            if !literals.contains(name) && name.as_ref() != "_" {
                *name = format!("##{}#{}", name, gensym()).into();
            }
        }
        Pattern::List(patterns) => {
            for p in patterns {
                mangle_pattern_vars(p, literals);
            }
        }
        Pattern::Ellipsis { pattern, tail } => {
            mangle_pattern_vars(pattern, literals);
            for p in tail {
                mangle_pattern_vars(p, literals);
            }
        }
        _ => {}
    }
}
```

**Apply during macro parsing:**
```rust
fn parse_macro_rule(expr: &Value) -> Result<MacroRule, EvalError> {
    let items = collect_list_items(expr)?;
    if items.len() != 2 {
        return Err(EvalError::SyntaxError(
            "macro rule: expected (pattern template)".to_string()
        ));
    }

    let mut pattern = parse_pattern(&items[0])?;
    let template = parse_template(&items[1])?;

    // Mangle pattern variables for hygiene
    mangle_pattern_vars(&mut pattern, &literals);

    Ok(MacroRule { pattern, template })
}
```

---

## Testing Strategy

### Unit Tests (from steel)

**Pattern matching tests:**
```rust
#[test]
fn test_match_basic() {
    let pattern = Pattern::List(vec![
        Pattern::Variable("when".into()),
        Pattern::Variable("test".into()),
        Pattern::Ellipsis {
            pattern: Box::new(Pattern::Variable("body".into())),
            tail: vec![],
        },
    ]);

    let expr = list_from_vec(vec![
        Value::Symbol("when".into()),
        Value::Boolean(true),
        Value::Integer(1),
        Value::Integer(2),
    ]);

    let bindings = match_pattern(&pattern, &expr, &[]).unwrap();

    assert_eq!(bindings.get("when"), Some(&Value::Symbol("when".into())));
    assert_eq!(bindings.get("test"), Some(&Value::Boolean(true)));
    assert_eq!(
        bindings.get("body"),
        Some(&list_from_vec(vec![Value::Integer(1), Value::Integer(2)]))
    );
}
```

**Template expansion tests:**
```rust
#[test]
fn test_expand_basic() {
    let template = Template::List(vec![
        Template::Literal(Value::Symbol("if".into())),
        Template::Variable("test".into()),
        Template::List(vec![
            Template::Literal(Value::Symbol("begin".into())),
            Template::Ellipsis {
                template: Box::new(Template::Variable("body".into())),
                tail: vec![],
            },
        ]),
    ]);

    let mut bindings = HashMap::new();
    bindings.insert("test".into(), Value::Boolean(true));
    bindings.insert(
        "body".into(),
        list_from_vec(vec![Value::Integer(1), Value::Integer(2)]),
    );

    let result = expand_template(&template, &bindings).unwrap();

    // Expected: (if #t (begin 1 2))
    let expected = list_from_vec(vec![
        Value::Symbol("if".into()),
        Value::Boolean(true),
        list_from_vec(vec![
            Value::Symbol("begin".into()),
            Value::Integer(1),
            Value::Integer(2),
        ]),
    ]);

    assert_eq!(result, expected);
}
```

### Integration Tests

**Reuse patina's existing test files:**
```bash
cargo run --release < tests/fixtures/examples/macros/01_basic_when_unless.scm
cargo run --release < tests/fixtures/examples/macros/02_hygiene_tests.scm
```

---

## Conclusion

Steel's implementation provides **exactly what Patina needs**:

### ✅ Advantages for Patina

1. **Pure Rust implementation** - No Scheme bootstrap required
2. **Clear, modular design** - Easy to understand and adapt
3. **Comprehensive testing** - Can reuse testing strategy
4. **R7RS compliant** - Full `syntax-rules` support
5. **Production-ready** - Steel is a mature project

### 📋 Implementation Plan

**Timeline:** 6 weeks to full R7RS macro support

| Week | Focus | Deliverable |
|------|-------|-------------|
| 2 | Data structures | Pattern, Template, Macro enums |
| 3 | Pattern matching | match_pattern function |
| 4 | Template expansion | expand_template function |
| 5 | Integration | define-syntax, macro expansion in eval |
| 6 | Hygiene | Identifier mangling, hygiene tests |

### 🎯 Success Criteria

- ✅ `when`/`unless` macros work
- ✅ `do` loop works
- ✅ All hygiene tests pass (02_hygiene_tests.scm)
- ✅ 245/245 r7rs-tests.scm passing (99%+)

### 📚 Key Files to Reference

**Steel source files:**
- `crates/steel-core/src/parser/expander.rs` - Pattern matching & macro data structures
- `crates/steel-core/src/parser/macro_template.rs` - Template validation
- `crates/steel-core/src/parser/expand_visitor.rs` - Macro expansion visitor

**Patina target files:**
- `src/macro_system/mod.rs` - New module (to create)
- `src/macro_system/pattern.rs` - Pattern matching
- `src/macro_system/template.rs` - Template expansion
- `src/macro_system/hygiene.rs` - Identifier mangling
- `src/eval/special_forms.rs` - Add `eval_define_syntax`
- `src/value/mod.rs` - Add `Value::Macro` variant

---

**Status:** ✅ Analysis Complete
**Recommendation:** Follow Steel's approach for Patina's macro implementation
**Next:** Begin implementing Pattern and Template enums (Week 2)
