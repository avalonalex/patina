//! I/O primitives for R7RS
//!
//! This module implements R7RS I/O operations:
//! - Port predicates: port?, input-port?, output-port?, etc.
//! - String ports: open-input-string, open-output-string, get-output-string
//! - Current ports: current-input-port, current-output-port, current-error-port
//! - EOF handling: eof-object?, eof-object
//! - Text I/O: display, write, newline, read-char, peek-char, char-ready?
//! - Read: read (parse S-expressions from port)

use super::super::EvalResult;
use super::super::error::EvalError;
use super::Evaluator;
use super::registry::PrimitiveFn;
use patina_frontend::Parser;
use patina_runtime::value::Value;
use patina_runtime::{Arity, Port, PortData, PortDirection};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{self, BufRead};
use std::rc::Rc;

// =============================================================================
// Datum Label Writer - Handles circular structures with #n= and #n# notation
// =============================================================================

/// Writer that handles circular/shared structures using datum labels
struct DatumLabelWriter {
    /// Maps pointer addresses to label numbers for circular structures
    labels: HashMap<usize, usize>,
    /// Set of addresses currently on the traversal stack (for detecting cycles)
    on_stack: HashMap<usize, bool>,
    /// Next label number to assign
    next_label: usize,
    /// Whether we're in display mode (strings without quotes, chars without #\)
    display_mode: bool,
    /// Whether to label all shared structures (write-shared) or only circular (write)
    label_shared: bool,
}

impl DatumLabelWriter {
    fn new(display_mode: bool, label_shared: bool) -> Self {
        Self {
            labels: HashMap::new(),
            on_stack: HashMap::new(),
            next_label: 0,
            display_mode,
            label_shared,
        }
    }

    /// First pass: find circular (and optionally shared) structures
    /// Uses DFS with on_stack tracking to detect back-edges (cycles)
    fn find_circular(&mut self, value: &Value) {
        match value {
            Value::Pair(p) => {
                let addr = Rc::as_ptr(p) as usize;

                // Check if we're revisiting something on the current stack (cycle!)
                if self.on_stack.get(&addr).copied().unwrap_or(false) {
                    // This is a back-edge - it's circular
                    if !self.labels.contains_key(&addr) {
                        self.labels.insert(addr, self.next_label);
                        self.next_label += 1;
                    }
                    return;
                }

                // Check if we've already fully explored this node
                if self.on_stack.contains_key(&addr) {
                    // Already visited and finished - this is sharing, not a cycle
                    // Only label if we want shared structures labeled
                    if self.label_shared && !self.labels.contains_key(&addr) {
                        self.labels.insert(addr, self.next_label);
                        self.next_label += 1;
                    }
                    return;
                }

                // Mark as on stack and recurse
                self.on_stack.insert(addr, true);
                let borrowed = p.borrow();
                self.find_circular(&borrowed.0);
                self.find_circular(&borrowed.1);
                // Mark as visited but no longer on stack
                self.on_stack.insert(addr, false);
            }
            Value::Vector(v) => {
                let addr = Rc::as_ptr(v) as usize;

                if self.on_stack.get(&addr).copied().unwrap_or(false) {
                    if !self.labels.contains_key(&addr) {
                        self.labels.insert(addr, self.next_label);
                        self.next_label += 1;
                    }
                    return;
                }

                if self.on_stack.contains_key(&addr) {
                    if self.label_shared && !self.labels.contains_key(&addr) {
                        self.labels.insert(addr, self.next_label);
                        self.next_label += 1;
                    }
                    return;
                }

                self.on_stack.insert(addr, true);
                let borrowed = v.borrow();
                for elem in borrowed.iter() {
                    self.find_circular(elem);
                }
                self.on_stack.insert(addr, false);
            }
            _ => {}
        }
    }

    /// Second pass: write with labels
    fn write_value(&self, value: &Value, out: &mut String, emitted: &mut HashMap<usize, bool>) {
        match value {
            Value::Pair(p) => {
                let addr = Rc::as_ptr(p) as usize;

                // Check if this structure has a label
                if let Some(&label) = self.labels.get(&addr) {
                    if emitted.get(&addr).copied().unwrap_or(false) {
                        // Already emitted - use back-reference
                        out.push_str(&format!("#{}#", label));
                        return;
                    } else {
                        // First time - emit with label definition
                        emitted.insert(addr, true);
                        out.push_str(&format!("#{}=", label));
                    }
                }

                // Check for quote shorthand
                if let Some(shorthand) = self.check_quote_shorthand(value) {
                    out.push_str(&shorthand);
                    return;
                }

                out.push('(');
                self.write_list_contents(p, out, emitted);
                out.push(')');
            }
            Value::Vector(v) => {
                let addr = Rc::as_ptr(v) as usize;

                if let Some(&label) = self.labels.get(&addr) {
                    if emitted.get(&addr).copied().unwrap_or(false) {
                        out.push_str(&format!("#{}#", label));
                        return;
                    } else {
                        emitted.insert(addr, true);
                        out.push_str(&format!("#{}=", label));
                    }
                }

                out.push_str("#(");
                let borrowed = v.borrow();
                for (i, elem) in borrowed.iter().enumerate() {
                    if i > 0 {
                        out.push(' ');
                    }
                    self.write_value(elem, out, emitted);
                }
                out.push(')');
            }
            // For non-compound types, use the standard Display
            Value::String(s) if self.display_mode => {
                out.push_str(&s.borrow());
            }
            Value::Character(c) if self.display_mode => {
                out.push(*c);
            }
            other => {
                out.push_str(&other.to_string());
            }
        }
    }

    fn write_list_contents(
        &self,
        pair: &Rc<RefCell<(Value, Value)>>,
        out: &mut String,
        emitted: &mut HashMap<usize, bool>,
    ) {
        let borrowed = pair.borrow();

        // Write the car
        self.write_value(&borrowed.0, out, emitted);

        // Handle the cdr
        match &borrowed.1 {
            Value::Null => {}
            Value::Pair(next_pair) => {
                let addr = Rc::as_ptr(next_pair) as usize;

                // Check if the cdr is a labeled structure we've already emitted
                if let Some(&label) = self.labels.get(&addr)
                    && emitted.get(&addr).copied().unwrap_or(false)
                {
                    // Back-reference in cdr position
                    out.push_str(&format!(" . #{}#", label));
                    return;
                }

                out.push(' ');
                self.write_list_contents(next_pair, out, emitted);
            }
            other => {
                out.push_str(" . ");
                self.write_value(other, out, emitted);
            }
        }
    }

    fn check_quote_shorthand(&self, value: &Value) -> Option<String> {
        if let Value::Pair(p) = value {
            let borrowed = p.borrow();
            let symbol_name = match &borrowed.0 {
                Value::Symbol(s) => Some(s.as_ref()),
                Value::Identifier(id) => Some(&*id.name),
                _ => None,
            };

            if let Some(name) = symbol_name {
                let prefix = match name {
                    "quote" => Some("'"),
                    "quasiquote" => Some("`"),
                    "unquote" => Some(","),
                    "unquote-splicing" => Some(",@"),
                    _ => None,
                };

                if let Some(p) = prefix {
                    // Check that cdr is a single-element list
                    if let Value::Pair(cdr_pair) = &borrowed.1 {
                        let cdr_borrowed = cdr_pair.borrow();
                        if matches!(cdr_borrowed.1, Value::Null) {
                            let mut result = String::from(p);
                            let mut emitted = HashMap::new();
                            self.write_value(&cdr_borrowed.0, &mut result, &mut emitted);
                            return Some(result);
                        }
                    }
                }
            }
        }
        None
    }
}

/// Format a value for write (machine-readable, with datum labels for circular structures only)
fn format_write(value: &Value) -> String {
    let mut writer = DatumLabelWriter::new(false, false); // don't label shared
    writer.find_circular(value);

    let mut output = String::new();
    let mut emitted = HashMap::new();
    writer.write_value(value, &mut output, &mut emitted);
    output
}

/// Format a value for display (human-readable, with datum labels for circular structures only)
fn format_display(value: &Value) -> String {
    let mut writer = DatumLabelWriter::new(true, false); // don't label shared
    writer.find_circular(value);

    let mut output = String::new();
    let mut emitted = HashMap::new();
    writer.write_value(value, &mut output, &mut emitted);
    output
}

