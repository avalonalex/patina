# Debug Mode PRD

## Overview

A comprehensive debug mode system for the Patina REPL that provides visibility into interpretation steps. The system is designed to be future-proof, supporting both the current tree-walking interpreter and a future VM-based bytecode execution model.

## Motivation

- **Learning**: Help users understand how Scheme evaluation works
- **Debugging**: Allow developers to trace execution flow and identify issues
- **Future-proof**: Support multiple execution models (tree-walking → bytecode VM)
- **Extensibility**: Easy to add new debug stages as the interpreter evolves

## Design Principles

1. **Staged Architecture**: Debug output organized by interpretation phase
2. **Granular Control**: Users can enable specific debug stages independently
3. **Non-invasive**: Debug code should not impact performance when disabled
4. **Extensible**: Easy to add new stages (macro expansion, bytecode, etc.)

## Debug Stages

### Current Implementation (Phase 1)

1. **Lexing** (`,debug lex`)
   - Show tokens as they're produced
   - Display token type, value, and position
   ```
   [LEX] Token(Integer, "42", line 1, col 1)
   [LEX] Token(Symbol, "+", line 1, col 4)
   ```

2. **Parsing** (`,debug parse`)
   - Show AST construction
   - Display Value enum variants being created
   ```
   [PARSE] Building list: (+ 1 2)
   [PARSE]   Symbol: +
   [PARSE]   Integer: 1
   [PARSE]   Integer: 2
   ```

3. **Evaluation** (`,debug eval`)
   - Show evaluation steps in tree-walking interpreter
   - Display expression being evaluated and result
   - Show environment lookups
   ```
   [EVAL] Evaluating: (+ 1 (* 2 3))
   [EVAL]   Eval arg: 1 => 1
   [EVAL]   Eval arg: (* 2 3)
   [EVAL]     Evaluating: (* 2 3)
   [EVAL]       Eval arg: 2 => 2
   [EVAL]       Eval arg: 3 => 3
   [EVAL]     Apply '*' => 6
   [EVAL]   Apply '+' => 7
   [EVAL] => 7
   ```

4. **Application** (`,debug apply`)
   - Show procedure applications
   - Display procedure name/type and arguments
   - Show return values
   ```
   [APPLY] Primitive '*'
   [APPLY]   Args: [2, 3]
   [APPLY]   => 6
   [APPLY] Lambda <closure>
   [APPLY]   Params: (x y)
   [APPLY]   Args: [1, 2]
   [APPLY]   Body: (+ x y)
   [APPLY]   => 3
   ```

5. **Environment** (`,debug env`)
   - Show variable lookups and bindings
   - Display environment chains
   ```
   [ENV] Lookup 'x' in current env
   [ENV]   Not found, checking parent
   [ENV]   Found: x = 42
   [ENV] Define 'y' = 10 in current env
   ```

### Future Implementation (Phase 2+)

6. **Macro Expansion** (`,debug expand`)
   - Show syntax transformations
   - Critical for debugging hygienic macros
   ```
   [EXPAND] Before: (let ((x 1)) (+ x 1))
   [EXPAND] After: ((lambda (x) (+ x 1)) 1)
   ```

7. **Compilation** (`,debug compile`)
   - Show bytecode generation from AST
   - Display instruction stream
   ```
   [COMPILE] Compiling: (+ 1 2)
   [COMPILE]   PUSH_CONST 1
   [COMPILE]   PUSH_CONST 2
   [COMPILE]   CALL_PRIMITIVE +, 2
   ```

8. **VM Execution** (`,debug vm`)
   - Show bytecode execution steps
   - Display stack state and instruction pointer
   ```
   [VM] IP=0: PUSH_CONST 1
   [VM]   Stack: [1]
   [VM] IP=1: PUSH_CONST 2
   [VM]   Stack: [1, 2]
   [VM] IP=2: CALL_PRIMITIVE +, 2
   [VM]   Stack: [3]
   ```

## User Interface

### REPL Commands

```scheme
,debug <stage>    ; Enable debug output for a stage
,debug off        ; Disable all debug output
,debug all        ; Enable all debug stages
,debug list       ; Show current debug settings
```

