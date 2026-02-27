//! Debug formatting utilities for macro hygiene debugging
//!
//! This module provides scope-aware formatting of TaggedValues to help debug macro
//! expansion and hygiene issues. When enabled, identifiers are displayed with
//! their scope sets, making it possible to trace how hygiene is being applied.
//!
//! ## Example Output
//!
//! Normal display: `(let ((temp x)) (if temp temp (my-or y)))`
//!
//! Debug display:  `(let{S3} ((temp{S3} x{S1,S2})) (if{S3} temp{S3} temp{S3} (my-or y{S1,S2})))`
//!
//! This makes it clear which identifiers came from the macro template (have S3)
//! versus which came from the use site (have S1,S2 but not S3).

use crate::heap::{Heap, HeapObjectData};
use crate::tagged_value::TaggedValue;
use std::fmt::Write;

/// Format a TaggedValue for display (without scope annotations)
///
/// Identifiers are shown as plain names without scope sets.
pub fn format_tagged(tv: TaggedValue, heap: &Heap) -> String {
    let mut buf = String::new();
    format_tagged_impl(tv, heap, &mut buf, false);
    buf
}

/// Format a TaggedValue with full scope information for debugging
///
/// Identifiers are annotated with their scope sets for hygiene debugging.
pub fn format_tagged_with_scopes(tv: TaggedValue, heap: &Heap) -> String {
    let mut buf = String::new();
    format_tagged_impl(tv, heap, &mut buf, true);
    buf
}

/// Format a real (f64) value in Scheme display style
///
/// Handles +inf.0, -inf.0, +nan.0, and ensures all inexact numbers
/// have a decimal point (e.g., "1.0" not "1").
///
/// Note: This is for display/write output. `number->string` uses a separate
/// implementation in conversion.rs that also handles -0.0 and scientific notation.
pub fn format_real(r: f64, buf: &mut String) {
    if r.is_infinite() {
        if r.is_sign_positive() {
            buf.push_str("+inf.0");
        } else {
            buf.push_str("-inf.0");
        }
    } else if r.is_nan() {
        buf.push_str("+nan.0");
    } else if r.fract() == 0.0 {
        write!(buf, "{:.1}", r).unwrap();
    } else {
        write!(buf, "{}", r).unwrap();
    }
}

/// Format a complex number from its real and imaginary TaggedValue parts
fn format_complex(real: TaggedValue, imag: TaggedValue, heap: &Heap, buf: &mut String) {
    // Format parts into temporary strings so we can inspect them
    let real_str = format_tagged(real, heap);
    let imag_str = format_tagged(imag, heap);

    let real_is_zero = real_str == "0" || real_str == "0.0";
    let imag_is_zero = imag_str == "0" || imag_str == "0.0";

    if real_is_zero && imag_is_zero {
        buf.push('0');
    } else if real_is_zero {
        // Pure imaginary
        if imag_str == "1" {
            buf.push_str("+i");
        } else if imag_str == "-1" {
            buf.push_str("-i");
        } else if imag_str.starts_with('-') || imag_str.starts_with('+') {
            write!(buf, "{}i", imag_str).unwrap();
        } else {
            write!(buf, "+{}i", imag_str).unwrap();
        }
    } else if imag_is_zero {
        buf.push_str(&real_str);
    } else if imag_str == "1" {
        write!(buf, "{}+i", real_str).unwrap();
    } else if imag_str == "-1" {
        write!(buf, "{}-i", real_str).unwrap();
    } else if imag_str.starts_with('-') || imag_str.starts_with('+') {
        write!(buf, "{}{}i", real_str, imag_str).unwrap();
    } else {
        write!(buf, "{}+{}i", real_str, imag_str).unwrap();
    }
}