/// Format a value for write-shared (machine-readable, with datum labels for all shared structures)
fn format_write_shared(value: &Value) -> String {
    let mut writer = DatumLabelWriter::new(false, true); // label shared
    writer.find_circular(value);

    let mut output = String::new();
    let mut emitted = HashMap::new();
    writer.write_value(value, &mut output, &mut emitted);
    output
}

/// Format a value for write-simple (machine-readable, no datum labels - may loop on circular structures)
fn format_write_simple(value: &Value) -> String {
    // write-simple doesn't handle circular structures, uses simple recursive formatting
    format_simple_recursive(value)
}

/// Simple recursive formatting without circular structure handling
/// Note: Will infinitely loop on circular structures - this is expected R7RS behavior
fn format_simple_recursive(value: &Value) -> String {
    match value {
        Value::Pair(pair) => {
            let borrowed = pair.borrow();
            let mut result = String::from("(");
            result.push_str(&format_simple_recursive(&borrowed.0));

            let mut current = borrowed.1.clone();
            drop(borrowed); // Release borrow before loop

            loop {
                match current {
                    Value::Null => break,
                    Value::Pair(ref p) => {
                        let p_borrowed = p.borrow();
                        result.push(' ');
                        result.push_str(&format_simple_recursive(&p_borrowed.0));
                        let next = p_borrowed.1.clone();
                        drop(p_borrowed);
                        current = next;
                    }
                    ref other => {
                        result.push_str(" . ");
                        result.push_str(&format_simple_recursive(other));
                        break;
                    }
                }
            }
            result.push(')');
            result
        }
        Value::Vector(v) => {
            let v = v.borrow();
            let mut result = String::from("#(");
            for (i, elem) in v.iter().enumerate() {
                if i > 0 {
                    result.push(' ');
                }
                result.push_str(&format_simple_recursive(elem));
            }
            result.push(')');
            result
        }
        // For all other types, use Display trait (which provides the correct formatting)
        other => other.to_string(),
    }
}

// =============================================================================
// Port Predicates
// =============================================================================

/// (port? obj) - Returns #t if obj is a port
fn port_p(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "port? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    Ok(Value::Boolean(matches!(args[0], Value::Port(_))))
}

/// (input-port? obj) - Returns #t if obj is an input port
fn input_port_p(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "input-port? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    let result = match &args[0] {
        Value::Port(p) => p.is_input(),
        _ => false,
    };
    Ok(Value::Boolean(result))
}

/// (output-port? obj) - Returns #t if obj is an output port
fn output_port_p(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "output-port? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    let result = match &args[0] {
        Value::Port(p) => p.is_output(),
        _ => false,
    };
    Ok(Value::Boolean(result))
}

/// (textual-port? obj) - Returns #t if obj is a textual port
fn textual_port_p(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "textual-port? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    let result = match &args[0] {
        Value::Port(p) => p.is_textual(),
        _ => false,
    };
    Ok(Value::Boolean(result))
}

/// (binary-port? obj) - Returns #t if obj is a binary port
fn binary_port_p(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "binary-port? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    let result = match &args[0] {
        Value::Port(p) => p.is_binary(),
        _ => false,
    };
    Ok(Value::Boolean(result))
}

/// (input-port-open? port) - Returns #t if port is open for input
fn input_port_open_p(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "input-port-open? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match &args[0] {
        Value::Port(p) => {
            if p.direction != PortDirection::Input {
                return Err(EvalError::TypeError(
                    "input-port-open? expects an input port".to_string(),
                ));
            }
            Ok(Value::Boolean(p.is_open()))
        }
        _ => Err(EvalError::TypeError(
            "input-port-open? expects a port".to_string(),
        )),
    }
}

/// (output-port-open? port) - Returns #t if port is open for output
fn output_port_open_p(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "output-port-open? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match &args[0] {
        Value::Port(p) => {
            if p.direction != PortDirection::Output {
                return Err(EvalError::TypeError(
                    "output-port-open? expects an output port".to_string(),
                ));
            }
            Ok(Value::Boolean(p.is_open()))
        }
        _ => Err(EvalError::TypeError(
            "output-port-open? expects a port".to_string(),
        )),
    }
}

// =============================================================================
// String Ports
// =============================================================================

/// (open-input-string string) - Create an input port from a string
fn open_input_string(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "open-input-string expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match &args[0] {
        Value::String(s) => {
            let content = s.borrow().clone();
            Ok(Value::Port(Port::new_input_string(content)))
        }
        _ => Err(EvalError::TypeError(
            "open-input-string expects a string".to_string(),
        )),
    }
}

/// (open-output-string) - Create an output port that accumulates to a string
fn open_output_string(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "open-output-string expects 0 arguments".to_string(),
            actual: args.len(),
        });
    }
    Ok(Value::Port(Port::new_output_string()))
}

/// (get-output-string port) - Get the accumulated string from an output string port
fn get_output_string(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "get-output-string expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match &args[0] {
        Value::Port(p) => match p.get_output_string() {
            Ok(s) => Ok(Value::String(Rc::new(RefCell::new(s)))),
            Err(e) => Err(EvalError::IOError(e.to_string())),
        },
        _ => Err(EvalError::TypeError(
            "get-output-string expects an output string port".to_string(),
        )),
    }
}

// =============================================================================
// Bytevector Ports (Binary I/O)
// =============================================================================

/// (open-input-bytevector bytevector) - Create an input port from a bytevector
fn open_input_bytevector(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "open-input-bytevector expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match &args[0] {
        Value::Bytevector(bv) => {
            let content = bv.borrow().clone();
            Ok(Value::Port(Port::new_input_bytevector(content)))
        }
        _ => Err(EvalError::TypeError(
            "open-input-bytevector expects a bytevector".to_string(),
        )),
    }
}

/// (open-output-bytevector) - Create an output port that accumulates to a bytevector
fn open_output_bytevector(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "open-output-bytevector expects 0 arguments".to_string(),
            actual: args.len(),
        });
    }
    Ok(Value::Port(Port::new_output_bytevector()))
}

/// (get-output-bytevector port) - Get the accumulated bytevector from an output bytevector port
fn get_output_bytevector(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "get-output-bytevector expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match &args[0] {
        Value::Port(p) => match p.get_output_bytevector() {
            Ok(bv) => Ok(Value::Bytevector(Rc::new(RefCell::new(bv)))),
            Err(e) => Err(EvalError::IOError(e.to_string())),
        },
        _ => Err(EvalError::TypeError(
            "get-output-bytevector expects an output bytevector port".to_string(),
        )),
    }
}

// =============================================================================
// Current Ports (as simple parameter-like globals for now)
// =============================================================================

// Thread-local storage for current ports
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

/// (current-input-port) - Returns the current input port
fn current_input_port(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "current-input-port expects 0 arguments".to_string(),
            actual: args.len(),
        });
    }
    Ok(Value::Port(get_current_input_port()))
}

/// (current-output-port) - Returns the current output port
fn current_output_port(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "current-output-port expects 0 arguments".to_string(),
            actual: args.len(),
        });
    }
    Ok(Value::Port(get_current_output_port()))
}

/// (current-error-port) - Returns the current error port
fn current_error_port(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "current-error-port expects 0 arguments".to_string(),
            actual: args.len(),
        });
    }
    Ok(Value::Port(get_current_error_port()))
}

// =============================================================================
// Port Operations
// =============================================================================

/// (close-port port) - Close a port
fn close_port(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "close-port expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match &args[0] {
        Value::Port(p) => {
            p.close();
            Ok(Value::Unspecified)
        }
        _ => Err(EvalError::TypeError(
            "close-port expects a port".to_string(),
        )),
    }
}

/// (close-input-port port) - Close an input port
fn close_input_port(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "close-input-port expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match &args[0] {
        Value::Port(p) => {
            if !p.is_input() {
                return Err(EvalError::TypeError(
                    "close-input-port expects an input port".to_string(),
                ));
            }
            p.close();
            Ok(Value::Unspecified)
        }
        _ => Err(EvalError::TypeError(
            "close-input-port expects a port".to_string(),
        )),
    }
}

