//! Patina Core - Foundation types for Patina Scheme interpreter
//!
//! This crate provides the foundational data types shared across all Patina components:
//! - `TaggedValue`: Compact NaN-boxed value representation (8 bytes)
//! - `Heap`: Arena-based storage for heap-allocated objects
//! - `Environment`: Lexical environment for variable bindings
//! - `CoreExpr`: Core intermediate representation for evaluation
//! - `ScopeId`, `ScopeSet`: Scope tracking for macro hygiene
//! - `PVRef`, `MatchEnv`: Pattern variable references for macro expansion
//! - `Library`: R7RS library representation
//! - `CompiledMacro`: Compiled syntax-rules macros
//! - `ErrorKind`, `ErrorDetail`: Unified error handling
//!
//! By placing these types in a foundation crate, we avoid circular dependencies
//! and enable type-safe representations (no `dyn Any` needed).
#![expect(
    clippy::disallowed_macros,
    reason = "the crate's `thread_local!` statics, per thread and none a heap value: \
              `cont_value::EMPTY_CONT_ENV`, the shared empty node; `port`'s `STDIN_UNREAD`, \
              `STDIN_POSITION`, `STDIN_FOLD_CASE` and `STDIN_CARRY`, standard input's state, \
              shared on purpose by every port that reads standard input, of which there is one \
              (per thread, so a second thread would keep its own read-ahead of the one stream, \
              C9), and `OUTPUT_FILES`, the open file output ports to flush at exit; `scope::SCOPE_ORIGINS` \
              and `scope_trace::PHASE`, debugging aids. Clippy takes this lint only at the crate \
              root, so a new `thread_local!` in the crate goes on this list (#622)"
)]
#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "the core's unit tests build environments with `Environment::with_parent` and \
                  evaluate nothing; the core's own code is linted like any crate's (#622)"
    )
)]

pub mod compiled_macro;
pub mod cont_value;
pub mod continuation;
pub mod core_expr;
pub mod core_syntax;
pub mod cps_expr;
pub mod debug_format;
pub mod environment;
pub mod error;
pub mod features;
pub mod heap;
pub mod library;
pub mod macro_debug;
pub mod numeric;
pub mod port;
pub mod procedure;
pub mod pvref;
pub mod record_type;
pub mod scope;
pub mod scope_resolve;
pub mod scope_trace;
pub mod source_document;
pub mod source_map;
pub mod tagged_value;
pub mod vfs;
pub mod walk;

// Re-export main types for convenience
pub use compiled_macro::{CompiledMacro, CompiledRule, Identifier, Pattern, Template};
pub use continuation::{
    CpsContinuation, DynamicWindRecord, WindRecord, next_dynamic_wind_id, next_prompt_id,
};
pub use core_expr::{
    CoreExpr, CoreExprKind, Formals, LambdaBody, QuasiConstructor, QuasiTemplate, ScopedParam,
    Symbol,
};
pub use core_syntax::{ALL_CORE_FORMS, CoreForm};
pub use cps_expr::{CpsExpr, CpsExprKind, CpsParam, CpsPrimitive, PromptTag};
pub use environment::{
    BindingLocation, Environment, IntroducedDefinition, ScopedBinding, ScopedSetError,
};
pub use error::{ErrorDetail, ErrorKind, ExceptionKind, ExceptionObject, SourceLocation};
pub use heap::PromiseState;
pub use library::Library;
pub use port::{Port, PortData, PortDirection, PortKind, StdioKind, StringPortData};
pub use procedure::{Arity, Procedure};
pub use pvref::{MatchEnv, MatchValue, PVRef};
pub use record_type::{RecordTypeDescriptor, next_record_type_id};
pub use scope::{ScopeId, ScopeSet};
pub use source_map::{SourceMap, prune_freed_locations};

// TaggedValue and heap types for compact value representation
pub use debug_format::{escape_invisible, format_tagged, format_tagged_with_scopes};
// The collector itself — `Collector`, `MarkSweepCollector`, `run_mark_phase`,
// `Heap::sweep`, `GcController::collect` — is crate-private (#624): code
// outside this crate collects only through `GcController::safe_point`.
pub use heap::gc::{
    ArenaCounts, AssertNoGc, GcController, GcDeferGuard, GcMode, GcRoots, GcStats, GcVisitor,
    MarkBits, NoGcScopes,
};
pub use heap::{
    GC_CHECK, GcFreedBits, Heap, SharedHeap, SpineEnd,
    gc::{trace_cont_env, trace_cont_value, trace_exception_handler, trace_prompt_frame},
    new_shared_heap,
};
pub use tagged_value::TaggedValue;
pub use vfs::{FileSystem, MemoryFs, NativeFs, OverlayFs};

#[cfg(test)]
pub use scope::reset_scope_counter;