### Examples

```scheme
> ,debug eval
Debug mode: eval

> (+ 1 (* 2 3))
[EVAL] Evaluating: (+ 1 (* 2 3))
[EVAL]   Eval arg: 1 => 1
[EVAL]   Eval arg: (* 2 3)
[EVAL]     Evaluating: (* 2 3)
[EVAL]       Eval arg: 2 => 2
[EVAL]       Eval arg: 3 => 3
[EVAL]     Apply '*' => 6
[EVAL]   Apply '+' => 7
[EVAL] => 7
7

> ,debug all
Debug mode: lex, parse, eval, apply, env

> (define x 10)
[LEX] Token(Symbol, "define", ...)
[LEX] Token(Symbol, "x", ...)
[LEX] Token(Integer, "10", ...)
[PARSE] Building list: (define x 10)
[PARSE]   Symbol: define
[PARSE]   Symbol: x
[PARSE]   Integer: 10
[EVAL] Evaluating: (define x 10)
[EVAL]   Special form: define
[ENV] Define 'x' = 10 in global env

> ,debug off
Debug mode disabled
```

## Technical Architecture

### Debug Trait (Extensible)

```rust
pub trait DebugOutput {
    fn is_enabled(&self, stage: DebugStage) -> bool;

    // Current stages
    fn debug_lex(&self, token: &Token);
    fn debug_parse(&self, value: &Value);
    fn debug_eval(&self, expr: &Value, env: &Environment);
    fn debug_apply(&self, proc: &str, args: &[Value], result: &Value);
    fn debug_env(&self, event: EnvEvent);

    // Future stages
    fn debug_expand(&self, before: &Value, after: &Value);
    fn debug_compile(&self, bytecode: &Instruction);
    fn debug_vm_step(&self, state: &VmState);
}

pub enum DebugStage {
    Lex,
    Parse,
    Eval,
    Apply,
    Env,
    Expand,    // Future
    Compile,   // Future
    Vm,        // Future
}
```

### Configuration

```rust
pub struct DebugConfig {
    enabled_stages: HashSet<DebugStage>,
    indent_level: usize,
    color: bool,
}
```

### Integration Points

1. **Lexer** - Add debug calls before returning tokens
2. **Parser** - Add debug calls during AST construction
3. **Evaluator** - Add debug calls at entry/exit of eval functions
4. **Primitives** - Add debug calls in apply_primitive
5. **Environment** - Add debug calls for lookups/bindings

### Performance Considerations

- Use feature flags or compile-time checks when debug is disabled
- Lazy format strings (only format when actually printing)
- Option to write to file instead of stderr for heavy traces

## Implementation Plan

### Phase 1: Core Infrastructure (Current)
- [ ] Define `DebugOutput` trait and `DebugStage` enum
- [ ] Implement `DebugConfig` with stage toggles
- [ ] Add REPL commands (`,debug` parser)
- [ ] Thread `DebugConfig` through `Evaluator`

### Phase 2: Basic Stages (Current)
- [ ] Implement `debug_eval` in evaluator
- [ ] Implement `debug_apply` in primitives
- [ ] Implement `debug_env` in environment
- [ ] Add indentation tracking for nested calls

### Phase 3: Input Stages (Current)
- [ ] Implement `debug_lex` in lexer
- [ ] Implement `debug_parse` in parser

### Phase 4: Future Stages (VM Phase)
- [ ] Implement `debug_expand` for macro expansion
- [ ] Implement `debug_compile` for bytecode generation
- [ ] Implement `debug_vm` for VM execution

## Open Questions

1. **Output destination**: stderr vs dedicated debug log file?
2. **Filtering**: Should we support filtering by expression pattern?
3. **Performance**: Should debug mode be a compile-time feature flag?
4. **Color coding**: Use colors to distinguish different stages?
5. **Depth limiting**: Limit recursion depth in output to prevent spam?

## Success Criteria

- Users can trace evaluation of any Scheme expression
- Debug output clearly shows execution flow
- System is extensible for future VM implementation
- Minimal performance impact when disabled
- Documentation and examples for all debug stages
