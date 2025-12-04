# Built-in Help System PRD

**Status:** Planned (Post Phase 1)
**Priority:** Medium (After bare-minimum R7RS compliance)
**Focus:** Usability & Debuggability

## Overview

Add a comprehensive built-in help system to make Patina more user-friendly and self-documenting. Users should be able to discover and learn about Scheme procedures directly from the REPL without consulting external documentation.

## Motivation

**Current Problem:**
- Users need to consult external R7RS spec or documentation
- No way to discover what procedures are available
- No examples or usage patterns in the REPL
- Difficult for beginners to learn Scheme interactively

**Target Use Cases:**
1. **Learning**: New Scheme users exploring the language
2. **Discovery**: Finding what procedures exist for a task
3. **Reference**: Quick lookup of syntax and examples
4. **Debugging**: Understanding procedure behavior and edge cases

## Design

### User Interface

```scheme
;; Show overview of help system
(help)

;; Show help for a specific procedure
(help 'map)
(help 'cons)
(help '+)

;; List procedures by category
(help 'category 'list)
(help 'category 'numeric)

;; Search for procedures
(help 'search "list")
```

### Example Output

```
> (help 'map)
map - List Operations (R7RS §6.4)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

SIGNATURE:
  (map proc list1 list2 ...)

ARGUMENTS:
  proc   - A procedure that accepts as many arguments as there are lists
  list1+ - One or more lists to process

RETURNS:
  A new list containing the results of applying proc to corresponding
  elements from the input lists.

DESCRIPTION:
  Applies proc element-wise to corresponding elements from all lists.
  Returns a list of the results in order. Terminates when the shortest
  list runs out.

EXAMPLES:
  (map (lambda (x) (* x 2)) '(1 2 3))
  => (2 4 6)

  (map + '(1 2 3) '(4 5 6))
  => (5 7 9)

  (map car '((a b) (c d) (e f)))
  => (a c e)

  ; Multiple lists - stops at shortest
  (map + '(1 2 3) '(10 20))
  => (11 22)

BEHAVIOR NOTES:
  • Terminates when the shortest list is exhausted
  • Lists can be circular (but not all of them)
  • Evaluation order of proc applications is unspecified
  • It is an error for proc to mutate any of the input lists

RELATED:
  for-each  - Like map but for side effects, returns unspecified
  apply     - Apply procedure to list of arguments
  filter    - Select elements matching a predicate (SRFI 1)
  fold      - Reduce list to single value (SRFI 1)

> (help)
Patina Scheme Help System
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

USAGE:
  (help)              Show this help message
  (help 'name)        Show help for a specific procedure
  (help 'category 'X) List procedures in category X
  (help 'search "X")  Search for procedures matching X

CATEGORIES:
  primitive    - Primitive expressions (quote, if, lambda, etc.)
  control      - Control flow (cond, and, or, apply, etc.)
  list         - List operations (car, cdr, map, append, etc.)
  numeric      - Numeric operations (+, -, *, abs, etc.)
  predicate    - Type predicates and equality (eq?, equal?, etc.)
  string       - String operations (planned)
  vector       - Vector operations (planned)
  io           - Input/output (planned)

EXAMPLES:
  (help 'map)
  (help 'category 'list)
  (help 'search "list")

For R7RS documentation: https://small.r7rs.org/
```

## Implementation Architecture

### Data Structure

```rust
// src/help/mod.rs

/// Documentation for a single procedure
pub struct ProcedureDoc {
    /// Procedure name (e.g., "map", "cons", "+")
    pub name: &'static str,

    /// Human-readable signature showing parameter names
    pub signature: &'static str,

    /// R7RS category (list, numeric, control, etc.)
    pub category: Category,

    /// R7RS section reference (e.g., "§6.4")
    pub section: &'static str,

    /// Brief one-line description
    pub summary: &'static str,

    /// Detailed multi-paragraph description
    pub description: &'static str,

    /// Parameter descriptions
    pub parameters: Vec<Parameter>,

    /// Return value description
    pub returns: &'static str,

    /// Code examples with expected output
    pub examples: Vec<Example>,

    /// Important behavioral notes, edge cases, errors
    pub notes: Vec<&'static str>,

    /// Related procedures
    pub related: Vec<&'static str>,
}

pub struct Parameter {
    pub name: &'static str,
    pub description: &'static str,
}

pub struct Example {
    pub code: &'static str,
    pub output: &'static str,
    pub explanation: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Primitive,      // quote, if, lambda, define, set!, begin
    Control,        // cond, and, or, apply, map, for-each
    List,           // cons, car, cdr, append, reverse, etc.
    Numeric,        // +, -, *, /, abs, quotient, etc.
    Predicate,      // eq?, equal?, null?, pair?, etc.
    String,         // string operations
    Vector,         // vector operations
    Character,      // character operations
    IO,             // input/output
    Exception,      // error handling
}
```

