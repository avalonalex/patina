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
//! Each takes the context (#706), and opens through the file system of the
//! context's instance. The opens and `call-with-*-file` are resumable, and
//! start and resume with one.
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
//! deferred — a nested loop, a library body loaded under a holder's guard —
//! it is posted for the next safe point that may collect instead, and the
//! open is resumed having collected nothing: the second attempt fails as the
//! first did, and raises the file error the first would have. It is made anyway, whatever
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
use patina_core::{CollectKind, Cx, Value};
use patina_runtime::EvalError;
use patina_runtime::Port;
use smallvec::smallvec;
use std::rc::Rc;

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

    fn open(self, path: &str, cx: &Cx<'_>) -> std::io::Result<Rc<Port>> {
        let fs = cx.instance().fs();
        let fs = fs.as_ref();
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
fn file_name<'gc>(cx: &Cx<'gc>, primitive: &str, name: Value<'gc>) -> Result<String, EvalError> {
    cx.get_string_contents(name)
        .ok_or_else(|| EvalError::TypeError(format!("{primitive} expects a string filename")))
}

/// Open the file `name` names, for `mode`, as a port on the heap; `None`
/// when descriptors ran out, which the caller collects and tries again
/// after. Any other failure is the file error.
fn try_open<'gc>(
    cx: &mut Cx<'gc>,
    primitive: &str,
    mode: OpenMode,
    name: Value<'gc>,
) -> Result<Option<Value<'gc>>, EvalError> {
    let path = file_name(cx, primitive, name)?;
    match mode.open(&path, cx) {
        Ok(port) => Ok(Some(cx.alloc_port(port))),
        Err(e) if out_of_descriptors(&e) => Ok(None),
        Err(e) => Err(open_error(mode, &path, e)),
    }
}

/// Open the file `name` names, for `mode`, after a collection asked for
/// because descriptors ran out: the last attempt, whose failure is the file
/// error.
fn open_again<'gc>(
    cx: &mut Cx<'gc>,
    primitive: &str,
    mode: OpenMode,
    name: Value<'gc>,
) -> Result<Value<'gc>, EvalError> {
    let path = file_name(cx, primitive, name)?;
    let port = mode
        .open(&path, cx)
        .map_err(|e| open_error(mode, &path, e))?;
    Ok(cx.alloc_port(port))
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
fn collect_then_retry(state: Value<'_>) -> Step<'_> {
    Step::Collect {
        kind: CollectKind::Major,
        state,
    }
}

/// An `open-*-file` primitive's first half: the port, or a collection to
/// retry after, keeping the file name.
fn open_start<'gc>(
    cx: &mut Cx<'gc>,
    primitive: &str,
    mode: OpenMode,
    args: &[Value<'gc>],
) -> Result<Step<'gc>, EvalError> {
    Ok(match try_open(cx, primitive, mode, args[0])? {
        Some(port) => Step::Done(port),
        None => collect_then_retry(args[0]),
    })
}

// Arity is checked by the registry before dispatch, so the halves do not
// check it again.

/// (open-input-file filename) - Opens a file for reading and returns an input port
pub(super) fn open_input_file<'gc>(
    cx: &mut Cx<'gc>,
    _ctx: &dyn ApplyContext,
    args: &[Value<'gc>],
) -> Result<Step<'gc>, EvalError> {
    open_start(cx, "open-input-file", OpenMode::Input, args)
}

/// `open-input-file` resumed after the collection it asked for: open again.
pub(super) fn open_input_file_again<'gc>(
    cx: &mut Cx<'gc>,
    _ctx: &dyn ApplyContext,
    name: Value<'gc>,
    _collected: Value<'gc>,
) -> Result<Step<'gc>, EvalError> {
    open_again(cx, "open-input-file", OpenMode::Input, name).map(Step::Done)
}

/// (open-output-file filename) - Opens a file for writing and returns an output port
pub(super) fn open_output_file<'gc>(
    cx: &mut Cx<'gc>,
    _ctx: &dyn ApplyContext,
    args: &[Value<'gc>],
) -> Result<Step<'gc>, EvalError> {
    open_start(cx, "open-output-file", OpenMode::Output, args)
}

/// `open-output-file` resumed after the collection it asked for.
pub(super) fn open_output_file_again<'gc>(
    cx: &mut Cx<'gc>,
    _ctx: &dyn ApplyContext,
    name: Value<'gc>,
    _collected: Value<'gc>,
) -> Result<Step<'gc>, EvalError> {
    open_again(cx, "open-output-file", OpenMode::Output, name).map(Step::Done)
}

/// (open-binary-input-file filename) - Opens a binary file for reading
pub(super) fn open_binary_input_file<'gc>(
    cx: &mut Cx<'gc>,
    _ctx: &dyn ApplyContext,
    args: &[Value<'gc>],
) -> Result<Step<'gc>, EvalError> {
    open_start(cx, "open-binary-input-file", OpenMode::BinaryInput, args)
}

/// `open-binary-input-file` resumed after the collection it asked for.
pub(super) fn open_binary_input_file_again<'gc>(
    cx: &mut Cx<'gc>,
    _ctx: &dyn ApplyContext,
    name: Value<'gc>,
    _collected: Value<'gc>,
) -> Result<Step<'gc>, EvalError> {
    open_again(cx, "open-binary-input-file", OpenMode::BinaryInput, name).map(Step::Done)
}