/// (close-output-port port) - Close an output port
fn close_output_port(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "close-output-port expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match &args[0] {
        Value::Port(p) => {
            if !p.is_output() {
                return Err(EvalError::TypeError(
                    "close-output-port expects an output port".to_string(),
                ));
            }
            p.close();
            Ok(Value::Unspecified)
        }
        _ => Err(EvalError::TypeError(
            "close-output-port expects a port".to_string(),
        )),
    }
}

// =============================================================================
// File I/O Operations
// =============================================================================

/// (open-input-file filename) - Opens a file for reading and returns an input port
fn open_input_file(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "open-input-file expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match &args[0] {
        Value::String(s) => {
            let path = s.borrow();
            let port = Port::open_input_file(&path).map_err(|e| {
                EvalError::IOError(format!("Cannot open '{}' for reading: {}", path, e))
            })?;
            Ok(Value::Port(port))
        }
        _ => Err(EvalError::TypeError(
            "open-input-file expects a string filename".to_string(),
        )),
    }
}

/// (open-output-file filename) - Opens a file for writing and returns an output port
fn open_output_file(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "open-output-file expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match &args[0] {
        Value::String(s) => {
            let path = s.borrow();
            let port = Port::open_output_file(&path).map_err(|e| {
                EvalError::IOError(format!("Cannot open '{}' for writing: {}", path, e))
            })?;
            Ok(Value::Port(port))
        }
        _ => Err(EvalError::TypeError(
            "open-output-file expects a string filename".to_string(),
        )),
    }
}

/// (open-binary-input-file filename) - Opens a binary file for reading
fn open_binary_input_file(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "open-binary-input-file expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match &args[0] {
        Value::String(s) => {
            let path = s.borrow();
            let port = Port::open_binary_input_file(&path).map_err(|e| {
                EvalError::IOError(format!("Cannot open '{}' for binary reading: {}", path, e))
            })?;
            Ok(Value::Port(port))
        }
        _ => Err(EvalError::TypeError(
            "open-binary-input-file expects a string filename".to_string(),
        )),
    }
}

/// (open-binary-output-file filename) - Opens a binary file for writing
fn open_binary_output_file(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "open-binary-output-file expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match &args[0] {
        Value::String(s) => {
            let path = s.borrow();
            let port = Port::open_binary_output_file(&path).map_err(|e| {
                EvalError::IOError(format!("Cannot open '{}' for binary writing: {}", path, e))
            })?;
            Ok(Value::Port(port))
        }
        _ => Err(EvalError::TypeError(
            "open-binary-output-file expects a string filename".to_string(),
        )),
    }
}

/// (flush-output-port [port]) - Flushes the output port
fn flush_output_port(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "flush-output-port expects 0 or 1 argument".to_string(),
            actual: args.len(),
        });
    }

    let port = get_output_port(&args, 0)?;
    port.flush()
        .map_err(|e| EvalError::IOError(format!("flush failed: {}", e)))?;
    Ok(Value::Unspecified)
}

/// (file-exists? filename) - Returns #t if file exists
fn file_exists_p(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "file-exists? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match &args[0] {
        Value::String(s) => {
            let path = s.borrow();
            Ok(Value::Boolean(std::path::Path::new(&*path).exists()))
        }
        _ => Err(EvalError::TypeError(
            "file-exists? expects a string filename".to_string(),
        )),
    }
}

/// (delete-file filename) - Deletes the file
fn delete_file(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "delete-file expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match &args[0] {
        Value::String(s) => {
            let path = s.borrow();
            std::fs::remove_file(&*path)
                .map_err(|e| EvalError::IOError(format!("Cannot delete '{}': {}", path, e)))?;
            Ok(Value::Unspecified)
        }
        _ => Err(EvalError::TypeError(
            "delete-file expects a string filename".to_string(),
        )),
    }
}

/// (call-with-port port proc) - Calls proc with port, then closes the port
fn call_with_port(
    eval: &Evaluator,
    args: Vec<Value>,
    _in_tail_position: bool,
) -> Result<EvalResult, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "call-with-port expects 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = match &args[0] {
        Value::Port(p) => p.clone(),
        _ => {
            return Err(EvalError::TypeError(
                "call-with-port expects a port as first argument".to_string(),
            ));
        }
    };

    let proc = &args[1];

    // Call the procedure with the port
    let result = eval.apply(proc.clone(), vec![Value::Port(port.clone())], false);

    // Close the port regardless of result
    port.close();

    // Return the result
    result
}

/// (call-with-input-file filename proc) - Opens file for reading, calls proc with port, closes
fn call_with_input_file(
    eval: &Evaluator,
    args: Vec<Value>,
    _in_tail_position: bool,
) -> Result<EvalResult, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "call-with-input-file expects 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let filename = match &args[0] {
        Value::String(s) => s.borrow().to_string(),
        _ => {
            return Err(EvalError::TypeError(
                "call-with-input-file expects a string filename".to_string(),
            ));
        }
    };

    let proc = &args[1];

    // Open the file
    let port = Port::open_input_file(&filename)
        .map_err(|e| EvalError::IOError(format!("Cannot open '{}': {}", filename, e)))?;

    // Call the procedure with the port
    let result = eval.apply(proc.clone(), vec![Value::Port(port.clone())], false);

    // Close the port regardless of result
    port.close();

    // Return the result
    result
}

/// (call-with-output-file filename proc) - Opens file for writing, calls proc with port, closes
fn call_with_output_file(
    eval: &Evaluator,
    args: Vec<Value>,
    _in_tail_position: bool,
) -> Result<EvalResult, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "call-with-output-file expects 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let filename = match &args[0] {
        Value::String(s) => s.borrow().to_string(),
        _ => {
            return Err(EvalError::TypeError(
                "call-with-output-file expects a string filename".to_string(),
            ));
        }
    };

    let proc = &args[1];

    // Open the file
    let port = Port::open_output_file(&filename)
        .map_err(|e| EvalError::IOError(format!("Cannot open '{}': {}", filename, e)))?;

    // Call the procedure with the port
    let result = eval.apply(proc.clone(), vec![Value::Port(port.clone())], false);

    // Close the port regardless of result (flush happens on close)
    port.close();

    // Return the result
    result
}

/// (with-input-from-file filename thunk) - Opens file, sets current-input-port, calls thunk
/// Dynamically rebinds current-input-port for the duration of the thunk call.
fn with_input_from_file(
    eval: &Evaluator,
    args: Vec<Value>,
    _in_tail_position: bool,
) -> Result<EvalResult, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "with-input-from-file expects 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let filename = match &args[0] {
        Value::String(s) => s.borrow().to_string(),
        _ => {
            return Err(EvalError::TypeError(
                "with-input-from-file expects a string filename".to_string(),
            ));
        }
    };

    let thunk = &args[1];

    // Open the file
    let port = Port::open_input_file(&filename)
        .map_err(|e| EvalError::IOError(format!("Cannot open '{}': {}", filename, e)))?;

    // Save the old current-input-port
    let old_port = get_current_input_port();

    // Set the new current-input-port
    set_current_input_port(port.clone());

    // Call the thunk and capture the result
    let result = eval.apply(thunk.clone(), vec![], false);

    // Restore the old current-input-port (even on error)
    set_current_input_port(old_port);

    // Close the file port
    port.close();

    result
}

/// (with-output-to-file filename thunk) - Opens file, sets current-output-port, calls thunk
/// Dynamically rebinds current-output-port for the duration of the thunk call.
fn with_output_to_file(
    eval: &Evaluator,
    args: Vec<Value>,
    _in_tail_position: bool,
) -> Result<EvalResult, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "with-output-to-file expects 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let filename = match &args[0] {
        Value::String(s) => s.borrow().to_string(),
        _ => {
            return Err(EvalError::TypeError(
                "with-output-to-file expects a string filename".to_string(),
            ));
        }
    };

    let thunk = &args[1];

    // Open the file
    let port = Port::open_output_file(&filename)
        .map_err(|e| EvalError::IOError(format!("Cannot open '{}': {}", filename, e)))?;

    // Save the old current-output-port
    let old_port = get_current_output_port();

    // Set the new current-output-port
    set_current_output_port(port.clone());

    // Call the thunk and capture the result
    let result = eval.apply(thunk.clone(), vec![], false);

    // Restore the old current-output-port (even on error)
    set_current_output_port(old_port);

    // Flush and close the file port
    port.flush().ok(); // Best effort flush
    port.close();

    result
}

