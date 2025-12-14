use num_bigint::BigInt;
use num_rational::BigRational;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::compiled_macro::CompiledMacro;
use crate::core_expr::{CoreExpr, ScopedParam};
use crate::cps_expr::{CpsExpr, PromptTag};
use crate::environment::Environment;
use crate::library::Library;
use crate::port::Port;
use crate::scope::{ScopeId, ScopeSet};

// =============================================================================
// Value Interning Infrastructure
// =============================================================================
//
// Symbol interning and small integer caching reduce allocations by sharing
// common values. This is Phase 4 of the VALUE_SIZE_OPTIMIZATION plan.
//
// Both use thread-local storage because Value contains Rc<...> types which
// are not Sync (not safe to share across threads). Thread-local storage
// provides per-thread caching without synchronization overhead.

// Thread-local symbol interner
// Maps symbol names to their interned `Rc<str>` representations.
thread_local! {
    static SYMBOL_INTERNER: RefCell<HashMap<String, Rc<str>>> = RefCell::new(HashMap::new());
}

// Thread-local cache for small integers (-128 to 127)
// These 256 integers are pre-allocated per thread and reused.
thread_local! {
    static SMALL_INTEGER_CACHE: RefCell<Option<Vec<Value>>> = const { RefCell::new(None) };
}

/// Initialize the small integer cache for the current thread
fn init_integer_cache() -> Vec<Value> {
    (-128i64..128).map(Value::Integer).collect()
}

/// Represents a Scheme value in the R7RS-small language
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Value {
    // Booleans
    Boolean(bool),

    // Numbers - R7RS requires full numeric tower
    Integer(i64),
    BigInteger(BigInt),
    Rational(BigRational),
    Real(f64),
    /// Complex numbers store their real and imaginary parts as boxed Values.
    /// This preserves exactness: `1+2i` has exact parts, `1.0+2.0i` has inexact parts.
    /// R7RS requires distinguishing `x+0i` (exact zero imag, is real) from
    /// `x+0.0i` (inexact zero imag, is NOT real).
    Complex(Box<(Value, Value)>), // (real_part, imag_part)

    // Characters (Unicode support)
    Character(char),

    // Strings (mutable in Scheme)
    // Uses Vec<char> for O(1) character indexing and mutation
    // UTF-8 conversion happens at I/O boundaries (display, write, file ops)
    // Memory: 4 bytes per character (fixed, predictable)
    String(Rc<RefCell<Vec<char>>>),

    // Symbols
    Symbol(Rc<str>),

    // Identifier for hygienic macro expansion (Racket-style scope sets)
    // Boxed to reduce enum size: 64 bytes -> 8 bytes
    //
    // Each identifier carries a set of scopes that track which binding forms it's inside.
    // This is the key to hygienic macro expansion: identifiers from different lexical
    // contexts have different scope sets, allowing them to be distinguished.
    //
    // ## Reference
    // - Flatt "Binding as Sets of Scopes" (POPL 2016)
    //
    // ## Hygiene via flip-scope
    // During macro expansion:
    // 1. Before expansion: flip macro_scope on INPUT (adds to use-site identifiers)
    // 2. After expansion: flip macro_scope on OUTPUT
    //    - Use-site identifiers: scope removed (was added, then toggled off)
    //    - Introduced identifiers: scope added (wasn't there, then toggled on)
    //
    // This distinguishes use-site vs introduced identifiers using only scopes.
    Identifier(Box<IdentifierData>),

    // Pairs and lists (mutable via set-car!/set-cdr!)
    // Uses RefCell to allow mutation through shared references
    Pair(Rc<RefCell<(Value, Value)>>),
    Null,

    // Vectors (mutable via vector-set!)
    // Uses RefCell to allow mutation through shared references
    Vector(Rc<RefCell<Vec<Value>>>),

    // Bytevectors
    // Uses RefCell to allow mutation through shared references
    Bytevector(Rc<RefCell<Vec<u8>>>),

    // Procedures (Rc to preserve identity for eq?/eqv?)
    // Using Rc instead of Box so that cloning preserves identity
    Procedure(Rc<Procedure>),

    // Parameters (R7RS dynamic parameters)
    // Parameters are special procedures that maintain dynamic state
    // Can be called with 0 args (get value) or 1 arg (set value)
    // Uses a stack to support nested parameterize
    //
    // TODO: For multi-threading support, parameter value stacks need to be thread-local.
    // Currently uses shared Rc<RefCell<Vec<Value>>> which is fine for single-threaded
    // R7RS-small. See SRFI-226 (Control Features) for thread-parameter interaction spec.
    Parameter {
        values: Rc<RefCell<Vec<Value>>>, // Stack of values (top = current value)
        converter: Option<Box<Value>>,   // Optional converter function
    },

    // Ports (for I/O)
    // Full port implementation with string ports, stdio, etc.
    Port(Rc<Port>),

    // Macros (for syntax-rules)
    // Type-safe macro storage - no dyn Any needed
    Macro(Rc<CompiledMacro>),

    // Libraries (R7RS Section 5.6)
    // Represents a loaded library with its exports and environment
    Library(Rc<Library>),

    // Record types (R7RS Section 5.5)
    // Record type descriptor - the type itself (result of define-record-type)
    RecordType(Rc<RecordTypeDescriptor>),

    // Record instance - an instance of a record type
    // Fields stored in a RefCell to allow mutation via modifier procedures
    Record {
        record_type: Rc<RecordTypeDescriptor>,
        fields: Rc<RefCell<Vec<Value>>>,
    },

    // Multiple values (R7RS Section 6.10)
    Values(Vec<Value>),

    // Promises (R7RS Section 6.10 - scheme lazy)
    // A promise is a delayed computation that can be forced to produce a value
    Promise(Rc<RefCell<PromiseState>>),

    // Environment specifier (R7RS Section 6.12 - scheme eval)
    // Represents a first-class environment that can be passed to eval
    // The mutable flag indicates whether definitions are allowed
    EnvironmentSpecifier {
        env: Rc<crate::environment::Environment>,
        mutable: bool,
    },

    // Special values
    Unspecified,
    Eof,

    // ==========================================================================
    // Exception Handling (R7RS Section 6.11)
    // ==========================================================================
    /// Exception object for R7RS exception handling
    /// Created by `error` procedure or by converting internal errors
    /// Used with `guard`, `raise`, `with-exception-handler`
    Exception(Rc<ExceptionObject>),

    /// Placeholder for datum label references during parsing.
    /// This is a temporary value that only exists during `read`.
    /// After parsing, all placeholders are resolved to actual values.
    /// R7RS Section 2.4: Datum labels for shared/cyclic structures.
    LabelPlaceholder(usize),

    // =========================================================================
    // CPS Continuation Support
    // =========================================================================
    /// Continuation prompt tag for delimited continuations
    /// Created by `make-continuation-prompt-tag`
    /// Used to identify prompt boundaries for shift/reset
    ContinuationPromptTag(Rc<PromptTag>),

    /// A captured CPS continuation (first-class continuation)
    /// This represents a "frozen" point in the computation that can be resumed.
    /// Created by `call/cc` or `shift`.
    Continuation(Rc<CpsContinuation>),
}