### Help Database

```rust
// src/help/database.rs

use once_cell::sync::Lazy;
use std::collections::HashMap;

/// Global help database, lazily initialized
pub static HELP_DB: Lazy<HashMap<&'static str, ProcedureDoc>> = Lazy::new(|| {
    let mut db = HashMap::new();

    // Populate with all documented procedures
    for doc in PROCEDURE_DOCS {
        db.insert(doc.name, doc);
    }

    db
});

/// All procedure documentation entries
static PROCEDURE_DOCS: &[ProcedureDoc] = &[
    // List operations
    ProcedureDoc {
        name: "map",
        signature: "(map proc list1 list2 ...)",
        category: Category::List,
        section: "§6.4",
        summary: "Apply procedure to corresponding elements of lists",
        description: "Applies proc element-wise to the elements of the lists \
                      and returns a list of the results, in order. If more than \
                      one list is given and not all lists have the same length, \
                      map terminates when the shortest list runs out.",
        parameters: vec![
            Parameter {
                name: "proc",
                description: "A procedure accepting as many arguments as there are lists",
            },
            Parameter {
                name: "list1+",
                description: "One or more lists to process in parallel",
            },
        ],
        returns: "A new list containing the results in order",
        examples: vec![
            Example {
                code: "(map (lambda (x) (* x 2)) '(1 2 3))",
                output: "(2 4 6)",
                explanation: Some("Double each element"),
            },
            Example {
                code: "(map + '(1 2 3) '(4 5 6))",
                output: "(5 7 9)",
                explanation: Some("Add corresponding elements"),
            },
        ],
        notes: vec![
            "Terminates when the shortest list is exhausted",
            "Lists can be circular, but not all of them",
            "Evaluation order is unspecified",
            "It is an error for proc to mutate the lists",
        ],
        related: vec!["for-each", "apply", "filter"],
    },

    // More entries...
];
```

### Primitive Implementation

```rust
// src/eval/mod.rs

fn primitive_help(&self, args: Vec<Value>) -> Result<Value, EvalError> {
    use crate::help::{HELP_DB, format_help, format_overview, search_procedures};

    match args.len() {
        0 => {
            // (help) - Show overview
            let overview = format_overview();
            println!("{}", overview);
            Ok(Value::Unspecified)
        }
        1 => {
            match &args[0] {
                Value::Symbol(name) => {
                    // (help 'map) - Show specific help
                    if let Some(doc) = HELP_DB.get(name.as_ref()) {
                        let formatted = format_help(doc);
                        println!("{}", formatted);
                        Ok(Value::Unspecified)
                    } else {
                        Err(EvalError::TypeError(
                            format!("No help available for '{}'", name)
                        ))
                    }
                }
                _ => Err(EvalError::TypeError(
                    "help: argument must be a symbol".to_string()
                ))
            }
        }
        2 => {
            // (help 'category 'list) or (help 'search "text")
            match (&args[0], &args[1]) {
                (Value::Symbol(cmd), Value::Symbol(arg)) if cmd.as_ref() == "category" => {
                    // List procedures in category
                    // TODO: implement category listing
                    Ok(Value::Unspecified)
                }
                (Value::Symbol(cmd), Value::String(query)) if cmd.as_ref() == "search" => {
                    // Search for procedures
                    let results = search_procedures(query);
                    // TODO: format and display results
                    Ok(Value::Unspecified)
                }
                _ => Err(EvalError::TypeError(
                    "help: invalid arguments".to_string()
                ))
            }
        }
        _ => Err(EvalError::WrongArity {
            expected: "0-2".to_string(),
            actual: args.len(),
        })
    }
}
```

### Display Formatting

```rust
// src/help/display.rs

use nu_ansi_term::{Color, Style};

/// Format procedure documentation for display
pub fn format_help(doc: &ProcedureDoc) -> String {
    let mut output = String::new();

    // Title with category
    let title = format!("{} - {} {}",
        doc.name,
        format_category(doc.category),
        doc.section
    );
    output.push_str(&Color::Cyan.bold().paint(title).to_string());
    output.push_str("\n");
    output.push_str(&"━".repeat(60));
    output.push_str("\n\n");

    // Signature
    output.push_str(&Color::Green.bold().paint("SIGNATURE:").to_string());
    output.push_str("\n  ");
    output.push_str(doc.signature);
    output.push_str("\n\n");

    // Description
    output.push_str(&Color::Green.bold().paint("DESCRIPTION:").to_string());
    output.push_str("\n  ");
    output.push_str(&wrap_text(doc.description, 70));
    output.push_str("\n\n");

    // Examples
    if !doc.examples.is_empty() {
        output.push_str(&Color::Green.bold().paint("EXAMPLES:").to_string());
        output.push_str("\n");
        for example in &doc.examples {
            output.push_str("  ");
            output.push_str(example.code);
            output.push_str("\n  => ");
            output.push_str(example.output);
            output.push_str("\n\n");
        }
    }

    // Notes
    if !doc.notes.is_empty() {
        output.push_str(&Color::Green.bold().paint("BEHAVIOR NOTES:").to_string());
        output.push_str("\n");
        for note in doc.notes {
            output.push_str("  • ");
            output.push_str(note);
            output.push_str("\n");
        }
        output.push_str("\n");
    }

    // Related
    if !doc.related.is_empty() {
        output.push_str(&Color::Green.bold().paint("RELATED:").to_string());
        output.push_str("\n  ");
        output.push_str(&doc.related.join(", "));
        output.push_str("\n");
    }

    output
}

fn wrap_text(text: &str, width: usize) -> String {
    // TODO: Implement word wrapping
    text.replace("\n", "\n  ")
}
```

