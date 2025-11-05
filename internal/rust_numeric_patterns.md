# Applying Chibi Numeric Patterns to Patina (Rust Implementation)

## 1. Type Representation

### Current Patina (from code)
```rust
pub enum Value {
    Integer(i64),
    BigInteger(BigInt),
    Rational(Ratio<BigInt>),
    Real(f64),
    Complex(Complex<f64>),
    // ...
}
```

### Suggested Improvements Based on Chibi

#### Option A: Keep Separate but Simplify
```rust
pub enum Value {
    // Exact integers
    Integer(i64),           // Tagged immediate equivalent
    BigInteger(BigInt),     // Exact but unlimited
    
    // Exact rationals
    Rational(Ratio),        // Must be normalized (see below)
    
    // Inexact reals
    Real(f64),              // Always inexact (no metadata needed)
    
    // Complex (can be mixed exact/inexact)
    Complex(Complex),       // See structure below
}

// Exactness is implicit in the variant!
// No need for separate exact/inexact flags

pub fn is_exact(v: &Value) -> bool {
    matches!(v, Value::Integer(_) | Value::BigInteger(_) | Value::Rational(_))
}

pub fn is_inexact(v: &Value) -> bool {
    !is_exact(v)
}
```

#### Option B: NaN-Boxing (Advanced, like Chibi)
```rust
// Store fixnums directly as tagged pointers (Rust unsafe code)
// This is complex and requires careful Rust handling
// Probably not worth it initially - good for later optimization
```

## 2. Ratio Type (with Normalization)

### Current Issue
Ratios might not be normalized, leading to:
- Multiple representations of same value (5/10 and 1/2)
- Equality comparisons failing
- Unnecessary computation

### Chibi's Solution: Lazy Normalization

```rust
pub struct Ratio {
    numerator: Box<BigInt>,
    denominator: Box<BigInt>,  // Always positive, > 1
}

impl Ratio {
    pub fn new(n: BigInt, d: BigInt) -> Self {
        let mut ratio = Ratio {
            numerator: Box::new(n),
            denominator: Box::new(d),
        };
        ratio.normalize();  // CRITICAL: normalize on creation
        ratio
    }
    
    fn normalize(&mut self) {
        // 1. Ensure denominator is positive
        if self.denominator < 0 {
            self.numerator = -self.numerator.clone();
            self.denominator = -self.denominator.clone();
        }
        
        // 2. Reduce to lowest terms
        let gcd = self.numerator.gcd(&self.denominator);
        self.numerator = self.numerator / gcd.clone();
        self.denominator = self.denominator / gcd;
        
        // 3. Check if denominator is 1 (redundant with type)
        // Note: Value enum should handle 0/1 return as Integer
    }
}

// Usage example
let ratio = Ratio::new(BigInt::from(6), BigInt::from(8));
// Result: Ratio { 3, 4 }  <- automatically normalized
```

## 3. Exactness Checking

### Implementation Pattern

```rust
// No metadata! Use type pattern matching.

pub trait Numeric: std::fmt::Display {
    fn is_exact(&self) -> bool;
    fn is_inexact(&self) -> bool { !self.is_exact() }
    fn is_integer(&self) -> bool;
    fn is_rational(&self) -> bool;
}

impl Numeric for Value {
    fn is_exact(&self) -> bool {
        matches!(self, 
            Value::Integer(_) 
            | Value::BigInteger(_) 
            | Value::Rational(_)
        )
    }
    
    fn is_integer(&self) -> bool {
        matches!(self, Value::Integer(_) | Value::BigInteger(_))
    }
    
    fn is_rational(&self) -> bool {
        self.is_integer() || matches!(self, Value::Rational(_))
    }
}

// Usage
match value {
    Value::Integer(i) => { /* exact integer */ },
    Value::BigInteger(b) => { /* exact integer */ },
    Value::Rational(r) => { /* exact rational */ },
    Value::Real(f) => { /* inexact real */ },
    Value::Complex(c) => { /* check components */ },
}
```

## 4. Overflow Detection & Promotion

### Current Issue
Integer arithmetic might overflow without promotion.

### Chibi-Inspired Solution

```rust
// In arithmetic operations
pub fn add(a: &Value, b: &Value) -> Result<Value, EvalError> {
    match (a, b) {
        // Fast path: fixnum + fixnum
        (Value::Integer(x), Value::Integer(y)) => {
            // Check bounds before assignment (like Chibi)
            match x.checked_add(*y) {
                Some(result) => Ok(Value::Integer(result)),
                None => {
                    // Overflow: promote to BigInt and retry
                    let x_big = BigInt::from(*x);
                    let y_big = BigInt::from(*y);
                    Ok(Value::BigInteger(x_big + y_big))
                }
            }
        },
        
        // Mixed types
        (Value::Integer(x), Value::Real(y)) => {
            Ok(Value::Real(*x as f64 + y))
        },
        (Value::Integer(x), Value::BigInteger(y)) => {
            let x_big = BigInt::from(*x);
            Ok(Value::BigInteger(x_big + y))
        },
        
        // Bigint + anything: delegate to BigInt
        (Value::BigInteger(x), Value::BigInteger(y)) => {
            Ok(Value::BigInteger(x + y))
        },
        
        // ... more combinations ...
        _ => Err(EvalError::type_error("number expected", a, b)),
    }
}

// Macro to reduce boilerplate
macro_rules! try_fix_op {
    ($x:expr, $y:expr, $op:expr) => {{
        match ($x as i128).checked_mul($y as i128) {
            Some(r) if r >= i64::MIN as i128 && r <= i64::MAX as i128 => {
                Ok(Value::Integer(r as i64))
            },
            _ => {
                // Promote and retry
                let x_big = BigInt::from($x);
                let y_big = BigInt::from($y);
                Ok(Value::BigInteger($op(x_big, y_big)))
            }
        }
    }};
}
```

