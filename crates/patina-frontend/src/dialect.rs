//! Which dialect the reader will accept.
//!
//! Patina reads a little more than R7RS-small: the R6RS surface syntax that
//! R7RS 7.1.1 reserves — square brackets, `#vu8(…)`, a `(library …)` form, and
//! a version reference in a library name — plus a bare `@` token. None of it
//! can appear in a conforming R7RS program, so reading it widens the accepted
//! language without changing what any conforming program means, and it is what
//! lets R6RS source be read at all.
//!
//! That leniency has one cost worth a switch. Patina is a teaching
//! implementation, and a learner who writes `(let ([x 1]) …)`, watches it work,
//! and concludes brackets are standard has been taught something false — they
//! will find out from a different implementation instead of from us.
//! `PATINA_STRICT_R7RS=1` closes the gap: the extensions become errors again,
//! so a program that runs under it is one whose *syntax* another R7RS
//! implementation will also accept.
//!
//! This is deliberately a strictness switch and not a dialect mode. A mode
//! would have to be triggered by something, and every candidate is unreliable:
//! `#!r6rs` is optional in R6RS and routinely omitted, `.sls` is convention
//! rather than normative, and an inline `(library …)` typed at the REPL has no
//! file at all. A mode inferred from those would be right most of the time and
//! silently wrong the rest, which is worse than not guessing. Here the reader
//! never guesses, and the person who knows the intent supplies it.
//!
//! What it does *not* separate is semantics, because it cannot. R6RS and R7RS
//! genuinely disagree about `error`'s signature, about whether `assert` exists,
//! and about the condition system, and no reader setting can tell those apart.
//! That boundary lives in the library layer, where a program declares its
//! dialect by importing `(rnrs base)` or `(scheme base)`.

/// Whether the R6RS surface syntax R7RS reserves is rejected.
///
/// Default is lenient. Set `PATINA_STRICT_R7RS=1` to reject it; `=0` is
/// explicitly lenient, matching [`crate::library_parser`]'s
/// `PATINA_STRICT_LIBRARY_SYNTAX`.
///
/// Read once per [`crate::Lexer`] rather than per token, and per call at the
/// library-parsing sites, which run once per library.
pub fn strict_r7rs() -> bool {
    std::env::var_os("PATINA_STRICT_R7RS").is_some_and(|v| v != "0")
}
