//! File I/O operations
//!
//! This module implements R7RS file I/O procedures:
//! - open-input-file, open-output-file
//! - open-binary-input-file, open-binary-output-file
//! - file-exists?, delete-file
//! - `(patina internal io)`'s call-with-input-file and call-with-output-file
//!   (`(scheme file)`'s are Scheme, over `open-input-file` and
//!   `open-output-file`)
//!
//! # Running out of descriptors (#607)
//!
//! Every procedure here that opens a file is a resumable primitive. An open
//! that fails because descriptors ran out (`EMFILE` or `ENFILE`,
//! [`out_of_descriptors`]) asks the machine for a full collection at its
//! call ([`Step::Collect`]), which closes every file port it finds dead, and
//! opens once more when it is resumed; if that fails too, it raises the
//! file error. The file name is the state the retry keeps, so asking
//! allocates nothing; `call-with-*-file` keeps its procedure too, in a pair.
//! As chibi's `open-input-file` and `open-output-file` do (`eval.c`).
//!
//! The collection runs in every GC mode, `PATINA_GC=0` included, and with
//! the call's arguments in the suspended frame. Where collection is
//! deferred — a nested loop, a library body being loaded — it is posted for
//! the next safe point that may collect instead, and the open is resumed
//! having collected nothing: the second attempt fails as the first did, and
//! raises the file error the first would have. It is made anyway, whatever
//! the machine answers, so that the error raised is the one the system
//! gives, `EMFILE` or `ENFILE`, with no allocation to carry the first one's.
//!
//! Most programs never get there: a file port is charged against descriptor
//! pressure when it gets its heap object (`Heap::alloc_port`), which posts
//! a collection once `min(128, RLIMIT_NOFILE / 4)` of the ports opened since
//! the last one are still open.

use crate::apply_context::ApplyContext;
use crate::registry::Step;
use patina_core::port::out_of_descriptors;
use patina_core::{CollectKind, Heap, TaggedValue};
use patina_runtime::EvalError;
use patina_runtime::Port;
use smallvec::smallvec;
use std::rc::Rc;

// =============================================================================
// TaggedValue extraction helpers
// =============================================================================

/// Extract a String content from TaggedValue
fn get_string_tv(tv: TaggedValue, heap: &std::cell::Ref<'_, Heap>) -> Option<String> {
    heap.get_string_contents(tv)
}

// =============================================================================
// Opening
// =============================================================================

/// How a primitive here opens its file.
#[derive(Clone, Copy)]
enum OpenMode {
    Input,
    Output,
    BinaryInput,
    BinaryOutput,
}

impl OpenMode {
    /// What the file is opened for, as the error says it.
    fn purpose(self) -> &'static str {
        match self {
            OpenMode::Input => "reading",
            OpenMode::Output => "writing",
            OpenMode::BinaryInput => "binary reading",
            OpenMode::BinaryOutput => "binary writing",
        }
    }

    fn open(self, path: &str, ctx: &dyn ApplyContext) -> std::io::Result<Rc<Port>> {
        let fs = ctx.fs().as_ref();
        match self {
            OpenMode::Input => Port::open_input_file(path, fs),
            OpenMode::Output => Port::open_output_file(path, fs),
            OpenMode::BinaryInput => Port::open_binary_input_file(path, fs),
            OpenMode::BinaryOutput => Port::open_binary_output_file(path, fs),
        }
    }
}

/// The file name `name` holds, or the type error `primitive` raises for one
/// that is not a string.
fn file_name(
    ctx: &dyn ApplyContext,
    primitive: &str,
    name: TaggedValue,
) -> Result<String, EvalError> {
    get_string_tv(name, &ctx.heap().borrow())
        .ok_or_else(|| EvalError::TypeError(format!("{primitive} expects a string filename")))
}

/// Open the file `name` names, for `mode`, as a port on the heap; `None`
/// when descriptors ran out, which the caller collects and tries again
/// after. Any other failure is the file error.
fn try_open(
    ctx: &dyn ApplyContext,
    primitive: &str,
    mode: OpenMode,
    name: TaggedValue,
) -> Result<Option<TaggedValue>, EvalError> {
    let path = file_name(ctx, primitive, name)?;
    match mode.open(&path, ctx) {
        Ok(port) => Ok(Some(ctx.heap().borrow_mut().alloc_port(port))),
        Err(e) if out_of_descriptors(&e) => Ok(None),
        Err(e) => Err(open_error(mode, &path, e)),
    }
}

