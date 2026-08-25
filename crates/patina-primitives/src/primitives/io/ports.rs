//! Port operations and predicates
//!
//! This module implements:
//! - Port predicates: port?, input-port?, output-port?, etc.
//! - String ports: open-input-string, open-output-string, get-output-string
//! - Bytevector ports: open-input-bytevector, open-output-bytevector, get-output-bytevector
//! - Current ports: current-input-port, current-output-port, current-error-port
//! - Port closing: close-port, close-input-port, close-output-port
use patina_core::{Heap, TaggedValue};
use patina_runtime::{EvalError, SharedHeap};
use patina_runtime::{Port, PortDirection};
use std::cell::RefCell;
use std::rc::Rc;

// =============================================================================
// TaggedValue extraction helpers
// =============================================================================

/// Extract a Port from TaggedValue
pub(super) fn get_port_tv<'a>(
    tv: TaggedValue,
    heap: &'a std::cell::Ref<'_, Heap>,
) -> Option<&'a Rc<Port>> {
    heap.get_port(tv)
}

/// Extract a String content from TaggedValue
fn get_string_tv(tv: TaggedValue, heap: &std::cell::Ref<'_, Heap>) -> Option<String> {
    heap.get_string_contents(tv)
}

/// Extract bytevector bytes from TaggedValue
fn get_bytevector_bytes_tv(tv: TaggedValue, heap: &std::cell::Ref<'_, Heap>) -> Option<Vec<u8>> {
    heap.get_bytevector_bytes(tv)
}

// =============================================================================
// Thread-local storage for current ports
// =============================================================================

thread_local! {
    static CURRENT_INPUT_PORT: RefCell<Rc<Port>> = RefCell::new(Port::stdin());
    static CURRENT_OUTPUT_PORT: RefCell<Rc<Port>> = RefCell::new(Port::stdout());
    static CURRENT_ERROR_PORT: RefCell<Rc<Port>> = RefCell::new(Port::stderr());
}

/// Get the current input port
pub fn get_current_input_port() -> Rc<Port> {
    CURRENT_INPUT_PORT.with(|p| p.borrow().clone())
}

/// Get the current output port
pub fn get_current_output_port() -> Rc<Port> {
    CURRENT_OUTPUT_PORT.with(|p| p.borrow().clone())
}

/// Get the current error port
pub fn get_current_error_port() -> Rc<Port> {
    CURRENT_ERROR_PORT.with(|p| p.borrow().clone())
}

/// Set the current input port (for dynamic rebinding)
pub fn set_current_input_port(port: Rc<Port>) {
    CURRENT_INPUT_PORT.with(|p| *p.borrow_mut() = port);
}

/// Set the current output port (for dynamic rebinding)
pub fn set_current_output_port(port: Rc<Port>) {
    CURRENT_OUTPUT_PORT.with(|p| *p.borrow_mut() = port);
}

/// Set the current error port (for dynamic rebinding)
pub fn set_current_error_port(port: Rc<Port>) {
    CURRENT_ERROR_PORT.with(|p| *p.borrow_mut() = port);
}

// =============================================================================
// Helper functions for getting ports from arguments
// =============================================================================

/// Helper to get the output port from tagged args or use current-output-port
pub(super) fn get_output_port_tagged(
    args: &[TaggedValue],
    arg_index: usize,
    heap: &std::cell::Ref<'_, Heap>,
) -> Result<Rc<Port>, EvalError> {
    if args.len() > arg_index {
        match get_port_tv(args[arg_index], heap) {
            Some(p) => {
                if !p.is_output() {
                    return Err(EvalError::TypeError("expected an output port".to_string()));
                }
                Ok(p.clone())
            }
            None => Err(EvalError::TypeError("expected a port".to_string())),
        }
    } else {
        Ok(get_current_output_port())
    }
}