/// State of a promise for lazy evaluation
#[derive(Debug, Clone)]
pub enum PromiseState {
    /// Not yet evaluated - contains the thunk to evaluate
    Delayed(Value),
    /// Evaluated - contains the cached result
    Forced(Value),
}

// =============================================================================
// Exception Object (R7RS Section 6.11)
// =============================================================================

/// Exception object for R7RS exception handling
///
/// Created by the `error` procedure or by converting internal errors.
/// Can be caught by `guard` or `with-exception-handler`.
#[derive(Debug, Clone)]
pub struct ExceptionObject {
    /// The kind of exception (error, file-error, read-error, etc.)
    pub kind: ExceptionKind,
    /// The error message string
    pub message: String,
    /// Additional values related to the error ("irritants" in R7RS terminology)
    pub irritants: Vec<Value>,
}

/// The kind of exception
///
/// R7RS defines specific predicates for different exception types:
/// - `error-object?` - any error object created by `error`
/// - `file-error?` - I/O errors related to files
/// - `read-error?` - errors during parsing/reading
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExceptionKind {
    /// Generic error created by `(error message irritant ...)`
    Error,
    /// File I/O error (maps from EvalError::IOError)
    /// Satisfies `file-error?` predicate
    FileError,
    /// Read/parse error (maps from parse errors)
    /// Satisfies `read-error?` predicate
    ReadError,
    /// User-defined exception kind
    Custom(String),
}

// =============================================================================
// CPS Continuation Types
// =============================================================================

/// A captured CPS continuation
///
/// In CPS, a continuation represents "what to do with a value". When captured
/// by `call/cc` or `shift`, the continuation becomes a first-class value that
/// can be stored and invoked later.
///
/// ## Full vs Delimited Continuations
///
/// - **Full continuation** (from `call/cc`): Captures everything from the call
///   site to the top level. When invoked, abandons the current computation.
///
/// - **Delimited continuation** (from `shift`): Captures only up to the nearest
///   enclosing `reset` prompt. When invoked, can return to the caller.
#[derive(Debug, Clone)]
pub struct CpsContinuation {
    /// The CPS expression representing the captured computation
    /// When the continuation is invoked with a value, this expression
    /// is evaluated with the value bound to `param`.
    pub body: Rc<CpsExpr>,