/// (open-binary-output-file filename) - Opens a binary file for writing
pub(super) fn open_binary_output_file<'gc>(
    cx: &mut Cx<'gc>,
    _ctx: &dyn ApplyContext,
    args: &[Value<'gc>],
) -> Result<Step<'gc>, EvalError> {
    open_start(cx, "open-binary-output-file", OpenMode::BinaryOutput, args)
}

/// `open-binary-output-file` resumed after the collection it asked for.
pub(super) fn open_binary_output_file_again<'gc>(
    cx: &mut Cx<'gc>,
    _ctx: &dyn ApplyContext,
    name: Value<'gc>,
    _collected: Value<'gc>,
) -> Result<Step<'gc>, EvalError> {
    open_again(cx, "open-binary-output-file", OpenMode::BinaryOutput, name).map(Step::Done)
}

/// (file-exists? filename) - Returns #t if file exists. Takes the context
/// (#706), and asks the instance's file system.
pub(super) fn file_exists_p<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "file-exists? expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match cx.get_string_contents(args[0]) {
        Some(path) => Ok(Value::boolean(
            cx.instance().fs().file_exists(std::path::Path::new(&path)),
        )),
        None => Err(EvalError::TypeError(
            "file-exists? expects a string filename".to_string(),
        )),
    }
}

/// (delete-file filename) - Deletes the file. Takes the context (#706).
pub(super) fn delete_file<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "delete-file expects 1 argument".to_string(),
            actual: args.len(),
        });
    }
    match cx.get_string_contents(args[0]) {
        Some(path) => {
            cx.instance()
                .fs()
                .remove_file(std::path::Path::new(&path))
                .map_err(|e| EvalError::IOError(format!("Cannot delete '{}': {}", path, e)))?;
            Ok(Value::UNSPECIFIED)
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
fn call_with<'gc>(port: Value<'gc>, proc: Value<'gc>) -> Step<'gc> {
    Step::Call {
        callee: proc,
        args: smallvec![port],
        state: port,
    }
}

/// A `call-with-*-file` primitive's first half: open, and call the procedure
/// with the port, or collect and retry, keeping the file name and the
/// procedure in a pair — the one allocation, on the way to a collection.
fn call_with_file_start<'gc>(
    cx: &mut Cx<'gc>,
    primitive: &str,
    mode: OpenMode,
    args: &[Value<'gc>],
) -> Result<Step<'gc>, EvalError> {
    let (name, proc) = (args[0], args[1]);
    Ok(match try_open(cx, primitive, mode, name)? {
        Some(port) => call_with(port, proc),
        None => collect_then_retry(cx.alloc_pair(name, proc)),
    })
}

/// A `call-with-*-file` primitive resumed. After its procedure returned,
/// `state` is the port: closed now, and only now, as `call-with-port`
/// closes it — only if the procedure returns (R7RS 6.13.1) — and the value
/// is the procedure's. A continuation captured in the procedure and
/// re-entered after this returned comes back here, and closes the closed
/// port again. After the collection asked for, `state` is the file name and
/// the procedure: open again, and call it.
fn call_with_file_resume<'gc>(
    cx: &mut Cx<'gc>,
    primitive: &str,
    mode: OpenMode,
    state: Value<'gc>,
    value: Value<'gc>,
) -> Result<Step<'gc>, EvalError> {
    if let Some(port) = cx.get_port(state) {
        port.close();
        return Ok(Step::Done(value));
    }
    let (name, proc) = cx.get_pair(state);
    let port = open_again(cx, primitive, mode, name)?;
    Ok(call_with(port, proc))
}

/// (call-with-input-file filename proc) - Opens file for reading, calls proc
/// with port, closes it if proc returns. Reached only by importing
/// `(patina internal io)`: `(scheme file)`'s is Scheme (#471).
pub(super) fn call_with_input_file<'gc>(
    cx: &mut Cx<'gc>,
    _ctx: &dyn ApplyContext,
    args: &[Value<'gc>],
) -> Result<Step<'gc>, EvalError> {
    call_with_file_start(cx, "call-with-input-file", OpenMode::Input, args)
}

/// `call-with-input-file` resumed ([`call_with_file_resume`]).
pub(super) fn call_with_input_file_resume<'gc>(
    cx: &mut Cx<'gc>,
    _ctx: &dyn ApplyContext,
    state: Value<'gc>,
    value: Value<'gc>,
) -> Result<Step<'gc>, EvalError> {
    call_with_file_resume(cx, "call-with-input-file", OpenMode::Input, state, value)
}

/// (call-with-output-file filename proc) - Opens file for writing, calls
/// proc with port, closes (and so flushes) it if proc returns. Reached only
/// by importing `(patina internal io)`: `(scheme file)`'s is Scheme (#471).
pub(super) fn call_with_output_file<'gc>(
    cx: &mut Cx<'gc>,
    _ctx: &dyn ApplyContext,
    args: &[Value<'gc>],
) -> Result<Step<'gc>, EvalError> {
    call_with_file_start(cx, "call-with-output-file", OpenMode::Output, args)
}

/// `call-with-output-file` resumed ([`call_with_file_resume`]).
pub(super) fn call_with_output_file_resume<'gc>(
    cx: &mut Cx<'gc>,
    _ctx: &dyn ApplyContext,
    state: Value<'gc>,
    value: Value<'gc>,
) -> Result<Step<'gc>, EvalError> {
    call_with_file_resume(cx, "call-with-output-file", OpenMode::Output, state, value)
}
