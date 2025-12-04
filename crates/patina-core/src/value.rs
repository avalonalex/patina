use num_bigint::BigInt;
use num_rational::BigRational;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::compiled_macro::CompiledMacro;
use crate::core_expr::{CoreExpr, ScopedParam};
use crate::environment::Environment;
use crate::library::Library;
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

    // Strings (mutable in Scheme, UTF-8 encoded)
    // Uses RefCell to allow mutation via string-set!
    // Note: Character indexing is O(n) which is explicitly allowed by R7RS
    String(Rc<RefCell<String>>),

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
    Parameter {
        values: Rc<RefCell<Vec<Value>>>, // Stack of values (top = current value)
        converter: Option<Box<Value>>,   // Optional converter function
    },

    // Ports (for I/O)
    InputPort,
    OutputPort,

    // Macros (for syntax-rules)
    // Type-safe macro storage - no dyn Any needed
    Macro(Rc<CompiledMacro>),

    // Libraries (R7RS Section 5.6)
    // Represents a loaded library with its exports and environment
    Library(Rc<Library>),

    // Multiple values (R7RS Section 6.10)
    Values(Vec<Value>),

    // Promises (R7RS Section 6.10 - scheme lazy)
    // A promise is a delayed computation that can be forced to produce a value
    Promise(Rc<RefCell<PromiseState>>),

    // Special values
    Unspecified,
    Eof,
}

/// State of a promise for lazy evaluation
#[derive(Debug, Clone)]
pub enum PromiseState {
    /// Not yet evaluated - contains the thunk to evaluate
    Delayed(Value),
    /// Evaluated - contains the cached result
    Forced(Value),
}

/// Data for a hygienic identifier (boxed in Value::Identifier)
#[derive(Debug, Clone)]
pub struct IdentifierData {
    pub name: Rc<str>,
    /// Set of scopes for scope-sets hygiene
    /// Empty set = top-level identifier
    pub scopes: ScopeSet,
}

/// Lambda body representation - type-safe replacement for `dyn Any`
///
/// This enum allows storing lambda bodies in either:
/// - `Values`: Legacy format as syntax Values (requires desugaring on each call)
/// - `Core`: Optimized format as CoreExpr (preserves scope IDs for hygiene)
#[derive(Debug, Clone)]
pub enum LambdaBody {
    /// Legacy: body as syntax Values (requires desugaring on each call)
    Values(Vec<Value>),
    /// Optimized: body as CoreExpr (preserves scope IDs for hygiene)
    Core(Vec<CoreExpr>),
}

/// A clause in a case-lambda: (params, variadic, body, binding_scope)
#[derive(Debug, Clone)]
pub struct CaseLambdaClause {
    pub params: Vec<ScopedParam>,
    pub variadic: Option<ScopedParam>,
    pub body: LambdaBody,
    pub binding_scope: Option<ScopeId>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Procedure {
    /// Built-in primitive procedure
    Primitive {
        name: &'static str,
        arity: Arity,
        library: Vec<String>, // Library namespace, e.g., ["scheme", "base"]
    },

    /// User-defined procedure (lambda)
    Lambda {
        /// Fixed parameters, each with optional scopes for hygiene
        params: Vec<ScopedParam>,
        /// Optional variadic parameter (rest parameter)
        variadic: Option<ScopedParam>,
        /// Procedure body
        body: LambdaBody,
        /// Captured environment for closures
        env: Rc<Environment>,
        /// Optional scope for parameter bindings (for scope-based hygiene)
        /// If Some AND parameters have no scopes, parameters are bound with this scope.
        /// If None, parameters' own scopes are used (from macro expansion).
        /// This enables hygienic macro expansion: free variables with matching
        /// scopes can see these bindings, while others cannot.
        binding_scope: Option<ScopeId>,
    },

    /// Case-lambda procedure (dispatches on argument count)
    CaseLambda {
        clauses: Vec<CaseLambdaClause>,
        env: Rc<Environment>, // Captured environment for closures
    },

    /// Continuation (for call/cc)
    Continuation,
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
            Value::InputPort | Value::OutputPort => "port",
            Value::Macro(_) => "macro",
            Value::Library(_) => "library",
            Value::Values(_) => "values",
            Value::Promise(_) => "promise",
            Value::Unspecified => "unspecified",
            Value::Eof => "eof-object",
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

                // Helper to check if value is one
                fn is_one(v: &Value) -> bool {
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
                        Value::Real(r) => *r == 1.0,
                        _ => false,
                    }
                }

                // Helper to check if value is negative one
                fn is_neg_one(v: &Value) -> bool {
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
                        Value::Real(r) => *r == -1.0,
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
                    if is_one(imag_part) {
                        write!(f, "+i")
                    } else if is_neg_one(imag_part) {
                        write!(f, "-i")
                    } else if is_negative(imag_part) {
                        write!(f, "{}i", imag_part)
                    } else {
                        write!(f, "+{}i", imag_part)
                    }
                } else if imag_is_zero {
                    // Pure real - display with exactness info
                    write!(f, "{}", real_part)
                } else if is_negative(imag_part) {
                    // Negative imaginary: use - instead of +-
                    if is_neg_one(imag_part) {
                        write!(f, "{}-i", real_part)
                    } else {
                        write!(f, "{}{}i", real_part, imag_part)
                    }
                } else {
                    // Positive imaginary
                    if is_one(imag_part) {
                        write!(f, "{}+i", real_part)
                    } else {
                        write!(f, "{}+{}i", real_part, imag_part)
                    }
                }
            }
            Value::Character(c) => write!(f, "#\\{}", c),
            Value::String(s) => write!(f, "\"{}\"", s.borrow()),
            Value::Symbol(s) => {
                if Self::symbol_needs_vertical_bars(s) {
                    write!(f, "|{}|", s)
                } else {
                    write!(f, "{}", s)
                }
            }
            Value::Identifier(id) => {
                // Display identifiers just as their name
                // Marks and scopes are internal - they shouldn't appear in output
                // They are used for lookup, not display
                if Self::symbol_needs_vertical_bars(&id.name) {
                    write!(f, "|{}|", id.name)
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
                Procedure::Lambda { .. } => write!(f, "#<procedure>"),
                Procedure::CaseLambda { .. } => write!(f, "#<procedure:case-lambda>"),
                Procedure::Continuation => write!(f, "#<continuation>"),
            },
            Value::Parameter { .. } => write!(f, "#<parameter>"),
            Value::InputPort => write!(f, "#<input-port>"),
            Value::OutputPort => write!(f, "#<output-port>"),
            Value::Macro(compiled) => write!(f, "#<macro:{}>", compiled.name),
            Value::Library(lib) => write!(f, "{}", lib),
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
            Value::Unspecified => write!(f, "#<unspecified>"),
            Value::Eof => write!(f, "#<eof>"),
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