    /// The parameter name that receives the value when continuation is invoked
    pub param: Rc<str>,

    /// The captured environment at the point of continuation capture
    pub env: Rc<Environment>,

    /// For delimited continuations: the prompt tag this was captured at
    /// None for full continuations (call/cc)
    pub prompt_tag: Option<Rc<PromptTag>>,

    /// Dynamic wind handlers that were active when this continuation was captured
    /// These need to be reinstalled when the continuation is invoked
    pub dynamic_winds: Vec<DynamicWindRecord>,

    /// Captured continuation bindings that were in scope when this continuation
    /// was captured. Each entry is (name, continuation) representing a let-cont
    /// binding that the continuation body may reference.
    /// This is a Vec of boxed CpsContinuations rather than a HashMap to avoid
    /// circular type dependencies and to keep patina-core dependency-free.
    pub captured_cont_bindings: Vec<(Rc<str>, Rc<CpsContinuation>)>,
}

/// Global counter for generating unique dynamic-wind IDs
static DYNAMIC_WIND_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A record of a dynamic-wind that needs to be managed during continuation jumps
#[derive(Debug, Clone)]
pub struct DynamicWindRecord {
    /// Unique identifier for this dynamic-wind invocation
    /// Used to find the common prefix when switching continuations
    pub id: u64,
    /// The "before" thunk to call when entering this dynamic extent
    pub before: Value,
    /// The "after" thunk to call when leaving this dynamic extent
    pub after: Value,
}

impl DynamicWindRecord {
    /// Create a new dynamic-wind record with a unique ID
    pub fn new(before: Value, after: Value) -> Self {
        Self {
            id: DYNAMIC_WIND_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
            before,
            after,
        }
    }
}

/// Data for a hygienic identifier (boxed in Value::Identifier)
#[derive(Debug, Clone)]
pub struct IdentifierData {
    pub name: Rc<str>,
    /// Set of scopes for scope-sets hygiene
    /// Empty set = top-level identifier
    pub scopes: ScopeSet,
}

// =============================================================================
// Record Types (R7RS Section 5.5)
// =============================================================================

use std::sync::atomic::{AtomicUsize, Ordering};

/// Global counter for generating unique record type IDs (generative semantics)
static RECORD_TYPE_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Generate a new unique record type ID
///
/// Each call to define-record-type creates a new type with a unique ID.
/// This ensures generative semantics: two record types with the same
/// name and fields are still distinct types.
pub fn next_record_type_id() -> usize {
    RECORD_TYPE_COUNTER.fetch_add(1, Ordering::SeqCst)
}

/// Record type descriptor - represents a record type itself
///
/// Created by `define-record-type`. Each invocation creates a new descriptor
/// with a unique ID (generative semantics), even if the name and fields match
/// a previous definition.
///
/// # R7RS Compliance
///
/// From R7RS Section 5.5:
/// > The define-record-type construct is generative: each use creates a new
/// > record type that is distinct from all existing types, including Scheme's
/// > predefined types and other record types — even record types of the same
/// > name or structure.
#[derive(Debug, Clone)]
pub struct RecordTypeDescriptor {
    /// Unique identifier for this record type (generative semantics)
    pub id: usize,
    /// Name of the record type (for display purposes)
    pub name: Rc<str>,
    /// Field names in declaration order
    pub fields: Vec<Rc<str>>,
}

impl RecordTypeDescriptor {
    /// Create a new record type descriptor with a unique ID
    pub fn new(name: &str, fields: Vec<String>) -> Self {
        RecordTypeDescriptor {
            id: next_record_type_id(),
            name: Rc::from(name),
            fields: fields.into_iter().map(|s| Rc::from(s.as_str())).collect(),
        }
    }

    /// Get the index of a field by name, if it exists
    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|f| f.as_ref() == name)
    }
}

impl PartialEq for RecordTypeDescriptor {
    fn eq(&self, other: &Self) -> bool {
        // Identity based on unique ID only (generative semantics)
        // Two record types with the same name/fields are still distinct
        self.id == other.id
    }
}

impl Eq for RecordTypeDescriptor {}

