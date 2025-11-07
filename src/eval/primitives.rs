//! Primitive procedures implementation
//!
//! This module implements all built-in Scheme primitive procedures:
//!
//! ## Arithmetic Operations
//! - `+`, `-`, `*`, `/` - Basic arithmetic with overflow detection and inexact contagion
//! - `=`, `<`, `>`, `<=`, `>=` - Numeric comparisons
//! - `quotient`, `remainder`, `modulo` - Integer division operations
//! - `abs`, `max`, `min` - Numeric utilities
//!
//! ## List and Pair Operations
//! - `cons`, `car`, `cdr` - Pair construction and access
//! - `list` - List construction
//! - `length`, `append`, `reverse` - List manipulation
//! - `list-ref`, `list-tail` - List access
//! - `memq`, `memv`, `member` - List membership
//! - `assq`, `assv`, `assoc` - Association list lookup
//!
//! ## Type Predicates
//! - `null?`, `pair?`, `list?` - Type checking
//! - `number?`, `integer?`, `boolean?`, `string?`, `symbol?`
//! - `exact?`, `inexact?` - Exactness predicates
//!
//! ## Equality Operations
//! - `eq?` - Reference equality
//! - `eqv?` - Value equality
//! - `equal?` - Structural equality
//!
//! ## Higher-Order Functions
//! - `map` - Apply function to list elements
//! - `for-each` - Iterate over list elements for side effects
//!
//! ## Multiple Values
//! - `values` - Return multiple values
//! - `call-with-values` - Receive multiple values

use crate::env::Environment;
use crate::value::{Arity, Procedure, Value};
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{Signed, ToPrimitive, Zero};
use std::cell::RefCell;
use std::rc::Rc;

use super::error::EvalError;
use super::Evaluator;

/// Internal representation for numeric operations
/// Automatically promotes to the most general type needed
#[derive(Debug, Clone)]
enum NumericValue {
    Integer(i64),
    BigInteger(BigInt),
    Rational(BigRational),
    Real(f64),
    Complex(Box<(NumericValue, NumericValue)>), // (real, imaginary) - components must not be Complex
}

impl NumericValue {
    fn from_value(v: Value) -> Result<Self, EvalError> {
        match v {
            Value::Integer(n) => Ok(NumericValue::Integer(n)),
            Value::BigInteger(n) => Ok(NumericValue::BigInteger(n)),
            Value::Rational(r) => Ok(NumericValue::Rational(r)),
            Value::Real(f) => Ok(NumericValue::Real(f)),
            Value::Complex(real, imag) => {
                // Convert old (f64, f64) to new representation
                Ok(NumericValue::Complex(Box::new((
                    NumericValue::Real(real),
                    NumericValue::Real(imag),
                ))))
            }
            other => Err(EvalError::TypeError(format!(
                "Expected number, got {}",
                other.type_name()
            ))),
        }
    }

    /// Check if a numeric value is zero
    #[allow(dead_code)]
    fn is_zero(&self) -> bool {
        match self {
            NumericValue::Integer(n) => *n == 0,
            NumericValue::BigInteger(n) => n.is_zero(),
            NumericValue::Rational(r) => r.is_zero(),
            NumericValue::Real(f) => *f == 0.0,
            NumericValue::Complex(parts) => {
                let (r, i) = &**parts;
                r.is_zero() && i.is_zero()
            }
        }
    }

    /// Check if a numeric value is exact (not Real or Complex with Real components)
    #[allow(dead_code)]
    fn is_exact(&self) -> bool {
        match self {
            NumericValue::Integer(_) | NumericValue::BigInteger(_) | NumericValue::Rational(_) => {
                true
            }
            NumericValue::Real(_) => false,
            NumericValue::Complex(parts) => {
                let (r, i) = &**parts;
                r.is_exact() && i.is_exact()
            }
        }
    }

    /// Construct a complex number from real and imaginary parts
    /// Returns error if either component is itself complex (R7RS requires real components)
    #[allow(dead_code)]
    fn make_complex(real: NumericValue, imag: NumericValue) -> Result<NumericValue, EvalError> {
        // Ban nested complex numbers (not mathematically meaningful)
        if matches!(real, NumericValue::Complex(_)) {
            return Err(EvalError::TypeError(
                "make-rectangular: real part must be a real number, not complex".to_string(),
            ));
        }
        if matches!(imag, NumericValue::Complex(_)) {
            return Err(EvalError::TypeError(
                "make-rectangular: imaginary part must be a real number, not complex".to_string(),
            ));
        }

        // Optimization: if imaginary is zero, return just real part
        if imag.is_zero() {
            return Ok(real);
        }

        Ok(NumericValue::Complex(Box::new((real, imag))))
    }

    fn into_value(self, evaluator: &Evaluator) -> Value {
        match self {
            NumericValue::Integer(n) => Value::Integer(n),
            NumericValue::BigInteger(n) => Value::BigInteger(n),
            NumericValue::Rational(r) => evaluator.rational_to_value(r),
            NumericValue::Real(f) => Value::Real(f),
            NumericValue::Complex(parts) => {
                let (r, i) = *parts;
                // For now, convert to old (f64, f64) representation
                // TODO: Update Value::Complex to use NumericValue
                let real_f64 = match r {
                    NumericValue::Integer(n) => n as f64,
                    NumericValue::BigInteger(n) => n.to_f64().unwrap_or(f64::INFINITY),
                    NumericValue::Rational(r) => r.to_f64().unwrap_or(f64::NAN),
                    NumericValue::Real(f) => f,
                    NumericValue::Complex(_) => {
                        panic!("Complex component cannot be complex")
                    }
                };
                let imag_f64 = match i {
                    NumericValue::Integer(n) => n as f64,
                    NumericValue::BigInteger(n) => n.to_f64().unwrap_or(f64::INFINITY),
                    NumericValue::Rational(r) => r.to_f64().unwrap_or(f64::NAN),
                    NumericValue::Real(f) => f,
                    NumericValue::Complex(_) => {
                        panic!("Complex component cannot be complex")
                    }
                };
                Value::Complex(real_f64, imag_f64)
            }
        }
    }

    /// Add two numeric values, promoting types as needed
    fn add(self, other: Self) -> Self {
        use NumericValue::*;
        match (self, other) {
            // Integer + Integer (with overflow check)
            (Integer(a), Integer(b)) => match a.checked_add(b) {
                Some(sum) => Integer(sum),
                None => BigInteger(BigInt::from(a) + BigInt::from(b)),
            },
            // Promote to BigInteger
            (BigInteger(a), Integer(b)) | (Integer(b), BigInteger(a)) => {
                BigInteger(a + BigInt::from(b))
            }
            (BigInteger(a), BigInteger(b)) => BigInteger(a + b),
            // Promote to Rational
            (Rational(a), Rational(b)) => Rational(a + b),
            (Rational(a), Integer(b)) | (Integer(b), Rational(a)) => {
                Rational(a + BigRational::from(BigInt::from(b)))
            }
            (Rational(a), BigInteger(b)) | (BigInteger(b), Rational(a)) => {
                Rational(a + BigRational::from(b))
            }
            // Promote to Real (inexact contagion)
            (Real(a), Real(b)) => Real(a + b),
            (Real(a), Integer(b)) | (Integer(b), Real(a)) => Real(a + b as f64),
            (Real(a), BigInteger(b)) | (BigInteger(b), Real(a)) => {
                Real(a + b.to_f64().unwrap_or(f64::INFINITY))
            }
            (Real(a), Rational(r)) | (Rational(r), Real(a)) => {
                Real(a + r.to_f64().unwrap_or(f64::INFINITY))
            }
            // Complex number addition: (a+bi) + (c+di) = (a+c) + (b+d)i
            (Complex(parts1), Complex(parts2)) => {
                let (r1, i1) = *parts1;
                let (r2, i2) = *parts2;
                Complex(Box::new((r1.add(r2), i1.add(i2))))
            }
            // Promote real to complex (a + (c+di) = (a+c) + di)
            (Complex(parts), other) | (other, Complex(parts)) => {
                let (r, i) = *parts;
                Complex(Box::new((r.add(other), i)))
            }
        }
    }

