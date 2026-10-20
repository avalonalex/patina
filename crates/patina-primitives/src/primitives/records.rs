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
//!
//! Each takes the context (#706). A record's fields are read and written by
//! value through the `Cx` (`Cx::record_ref`, and the funnel's
//! `RecordField` for `%record-set!`), never through the record's `Rc`
//! payload (#708).
use crate::registry::{PrimitiveFn, PrimitiveRegistry};
use patina_core::{Arity, RecordTypeDescriptor};
use patina_core::{Cx, RecordField, Value};
use patina_runtime::EvalError;
use std::rc::Rc;

/// Extract a symbol name from a value
fn extract_symbol_name<'gc>(cx: &Cx<'gc>, tv: Value<'gc>) -> Result<String, EvalError> {
    match cx.get_symbol_or_identifier_name(tv) {
        Some(name) => Ok(name.to_string()),
        None => Err(EvalError::TypeError(format!(
            "Expected symbol, got {}",
            cx.type_name(tv)
        ))),
    }
}

/// Extract a list of symbol names from a pair chain
fn extract_symbol_list<'gc>(cx: &Cx<'gc>, tv: Value<'gc>) -> Result<Vec<String>, EvalError> {
    let mut result = Vec::new();
    let mut current = tv;

    loop {
        if current == Value::NULL {
            break;
        }

        // try_pair extracts car and cdr from pairs
        let pair = cx.try_pair(current);
        if let Some((car, cdr)) = pair {
            let name = extract_symbol_name(cx, car)?;
            result.push(name);
            current = cdr;
        } else {
            return Err(EvalError::TypeError(format!(
                "Expected proper list of symbols, got {}",
                cx.type_name(current)
            )));
        }
    }

    Ok(result)
}

/// %make-record-type: Create a new record type descriptor
///
/// (define rtd (%make-record-type 'name '(field1 field2 ...)))
///
/// Returns a new RecordTypeDescriptor with a unique ID (generative semantics).
fn make_record_type<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "%make-record-type expects 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let name = extract_symbol_name(cx, args[0])?;
    let fields = extract_symbol_list(cx, args[1])?;

    let rtd = RecordTypeDescriptor::new(&name, fields);
    Ok(cx.alloc_record_type(Rc::new(rtd)))
}

/// %record-type?: Check if value is a record type descriptor
fn record_type_p<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    Ok(Value::boolean(cx.is_record_type(args[0])))
}

/// %record?: Check if value is a record instance
fn record_p<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    Ok(Value::boolean(cx.is_record(args[0])))
}

/// %record-type-of: Get the record type descriptor from a record instance
///
/// (%record-type-of record) => rtd
fn record_type_of<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "%record-type-of expects 1 argument".to_string(),
            actual: args.len(),
        });
    }

    if let Some(rtd) = cx.record_type_of(args[0]) {
        Ok(cx.alloc_record_type(rtd))
    } else {
        Err(EvalError::TypeError(format!(
            "%record-type-of: expected record, got {}",
            cx.type_name(args[0])
        )))
    }
}

/// %make-record: Create a new record instance
///
/// (%make-record rtd field-values-vector) => record
///
/// field-values-vector must be a vector with exactly as many elements
/// as the record type has fields.
fn make_record<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "%make-record expects 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    // Get the record type descriptor
    let rtd = if let Some(rtd) = cx.get_record_type(args[0]) {
        rtd
    } else {
        return Err(EvalError::TypeError(format!(
            "%make-record: expected record-type, got {}",
            cx.type_name(args[0])
        )));
    };

    // Get field values from vector as values
    let fields: Vec<Value<'gc>> = if args[1].is_vector() {
        cx.vector_slice(args[1]).to_vec()
    } else {
        return Err(EvalError::TypeError(format!(
            "%make-record: expected vector of field values, got {}",
            cx.type_name(args[1])
        )));
    };

    if fields.len() != rtd.fields.len() {
        return Err(EvalError::InvalidSyntax(format!(
            "%make-record: expected {} fields, got {}",
            rtd.fields.len(),
            fields.len()
        )));
    }

    Ok(cx.alloc_record(rtd, fields))
}

/// %record-ref: Read a field from a record by index
///
/// (%record-ref record index) => value
fn record_ref<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "%record-ref expects 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    // Extract index from second arg
    let index = if args[1].is_fixnum() {
        args[1].as_fixnum_unchecked() as usize
    } else {
        return Err(EvalError::TypeError(format!(
            "%record-ref: expected integer index, got {}",
            cx.type_name(args[1])
        )));
    };

    if let Some(value) = cx.record_ref(args[0], index) {
        return Ok(value);
    }
    // No such field, or no record: say which.
    if let Some(record_type) = cx.record_type_of(args[0]) {
        Err(EvalError::InvalidSyntax(format!(
            "%record-ref: index {} out of bounds for record type {} with {} fields",
            index,
            record_type.name,
            cx.record_len(args[0]).unwrap_or_default()
        )))
    } else {
        Err(EvalError::TypeError(format!(
            "%record-ref: expected record, got {}",
            cx.type_name(args[0])
        )))
    }
}

