# Steel vs Patina Hygiene Implementation Comparison

## Steel's Approach (from `rename_idents.rs`)

### Key Data Structure
```rust
pub struct RenameIdentifiersVisitor<'a> {
    introduced_identifiers: HashSet<InternedString>,  // Tracks bindings introduced by macro
    pattern_variables: &'a [InternedString],          // Pattern variable NAMES
    syntax: &'a [InternedString],                     // Literal keywords (like 'else', '=>')
}
```

### Critical Logic (Line 117-130)

```rust
fn visit_atom(&mut self, a: &mut Atom) -> Self::Output {
    let token = a.syn.ty.clone();
    if let TokenType::Identifier(s) = token {
        // 1. Don't rename literal keywords
        if self.syntax.contains(&s) || s == *DATUM_SYNTAX {
            return;
        }

        // 2. Rename if it's a gensym (introduced identifier OR pattern variable)
        if self.is_gensym(&s) {
            a.syn.ty = TokenType::Identifier(("##".to_string() + s.resolve()).into());
        } else {
            // 3. Mark as unresolved (will be resolved later in outer scope)
            a.syn.unresolved = true;
        }
    }
}

fn is_gensym(&self, ident: &InternedString) -> bool {
    self.introduced_identifiers.contains(ident) || self.pattern_variables.contains(ident)
}
```

### Key Insight

**Steel's strategy:**
- Rename ALL identifiers to `##name`
- Mark some as "unresolved" to be looked up in outer scope later
- Pattern variables are renamed but tracked separately

**The difference from our approach:**
1. Steel renames **pattern variable NAMES** (`a`, `b`) → `##a`, `##b`
2. BUT Steel does NOT collect symbols from pattern variable **VALUES**
3. Steel marks unmatched identifiers as "unresolved" for later resolution

## Patina's Approach

### Key Data Structure
```rust
// In expand_macro()
let mut pattern_vars: HashSet<Rc<str>> = bindings.keys().cloned().collect();
collect_symbols_from_bindings(&bindings, &mut pattern_vars);
```

### Critical Logic

```rust
// Hygiene application
pub fn apply_hygiene(
    expr: &Value,
    pattern_vars: &HashSet<Rc<str>>,
    env: &Rc<Environment>,
) -> Value {
    let free_identifiers = find_free_identifiers(expr, pattern_vars, env);
    // ... rename free_identifiers
}

fn collect_free_identifiers(...) {
    if !pattern_vars.contains(name)        // Not a pattern variable
        && !is_gensym(name)                // Not already renamed
        && !is_special_form(name)          // Not a special form
        && !is_macro(name, env)            // Not a macro
    {
        free_ids.insert(name.clone());
    }
}
```

### Key Insight

**Patina's strategy:**
- Collect pattern variable NAMES (`a`, `b`)
- **ALSO collect symbols from pattern variable VALUES** (`x`, `y` from `a→x`, `b→y`)
- Don't rename anything in the combined set
- Rename everything else

## The Fundamental Difference

### Steel's Two-Phase Approach
1. **Rename everything** to `##name` (including pattern vars)
2. **Mark unresolved** for outer scope lookup
3. Later resolution phase connects unresolved to outer bindings

### Patina's One-Phase Approach
1. **Identify what NOT to rename** (pattern vars + their values + macros + special forms)
2. **Rename the rest** (free identifiers introduced by template)
3. No later resolution needed

## Example: `(swap! x y)`

Pattern: `(swap! a b)`
Template: `(let ((temp a)) (set! a b) (set! b temp))`
Expansion: `(let ((temp x)) (set! x y) (set! y temp))`

### Steel's Processing

**Pattern variables:** `[a, b]`
**Introduced identifiers:** `[temp]` (added when visiting `define`/`let` bindings)

**After visiting atoms:**
- `let` → not renamed (special form)
- `temp` → `##temp` (in introduced_identifiers)
- `x` → `##x` (is_gensym because `a` is in pattern_variables, and this atom has name from pattern)
- `y` → `##y` (same reason)
- `set!` → not renamed (special form)

Wait, this doesn't match what we expect! Let me re-read...

Actually, looking at line 124: `if self.is_gensym(&s)` and line 33:
```rust
pub fn is_gensym(&self, ident: &InternedString) -> bool {
    self.introduced_identifiers.contains(ident) || self.pattern_variables.contains(ident)
}
```

So `is_gensym` returns true if the identifier IS a pattern variable name OR was introduced.
This means **Steel DOES rename pattern variables** but somehow tracks them differently?

Let me look for where pattern_variables comes from...

## The Resolution

After closer inspection, Steel's approach is:
1. Pattern variable **names** (`a`, `b`) are in `pattern_variables`
2. When visiting an atom, if it matches a pattern variable name, it gets renamed
3. BUT during template expansion, pattern variables are **already substituted**
4. So `a` → `x` happens during expansion
5. Then during renaming, `x` is NOT in pattern_variables, so it's marked "unresolved"
6. "Unresolved" means: look this up in the outer (use-site) environment

**Key difference:** Steel uses "unresolved" flag to defer resolution to use-site scope!

## Patina's Solution is Simpler and More Direct

We avoid the two-phase complexity by:
1. Collecting pattern variable values upfront
2. Adding them to the exclusion set
3. One-pass renaming

This is actually **more aligned with R7RS semantics**:
- Pattern variables come from the input → should NOT be renamed
- Free identifiers from template → SHOULD be renamed

## Verdict

**Patina's approach is correct and simpler!**

We don't need to change anything. Steel's approach works but is more complex because:
- It uses a visitor pattern with mutation
- It has a two-phase resolution (rename → resolve unresolved)
- It marks identifiers with metadata (unresolved flag)

Our approach:
- Direct functional renaming
- Single-pass
- Clear separation: what to rename vs what to preserve

## Quoted Symbols Handling

### Steel's Approach
Steel's `visit_quote` at line 109:
```rust
fn visit_quote(&mut self, quote: &mut super::ast::Quote) -> Self::Output {
    self.visit(&mut quote.expr);  // Still visits inside quote!
}
```

Steel DOES rename inside quotes, which is technically incorrect.

### Patina's Approach (Fixed 2025-11-08)

✅ **Patina now correctly handles quoted symbols!**

```rust
fn collect_free_identifiers(...) {
    match expr {
        Value::Pair(pair) => {
            // Check if this is a quote form - skip renaming inside
            if let Value::Symbol(sym) = &pair.0 {
                if sym.as_ref() == "quote" {
                    return; // Don't collect identifiers inside quote
                }
            }
            // ... continue recursion
        }
    }
}

fn rename_identifiers(...) {
    match expr {
        Value::Pair(pair) => {
            // Check if this is a quote form - skip renaming
            if let Value::Symbol(sym) = &pair.0 {
                if sym.as_ref() == "quote" {
                    return expr.clone(); // Return unchanged
                }
            }
            // ... continue recursion
        }
    }
}
```

**Result:** Quoted symbols are now preserved correctly!

Example that now works:
```scheme
(define-syntax assert
  (syntax-rules ()
    ((assert test)
     (if test 'ok 'failed))))

(assert (= 2 2))  ; => ok  (not ##ok#N!)
```
