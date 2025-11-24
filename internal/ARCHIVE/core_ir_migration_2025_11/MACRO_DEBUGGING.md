# Macro Debugging Guide

This guide explains how to debug macro expansions in Patina using the built-in debugging tools.

## Table of Contents

- [Quick Start](#quick-start)
- [Enabling Macro Debug Output](#enabling-macro-debug-output)
- [MacroTracer for Selective Debugging](#macrotracer-for-selective-debugging)
- [Understanding Expansion Traces](#understanding-expansion-traces)
- [Marks-and-Ribs Hygiene System](#marks-and-ribs-hygiene-system)
- [Common Debugging Scenarios](#common-debugging-scenarios)
- [Error Messages with Context](#error-messages-with-context)

## Quick Start

### Enable All Macro Debugging

Use the `macro-debug-on` primitive from the REPL or in code:

```scheme
(macro-debug-on)   ; Enable macro debugging
(macro-debug-off)  ; Disable macro debugging
```

This will print detailed information about every macro expansion, including:
- Which macro is being expanded
- The input form
- Which pattern rules are tried
- Pattern matching results
- Template expansion output
- Hygiene (marks-and-ribs) transformations

### Trace Specific Macros

For more focused debugging, use `MacroTracer` from Rust code:

```rust
use patina_macros::MacroTracer;

// Before running the code
MacroTracer::enable_for(&["let-values", "cond"]);

// Run your Scheme code
interpreter.eval_str(code)?;

// Print the expansion history
MacroTracer::print_history();

// Or get it programmatically
let history = MacroTracer::get_history();
for step in history {
    println!("{}", step);
}
```

## Enabling Macro Debug Output

### From Scheme Code

The recommended way to enable macro debugging is using the primitives:

```scheme
; Enable debugging
(macro-debug-on)

; Your macro code here
(let-values (((a b) (values 1 2)))
  (list a b))

; Disable when done
(macro-debug-off)
```

### From Rust Code

You can also enable it programmatically:

```rust
use patina_runtime::macro_debug;

// Enable debugging
macro_debug::enable();

// Run your code
interpreter.eval_str(scheme_code)?;

// Disable
macro_debug::disable();
```

### Example Output

```
[MACRO] Expanding macro: let-values
[MACRO]   Definition (1 rule(s)):
[MACRO]     Rule 1: (let-values (binding ...) body1 body2 ...) -> <template>
[MACRO]
[MACRO]   Input: (let-values (((a b) (values 1 2))) (list a b))
[MACRO]   Trying 1 rule(s):
[MACRO]
[MACRO]   === Trying rule 1 ===
[MACRO]   Pattern: (let-values (binding ...) body1 body2 ...)
[MACRO]   ✓ Match successful!
[MACRO]
[MACRO]   === Expanding template ===
[MACRO]   Template: (call-with-values (lambda () (values 1 2)) (lambda (a b) (begin (list a b))))
[MARKS-AND-RIBS] Marking 'call-with-values' with mark 42 - introduced identifier
[MACRO]   Expanded (with hygiene): (call-with-values (lambda () (values 1 2)) (lambda (a b) (begin (list a b))))
[MACRO]
[MACRO] ========================================
[MACRO] Expansion complete!
[MACRO]
```

### What Gets Logged

1. **Macro Name**: Which macro is being expanded
2. **Rules**: All available pattern/template rules
3. **Input**: The actual form being matched
4. **Pattern Matching**: Each rule tried and whether it matched
5. **Template Expansion**: The expanded result
6. **Hygiene**: Marks-and-ribs transformations applied to identifiers

## MacroTracer for Selective Debugging

`MacroTracer` allows fine-grained control over which macros to trace and how much detail to capture.

### Basic Usage

```rust
use patina_macros::MacroTracer;

// Enable tracing for specific macros
MacroTracer::enable_for(&["let-values", "cond", "my-custom-macro"]);

// Set maximum trace depth (0 = unlimited)
MacroTracer::set_max_depth(10);

// Run your code
let result = interpreter.eval_str(scheme_code)?;

// View the trace
MacroTracer::print_history();

// Clear for next run
MacroTracer::clear();
```

### Trace All Macros

```rust
// Enable tracing for all macros
MacroTracer::enable_all();

// Your code...

MacroTracer::print_history();
```

### Disable Tracing

```rust
MacroTracer::disable();
```

### Get Expansion History

```rust
let history = MacroTracer::get_history();

for (i, step) in history.iter().enumerate() {
    println!("Step {}: {}", i + 1, step);
    println!("  Macro: {}", step.macro_name);
    println!("  Rule: {}/{}", step.rule_index + 1, step.total_rules);
    println!("  Input: {}", step.input);
}
```

### Control Trace Depth

```rust
// Limit tracing to 5 levels deep
MacroTracer::set_max_depth(5);

// This prevents infinite output when debugging recursive macros
```

## Marks-and-Ribs Hygiene System

Patina uses the **marks-and-ribs** hygiene algorithm from Chez Scheme to ensure macros don't accidentally capture variables from the use site.

### How It Works

1. **Marks**: Each macro expansion gets a unique "mark" (integer ID)
2. **Wrapping**: Template identifiers are wrapped with the expansion mark
3. **Comparison**: Two identifiers are "the same" only if they have the same name AND same marks
4. **Pattern Variables**: Variables from the input pattern are NOT marked (they should capture use-site bindings)

### Example

```scheme
(define-syntax my-let
  (syntax-rules ()
    ((my-let ((var val)) body)
     ((lambda (var) body) val))))

(let ((x 'outer))
  (my-let ((x 'inner))
    x))  ; Returns 'inner
```

**What happens**:
1. `my-let` expands with mark #42
2. Template `lambda` gets mark: `lambda@42`
3. Template `var` parameter is a pattern variable - no mark
4. Template `body` is a pattern variable - no mark
5. Final: `((lambda@42 (x) x) 'inner)`
6. The `x` parameter and body `x` have no marks, so they match
7. Result: `'inner`

### Debugging Hygiene

Enable macro debugging to see marks being applied:

```scheme
(macro-debug-on)

(define-syntax swap
  (syntax-rules ()
    ((swap x y)
     (let ((tmp x))
       (set! x y)
       (set! y tmp)))))

(let ((a 1) (b 2))
  (swap a b)
  (list a b))
```

Output will show:
```
[MARKS-AND-RIBS] Marking 'let' with mark 123 - introduced identifier
[MARKS-AND-RIBS] Marking 'tmp' with mark 123 - introduced identifier
[MARKS-AND-RIBS] Marking 'set!' with mark 123 - introduced identifier
```

Pattern variables `x` and `y` are NOT marked, so they correctly refer to `a` and `b` from the use site.

### Value Types for Hygiene

Patina has several Value types to support hygiene:

- **`Value::Symbol`**: Regular symbol (e.g., `x`)
- **`Value::WrappedIdentifier`**: Symbol with marks (e.g., `tmp@123`)
- **`Value::Identifier`**: Symbol with captured environment (for free variables in macros)

### Hygiene in Error Messages

The marks are internal - they don't appear in error messages or output:

```scheme
(define-syntax bad
  (syntax-rules ()
    ((bad) undefined-var)))

(bad)
; Error: Undefined variable: undefined-var
; (NOT: "Undefined variable: undefined-var@42")
```

### References

- **Original Paper**: Dybvig et al., "Syntactic abstraction in Scheme" (1993)
- **Implementation**: Chez Scheme `syntax.ss`
- **Code**: `crates/patina-macros/src/marks_and_ribs.rs`

## Understanding Expansion Traces

An expansion trace shows the sequence of macro expansions that occurred.

### Trace Format

```
Macro expansion history:
1.  let-values (rule 1/1): ((let-values (((a b) (values 1 2))) (list a b)))
2.  cond (rule 2/3): ((cond (test1 result1) (else result2)))
3.  my-macro (rule 1/2): ((my-macro x y))
```

Each step shows:
- **Macro name**: Which macro was expanded
- **Rule**: Which pattern/template rule matched (and how many total rules exist)
- **Input**: The form that was being expanded

### Reading Nested Expansions

When macros expand to other macros, the trace shows the expansion order:

```
1.  let-values (rule 1/1): (...)   # First expansion
2.  call-with-values (rule 1/1): (...)  # Result from let-values contains call-with-values
3.  lambda (rule 1/1): (...)  # Which contains lambda
```

This helps you understand the macro expansion pipeline.

## Common Debugging Scenarios

### Scenario 1: Macro Not Expanding

**Problem**: Your macro isn't being called.

**Debug Steps**:

1. Enable macro debugging:
   ```scheme
   (macro-debug-on)
   ```

2. Look for the macro name in the output. If it doesn't appear, the macro might not be defined or in scope.

3. Check the macro definition:
   ```scheme
   (define-syntax my-macro
     (syntax-rules ()
       ((my-macro x) (list x x))))
   ```

### Scenario 2: Pattern Not Matching

**Problem**: You get "No matching pattern for macro" error.

**Debug Steps**:

1. Enable macro debugging to see which rules are tried:
   ```
   [MACRO]   === Trying rule 1 ===
   [MACRO]   Pattern: (my-macro x y)
   [MACRO]   ✗ Match failed: Expected list, got symbol
   ```

2. Compare your input with the pattern:
   - **Your input**: `(my-macro 1)`
   - **Expected pattern**: `(my-macro x y)` - needs 2 arguments!

3. Fix: Either adjust your input or add a new rule:
   ```scheme
   (syntax-rules ()
     ((my-macro x) (list x))      ; One argument
     ((my-macro x y) (list x y))) ; Two arguments
   ```

### Scenario 3: Hygiene Issue - Variable Capture

**Problem**: Macro is capturing the wrong variable.

**Example**:
```scheme
; This should work correctly with marks-and-ribs
(define-syntax bad-swap
  (syntax-rules ()
    ((bad-swap a b)
     (let ((tmp a))
       (set! a b)
       (set! b tmp)))))

(let ((tmp 'outer))
  (let ((x 1) (y 2))
    (bad-swap x y)
    (list x y tmp)))  ; tmp should still be 'outer
```

**Debug Steps**:

1. Enable macro debugging:
   ```scheme
   (macro-debug-on)
   ```

2. Look for marks being applied:
   ```
   [MARKS-AND-RIBS] Marking 'tmp' with mark 123 - introduced identifier
   ```

3. The introduced `tmp` has a mark, so it won't conflict with the user's `tmp`

### Scenario 4: Recursive Macro Stack Overflow

**Problem**: Infinite macro expansion.

**Debug Steps**:

1. Use `MacroTracer` with depth limit:
   ```rust
   MacroTracer::enable_for(&["my-recursive-macro"]);
   MacroTracer::set_max_depth(10);
   ```

2. Check the trace to see where recursion doesn't terminate:
   ```
   1.  my-macro (rule 1/2): ((my-macro 10))
   2.  my-macro (rule 1/2): ((my-macro 10))  # Same input!
   3.  my-macro (rule 1/2): ((my-macro 10))  # Not decreasing
   ```

3. Fix the base case in your macro.

### Scenario 5: Understanding Macro Expansion Order

**Problem**: Want to understand complex macro interactions.

**Debug Steps**:

1. Trace all involved macros:
   ```rust
   MacroTracer::enable_for(&["let-values", "let*-values", "call-with-values"]);
   ```

2. Run your code and examine the expansion order:
   ```
   1.  let*-values expands to let-values
   2.  let-values expands to call-with-values
   3.  call-with-values is a special form (stops here)
   ```

## Error Messages with Context

When a macro expansion fails, the error message includes expansion context.

### Example Error

```
Macro expansion error: Undefined pattern variable

  in macro: my-macro

Expansion trace:
  let*-values (rule 1/1): ((let*-values ...))
  my-macro (rule 1/1): ((my-macro x y))
```

This tells you:
- **What went wrong**: Undefined pattern variable
- **Where**: In macro `my-macro`
- **How you got there**: Via `let*-values` which expanded to `my-macro`

## Best Practices

1. **Start Small**: Debug macros in isolation before using them in complex code.

2. **Use MacroTracer for Specific Macros**: Instead of enabling all debugging, use `MacroTracer` to focus on problematic macros.

3. **Check Pattern Variables**: Ensure pattern variables match the expected structure.

4. **Test Hygiene**: Verify that temporary variables don't capture user bindings.

5. **Limit Trace Depth**: When debugging recursive macros, use `set_max_depth()` to prevent overwhelming output.

6. **Read Traces Bottom-Up**: The last expansion in the trace is often where the error occurs.

## API Reference

### Scheme Primitives

```scheme
(macro-debug-on)   ; Enable macro debugging output
(macro-debug-off)  ; Disable macro debugging output
```

### Rust API

#### macro_debug Module

```rust
use patina_runtime::macro_debug;

// Enable/disable debugging
macro_debug::enable();
macro_debug::disable();
macro_debug::is_enabled() -> bool;
```

#### MacroTracer Methods

```rust
use patina_macros::MacroTracer;

// Enable/disable tracing
MacroTracer::enable_for(&["macro1", "macro2"]);
MacroTracer::enable_all();
MacroTracer::disable();

// Control depth
MacroTracer::set_max_depth(depth: usize);
MacroTracer::current_depth() -> usize;

// Get trace data
MacroTracer::get_history() -> Vec<ExpansionStep>;
MacroTracer::print_history();

// Clear history
MacroTracer::clear();
```

#### ExpansionStep

```rust
pub struct ExpansionStep {
    pub macro_name: Rc<str>,
    pub rule_index: usize,
    pub total_rules: usize,
    pub input: String,
}
```

## See Also

- **Hygiene Implementation**: `crates/patina-macros/src/marks_and_ribs.rs`
- **Macro Expander**: `crates/patina-macros/src/macro_expander/`
- **Feature Status**: [FEATURE_STATUS.md](FEATURE_STATUS.md)
- **Testing**: [TEST_ORGANIZATION.md](TEST_ORGANIZATION.md)