/// Open the file `name` names, for `mode`, after a collection asked for
/// because descriptors ran out: the last attempt, whose failure is the file
/// error.
fn open_again(
    ctx: &dyn ApplyContext,
    primitive: &str,
    mode: OpenMode,
    name: TaggedValue,
) -> Result<TaggedValue, EvalError> {
    let path = file_name(ctx, primitive, name)?;
    let port = mode
        .open(&path, ctx)
        .map_err(|e| open_error(mode, &path, e))?;
    Ok(ctx.heap().borrow_mut().alloc_port(port))
}

/// The file error for an open that failed. `file-error?` recognises it by
/// its "Cannot open".
fn open_error(mode: OpenMode, path: &str, e: std::io::Error) -> EvalError {
    EvalError::IOError(format!(
        "Cannot open '{}' for {}: {}",
        path,
        mode.purpose(),
        e
    ))
}

/// A full collection at the call, after which the open is tried again with
/// `state` (#607).
fn collect_then_retry(state: TaggedValue) -> Step {
    Step::Collect {
        kind: CollectKind::Major,
        state,
    }
}

/// An `open-*-file` primitive's first half: the port, or a collection to
/// retry after, keeping the file name.
fn open_start(
    ctx: &dyn ApplyContext,
    primitive: &str,
    mode: OpenMode,
    args: &[TaggedValue],
) -> Result<Step, EvalError> {
    Ok(match try_open(ctx, primitive, mode, args[0])? {
        Some(port) => Step::Done(port),
        None => collect_then_retry(args[0]),
    })
}

// Arity is checked by the registry before dispatch, so the halves do not
// check it again.

/// (open-input-file filename) - Opens a file for reading and returns an input port
pub(super) fn open_input_file(
    ctx: &dyn ApplyContext,
    args: &[TaggedValue],
) -> Result<Step, EvalError> {
    open_start(ctx, "open-input-file", OpenMode::Input, args)
}

/// `open-input-file` resumed after the collection it asked for: open again.
pub(super) fn open_input_file_again(
    ctx: &dyn ApplyContext,
    name: TaggedValue,
    _collected: TaggedValue,
) -> Result<Step, EvalError> {
    open_again(ctx, "open-input-file", OpenMode::Input, name).map(Step::Done)
}

/// (open-output-file filename) - Opens a file for writing and returns an output port
pub(super) fn open_output_file(
    ctx: &dyn ApplyContext,
    args: &[TaggedValue],
) -> Result<Step, EvalError> {
    open_start(ctx, "open-output-file", OpenMode::Output, args)
}

/// `open-output-file` resumed after the collection it asked for.
pub(super) fn open_output_file_again(
    ctx: &dyn ApplyContext,
    name: TaggedValue,
    _collected: TaggedValue,
) -> Result<Step, EvalError> {
    open_again(ctx, "open-output-file", OpenMode::Output, name).map(Step::Done)
}

/// (open-binary-input-file filename) - Opens a binary file for reading
pub(super) fn open_binary_input_file(
    ctx: &dyn ApplyContext,
    args: &[TaggedValue],
) -> Result<Step, EvalError> {
    open_start(ctx, "open-binary-input-file", OpenMode::BinaryInput, args)
}

/// `open-binary-input-file` resumed after the collection it asked for.
pub(super) fn open_binary_input_file_again(
    ctx: &dyn ApplyContext,
    name: TaggedValue,
    _collected: TaggedValue,
) -> Result<Step, EvalError> {
    open_again(ctx, "open-binary-input-file", OpenMode::BinaryInput, name).map(Step::Done)
}

/// (open-binary-output-file filename) - Opens a binary file for writing
pub(super) fn open_binary_output_file(
    ctx: &dyn ApplyContext,
    args: &[TaggedValue],
) -> Result<Step, EvalError> {
    open_start(ctx, "open-binary-output-file", OpenMode::BinaryOutput, args)
}

/// `open-binary-output-file` resumed after the collection it asked for.
pub(super) fn open_binary_output_file_again(
    ctx: &dyn ApplyContext,
    name: TaggedValue,
    _collected: TaggedValue,
) -> Result<Step, EvalError> {
    open_again(ctx, "open-binary-output-file", OpenMode::BinaryOutput, name).map(Step::Done)
}

/// (file-exists? filename) - Returns #t if file exists
pub(super) fn file_exists_p(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "file-exists? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    let heap = ctx.heap();
    let heap_ref = heap.borrow();
    match get_string_tv(args[0], &heap_ref) {
        Some(path) => Ok(TaggedValue::boolean(
            ctx.fs().file_exists(std::path::Path::new(&path)),
        )),
        None => Err(EvalError::TypeError(
            "file-exists? expects a string filename".to_string(),
        )),
    }
}