/// Lambda body representation - type-safe replacement for `dyn Any`
///
/// Lambda body stored as CoreExpr (preserves scope IDs for hygiene)
///
/// Previously this was an enum with `Values(Vec<Value>)` and `Core(Vec<CoreExpr>)`,
/// but the Values variant was never used. All lambdas are now created with CoreExpr bodies.
pub type LambdaBody = Vec<CoreExpr>;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Procedure {
    /// Built-in primitive procedure
    Primitive {
        name: &'static str,
        arity: Arity,
        library: Vec<String>, // Library namespace, e.g., ["scheme", "base"]
    },

    /// CPS-style lambda - for use with CPS evaluator
    ///
    /// These lambdas are created by CPS transformation and must be evaluated
    /// by the CPS evaluator. They have an explicit continuation parameter
    /// and their body is a CpsExpr that will call the continuation.
    CpsLambda {
        /// Fixed parameters, each with optional scopes for hygiene
        params: Vec<ScopedParam>,
        /// Optional variadic parameter (rest parameter)
        variadic: Option<ScopedParam>,
        /// Name of the continuation parameter
        cont_param: Rc<str>,
        /// Procedure body (CPS-style CpsExpr)
        body: Rc<CpsExpr>,
        /// Captured environment for closures
        env: Rc<Environment>,
        /// Binding scope for parameters without scopes (for hygiene)
        /// When present, parameters without explicit scopes will also be bound
        /// with this scope, allowing macro-expanded references to find them.
        binding_scope: Option<ScopeId>,
    },
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Arity {
    Exact(usize),
    Min(usize),
    Range(usize, usize),
}

impl Value {
    pub fn is_truthy(&self) -> bool {
        !matches!(self, Value::Boolean(false))
    }

    #[allow(dead_code)]
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Boolean(_) => "boolean",
            Value::Integer(_) | Value::BigInteger(_) => "integer",
            Value::Rational(_) => "rational",
            Value::Real(_) => "real",
            Value::Complex(_) => "complex",
            Value::Character(_) => "character",
            Value::String(_) => "string",
            Value::Symbol(_) => "symbol",
            Value::Identifier(_) => "identifier",
            Value::Pair(_) | Value::Null => "list",
            Value::Vector(_) => "vector",
            Value::Bytevector(_) => "bytevector",
            Value::Procedure(_) => "procedure",
            Value::Parameter { .. } => "parameter",
            Value::Port(_) => "port",
            Value::Macro(_) => "macro",
            Value::Library(_) => "library",
            Value::RecordType(_) => "record-type",
            Value::Record { .. } => "record",
            Value::Values(_) => "values",
            Value::Promise(_) => "promise",
            Value::EnvironmentSpecifier { .. } => "environment",
            Value::Unspecified => "unspecified",
            Value::Eof => "eof-object",
            Value::Exception(_) => "error-object",
            Value::LabelPlaceholder(_) => "label-placeholder",
            Value::ContinuationPromptTag(_) => "continuation-prompt-tag",
            Value::Continuation(_) => "continuation",
        }
    }

    // =========================================================================
    // Interned Constructors
    // =========================================================================

    /// Create an interned symbol
    ///
    /// Symbols with the same name share the same `Rc<str>`, making `eq?`
    /// comparison O(1) (pointer comparison) and reducing memory usage.
    ///
    /// # Example
    /// ```ignore
    /// let a = Value::symbol("foo");
    /// let b = Value::symbol("foo");
    /// // a and b share the same Rc<str>
    /// ```
    pub fn symbol(name: &str) -> Value {
        SYMBOL_INTERNER.with(|interner| {
            let mut map = interner.borrow_mut();
            let rc = map
                .entry(name.to_string())
                .or_insert_with(|| Rc::from(name))
                .clone();
            Value::Symbol(rc)
        })
    }

    /// Create an identifier value with empty scopes (for hygiene)
    ///
    /// Identifiers are like symbols but carry scope sets for macro hygiene.
    /// This convenience constructor creates an identifier with no scopes.
    ///
    /// # Example
    /// ```ignore
    /// let id = Value::identifier("if");
    /// // Creates Identifier with name "if" and empty scope set
    /// ```
    pub fn identifier(name: &str) -> Value {
        Value::Identifier(Box::new(IdentifierData {
            name: Rc::from(name),
            scopes: ScopeSet::new(),
        }))
    }

    /// Create an integer value, using cache for small integers
    ///
    /// Integers in the range -128..128 are cached and reused, avoiding
    /// allocation for the most commonly used integer values.
    ///
    /// # Example
    /// ```ignore
    /// let a = Value::integer(42);  // Cached, no allocation
    /// let b = Value::integer(42);  // Returns same cached value
    /// let c = Value::integer(1000); // Not cached, creates new value
    /// ```
    pub fn integer(n: i64) -> Value {
        if (-128..128).contains(&n) {
            // Use cached value (index: -128 -> 0, 127 -> 255)
            SMALL_INTEGER_CACHE.with(|cache| {
                let mut cache_ref = cache.borrow_mut();
                if cache_ref.is_none() {
                    *cache_ref = Some(init_integer_cache());
                }
                cache_ref.as_ref().unwrap()[(n + 128) as usize].clone()
            })
        } else {
            Value::Integer(n)
        }
    }

    /// Get the interned `Rc<str>` for a symbol name
    ///
    /// This is useful when you need just the `Rc<str>` without wrapping
    /// it in a `Value::Symbol`, e.g., for use in `IdentifierData`.
    pub fn intern_symbol_name(name: &str) -> Rc<str> {
        SYMBOL_INTERNER.with(|interner| {
            let mut map = interner.borrow_mut();
            map.entry(name.to_string())
                .or_insert_with(|| Rc::from(name))
                .clone()
        })
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Boolean(b) => write!(f, "#{}", if *b { "t" } else { "f" }),
            Value::Integer(n) => write!(f, "{}", n),
            Value::BigInteger(n) => write!(f, "{}", n),
            Value::Rational(r) => write!(f, "{}", r),
            Value::Real(r) => {
                // Handle special floating point values
                if r.is_infinite() {
                    if r.is_sign_positive() {
                        write!(f, "+inf.0")
                    } else {
                        write!(f, "-inf.0")
                    }
                } else if r.is_nan() {
                    write!(f, "+nan.0")
                } else if r.fract() == 0.0 {
                    // Always display inexact numbers with decimal point
                    // to distinguish from exact integers
                    write!(f, "{:.1}", r)
                } else {
                    write!(f, "{}", r)
                }
            }
            Value::Complex(parts) => {
                let (ref real_part, ref imag_part) = **parts;

                // Helper to check if a value is zero
                fn is_zero(v: &Value) -> bool {
                    match v {
                        Value::Integer(n) => *n == 0,
                        Value::BigInteger(n) => n.sign() == num_bigint::Sign::NoSign,
                        Value::Rational(r) => {
                            use num_traits::Zero;
                            r.is_zero()
                        }
                        Value::Real(r) => *r == 0.0,
                        _ => false,
                    }
                }

                // Helper to check if value is exact one (not inexact 1.0)
                // R7RS requires preserving exactness: 1+i is different from 1.0+1.0i
                fn is_exact_one(v: &Value) -> bool {
                    match v {
                        Value::Integer(n) => *n == 1,
                        Value::BigInteger(n) => {
                            use num_traits::One;
                            n == &BigInt::one()
                        }
                        Value::Rational(r) => {
                            use num_traits::One;
                            r.is_one()
                        }
                        // Inexact 1.0 should NOT be treated as "one" for display purposes
                        Value::Real(_) => false,
                        _ => false,
                    }
                }

                // Helper to check if value is exact negative one (not inexact -1.0)
                fn is_exact_neg_one(v: &Value) -> bool {
                    match v {
                        Value::Integer(n) => *n == -1,
                        Value::BigInteger(n) => {
                            use num_traits::One;
                            n == &(-BigInt::one())
                        }
                        Value::Rational(r) => {
                            use num_traits::One;
                            r == &(-BigRational::one())
                        }
                        // Inexact -1.0 should NOT be treated as "negative one" for display purposes
                        Value::Real(_) => false,
                        _ => false,
                    }
                }

                // Helper to check if value is negative
                fn is_negative(v: &Value) -> bool {
                    match v {
                        Value::Integer(n) => *n < 0,
                        Value::BigInteger(n) => n.sign() == num_bigint::Sign::Minus,
                        Value::Rational(r) => {
                            use num_traits::Zero;
                            r < &BigRational::zero()
                        }
                        Value::Real(r) => *r < 0.0,
                        _ => false,
                    }
                }

                let real_is_zero = is_zero(real_part);
                let imag_is_zero = is_zero(imag_part);

                // Format complex numbers properly
                if real_is_zero && imag_is_zero {
                    write!(f, "0")
                } else if real_is_zero {
                    // Pure imaginary
                    if is_exact_one(imag_part) {
                        write!(f, "+i")
                    } else if is_exact_neg_one(imag_part) {
                        write!(f, "-i")
                    } else if is_negative(imag_part) {
                        write!(f, "{}i", imag_part)
                    } else {
                        // Check if imag_part already has a sign (e.g., +inf.0, +nan.0)
                        let imag_str = imag_part.to_string();
                        if imag_str.starts_with('+') || imag_str.starts_with('-') {
                            write!(f, "{}i", imag_part)
                        } else {
                            write!(f, "+{}i", imag_part)
                        }
                    }
                } else if imag_is_zero {
                    // Pure real - display with exactness info
                    write!(f, "{}", real_part)
                } else if is_negative(imag_part) {
                    // Negative imaginary: use - instead of +-
                    if is_exact_neg_one(imag_part) {
                        write!(f, "{}-i", real_part)
                    } else {
                        write!(f, "{}{}i", real_part, imag_part)
                    }
                } else {
                    // Positive imaginary
                    if is_exact_one(imag_part) {
                        write!(f, "{}+i", real_part)
                    } else {
                        // Check if imag_part already has a sign (e.g., +inf.0, +nan.0)
                        let imag_str = imag_part.to_string();
                        if imag_str.starts_with('+') || imag_str.starts_with('-') {
                            write!(f, "{}{}i", real_part, imag_part)
                        } else {
                            write!(f, "{}+{}i", real_part, imag_part)
                        }
                    }
                }
            }
            Value::Character(c) => write!(f, "#\\{}", c),
            Value::String(s) => {
                // Convert Vec<char> to String for display
                let chars = s.borrow();
                let utf8_string: String = chars.iter().collect();
                write!(f, "\"{}\"", utf8_string)
            }
            Value::Symbol(s) => {
                if Self::symbol_needs_vertical_bars(s) {
                    write!(f, "|{}|", Self::escape_for_vertical_bar(s))
                } else {
                    write!(f, "{}", s)
                }
            }
            Value::Identifier(id) => {
                // Display identifiers just as their name
                // Marks and scopes are internal - they shouldn't appear in output
                // They are used for lookup, not display
                if Self::symbol_needs_vertical_bars(&id.name) {
                    write!(f, "|{}|", Self::escape_for_vertical_bar(&id.name))
                } else {
                    write!(f, "{}", id.name)
                }
            }
            Value::Null => write!(f, "()"),
            Value::Pair(_) => {
                // Check for special shorthand forms: quote, quasiquote, unquote, unquote-splicing
                if let Some(shorthand) = self.check_quote_shorthand() {
                    write!(f, "{}", shorthand)
                } else {
                    write!(f, "(")?;
                    self.fmt_list_contents(f)?;
                    write!(f, ")")
                }
            }
            Value::Vector(v) => {
                write!(f, "#(")?;
                let vec = v.borrow();
                for (i, val) in vec.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", val)?;
                }
                write!(f, ")")
            }
            Value::Bytevector(bv) => write!(f, "#u8({:?})", bv.borrow()),
            Value::Procedure(proc) => match proc.as_ref() {
                Procedure::Primitive { name, library, .. } => {
                    write!(f, "#<procedure:{}:{}>", library.join("."), name)
                }
                Procedure::CpsLambda { .. } => write!(f, "#<procedure>"),
            },
            Value::Parameter { .. } => write!(f, "#<parameter>"),
            Value::Port(port) => write!(f, "{}", port),
            Value::Macro(compiled) => write!(f, "#<macro:{}>", compiled.name),
            Value::Library(lib) => write!(f, "{}", lib),
            Value::RecordType(rtd) => write!(f, "#<record-type {}>", rtd.name),
            Value::Record { record_type, .. } => write!(f, "#<record {}>", record_type.name),
            Value::Values(vals) => {
                // Multiple values are usually only seen internally
                // Display as space-separated values
                for (i, val) in vals.iter().enumerate() {
                    if i > 0 {
                        writeln!(f)?;
                    }
                    write!(f, "{}", val)?;
                }
                Ok(())
            }
            Value::Promise(_) => write!(f, "#<promise>"),
            Value::EnvironmentSpecifier { .. } => write!(f, "#<environment>"),
            Value::Unspecified => write!(f, "#<unspecified>"),
            Value::Eof => write!(f, "#<eof>"),
            Value::Exception(exc) => {
                write!(f, "#<error-object: {}>", exc.message)
            }
            Value::LabelPlaceholder(n) => write!(f, "#<label-placeholder:{}>", n),
            Value::ContinuationPromptTag(tag) => write!(f, "{}", tag),
            Value::Continuation(_) => write!(f, "#<continuation>"),
        }
    }
}