// =============================================================================
// EOF Handling
// =============================================================================

/// (eof-object? obj) - Returns #t if obj is the EOF object
fn eof_object_p(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "eof-object? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    Ok(Value::Boolean(matches!(args[0], Value::Eof)))
}

/// (eof-object) - Returns an EOF object
fn eof_object(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "eof-object expects 0 arguments".to_string(),
            actual: args.len(),
        });
    }
    Ok(Value::Eof)
}

// =============================================================================
// Text Output
// =============================================================================

/// Helper to get the output port from args or use current-output-port
fn get_output_port(args: &[Value], arg_index: usize) -> Result<Rc<Port>, EvalError> {
    if args.len() > arg_index {
        match &args[arg_index] {
            Value::Port(p) => {
                if !p.is_output() {
                    return Err(EvalError::TypeError("expected an output port".to_string()));
                }
                Ok(p.clone())
            }
            _ => Err(EvalError::TypeError("expected a port".to_string())),
        }
    } else {
        Ok(get_current_output_port())
    }
}

/// Helper to get the input port from args or use current-input-port
fn get_input_port(args: &[Value], arg_index: usize) -> Result<Rc<Port>, EvalError> {
    if args.len() > arg_index {
        match &args[arg_index] {
            Value::Port(p) => {
                if !p.is_input() {
                    return Err(EvalError::TypeError("expected an input port".to_string()));
                }
                Ok(p.clone())
            }
            _ => Err(EvalError::TypeError("expected a port".to_string())),
        }
    } else {
        Ok(get_current_input_port())
    }
}

/// (display obj [port]) - Write obj in human-readable format
/// Handles circular structures using datum labels (#n= and #n#)
fn display(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "display expects 1 or 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_output_port(&args, 1)?;
    let output = format_display(&args[0]);

    port.write_string(&output)
        .map_err(|e| EvalError::IOError(e.to_string()))?;

    Ok(Value::Unspecified)
}

/// (write obj [port]) - Write obj in machine-readable format
/// Handles circular structures using datum labels (#n= and #n#)
fn write(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "write expects 1 or 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_output_port(&args, 1)?;
    let output = format_write(&args[0]);

    port.write_string(&output)
        .map_err(|e| EvalError::IOError(e.to_string()))?;

    Ok(Value::Unspecified)
}

/// (write-shared obj [port]) - Write obj with datum labels for all shared structures
/// Labels both circular and shared (multiply-referenced) structures
fn write_shared(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "write-shared expects 1 or 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_output_port(&args, 1)?;
    let output = format_write_shared(&args[0]);

    port.write_string(&output)
        .map_err(|e| EvalError::IOError(e.to_string()))?;

    Ok(Value::Unspecified)
}

/// (write-simple obj [port]) - Write obj without datum labels
/// Does not handle circular structures (may loop infinitely)
fn write_simple(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "write-simple expects 1 or 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_output_port(&args, 1)?;
    let output = format_write_simple(&args[0]);

    port.write_string(&output)
        .map_err(|e| EvalError::IOError(e.to_string()))?;

    Ok(Value::Unspecified)
}

/// (newline [port]) - Write a newline character
fn newline(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "newline expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_output_port(&args, 0)?;
    port.write_string("\n")
        .map_err(|e| EvalError::IOError(e.to_string()))?;

    Ok(Value::Unspecified)
}

/// (write-char char [port]) - Write a single character
fn write_char(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "write-char expects 1 or 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let ch = match &args[0] {
        Value::Character(c) => *c,
        _ => {
            return Err(EvalError::TypeError(
                "write-char expects a character".to_string(),
            ));
        }
    };

    let port = get_output_port(&args, 1)?;
    port.write_char(ch)
        .map_err(|e| EvalError::IOError(e.to_string()))?;

    Ok(Value::Unspecified)
}

/// (write-string string [port [start [end]]]) - Write a string
fn write_string(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() || args.len() > 4 {
        return Err(EvalError::WrongArity {
            expected: "write-string expects 1 to 4 arguments".to_string(),
            actual: args.len(),
        });
    }

    let s = match &args[0] {
        Value::String(s) => s.borrow().clone(),
        _ => {
            return Err(EvalError::TypeError(
                "write-string expects a string".to_string(),
            ));
        }
    };

    let port = get_output_port(&args, 1)?;

    // Handle optional start/end indices
    let start = if args.len() > 2 {
        match &args[2] {
            Value::Integer(n) => *n as usize,
            _ => {
                return Err(EvalError::TypeError(
                    "write-string: start must be an integer".to_string(),
                ));
            }
        }
    } else {
        0
    };

    let end = if args.len() > 3 {
        match &args[3] {
            Value::Integer(n) => *n as usize,
            _ => {
                return Err(EvalError::TypeError(
                    "write-string: end must be an integer".to_string(),
                ));
            }
        }
    } else {
        s.len()
    };

    // Get substring by character indices (not byte indices)
    let chars: Vec<char> = s.chars().collect();
    if start > chars.len() || end > chars.len() || start > end {
        return Err(EvalError::IndexOutOfBounds(format!(
            "write-string: index {} out of range for string of length {}",
            if start > chars.len() { start } else { end },
            chars.len()
        )));
    }

    let substr: String = chars[start..end].iter().collect();
    port.write_string(&substr)
        .map_err(|e| EvalError::IOError(e.to_string()))?;

    Ok(Value::Unspecified)
}

// =============================================================================
// Text Input
// =============================================================================

