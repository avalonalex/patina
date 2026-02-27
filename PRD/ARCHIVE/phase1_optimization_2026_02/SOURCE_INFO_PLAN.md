# Source Information Tracking - Implementation Plan

**Status:** Planning
**Priority:** High - Critical for debugging and error reporting
**Created:** 2025-11-19

## Problem Statement

Currently, Patina loses all source location information when parsing Scheme code. When the lexer/parser processes source code, they build an AST (represented as `Value` enum), but they don't preserve:

- **File name** - Which file the code came from
- **Line number** - Which line in the source
- **Column number** - Which column/character position
- **Span** - The start and end positions of an expression

This makes debugging difficult because:
1. Error messages can't point to exact source locations
2. Stack traces don't show where code was defined
3. Macro expansion errors are hard to diagnose
4. IDE integration (future) requires source mapping

**Example of current behavior:**
```
Error: Unbound variable: x
```

**Desired behavior:**
```
Error: Unbound variable: x
  at example.scm:15:8
     (define (foo) x)
                   ^
```

## Architecture Analysis

### Current Flow

```
Source String
    ↓
Lexer → Token (no source info)
    ↓
Parser → Value (AST, no source info)
    ↓
Macro Expander → Value (transformed AST, no source info)
    ↓
Evaluator → Value (runtime value)
```

### Key Observations

1. **Value enum serves dual purpose**: AST during parsing/macro expansion, runtime value during evaluation
2. **Token has no location tracking**: Currently just data (Boolean, Number, String, Identifier, etc.)
3. **Parser immediately discards tokens**: After extracting data, position info is lost
4. **Macro expansion complicates tracking**: Generated code needs to track origin of template fragments

## Design Options

### Option A: Wrapper Type (Syntax Objects) - RECOMMENDED

**Approach**: Create a `Syntax` type that wraps `Value` + `SourceInfo`

```rust
// In patina-runtime/src/syntax.rs
#[derive(Debug, Clone)]
pub struct Syntax {
    pub value: Value,
    pub source: Option<SourceInfo>,
}

#[derive(Debug, Clone)]
pub struct SourceInfo {
    pub file: Option<Rc<str>>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Copy)]
pub struct SourceSpan {
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
    pub start_offset: usize,  // byte offset in file
    pub end_offset: usize,
}
```

**Pros:**
- ✅ Non-breaking: Can introduce gradually
- ✅ Clear separation: Source info is metadata, not core data
- ✅ Flexible: Can attach/detach source info as needed
- ✅ Similar to Racket's approach (syntax objects)
- ✅ Runtime values don't carry unnecessary metadata

**Cons:**
- ⚠️ More code changes: Need to update many function signatures
- ⚠️ Type complexity: `Syntax` vs `Value` distinction
- ⚠️ Unwrapping needed: Must extract `Value` when source info not needed

### Option B: Extend Value Enum

**Approach**: Add source info as a field in every `Value` variant

```rust
pub enum Value {
    Boolean(bool, Option<SourceInfo>),
    Integer(i64, Option<SourceInfo>),
    // ... all other variants
}
```

**Pros:**
- ✅ Simple conceptually: Everything has source info

**Cons:**
- ❌ BREAKING: Changes all Value constructors
- ❌ Memory overhead: Source info on runtime values
- ❌ Conceptual confusion: Runtime values shouldn't have source info
- ❌ Pattern matching becomes cumbersome

### Option C: Side Table Mapping

**Approach**: Maintain a separate HashMap mapping Value → SourceInfo

**Pros:**
- ✅ Non-invasive to Value definition

**Cons:**
- ❌ Identity/equality issues: Values are cloned frequently
- ❌ Lifetime/ownership complexity
- ❌ Performance: HashMap lookups
- ❌ Hard to maintain consistency

## Recommended Approach: Option A (Syntax Objects)

### Architecture After Implementation

```
Source String + File Path
    ↓
Lexer → Token { data, span }
    ↓
Parser → Syntax { value: Value, source: SourceInfo }
    ↓
Macro Expander → Syntax (preserves source info through expansion)
    ↓
Evaluator → Value (strips source info during evaluation)
```

