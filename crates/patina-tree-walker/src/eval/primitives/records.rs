//! Record type primitives (R7RS Section 5.5)
//!
//! Implements low-level primitives for the record type system.
//! These are internal primitives (prefixed with %) used by the
//! define-record-type macro in base-extras.scm.
//!
//! The primitives provide:
//! - Record type creation (%make-record-type)
//! - Record instance creation (%make-record)
//! - Field access (%record-ref, %record-set!)
//! - Type predicates (%record?, %record-type?)
//! - Introspection (%record-type-of, %record-type-name, %record-type-fields)

use super::super::EvalResult;
use super::super::Evaluator;
use super::super::error::EvalError;
use super::registry::{PrimitiveFn, PrimitiveRegistry};
use patina_runtime::value::{Arity, RecordTypeDescriptor, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Extract a symbol name from a Value
fn extract_symbol(value: &Value) -> Result<String, EvalError> {
    match value {
        Value::Symbol(s) => Ok(s.to_string()),
        Value::Identifier(id) => Ok(id.name.to_string()),
        _ => Err(EvalError::TypeError(format!(
            "Expected symbol, got {}",
            value.type_name()
        ))),
    }
}

/// Extract a list of symbols from a Value
fn extract_symbol_list(value: &Value) -> Result<Vec<String>, EvalError> {
    let mut result = Vec::new();
    let mut current = value.clone();

    loop {
        match current {
            Value::Null => break,
            Value::Pair(pair) => {
                let (car, cdr) = pair.borrow().clone();
                result.push(extract_symbol(&car)?);
                current = cdr;
            }
            _ => {
                return Err(EvalError::TypeError(format!(
                    "Expected proper list of symbols, got {}",
                    current.type_name()
                )));
            }
        }
    }

    Ok(result)
}

/// %make-record-type: Create a new record type descriptor
///
/// (define rtd (%make-record-type 'name '(field1 field2 ...)))
///
/// Returns a new RecordTypeDescriptor with a unique ID (generative semantics).
fn make_record_type(_evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "%make-record-type expects 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let name = extract_symbol(&args[0])?;
    let fields = extract_symbol_list(&args[1])?;

    let rtd = RecordTypeDescriptor::new(&name, fields);
    Ok(Value::RecordType(Rc::new(rtd)))
}

/// %record-type?: Check if value is a record type descriptor
fn record_type_p(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.make_type_predicate(args, |v| matches!(v, Value::RecordType(_)))
}

/// %record?: Check if value is a record instance
fn record_p(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.make_type_predicate(args, |v| matches!(v, Value::Record { .. }))
}

/// %record-type-of: Get the record type descriptor from a record instance
///
/// (%record-type-of record) => rtd
fn record_type_of(_evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "%record-type-of expects 1 argument".to_string(),
            actual: args.len(),
        });
    }

    match &args[0] {
        Value::Record { record_type, .. } => Ok(Value::RecordType(record_type.clone())),
        _ => Err(EvalError::TypeError(format!(
            "%record-type-of: expected record, got {}",
            args[0].type_name()
        ))),
    }
}

/// %make-record: Create a new record instance
///
/// (%make-record rtd field-values-vector) => record
///
/// field-values-vector must be a vector with exactly as many elements
/// as the record type has fields.
fn make_record(_evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "%make-record expects 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let rtd = match &args[0] {
        Value::RecordType(rtd) => rtd.clone(),
        _ => {
            return Err(EvalError::TypeError(format!(
                "%make-record: expected record-type, got {}",
                args[0].type_name()
            )));
        }
    };

    let fields = match &args[1] {
        Value::Vector(v) => v.borrow().clone(),
        _ => {
            return Err(EvalError::TypeError(format!(
                "%make-record: expected vector of field values, got {}",
                args[1].type_name()
            )));
        }
    };

    if fields.len() != rtd.fields.len() {
        return Err(EvalError::InvalidSyntax(format!(
            "%make-record: expected {} fields, got {}",
            rtd.fields.len(),
            fields.len()
        )));
    }

    Ok(Value::Record {
        record_type: rtd,
        fields: Rc::new(RefCell::new(fields)),
    })
}

/// %record-ref: Read a field from a record by index
///
/// (%record-ref record index) => value
fn record_ref(_evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "%record-ref expects 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let (record_type, fields) = match &args[0] {
        Value::Record {
            record_type,
            fields,
        } => (record_type, fields),
        _ => {
            return Err(EvalError::TypeError(format!(
                "%record-ref: expected record, got {}",
                args[0].type_name()
            )));
        }
    };

    let index = match &args[1] {
        Value::Integer(n) => *n as usize,
        _ => {
            return Err(EvalError::TypeError(format!(
                "%record-ref: expected integer index, got {}",
                args[1].type_name()
            )));
        }
    };

    let fields_ref = fields.borrow();
    if index >= fields_ref.len() {
        return Err(EvalError::InvalidSyntax(format!(
            "%record-ref: index {} out of bounds for record type {} with {} fields",
            index,
            record_type.name,
            fields_ref.len()
        )));
    }

    Ok(fields_ref[index].clone())
}