/// Helper to get the input port from tagged args or use current-input-port
pub(super) fn get_input_port_tagged(
    args: &[TaggedValue],
    arg_index: usize,
    heap: &std::cell::Ref<'_, Heap>,
) -> Result<Rc<Port>, EvalError> {
    if args.len() > arg_index {
        match get_port_tv(args[arg_index], heap) {
            Some(p) => {
                if !p.is_input() {
                    return Err(EvalError::TypeError("expected an input port".to_string()));
                }
                Ok(p.clone())
            }
            None => Err(EvalError::TypeError("expected a port".to_string())),
        }
    } else {
        Ok(get_current_input_port())
    }
}

// =============================================================================
// Port Predicates
// =============================================================================

/// (port? obj) - Returns #t if obj is a port
pub(super) fn port_p(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "port? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    let heap_ref = heap.borrow();
    Ok(TaggedValue::boolean(
        get_port_tv(args[0], &heap_ref).is_some(),
    ))
}

/// (input-port? obj) - Returns #t if obj is an input port
pub(super) fn input_port_p(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "input-port? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    let heap_ref = heap.borrow();
    let result = get_port_tv(args[0], &heap_ref)
        .map(|p| p.is_input())
        .unwrap_or(false);
    Ok(TaggedValue::boolean(result))
}

/// (output-port? obj) - Returns #t if obj is an output port
pub(super) fn output_port_p(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "output-port? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    let heap_ref = heap.borrow();
    let result = get_port_tv(args[0], &heap_ref)
        .map(|p| p.is_output())
        .unwrap_or(false);
    Ok(TaggedValue::boolean(result))
}

/// (textual-port? obj) - Returns #t if obj is a textual port
pub(super) fn textual_port_p(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "textual-port? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    let heap_ref = heap.borrow();
    let result = get_port_tv(args[0], &heap_ref)
        .map(|p| p.is_textual())
        .unwrap_or(false);
    Ok(TaggedValue::boolean(result))
}

/// (binary-port? obj) - Returns #t if obj is a binary port
pub(super) fn binary_port_p(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "binary-port? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    let heap_ref = heap.borrow();
    let result = get_port_tv(args[0], &heap_ref)
        .map(|p| p.is_binary())
        .unwrap_or(false);
    Ok(TaggedValue::boolean(result))
}

/// (input-port-open? port) - Returns #t if port is open for input
pub(super) fn input_port_open_p(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "input-port-open? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    let heap_ref = heap.borrow();
    match get_port_tv(args[0], &heap_ref) {
        Some(p) => {
            // R7RS 6.13.1: "#t if port is still open and capable of
            // performing input" — an output-only port is simply not, rather
            // than a type error. Larceny's file suite maps every port
            // predicate over a fresh binary port and expects (#f ...) here.
            if p.direction != PortDirection::Input {
                return Ok(TaggedValue::FALSE);
            }
            Ok(TaggedValue::boolean(p.is_open()))
        }
        None => Err(EvalError::TypeError(
            "input-port-open? expects a port".to_string(),
        )),
    }
}

/// (output-port-open? port) - Returns #t if port is open for output
pub(super) fn output_port_open_p(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "output-port-open? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    let heap_ref = heap.borrow();
    match get_port_tv(args[0], &heap_ref) {
        Some(p) => {
            // Mirror of input-port-open?: an input-only port is not capable
            // of output, so #f, not an error.
            if p.direction != PortDirection::Output {
                return Ok(TaggedValue::FALSE);
            }
            Ok(TaggedValue::boolean(p.is_open()))
        }
        None => Err(EvalError::TypeError(
            "output-port-open? expects a port".to_string(),
        )),
    }
}

// =============================================================================
// String Ports
// =============================================================================

/// (open-input-string string) - Create an input port from a string
pub(super) fn open_input_string(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "open-input-string expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    let heap_ref = heap.borrow();
    match get_string_tv(args[0], &heap_ref) {
        Some(content) => {
            drop(heap_ref);
            Ok(heap
                .borrow_mut()
                .alloc_port(Port::new_input_string(content)))
        }
        None => Err(EvalError::TypeError(
            "open-input-string expects a string".to_string(),
        )),
    }
}

/// (open-output-string) - Create an output port that accumulates to a string
pub(super) fn open_output_string(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "open-output-string expects 0 arguments".to_string(),
            actual: args.len(),
        });
    }
    Ok(heap.borrow_mut().alloc_port(Port::new_output_string()))
}

