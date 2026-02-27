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
use patina_core::TaggedValue;
use patina_core::heap::SharedHeap;
use patina_core::{Arity, RecordTypeDescriptor};
use std::cell::RefCell;
use std::rc::Rc;

/// Extract a symbol name from a TaggedValue using heap methods
fn extract_symbol_name(tv: TaggedValue, heap: &SharedHeap) -> Result<String, EvalError> {
    let heap_ref = heap.borrow();
    match heap_ref.get_symbol_or_identifier_name(tv) {
        Some(name) => Ok(name.to_string()),
        None => Err(EvalError::TypeError(format!(
            "Expected symbol, got {}",
            heap_ref.type_name(tv)
        ))),
    }
}

/// Extract a list of symbol names from a TaggedValue pair chain
fn extract_symbol_list_tagged(
    tv: TaggedValue,
    heap: &SharedHeap,
) -> Result<Vec<String>, EvalError> {
    let mut result = Vec::new();
    let mut current = tv;

    loop {
        if current == TaggedValue::NULL {
            break;
        }

        // try_pair extracts car and cdr from pairs
        let pair = heap.borrow().try_pair(current);
        if let Some((car, cdr)) = pair {
            let name = extract_symbol_name(car, heap)?;
            result.push(name);
            current = cdr;
        } else {
            return Err(EvalError::TypeError(format!(
                "Expected proper list of symbols, got {}",
                heap.borrow().type_name(current)
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
fn make_record_type(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "%make-record-type expects 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let name = extract_symbol_name(args[0], heap)?;
    let fields = extract_symbol_list_tagged(args[1], heap)?;

    let rtd = RecordTypeDescriptor::new(&name, fields);
    Ok(heap.borrow_mut().alloc_record_type(Rc::new(rtd)))
}

/// %record-type?: Check if value is a record type descriptor
fn record_type_p(evaluator: &Evaluator, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    Ok(TaggedValue::boolean(heap.borrow().is_record_type(args[0])))
}

/// %record?: Check if value is a record instance
fn record_p(evaluator: &Evaluator, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    Ok(TaggedValue::boolean(heap.borrow().is_record(args[0])))
}

/// %record-type-of: Get the record type descriptor from a record instance
///
/// (%record-type-of record) => rtd
fn record_type_of(evaluator: &Evaluator, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "%record-type-of expects 1 argument".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();

    if let Some((rtd, _fields)) = heap_ref.get_record(args[0]) {
        let rtd_clone = rtd.clone();
        drop(heap_ref);
        Ok(heap.borrow_mut().alloc_record_type(rtd_clone))
    } else {
        Err(EvalError::TypeError(format!(
            "%record-type-of: expected record, got {}",
            heap_ref.type_name(args[0])
        )))
    }
}

/// %make-record: Create a new record instance
///
/// (%make-record rtd field-values-vector) => record
///
/// field-values-vector must be a vector with exactly as many elements
/// as the record type has fields.
fn make_record(evaluator: &Evaluator, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "%make-record expects 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();

    // Get the record type descriptor
    let rtd = {
        let heap_ref = heap.borrow();
        if let Some(rtd) = heap_ref.get_record_type(args[0]) {
            rtd.clone()
        } else {
            return Err(EvalError::TypeError(format!(
                "%make-record: expected record-type, got {}",
                heap_ref.type_name(args[0])
            )));
        }
    };

    // Get field values from vector as TaggedValues
    let fields: Vec<TaggedValue> = {
        let heap_ref = heap.borrow();
        if args[1].is_vector() {
            heap_ref.vector_slice(args[1]).to_vec()
        } else {
            return Err(EvalError::TypeError(format!(
                "%make-record: expected vector of field values, got {}",
                heap_ref.type_name(args[1])
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

    Ok(heap
        .borrow_mut()
        .alloc_record(rtd, Rc::new(RefCell::new(fields))))
}

/// %record-ref: Read a field from a record by index
///
/// (%record-ref record index) => value
fn record_ref(evaluator: &Evaluator, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
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
        let heap = evaluator.global_env.heap();
        let heap_ref = heap.borrow();
        return Err(EvalError::TypeError(format!(
            "%record-ref: expected integer index, got {}",
            heap_ref.type_name(args[1])
        )));
    };

    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();

    if let Some((record_type, fields)) = heap_ref.get_record(args[0]) {
        let fields_ref = fields.borrow();
        if index >= fields_ref.len() {
            return Err(EvalError::InvalidSyntax(format!(
                "%record-ref: index {} out of bounds for record type {} with {} fields",
                index,
                record_type.name,
                fields_ref.len()
            )));
        }
        Ok(fields_ref[index])
    } else {
        Err(EvalError::TypeError(format!(
            "%record-ref: expected record, got {}",
            heap_ref.type_name(args[0])
        )))
    }
}

/// %record-set!: Write a field in a record by index
///
/// (%record-set! record index value) => unspecified
fn record_set(evaluator: &Evaluator, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
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
        let heap = evaluator.global_env.heap();
        let heap_ref = heap.borrow();
        return Err(EvalError::TypeError(format!(
            "%record-set!: expected integer index, got {}",
            heap_ref.type_name(args[1])
        )));
    };

    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();

    if let Some((record_type, fields)) = heap_ref.get_record(args[0]) {
        let mut fields_ref = fields.borrow_mut();
        if index >= fields_ref.len() {
            return Err(EvalError::InvalidSyntax(format!(
                "%record-set!: index {} out of bounds for record type {} with {} fields",
                index,
                record_type.name,
                fields_ref.len()
            )));
        }
        fields_ref[index] = args[2];
        Ok(TaggedValue::UNSPECIFIED)
    } else {
        Err(EvalError::TypeError(format!(
            "%record-set!: expected record, got {}",
            heap_ref.type_name(args[0])
        )))
    }
}

/// %record-type-name: Get the name of a record type as a symbol
///
/// (%record-type-name rtd) => symbol
fn record_type_name(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "%record-type-name expects 1 argument".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();

    if let Some(rtd) = heap_ref.get_record_type(args[0]) {
        let name = rtd.name.clone();
        drop(heap_ref);
        Ok(heap.borrow_mut().intern_symbol(&name))
    } else {
        Err(EvalError::TypeError(format!(
            "%record-type-name: expected record-type, got {}",
            heap_ref.type_name(args[0])
        )))
    }
}

/// %record-type-fields: Get the field names of a record type as a list
///
/// (%record-type-fields rtd) => (field1 field2 ...)
fn record_type_fields(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "%record-type-fields expects 1 argument".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();

    let fields: Vec<Rc<str>> = if let Some(rtd) = heap_ref.get_record_type(args[0]) {
        rtd.fields.clone()
    } else {
        return Err(EvalError::TypeError(format!(
            "%record-type-fields: expected record-type, got {}",
            heap_ref.type_name(args[0])
        )));
    };
    drop(heap_ref);

    // Build a proper list of symbols from the field names using heap operations
    let mut result = TaggedValue::NULL;
    let mut heap_mut = heap.borrow_mut();
    for field in fields.iter().rev() {
        let sym = heap_mut.intern_symbol(field);
        result = heap_mut.alloc_pair(sym, result);
    }

    Ok(result)
}

/// %record-type-field-index: Get the index of a field by name
///
/// (%record-type-field-index rtd 'field-name) => integer or #f
fn record_type_field_index(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 2 {
        return Err(EvalError::WrongArity {
            expected: "%record-type-field-index expects 2 arguments".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let field_name = extract_symbol_name(args[1], heap)?;

    let heap_ref = heap.borrow();

    let rtd = if let Some(rtd) = heap_ref.get_record_type(args[0]) {
        rtd
    } else {
        return Err(EvalError::TypeError(format!(
            "%record-type-field-index: expected record-type, got {}",
            heap_ref.type_name(args[0])
        )));
    };

    match rtd.field_index(&field_name) {
        Some(index) => Ok(TaggedValue::fixnum(index as i64)),
        None => Ok(TaggedValue::boolean(false)),
    }
}

/// Register all record primitives
pub fn register(registry: &mut PrimitiveRegistry) {
    // Record type creation
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "%make-record-type",
        Arity::Exact(2),
        "Create a new record type descriptor with the given name and field names.",
        |eval, args, _tail| make_record_type(eval, args).map(EvalResult::Tagged),
    ));

    // Type predicates
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "%record-type?",
        Arity::Exact(1),
        "Returns #t if obj is a record type descriptor.",
        |eval, args, _tail| record_type_p(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "%record?",
        Arity::Exact(1),
        "Returns #t if obj is a record instance.",
        |eval, args, _tail| record_p(eval, args).map(EvalResult::Tagged),
    ));

    // Record type introspection
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "%record-type-of",
        Arity::Exact(1),
        "Returns the record type descriptor of a record instance.",
        |eval, args, _tail| record_type_of(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "%record-type-name",
        Arity::Exact(1),
        "Returns the name of a record type as a symbol.",
        |eval, args, _tail| record_type_name(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "%record-type-fields",
        Arity::Exact(1),
        "Returns the field names of a record type as a list of symbols.",
        |eval, args, _tail| record_type_fields(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "%record-type-field-index",
        Arity::Exact(2),
        "Returns the index of a field in a record type, or #f if not found.",
        |eval, args, _tail| record_type_field_index(eval, args).map(EvalResult::Tagged),
    ));

    // Record instance creation and access
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "%make-record",
        Arity::Exact(2),
        "Create a new record instance with the given type and field values vector.",
        |eval, args, _tail| make_record(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "%record-ref",
        Arity::Exact(2),
        "Read a field from a record by index.",
        |eval, args, _tail| record_ref(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "%record-set!",
        Arity::Exact(3),
        "Write a field in a record by index.",
        |eval, args, _tail| record_set(eval, args).map(EvalResult::Tagged),
    ));
}