/// %record-set!: Write a field in a record by index
///
/// (%record-set! record index value) => unspecified
fn record_set<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 3 {
        return Err(EvalError::WrongArity {
            expected: "%record-set! expects 3 arguments".to_string(),
            actual: args.len(),
        });
    }

    // Extract index from second arg
    let index = if args[1].is_fixnum() {
        args[1].as_fixnum_unchecked() as usize
    } else {
        return Err(EvalError::TypeError(format!(
            "%record-set!: expected integer index, got {}",
            cx.type_name(args[1])
        )));
    };

    let Some(len) = cx.record_len(args[0]) else {
        return Err(EvalError::TypeError(format!(
            "%record-set!: expected record, got {}",
            cx.type_name(args[0])
        )));
    };
    if index >= len {
        let record_type = cx.record_type_of(args[0]);
        return Err(EvalError::InvalidSyntax(format!(
            "%record-set!: index {} out of bounds for record type {} with {} fields",
            index,
            record_type.as_ref().map_or("", |rtd| &*rtd.name),
            len
        )));
    }
    // The funnel's record field: it counts the store in the census
    // (`Site::RecordSet`) and checks what it stores (#708).
    cx.store(RecordField(args[0], index), args[2]);
    Ok(Value::UNSPECIFIED)
}

/// %record-type-name: Get the name of a record type as a symbol
///
/// (%record-type-name rtd) => symbol
fn record_type_name<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "%record-type-name expects 1 argument".to_string(),
            actual: args.len(),
        });
    }

    if let Some(rtd) = cx.get_record_type(args[0]) {
        Ok(cx.intern_symbol(&rtd.name))
    } else {
        Err(EvalError::TypeError(format!(
            "%record-type-name: expected record-type, got {}",
            cx.type_name(args[0])
        )))
    }
}

/// %record-type-fields: Get the field names of a record type as a list
///
/// (%record-type-fields rtd) => (field1 field2 ...)
fn record_type_fields<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "%record-type-fields expects 1 argument".to_string(),
            actual: args.len(),
        });
    }

    let fields: Vec<Rc<str>> = if let Some(rtd) = cx.get_record_type(args[0]) {
        rtd.fields.clone()
    } else {
        return Err(EvalError::TypeError(format!(
            "%record-type-fields: expected record-type, got {}",
            cx.type_name(args[0])
        )));
    };

    // Build a proper list of symbols from the field names
    let mut result = Value::NULL;
    for field in fields.iter().rev() {
        let sym = cx.intern_symbol(field);
        result = cx.alloc_pair(sym, result);
    }

    Ok(result)
}

/// %record-type-field-index: Get the index of a field by name
///
/// (%record-type-field-index rtd 'field-name) => integer or #f
fn record_type_field_index<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "%record-type-field-index expects 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let field_name = extract_symbol_name(cx, args[1])?;

    let rtd = if let Some(rtd) = cx.get_record_type(args[0]) {
        rtd
    } else {
        return Err(EvalError::TypeError(format!(
            "%record-type-field-index: expected record-type, got {}",
            cx.type_name(args[0])
        )));
    };

    match rtd.field_index(&field_name) {
        Some(index) => Ok(Value::fixnum(index as i64)),
        None => Ok(Value::boolean(false)),
    }
}

/// Register all record primitives
pub fn register(registry: &mut PrimitiveRegistry) {
    // Record type creation
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "%make-record-type",
        Arity::Exact(2),
        "Create a new record type descriptor with the given name and field names.",
        make_record_type,
    ));

    // Type predicates
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "%record-type?",
        Arity::Exact(1),
        "Returns #t if obj is a record type descriptor.",
        record_type_p,
    ));

    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "%record?",
        Arity::Exact(1),
        "Returns #t if obj is a record instance.",
        record_p,
    ));

    // Record type introspection
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "%record-type-of",
        Arity::Exact(1),
        "Returns the record type descriptor of a record instance.",
        record_type_of,
    ));

    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "%record-type-name",
        Arity::Exact(1),
        "Returns the name of a record type as a symbol.",
        record_type_name,
    ));

    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "%record-type-fields",
        Arity::Exact(1),
        "Returns the field names of a record type as a list of symbols.",
        record_type_fields,
    ));

    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "%record-type-field-index",
        Arity::Exact(2),
        "Returns the index of a field in a record type, or #f if not found.",
        record_type_field_index,
    ));

    // Record instance creation and access
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "%make-record",
        Arity::Exact(2),
        "Create a new record instance with the given type and field values vector.",
        make_record,
    ));

    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "%record-ref",
        Arity::Exact(2),
        "Read a field from a record by index.",
        record_ref,
    ));

    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "%record-set!",
        Arity::Exact(3),
        "Write a field in a record by index.",
        record_set,
    ));
}
