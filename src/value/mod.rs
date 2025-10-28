use num_bigint::BigInt;
use num_rational::BigRational;
use std::rc::Rc;

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

    // Strings (mutable in Scheme)
    String(Rc<String>),

    // Symbols
    Symbol(Rc<str>),

    // Pairs and lists
    Pair(Rc<(Value, Value)>),
    Null,

    // Vectors (mutable)
    Vector(Rc<Vec<Value>>),

    // Bytevectors
    Bytevector(Rc<Vec<u8>>),

    // Procedures
    Procedure(Procedure),

    // Ports (for I/O)
    InputPort,
    OutputPort,

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
        // Environment will be added when we implement closures
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
            Value::Real(r) => write!(f, "{}", r),
            Value::Complex(r, i) => write!(f, "{}+{}i", r, i),
            Value::Character(c) => write!(f, "#\\{}", c),
            Value::String(s) => write!(f, "\"{}\"", s),
            Value::Symbol(s) => write!(f, "{}", s),
            Value::Null => write!(f, "()"),
            Value::Pair(_) => {
                write!(f, "(")?;
                self.fmt_list_contents(f)?;
                write!(f, ")")
            }
            Value::Vector(v) => {
                write!(f, "#(")?;
                for (i, val) in v.iter().enumerate() {
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
