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

pub mod compiled_macro;
pub mod cont_value;
pub mod continuation;
pub mod core_expr;
pub mod core_syntax;
pub mod cps_expr;
pub mod debug_format;
pub mod environment;
pub mod error;
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
pub mod tagged_value;
pub mod vfs;

// Re-export main types for convenience
pub use compiled_macro::{
    CompiledMacro, CompiledRule, Identifier, LiteralBinding, Pattern, Template,
};
pub use continuation::{CpsContinuation, DynamicWindRecord, next_dynamic_wind_id};
pub use core_expr::{CoreExpr, CoreExprKind, Formals, LambdaBody, ScopedParam, Symbol};
pub use core_syntax::{ALL_CORE_FORMS, CoreForm};
pub use cps_expr::{CpsExpr, CpsExprKind, CpsParam, CpsPrimitive, PromptTag};
pub use environment::{Environment, ScopedBinding};
pub use error::{ErrorDetail, ErrorKind, ExceptionKind, ExceptionObject, SourceLocation};
pub use heap::PromiseState;
pub use library::Library;
pub use port::{Port, PortData, PortDirection, PortKind, StdioKind, StringPortData};
pub use procedure::{Arity, Procedure};
pub use pvref::{MatchEnv, MatchValue, PVRef};
pub use record_type::{RecordTypeDescriptor, next_record_type_id};
pub use scope::{ScopeId, ScopeSet};

// TaggedValue and heap types for compact value representation
pub use debug_format::{escape_invisible, format_tagged, format_tagged_with_scopes};
pub use heap::gc::{
    ArenaCounts, Collector, GcController, GcDeferGuard, GcMode, GcRoots, GcStats, GcVisitor,
    MarkBits, MarkSweepCollector, run_mark_phase,
};
pub use heap::{
    GcFreedBits, Heap, SharedHeap,
    gc::{trace_cont_env, trace_cont_value, trace_exception_handler},
    new_shared_heap,
};
pub use tagged_value::TaggedValue;
pub use vfs::{FileSystem, MemoryFs, NativeFs, OverlayFs};

#[cfg(test)]
pub use scope::reset_scope_counter;