### Key Principles

1. **Syntax during compilation**: Use `Syntax` in frontend (lexer, parser, macro expander)
2. **Value during evaluation**: Evaluator works with `Value`, not `Syntax`
3. **Preserve through macros**: Macro expansion maintains source info from original code
4. **Strip at eval boundary**: Convert `Syntax` → `Value` when entering evaluator

## Implementation Phases

### Phase 1: Foundation (Non-Breaking)

**Goal**: Add new types without breaking existing code

**Tasks:**
1. Create `patina-runtime/src/syntax.rs`:
   - Define `SourceInfo` struct
   - Define `SourceSpan` struct
   - Define `Syntax` struct with `From<Value>` conversions
   - Add helper methods: `with_source()`, `without_source()`, `map_value()`

2. Update `Token` in `patina-frontend/src/lexer/mod.rs`:
   - Add optional `span: Option<SourceSpan>` field to Token
   - Update lexer to track position (line, column, byte offset)
   - Add methods: `current_position()`, `span_from(start)`

3. Add test for source tracking:
   - Create `crates/patina-frontend/tests/source_tracking.rs`
   - Test that lexer correctly tracks positions
   - Test that parser preserves source info

**Compatibility**: 100% backward compatible. Existing code continues to work.

### Phase 2: Parser Integration (Minimal Breaking)

**Goal**: Make parser produce `Syntax` instead of `Value`

**Tasks:**
1. Update `Parser` public API:
   - Change `parse() -> Result<Value>` to `parse() -> Result<Syntax>`
   - Change `parse_expr()` to return `Syntax` internally
   - Track source spans for each expression parsed
   - For compound expressions (lists, vectors), span covers entire expression

2. Update `patina-interpreter`:
   - Add conversion layer: `Syntax` → `Value` before passing to evaluator
   - This maintains evaluator API compatibility

3. Update tests:
   - Frontend tests that check parsed results
   - Add `.value` accessor where tests expect `Value`

**Breaking Changes**: Only parser public API changes. Tests need minor updates.

### Phase 3: Macro Expander Integration (Medium Breaking)

**Goal**: Preserve source info through macro expansion

**Tasks:**
1. Update macro expander to work with `Syntax`:
   - Pattern matching works on `syntax.value`
   - Template expansion preserves source info from pattern matches
   - Generated code gets source info from the macro use site

2. Implement hygiene-aware source tracking:
   - Generated identifiers (gensyms) track macro expansion site
   - Original identifiers keep their original source info

3. Update `eval_step_impl` macro handling:
   - When evaluating macro invocations, preserve source context
   - Error messages can show both macro use and definition sites

**Breaking Changes**: Macro expander API changes. Internal to frontend.

### Phase 4: Error Reporting Integration

**Goal**: Use source info in error messages

**Tasks:**
1. Update `EvalError` in `patina-tree-walker/src/eval/error.rs`:
   - Add optional `source: Option<SourceInfo>` field
   - Implement `Display` to show source context

2. Update evaluator to capture source info:
   - Pass `Syntax` deeper into evaluator (instead of immediate conversion)
   - Attach source info to errors when available

3. Create error formatting utilities:
   - Pretty-print source context (show relevant lines)
   - Point to exact column with caret (^)
   - Support multi-line expressions

4. Update all error creation sites:
   - Add source context where available
   - Maintain backward compatibility for errors without source

**Breaking Changes**: Error type changes. Mostly internal.

### Phase 5: REPL Integration

**Goal**: Source tracking works seamlessly in the REPL

**Tasks:**
1. Update `Repl` struct:
   - Add `expression_counter: usize` field
   - Generate synthetic source names: `"<repl-N>"`

2. Add new interpreter method:
   - `eval_str_with_source(input: &str, source_name: &str)`
   - Keep backward-compatible `eval_str()` calling with `"<input>"`

