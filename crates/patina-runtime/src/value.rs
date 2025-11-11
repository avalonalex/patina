use num_bigint::BigInt;
use num_rational::BigRational;
use std::cell::RefCell;
use std::rc::Rc;

use crate::environment::Environment;

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

    // Multiple values (R7RS Section 6.10)
    Values(Vec<Value>),

    // Special values
    Unspecified,
    Eof,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Procedure {
    /// Built-in primitive procedure
    Primitive { name: &'static str, arity: Arity },

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
            Value::Values(_) => "values",
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
                write!(f, "(")?;
                self.fmt_list_contents(f)?;
                write!(f, ")")
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
            Value::Procedure(_) => write!(f, "#<procedure>"),
            Value::InputPort => write!(f, "#<input-port>"),
            Value::OutputPort => write!(f, "#<output-port>"),
            Value::Macro { name, .. } => write!(f, "#<macro:{}>", name),
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
            Value::Unspecified => write!(f, "#<unspecified>"),
            Value::Eof => write!(f, "#<eof>"),
        }
    }
}

impl Value {
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