impl Value {
    /// Check if a symbol name needs vertical bar notation when displayed
    ///
    /// Returns true if the symbol contains characters that require vertical bars
    /// to be read back correctly (spaces, parentheses, starts with digit, etc.)
    fn symbol_needs_vertical_bars(name: &str) -> bool {
        // Empty symbol needs bars
        if name.is_empty() {
            return true;
        }

        // Check if it looks like a number (would be parsed as number, not symbol)
        let first_char = name.chars().next().unwrap();

        // Starts with digit -> looks like number (including partial like "123abc")
        if first_char.is_ascii_digit() {
            return true;
        }

        // Check for +/- prefix (complex number or signed number)
        if (first_char == '+' || first_char == '-') && name.len() > 1 {
            let second_char = name.chars().nth(1).unwrap();

            // +/-digit -> number
            if second_char.is_ascii_digit() {
                return true;
            }

            // +/-.digit -> number (e.g., -.4)
            if second_char == '.' && name.len() > 2 {
                let third_char = name.chars().nth(2).unwrap();
                if third_char.is_ascii_digit() {
                    return true;
                }
            }

            // +i, -i, +I, -I -> imaginary number
            if matches!(second_char, 'i' | 'I') && name.len() == 2 {
                return true;
            }

            // Check for special floats or things that start with them (case insensitive)
            let lower = name.to_lowercase();
            if lower == "+inf.0" || lower == "-inf.0"
                || lower == "+nan.0" || lower == "-nan.0"
                || lower.starts_with("+inf.0") // e.g., +inf.0xyz
                || lower.starts_with("-inf.0")
                || lower.starts_with("+nan.0") // e.g., +nan.0abc
                || lower.starts_with("-nan.0")
            {
                return true;
            }
        }

        // Single dot needs bars
        if name == "." {
            return true;
        }

        // Check each character
        for (i, ch) in name.chars().enumerate() {
            // Non-ASCII needs bars (per R7RS spec)
            if !ch.is_ascii() {
                return true;
            }

            // First character must be initial or special initial
            if i == 0 {
                let is_valid_initial = ch.is_ascii_alphabetic()
                    || matches!(
                        ch,
                        '!' | '$'
                            | '%'
                            | '&'
                            | '*'
                            | '/'
                            | ':'
                            | '<'
                            | '='
                            | '>'
                            | '?'
                            | '^'
                            | '_'
                            | '~'
                            | '+'
                            | '-'
                            | '.'
                    );
                if !is_valid_initial {
                    return true;
                }
            } else {
                // Subsequent characters can be initial, digit, or special subsequent
                let is_valid_subsequent = ch.is_ascii_alphanumeric()
                    || matches!(
                        ch,
                        '!' | '$'
                            | '%'
                            | '&'
                            | '*'
                            | '/'
                            | ':'
                            | '<'
                            | '='
                            | '>'
                            | '?'
                            | '^'
                            | '_'
                            | '~'
                            | '+'
                            | '-'
                            | '.'
                            | '@'
                    );
                if !is_valid_subsequent {
                    return true;
                }
            }
        }

        false
    }