3. Update REPL loop:
   - Pass synthetic source name to interpreter
   - Error messages automatically include source location

4. Optional: Expression history tracking:
   - Store mapping `"<repl-N>" -> source_code`
   - Display full context when showing old errors

**Breaking Changes**: None (REPL is user-facing, API unchanged)

### Phase 6: Stack Trace Enhancement (Future)

**Goal**: Show source locations in stack traces

**Tasks:**
1. Track call stack with source info:
   - Maintain stack of `(function_name, source_info)` during evaluation
   - Show full trace on errors

2. Lambda source tracking:
   - `Procedure::Lambda` stores source info of definition site
   - Stack traces show where functions were defined

3. Advanced REPL features:
   - Show call stacks spanning multiple REPL expressions
   - Cross-reference between REPL definitions

**Breaking Changes**: Error type changes. Mostly internal.

## Multi-Backend Considerations

### Why Generic Syntax?

Patina's architecture supports multiple evaluation backends (tree-walker, VM, JIT). Making `Syntax` generic over the data type provides several benefits:

**1. Backend-Specific Representations**

Different backends need different internal representations:

```rust
// Tree-walker: Uses Value enum as AST
type TreeWalkerSyntax = Syntax<Value>;

// VM backend: Uses bytecode with source mapping
type VMSyntax = Syntax<VMInstruction>;

// JIT backend: Uses IR nodes
type JITSyntax = Syntax<IRNode>;
```

**2. Source Tracking Through Compilation Pipelines**

Source info can flow through multi-stage compilation:

```
Source → Syntax<Value> → [Macro Expand] → Syntax<Value>
       → [Compile to VM] → Syntax<VMInstruction>
       → [Optimize] → Syntax<VMInstruction>
       → [Emit] → Bytecode + SourceMap
```

Each transformation preserves source info for debugging.

**3. Common Error Reporting Infrastructure**

All backends share the same `SourceInfo` and `SourceSpan` types, enabling:
- Consistent error messages across backends
- Single implementation of pretty-printing
- Unified stack trace formatting

### Design Decision: Generic with Default

```rust
pub struct Syntax<T = Value> {
    pub data: T,
    pub source: Option<SourceInfo>,
}
```

**The default type parameter `T = Value` means:**
- Phase 1-5: Can write `Syntax` and it means `Syntax<Value>`
- Future: When adding VM backend, use `Syntax<VMInstruction>` explicitly
- Migration is gradual - existing code doesn't break

**Field naming: `data` instead of `value`**
- `value` would be misleading for `Syntax<VMInstruction>`
- `data` is generic and works for any backend
- For tree-walker, `syntax.data` is the `Value` enum

### Backend Pipeline Example

**Tree-walker (current):**
```rust
source → Parser → Syntax<Value> → eval() → Value
```

**Future VM backend:**
```rust
source → Parser → Syntax<Value>
       → Compiler → Syntax<VMInstruction>
       → VM::execute() → Value
```

**Future JIT backend:**
```rust
source → Parser → Syntax<Value>
       → Analyzer → Syntax<TypedIR>
       → JIT::compile() → NativeCode
       → execute() → Value
```

Each stage maintains source info for debugging.

### Alternative: Non-Generic Syntax

An alternative design would be to keep `Syntax` specific to `Value`:

```rust
// Non-generic version
pub struct Syntax {
    pub value: Value,
    pub source: Option<SourceInfo>,
}

// VM backend would need its own type
pub struct VMSyntax {
    pub instruction: VMInstruction,
    pub source: Option<SourceInfo>,
}
```

**Why we chose generic instead:**
- ✅ Avoids code duplication (each backend reimplements source tracking)
- ✅ Generic `map()` and utility methods work for all backends
- ✅ Error types can be generic over `Syntax<T>`
- ✅ Compiler infrastructure can be polymorphic
- ✅ Type safety: Can't accidentally mix different representations

**Trade-off:**
- ⚠️ Slightly more complex type signatures
- ⚠️ Field name `data` instead of `value` (less specific)

