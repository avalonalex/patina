//! Datum Label Writer for handling circular and shared structures
//!
//! This module implements R7RS datum labels (#n= and #n#) for handling
//! circular and shared structures in output operations.

use patina_runtime::value::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Writer that handles circular/shared structures using datum labels
pub(super) struct DatumLabelWriter {
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
    pub(super) fn new(display_mode: bool, label_shared: bool) -> Self {
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
    pub(super) fn find_circular(&mut self, value: &Value) {
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
    pub(super) fn write_value(
        &self,
        value: &Value,
        out: &mut String,
        emitted: &mut HashMap<usize, bool>,
    ) {
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
pub(super) fn format_write(value: &Value) -> String {
    let mut writer = DatumLabelWriter::new(false, false); // don't label shared
    writer.find_circular(value);

    let mut output = String::new();
    let mut emitted = HashMap::new();
    writer.write_value(value, &mut output, &mut emitted);
    output
}

/// Format a value for display (human-readable, with datum labels for circular structures only)
pub(super) fn format_display(value: &Value) -> String {
    let mut writer = DatumLabelWriter::new(true, false); // don't label shared
    writer.find_circular(value);

    let mut output = String::new();
    let mut emitted = HashMap::new();
    writer.write_value(value, &mut output, &mut emitted);
    output
}

/// Format a value for write-shared (machine-readable, with datum labels for all shared structures)
pub(super) fn format_write_shared(value: &Value) -> String {
    let mut writer = DatumLabelWriter::new(false, true); // label shared
    writer.find_circular(value);

    let mut output = String::new();
    let mut emitted = HashMap::new();
    writer.write_value(value, &mut output, &mut emitted);
    output
}

/// Format a value for write-simple (machine-readable, no datum labels - may loop on circular structures)
pub(super) fn format_write_simple(value: &Value) -> String {
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