/// (get-output-string port) - Get the accumulated string from an output string port
pub(super) fn get_output_string(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "get-output-string expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    let heap_ref = heap.borrow();
    match get_port_tv(args[0], &heap_ref) {
        Some(p) => match p.get_output_string() {
            Ok(s) => {
                drop(heap_ref);
                Ok(heap.borrow_mut().alloc_string(s))
            }
            Err(e) => Err(EvalError::IOError(e.to_string())),
        },
        None => Err(EvalError::TypeError(
            "get-output-string expects an output string port".to_string(),
        )),
    }
}

// =============================================================================
// Bytevector Ports (Binary I/O)
// =============================================================================

/// (open-input-bytevector bytevector) - Create an input port from a bytevector
pub(super) fn open_input_bytevector(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "open-input-bytevector expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    let heap_ref = heap.borrow();
    match get_bytevector_bytes_tv(args[0], &heap_ref) {
        Some(content) => {
            drop(heap_ref);
            Ok(heap
                .borrow_mut()
                .alloc_port(Port::new_input_bytevector(content)))
        }
        None => Err(EvalError::TypeError(
            "open-input-bytevector expects a bytevector".to_string(),
        )),
    }
}

/// (open-output-bytevector) - Create an output port that accumulates to a bytevector
pub(super) fn open_output_bytevector(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "open-output-bytevector expects 0 arguments".to_string(),
            actual: args.len(),
        });
    }
    Ok(heap.borrow_mut().alloc_port(Port::new_output_bytevector()))
}

/// (get-output-bytevector port) - Get the accumulated bytevector from an output bytevector port
pub(super) fn get_output_bytevector(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "get-output-bytevector expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    let heap_ref = heap.borrow();
    match get_port_tv(args[0], &heap_ref) {
        Some(p) => match p.get_output_bytevector() {
            Ok(bv) => {
                drop(heap_ref);
                Ok(heap.borrow_mut().alloc_bytevector(bv))
            }
            Err(e) => Err(EvalError::IOError(e.to_string())),
        },
        None => Err(EvalError::TypeError(
            "get-output-bytevector expects an output bytevector port".to_string(),
        )),
    }
}

// =============================================================================
// Current Ports
// =============================================================================

// These three are **parameter objects**, as R7RS §6.13.1 requires, not plain
// procedures: each reads with no argument and installs with one.
//
// `parameterize` (`lib/scheme/base/parameters.scm`) drives a parameter through
// the object itself — `(p)` to read the current value, `(p v)` to install one,
// `(p old)` from `dynamic-wind`'s after-thunk to restore it. So accepting the
// setter arity is not a shortcut around the dynamic binding: the thread-local
// *is* the binding, and writing it is what every other primitive that calls
// `get_current_output_port()` observes. A Scheme-level rebinding that left the
// thread-local alone would be the broken version, because `display` with no
// port argument reads the thread-local and would ignore it.
//
// Written out three times rather than shared: each has to exist anyway as its
// own `fn` (the registry takes bare fn pointers, not closures), so factoring
// the body out moved it behind three function-pointer parameters without
// removing a function. The setter arm defers to this file's existing port
// validator for the right direction, whose "argument absent, use the current
// port" branch cannot be reached from here.