    /// Subtract two numeric values
    fn subtract(self, other: Self) -> Self {
        use NumericValue::*;
        match (self, other) {
            (Integer(a), Integer(b)) => match a.checked_sub(b) {
                Some(diff) => Integer(diff),
                None => BigInteger(BigInt::from(a) - BigInt::from(b)),
            },
            (BigInteger(a), Integer(b)) => BigInteger(a - BigInt::from(b)),
            (Integer(a), BigInteger(b)) => BigInteger(BigInt::from(a) - b),
            (BigInteger(a), BigInteger(b)) => BigInteger(a - b),
            (Rational(a), Rational(b)) => Rational(a - b),
            (Rational(a), Integer(b)) => Rational(a - BigRational::from(BigInt::from(b))),
            (Integer(a), Rational(b)) => Rational(BigRational::from(BigInt::from(a)) - b),
            (Rational(a), BigInteger(b)) => Rational(a - BigRational::from(b)),
            (BigInteger(a), Rational(b)) => Rational(BigRational::from(a) - b),
            (Real(a), Real(b)) => Real(a - b),
            (Real(a), Integer(b)) => Real(a - b as f64),
            (Integer(a), Real(b)) => Real(a as f64 - b),
            (Real(a), BigInteger(b)) => Real(a - b.to_f64().unwrap_or(f64::INFINITY)),
            (BigInteger(a), Real(b)) => Real(a.to_f64().unwrap_or(f64::INFINITY) - b),
            (Real(a), Rational(r)) => Real(a - r.to_f64().unwrap_or(f64::INFINITY)),
            (Rational(r), Real(a)) => Real(r.to_f64().unwrap_or(f64::INFINITY) - a),
            // Complex subtraction: (a+bi) - (c+di) = (a-c) + (b-d)i
            (Complex(parts1), Complex(parts2)) => {
                let (r1, i1) = *parts1;
                let (r2, i2) = *parts2;
                Complex(Box::new((r1.subtract(r2), i1.subtract(i2))))
            }
            // Real - Complex
            (other, Complex(parts)) => {
                let (r, i) = *parts;
                Complex(Box::new((other.subtract(r), i.negate())))
            }
            // Complex - Real
            (Complex(parts), other) => {
                let (r, i) = *parts;
                Complex(Box::new((r.subtract(other), i)))
            }
        }
    }

    /// Multiply two numeric values
    fn multiply(self, other: Self) -> Self {
        use NumericValue::*;
        match (self, other) {
            (Integer(a), Integer(b)) => match a.checked_mul(b) {
                Some(product) => Integer(product),
                None => BigInteger(BigInt::from(a) * BigInt::from(b)),
            },
            (BigInteger(a), Integer(b)) | (Integer(b), BigInteger(a)) => {
                BigInteger(a * BigInt::from(b))
            }
            (BigInteger(a), BigInteger(b)) => BigInteger(a * b),
            (Rational(a), Rational(b)) => Rational(a * b),
            (Rational(a), Integer(b)) | (Integer(b), Rational(a)) => {
                Rational(a * BigRational::from(BigInt::from(b)))
            }
            (Rational(a), BigInteger(b)) | (BigInteger(b), Rational(a)) => {
                Rational(a * BigRational::from(b))
            }
            (Real(a), Real(b)) => Real(a * b),
            (Real(a), Integer(b)) | (Integer(b), Real(a)) => Real(a * b as f64),
            (Real(a), BigInteger(b)) | (BigInteger(b), Real(a)) => {
                Real(a * b.to_f64().unwrap_or(f64::INFINITY))
            }
            (Real(a), Rational(r)) | (Rational(r), Real(a)) => {
                Real(a * r.to_f64().unwrap_or(f64::INFINITY))
            }
            // Complex multiplication: (a+bi)(c+di) = (ac-bd) + (ad+bc)i
            (Complex(parts1), Complex(parts2)) => {
                let (r1, i1) = *parts1;
                let (r2, i2) = *parts2;
                // (a+bi)(c+di) = ac + adi + bci + bdi²
                //               = ac - bd + (ad + bc)i
                let ac = r1.clone().multiply(r2.clone());
                let bd = i1.clone().multiply(i2.clone());
                let ad = r1.multiply(i2.clone());
                let bc = i1.multiply(r2);
                Complex(Box::new((ac.subtract(bd), ad.add(bc))))
            }
            // Promote real to complex
            (Complex(parts), other) | (other, Complex(parts)) => {
                let (r, i) = *parts;
                Complex(Box::new((r.multiply(other.clone()), i.multiply(other))))
            }
        }
    }

    /// Negate a numeric value
    fn negate(self) -> Self {
        use NumericValue::*;
        match self {
            Integer(n) => match n.checked_neg() {
                Some(neg) => Integer(neg),
                None => BigInteger(-BigInt::from(n)),
            },
            BigInteger(n) => BigInteger(-n),
            Rational(r) => Rational(-r),
            Real(f) => Real(-f),
            Complex(parts) => {
                let (r, i) = *parts;
                Complex(Box::new((r.negate(), i.negate())))
            }
        }
    }
}

impl Evaluator {
    /// Dispatcher that routes primitive calls to specific implementations
    pub(super) fn apply_primitive(&self, name: &str, args: Vec<Value>) -> Result<Value, EvalError> {
        match name {
            "+" => self.primitive_add(args),
            "-" => self.primitive_subtract(args),
            "*" => self.primitive_multiply(args),
            "/" => self.primitive_divide(args),
            "=" => self.primitive_numeric_equal(args),
            "<" => self.primitive_less_than(args),
            ">" => self.primitive_greater_than(args),
            "<=" => self.primitive_less_equal(args),
            ">=" => self.primitive_greater_equal(args),
            "cons" => self.primitive_cons(args),
            "car" => self.primitive_car(args),
            "cdr" => self.primitive_cdr(args),
            "null?" => self.primitive_null_p(args),
            "pair?" => self.primitive_pair_p(args),
            "list" => Ok(self.list_from_vec(args)),
            "list?" => self.primitive_list_p(args),
            "length" => self.primitive_length(args),
            "append" => self.primitive_append(args),
            "reverse" => self.primitive_reverse(args),
            "list-ref" => self.primitive_list_ref(args),
            "list-tail" => self.primitive_list_tail(args),
            "memq" => self.primitive_memq(args),
            "memv" => self.primitive_memv(args),
            "member" => self.primitive_member(args),
            "assq" => self.primitive_assq(args),
            "assv" => self.primitive_assv(args),
            "assoc" => self.primitive_assoc(args),
            "map" => self.primitive_map(args),
            "for-each" => self.primitive_for_each(args),
            "number?" => self.primitive_number_p(args),
            "integer?" => self.primitive_integer_p(args),
            "boolean?" => self.primitive_boolean_p(args),
            "string?" => self.primitive_string_p(args),
            "symbol?" => self.primitive_symbol_p(args),
            "eq?" => self.primitive_eq(args),
            "eqv?" => self.primitive_eqv(args),
            "equal?" => self.primitive_equal(args),
            "values" => self.primitive_values(args),
            "call-with-values" => self.primitive_call_with_values(args),
            "quotient" => self.primitive_quotient(args),
            "remainder" => self.primitive_remainder(args),
            "modulo" => self.primitive_modulo(args),
            "abs" => self.primitive_abs(args),
            "max" => self.primitive_max(args),
            "min" => self.primitive_min(args),
            "exact?" => self.primitive_exact_p(args),
            "inexact?" => self.primitive_inexact_p(args),
            "boolean=?" => self.primitive_boolean_equal(args),
            "procedure?" => self.primitive_procedure_p(args),
            "char?" => self.primitive_char_p(args),
            // String operations
            "string-length" => self.primitive_string_length(args),
            "string-ref" => self.primitive_string_ref(args),
            "string-set!" => self.primitive_string_set(args),
            "make-string" => self.primitive_make_string(args),
            "string" => self.primitive_string(args),
            "string=?" => self.primitive_string_equal(args),
            "string<?" => self.primitive_string_less(args),
            "string>?" => self.primitive_string_greater(args),
            "string<=?" => self.primitive_string_less_equal(args),
            "string>=?" => self.primitive_string_greater_equal(args),
            "string-ci=?" => self.primitive_string_ci_equal(args),
            "string-ci<?" => self.primitive_string_ci_less(args),
            "string-ci>?" => self.primitive_string_ci_greater(args),
            "string-ci<=?" => self.primitive_string_ci_less_equal(args),
            "string-ci>=?" => self.primitive_string_ci_greater_equal(args),
            "string-append" => self.primitive_string_append(args),
            "substring" => self.primitive_substring(args),
            "string->list" => self.primitive_string_to_list(args),
            "list->string" => self.primitive_list_to_string(args),
            "string-copy" => self.primitive_string_copy(args),
            _ => Err(EvalError::InvalidSyntax(format!(
                "Unknown primitive: {}",
                name
            ))),
        }
    }