The benefits outweigh the costs, especially for long-term multi-backend support.

## API Design

### Core Types

```rust
// patina-runtime/src/syntax.rs

/// Source information for an expression
#[derive(Debug, Clone)]
pub struct SourceInfo {
    /// Optional file path (None for REPL input, synthetic macros)
    pub file: Option<Rc<str>>,
    /// Span within the file
    pub span: SourceSpan,
}

/// Position span in source code
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceSpan {
    /// Starting line (1-indexed)
    pub start_line: usize,
    /// Starting column (1-indexed)
    pub start_col: usize,
    /// Ending line (1-indexed)
    pub end_line: usize,
    /// Ending column (1-indexed, exclusive)
    pub end_col: usize,
    /// Starting byte offset in file (0-indexed)
    pub start_offset: usize,
    /// Ending byte offset in file (0-indexed, exclusive)
    pub end_offset: usize,
}

/// Syntax object: Generic data annotated with source information
///
/// This type is generic to support multiple backend representations:
/// - Tree-walker: `Syntax<Value>` (current AST as Value enum)
/// - VM backend: `Syntax<VMInstruction>` (bytecode with source mapping)
/// - JIT backend: `Syntax<IRNode>` (intermediate representation)
///
/// For Phase 1 implementation, we use a type alias to `Value` for simplicity,
/// but the generic design enables future multi-backend support.
#[derive(Debug, Clone)]
pub struct Syntax<T = Value> {
    /// The actual data (AST node, bytecode, IR, etc.)
    pub data: T,
    /// Optional source information (None for runtime-generated values)
    pub source: Option<SourceInfo>,
}

impl<T> Syntax<T> {
    /// Create syntax with source info
    pub fn new(data: T, source: Option<SourceInfo>) -> Self {
        Syntax { data, source }
    }

    /// Create syntax without source info
    pub fn without_source(data: T) -> Self {
        Syntax { data, source: None }
    }

    /// Create syntax with source info
    pub fn with_source(data: T, source: SourceInfo) -> Self {
        Syntax { data, source: Some(source) }
    }

    /// Map the data while preserving source info
    pub fn map<F, U>(self, f: F) -> Syntax<U>
    where
        F: FnOnce(T) -> U,
    {
        Syntax {
            data: f(self.data),
            source: self.source,
        }
    }

    /// Unwrap to get the data, discarding source info
    pub fn into_inner(self) -> T {
        self.data
    }

    /// Get a reference to the inner data
    pub fn as_ref(&self) -> &T {
        &self.data
    }
}

// Convenience type alias for current tree-walker implementation
// This makes migration easier - code can use `ValueSyntax` initially,
// then switch to generic `Syntax<T>` when adding new backends
pub type ValueSyntax = Syntax<Value>;

// Conversion from Value (for backward compatibility)
impl From<Value> for ValueSyntax {
    fn from(value: Value) -> Self {
        Syntax::without_source(value)
    }
}
```

### Lexer Changes

```rust
// patina-frontend/src/lexer/mod.rs

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Same variants as current Token
    LeftParen,
    RightParen,
    Boolean(bool),
    Number(String),
    // ... etc
}

pub struct Lexer {
    input: Vec<char>,
    position: usize,
    line: usize,        // 1-indexed
    column: usize,      // 1-indexed
    line_start: usize,  // byte offset of current line start
}

impl Lexer {
    fn current_position(&self) -> (usize, usize, usize) {
        (self.line, self.column, self.position)
    }

    fn span_from(&self, start: (usize, usize, usize)) -> SourceSpan {
        SourceSpan {
            start_line: start.0,
            start_col: start.1,
            end_line: self.line,
            end_col: self.column,
            start_offset: start.2,
            end_offset: self.position,
        }
    }
}
```

### Parser Changes

