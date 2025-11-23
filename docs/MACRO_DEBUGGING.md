# Macro Debugging Guide

This guide explains how to debug macro expansions in Patina using the built-in debugging tools.

## Table of Contents

- [Quick Start](#quick-start)
- [MACRO_DEBUG Environment Variable](#macro_debug-environment-variable)
- [MacroTracer for Selective Debugging](#macrotracer-for-selective-debugging)
- [Understanding Expansion Traces](#understanding-expansion-traces)
- [Common Debugging Scenarios](#common-debugging-scenarios)
- [Error Messages with Context](#error-messages-with-context)

## Quick Start

### Enable All Macro Debugging

Set the `MACRO_DEBUG` environment variable:

```bash
MACRO_DEBUG=1 cargo run
```

This will print detailed information about every macro expansion, including:
- Which macro is being expanded
- The input form
- Which pattern rules are tried
- Pattern matching results
- Template expansion output
- Hygiene renaming

### Trace Specific Macros

For more focused debugging, use `MacroTracer` in your code:

```scheme
; Enable tracing for specific macros
(begin
  ; Your code here - these macros will be traced
  (let-values (((a b) (values 1 2)))
    (list a b)))
```

Then in Rust:

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

## MACRO_DEBUG Environment Variable

The `MACRO_DEBUG` environment variable enables comprehensive logging of all macro expansions.

### Usage

```bash
# Enable macro debugging
export MACRO_DEBUG=1

# Run your program
./patina my_program.scm

# Or combine with cargo run
MACRO_DEBUG=1 cargo run -- my_program.scm
```

### Example Output

```
[MACRO] Expanding macro: let-values
[MACRO]   Definition (1 rule(s)):
[MACRO]     Rule 1: ((let-values (binding ...) body1 body2 ...)) -> <template>
[MACRO]
[MACRO]   Input: ((let-values (((a b) (values 1 2))) (list a b)))
[MACRO]   Trying 1 rule(s):
[MACRO]
[MACRO]   === Trying rule 1 ===
[MACRO]   Pattern: ((let-values (binding ...) body1 body2 ...))
[MACRO]   ✓ Match successful!
[MACRO]
[MACRO]   === Expanding template ===
[MACRO]   Template: (call-with-values (lambda () (values 1 2)) (lambda (a b) (begin (list a b))))
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
6. **Hygiene**: Any identifier renamings that occurred

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

// This prevents infinite recursion when debugging recursive macros
```

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

1. Enable `MACRO_DEBUG`:
   ```bash
   MACRO_DEBUG=1 ./patina
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

1. Enable `MACRO_DEBUG` to see which rules are tried:
   ```
   [MACRO]   === Trying rule 1 ===
   [MACRO]   Pattern: ((my-macro x y))
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

### Scenario 3: Wrong Variables Captured (Hygiene Issue)

**Problem**: Macro is using wrong binding.

**Example**:
```scheme
(let ((x 'outer))
  (let-syntax ((m (syntax-rules () ((m) x))))
    (let ((x 'inner))
      (m))))  ; Should return 'outer, but returns 'inner?
```

**Debug Steps**:

1. Enable `MACRO_DEBUG` and look for hygiene messages:
   ```
   [HYGIENE] Not renaming 'x' - free variable in template (lexical scoping)
   ```

2. Check if `x` has a captured environment. Free variables should use definition-time bindings.

3. The debug output will show if identifiers are being renamed or using captured environments.

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
Macro expansion error: Undefined free variable in macro template: foo

  in macro: my-macro

Expansion trace:
  let*-values (rule 1/1): ((let*-values ...))
  my-macro (rule 1/1): ((my-macro x y))
```

This tells you:
- **What went wrong**: Undefined variable `foo`
- **Where**: In macro `my-macro`
- **How you got there**: Via `let*-values` which expanded to `my-macro`

### Accessing Error Context in Code

```rust
match interpreter.eval_str(code) {
    Err(e) => {
        // Get formatted error with trace
        let error_msg = e.format_with_trace();
        eprintln!("{}", error_msg);
    }
    Ok(result) => {
        println!("Result: {}", result);
    }
}
```

## Best Practices

1. **Start Small**: Debug macros in isolation before using them in complex code.

2. **Use MacroTracer for Specific Macros**: Instead of `MACRO_DEBUG` which shows everything, use `MacroTracer` to focus on problematic macros.

3. **Check Pattern Variables**: Ensure pattern variables match the expected structure.

4. **Test Hygiene**: Verify that free variables capture the right bindings.

5. **Limit Trace Depth**: When debugging recursive macros, use `set_max_depth()` to prevent overwhelming output.

6. **Read Traces Bottom-Up**: The last expansion in the trace is often where the error occurs.

## API Reference

### MacroTracer Methods

```rust
// Enable/disable tracing
MacroTracer::enable_for(&["macro1", "macro2"])
MacroTracer::enable_all()
MacroTracer::disable()

// Control depth
MacroTracer::set_max_depth(depth: usize)
MacroTracer::current_depth() -> usize

// Get trace data
MacroTracer::get_history() -> Vec<ExpansionStep>
MacroTracer::print_history()

// Clear history
MacroTracer::clear()
```

### ExpansionStep Fields

```rust
pub struct ExpansionStep {
    pub macro_name: Rc<str>,
    pub rule_index: usize,
    pub total_rules: usize,
    pub input: String,
}
```

### MacroError Methods

```rust
// Add context to an error
error.with_context(macro_name, expansion_trace)

// Format with full trace
error.format_with_trace() -> String
```

## Examples

### Example 1: Debug a Failing Macro

```rust
use patina_interpreter::Interpreter;
use patina_macros::MacroTracer;

let interp = Interpreter::new();

// Define a macro
interp.eval_str(r#"
(define-syntax broken-macro
  (syntax-rules ()
    ((broken-macro x) (undefined-function x))))
"#)?;

// Enable tracing
MacroTracer::enable_for(&["broken-macro"]);

// Try to use it (will fail)
match interp.eval_str("(broken-macro 42)") {
    Err(e) => {
        eprintln!("Error: {}", e.format_with_trace());
        MacroTracer::print_history();
    }
    Ok(_) => println!("Success"),
}
```

### Example 2: Trace Macro Expansion Chain

```rust
MacroTracer::enable_for(&["let*-values", "let-values"]);

interp.eval_str(r#"
(let*-values (((a b) (values 1 2))
              ((c d) (values a b)))
  (list c d))
"#)?;

// See how let*-values expands to let-values
for step in MacroTracer::get_history() {
    println!("{}", step);
}
```

## See Also

- [Macro Architecture Review](MACRO_ARCHITECTURE_REVIEW.md) - Deep dive into macro system internals
- [Hygiene Research](../internal/HYGIENE_RESEARCH.md) - How hygienic macro expansion works
- [Feature Status](FEATURE_STATUS.md) - Which macros are implemented