/// (read-char [port]) - Read a single character
fn read_char(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "read-char expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_input_port(&args, 0)?;
    match port.read_char() {
        Ok(Some(ch)) => Ok(Value::Character(ch)),
        Ok(None) => Ok(Value::Eof),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (peek-char [port]) - Peek at next character without consuming it
fn peek_char(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "peek-char expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_input_port(&args, 0)?;
    match port.peek_char() {
        Ok(Some(ch)) => Ok(Value::Character(ch)),
        Ok(None) => Ok(Value::Eof),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (char-ready? [port]) - Check if a character is ready to be read
fn char_ready_p(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "char-ready? expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_input_port(&args, 0)?;
    match port.char_ready() {
        Ok(ready) => Ok(Value::Boolean(ready)),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (read-line [port]) - Read a line as a string
fn read_line(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "read-line expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_input_port(&args, 0)?;
    match port.read_line() {
        Ok(Some(mut line)) => {
            // Remove trailing newline if present (R7RS behavior)
            if line.ends_with('\n') {
                line.pop();
                if line.ends_with('\r') {
                    line.pop();
                }
            }
            Ok(Value::String(Rc::new(RefCell::new(line))))
        }
        Ok(None) => Ok(Value::Eof),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (read-string k [port]) - Read k characters from port
/// Returns a string of up to k characters, or eof-object if at EOF before reading any
fn read_string(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "read-string expects 1 or 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let k = match &args[0] {
        Value::Integer(n) if *n >= 0 => *n as usize,
        Value::Integer(_) => {
            return Err(EvalError::TypeError(
                "read-string: k must be a non-negative integer".to_string(),
            ));
        }
        _ => {
            return Err(EvalError::TypeError(
                "read-string: k must be an integer".to_string(),
            ));
        }
    };

    let port = get_input_port(&args, 1)?;

    // Special case: reading 0 characters returns empty string
    if k == 0 {
        return Ok(Value::String(Rc::new(RefCell::new(String::new()))));
    }

    let mut result = String::new();
    for _ in 0..k {
        match port.read_char() {
            Ok(Some(ch)) => result.push(ch),
            Ok(None) => break, // EOF
            Err(e) => return Err(EvalError::IOError(e.to_string())),
        }
    }

    if result.is_empty() {
        // If we couldn't read any characters (hit EOF immediately), return EOF
        Ok(Value::Eof)
    } else {
        Ok(Value::String(Rc::new(RefCell::new(result))))
    }
}

// =============================================================================
// Binary I/O
// =============================================================================

/// Helper to get a binary input port from args
fn get_binary_input_port(args: &[Value], idx: usize) -> Result<Rc<Port>, EvalError> {
    if args.len() > idx {
        match &args[idx] {
            Value::Port(p) => {
                if !p.is_input() {
                    return Err(EvalError::TypeError("expected input port".to_string()));
                }
                if !p.is_binary() {
                    return Err(EvalError::TypeError("expected binary port".to_string()));
                }
                Ok(p.clone())
            }
            _ => Err(EvalError::TypeError("expected port".to_string())),
        }
    } else {
        // No port specified - for binary operations, we need an explicit port
        Err(EvalError::TypeError(
            "binary I/O operations require an explicit port".to_string(),
        ))
    }
}

/// Helper to get a binary output port from args
fn get_binary_output_port(args: &[Value], idx: usize) -> Result<Rc<Port>, EvalError> {
    if args.len() > idx {
        match &args[idx] {
            Value::Port(p) => {
                if !p.is_output() {
                    return Err(EvalError::TypeError("expected output port".to_string()));
                }
                if !p.is_binary() {
                    return Err(EvalError::TypeError("expected binary port".to_string()));
                }
                Ok(p.clone())
            }
            _ => Err(EvalError::TypeError("expected port".to_string())),
        }
    } else {
        // No port specified - for binary operations, we need an explicit port
        Err(EvalError::TypeError(
            "binary I/O operations require an explicit port".to_string(),
        ))
    }
}

/// (read-u8 [port]) - Read a single byte from a binary input port
fn read_u8(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "read-u8 expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_binary_input_port(&args, 0)?;
    match port.read_u8() {
        Ok(Some(byte)) => Ok(Value::Integer(byte as i64)),
        Ok(None) => Ok(Value::Eof),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (peek-u8 [port]) - Peek at next byte without consuming it
fn peek_u8(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "peek-u8 expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_binary_input_port(&args, 0)?;
    match port.peek_u8() {
        Ok(Some(byte)) => Ok(Value::Integer(byte as i64)),
        Ok(None) => Ok(Value::Eof),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (u8-ready? [port]) - Check if a byte is ready to be read
fn u8_ready_p(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "u8-ready? expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_binary_input_port(&args, 0)?;
    match port.u8_ready() {
        Ok(ready) => Ok(Value::Boolean(ready)),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (write-u8 byte [port]) - Write a byte to a binary output port
fn write_u8(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "write-u8 expects 1 or 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let byte = match &args[0] {
        Value::Integer(n) if *n >= 0 && *n <= 255 => *n as u8,
        Value::Integer(_) => {
            return Err(EvalError::TypeError(
                "write-u8: byte must be an exact integer in [0, 255]".to_string(),
            ));
        }
        _ => {
            return Err(EvalError::TypeError(
                "write-u8: first argument must be an exact integer".to_string(),
            ));
        }
    };

    let port = get_binary_output_port(&args, 1)?;
    match port.write_u8(byte) {
        Ok(()) => Ok(Value::Unspecified),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (read-bytevector k [port]) - Read up to k bytes from a binary input port
fn read_bytevector(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "read-bytevector expects 1 or 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let k = match &args[0] {
        Value::Integer(n) if *n >= 0 => *n as usize,
        Value::Integer(_) => {
            return Err(EvalError::TypeError(
                "read-bytevector: k must be a non-negative integer".to_string(),
            ));
        }
        _ => {
            return Err(EvalError::TypeError(
                "read-bytevector: k must be an integer".to_string(),
            ));
        }
    };

    let port = get_binary_input_port(&args, 1)?;
    match port.read_bytevector(k) {
        Ok(Some(bytes)) => Ok(Value::Bytevector(Rc::new(RefCell::new(bytes)))),
        Ok(None) => Ok(Value::Eof),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (read-bytevector! bytevector [port [start [end]]]) - Read into existing bytevector
fn read_bytevector_bang(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() || args.len() > 4 {
        return Err(EvalError::WrongArity {
            expected: "read-bytevector! expects 1-4 arguments".to_string(),
            actual: args.len(),
        });
    }

    let bv = match &args[0] {
        Value::Bytevector(bv) => bv.clone(),
        _ => {
            return Err(EvalError::TypeError(
                "read-bytevector!: first argument must be a bytevector".to_string(),
            ));
        }
    };

    let port = if args.len() > 1 {
        get_binary_input_port(&args, 1)?
    } else {
        return Err(EvalError::TypeError(
            "read-bytevector!: port argument required".to_string(),
        ));
    };

    let bv_len = bv.borrow().len();

    let start = if args.len() > 2 {
        match &args[2] {
            Value::Integer(n) if *n >= 0 && (*n as usize) <= bv_len => *n as usize,
            Value::Integer(_) => {
                return Err(EvalError::TypeError(
                    "read-bytevector!: start out of range".to_string(),
                ));
            }
            _ => {
                return Err(EvalError::TypeError(
                    "read-bytevector!: start must be an integer".to_string(),
                ));
            }
        }
    } else {
        0
    };

    let end = if args.len() > 3 {
        match &args[3] {
            Value::Integer(n) if *n >= 0 && (*n as usize) <= bv_len && (*n as usize) >= start => {
                *n as usize
            }
            Value::Integer(_) => {
                return Err(EvalError::TypeError(
                    "read-bytevector!: end out of range".to_string(),
                ));
            }
            _ => {
                return Err(EvalError::TypeError(
                    "read-bytevector!: end must be an integer".to_string(),
                ));
            }
        }
    } else {
        bv_len
    };

    let mut bv_mut = bv.borrow_mut();
    match port.read_bytevector_into(&mut bv_mut, start, end) {
        Ok(Some(n)) => Ok(Value::Integer(n as i64)),
        Ok(None) => Ok(Value::Eof),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

/// (write-bytevector bytevector [port [start [end]]]) - Write bytevector to port
fn write_bytevector(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() || args.len() > 4 {
        return Err(EvalError::WrongArity {
            expected: "write-bytevector expects 1-4 arguments".to_string(),
            actual: args.len(),
        });
    }

    let bv = match &args[0] {
        Value::Bytevector(bv) => bv.clone(),
        _ => {
            return Err(EvalError::TypeError(
                "write-bytevector: first argument must be a bytevector".to_string(),
            ));
        }
    };

    let port = if args.len() > 1 {
        get_binary_output_port(&args, 1)?
    } else {
        return Err(EvalError::TypeError(
            "write-bytevector: port argument required".to_string(),
        ));
    };

    let bv_borrowed = bv.borrow();
    let bv_len = bv_borrowed.len();

    let start = if args.len() > 2 {
        match &args[2] {
            Value::Integer(n) if *n >= 0 && (*n as usize) <= bv_len => *n as usize,
            Value::Integer(_) => {
                return Err(EvalError::TypeError(
                    "write-bytevector: start out of range".to_string(),
                ));
            }
            _ => {
                return Err(EvalError::TypeError(
                    "write-bytevector: start must be an integer".to_string(),
                ));
            }
        }
    } else {
        0
    };

    let end = if args.len() > 3 {
        match &args[3] {
            Value::Integer(n) if *n >= 0 && (*n as usize) <= bv_len && (*n as usize) >= start => {
                *n as usize
            }
            Value::Integer(_) => {
                return Err(EvalError::TypeError(
                    "write-bytevector: end out of range".to_string(),
                ));
            }
            _ => {
                return Err(EvalError::TypeError(
                    "write-bytevector: end must be an integer".to_string(),
                ));
            }
        }
    } else {
        bv_len
    };

    match port.write_bytevector(&bv_borrowed[start..end]) {
        Ok(()) => Ok(Value::Unspecified),
        Err(e) => Err(EvalError::IOError(e.to_string())),
    }
}

// =============================================================================
// Read (S-expression parsing)
// =============================================================================

/// (read [port]) - Read a Scheme expression from an input port
/// Returns the parsed value, or eof-object if at end of input
fn read(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() > 1 {
        return Err(EvalError::WrongArity {
            expected: "read expects 0 or 1 arguments".to_string(),
            actual: args.len(),
        });
    }

    let port = get_input_port(&args, 0)?;

    // Determine what kind of port we have and get content if applicable
    let (remaining, is_stdin, is_file) = {
        let data = port.data.borrow();
        match &*data {
            PortData::String(s) => {
                // For string ports, copy the remaining content
                (Some(s.content[s.position..].to_string()), false, false)
            }
            PortData::Stdio(patina_runtime::StdioKind::Stdin) => (None, true, false),
            PortData::Stdio(_) => {
                return Err(EvalError::TypeError("not an input port".to_string()));
            }
            PortData::File(_) => (None, false, true),
            PortData::Bytevector(_) => {
                return Err(EvalError::TypeError("read: not a textual port".to_string()));
            }
            PortData::Closed => {
                return Err(EvalError::IOError("port is closed".to_string()));
            }
        }
    };

    if is_stdin {
        return read_from_stdin();
    }

    if is_file {
        return read_from_file_port(&port);
    }

    let remaining = remaining.unwrap();
    if remaining.trim().is_empty() {
        return Ok(Value::Eof);
    }

    // Parse the expression
    let mut parser =
        Parser::new(&remaining).map_err(|e| EvalError::InvalidSyntax(format!("read: {}", e)))?;

    let result = parser.parse();

    match result {
        Ok(value) => {
            // Calculate bytes consumed by finding how much the parser consumed
            let consumed = calculate_consumed_bytes(&remaining, &value);
            port.advance_position(consumed)
                .map_err(|e| EvalError::IOError(e.to_string()))?;
            Ok(value)
        }
        Err(patina_frontend::ParseError::UnexpectedEof) => Ok(Value::Eof),
        Err(e) => Err(EvalError::InvalidSyntax(format!("read: {}", e))),
    }
}

/// Read a complete S-expression from stdin
/// This accumulates lines until we have a complete expression
fn read_from_stdin() -> Result<Value, EvalError> {
    let stdin = io::stdin();
    let mut handle = stdin.lock();
    let mut buffer = String::new();

    loop {
        let mut line = String::new();
        match handle.read_line(&mut line) {
            Ok(0) => {
                // EOF
                if buffer.trim().is_empty() {
                    return Ok(Value::Eof);
                }
                // Try to parse what we have
                break;
            }
            Ok(_) => {
                buffer.push_str(&line);
                // Try to parse - if successful, we're done
                // If we get UnexpectedEof, we need more input
                if let Ok(mut parser) = Parser::new(&buffer) {
                    match parser.parse() {
                        Ok(value) => return Ok(value),
                        Err(patina_frontend::ParseError::UnexpectedEof) => {
                            // Need more input
                            continue;
                        }
                        Err(e) => {
                            return Err(EvalError::InvalidSyntax(format!("read: {}", e)));
                        }
                    }
                }
            }
            Err(e) => return Err(EvalError::IOError(e.to_string())),
        }
    }

    // Try to parse the accumulated buffer
    let mut parser =
        Parser::new(&buffer).map_err(|e| EvalError::InvalidSyntax(format!("read: {}", e)))?;

    match parser.parse() {
        Ok(value) => Ok(value),
        Err(patina_frontend::ParseError::UnexpectedEof) => Ok(Value::Eof),
        Err(e) => Err(EvalError::InvalidSyntax(format!("read: {}", e))),
    }
}

/// Read a complete S-expression from a file port
/// Reads lines until we have a complete expression
fn read_from_file_port(port: &Rc<Port>) -> Result<Value, EvalError> {
    let mut buffer = String::new();

    loop {
        // Read a line from the file
        match port.read_line() {
            Ok(None) => {
                // EOF
                if buffer.trim().is_empty() {
                    return Ok(Value::Eof);
                }
                // Try to parse what we have
                break;
            }
            Ok(Some(line)) => {
                buffer.push_str(&line);
                // Try to parse - if successful, we're done
                // If we get UnexpectedEof, we need more input
                if let Ok(mut parser) = Parser::new(&buffer) {
                    match parser.parse() {
                        Ok(value) => return Ok(value),
                        Err(patina_frontend::ParseError::UnexpectedEof) => {
                            // Need more input
                            continue;
                        }
                        Err(e) => {
                            return Err(EvalError::InvalidSyntax(format!("read: {}", e)));
                        }
                    }
                }
            }
            Err(e) => return Err(EvalError::IOError(e.to_string())),
        }
    }

    // Try to parse the accumulated buffer
    let mut parser =
        Parser::new(&buffer).map_err(|e| EvalError::InvalidSyntax(format!("read: {}", e)))?;

    match parser.parse() {
        Ok(value) => Ok(value),
        Err(patina_frontend::ParseError::UnexpectedEof) => Ok(Value::Eof),
        Err(e) => Err(EvalError::InvalidSyntax(format!("read: {}", e))),
    }
}

/// Calculate how many bytes were consumed to parse a value
/// This is done by re-parsing and finding where the parser stops
fn calculate_consumed_bytes(input: &str, _parsed: &Value) -> usize {
    // Skip leading whitespace
    let trimmed = input.trim_start();
    let whitespace_len = input.len() - trimmed.len();

    if trimmed.is_empty() {
        return input.len();
    }

    // Parse once and measure by trying increasingly smaller substrings
    // until parsing fails or gives a different result
    //
    // A more efficient approach: find the logical end of the expression
    // by tracking parentheses/brackets and string boundaries
    let mut depth = 0;
    let mut in_string = false;
    let mut in_line_comment = false;
    let mut i = 0;
    let chars: Vec<char> = trimmed.chars().collect();

    while i < chars.len() {
        let ch = chars[i];

        if in_line_comment {
            if ch == '\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }

        if in_string {
            if ch == '\\' && i + 1 < chars.len() {
                i += 2; // Skip escape sequence
                continue;
            }
            if ch == '"' {
                in_string = false;
                if depth == 0 {
                    i += 1;
                    break;
                }
            }
            i += 1;
            continue;
        }

        match ch {
            ';' => {
                in_line_comment = true;
                i += 1;
            }
            '"' => {
                in_string = true;
                i += 1;
            }
            '(' | '[' | '{' => {
                depth += 1;
                i += 1;
            }
            ')' | ']' | '}' => {
                depth -= 1;
                i += 1;
                if depth == 0 {
                    break;
                }
            }
            '#' if i + 1 < chars.len() && chars[i + 1] == '(' => {
                // Vector #(
                depth += 1;
                i += 2;
            }
            '#' if i + 3 < chars.len()
                && chars[i + 1] == 'u'
                && chars[i + 2] == '8'
                && chars[i + 3] == '(' =>
            {
                // Bytevector #u8(
                depth += 1;
                i += 4;
            }
            '\'' | '`' => {
                // Quote/quasiquote - need to include the following expression
                i += 1;
            }
            ',' => {
                // Unquote - may be ,@ or just ,
                i += 1;
                if i < chars.len() && chars[i] == '@' {
                    i += 1;
                }
            }
            _ if ch.is_whitespace() => {
                if depth == 0 && i > 0 {
                    // End of atom at top level
                    break;
                }
                i += 1;
            }
            _ => {
                // Part of an atom
                if depth == 0 {
                    // Read until delimiter
                    while i < chars.len() {
                        let c = chars[i];
                        if c.is_whitespace()
                            || c == '('
                            || c == ')'
                            || c == '"'
                            || c == ';'
                            || c == '\''
                            || c == '`'
                            || c == ','
                        {
                            break;
                        }
                        i += 1;
                    }
                    break;
                }
                i += 1;
            }
        }
    }

    // Calculate byte offset from character offset
    let consumed_chars: String = chars[..i].iter().collect();
    whitespace_len + consumed_chars.len()
}

// =============================================================================
// Registration
// =============================================================================

pub(super) fn register(registry: &mut super::PrimitiveRegistry) {
    // Port predicates
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "port?",
        Arity::Exact(1),
        "Returns #t if obj is a port.",
        |eval, args, _tail| port_p(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "input-port?",
        Arity::Exact(1),
        "Returns #t if obj is an input port.",
        |eval, args, _tail| input_port_p(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "output-port?",
        Arity::Exact(1),
        "Returns #t if obj is an output port.",
        |eval, args, _tail| output_port_p(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "textual-port?",
        Arity::Exact(1),
        "Returns #t if obj is a textual port.",
        |eval, args, _tail| textual_port_p(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "binary-port?",
        Arity::Exact(1),
        "Returns #t if obj is a binary port.",
        |eval, args, _tail| binary_port_p(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "input-port-open?",
        Arity::Exact(1),
        "Returns #t if port is an open input port.",
        |eval, args, _tail| input_port_open_p(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "output-port-open?",
        Arity::Exact(1),
        "Returns #t if port is an open output port.",
        |eval, args, _tail| output_port_open_p(eval, args).map(EvalResult::Value),
    ));

    // String ports
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "open-input-string",
        Arity::Exact(1),
        "Create an input port from a string.",
        |eval, args, _tail| open_input_string(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "open-output-string",
        Arity::Exact(0),
        "Create an output port that accumulates to a string.",
        |eval, args, _tail| open_output_string(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "get-output-string",
        Arity::Exact(1),
        "Get the accumulated string from an output string port.",
        |eval, args, _tail| get_output_string(eval, args).map(EvalResult::Value),
    ));

    // Bytevector ports
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "open-input-bytevector",
        Arity::Exact(1),
        "Create an input port from a bytevector.",
        |eval, args, _tail| open_input_bytevector(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "open-output-bytevector",
        Arity::Exact(0),
        "Create an output port that accumulates to a bytevector.",
        |eval, args, _tail| open_output_bytevector(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "get-output-bytevector",
        Arity::Exact(1),
        "Get the accumulated bytevector from an output bytevector port.",
        |eval, args, _tail| get_output_bytevector(eval, args).map(EvalResult::Value),
    ));

    // Current ports
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "current-input-port",
        Arity::Exact(0),
        "Returns the current input port.",
        |eval, args, _tail| current_input_port(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "current-output-port",
        Arity::Exact(0),
        "Returns the current output port.",
        |eval, args, _tail| current_output_port(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "current-error-port",
        Arity::Exact(0),
        "Returns the current error port.",
        |eval, args, _tail| current_error_port(eval, args).map(EvalResult::Value),
    ));

    // Port operations
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "close-port",
        Arity::Exact(1),
        "Close a port.",
        |eval, args, _tail| close_port(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "close-input-port",
        Arity::Exact(1),
        "Close an input port.",
        |eval, args, _tail| close_input_port(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "close-output-port",
        Arity::Exact(1),
        "Close an output port.",
        |eval, args, _tail| close_output_port(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "call-with-port",
        Arity::Exact(2),
        "Calls proc with port, then closes the port.",
        call_with_port,
    ));

    // EOF handling
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "eof-object?",
        Arity::Exact(1),
        "Returns #t if obj is the EOF object.",
        |eval, args, _tail| eof_object_p(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "eof-object",
        Arity::Exact(0),
        "Returns an EOF object.",
        |eval, args, _tail| eof_object(eval, args).map(EvalResult::Value),
    ));

    // Text output
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "display",
        Arity::Range(1, 2),
        "Writes obj to the textual output port in a human-readable format.",
        |eval, args, _tail| display(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "write",
        Arity::Range(1, 2),
        "Writes obj to the textual output port in a machine-readable format.",
        |eval, args, _tail| write(eval, args).map(EvalResult::Value),
    ));

    // (scheme write) library procedures
    // display and write are also exported from (scheme write)
    registry.register(PrimitiveFn::new(
        "scheme.write",
        "display",
        Arity::Range(1, 2),
        "Writes obj to the textual output port in a human-readable format.",
        |eval, args, _tail| display(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.write",
        "write",
        Arity::Range(1, 2),
        "Writes obj to the textual output port in a machine-readable format.",
        |eval, args, _tail| write(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.write",
        "write-shared",
        Arity::Range(1, 2),
        "Writes obj with datum labels for all shared structures.",
        |eval, args, _tail| write_shared(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.write",
        "write-simple",
        Arity::Range(1, 2),
        "Writes obj without datum labels (may loop on circular structures).",
        |eval, args, _tail| write_simple(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "newline",
        Arity::Range(0, 1),
        "Writes a newline to the textual output port.",
        |eval, args, _tail| newline(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "write-char",
        Arity::Range(1, 2),
        "Writes a character to the textual output port.",
        |eval, args, _tail| write_char(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "write-string",
        Arity::Range(1, 4),
        "Writes a string to the textual output port.",
        |eval, args, _tail| write_string(eval, args).map(EvalResult::Value),
    ));

    // Text input
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "read-char",
        Arity::Range(0, 1),
        "Reads a single character from the input port.",
        |eval, args, _tail| read_char(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "peek-char",
        Arity::Range(0, 1),
        "Peeks at the next character without consuming it.",
        |eval, args, _tail| peek_char(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "char-ready?",
        Arity::Range(0, 1),
        "Returns #t if a character is ready to be read.",
        |eval, args, _tail| char_ready_p(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "read-line",
        Arity::Range(0, 1),
        "Reads a line from the input port.",
        |eval, args, _tail| read_line(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "read-string",
        Arity::Range(1, 2),
        "Reads k characters from the input port.",
        |eval, args, _tail| read_string(eval, args).map(EvalResult::Value),
    ));

    // Binary I/O
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "read-u8",
        Arity::Range(0, 1),
        "Reads a single byte from a binary input port.",
        |eval, args, _tail| read_u8(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "peek-u8",
        Arity::Range(0, 1),
        "Peeks at the next byte without consuming it.",
        |eval, args, _tail| peek_u8(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "u8-ready?",
        Arity::Range(0, 1),
        "Returns #t if a byte is ready to be read.",
        |eval, args, _tail| u8_ready_p(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "write-u8",
        Arity::Range(1, 2),
        "Writes a byte to a binary output port.",
        |eval, args, _tail| write_u8(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "read-bytevector",
        Arity::Range(1, 2),
        "Reads up to k bytes from a binary input port.",
        |eval, args, _tail| read_bytevector(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "read-bytevector!",
        Arity::Range(1, 4),
        "Reads into an existing bytevector from a binary input port.",
        |eval, args, _tail| read_bytevector_bang(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "write-bytevector",
        Arity::Range(1, 4),
        "Writes a bytevector to a binary output port.",
        |eval, args, _tail| write_bytevector(eval, args).map(EvalResult::Value),
    ));

    // Read (S-expression parsing) - in (scheme base)
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "read",
        Arity::Range(0, 1),
        "Reads a Scheme expression from the input port.",
        |eval, args, _tail| read(eval, args).map(EvalResult::Value),
    ));

    // (scheme read) library - re-exports read
    registry.register(PrimitiveFn::new(
        "scheme.read",
        "read",
        Arity::Range(0, 1),
        "Reads a Scheme expression from the input port.",
        |eval, args, _tail| read(eval, args).map(EvalResult::Value),
    ));

    // File I/O operations (scheme file library)
    registry.register(PrimitiveFn::new(
        "scheme.file",
        "open-input-file",
        Arity::Exact(1),
        "Opens a file for reading and returns an input port.",
        |eval, args, _tail| open_input_file(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.file",
        "open-output-file",
        Arity::Exact(1),
        "Opens a file for writing and returns an output port.",
        |eval, args, _tail| open_output_file(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.file",
        "open-binary-input-file",
        Arity::Exact(1),
        "Opens a binary file for reading and returns an input port.",
        |eval, args, _tail| open_binary_input_file(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.file",
        "open-binary-output-file",
        Arity::Exact(1),
        "Opens a binary file for writing and returns an output port.",
        |eval, args, _tail| open_binary_output_file(eval, args).map(EvalResult::Value),
    ));

    // Flush is in scheme base
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "flush-output-port",
        Arity::Range(0, 1),
        "Flushes the output port buffer.",
        |eval, args, _tail| flush_output_port(eval, args).map(EvalResult::Value),
    ));

    // File utilities (scheme file library)
    registry.register(PrimitiveFn::new(
        "scheme.file",
        "file-exists?",
        Arity::Exact(1),
        "Returns #t if the file exists.",
        |eval, args, _tail| file_exists_p(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.file",
        "delete-file",
        Arity::Exact(1),
        "Deletes the specified file.",
        |eval, args, _tail| delete_file(eval, args).map(EvalResult::Value),
    ));

    // Higher-order file operations (scheme file library)
    registry.register(PrimitiveFn::new(
        "scheme.file",
        "call-with-input-file",
        Arity::Exact(2),
        "Opens file for reading, calls proc with port, closes port.",
        call_with_input_file,
    ));

    registry.register(PrimitiveFn::new(
        "scheme.file",
        "call-with-output-file",
        Arity::Exact(2),
        "Opens file for writing, calls proc with port, closes port.",
        call_with_output_file,
    ));

    registry.register(PrimitiveFn::new(
        "scheme.file",
        "with-input-from-file",
        Arity::Exact(2),
        "Opens file and sets current-input-port for the duration of thunk.",
        with_input_from_file,
    ));

    registry.register(PrimitiveFn::new(
        "scheme.file",
        "with-output-to-file",
        Arity::Exact(2),
        "Opens file and sets current-output-port for the duration of thunk.",
        with_output_to_file,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_port_round_trip() {
        let eval = Evaluator::new();

        // Create output port and write to it
        let out_port = open_output_string(&eval, vec![]).unwrap();

        if let Value::Port(p) = &out_port {
            p.write_string("hello").unwrap();
        }

        // Get the string back
        let result = get_output_string(&eval, vec![out_port]).unwrap();
        if let Value::String(s) = result {
            assert_eq!(*s.borrow(), "hello");
        } else {
            panic!("Expected string");
        }
    }

    #[test]
    fn test_input_string_port() {
        let eval = Evaluator::new();
        let input = Value::String(Rc::new(RefCell::new("abc".to_string())));
        let port = open_input_string(&eval, vec![input]).unwrap();

        // Read characters
        let ch1 = read_char(&eval, vec![port.clone()]).unwrap();
        assert!(matches!(ch1, Value::Character('a')));

        let ch2 = read_char(&eval, vec![port.clone()]).unwrap();
        assert!(matches!(ch2, Value::Character('b')));

        let ch3 = read_char(&eval, vec![port.clone()]).unwrap();
        assert!(matches!(ch3, Value::Character('c')));

        let eof = read_char(&eval, vec![port]).unwrap();
        assert!(matches!(eof, Value::Eof));
    }

    #[test]
    fn test_port_predicates() {
        let eval = Evaluator::new();

        let in_port = open_input_string(
            &eval,
            vec![Value::String(Rc::new(RefCell::new("".to_string())))],
        )
        .unwrap();
        let out_port = open_output_string(&eval, vec![]).unwrap();

        assert!(matches!(
            port_p(&eval, vec![in_port.clone()]),
            Ok(Value::Boolean(true))
        ));
        assert!(matches!(
            input_port_p(&eval, vec![in_port.clone()]),
            Ok(Value::Boolean(true))
        ));
        assert!(matches!(
            output_port_p(&eval, vec![in_port.clone()]),
            Ok(Value::Boolean(false))
        ));
        assert!(matches!(
            textual_port_p(&eval, vec![in_port]),
            Ok(Value::Boolean(true))
        ));

        assert!(matches!(
            port_p(&eval, vec![out_port.clone()]),
            Ok(Value::Boolean(true))
        ));
        assert!(matches!(
            input_port_p(&eval, vec![out_port.clone()]),
            Ok(Value::Boolean(false))
        ));
        assert!(matches!(
            output_port_p(&eval, vec![out_port]),
            Ok(Value::Boolean(true))
        ));

        // Non-port value
        assert!(matches!(
            port_p(&eval, vec![Value::Integer(42)]),
            Ok(Value::Boolean(false))
        ));
    }

    #[test]
    fn test_eof_handling() {
        let eval = Evaluator::new();

        let eof = eof_object(&eval, vec![]).unwrap();
        assert!(matches!(eof, Value::Eof));

        let is_eof = eof_object_p(&eval, vec![eof]).unwrap();
        assert!(matches!(is_eof, Value::Boolean(true)));

        let not_eof = eof_object_p(&eval, vec![Value::Integer(42)]).unwrap();
        assert!(matches!(not_eof, Value::Boolean(false)));
    }

    #[test]
    fn test_read_basic() {
        let eval = Evaluator::new();

        // Read an integer
        let input = Value::String(Rc::new(RefCell::new("42".to_string())));
        let port = open_input_string(&eval, vec![input]).unwrap();
        let result = read(&eval, vec![port.clone()]).unwrap();
        assert!(matches!(result, Value::Integer(42)));

        // Subsequent read should return EOF
        let eof = read(&eval, vec![port]).unwrap();
        assert!(matches!(eof, Value::Eof));
    }

    #[test]
    fn test_read_list() {
        let eval = Evaluator::new();

        let input = Value::String(Rc::new(RefCell::new("(+ 1 2)".to_string())));
        let port = open_input_string(&eval, vec![input]).unwrap();
        let result = read(&eval, vec![port]).unwrap();
        assert!(matches!(result, Value::Pair(_)));
    }

    #[test]
    fn test_read_multiple_expressions() {
        let eval = Evaluator::new();

        let input = Value::String(Rc::new(RefCell::new("1 2 3".to_string())));
        let port = open_input_string(&eval, vec![input]).unwrap();

        let r1 = read(&eval, vec![port.clone()]).unwrap();
        assert!(matches!(r1, Value::Integer(1)));

        let r2 = read(&eval, vec![port.clone()]).unwrap();
        assert!(matches!(r2, Value::Integer(2)));

        let r3 = read(&eval, vec![port.clone()]).unwrap();
        assert!(matches!(r3, Value::Integer(3)));

        let eof = read(&eval, vec![port]).unwrap();
        assert!(matches!(eof, Value::Eof));
    }

    #[test]
    fn test_read_quoted() {
        let eval = Evaluator::new();

        let input = Value::String(Rc::new(RefCell::new("'foo".to_string())));
        let port = open_input_string(&eval, vec![input]).unwrap();
        let result = read(&eval, vec![port]).unwrap();
        // Should be (quote foo)
        assert!(matches!(result, Value::Pair(_)));
    }

    #[test]
    fn test_read_sexp_string() {
        let eval = Evaluator::new();

        let input = Value::String(Rc::new(RefCell::new("\"hello world\"".to_string())));
        let port = open_input_string(&eval, vec![input]).unwrap();
        let result = read(&eval, vec![port]).unwrap();
        if let Value::String(s) = result {
            assert_eq!(*s.borrow(), "hello world");
        } else {
            panic!("Expected string, got {:?}", result);
        }
    }

    #[test]
    fn test_read_string_basic() {
        let eval = Evaluator::new();

        // Read 3 chars from "abcd"
        let input = Value::String(Rc::new(RefCell::new("abcd".to_string())));
        let port = open_input_string(&eval, vec![input]).unwrap();
        let result = read_string(&eval, vec![Value::Integer(3), port]).unwrap();
        if let Value::String(s) = result {
            assert_eq!(*s.borrow(), "abc");
        } else {
            panic!("Expected string, got {:?}", result);
        }
    }

    #[test]
    fn test_read_string_eof() {
        let eval = Evaluator::new();

        // Read from empty string returns EOF
        let input = Value::String(Rc::new(RefCell::new("".to_string())));
        let port = open_input_string(&eval, vec![input]).unwrap();
        let result = read_string(&eval, vec![Value::Integer(3), port]).unwrap();
        assert!(matches!(result, Value::Eof));
    }

    #[test]
    fn test_read_string_partial() {
        let eval = Evaluator::new();

        // Read more chars than available
        let input = Value::String(Rc::new(RefCell::new("hi".to_string())));
        let port = open_input_string(&eval, vec![input]).unwrap();
        let result = read_string(&eval, vec![Value::Integer(10), port]).unwrap();
        if let Value::String(s) = result {
            assert_eq!(*s.borrow(), "hi");
        } else {
            panic!("Expected string, got {:?}", result);
        }
    }

    #[test]
    fn test_read_string_zero() {
        let eval = Evaluator::new();

        // Read 0 chars returns empty string (not EOF)
        let input = Value::String(Rc::new(RefCell::new("hello".to_string())));
        let port = open_input_string(&eval, vec![input]).unwrap();
        let result = read_string(&eval, vec![Value::Integer(0), port]).unwrap();
        if let Value::String(s) = result {
            assert_eq!(*s.borrow(), "");
        } else {
            panic!("Expected empty string, got {:?}", result);
        }
    }

    #[test]
    fn test_read_string_with_newline() {
        let eval = Evaluator::new();

        // Read includes newline character
        let input = Value::String(Rc::new(RefCell::new("abc\ndef".to_string())));
        let port = open_input_string(&eval, vec![input]).unwrap();
        let result = read_string(&eval, vec![Value::Integer(5), port]).unwrap();
        if let Value::String(s) = result {
            assert_eq!(*s.borrow(), "abc\nd");
        } else {
            panic!("Expected string, got {:?}", result);
        }
    }
}