```rust
// patina-frontend/src/parser/mod.rs

pub struct Parser {
    lexer: Lexer,
    current_token: Token,
    file: Option<Rc<str>>,  // Source file path
}

impl Parser {
    pub fn new(input: &str) -> Result<Self, ParseError> {
        // ...
    }

    pub fn new_with_file(input: &str, file: impl Into<Rc<str>>) -> Result<Self, ParseError> {
        // ...
    }

    // Primary API - returns Syntax<Value> (using default type parameter)
    pub fn parse(&mut self) -> Result<Syntax, ParseError> {
        self.parse_expr()
    }

    fn parse_expr(&mut self) -> Result<Syntax, ParseError> {
        let start_span = self.current_token.span;
        let value = match &self.current_token.kind {
            TokenKind::Boolean(b) => {
                let val = Value::Boolean(*b);
                self.advance()?;
                val
            }
            // ... other cases
        };
        let end_span = self.current_token.span;
        let span = merge_spans(start_span, end_span);
        Ok(Syntax::with_source(value, SourceInfo {
            file: self.file.clone(),
            span,
        }))
    }
}

// Note: `Syntax` without type parameter defaults to `Syntax<Value>`
// This keeps the API simple while supporting future generic use
```

### Using Syntax in Code

```rust
// Reading the AST data (tree-walker backend)
let syntax: Syntax = parser.parse()?;
let value: &Value = &syntax.data;  // or syntax.as_ref()

// Transforming while preserving source
let new_syntax = syntax.map(|value| {
    // Transform the value
    expand_macro(value)
});

// Converting to runtime value (strips source info)
let runtime_value = syntax.into_inner();

// For future VM backend
fn compile_to_vm(syntax: Syntax) -> Syntax<VMInstruction> {
    syntax.map(|value| {
        // Compile Value to VMInstruction
        compile(value)
    })
    // Source info is preserved through transformation!
}
```

## Testing Strategy

### Unit Tests

1. **Source span tracking** (`patina-frontend/tests/source_tracking.rs`):
   ```rust
   #[test]
   fn test_simple_expression_span() {
       let mut parser = Parser::new_with_file("42", "test.scm").unwrap();
       let syntax = parser.parse().unwrap();
       assert_eq!(syntax.source.unwrap().span.start_line, 1);
       assert_eq!(syntax.source.unwrap().span.start_col, 1);
   }

   #[test]
   fn test_list_expression_span() {
       let mut parser = Parser::new_with_file("(+ 1 2)", "test.scm").unwrap();
       let syntax = parser.parse().unwrap();
       // Span should cover entire list
       let span = syntax.source.unwrap().span;
       assert_eq!(span.start_col, 1);
       assert_eq!(span.end_col, 8);
   }
   ```

2. **Error reporting** (`patina-tree-walker/tests/error_messages.rs`):
   ```rust
   #[test]
   fn test_error_with_source_info() {
       let mut interp = Interpreter::new();
       let result = interp.eval_file("(+ x 1)", "test.scm");
       assert!(result.is_err());
       let err = result.unwrap_err();
       // Check that error message includes source location
       assert!(err.to_string().contains("test.scm:1:4"));
   }
   ```

### Integration Tests

1. Test source tracking through macro expansion
2. Test multi-line expression spans
3. Test REPL input tracking
4. Test error messages at various stages (parse, macro expand, eval)

## Migration Path for Existing Code

### Phase 1: Parallel API

```rust
// Parser provides both APIs temporarily
impl Parser {
    // New API (returns Syntax)
    pub fn parse(&mut self) -> Result<Syntax, ParseError> {
        // ...
    }

    // Deprecated legacy API (returns Value)
    #[deprecated(note = "Use parse() which returns Syntax")]
    pub fn parse_value(&mut self) -> Result<Value, ParseError> {
        Ok(self.parse()?.into_value())
    }
}
```

### Phase 2: Update Call Sites

Update call sites one-by-one:
```rust
// Before:
let value = parser.parse()?;
eval(value, env)

// After:
let syntax = parser.parse()?;
eval(syntax.data, env)
// Or keep syntax for better errors:
eval_with_source(syntax, env)

// Or use into_inner() for clarity:
let syntax = parser.parse()?;
eval(syntax.into_inner(), env)
```