## Data Sources

### Primary Sources (in order of priority)

1. **R7RS Specification** (`spec/r7rs-small-spec/procs.tex`)
   - Official descriptions
   - Parameter specifications
   - Behavioral notes
   - Error conditions

2. **Chibi Test Suite** (`reference/chibi-scheme/tests/r7rs-tests.scm`)
   - Verified working examples
   - Edge cases
   - Expected outputs

3. **Chibi init-7.scm** (`reference/chibi-scheme/lib/init-7.scm`)
   - Implementation notes
   - Common patterns

### Documentation Coverage Plan

**Phase 1: Core Procedures (~30 entries)**
- Essential special forms: quote, if, lambda, define, set!, begin
- Control: cond, and, or, let, let*, letrec, apply, map, for-each
- Lists: cons, car, cdr, list, append, reverse, length, list-ref
- Numeric: +, -, *, /, =, <, >, abs
- Predicates: eq?, eqv?, equal?, null?, pair?, list?

**Phase 2: Extended Procedures (~50 entries)**
- More list operations
- Full numeric tower
- String operations
- Character operations

**Phase 3: Complete R7RS (~100+ entries)**
- Vector operations
- I/O operations
- Exception handling
- Advanced features

## Integration

### Bootstrap Integration

Add to `lib/bootstrap.scm`:

```scheme
;; Help system wrapper (actual implementation in Rust primitive)
;; This is just to make (help) feel more "native"
;; The real work is done by the primitive_help Rust function
```

### REPL Integration

The help system integrates naturally with the existing REPL:
- No special command prefix needed (uses Scheme syntax)
- Output uses existing REPL display mechanisms
- Syntax highlighting via `nu-ansi-term` (already used in REPL)

## Future Enhancements

### Interactive Mode
```scheme
(help 'interactive)  ; Enter interactive help browser
```

### Examples Can Be Run
```scheme
(help 'map 'try 0)  ; Run the first example
```

### Custom Documentation
```scheme
(define-help 'my-procedure
  '((signature "(my-procedure x y)")
    (description "Does something cool")
    (examples ("(my-procedure 1 2)" "3"))))
```

### Web Export
Generate HTML documentation from the help database for hosting online.

## Implementation Checklist

- [ ] Create `src/help/mod.rs` module structure
- [ ] Define `ProcedureDoc` and related data structures
- [ ] Create help database with initial 10 procedures
- [ ] Implement `primitive_help` in evaluator
- [ ] Register `help` as a primitive procedure
- [ ] Implement display formatting with colors
- [ ] Add documentation for 30 core procedures
- [ ] Implement category listing
- [ ] Implement search functionality
- [ ] Add help system to test suite
- [ ] Document help system usage in README

## Dependencies

- `nu-ansi-term` - Already in use for REPL syntax highlighting
- `once_cell` - For lazy static initialization (add to Cargo.toml)

## Success Criteria

1. **Usability**: New users can learn Scheme without leaving the REPL
2. **Completeness**: All implemented procedures have documentation
3. **Accuracy**: All examples are tested and verified
4. **Performance**: Help lookup is instant (< 1ms)
5. **Maintainability**: Easy to add new documentation entries

## Timeline Estimate

- **Setup & Infrastructure**: 2-3 hours
- **Initial 10 procedures**: 2 hours
- **30 core procedures**: 4-6 hours
- **Testing & Polish**: 2 hours
- **Total**: ~10-15 hours of focused work

## Notes

- Implement after Phase 1 (bare minimum R7RS) is complete
- Focus on usability and discoverability
- Keep documentation concise but complete
- All examples should be runnable and tested
- Consider this a living document - update as implementation evolves

## References

- R7RS Specification: `spec/r7rs-small-spec/procs.tex`
- Racket XREPL: https://docs.racket-lang.org/xrepl/
- Chibi Documentation: `reference/chibi-scheme/lib/chibi/doc.scm`