    // ========== Helper Functions for Primitives ==========

    /// Check that exactly `expected` arguments were provided
    fn check_arity_exact(
        &self,
        args: &[Value],
        expected: usize,
        fn_name: &str,
    ) -> Result<(), EvalError> {
        if args.len() != expected {
            return Err(EvalError::WrongArity {
                expected: format!("{} expects {} argument(s)", fn_name, expected),
                actual: args.len(),
            });
        }
        Ok(())
    }

    /// Check that at least `min` arguments were provided
    fn check_arity_min(&self, args: &[Value], min: usize, fn_name: &str) -> Result<(), EvalError> {
        if args.len() < min {
            return Err(EvalError::WrongArity {
                expected: format!("{} expects at least {} argument(s)", fn_name, min),
                actual: args.len(),
            });
        }
        Ok(())
    }

    /// Convert a Scheme list to a Vec, validating it's a proper list
    fn list_to_vec(&self, list: Value, fn_name: &str) -> Result<Vec<Value>, EvalError> {
        let mut items = Vec::new();
        let mut current = list;

        while let Value::Pair(pair) = current {
            items.push(pair.0.clone());
            current = pair.1.clone();
        }

        if !matches!(current, Value::Null) {
            return Err(EvalError::TypeError(format!(
                "{}: argument must be a proper list",
                fn_name
            )));
        }

        Ok(items)
    }

    /// Generic type predicate helper
    fn make_type_predicate<F>(&self, args: Vec<Value>, predicate: F) -> Result<Value, EvalError>
    where
        F: Fn(&Value) -> bool,
    {
        self.check_arity_exact(&args, 1, "type predicate")?;
        Ok(Value::Boolean(predicate(&args[0])))
    }

    /// Generic numeric comparison helper
    fn primitive_numeric_compare<F>(
        &self,
        args: Vec<Value>,
        op: F,
        op_name: &str,
    ) -> Result<Value, EvalError>
    where
        F: Fn(&BigRational, &BigRational) -> bool,
    {
        self.check_arity_min(&args, 2, op_name)?;

        for i in 0..args.len() - 1 {
            let (a, _) = self.value_to_comparable(&args[i])?;
            let (b, _) = self.value_to_comparable(&args[i + 1])?;
            if !op(&a, &b) {
                return Ok(Value::Boolean(false));
            }
        }
        Ok(Value::Boolean(true))
    }

    // ========== End Helper Functions ==========

    /// Install all primitives into the global environment
    pub(super) fn install_primitives(env: &Rc<Environment>) {
        let primitives = [
            ("+", Arity::Min(0)),
            ("-", Arity::Min(1)),
            ("*", Arity::Min(0)),
            ("/", Arity::Min(1)),
            ("=", Arity::Min(2)),
            ("<", Arity::Min(2)),
            (">", Arity::Min(2)),
            ("<=", Arity::Min(2)),
            (">=", Arity::Min(2)),
            ("cons", Arity::Exact(2)),
            ("car", Arity::Exact(1)),
            ("cdr", Arity::Exact(1)),
            ("null?", Arity::Exact(1)),
            ("pair?", Arity::Exact(1)),
            ("list", Arity::Min(0)),
            ("list?", Arity::Exact(1)),
            ("length", Arity::Exact(1)),
            ("append", Arity::Min(0)),
            ("reverse", Arity::Exact(1)),
            ("list-ref", Arity::Exact(2)),
            ("list-tail", Arity::Exact(2)),
            ("memq", Arity::Exact(2)),
            ("memv", Arity::Exact(2)),
            ("member", Arity::Range(2, 3)),
            ("assq", Arity::Exact(2)),
            ("assv", Arity::Exact(2)),
            ("assoc", Arity::Range(2, 3)),
            ("map", Arity::Min(2)),
            ("for-each", Arity::Min(2)),
            ("number?", Arity::Exact(1)),
            ("integer?", Arity::Exact(1)),
            ("boolean?", Arity::Exact(1)),
            ("string?", Arity::Exact(1)),
            ("symbol?", Arity::Exact(1)),
            ("eq?", Arity::Exact(2)),
            ("eqv?", Arity::Exact(2)),
            ("equal?", Arity::Exact(2)),
            ("values", Arity::Min(0)),
            ("call-with-values", Arity::Exact(2)),
            ("quotient", Arity::Exact(2)),
            ("remainder", Arity::Exact(2)),
            ("modulo", Arity::Exact(2)),
            ("abs", Arity::Exact(1)),
            ("max", Arity::Min(1)),
            ("min", Arity::Min(1)),
            ("exact?", Arity::Exact(1)),
            ("inexact?", Arity::Exact(1)),
            ("boolean=?", Arity::Min(2)),
            ("procedure?", Arity::Exact(1)),
            ("char?", Arity::Exact(1)),
            // String operations
            ("string-length", Arity::Exact(1)),
            ("string-ref", Arity::Exact(2)),
            ("string-set!", Arity::Exact(3)),
            ("make-string", Arity::Range(1, 2)),
            ("string", Arity::Min(0)),
            ("string=?", Arity::Min(2)),
            ("string<?", Arity::Min(2)),
            ("string>?", Arity::Min(2)),
            ("string<=?", Arity::Min(2)),
            ("string>=?", Arity::Min(2)),
            ("string-ci=?", Arity::Min(2)),
            ("string-ci<?", Arity::Min(2)),
            ("string-ci>?", Arity::Min(2)),
            ("string-ci<=?", Arity::Min(2)),
            ("string-ci>=?", Arity::Min(2)),
            ("string-append", Arity::Min(0)),
            ("substring", Arity::Exact(3)),
            ("string->list", Arity::Range(1, 3)),
            ("list->string", Arity::Exact(1)),
            ("string-copy", Arity::Range(1, 3)),
        ];

        for (name, arity) in primitives {
            env.define(
                name.to_string(),
                Value::Procedure(Procedure::Primitive { name, arity }),
            );
        }
    }

    // ===== Arithmetic Primitives =====

    pub(super) fn primitive_add(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        let mut result = NumericValue::Integer(0);

        for arg in args {
            let num = NumericValue::from_value(arg)
                .map_err(|_| EvalError::TypeError("+ expects numbers".to_string()))?;
            result = result.add(num);
        }

        Ok(result.into_value(self))
    }

    pub(super) fn primitive_subtract(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.is_empty() {
            return Err(EvalError::WrongArity {
                expected: "at least 1".to_string(),
                actual: 0,
            });
        }

        if args.len() == 1 {
            let num = NumericValue::from_value(args[0].clone())
                .map_err(|_| EvalError::TypeError("- expects numbers".to_string()))?;
            return Ok(num.negate().into_value(self));
        }

        let mut result = NumericValue::from_value(args[0].clone())
            .map_err(|_| EvalError::TypeError("- expects numbers".to_string()))?;

        for arg in &args[1..] {
            let num = NumericValue::from_value(arg.clone())
                .map_err(|_| EvalError::TypeError("- expects numbers".to_string()))?;
            result = result.subtract(num);
        }

        Ok(result.into_value(self))
    }