## 5. Division Semantics

### Key Insight from Chibi
Different operators have different semantics!

```rust
pub fn divide(a: &Value, b: &Value) -> Result<Value, EvalError> {
    match (a, b) {
        // Exact division: return ratio (preserves exactness)
        (Value::Integer(x), Value::Integer(y)) if *y != 0 => {
            let ratio = Ratio::new(
                BigInt::from(*x),
                BigInt::from(*y),
            );
            // If denominator reduced to 1, return integer
            if ratio.denominator == 1 {
                Ok(Value::Integer(
                    ratio.numerator.to_i64().unwrap()
                ))
            } else {
                Ok(Value::Rational(ratio))
            }
        },
        
        // Flonum division: IEEE 754
        (Value::Real(x), Value::Real(y)) => {
            Ok(Value::Real(x / y))  // Returns inf or nan per IEEE 754
        },
        
        // Mixed: promote and retry
        (Value::Integer(x), Value::Real(y)) => {
            Ok(Value::Real(*x as f64 / y))
        },
        
        (Value::Integer(0), _) => {
            Err(EvalError::divide_by_zero())
        },
        _ => Err(EvalError::type_error("number", a, b)),
    }
}

// Separate procedures
pub fn quotient(a: &Value, b: &Value) -> Result<Value, EvalError> {
    // Integer division, toward zero
    match (a, b) {
        (Value::Integer(x), Value::Integer(y)) if *y != 0 => {
            Ok(Value::Integer(x / y))  // Truncate toward zero
        },
        _ => Err(EvalError::type_error("integer", a, b)),
    }
}

pub fn remainder(a: &Value, b: &Value) -> Result<Value, EvalError> {
    // Euclidean remainder
    match (a, b) {
        (Value::Integer(x), Value::Integer(y)) if *y != 0 => {
            Ok(Value::Integer(x % y))
        },
        _ => Err(EvalError::type_error("integer", a, b)),
    }
}
```

## 6. Complex Numbers

### Chibi Pattern: Pairs of Reals

```rust
pub struct Complex {
    pub real: Box<Value>,    // Can be Integer, BigInteger, Rational, or Real
    pub imag: Box<Value>,    // Same types as real
}

impl Complex {
    pub fn new(real: Value, imag: Value) -> Value {
        // Optimization: if imaginary is zero, return real
        if matches!(imag, Value::Integer(0) | Value::Real(0.0)) {
            real  // Simplify 3+0i to 3
        } else {
            Value::Complex(Complex {
                real: Box::new(real),
                imag: Box::new(imag),
            })
        }
    }
}

// Exactness of complex depends on components
pub fn is_exact_complex(real: &Value, imag: &Value) -> bool {
    real.is_exact() && imag.is_exact()
}

// Conversion: exact -> inexact
pub fn exact_to_inexact(v: &Value) -> Value {
    match v {
        Value::Integer(i) => Value::Real(*i as f64),
        Value::BigInteger(b) => Value::Real(b.to_f64()),
        Value::Rational(r) => Value::Real(
            r.numerator.to_f64() / r.denominator.to_f64()
        ),
        Value::Real(f) => Value::Real(*f),
        Value::Complex(c) => {
            // Recursive: convert both parts
            Value::Complex(Complex {
                real: Box::new(exact_to_inexact(&c.real)),
                imag: Box::new(exact_to_inexact(&c.imag)),
            })
        },
    }
}
```

## 7. Special IEEE 754 Values

### Parse and Handle Correctly

