//! Directory operations, routed through the VFS `FileSystem` trait.
//!
//! These back the portable half of `(chibi filesystem)` — the half that can be
//! given an in-memory implementation. The POSIX half (file descriptors, stat
//! fields, symlinks, pipes) is deliberately not here; see the `patina`
//! `cond-expand` branch in `lib/chibi/filesystem.sld` for where that boundary
//! is drawn and why.
//!
//! Names follow `(chibi filesystem)`, which is what the ecosystem imports:
//! `directory-files` rather than `read-dir`, `current-directory` as a getter
//! with `change-directory` as the separate setter.
//!
//! **Failures raise here; chibi's return `#f`.** `create-directory`,
//! `delete-directory` and `change-directory` answer `#f` on failure upstream,
//! and `directory-files` answers `'()` for a directory it cannot read. Every
//! one of these raises a catchable `IOError` instead, which is a deliberate
//! deviation and a recorded one (audit F1): a silent `#f` is how a caller ends
//! up deleting the wrong tree, and the raising form is what Patina's own
//! callers are written against.
//!
//! The cost is real and belongs here rather than in a commit message:
//! chibi-ecosystem code that *branches* on `#f` gets an exception instead —
//! including upstream's own `create-directory*` idiom, `(or (file-directory?
//! dir) … (create-directory dir))`. `lib/chibi/filesystem.sld`'s `(patina …)`
//! branch is written against the raising form (its `delete-file-hierarchy`
//! spells tolerance with `guard` rather than a return-value test), and
//! `lib/chibi/PROVENANCE.md` records it for that tree.

use crate::apply_context::ApplyContext;
use patina_core::{Heap, TaggedValue};
use patina_runtime::EvalError;
use std::path::Path;

fn get_string_tv(tv: TaggedValue, heap: &std::cell::Ref<'_, Heap>) -> Option<String> {
    heap.get_string_contents(tv)
}

/// Pull a single string argument, or report the arity/type error naming `who`.
fn one_path(who: &str, ctx: &dyn ApplyContext, args: &[TaggedValue]) -> Result<String, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: format!("{who} expects 1 argument"),
            actual: args.len(),
        });
    }
    let heap = ctx.heap();
    let heap_ref = heap.borrow();
    get_string_tv(args[0], &heap_ref)
        .ok_or_else(|| EvalError::TypeError(format!("{who} expects a string path")))
}

fn io_error(who: &str, path: &str, e: std::io::Error) -> EvalError {
    EvalError::IOError(format!("{who}: {path}: {e}"))
}

/// `(directory-files dir)` — entry names in `dir`.
///
/// Includes `.` and `..`, which the VFS layer omits: chibi's callers expect
/// them (its own `directory-fold-tree` filters them out by name), so leaving
/// them off would silently change the traversal every consumer writes.
pub(super) fn directory_files(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    let path = one_path("directory-files", ctx, &args)?;
    let mut names = ctx
        .fs()
        .read_dir(Path::new(&path))
        .map_err(|e| io_error("directory-files", &path, e))?;
    names.sort();

    let heap = ctx.heap();
    let mut list = TaggedValue::NULL;
    for name in names
        .iter()
        .rev()
        .chain([&"..".to_string(), &".".to_string()])
    {
        let s = heap.borrow_mut().alloc_string(name.clone());
        list = heap.borrow_mut().alloc_pair(s, list);
    }
    Ok(list)
}

/// `(create-directory dir [mode])` — the mode is accepted and ignored, since
/// the VFS layer has no permission model to apply it to.
pub(super) fn create_directory(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "create-directory expects 1 or 2 arguments".to_string(),
            actual: args.len(),
        });
    }
    let path = one_path("create-directory", ctx, &args[..1])?;
    ctx.fs()
        .create_dir(Path::new(&path))
        .map_err(|e| io_error("create-directory", &path, e))?;
    Ok(TaggedValue::boolean(true))
}

/// `(delete-directory dir)` — the directory must be empty.
pub(super) fn delete_directory(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    let path = one_path("delete-directory", ctx, &args)?;
    ctx.fs()
        .remove_dir(Path::new(&path))
        .map_err(|e| io_error("delete-directory", &path, e))?;
    Ok(TaggedValue::boolean(true))
}

/// `(current-directory)` — getter only, as in `(chibi filesystem)`.
pub(super) fn current_directory(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "current-directory expects 0 arguments".to_string(),
            actual: args.len(),
        });
    }
    let dir = ctx
        .fs()
        .current_dir()
        .map_err(|e| io_error("current-directory", ".", e))?;
    let heap = ctx.heap();
    let s = heap
        .borrow_mut()
        .alloc_string(dir.to_string_lossy().into_owned());
    Ok(s)
}

/// `(change-directory dir)`
pub(super) fn change_directory(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    let path = one_path("change-directory", ctx, &args)?;
    ctx.fs()
        .set_current_dir(Path::new(&path))
        .map_err(|e| io_error("change-directory", &path, e))?;
    Ok(TaggedValue::boolean(true))
}

/// `(file-directory? path)`
pub(super) fn file_directory_p(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    let path = one_path("file-directory?", ctx, &args)?;
    Ok(TaggedValue::boolean(ctx.fs().is_dir(Path::new(&path))))
}

/// `(file-regular? path)` — a plain file, so explicitly not a directory.
pub(super) fn file_regular_p(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    let path = one_path("file-regular?", ctx, &args)?;
    let p = Path::new(&path);
    Ok(TaggedValue::boolean(
        ctx.fs().is_file(p) && !ctx.fs().is_dir(p),
    ))
}