    /// Escape special characters for display inside vertical bar notation
    ///
    /// R7RS requires that `|` and `\` be escaped as `\|` and `\\` respectively
    /// when appearing inside vertical bar identifiers.
    fn escape_for_vertical_bar(name: &str) -> String {
        let mut result = String::with_capacity(name.len());
        for ch in name.chars() {
            match ch {
                '|' => result.push_str("\\|"),
                '\\' => result.push_str("\\\\"),
                _ => result.push(ch),
            }
        }
        result
    }

    /// Check if this value is a special quote form that can be displayed with shorthand syntax
    ///
    /// Returns Some(formatted_string) if this is a quote-like form, None otherwise.
    ///
    /// Handles:
    /// - (quote expr) -> 'expr
    /// - (quasiquote expr) -> `expr
    /// - (unquote expr) -> ,expr
    /// - (unquote-splicing expr) -> ,@expr
    fn check_quote_shorthand(&self) -> Option<String> {
        if let Value::Pair(pair) = self {
            let borrowed = pair.borrow();
            // Check if car is one of the special symbols
            if let Value::Symbol(sym) = &borrowed.0 {
                let prefix = match sym.as_ref() {
                    "quote" => "'",
                    "quasiquote" => "`",
                    "unquote" => ",",
                    "unquote-splicing" => ",@",
                    _ => return None,
                };

                // Check if cdr is a proper 1-element list: (keyword expr)
                if let Value::Pair(cdr_pair) = &borrowed.1 {
                    let cdr_borrowed = cdr_pair.borrow();
                    if matches!(cdr_borrowed.1, Value::Null) {
                        // It's (keyword expr), format as prefix + expr
                        return Some(format!("{}{}", prefix, cdr_borrowed.0));
                    }
                }

                // If we get here, it's malformed (e.g., (quote a b) or (quote . a))
                // Fall through to regular list formatting
            }
        }
        None
    }

