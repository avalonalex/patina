//! Which dialect the reader will accept.
//!
//! Patina reads a little more than R7RS-small: the R6RS surface syntax that
//! R7RS 7.1.1 reserves — square brackets, `#vu8(…)`, a `(library …)` form, and
//! a version reference in a library name — plus a bare `@` token. None of it
//! can appear in a conforming R7RS program, so reading it widens the accepted
//! language without changing what any conforming program means, and it is what
//! lets R6RS source be read at all.
//!
//! It is nonetheless **off by default**. Patina is a teaching implementation of
//! R7RS, and a learner who writes `(let ([x 1]) …)`, watches it work, and
//! concludes brackets are standard has been taught something false — they will
//! find out from a different implementation instead of from us. So the reader
//! is R7RS unless asked otherwise, and a program that runs without
//! `PATINA_ALLOW_R6RS` is one whose *syntax* another R7RS implementation will
//! also accept.
//!
//! This is deliberately one switch and not an inferred dialect mode. A mode
//! would have to be triggered by something, and every candidate is unreliable:
//! `#!r6rs` is optional in R6RS and routinely omitted, `.sls` is convention
//! rather than normative, and an inline `(library …)` typed at the REPL has no
//! file at all. A mode inferred from those would be right most of the time and
//! silently wrong the rest. Here the reader never guesses, and the person who
//! knows the intent supplies it.
//!
//! The cost of one switch is granularity: it is process-wide, so a program run
//! with it gets R6RS reading for every file it loads, R7RS ones included. The
//! refinement available later is to widen — never narrow — for a file that
//! declares itself, by `.sls` extension or a `#!r6rs` line. That does not
//! reopen the objection above, because such a file is opting in about itself
//! and the switch remains the escape hatch for one that does not.
//!
//! The bundled R6RS libraries under `lib/r6rs/` need none of this: they are
//! Clinger's R7RS ports, written in plain `define-library` with no bracket,
//! `#vu8(` or version reference anywhere, so they load under the strict
//! default like any other bundled library.
//!
//! What it does *not* separate is semantics, because it cannot. R6RS and R7RS
//! genuinely disagree about `error`'s signature, about whether `assert` exists,
//! and about the condition system, and no reader setting can tell those apart.
//! That boundary lives in the library layer, where a program declares its
//! dialect by importing `(rnrs base)` or `(scheme base)`.

/// Whether the R6RS surface syntax R7RS reserves is read.
///
/// Default is R7RS-only. Set `PATINA_ALLOW_R6RS=1` to read it; `=0` is
/// explicitly off, matching [`crate::library_parser`]'s
/// `PATINA_STRICT_LIBRARY_SYNTAX`.
///
/// Read once per [`crate::Lexer`] rather than per token, and per call at the
/// library-parsing sites, which run once per library.
pub fn allow_r6rs() -> bool {
    std::env::var_os("PATINA_ALLOW_R6RS").is_some_and(|v| v != "0")
}