/// (current-input-port) / (current-input-port port)
pub(super) fn current_input_port(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    match args {
        [] => Ok(heap.borrow_mut().alloc_port(get_current_input_port())),
        [_] => {
            let port = get_input_port_tagged(args, 0, &heap.borrow())?;
            set_current_input_port(port);
            Ok(TaggedValue::UNSPECIFIED)
        }
        _ => Err(EvalError::WrongArity {
            expected: "current-input-port expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        }),
    }
}

/// (current-output-port) / (current-output-port port)
pub(super) fn current_output_port(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    match args {
        [] => Ok(heap.borrow_mut().alloc_port(get_current_output_port())),
        [_] => {
            let port = get_output_port_tagged(args, 0, &heap.borrow())?;
            set_current_output_port(port);
            Ok(TaggedValue::UNSPECIFIED)
        }
        _ => Err(EvalError::WrongArity {
            expected: "current-output-port expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        }),
    }
}

/// (current-error-port) / (current-error-port port)
pub(super) fn current_error_port(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    match args {
        [] => Ok(heap.borrow_mut().alloc_port(get_current_error_port())),
        [_] => {
            let port = get_output_port_tagged(args, 0, &heap.borrow())?;
            set_current_error_port(port);
            Ok(TaggedValue::UNSPECIFIED)
        }
        _ => Err(EvalError::WrongArity {
            expected: "current-error-port expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        }),
    }
}

// =============================================================================
// Port Operations
// =============================================================================

/// (close-port port) - Close a port
pub(super) fn close_port(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "close-port expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    let heap_ref = heap.borrow();
    match get_port_tv(args[0], &heap_ref) {
        Some(p) => {
            p.close();
            Ok(TaggedValue::UNSPECIFIED)
        }
        None => Err(EvalError::TypeError(
            "close-port expects a port".to_string(),
        )),
    }
}

/// (close-input-port port) - Close an input port
pub(super) fn close_input_port(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "close-input-port expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    let heap_ref = heap.borrow();
    match get_port_tv(args[0], &heap_ref) {
        Some(p) => {
            if !p.is_input() {
                return Err(EvalError::TypeError(
                    "close-input-port expects an input port".to_string(),
                ));
            }
            p.close();
            Ok(TaggedValue::UNSPECIFIED)
        }
        None => Err(EvalError::TypeError(
            "close-input-port expects a port".to_string(),
        )),
    }
}

/// (close-output-port port) - Close an output port
pub(super) fn close_output_port(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "close-output-port expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    let heap_ref = heap.borrow();
    match get_port_tv(args[0], &heap_ref) {
        Some(p) => {
            if !p.is_output() {
                return Err(EvalError::TypeError(
                    "close-output-port expects an output port".to_string(),
                ));
            }
            p.close();
            Ok(TaggedValue::UNSPECIFIED)
        }
        None => Err(EvalError::TypeError(
            "close-output-port expects a port".to_string(),
        )),
    }
}

// =============================================================================
// EOF Handling
// =============================================================================

/// (eof-object? obj) - Returns #t if obj is the EOF object
pub(super) fn eof_object_p(
    _heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "eof-object? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    // Check directly against TaggedValue EOF constant - no conversion needed
    Ok(TaggedValue::boolean(args[0] == TaggedValue::EOF))
}

/// (eof-object) - Returns an EOF object
pub(super) fn eof_object(
    _heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "eof-object expects 0 arguments".to_string(),
            actual: args.len(),
        });
    }
    Ok(TaggedValue::EOF)
}

/// (flush-output-port [port]) - Flushes the output port
pub(super) fn flush_output_port(
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "flush-output-port expects 0 or 1 argument".to_string(),
            actual: args.len(),
        });
    }

    let port = if args.is_empty() {
        get_current_output_port()
    } else {
        let heap_ref = heap.borrow();
        match get_port_tv(args[0], &heap_ref) {
            Some(p) => {
                if !p.is_output() {
                    return Err(EvalError::TypeError("expected an output port".to_string()));
                }
                p.clone()
            }
            None => return Err(EvalError::TypeError("expected a port".to_string())),
        }
    };
    port.flush()
        .map_err(|e| EvalError::IOError(format!("flush failed: {}", e)))?;
    Ok(TaggedValue::UNSPECIFIED)
}

// =============================================================================
// call-with-port
// =============================================================================

/// (call-with-port port proc) - Calls proc with port, then closes the port
pub(super) fn call_with_port(
    ctx: &dyn crate::apply_context::ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "call-with-port expects 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let heap = ctx.heap();

    // Extract port for close() call
    let port = heap.borrow().get_port(args[0]).cloned().ok_or_else(|| {
        EvalError::TypeError("call-with-port expects a port as first argument".to_string())
    })?;

    // Call the procedure with the port — both already TaggedValues
    let result = ctx.apply_proc(args[1], vec![args[0]]);

    // Close the port regardless of result
    port.close();

    // Return the result
    result
}
