use num_bigint::BigInt;
use num_rational::BigRational;
use std::cell::RefCell;
use std::rc::Rc;

use crate::environment::Environment;
use crate::library::Library;

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
    Complex(f64, f64), // real, imaginary

    // Characters (Unicode support)
    Character(char),

    // Strings (mutable in Scheme, UTF-8 encoded)
    // Uses RefCell to allow mutation via string-set!
    // Note: Character indexing is O(n) which is explicitly allowed by R7RS
    String(Rc<RefCell<String>>),

    // Symbols
    Symbol(Rc<str>),

    // Pairs and lists
    Pair(Rc<(Value, Value)>),
    Null,

    // Vectors (mutable via vector-set!)
    // Uses RefCell to allow mutation through shared references
    Vector(Rc<RefCell<Vec<Value>>>),

    // Bytevectors
    Bytevector(Rc<Vec<u8>>),

    // Procedures
    Procedure(Procedure),

    // Ports (for I/O)
    InputPort,
    OutputPort,

    // Macros (for syntax-rules)
    // Note: Macro type will be defined in frontend, stored as opaque data here
    Macro {
        name: Rc<str>,
        // Opaque macro data - frontend will cast this appropriately
        data: Rc<dyn std::any::Any>,
    },

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
        params: Vec<String>,
        variadic: Option<String>, // For rest parameters
        body: Vec<Value>,
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
            Value::Complex(_, _) => "complex",
            Value::Character(_) => "character",
            Value::String(_) => "string",
            Value::Symbol(_) => "symbol",
            Value::Pair(_) | Value::Null => "list",
            Value::Vector(_) => "vector",
            Value::Bytevector(_) => "bytevector",
            Value::Procedure(_) => "procedure",
            Value::InputPort | Value::OutputPort => "port",
            Value::Macro { .. } => "macro",
            Value::Library(_) => "library",
            Value::Values(_) => "values",
            Value::Promise(_) => "promise",
            Value::Unspecified => "unspecified",
            Value::Eof => "eof-object",
        }
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
            Value::Complex(r, i) => {
                // Format complex numbers properly
                if *r == 0.0 && *i == 0.0 {
                    write!(f, "0")
                } else if *r == 0.0 {
                    // Pure imaginary
                    if *i == 1.0 {
                        write!(f, "+i")
                    } else if *i == -1.0 {
                        write!(f, "-i")
                    } else if *i < 0.0 {
                        write!(f, "{}i", i)
                    } else {
                        write!(f, "+{}i", i)
                    }
                } else if *i == 0.0 {
                    // Pure real
                    write!(f, "{}", r)
                } else if *i < 0.0 {
                    // Negative imaginary: use - instead of +-
                    if *i == -1.0 {
                        write!(f, "{}-i", r)
                    } else {
                        write!(f, "{}{}i", r, i)
                    }
                } else {
                    // Positive imaginary
                    if *i == 1.0 {
                        write!(f, "{}+i", r)
                    } else {
                        write!(f, "{}+{}i", r, i)
                    }
                }
            }
            Value::Character(c) => write!(f, "#\\{}", c),
            Value::String(s) => write!(f, "\"{}\"", s.borrow()),
            Value::Symbol(s) => write!(f, "{}", s),
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
            Value::Bytevector(bv) => write!(f, "#u8({:?})", bv),
            Value::Procedure(proc) => match proc {
                Procedure::Primitive { name, library, .. } => {
                    write!(f, "#<procedure:{}:{}>", library.join("."), name)
                }
                Procedure::Lambda { .. } => write!(f, "#<procedure>"),
                Procedure::Continuation => write!(f, "#<continuation>"),
            },
            Value::InputPort => write!(f, "#<input-port>"),
            Value::OutputPort => write!(f, "#<output-port>"),
            Value::Macro { name, .. } => write!(f, "#<macro:{}>", name),
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
            // Check if car is one of the special symbols
            if let Value::Symbol(sym) = &pair.0 {
                let prefix = match sym.as_ref() {
                    "quote" => "'",
                    "quasiquote" => "`",
                    "unquote" => ",",
                    "unquote-splicing" => ",@",
                    _ => return None,
                };

                // Check if cdr is a proper 1-element list: (keyword expr)
                if let Value::Pair(cdr_pair) = &pair.1
                    && matches!(cdr_pair.1, Value::Null)
                {
                    // It's (keyword expr), format as prefix + expr
                    return Some(format!("{}{}", prefix, cdr_pair.0));
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
                write!(f, "{}", p.0)?;
                match &p.1 {
                    Value::Null => Ok(()),
                    Value::Pair(_) => {
                        write!(f, " ")?;
                        p.1.fmt_list_contents(f)
                    }
                    other => write!(f, " . {}", other),
                }
            }
            _ => Ok(()),
        }
    }
}