/// Format a HeapObjectData value
fn format_object(obj: &HeapObjectData, heap: &Heap, buf: &mut String, with_scopes: bool) {
    match obj {
        HeapObjectData::BigInt(n) => write!(buf, "{}", n).unwrap(),
        HeapObjectData::Rational(r) => write!(buf, "{}", r).unwrap(),
        HeapObjectData::Real(r) => format_real(*r, buf),
        HeapObjectData::Complex { real, imag } => format_complex(*real, *imag, heap, buf),
        HeapObjectData::Symbol(s) => buf.push_str(s),
        HeapObjectData::Identifier { name, scopes } => {
            buf.push_str(name);
            if with_scopes && !scopes.is_empty() {
                write!(buf, "{}", scopes).unwrap();
            }
        }
        HeapObjectData::Bytevector(bv) => write!(buf, "#u8({:?})", bv).unwrap(),
        HeapObjectData::Exception { message, .. } => {
            write!(buf, "#<error-object: {}>", message).unwrap()
        }
        HeapObjectData::Procedure(p) => {
            use crate::procedure::Procedure;
            match p.as_ref() {
                Procedure::Primitive { name, library, .. } => {
                    write!(buf, "#<procedure:{}:{}>", library.join("."), name).unwrap()
                }
                Procedure::CpsLambda { .. } => buf.push_str("#<procedure>"),
            }
        }
        HeapObjectData::Port(p) => write!(buf, "{}", p).unwrap(),
        HeapObjectData::Macro(m) => write!(buf, "#<macro:{}>", m.name).unwrap(),
        HeapObjectData::RecordType(rtd) => write!(buf, "#<record-type {}>", rtd.name).unwrap(),
        HeapObjectData::Record { record_type, .. } => {
            write!(buf, "#<record {}>", record_type.name).unwrap()
        }
        HeapObjectData::Continuation(_) => buf.push_str("#<continuation>"),
        HeapObjectData::Parameter { .. } => buf.push_str("#<parameter>"),
        HeapObjectData::Promise(_) => buf.push_str("#<promise>"),
        HeapObjectData::Library(lib) => write!(buf, "{}", lib).unwrap(),
        HeapObjectData::Values(vals) => {
            for (i, val) in vals.iter().enumerate() {
                if i > 0 {
                    buf.push('\n');
                }
                format_tagged_impl(*val, heap, buf, with_scopes);
            }
        }
        HeapObjectData::EnvironmentSpecifier { .. } => buf.push_str("#<environment>"),
        HeapObjectData::PromptTag(tag) => write!(buf, "{}", tag).unwrap(),
        HeapObjectData::LabelPlaceholder(n) => write!(buf, "#<label-placeholder:{}>", n).unwrap(),
    }
}

/// Unified recursive formatter for TaggedValue
fn format_tagged_impl(tv: TaggedValue, heap: &Heap, buf: &mut String, with_scopes: bool) {
    // Immediate values
    if tv.is_fixnum() {
        write!(buf, "{}", tv.as_fixnum_unchecked()).unwrap();
        return;
    }
    if tv == TaggedValue::TRUE {
        buf.push_str("#t");
        return;
    }
    if tv == TaggedValue::FALSE {
        buf.push_str("#f");
        return;
    }
    if tv == TaggedValue::NULL {
        buf.push_str("()");
        return;
    }
    if tv.is_char() {
        let c = tv.as_char_unchecked();
        write!(buf, "#\\{}", c).unwrap();
        return;
    }
    if tv == TaggedValue::EOF {
        buf.push_str("#<eof>");
        return;
    }
    if tv == TaggedValue::UNSPECIFIED {
        buf.push_str("#<unspecified>");
        return;
    }

    // Native pairs
    if tv.is_pair() {
        buf.push('(');
        format_tagged_list(tv, heap, buf, with_scopes);
        buf.push(')');
        return;
    }

    // Native strings
    if tv.is_string() {
        let s = heap.get_string_as_utf8(tv);
        write!(buf, "\"{}\"", s).unwrap();
        return;
    }

    // Native vectors
    if tv.is_vector() {
        buf.push_str("#(");
        let len = heap.vector_len(tv);
        for i in 0..len {
            if i > 0 {
                buf.push(' ');
            }
            format_tagged_impl(heap.vector_ref(tv, i), heap, buf, with_scopes);
        }
        buf.push(')');
        return;
    }

    // Object types
    if tv.is_object() {
        let obj = heap.get_object(tv);
        format_object(obj, heap, buf, with_scopes);
        return;
    }

    // Closures or other unknown tags
    buf.push_str("#<object>");
}

/// Format tagged list contents, handling dotted lists
fn format_tagged_list(tv: TaggedValue, heap: &Heap, buf: &mut String, with_scopes: bool) {
    let mut current = tv;
    let mut first = true;

    loop {
        if current == TaggedValue::NULL {
            break;
        }
        if current.is_pair() {
            let car = heap.car(current);
            let cdr = heap.cdr(current);
            if !first {
                buf.push(' ');
            }
            first = false;
            format_tagged_impl(car, heap, buf, with_scopes);
            current = cdr;
        } else {
            // Dotted list tail
            buf.push_str(" . ");
            format_tagged_impl(current, heap, buf, with_scopes);
            break;
        }
    }
}