/// %record-set!: Write a field in a record by index
///
/// (%record-set! record index value) => unspecified
fn record_set(_evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 3 {
        return Err(EvalError::WrongArity {
            expected: "%record-set! expects 3 arguments".to_string(),
            actual: args.len(),
        });
    }

    let (record_type, fields) = match &args[0] {
        Value::Record {
            record_type,
            fields,
        } => (record_type, fields),
        _ => {
            return Err(EvalError::TypeError(format!(
                "%record-set!: expected record, got {}",
                args[0].type_name()
            )));
        }
    };

    let index = match &args[1] {
        Value::Integer(n) => *n as usize,
        _ => {
            return Err(EvalError::TypeError(format!(
                "%record-set!: expected integer index, got {}",
                args[1].type_name()
            )));
        }
    };

    let mut fields_ref = fields.borrow_mut();
    if index >= fields_ref.len() {
        return Err(EvalError::InvalidSyntax(format!(
            "%record-set!: index {} out of bounds for record type {} with {} fields",
            index,
            record_type.name,
            fields_ref.len()
        )));
    }

    fields_ref[index] = args[2].clone();
    Ok(Value::Unspecified)
}

/// %record-type-name: Get the name of a record type as a symbol
///
/// (%record-type-name rtd) => symbol
fn record_type_name(_evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "%record-type-name expects 1 argument".to_string(),
            actual: args.len(),
        });
    }

    match &args[0] {
        Value::RecordType(rtd) => Ok(Value::symbol(&rtd.name)),
        _ => Err(EvalError::TypeError(format!(
            "%record-type-name: expected record-type, got {}",
            args[0].type_name()
        ))),
    }
}

/// %record-type-fields: Get the field names of a record type as a list
///
/// (%record-type-fields rtd) => (field1 field2 ...)
fn record_type_fields(_evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "%record-type-fields expects 1 argument".to_string(),
            actual: args.len(),
        });
    }

    match &args[0] {
        Value::RecordType(rtd) => {
            // Build a proper list of symbols from the field names
            let mut result = Value::Null;
            for field in rtd.fields.iter().rev() {
                result = Value::Pair(Rc::new(RefCell::new((Value::symbol(field), result))));
            }
            Ok(result)
        }
        _ => Err(EvalError::TypeError(format!(
            "%record-type-fields: expected record-type, got {}",
            args[0].type_name()
        ))),
    }
}

/// %record-type-field-index: Get the index of a field by name
///
/// (%record-type-field-index rtd 'field-name) => integer or #f
fn record_type_field_index(_evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "%record-type-field-index expects 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let rtd = match &args[0] {
        Value::RecordType(rtd) => rtd,
        _ => {
            return Err(EvalError::TypeError(format!(
                "%record-type-field-index: expected record-type, got {}",
                args[0].type_name()
            )));
        }
    };

    let field_name = extract_symbol(&args[1])?;

    match rtd.field_index(&field_name) {
        Some(index) => Ok(Value::Integer(index as i64)),
        None => Ok(Value::Boolean(false)),
    }
}

/// Register all record primitives
pub fn register(registry: &mut PrimitiveRegistry) {
    // Record type creation
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "%make-record-type",
        Arity::Exact(2),
        "Create a new record type descriptor with the given name and field names.",
        |eval, args, _tail| make_record_type(eval, args).map(EvalResult::Value),
    ));

    // Type predicates
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "%record-type?",
        Arity::Exact(1),
        "Returns #t if obj is a record type descriptor.",
        |eval, args, _tail| record_type_p(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "%record?",
        Arity::Exact(1),
        "Returns #t if obj is a record instance.",
        |eval, args, _tail| record_p(eval, args).map(EvalResult::Value),
    ));

    // Record type introspection
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "%record-type-of",
        Arity::Exact(1),
        "Returns the record type descriptor of a record instance.",
        |eval, args, _tail| record_type_of(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "%record-type-name",
        Arity::Exact(1),
        "Returns the name of a record type as a symbol.",
        |eval, args, _tail| record_type_name(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "%record-type-fields",
        Arity::Exact(1),
        "Returns the field names of a record type as a list of symbols.",
        |eval, args, _tail| record_type_fields(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "%record-type-field-index",
        Arity::Exact(2),
        "Returns the index of a field in a record type, or #f if not found.",
        |eval, args, _tail| record_type_field_index(eval, args).map(EvalResult::Value),
    ));

    // Record instance creation and access
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "%make-record",
        Arity::Exact(2),
        "Create a new record instance with the given type and field values vector.",
        |eval, args, _tail| make_record(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "%record-ref",
        Arity::Exact(2),
        "Read a field from a record by index.",
        |eval, args, _tail| record_ref(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "%record-set!",
        Arity::Exact(3),
        "Write a field in a record by index.",
        |eval, args, _tail| record_set(eval, args).map(EvalResult::Value),
    ));
}