/// (delete-file filename) - Deletes the file
pub(super) fn delete_file(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "delete-file expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    let heap = ctx.heap();
    let heap_ref = heap.borrow();
    match get_string_tv(args[0], &heap_ref) {
        Some(path) => {
            ctx.fs()
                .remove_file(std::path::Path::new(&path))
                .map_err(|e| EvalError::IOError(format!("Cannot delete '{}': {}", path, e)))?;
            Ok(TaggedValue::UNSPECIFIED)
        }
        None => Err(EvalError::TypeError(
            "delete-file expects a string filename".to_string(),
        )),
    }
}

// =============================================================================
// (patina internal io)'s call-with-input-file and call-with-output-file
// =============================================================================

/// Call `proc` with `port` as a call the machine makes, keeping the port to
/// close when it returns.
fn call_with(port: TaggedValue, proc: TaggedValue) -> Step {
    Step::Call {
        callee: proc,
        args: smallvec![port],
        state: port,
    }
}

/// A `call-with-*-file` primitive's first half: open, and call the procedure
/// with the port, or collect and retry, keeping the file name and the
/// procedure in a pair — the one allocation, on the way to a collection.
fn call_with_file_start(
    ctx: &dyn ApplyContext,
    primitive: &str,
    mode: OpenMode,
    args: &[TaggedValue],
) -> Result<Step, EvalError> {
    let (name, proc) = (args[0], args[1]);
    Ok(match try_open(ctx, primitive, mode, name)? {
        Some(port) => call_with(port, proc),
        None => collect_then_retry(ctx.heap().borrow_mut().alloc_pair(name, proc)),
    })
}

/// A `call-with-*-file` primitive resumed. After its procedure returned,
/// `state` is the port: closed now, and only now, as `call-with-port`
/// closes it — only if the procedure returns (R7RS 6.13.1) — and the value
/// is the procedure's. A continuation captured in the procedure and
/// re-entered after this returned comes back here, and closes the closed
/// port again. After the collection asked for, `state` is the file name and
/// the procedure: open again, and call it.
fn call_with_file_resume(
    ctx: &dyn ApplyContext,
    primitive: &str,
    mode: OpenMode,
    state: TaggedValue,
    value: TaggedValue,
) -> Result<Step, EvalError> {
    let heap = ctx.heap();
    let port = heap.borrow().get_port(state).cloned();
    if let Some(port) = port {
        port.close();
        return Ok(Step::Done(value));
    }
    let (name, proc) = heap.borrow().get_pair(state);
    let port = open_again(ctx, primitive, mode, name)?;
    Ok(call_with(port, proc))
}

/// (call-with-input-file filename proc) - Opens file for reading, calls proc
/// with port, closes it if proc returns. Reached only by importing
/// `(patina internal io)`: `(scheme file)`'s is Scheme (#471).
pub(super) fn call_with_input_file(
    ctx: &dyn ApplyContext,
    args: &[TaggedValue],
) -> Result<Step, EvalError> {
    call_with_file_start(ctx, "call-with-input-file", OpenMode::Input, args)
}

/// `call-with-input-file` resumed ([`call_with_file_resume`]).
pub(super) fn call_with_input_file_resume(
    ctx: &dyn ApplyContext,
    state: TaggedValue,
    value: TaggedValue,
) -> Result<Step, EvalError> {
    call_with_file_resume(ctx, "call-with-input-file", OpenMode::Input, state, value)
}

/// (call-with-output-file filename proc) - Opens file for writing, calls
/// proc with port, closes (and so flushes) it if proc returns. Reached only
/// by importing `(patina internal io)`: `(scheme file)`'s is Scheme (#471).
pub(super) fn call_with_output_file(
    ctx: &dyn ApplyContext,
    args: &[TaggedValue],
) -> Result<Step, EvalError> {
    call_with_file_start(ctx, "call-with-output-file", OpenMode::Output, args)
}

/// `call-with-output-file` resumed ([`call_with_file_resume`]).
pub(super) fn call_with_output_file_resume(
    ctx: &dyn ApplyContext,
    state: TaggedValue,
    value: TaggedValue,
) -> Result<Step, EvalError> {
    call_with_file_resume(ctx, "call-with-output-file", OpenMode::Output, state, value)
}
