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

/// The error for `v`, given where a record of the type `rtd` was asked for:
/// an accessor's, a modifier's or a predicate's argument (#736).
fn not_of_type<'gc>(cx: &Cx<'gc>, who: &str, v: Value<'gc>, rtd: Value<'gc>) -> EvalError {
    let Some(expected) = cx.get_record_type(rtd) else {
        return EvalError::TypeError(format!(
            "{who}: expected record-type, got {}",
            cx.type_name(rtd)
        ));
    };
    let got = match cx.record_type_of(v) {
        Some(actual) => format!("a {} record", actual.name),
        None => cx.type_name(v).to_string(),
    };
    EvalError::TypeError(format!(
        "{who}: expected a {} record, got {got}",
        expected.name
    ))
}

/// %record-of?: Whether obj is a record of the type rtd, compared by
/// identity with no allocation: a predicate's whole body (#736)
///
/// (%record-of? obj rtd) => boolean
fn record_of_p<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "%record-of? expects 2 arguments".to_string(),
            actual: args.len(),
        });
    }
    if cx.is_record_of(args[0], args[1]) {
        return Ok(Value::TRUE);
    }
    if !cx.is_record_type(args[1]) {
        return Err(not_of_type(cx, "%record-of?", args[0], args[1]));
    }
    Ok(Value::FALSE)
}

/// %make-record-from: A record of the type rtd whose fields at `indices`, a
/// vector of field indices, hold the values in order, and whose other
/// fields are unspecified: a constructor's whole body, given its arguments
/// (#736)
///
/// (%make-record-from rtd indices value ...) => record
fn make_record_from<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "%make-record-from expects at least 2 arguments".to_string(),
            actual: args.len(),
        });
    }
    let Some(rtd) = cx.get_record_type(args[0]) else {
        return Err(EvalError::TypeError(format!(
            "%make-record-from: expected record-type, got {}",
            cx.type_name(args[0])
        )));
    };
    if !args[1].is_vector() {
        return Err(EvalError::TypeError(format!(
            "%make-record-from: expected vector of field indices, got {}",
            cx.type_name(args[1])
        )));
    }
    let values = &args[2..];
    let mut fields = vec![Value::UNSPECIFIED; rtd.fields.len()];
    {
        let indices = cx.vector_slice(args[1]);
        if indices.len() != values.len() {
            return Err(EvalError::InvalidSyntax(format!(
                "%make-record-from: {} field indices for {} values",
                indices.len(),
                values.len()
            )));
        }
        for (position, (index, value)) in indices.iter().zip(values).enumerate() {
            let slot = index
                .is_fixnum()
                .then(|| index.as_fixnum_unchecked())
                .and_then(|i| usize::try_from(i).ok())
                .and_then(|i| fields.get_mut(i));
            let Some(slot) = slot else {
                return Err(EvalError::InvalidSyntax(format!(
                    "%make-record-from: index {position} names no field of record type {}",
                    rtd.name
                )));
            };
            *slot = *value;
        }
    }
    Ok(cx.alloc_record(rtd, fields))
}

/// The type and the index an accessor's or modifier's key pairs, `(rtd . index)`:
/// the one value it captures, so that its call runs as many of the VM's
/// instructions as an unchecked one did. A second captured variable was a
/// load more, about 180 instructions of the processor's (#736).
fn field_key<'gc>(
    cx: &Cx<'gc>,
    who: &str,
    key: Value<'gc>,
) -> Result<(Value<'gc>, usize), EvalError> {
    if key.is_pair() {
        let index = cx.cdr(key);
        if index.is_fixnum() {
            return Ok((cx.car(key), index.as_fixnum_unchecked() as usize));
        }
    }
    Err(EvalError::TypeError(format!(
        "{who}: expected a field key, got {}",
        cx.type_name(key)
    )))
}

/// %record-field: Read the field `key` names from a record of its type: an
/// accessor's whole body (#736)
///
/// (%record-field record key) => value
fn record_field<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "%record-field expects 2 arguments".to_string(),
            actual: args.len(),
        });
    }
    let (rtd, index) = field_key(cx, "%record-field", args[1])?;
    if !cx.is_record_of(args[0], rtd) {
        return Err(not_of_type(cx, "%record-field", args[0], rtd));
    }
    cx.record_ref(args[0], index).ok_or_else(|| {
        EvalError::InvalidSyntax(format!("%record-field: no field at index {index}"))
    })
}

/// %record-field-set!: Write the field `key` names in a record of its type:
/// a modifier's whole body (#736)
///
/// (%record-field-set! record key value) => unspecified
fn record_field_set<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if args.len() != 3 {
        return Err(EvalError::WrongArity {
            expected: "%record-field-set! expects 3 arguments".to_string(),
            actual: args.len(),
        });
    }
    let (rtd, index) = field_key(cx, "%record-field-set!", args[1])?;
    if !cx.is_record_of(args[0], rtd) {
        return Err(not_of_type(cx, "%record-field-set!", args[0], rtd));
    }
    if index >= cx.record_len(args[0]).unwrap_or_default() {
        return Err(EvalError::InvalidSyntax(format!(
            "%record-field-set!: no field at index {index}"
        )));
    }
    // The funnel's record field: it counts the store in the census
    // (`Site::RecordSet`) and checks what it stores (#708).
    cx.store(RecordField(args[0], index), args[2]);
    Ok(Value::UNSPECIFIED)
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

    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "%record-of?",
        Arity::Exact(2),
        "Returns #t if obj is a record of the given type.",
        record_of_p,
    ));

    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "%make-record-from",
        Arity::Min(2),
        "Create a record of the given type, the values at the given field indices.",
        make_record_from,
    ));

    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "%record-field",
        Arity::Exact(2),
        "Read the field a (type . index) key names, from a record of that type.",
        record_field,
    ));

    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "%record-field-set!",
        Arity::Exact(3),
        "Write the field a (type . index) key names, in a record of that type.",
        record_field_set,
    ));
}