    pub(super) fn primitive_multiply(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        let mut result = NumericValue::Integer(1);

        for arg in args {
            let num = NumericValue::from_value(arg)
                .map_err(|_| EvalError::TypeError("* expects numbers".to_string()))?;
            result = result.multiply(num);
        }

        Ok(result.into_value(self))
    }

    pub(super) fn primitive_divide(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.is_empty() {
            return Err(EvalError::WrongArity {
                expected: "at least 1".to_string(),
                actual: 0,
            });
        }

        // Check if any argument is inexact (Real) - if so, use float arithmetic
        let has_inexact = args.iter().any(|v| matches!(v, Value::Real(_)));

        if has_inexact {
            // Inexact contagion: use float arithmetic
            // Division by inexact zero produces +inf.0 or -inf.0 (not an error)
            let to_f64 = |v: &Value| -> Result<f64, EvalError> {
                match v {
                    Value::Integer(n) => Ok(*n as f64),
                    Value::BigInteger(n) => Ok(n.to_f64().unwrap_or(f64::INFINITY)),
                    Value::Rational(r) => Ok(r.to_f64().unwrap_or(f64::INFINITY)),
                    Value::Real(n) => Ok(*n),
                    other => Err(EvalError::TypeError(format!(
                        "/ expects numbers, got {}",
                        other.type_name()
                    ))),
                }
            };

            if args.len() == 1 {
                let x = to_f64(&args[0])?;
                // Inexact division by zero is allowed, produces inf
                return Ok(Value::Real(1.0 / x));
            }

            let mut result = to_f64(&args[0])?;
            for arg in &args[1..] {
                let divisor = to_f64(arg)?;
                // Inexact division by zero is allowed, produces inf
                result /= divisor;
            }
            Ok(Value::Real(result))
        } else {
            // All exact: use rational arithmetic
            let to_rational = |v: &Value| -> Result<BigRational, EvalError> {
                match v {
                    Value::Integer(n) => Ok(BigRational::from(BigInt::from(*n))),
                    Value::BigInteger(n) => Ok(BigRational::from(n.clone())),
                    Value::Rational(r) => Ok(r.clone()),
                    other => Err(EvalError::TypeError(format!(
                        "/ expects numbers, got {}",
                        other.type_name()
                    ))),
                }
            };

            if args.len() == 1 {
                let x = to_rational(&args[0])?;
                if x.is_zero() {
                    return Err(EvalError::DivisionByZero);
                }
                let result = BigRational::from(BigInt::from(1)) / x;
                return Ok(self.rational_to_value(result));
            }

            let mut result = to_rational(&args[0])?;
            for arg in &args[1..] {
                let divisor = to_rational(arg)?;
                if divisor.is_zero() {
                    return Err(EvalError::DivisionByZero);
                }
                result /= divisor;
            }
            Ok(self.rational_to_value(result))
        }
    }

    /// Helper to convert BigRational to the appropriate Value type
    /// Simplifies to Integer or BigInteger if denominator is 1
    fn rational_to_value(&self, r: BigRational) -> Value {
        if r.denom() == &BigInt::from(1) {
            // Denominator is 1, simplify to integer
            let numer = r.numer();
            if let Some(n) = numer.to_i64() {
                Value::Integer(n)
            } else {
                Value::BigInteger(numer.clone())
            }
        } else {
            Value::Rational(r)
        }
    }

    // ===== Comparison Primitives =====

    /// Helper for exact numeric comparison
    /// Returns (rational_value, is_exact) for proper comparison
    fn value_to_comparable(&self, v: &Value) -> Result<(BigRational, bool), EvalError> {
        match v {
            Value::Integer(n) => Ok((BigRational::from(BigInt::from(*n)), true)),
            Value::BigInteger(n) => Ok((BigRational::from(n.clone()), true)),
            Value::Rational(r) => Ok((r.clone(), true)),
            Value::Real(f) => {
                // For inexact numbers, convert to rational approximation
                // This maintains correct comparison semantics
                use num_rational::Ratio;
                let ratio = Ratio::from_float(*f).ok_or_else(|| {
                    EvalError::TypeError("Cannot compare with NaN or infinity".to_string())
                })?;
                Ok((ratio, false))
            }
            other => Err(EvalError::TypeError(format!(
                "Expected number, got {}",
                other.type_name()
            ))),
        }
    }

    pub(super) fn primitive_numeric_equal(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.primitive_numeric_compare(args, |a, b| a == b, "=")
    }

    pub(super) fn primitive_less_than(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.primitive_numeric_compare(args, |a, b| a < b, "<")
    }

    pub(super) fn primitive_greater_than(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.primitive_numeric_compare(args, |a, b| a > b, ">")
    }

    pub(super) fn primitive_less_equal(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.primitive_numeric_compare(args, |a, b| a <= b, "<=")
    }

    pub(super) fn primitive_greater_equal(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.primitive_numeric_compare(args, |a, b| a >= b, ">=")
    }

    // ===== Pair/List Primitives =====

    pub(super) fn primitive_cons(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.check_arity_exact(&args, 2, "cons")?;
        Ok(Value::Pair(Rc::new((args[0].clone(), args[1].clone()))))
    }

    pub(super) fn primitive_car(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.check_arity_exact(&args, 1, "car")?;
        match &args[0] {
            Value::Pair(pair) => Ok(pair.0.clone()),
            _ => Err(EvalError::TypeError("car expects a pair".to_string())),
        }
    }

    pub(super) fn primitive_cdr(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.check_arity_exact(&args, 1, "cdr")?;
        match &args[0] {
            Value::Pair(pair) => Ok(pair.1.clone()),
            _ => Err(EvalError::TypeError("cdr expects a pair".to_string())),
        }
    }