### Phase 3: Remove Legacy API

Once all call sites updated, remove deprecated methods.

## Performance Considerations

### Memory Overhead

- **SourceSpan**: 6 × usize = 48 bytes (on 64-bit)
- **SourceInfo**: 48 + Rc overhead ≈ 64 bytes
- **Syntax**: Value + Option<SourceInfo> ≈ Value size + 72 bytes

**Mitigation:**
- Source info only present during parsing/macro expansion
- Stripped before evaluation (runtime values don't carry it)
- Rc sharing means identical source files share file path string

### Performance Impact

- **Parsing**: Minimal (just tracking position, which is O(1) per token)
- **Macro expansion**: Need to clone source info, but dominated by macro expansion cost
- **Evaluation**: Zero impact (no source info present)

## Alternative Designs Considered

### Design: Store Source in Value Directly

```rust
pub enum Value {
    Boolean(bool),
    Integer(i64),
    // ... all unchanged
}

// Separate type for values with source
pub struct SourceValue {
    value: Value,
    source: SourceInfo,
}
```

**Rejected because**: Still requires threading `SourceValue` through codebase. Not much simpler than `Syntax`.

### Design: Use Type Alias

```rust
type Syntax = (Value, Option<SourceInfo>);
```

**Rejected because**: Tuple destructuring is less clear. No encapsulation or helper methods.

## REPL Integration

### Problem: REPL Input is Ephemeral

REPL expressions don't come from files, so we need a strategy for tracking their source:

**Key Challenges:**
1. No actual file to reference
2. Multi-line expressions need line/column tracking
3. User expects helpful error messages even for REPL input
4. History should be referenceable

### Solution: Synthetic Source Files

Treat each REPL expression as a synthetic "file" with a unique identifier:

```rust
// In patina-repl/src/repl/mod.rs

pub struct Repl {
    editor: Editor<SchemeHelper, FileHistory>,
    interpreter: TreeWalkInterpreter,
    expression_counter: usize,  // NEW: Track REPL expressions
}

impl Repl {
    pub fn run(&mut self) -> rustyline::Result<()> {
        // ... REPL loop ...

        match readline {
            Ok(line) => {
                self.expression_counter += 1;
                let source_name = format!("<repl-{}>", self.expression_counter);

                // Pass source identifier to interpreter
                match self.interpreter.eval_str_with_source(&line, &source_name) {
                    Ok(result) => {
                        if !matches!(result, Value::Unspecified) {
                            println!("{}", result);
                        }
                    }
                    Err(e) => eprintln!("{}", e),  // Error includes source info
                }
            }
            // ... error handling ...
        }
    }
}
```

### New Interpreter API

```rust
// In patina-interpreter/src/lib.rs

impl<B: Backend> Interpreter<B> {
    /// Evaluate a string with source identification
    pub fn eval_str_with_source(
        &self,
        input: &str,
        source_name: &str
    ) -> Result<Value, InterpreterError<B::Error>> {
        let mut parser = Parser::new_with_file(input, source_name)?;
        let syntax = parser.parse()?;
        self.backend
            .eval_global(&syntax.data)  // Extract Value from Syntax<Value>
            .map_err(InterpreterError::Backend)
    }

    // Keep backward-compatible API
    pub fn eval_str(&self, input: &str) -> Result<Value, InterpreterError<B::Error>> {
        self.eval_str_with_source(input, "<input>")
    }
}
```

### Example REPL Sessions

**Single-line error:**
```scheme
patina> (+ x 1)
Error: Unbound variable: x
  at <repl-1>:1:4
     (+ x 1)
        ^
```

**Multi-line expression error:**
```scheme
patina> (define (factorial n)
...       (if (= n 0)
...           1
...           (* n (factoial (- n 1)))))
Error: Unbound variable: factoial
  at <repl-5>:4:18
     (* n (factoial (- n 1)))))
              ^
  help: did you mean 'factorial'?
```

**Cross-reference previous definitions:**
```scheme
patina> (define x 10)
patina> (define y (* x z))
Error: Unbound variable: z
  at <repl-2>:1:18
     (define y (* x z))
                    ^
  note: x was defined at <repl-1>:1:9
```

**Macro expansion errors:**
```scheme
patina> (define-syntax bad-macro
...       (syntax-rules ()
...         [(bad-macro x) (+ x undefined-var)]))
patina> (bad-macro 5)
Error: Unbound variable: undefined-var
  at <repl-4>:1:1
     (bad-macro 5)
     ^
  note: in macro expansion of 'bad-macro' defined at <repl-3>:3:24
```

### REPL History Enhancement (Future)

Store mapping of REPL expressions for better error context:

```rust
pub struct Repl {
    // ...
    expression_history: HashMap<String, String>,  // "<repl-N>" -> source code
}

// When displaying errors, can show full context even for old expressions
Error: ...
  at <repl-42>:2:5

  Full expression from <repl-42>:
  1 | (define (process data)
  2 |   (map proces data))
            ^
  3 |   result)
```

### Benefits for REPL Users

1. **Immediate feedback**: See exactly where syntax errors occur
2. **Multi-line clarity**: Track position within complex expressions
3. **History references**: Understand which expression caused an issue
4. **Learning aid**: New Scheme users can see precise error locations
5. **Macro debugging**: Trace errors through macro expansions

### Testing REPL Source Tracking

```rust
// In patina-repl/tests/repl_errors.rs

#[test]
fn test_repl_error_shows_source_location() {
    let mut repl = Repl::new().unwrap();

    // Simulate REPL input
    let result = repl.eval_expression("(+ x 1)", "<repl-1>");

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("<repl-1>:1:4"));
    assert!(err.contains("(+ x 1)"));
}

#[test]
fn test_multiline_repl_error() {
    let input = "(define (foo)\n  (bar undefined-var))";
    let result = repl.eval_expression(input, "<repl-2>");

    let err = result.unwrap_err().to_string();
    assert!(err.contains("<repl-2>:2:8"));
}
```

## Future Enhancements

### Phase 6+: Advanced Features

1. **Source maps for compiled code**:
   - When adding VM/JIT backends, maintain source mapping
   - Enable debugger to show original source

2. **IDE integration**:
   - LSP server can use source info for "go to definition"
   - Hover information can show where identifiers are bound

3. **Pretty error messages**:
   - Color-coded output
   - Multi-line context with line numbers
   - Suggestions for common mistakes (like "did you mean 'factorial'?")

4. **Coverage tracking**:
   - Track which source lines have been executed
   - Generate coverage reports

5. **REPL session replay**:
   - Save REPL sessions with source info
   - Replay sessions with full error context
   - Export REPL sessions to .scm files with preserved line numbers

## References

- **Racket**: Uses syntax objects extensively for source tracking and hygiene
- **Chibi Scheme**: Uses source annotations in AST
- **Chez Scheme**: Tracks source info through compiler passes
- **Rust compiler**: `rustc_span` crate provides similar functionality

## Success Criteria

1. ✅ Error messages show file:line:col for parse errors
2. ✅ Error messages show source location for eval errors
3. ✅ Macro expansion errors show both macro definition and use site
4. ✅ Source tracking adds <10% overhead to parse time
5. ✅ Existing tests pass with minimal changes
6. ✅ Documentation updated with examples

## Timeline Estimate

- **Phase 1** (Foundation): 1-2 days
- **Phase 2** (Parser Integration): 2-3 days
- **Phase 3** (Macro Expander): 3-4 days
- **Phase 4** (Error Reporting): 2-3 days
- **Phase 5** (REPL Integration): 1-2 days
- **Phase 6** (Stack Traces): 3-4 days (future)

**Total for Phases 1-5**: ~11-14 days of focused work

## Related Documentation

- `docs/FEATURE_STATUS.md` - Track implementation progress
- `internal/MILESTONES.md` - Record when source tracking is completed
- `docs/API.md` - Update with new Syntax API