```rust
pub fn parse_number(s: &str) -> Result<Value, ParseError> {
    match s {
        "+inf.0" => Ok(Value::Real(f64::INFINITY)),
        "-inf.0" => Ok(Value::Real(f64::NEG_INFINITY)),
        "+nan.0" => Ok(Value::Real(f64::NAN)),
        "-0.0" => Ok(Value::Real(-0.0)),
        
        // Rational syntax with optional markers
        s if s.contains('/') && !s.contains('.') => {
            let parts: Vec<&str> = s.split('/').collect();
            let numerator = parts[0].parse::<BigInt>()?;
            let denominator = parts[1].parse::<BigInt>()?;
            Ok(Value::Rational(Ratio::new(numerator, denominator)))
        },
        
        // Exactness markers
        s if s.starts_with("#e") => {
            // Force exact: parse and ensure integer
            let num = parse_number(&s[2..])?;
            match num {
                Value::Real(f) if f.fract() == 0.0 => {
                    Ok(Value::Integer(f as i64))
                },
                Value::Real(f) => {
                    // Convert float to ratio
                    // This is complex; see next section
                    Err(ParseError::not_an_integer())
                },
                _ => Ok(num),
            }
        },
        
        s if s.starts_with("#i") => {
            // Force inexact: convert result to float
            let num = parse_number(&s[2..])?;
            Ok(exact_to_inexact(&num))
        },
        
        // Standard integer/float parsing
        _ => {
            if let Ok(i) = s.parse::<i64>() {
                Ok(Value::Integer(i))
            } else if let Ok(f) = s.parse::<f64>() {
                Ok(Value::Real(f))
            } else {
                Err(ParseError::invalid_number(s))
            }
        }
    }
}

pub fn check_special_floats(f: f64) -> bool {
    f.is_nan() || f.is_infinite()
}

pub fn negate_zero(f: f64) -> f64 {
    // Preserve negative zero semantics
    if f == 0.0 && f.is_sign_negative() {
        -0.0
    } else {
        -f
    }
}
```

## 8. Type Promotion Rules

### Decision Tree

```rust
pub fn promote_operands(a: &Value, b: &Value) -> (Value, Value) {
    // Promotion hierarchy: Real > Rational > BigInteger > Integer
    match (a, b) {
        // Same type: no promotion needed
        (Value::Integer(_), Value::Integer(_)) |
        (Value::Real(_), Value::Real(_)) => (a.clone(), b.clone()),
        
        // Promote left
        (Value::Integer(x), other) => {
            (Value::BigInteger(BigInt::from(*x)), other.clone())
        },
        (Value::BigInteger(_), Value::Real(_)) => {
            (exact_to_inexact(a), b.clone())
        },
        
        // Promote right
        (other, Value::Integer(y)) => {
            (a.clone(), Value::BigInteger(BigInt::from(*y)))
        },
        (Value::Real(_), Value::BigInteger(_)) => {
            (a.clone(), exact_to_inexact(b))
        },
        
        // Complex promotion
        (Value::Real(_), Value::Rational(_)) => {
            (a.clone(), exact_to_inexact(b))
        },
        
        // Others: return as-is and let operator decide
        _ => (a.clone(), b.clone()),
    }
}
```

## 9. Implementation Checklist

```rust
// In src/value/mod.rs or similar

// [ ] Add is_exact(), is_inexact() predicates
// [ ] Implement Ratio with auto-normalization
// [ ] Add overflow detection to Integer operations
// [ ] Implement automatic BigInteger promotion
// [ ] Handle IEEE 754 special values (+inf, -inf, +nan)
// [ ] Parse exactness markers (#e, #i)
// [ ] Parse rational literals (3/5)
// [ ] Implement exact->inexact conversion
// [ ] Implement inexact->exact conversion
// [ ] Division returns Rational (not float)
// [ ] Quotient/remainder are separate operations
// [ ] Complex number pairs with simplification
// [ ] Equality works with all types
// [ ] Printing uses correct number format
// [ ] Negative zero handled correctly
```

## 10. Testing Strategy

Based on Chibi's comprehensive approach:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_fixnum_overflow() {
        let max = Value::Integer(i64::MAX);
        let one = Value::Integer(1);
        let result = add(&max, &one).unwrap();
        assert!(matches!(result, Value::BigInteger(_)));
    }
    
    #[test]
    fn test_ratio_normalization() {
        let ratio = Ratio::new(
            BigInt::from(6),
            BigInt::from(8),
        );
        assert_eq!(ratio.numerator, BigInt::from(3));
        assert_eq!(ratio.denominator, BigInt::from(4));
    }
    
    #[test]
    fn test_exactness_preserved() {
        let x = Value::Integer(3);
        let y = Value::Integer(5);
        let result = divide(&x, &y).unwrap();
        assert!(result.is_exact());  // Returns Rational
        assert!(!matches!(result, Value::Real(_)));
    }
    
    #[test]
    fn test_complex_simplification() {
        let zero = Value::Integer(0);
        let three = Value::Integer(3);
        let result = Complex::new(three, zero);
        assert!(matches!(result, Value::Integer(3)));
    }
}
```

---

## Summary: Chibi Patterns for Patina

1. **No exactness metadata** - Let type determine it
2. **Lazy normalization** - Normalize ratios on creation
3. **Transparent promotion** - Overflow -> BigInteger automatically
4. **Type-based dispatch** - Use pattern matching extensively
5. **Separate division operations** - `/` (exact), `quotient`, `remainder`
6. **IEEE 754 compliance** - Support special values
7. **Complex = pair of reals** - Simplify if imag is zero
8. **Recursive conversion** - exact->inexact for all types including complex