    pub(super) fn primitive_list_p(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 1 {
            return Err(EvalError::WrongArity {
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        let mut slow = args[0].clone();
        let mut fast = args[0].clone();

        loop {
            match &fast {
                Value::Null => return Ok(Value::Boolean(true)),
                Value::Pair(pair1) => {
                    fast = pair1.1.clone();
                    match &fast {
                        Value::Null => return Ok(Value::Boolean(true)),
                        Value::Pair(pair2) => {
                            fast = pair2.1.clone();
                            if let Value::Pair(slow_pair) = &slow {
                                slow = slow_pair.1.clone();
                            }
                            if Self::values_equal(&slow, &fast)? {
                                return Ok(Value::Boolean(false));
                            }
                        }
                        _ => return Ok(Value::Boolean(false)),
                    }
                }
                _ => return Ok(Value::Boolean(false)),
            }
        }
    }

    pub(super) fn primitive_length(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.check_arity_exact(&args, 1, "length")?;
        let items = self.list_to_vec(args[0].clone(), "length")?;
        Ok(Value::Integer(items.len() as i64))
    }

    pub(super) fn primitive_append(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.is_empty() {
            return Ok(Value::Null);
        }

        if args.len() == 1 {
            return Ok(args[0].clone());
        }

        let mut result_items = Vec::new();
        for (i, list) in args.iter().enumerate() {
            if i == args.len() - 1 {
                if result_items.is_empty() {
                    return Ok(list.clone());
                }
                let mut result = list.clone();
                for item in result_items.into_iter().rev() {
                    result = Value::Pair(Rc::new((item, result)));
                }
                return Ok(result);
            }

            let mut current = list.clone();
            while let Value::Pair(pair) = current {
                result_items.push(pair.0.clone());
                current = pair.1.clone();
            }

            if !matches!(current, Value::Null) {
                return Err(EvalError::TypeError(format!(
                    "append: argument {} must be a proper list",
                    i + 1
                )));
            }
        }

        Ok(self.list_from_vec(result_items))
    }

    pub(super) fn primitive_reverse(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.check_arity_exact(&args, 1, "reverse")?;
        let mut items = self.list_to_vec(args[0].clone(), "reverse")?;
        items.reverse();
        Ok(self.list_from_vec(items))
    }

    pub(super) fn primitive_list_ref(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.check_arity_exact(&args, 2, "list-ref")?;

        let k = match &args[1] {
            Value::Integer(n) if *n >= 0 => *n as usize,
            Value::Integer(_) => {
                return Err(EvalError::TypeError(
                    "list-ref: index must be non-negative".to_string(),
                ))
            }
            _ => {
                return Err(EvalError::TypeError(
                    "list-ref: index must be an integer".to_string(),
                ))
            }
        };

        let items = self.list_to_vec(args[0].clone(), "list-ref")?;
        items
            .get(k)
            .cloned()
            .ok_or_else(|| EvalError::TypeError("list-ref: index out of bounds".to_string()))
    }

    pub(super) fn primitive_list_tail(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.check_arity_exact(&args, 2, "list-tail")?;

        let k = match &args[1] {
            Value::Integer(n) if *n >= 0 => *n as usize,
            Value::Integer(_) => {
                return Err(EvalError::TypeError(
                    "list-tail: index must be non-negative".to_string(),
                ))
            }
            _ => {
                return Err(EvalError::TypeError(
                    "list-tail: index must be an integer".to_string(),
                ))
            }
        };

        let items = self.list_to_vec(args[0].clone(), "list-tail")?;
        if k > items.len() {
            return Err(EvalError::TypeError(
                "list-tail: index out of bounds".to_string(),
            ));
        }
        Ok(self.list_from_vec(items[k..].to_vec()))
    }

    // ===== List Search Primitives =====

    pub(super) fn primitive_memq(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 2 {
            return Err(EvalError::WrongArity {
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        let obj = &args[0];
        let mut current = args[1].clone();

        while let Value::Pair(pair) = current {
            if self.values_eq(obj, &pair.0) {
                return Ok(Value::Pair(pair));
            }
            current = pair.1.clone();
        }

        Ok(Value::Boolean(false))
    }

    pub(super) fn primitive_memv(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 2 {
            return Err(EvalError::WrongArity {
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        let obj = &args[0];
        let mut current = args[1].clone();

        while let Value::Pair(pair) = current {
            if self.values_eqv(obj, &pair.0) {
                return Ok(Value::Pair(pair));
            }
            current = pair.1.clone();
        }

        Ok(Value::Boolean(false))
    }

    pub(super) fn primitive_member(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() < 2 || args.len() > 3 {
            return Err(EvalError::WrongArity {
                expected: "2 or 3".to_string(),
                actual: args.len(),
            });
        }

        let obj = &args[0];
        let mut current = args[1].clone();
        let compare_proc = args.get(2);

        while let Value::Pair(pair) = current.clone() {
            let matches = if let Some(proc) = compare_proc {
                let result = self.apply(proc.clone(), vec![obj.clone(), pair.0.clone()])?;
                result.is_truthy()
            } else {
                Self::values_equal(obj, &pair.0)?
            };

            if matches {
                return Ok(Value::Pair(pair));
            }
            current = pair.1.clone();
        }

        Ok(Value::Boolean(false))
    }

    pub(super) fn primitive_assq(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 2 {
            return Err(EvalError::WrongArity {
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        let obj = &args[0];
        let mut current = args[1].clone();

        while let Value::Pair(pair) = current {
            if let Value::Pair(entry) = &pair.0 {
                if self.values_eq(obj, &entry.0) {
                    return Ok(pair.0.clone());
                }
            }
            current = pair.1.clone();
        }

        Ok(Value::Boolean(false))
    }

    pub(super) fn primitive_assv(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 2 {
            return Err(EvalError::WrongArity {
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        let obj = &args[0];
        let mut current = args[1].clone();

        while let Value::Pair(pair) = current {
            if let Value::Pair(entry) = &pair.0 {
                if self.values_eqv(obj, &entry.0) {
                    return Ok(pair.0.clone());
                }
            }
            current = pair.1.clone();
        }

        Ok(Value::Boolean(false))
    }

    pub(super) fn primitive_assoc(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() < 2 || args.len() > 3 {
            return Err(EvalError::WrongArity {
                expected: "2 or 3".to_string(),
                actual: args.len(),
            });
        }

        let obj = &args[0];
        let mut current = args[1].clone();
        let compare_proc = args.get(2);

        while let Value::Pair(pair) = current.clone() {
            if let Value::Pair(entry) = &pair.0 {
                let matches = if let Some(proc) = compare_proc {
                    let result = self.apply(proc.clone(), vec![obj.clone(), entry.0.clone()])?;
                    result.is_truthy()
                } else {
                    Self::values_equal(obj, &entry.0)?
                };

                if matches {
                    return Ok(pair.0.clone());
                }
            }
            current = pair.1.clone();
        }

        Ok(Value::Boolean(false))
    }

    // ===== Type Predicates =====

    pub(super) fn primitive_null_p(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.make_type_predicate(args, |v| matches!(v, Value::Null))
    }

    pub(super) fn primitive_pair_p(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.make_type_predicate(args, |v| matches!(v, Value::Pair(_)))
    }

    pub(super) fn primitive_number_p(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.make_type_predicate(args, |v| {
            matches!(
                v,
                Value::Integer(_)
                    | Value::BigInteger(_)
                    | Value::Rational(_)
                    | Value::Real(_)
                    | Value::Complex(_, _)
            )
        })
    }

    pub(super) fn primitive_integer_p(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.make_type_predicate(args, |v| {
            matches!(v, Value::Integer(_) | Value::BigInteger(_))
        })
    }

    pub(super) fn primitive_boolean_p(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.make_type_predicate(args, |v| matches!(v, Value::Boolean(_)))
    }

    pub(super) fn primitive_string_p(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.make_type_predicate(args, |v| matches!(v, Value::String(_)))
    }

    pub(super) fn primitive_symbol_p(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.make_type_predicate(args, |v| matches!(v, Value::Symbol(_)))
    }

    pub(super) fn primitive_exact_p(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.make_type_predicate(args, |v| {
            matches!(
                v,
                Value::Integer(_) | Value::BigInteger(_) | Value::Rational(_)
            )
        })
    }

    pub(super) fn primitive_inexact_p(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.make_type_predicate(args, |v| matches!(v, Value::Real(_) | Value::Complex(_, _)))
    }

    pub(super) fn primitive_procedure_p(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.make_type_predicate(args, |v| matches!(v, Value::Procedure(_)))
    }

    pub(super) fn primitive_char_p(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.make_type_predicate(args, |v| matches!(v, Value::Character(_)))
    }

    pub(super) fn primitive_boolean_equal(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.check_arity_min(&args, 2, "boolean=?")?;

        // First, check that all arguments are booleans
        for arg in &args {
            if !matches!(arg, Value::Boolean(_)) {
                return Err(EvalError::TypeError(
                    "boolean=? expects all arguments to be booleans".to_string(),
                ));
            }
        }

        // Extract first boolean value
        let first_val = match &args[0] {
            Value::Boolean(b) => *b,
            _ => unreachable!(), // Already checked above
        };

        // Check that all remaining values are equal to the first
        for arg in &args[1..] {
            match arg {
                Value::Boolean(b) => {
                    if *b != first_val {
                        return Ok(Value::Boolean(false));
                    }
                }
                _ => unreachable!(), // Already checked above
            }
        }

        Ok(Value::Boolean(true))
    }

    // ===== Equality Primitives =====

    pub(super) fn values_eq(&self, a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::Boolean(a), Value::Boolean(b)) => a == b,
            (Value::Null, Value::Null) => true,
            (Value::Symbol(a), Value::Symbol(b)) => a.as_ref() == b.as_ref(),
            _ => false,
        }
    }

    pub(super) fn primitive_eq(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.check_arity_exact(&args, 2, "eq?")?;
        Ok(Value::Boolean(self.values_eq(&args[0], &args[1])))
    }

    pub(super) fn values_eqv(&self, a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::Boolean(a), Value::Boolean(b)) => a == b,
            (Value::Null, Value::Null) => true,
            (Value::Symbol(a), Value::Symbol(b)) => a.as_ref() == b.as_ref(),
            (Value::Integer(a), Value::Integer(b)) => a == b,
            (Value::Real(a), Value::Real(b)) => a == b,
            (Value::Character(a), Value::Character(b)) => a == b,
            _ => false,
        }
    }

    pub(super) fn primitive_eqv(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.check_arity_exact(&args, 2, "eqv?")?;
        Ok(Value::Boolean(self.values_eqv(&args[0], &args[1])))
    }

    pub(super) fn primitive_equal(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.check_arity_exact(&args, 2, "equal?")?;
        Ok(Value::Boolean(Self::values_equal(&args[0], &args[1])?))
    }

    pub(super) fn values_equal(a: &Value, b: &Value) -> Result<bool, EvalError> {
        Ok(match (a, b) {
            (Value::Boolean(x), Value::Boolean(y)) => x == y,
            (Value::Integer(x), Value::Integer(y)) => x == y,
            (Value::Real(x), Value::Real(y)) => x == y,
            (Value::Character(x), Value::Character(y)) => x == y,
            (Value::Null, Value::Null) => true,
            (Value::String(x), Value::String(y)) => *x.borrow() == *y.borrow(),
            (Value::Symbol(x), Value::Symbol(y)) => x.as_ref() == y.as_ref(),
            (Value::Pair(x), Value::Pair(y)) => {
                Self::values_equal(&x.0, &y.0)? && Self::values_equal(&x.1, &y.1)?
            }
            (Value::Vector(x), Value::Vector(y)) => {
                if x.len() != y.len() {
                    false
                } else {
                    let mut equal = true;
                    for (a, b) in x.iter().zip(y.iter()) {
                        if !Self::values_equal(a, b)? {
                            equal = false;
                            break;
                        }
                    }
                    equal
                }
            }
            _ => false,
        })
    }

    // ===== Higher-Order Functions =====

    pub(super) fn primitive_map(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.is_empty() {
            return Err(EvalError::WrongArity {
                expected: "at least 1".to_string(),
                actual: 0,
            });
        }

        let proc = &args[0];
        let lists = &args[1..];

        if lists.is_empty() {
            return Err(EvalError::WrongArity {
                expected: "at least 2".to_string(),
                actual: 1,
            });
        }

        let mut list_vecs: Vec<Vec<Value>> = Vec::new();
        for list in lists {
            let mut items = Vec::new();
            let mut current = list.clone();

            while let Value::Pair(pair) = current {
                items.push(pair.0.clone());
                current = pair.1.clone();
            }

            if !matches!(current, Value::Null) {
                return Err(EvalError::TypeError(
                    "map: argument must be a proper list".to_string(),
                ));
            }

            list_vecs.push(items);
        }

        let min_len = list_vecs.iter().map(|v| v.len()).min().unwrap_or(0);

        let mut results = Vec::new();
        for i in 0..min_len {
            let mut proc_args = Vec::new();
            for list_vec in &list_vecs {
                proc_args.push(list_vec[i].clone());
            }

            let result = self.apply(proc.clone(), proc_args)?;
            results.push(result);
        }

        Ok(self.list_from_vec(results))
    }

    pub(super) fn primitive_for_each(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.is_empty() {
            return Err(EvalError::WrongArity {
                expected: "at least 1".to_string(),
                actual: 0,
            });
        }

        let proc = &args[0];
        let lists = &args[1..];

        if lists.is_empty() {
            return Err(EvalError::WrongArity {
                expected: "at least 2".to_string(),
                actual: 1,
            });
        }

        let mut list_vecs: Vec<Vec<Value>> = Vec::new();
        for list in lists {
            let mut items = Vec::new();
            let mut current = list.clone();

            while let Value::Pair(pair) = current {
                items.push(pair.0.clone());
                current = pair.1.clone();
            }

            if !matches!(current, Value::Null) {
                return Err(EvalError::TypeError(
                    "for-each: argument must be a proper list".to_string(),
                ));
            }

            list_vecs.push(items);
        }

        let min_len = list_vecs.iter().map(|v| v.len()).min().unwrap_or(0);

        for i in 0..min_len {
            let mut proc_args = Vec::new();
            for list_vec in &list_vecs {
                proc_args.push(list_vec[i].clone());
            }

            self.apply(proc.clone(), proc_args)?;
        }

        Ok(Value::Unspecified)
    }

    // ===== Multiple Values Primitives =====

    pub(super) fn primitive_values(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        match args.len() {
            0 => Ok(Value::Unspecified),
            1 => Ok(args.into_iter().next().unwrap()),
            _ => Ok(Value::Values(args)),
        }
    }

    pub(super) fn primitive_call_with_values(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 2 {
            return Err(EvalError::WrongArity {
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        let producer = &args[0];
        let consumer = &args[1];

        let produced = self.apply(producer.clone(), vec![])?;

        let consumer_args = match produced {
            Value::Values(vals) => vals,
            other => vec![other],
        };

        self.apply(consumer.clone(), consumer_args)
    }

    // ===== Numeric Operations =====

    pub(super) fn primitive_quotient(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 2 {
            return Err(EvalError::WrongArity {
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        match (&args[0], &args[1]) {
            (Value::Integer(a), Value::Integer(b)) => {
                if *b == 0 {
                    return Err(EvalError::DivisionByZero);
                }
                Ok(Value::Integer(a / b))
            }
            (Value::BigInteger(a), Value::BigInteger(b)) => {
                if b.is_zero() {
                    return Err(EvalError::DivisionByZero);
                }
                Ok(Value::BigInteger(a / b))
            }
            (Value::Integer(a), Value::BigInteger(b)) => {
                if b.is_zero() {
                    return Err(EvalError::DivisionByZero);
                }
                Ok(Value::BigInteger(BigInt::from(*a) / b))
            }
            (Value::BigInteger(a), Value::Integer(b)) => {
                if *b == 0 {
                    return Err(EvalError::DivisionByZero);
                }
                Ok(Value::BigInteger(a / BigInt::from(*b)))
            }
            _ => Err(EvalError::TypeError(format!(
                "quotient requires integers, got {} and {}",
                args[0].type_name(),
                args[1].type_name()
            ))),
        }
    }

    pub(super) fn primitive_remainder(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 2 {
            return Err(EvalError::WrongArity {
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        match (&args[0], &args[1]) {
            (Value::Integer(a), Value::Integer(b)) => {
                if *b == 0 {
                    return Err(EvalError::DivisionByZero);
                }
                Ok(Value::Integer(a % b))
            }
            (Value::BigInteger(a), Value::BigInteger(b)) => {
                if b.is_zero() {
                    return Err(EvalError::DivisionByZero);
                }
                Ok(Value::BigInteger(a % b))
            }
            (Value::Integer(a), Value::BigInteger(b)) => {
                if b.is_zero() {
                    return Err(EvalError::DivisionByZero);
                }
                Ok(Value::BigInteger(BigInt::from(*a) % b))
            }
            (Value::BigInteger(a), Value::Integer(b)) => {
                if *b == 0 {
                    return Err(EvalError::DivisionByZero);
                }
                Ok(Value::BigInteger(a % BigInt::from(*b)))
            }
            _ => Err(EvalError::TypeError(format!(
                "remainder requires integers, got {} and {}",
                args[0].type_name(),
                args[1].type_name()
            ))),
        }
    }

    pub(super) fn primitive_modulo(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 2 {
            return Err(EvalError::WrongArity {
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        match (&args[0], &args[1]) {
            (Value::Integer(a), Value::Integer(b)) => {
                if *b == 0 {
                    return Err(EvalError::DivisionByZero);
                }
                Ok(Value::Integer(a.rem_euclid(*b)))
            }
            (Value::BigInteger(a), Value::BigInteger(b)) => {
                if b.is_zero() {
                    return Err(EvalError::DivisionByZero);
                }
                use num_traits::Euclid;
                Ok(Value::BigInteger(a.rem_euclid(b)))
            }
            (Value::Integer(a), Value::BigInteger(b)) => {
                if b.is_zero() {
                    return Err(EvalError::DivisionByZero);
                }
                use num_traits::Euclid;
                Ok(Value::BigInteger(BigInt::from(*a).rem_euclid(b)))
            }
            (Value::BigInteger(a), Value::Integer(b)) => {
                if *b == 0 {
                    return Err(EvalError::DivisionByZero);
                }
                use num_traits::Euclid;
                Ok(Value::BigInteger(a.rem_euclid(&BigInt::from(*b))))
            }
            _ => Err(EvalError::TypeError(format!(
                "modulo requires integers, got {} and {}",
                args[0].type_name(),
                args[1].type_name()
            ))),
        }
    }

    pub(super) fn primitive_abs(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.len() != 1 {
            return Err(EvalError::WrongArity {
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        match &args[0] {
            Value::Integer(n) => {
                if *n == i64::MIN {
                    Ok(Value::BigInteger(BigInt::from(*n).abs()))
                } else {
                    Ok(Value::Integer(n.abs()))
                }
            }
            Value::BigInteger(n) => Ok(Value::BigInteger(n.abs())),
            Value::Rational(r) => Ok(Value::Rational(r.abs())),
            Value::Real(r) => Ok(Value::Real(r.abs())),
            _ => Err(EvalError::TypeError(format!(
                "abs requires a number, got {}",
                args[0].type_name()
            ))),
        }
    }

    pub(super) fn primitive_max(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.is_empty() {
            return Err(EvalError::WrongArity {
                expected: "at least 1".to_string(),
                actual: 0,
            });
        }

        let mut max_val = match &args[0] {
            Value::Integer(n) => *n,
            _ => {
                return Err(EvalError::TypeError(format!(
                    "max requires integers, got {}",
                    args[0].type_name()
                )))
            }
        };

        for arg in &args[1..] {
            match arg {
                Value::Integer(n) => {
                    if *n > max_val {
                        max_val = *n;
                    }
                }
                _ => {
                    return Err(EvalError::TypeError(format!(
                        "max requires integers, got {}",
                        arg.type_name()
                    )))
                }
            }
        }

        Ok(Value::Integer(max_val))
    }

    pub(super) fn primitive_min(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.is_empty() {
            return Err(EvalError::WrongArity {
                expected: "at least 1".to_string(),
                actual: 0,
            });
        }

        let mut min_val = match &args[0] {
            Value::Integer(n) => *n,
            _ => {
                return Err(EvalError::TypeError(format!(
                    "min requires integers, got {}",
                    args[0].type_name()
                )))
            }
        };

        for arg in &args[1..] {
            match arg {
                Value::Integer(n) => {
                    if *n < min_val {
                        min_val = *n;
                    }
                }
                _ => {
                    return Err(EvalError::TypeError(format!(
                        "min requires integers, got {}",
                        arg.type_name()
                    )))
                }
            }
        }

        Ok(Value::Integer(min_val))
    }

    // ===== String Primitives =====

    pub(super) fn primitive_string_length(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.check_arity_exact(&args, 1, "string-length")?;

        match &args[0] {
            Value::String(s) => {
                // Count Unicode characters, not bytes
                let len = s.borrow().chars().count();
                Ok(Value::Integer(len as i64))
            }
            _ => Err(EvalError::TypeError(format!(
                "string-length expects a string, got {}",
                args[0].type_name()
            ))),
        }
    }

    pub(super) fn primitive_string_ref(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.check_arity_exact(&args, 2, "string-ref")?;

        let (s, k) = match (&args[0], &args[1]) {
            (Value::String(s), Value::Integer(k)) => (s, *k),
            (Value::String(_), _) => {
                return Err(EvalError::TypeError(
                    "string-ref index must be an integer".to_string(),
                ))
            }
            _ => {
                return Err(EvalError::TypeError(format!(
                    "string-ref expects string and integer, got {} and {}",
                    args[0].type_name(),
                    args[1].type_name()
                )))
            }
        };

        if k < 0 {
            return Err(EvalError::IndexOutOfBounds(format!(
                "string-ref index must be non-negative, got {}",
                k
            )));
        }

        // Get nth character (O(n) - R7RS compliant)
        let ch = s.borrow().chars().nth(k as usize).ok_or_else(|| {
            EvalError::IndexOutOfBounds(format!(
                "string-ref index {} out of bounds for string of length {}",
                k,
                s.borrow().chars().count()
            ))
        })?;

        Ok(Value::Character(ch))
    }

    pub(super) fn primitive_string_set(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.check_arity_exact(&args, 3, "string-set!")?;

        let (s, k, ch) = match (&args[0], &args[1], &args[2]) {
            (Value::String(s), Value::Integer(k), Value::Character(ch)) => (s, *k, *ch),
            (Value::String(_), Value::Integer(_), _) => {
                return Err(EvalError::TypeError(
                    "string-set! third argument must be a character".to_string(),
                ))
            }
            (Value::String(_), _, _) => {
                return Err(EvalError::TypeError(
                    "string-set! index must be an integer".to_string(),
                ))
            }
            _ => {
                return Err(EvalError::TypeError(
                    "string-set! expects string, integer, and character".to_string(),
                ))
            }
        };

        if k < 0 {
            return Err(EvalError::IndexOutOfBounds(format!(
                "string-set! index must be non-negative, got {}",
                k
            )));
        }

        // Convert to Vec<char>, mutate, convert back
        // This is O(n) but works correctly with UTF-8
        let mut chars: Vec<char> = s.borrow().chars().collect();

        if k as usize >= chars.len() {
            return Err(EvalError::IndexOutOfBounds(format!(
                "string-set! index {} out of bounds for string of length {}",
                k,
                chars.len()
            )));
        }

        chars[k as usize] = ch;
        *s.borrow_mut() = chars.into_iter().collect();

        Ok(Value::Unspecified)
    }

    pub(super) fn primitive_make_string(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.is_empty() || args.len() > 2 {
            return Err(EvalError::WrongArity {
                expected: "make-string expects 1 or 2 arguments".to_string(),
                actual: args.len(),
            });
        }

        let k = match &args[0] {
            Value::Integer(k) => *k,
            _ => {
                return Err(EvalError::TypeError(
                    "make-string length must be an integer".to_string(),
                ))
            }
        };

        if k < 0 {
            return Err(EvalError::TypeError(format!(
                "make-string length must be non-negative, got {}",
                k
            )));
        }

        let fill_char = if args.len() == 2 {
            match &args[1] {
                Value::Character(ch) => *ch,
                _ => {
                    return Err(EvalError::TypeError(
                        "make-string fill character must be a character".to_string(),
                    ))
                }
            }
        } else {
            ' ' // Default fill character
        };

        let s = std::iter::repeat(fill_char).take(k as usize).collect::<String>();
        Ok(Value::String(Rc::new(RefCell::new(s))))
    }

    pub(super) fn primitive_string(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        let mut chars = Vec::new();

        for arg in args {
            match arg {
                Value::Character(ch) => chars.push(ch),
                _ => {
                    return Err(EvalError::TypeError(format!(
                        "string expects characters, got {}",
                        arg.type_name()
                    )))
                }
            }
        }

        let s = chars.into_iter().collect::<String>();
        Ok(Value::String(Rc::new(RefCell::new(s))))
    }

    pub(super) fn primitive_string_equal(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.check_arity_min(&args, 2, "string=?")?;

        // Get first string for comparison
        let first = match &args[0] {
            Value::String(s) => s.borrow().clone(),
            _ => {
                return Err(EvalError::TypeError(
                    "string=? expects strings".to_string(),
                ))
            }
        };

        // Compare all remaining strings
        for arg in &args[1..] {
            match arg {
                Value::String(s) => {
                    if *s.borrow() != first {
                        return Ok(Value::Boolean(false));
                    }
                }
                _ => {
                    return Err(EvalError::TypeError(
                        "string=? expects strings".to_string(),
                    ))
                }
            }
        }

        Ok(Value::Boolean(true))
    }

    pub(super) fn primitive_string_less(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.string_compare(args, |a, b| a < b, "string<?")
    }

    pub(super) fn primitive_string_greater(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.string_compare(args, |a, b| a > b, "string>?")
    }

    pub(super) fn primitive_string_less_equal(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.string_compare(args, |a, b| a <= b, "string<=?")
    }

    pub(super) fn primitive_string_greater_equal(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.string_compare(args, |a, b| a >= b, "string>=?")
    }

    fn string_compare<F>(&self, args: Vec<Value>, cmp: F, fn_name: &str) -> Result<Value, EvalError>
    where
        F: Fn(&str, &str) -> bool,
    {
        self.check_arity_min(&args, 2, fn_name)?;

        for i in 0..args.len() - 1 {
            let (a, b) = match (&args[i], &args[i + 1]) {
                (Value::String(a), Value::String(b)) => (a.borrow().clone(), b.borrow().clone()),
                _ => {
                    return Err(EvalError::TypeError(format!(
                        "{} expects strings",
                        fn_name
                    )))
                }
            };

            if !cmp(&a, &b) {
                return Ok(Value::Boolean(false));
            }
        }

        Ok(Value::Boolean(true))
    }

    // Case-insensitive string comparisons
    pub(super) fn primitive_string_ci_equal(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.string_ci_compare(args, |a, b| a == b, "string-ci=?")
    }

    pub(super) fn primitive_string_ci_less(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.string_ci_compare(args, |a, b| a < b, "string-ci<?")
    }

    pub(super) fn primitive_string_ci_greater(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.string_ci_compare(args, |a, b| a > b, "string-ci>?")
    }

    pub(super) fn primitive_string_ci_less_equal(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.string_ci_compare(args, |a, b| a <= b, "string-ci<=?")
    }

    pub(super) fn primitive_string_ci_greater_equal(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.string_ci_compare(args, |a, b| a >= b, "string-ci>=?")
    }

    fn string_ci_compare<F>(&self, args: Vec<Value>, cmp: F, fn_name: &str) -> Result<Value, EvalError>
    where
        F: Fn(&str, &str) -> bool,
    {
        self.check_arity_min(&args, 2, fn_name)?;

        for i in 0..args.len() - 1 {
            let (a, b) = match (&args[i], &args[i + 1]) {
                (Value::String(a), Value::String(b)) => (
                    a.borrow().to_lowercase(),
                    b.borrow().to_lowercase(),
                ),
                _ => {
                    return Err(EvalError::TypeError(format!(
                        "{} expects strings",
                        fn_name
                    )))
                }
            };

            if !cmp(&a, &b) {
                return Ok(Value::Boolean(false));
            }
        }

        Ok(Value::Boolean(true))
    }

    pub(super) fn primitive_string_append(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        let mut result = String::new();

        for arg in args {
            match arg {
                Value::String(s) => result.push_str(&s.borrow()),
                _ => {
                    return Err(EvalError::TypeError(format!(
                        "string-append expects strings, got {}",
                        arg.type_name()
                    )))
                }
            }
        }

        Ok(Value::String(Rc::new(RefCell::new(result))))
    }

    pub(super) fn primitive_substring(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.check_arity_exact(&args, 3, "substring")?;

        let (s, start, end) = match (&args[0], &args[1], &args[2]) {
            (Value::String(s), Value::Integer(start), Value::Integer(end)) => (s, *start, *end),
            (Value::String(_), _, _) => {
                return Err(EvalError::TypeError(
                    "substring indices must be integers".to_string(),
                ))
            }
            _ => {
                return Err(EvalError::TypeError(
                    "substring expects string and two integers".to_string(),
                ))
            }
        };

        if start < 0 || end < 0 {
            return Err(EvalError::IndexOutOfBounds(
                "substring indices must be non-negative".to_string(),
            ));
        }

        if start > end {
            return Err(EvalError::IndexOutOfBounds(
                "substring start index must be <= end index".to_string(),
            ));
        }

        let chars: Vec<char> = s.borrow().chars().collect();

        if end as usize > chars.len() {
            return Err(EvalError::IndexOutOfBounds(format!(
                "substring end index {} out of bounds for string of length {}",
                end,
                chars.len()
            )));
        }

        let substr: String = chars[start as usize..end as usize].iter().collect();
        Ok(Value::String(Rc::new(RefCell::new(substr))))
    }

    pub(super) fn primitive_string_to_list(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.is_empty() || args.len() > 3 {
            return Err(EvalError::WrongArity {
                expected: "string->list expects 1 to 3 arguments".to_string(),
                actual: args.len(),
            });
        }

        let s = match &args[0] {
            Value::String(s) => s.borrow().clone(),
            _ => {
                return Err(EvalError::TypeError(
                    "string->list expects a string".to_string(),
                ))
            }
        };

        let chars: Vec<char> = s.chars().collect();
        let start = if args.len() >= 2 {
            match &args[1] {
                Value::Integer(n) => *n as usize,
                _ => {
                    return Err(EvalError::TypeError(
                        "string->list start index must be an integer".to_string(),
                    ))
                }
            }
        } else {
            0
        };

        let end = if args.len() >= 3 {
            match &args[2] {
                Value::Integer(n) => *n as usize,
                _ => {
                    return Err(EvalError::TypeError(
                        "string->list end index must be an integer".to_string(),
                    ))
                }
            }
        } else {
            chars.len()
        };

        if start > end || end > chars.len() {
            return Err(EvalError::IndexOutOfBounds(
                "string->list indices out of bounds".to_string(),
            ));
        }

        let char_values: Vec<Value> = chars[start..end]
            .iter()
            .map(|&ch| Value::Character(ch))
            .collect();

        Ok(self.list_from_vec(char_values))
    }

    pub(super) fn primitive_list_to_string(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        self.check_arity_exact(&args, 1, "list->string")?;

        let chars = self.list_to_vec(args[0].clone(), "list->string")?;
        let mut result = String::new();

        for ch_val in chars {
            match ch_val {
                Value::Character(ch) => result.push(ch),
                _ => {
                    return Err(EvalError::TypeError(
                        "list->string expects a list of characters".to_string(),
                    ))
                }
            }
        }

        Ok(Value::String(Rc::new(RefCell::new(result))))
    }

    pub(super) fn primitive_string_copy(&self, args: Vec<Value>) -> Result<Value, EvalError> {
        if args.is_empty() || args.len() > 3 {
            return Err(EvalError::WrongArity {
                expected: "string-copy expects 1 to 3 arguments".to_string(),
                actual: args.len(),
            });
        }

        let s = match &args[0] {
            Value::String(s) => s.borrow().clone(),
            _ => {
                return Err(EvalError::TypeError(
                    "string-copy expects a string".to_string(),
                ))
            }
        };

        if args.len() == 1 {
            // Simple copy
            return Ok(Value::String(Rc::new(RefCell::new(s))));
        }

        // Copy substring
        let chars: Vec<char> = s.chars().collect();
        let start = match &args[1] {
            Value::Integer(n) => *n as usize,
            _ => {
                return Err(EvalError::TypeError(
                    "string-copy start index must be an integer".to_string(),
                ))
            }
        };

        let end = if args.len() >= 3 {
            match &args[2] {
                Value::Integer(n) => *n as usize,
                _ => {
                    return Err(EvalError::TypeError(
                        "string-copy end index must be an integer".to_string(),
                    ))
                }
            }
        } else {
            chars.len()
        };

        if start > end || end > chars.len() {
            return Err(EvalError::IndexOutOfBounds(
                "string-copy indices out of bounds".to_string(),
            ));
        }

        let substr: String = chars[start..end].iter().collect();
        Ok(Value::String(Rc::new(RefCell::new(substr))))
    }
}