    fn fmt_list_contents(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Pair(p) => {
                let borrowed = p.borrow();
                write!(f, "{}", borrowed.0)?;
                match &borrowed.1 {
                    Value::Null => Ok(()),
                    Value::Pair(_) => {
                        write!(f, " ")?;
                        borrowed.1.fmt_list_contents(f)
                    }
                    other => write!(f, " . {}", other),
                }
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== Complex Number Display Tests ==========

    #[test]
    fn test_complex_display_exact_one() {
        // Exact 1+i should display as "1+i"
        let c = Value::Complex(Box::new((Value::Integer(1), Value::Integer(1))));
        assert_eq!(c.to_string(), "1+i");
    }

    #[test]
    fn test_complex_display_exact_neg_one() {
        // Exact 1-i should display as "1-i"
        let c = Value::Complex(Box::new((Value::Integer(1), Value::Integer(-1))));
        assert_eq!(c.to_string(), "1-i");
    }

    #[test]
    fn test_complex_display_inexact_one() {
        // Inexact 1.0+1.0i should display as "1.0+1.0i" (preserves exactness)
        let c = Value::Complex(Box::new((Value::Real(1.0), Value::Real(1.0))));
        assert_eq!(c.to_string(), "1.0+1.0i");
    }

    #[test]
    fn test_complex_display_inexact_neg_one() {
        // Inexact 1.0-1.0i should display as "1.0-1.0i" (preserves exactness)
        let c = Value::Complex(Box::new((Value::Real(1.0), Value::Real(-1.0))));
        assert_eq!(c.to_string(), "1.0-1.0i");
    }

    #[test]
    fn test_complex_display_positive_infinity() {
        // +inf.0+inf.0i should NOT have double + sign
        let c = Value::Complex(Box::new((
            Value::Real(f64::INFINITY),
            Value::Real(f64::INFINITY),
        )));
        assert_eq!(c.to_string(), "+inf.0+inf.0i");
    }

    #[test]
    fn test_complex_display_negative_real_positive_inf_imag() {
        // -inf.0+inf.0i
        let c = Value::Complex(Box::new((
            Value::Real(f64::NEG_INFINITY),
            Value::Real(f64::INFINITY),
        )));
        assert_eq!(c.to_string(), "-inf.0+inf.0i");
    }

    #[test]
    fn test_complex_display_real_with_positive_inf_imag() {
        // 1.0+inf.0i should NOT have double + sign
        let c = Value::Complex(Box::new((Value::Real(1.0), Value::Real(f64::INFINITY))));
        assert_eq!(c.to_string(), "1.0+inf.0i");
    }

    #[test]
    fn test_complex_display_pure_positive_inf_imag() {
        // Pure imaginary +inf.0i (real is zero)
        let c = Value::Complex(Box::new((Value::Integer(0), Value::Real(f64::INFINITY))));
        assert_eq!(c.to_string(), "+inf.0i");
    }

    #[test]
    fn test_complex_display_pure_negative_inf_imag() {
        // Pure imaginary -inf.0i (real is zero)
        let c = Value::Complex(Box::new((
            Value::Integer(0),
            Value::Real(f64::NEG_INFINITY),
        )));
        assert_eq!(c.to_string(), "-inf.0i");
    }

    #[test]
    fn test_complex_display_nan_imag() {
        // 1.0+nan.0i - NaN displays with + prefix
        let c = Value::Complex(Box::new((Value::Real(1.0), Value::Real(f64::NAN))));
        let s = c.to_string();
        // NaN can display as +nan.0 or nan.0, just ensure no double +
        assert!(!s.contains("++"), "Should not have double + sign: {}", s);
        assert!(s.ends_with("i"), "Should end with i: {}", s);
    }

    #[test]
    fn test_complex_display_normal_positive_imag() {
        // Normal case: 3.0+4.0i
        let c = Value::Complex(Box::new((Value::Real(3.0), Value::Real(4.0))));
        assert_eq!(c.to_string(), "3.0+4.0i");
    }

    #[test]
    fn test_complex_display_normal_negative_imag() {
        // Normal case: 3.0-4.0i
        let c = Value::Complex(Box::new((Value::Real(3.0), Value::Real(-4.0))));
        assert_eq!(c.to_string(), "3.0-4.0i");
    }
}
