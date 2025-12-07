use crate::lexer::{LexError, Lexer, Token};
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{ToPrimitive, Zero};
use patina_runtime::Value;
use std::cell::RefCell;
use std::rc::Rc;
use std::str::FromStr;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("Lexer error: {0}")]
    LexError(#[from] LexError),

    #[error("Unexpected EOF")]
    UnexpectedEof,

    #[error("Unexpected token: {0:?}")]
    UnexpectedToken(Token),

    #[error("Invalid syntax: {0}")]
    InvalidSyntax(String),
}

pub struct Parser {
    lexer: Lexer,
    current_token: Token,
}

impl Parser {
    pub fn new(input: &str) -> Result<Self, ParseError> {
        let mut lexer = Lexer::new(input);
        let current_token = lexer.next_token()?;
        Ok(Parser {
            lexer,
            current_token,
        })
    }

    fn advance(&mut self) -> Result<(), ParseError> {
        self.current_token = self.lexer.next_token()?;
        Ok(())
    }

    pub fn parse(&mut self) -> Result<Value, ParseError> {
        self.parse_expr()
    }

    /// Parse all expressions from the input until EOF.
    ///
    /// Returns a vector of all parsed expressions. Useful for parsing
    /// files that contain multiple top-level expressions (like included files).
    pub fn parse_all(&mut self) -> Result<Vec<Value>, ParseError> {
        let mut exprs = Vec::new();
        while self.current_token != Token::Eof {
            exprs.push(self.parse_expr()?);
        }
        Ok(exprs)
    }

    fn parse_expr(&mut self) -> Result<Value, ParseError> {
        // Handle datum comments: #; skips the next datum
        // Multiple #; in a row each skip one datum
        while self.current_token == Token::DatumComment {
            self.advance()?; // consume #;
            self.skip_datum()?; // skip the commented datum
        }

        match &self.current_token.clone() {
            Token::Boolean(b) => {
                let val = Value::Boolean(*b);
                self.advance()?;
                Ok(val)
            }
            Token::Number(s) => {
                let val = self.parse_number(s)?;
                self.advance()?;
                Ok(val)
            }
            Token::Character(c) => {
                let val = Value::Character(*c);
                self.advance()?;
                Ok(val)
            }
            Token::String(s) => {
                let val = Value::String(Rc::new(RefCell::new(s.clone())));
                self.advance()?;
                Ok(val)
            }
            Token::Identifier(s) => {
                let val = Value::symbol(s);
                self.advance()?;
                Ok(val)
            }
            Token::Quote => {
                self.advance()?;
                let quoted = self.parse_expr()?;
                Ok(self.make_list(vec![Value::symbol("quote"), quoted]))
            }
            Token::Quasiquote => {
                self.advance()?;
                let quoted = self.parse_expr()?;
                Ok(self.make_list(vec![Value::symbol("quasiquote"), quoted]))
            }
            Token::Unquote => {
                self.advance()?;
                let quoted = self.parse_expr()?;
                Ok(self.make_list(vec![Value::symbol("unquote"), quoted]))
            }
            Token::UnquoteSplicing => {
                self.advance()?;
                let quoted = self.parse_expr()?;
                Ok(self.make_list(vec![Value::symbol("unquote-splicing"), quoted]))
            }
            Token::LeftParen => self.parse_list(),
            Token::VectorOpen => self.parse_vector(),
            Token::BytevectorOpen => self.parse_bytevector(),
            Token::Eof => Err(ParseError::UnexpectedEof),
            token => Err(ParseError::UnexpectedToken(token.clone())),
        }
    }

    /// Skip a single datum (used for #; datum comments)
    /// This parses the datum but discards the result
    fn skip_datum(&mut self) -> Result<(), ParseError> {
        // Handle nested datum comments within the skipped datum
        while self.current_token == Token::DatumComment {
            self.advance()?;
            self.skip_datum()?;
        }

        match &self.current_token {
            Token::Boolean(_)
            | Token::Number(_)
            | Token::Character(_)
            | Token::String(_)
            | Token::Identifier(_) => {
                self.advance()?;
                Ok(())
            }
            Token::Quote | Token::Quasiquote | Token::Unquote | Token::UnquoteSplicing => {
                self.advance()?;
                self.skip_datum()
            }
            Token::LeftParen => self.skip_list(),
            Token::VectorOpen | Token::BytevectorOpen => self.skip_list(), // Same structure as list
            Token::Eof => Err(ParseError::UnexpectedEof),
            token => Err(ParseError::UnexpectedToken(token.clone())),
        }
    }

    /// Skip a list structure (used by skip_datum)
    fn skip_list(&mut self) -> Result<(), ParseError> {
        self.advance()?; // consume ( or #( or #u8(

        while self.current_token != Token::RightParen {
            if self.current_token == Token::Eof {
                return Err(ParseError::UnexpectedEof);
            }
            if self.current_token == Token::Dot {
                self.advance()?; // consume .
                self.skip_datum()?; // skip tail
                break;
            }
            self.skip_datum()?;
        }

        if self.current_token != Token::RightParen {
            return Err(ParseError::UnexpectedToken(self.current_token.clone()));
        }
        self.advance()?; // consume )
        Ok(())
    }

    fn parse_list(&mut self) -> Result<Value, ParseError> {
        self.advance()?; // consume (

        let mut elements = Vec::new();
        let mut dotted_tail = None;

        while self.current_token != Token::RightParen {
            if self.current_token == Token::Eof {
                return Err(ParseError::UnexpectedEof);
            }

            if self.current_token == Token::Dot {
                self.advance()?;
                dotted_tail = Some(self.parse_expr()?);
                // After the tail, skip any datum comments before )
                while self.current_token == Token::DatumComment {
                    self.advance()?;
                    self.skip_datum()?;
                }
                break;
            }

            elements.push(self.parse_expr()?);
        }

        if self.current_token != Token::RightParen {
            return Err(ParseError::UnexpectedToken(self.current_token.clone()));
        }
        self.advance()?; // consume )

        if let Some(tail) = dotted_tail {
            Ok(self.make_dotted_list(elements, tail))
        } else {
            Ok(self.make_list(elements))
        }
    }

    fn parse_vector(&mut self) -> Result<Value, ParseError> {
        self.advance()?; // consume #(

        let mut elements = Vec::new();

        while self.current_token != Token::RightParen {
            if self.current_token == Token::Eof {
                return Err(ParseError::UnexpectedEof);
            }
            elements.push(self.parse_expr()?);
        }

        self.advance()?; // consume )
        Ok(Value::Vector(Rc::new(RefCell::new(elements))))
    }

    fn parse_bytevector(&mut self) -> Result<Value, ParseError> {
        self.advance()?; // consume #u8(

        let mut bytes = Vec::new();

        while self.current_token != Token::RightParen {
            if self.current_token == Token::Eof {
                return Err(ParseError::UnexpectedEof);
            }

            if let Token::Number(s) = &self.current_token.clone() {
                // Parse the number (handles decimal, hex #x, binary #b, octal #o)
                let value = self.parse_number(s)?;

                // Extract integer value and validate it's a valid byte (0-255)
                let byte = match value {
                    Value::Integer(n) if (0..=255).contains(&n) => n as u8,
                    Value::Integer(n) => {
                        return Err(ParseError::InvalidSyntax(format!(
                            "Byte value out of range (0-255): {}",
                            n
                        )));
                    }
                    _ => {
                        return Err(ParseError::InvalidSyntax(
                            "Bytevector must contain only integer bytes (0-255)".to_string(),
                        ));
                    }
                };
                bytes.push(byte);
                self.advance()?;
            } else {
                return Err(ParseError::InvalidSyntax(
                    "Bytevector must contain only bytes (0-255)".to_string(),
                ));
            }
        }

        self.advance()?; // consume )
        Ok(Value::Bytevector(Rc::new(RefCell::new(bytes))))
    }

    fn parse_number(&self, s: &str) -> Result<Value, ParseError> {
        // Parse numbers following the R7RS numeric tower

        // Handle R7RS numeric prefixes: #e #i #b #o #d #x
        if s.starts_with('#') {
            return self.parse_number_with_prefix(s);
        }

        // Check for special R7RS floating-point literals (case-insensitive)
        match s.to_lowercase().as_str() {
            "+inf.0" => return Ok(Value::Real(f64::INFINITY)),
            "-inf.0" => return Ok(Value::Real(f64::NEG_INFINITY)),
            "+nan.0" | "-nan.0" => return Ok(Value::Real(f64::NAN)),
            _ => {}
        }

        // Check for polar notation: r@theta
        if s.contains('@') {
            return self.parse_polar(s);
        }

        // Check for rectangular notation: a+bi or a-bi (ends with 'i' or 'I')
        if s.ends_with('i') || s.ends_with('I') {
            return self.parse_rectangular(s);
        }

        // Check if it's a rational literal (contains '/')
        if s.contains('/') {
            // Try to parse as rational: numerator/denominator
            let parts: Vec<&str> = s.split('/').collect();
            if parts.len() == 2 {
                // Parse numerator and denominator
                let numer = BigInt::from_str(parts[0]).map_err(|_| {
                    ParseError::InvalidSyntax(format!("Invalid rational numerator: {}", parts[0]))
                })?;
                let denom = BigInt::from_str(parts[1]).map_err(|_| {
                    ParseError::InvalidSyntax(format!("Invalid rational denominator: {}", parts[1]))
                })?;

                if denom.is_zero() {
                    return Err(ParseError::InvalidSyntax(
                        "Rational denominator cannot be zero".to_string(),
                    ));
                }

                let ratio = BigRational::new(numer, denom);

                // Simplify to integer if denominator is 1
                if ratio.denom() == &BigInt::from(1) {
                    let numer = ratio.numer();
                    if let Some(n) = numer.to_i64() {
                        return Ok(Value::integer(n));
                    } else {
                        return Ok(Value::BigInteger(numer.clone()));
                    }
                }

                return Ok(Value::Rational(ratio));
            }
        }

        // Try i64 first (fast path for small integers)
        if let Ok(n) = s.parse::<i64>() {
            return Ok(Value::integer(n));
        }

        // If it doesn't fit in i64, try BigInt (for large integers)
        if let Ok(n) = BigInt::from_str(s) {
            return Ok(Value::BigInteger(n));
        }

        // If it's not an integer, try floating point
        // R7RS allows alternate exponent markers: s (short), f (single), d (double), l (long)
        // Per R7RS 7.1.1: "implementations may accept" these markers - they're optional.
        // The spec gives names but no specific precision requirements, only that s < f < d < l
        // and "the default precision has at least as much precision as double".
        // We normalize all to 'e' and parse as f64, which satisfies the spec since we only
        // have one inexact type and it's double precision.
        let normalized = Self::normalize_exponent_markers(s);
        if let Ok(f) = normalized.parse::<f64>() {
            return Ok(Value::Real(f));
        }

        // Nothing worked - invalid number
        Err(ParseError::InvalidSyntax(format!("Invalid number: {}", s)))
    }

    /// Normalize R7RS alternate exponent markers to standard 'e'
    ///
    /// R7RS 7.1.1 allows optional precision-indicating exponent markers:
    /// - s/S: short precision
    /// - f/F: single precision
    /// - d/D: double precision
    /// - l/L: long precision
    ///
    /// The spec defines no specific precision requirements, only ordering (s < f < d < l)
    /// and that default precision is at least double. Since we use f64 for all inexact
    /// numbers, normalizing to 'e' is spec-compliant.
    fn normalize_exponent_markers(s: &str) -> std::borrow::Cow<'_, str> {
        // Quick check: if no alternate markers, return as-is
        if !s
            .chars()
            .any(|c| matches!(c, 's' | 'S' | 'f' | 'F' | 'd' | 'D' | 'l' | 'L'))
        {
            return std::borrow::Cow::Borrowed(s);
        }

        // Replace alternate exponent markers with 'e'
        let mut result = String::with_capacity(s.len());
        for c in s.chars() {
            match c {
                's' | 'S' | 'f' | 'F' | 'd' | 'D' | 'l' | 'L' => result.push('e'),
                _ => result.push(c),
            }
        }
        std::borrow::Cow::Owned(result)
    }

    fn parse_number_with_prefix(&self, s: &str) -> Result<Value, ParseError> {
        // R7RS numeric prefixes: #e (exact), #i (inexact), #b (binary), #o (octal), #d (decimal), #x (hex)
        // These can be combined, e.g., #e#x10, #i#b1010

        let mut exactness: Option<bool> = None; // None = unspecified, Some(true) = exact, Some(false) = inexact
        let mut radix = 10u32; // Default radix is 10
        let mut rest = s;

        // Parse prefixes
        while rest.starts_with('#') {
            if rest.len() < 2 {
                return Err(ParseError::InvalidSyntax(format!("Invalid number: {}", s)));
            }

            let prefix_char = rest.chars().nth(1).unwrap().to_ascii_lowercase();

            match prefix_char {
                'e' => {
                    if exactness.is_some() {
                        return Err(ParseError::InvalidSyntax(format!(
                            "Duplicate exactness prefix: {}",
                            s
                        )));
                    }
                    exactness = Some(true);
                    rest = &rest[2..];
                }
                'i' => {
                    if exactness.is_some() {
                        return Err(ParseError::InvalidSyntax(format!(
                            "Duplicate exactness prefix: {}",
                            s
                        )));
                    }
                    exactness = Some(false);
                    rest = &rest[2..];
                }
                'b' => {
                    radix = 2;
                    rest = &rest[2..];
                }
                'o' => {
                    radix = 8;
                    rest = &rest[2..];
                }
                'd' => {
                    radix = 10;
                    rest = &rest[2..];
                }
                'x' => {
                    radix = 16;
                    rest = &rest[2..];
                }
                _ => {
                    return Err(ParseError::InvalidSyntax(format!(
                        "Unknown prefix: #{}",
                        prefix_char
                    )));
                }
            }
        }

        if rest.is_empty() {
            return Err(ParseError::InvalidSyntax(format!(
                "Number has prefixes but no digits: {}",
                s
            )));
        }

        // Parse the number based on radix
        let value = if radix == 10 {
            // For decimal, use the existing parse_number logic (handles floats, rationals, complex, etc.)
            self.parse_number(rest)?
        } else if rest.contains('/') {
            // Parse rational with given radix: e.g., #x1/10 -> 1/16
            let parts: Vec<&str> = rest.split('/').collect();
            if parts.len() != 2 {
                return Err(ParseError::InvalidSyntax(format!(
                    "Invalid rational: {}",
                    rest
                )));
            }

            let radix_name = match radix {
                2 => "binary",
                8 => "octal",
                16 => "hexadecimal",
                _ => "numeric",
            };

            let numer = BigInt::parse_bytes(parts[0].as_bytes(), radix).ok_or_else(|| {
                ParseError::InvalidSyntax(format!("Invalid {} numerator: {}", radix_name, parts[0]))
            })?;
            let denom = BigInt::parse_bytes(parts[1].as_bytes(), radix).ok_or_else(|| {
                ParseError::InvalidSyntax(format!(
                    "Invalid {} denominator: {}",
                    radix_name, parts[1]
                ))
            })?;

            if denom.is_zero() {
                return Err(ParseError::InvalidSyntax(
                    "Rational denominator cannot be zero".to_string(),
                ));
            }

            let ratio = BigRational::new(numer, denom);

            // Simplify to integer if denominator is 1
            if ratio.denom() == &BigInt::from(1) {
                let numer = ratio.numer();
                if let Some(n) = numer.to_i64() {
                    Value::integer(n)
                } else {
                    Value::BigInteger(numer.clone())
                }
            } else {
                Value::Rational(ratio)
            }
        } else {
            // For non-decimal radix, parse as integer only
            match i64::from_str_radix(rest, radix) {
                Ok(n) => Value::integer(n),
                Err(_) => {
                    // Try BigInt if it doesn't fit in i64
                    BigInt::parse_bytes(rest.as_bytes(), radix)
                        .map(Value::BigInteger)
                        .ok_or_else(|| {
                            let radix_name = match radix {
                                2 => "binary",
                                8 => "octal",
                                16 => "hexadecimal",
                                _ => "numeric",
                            };
                            ParseError::InvalidSyntax(format!(
                                "Invalid {} number: {}",
                                radix_name, rest
                            ))
                        })?
                }
            }
        };

        // Apply exactness conversion if specified
        match exactness {
            Some(true) => {
                // Force exact - if it's already exact, keep it; if inexact, convert to rational
                match value {
                    Value::Integer(_) | Value::BigInteger(_) | Value::Rational(_) => Ok(value),
                    Value::Real(f) => {
                        // Convert float to rational (this is approximate)
                        // For now, just return an error as exact conversion of arbitrary floats is complex
                        // In a full implementation, we'd need to convert the float to a rational
                        if f.fract() == 0.0 && f.is_finite() {
                            Ok(Value::integer(f as i64))
                        } else {
                            Err(ParseError::InvalidSyntax(format!(
                                "#e prefix on inexact number not yet fully supported: {}",
                                s
                            )))
                        }
                    }
                    _ => Ok(value),
                }
            }
            Some(false) => {
                // Force inexact - convert to float
                match value {
                    Value::Integer(n) => Ok(Value::Real(n as f64)),
                    Value::BigInteger(ref b) => {
                        // Convert BigInt to float (may lose precision)
                        if let Some(n) = b.to_i64() {
                            Ok(Value::Real(n as f64))
                        } else {
                            // Too large for precise float representation
                            Ok(Value::Real(b.to_f64().unwrap_or(f64::INFINITY)))
                        }
                    }
                    Value::Rational(ref r) => Ok(Value::Real(r.to_f64().unwrap_or(f64::NAN))),
                    Value::Real(_) => Ok(value), // Already inexact
                    _ => Ok(value),
                }
            }
            None => Ok(value), // No exactness specified, keep as-is
        }
    }

    /// Helper to create a Complex value from two real components
    fn make_complex(real: Value, imag: Value) -> Value {
        Value::Complex(Box::new((real, imag)))
    }

    fn parse_rectangular(&self, s: &str) -> Result<Value, ParseError> {
        // Remove the trailing 'i' or 'I'
        let s_no_i = &s[..s.len() - 1];

        // Handle special cases: +i, -i
        if s_no_i == "+" || s_no_i.is_empty() {
            // +i means 0+1i (both exact integers)
            return Ok(Self::make_complex(Value::Integer(0), Value::Integer(1)));
        }
        if s_no_i == "-" {
            // -i means 0-1i (both exact integers)
            return Ok(Self::make_complex(Value::Integer(0), Value::Integer(-1)));
        }

        // Find the position of + or - that separates real and imaginary parts
        // We need to skip the leading sign (if any)
        let start_pos = if s_no_i.starts_with('+') || s_no_i.starts_with('-') {
            1
        } else {
            0
        };

        // Find the separator (+ or -) after the start position
        if let Some(sep_pos) = s_no_i[start_pos..].find(['+', '-']) {
            let real_sep_pos = start_pos + sep_pos;
            let real_part_str = &s_no_i[..real_sep_pos];
            let imag_part_str = &s_no_i[real_sep_pos..];

            // Handle empty imaginary part (like "3+i" or "3-i")
            let imag_str = if imag_part_str == "+" || imag_part_str == "-" {
                format!("{}1", imag_part_str)
            } else {
                imag_part_str.to_string()
            };

            // Parse both parts as Values (preserving exactness)
            let real_val = if real_part_str.is_empty() {
                Value::Integer(0)
            } else {
                self.parse_real_component_as_value(real_part_str)?
            };
            let imag_val = self.parse_real_component_as_value(&imag_str)?;

            Ok(Self::make_complex(real_val, imag_val))
        } else {
            // No separator found - this is pure imaginary like "+5i" or "-3i"
            let imag_val = self.parse_real_component_as_value(s_no_i)?;
            Ok(Self::make_complex(Value::Integer(0), imag_val))
        }
    }

    fn parse_polar(&self, s: &str) -> Result<Value, ParseError> {
        let parts: Vec<&str> = s.split('@').collect();
        if parts.len() != 2 {
            return Err(ParseError::InvalidSyntax(format!(
                "Invalid polar notation: {}",
                s
            )));
        }

        let magnitude = self.parse_real_component(parts[0])?;
        let angle = self.parse_real_component(parts[1])?;

        // Convert polar to rectangular: r@θ = r*cos(θ) + r*sin(θ)i
        // Polar coordinates always produce inexact results
        let real = magnitude * angle.cos();
        let imag = magnitude * angle.sin();

        Ok(Self::make_complex(Value::Real(real), Value::Real(imag)))
    }

    /// Parse a component as a Value, preserving exactness
    /// This is key for R7RS complex number semantics: `1+0i` has exact 0 imaginary,
    /// while `1+0.0i` has inexact 0.0 imaginary.
    fn parse_real_component_as_value(&self, s: &str) -> Result<Value, ParseError> {
        // Check for special R7RS floating-point literals (case-insensitive, always inexact)
        match s.to_lowercase().as_str() {
            "+inf.0" => return Ok(Value::Real(f64::INFINITY)),
            "-inf.0" => return Ok(Value::Real(f64::NEG_INFINITY)),
            "+nan.0" | "-nan.0" => return Ok(Value::Real(f64::NAN)),
            _ => {}
        }

        // Check if it's a float (has decimal point or exponent) -> inexact
        let is_float = s.contains('.') || s.contains('e') || s.contains('E');

        // Handle sign prefix for parsing
        let (sign, num_str) = if let Some(stripped) = s.strip_prefix('+') {
            (1i64, stripped)
        } else if let Some(stripped) = s.strip_prefix('-') {
            (-1i64, stripped)
        } else {
            (1i64, s)
        };

        if is_float {
            // Parse as inexact (Real)
            let val = s
                .parse::<f64>()
                .map_err(|_| ParseError::InvalidSyntax(format!("Invalid real number: {}", s)))?;
            return Ok(Value::Real(val));
        }

        // Try rational (exact)
        if num_str.contains('/') {
            let parts: Vec<&str> = num_str.split('/').collect();
            if parts.len() == 2 {
                let numer = BigInt::from_str(parts[0]).map_err(|_| {
                    ParseError::InvalidSyntax(format!("Invalid numerator: {}", parts[0]))
                })?;
                let denom = BigInt::from_str(parts[1]).map_err(|_| {
                    ParseError::InvalidSyntax(format!("Invalid denominator: {}", parts[1]))
                })?;
                let ratio = if sign < 0 {
                    BigRational::new(-numer, denom)
                } else {
                    BigRational::new(numer, denom)
                };
                return Ok(Value::Rational(ratio));
            }
        }

        // Try i64 first (exact)
        if let Ok(n) = num_str.parse::<i64>() {
            return Ok(Value::Integer(sign * n));
        }

        // Try BigInt (exact)
        if let Ok(n) = BigInt::from_str(num_str) {
            let n = if sign < 0 { -n } else { n };
            return Ok(Value::BigInteger(n));
        }

        Err(ParseError::InvalidSyntax(format!(
            "Invalid real number: {}",
            s
        )))
    }

    /// Parse a component that should be a real number (int, bigint, rational, or float)
    /// Returns f64 (for polar coordinates where we always need inexact)
    fn parse_real_component(&self, s: &str) -> Result<f64, ParseError> {
        // Check for special R7RS floating-point literals (case-insensitive)
        match s.to_lowercase().as_str() {
            "+inf.0" => return Ok(f64::INFINITY),
            "-inf.0" => return Ok(f64::NEG_INFINITY),
            "+nan.0" | "-nan.0" => return Ok(f64::NAN),
            _ => {}
        }

        // Try i64 first
        if let Ok(n) = s.parse::<i64>() {
            return Ok(n as f64);
        }

        // Try BigInt
        if let Ok(n) = BigInt::from_str(s) {
            return Ok(n.to_f64().unwrap_or(f64::INFINITY));
        }

        // Try rational
        if s.contains('/') {
            let parts: Vec<&str> = s.split('/').collect();
            if parts.len() == 2 {
                let numer = BigInt::from_str(parts[0]).map_err(|_| {
                    ParseError::InvalidSyntax(format!("Invalid numerator: {}", parts[0]))
                })?;
                let denom = BigInt::from_str(parts[1]).map_err(|_| {
                    ParseError::InvalidSyntax(format!("Invalid denominator: {}", parts[1]))
                })?;
                let ratio = BigRational::new(numer, denom);
                return Ok(ratio.to_f64().unwrap_or(f64::NAN));
            }
        }

        // Try float
        s.parse::<f64>()
            .map_err(|_| ParseError::InvalidSyntax(format!("Invalid real number: {}", s)))
    }

    fn make_list(&self, elements: Vec<Value>) -> Value {
        elements.into_iter().rev().fold(Value::Null, |acc, elem| {
            Value::Pair(Rc::new(RefCell::new((elem, acc))))
        })
    }

    fn make_dotted_list(&self, elements: Vec<Value>, tail: Value) -> Value {
        elements.into_iter().rev().fold(tail, |acc, elem| {
            Value::Pair(Rc::new(RefCell::new((elem, acc))))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_atom() {
        let mut parser = Parser::new("42").unwrap();
        let result = parser.parse().unwrap();
        assert!(matches!(result, Value::Integer(42)));
    }

    #[test]
    fn test_parse_list() {
        let mut parser = Parser::new("(+ 1 2)").unwrap();
        let result = parser.parse().unwrap();
        // Should be a list with three elements
        assert!(matches!(result, Value::Pair(_)));
    }

    #[test]
    fn test_parse_small_integer() {
        // Small integers should parse as Integer (i64)
        let mut parser = Parser::new("42").unwrap();
        let result = parser.parse().unwrap();
        assert!(matches!(result, Value::Integer(42)));
    }

    #[test]
    fn test_parse_i64_max() {
        // i64::MAX should still parse as Integer
        let mut parser = Parser::new("9223372036854775807").unwrap();
        let result = parser.parse().unwrap();
        assert!(matches!(result, Value::Integer(9223372036854775807)));
    }

    #[test]
    fn test_parse_i64_min() {
        // i64::MIN should parse as Integer
        let mut parser = Parser::new("-9223372036854775808").unwrap();
        let result = parser.parse().unwrap();
        assert!(matches!(result, Value::Integer(-9223372036854775808)));
    }

    #[test]
    fn test_parse_beyond_i64_max() {
        // i64::MAX + 1 should parse as BigInteger
        let mut parser = Parser::new("9223372036854775808").unwrap();
        let result = parser.parse().unwrap();
        match result {
            Value::BigInteger(n) => {
                assert_eq!(n.to_string(), "9223372036854775808");
            }
            other => panic!("Expected BigInteger, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_large_bigint() {
        // Very large number should parse as BigInteger
        let mut parser = Parser::new("10000000000000000000").unwrap();
        let result = parser.parse().unwrap();
        match result {
            Value::BigInteger(n) => {
                assert_eq!(n.to_string(), "10000000000000000000");
            }
            other => panic!("Expected BigInteger, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_huge_bigint() {
        // Astronomically large number (2^100) should parse as BigInteger
        let mut parser = Parser::new("1267650600228229401496703205376").unwrap();
        let result = parser.parse().unwrap();
        match result {
            Value::BigInteger(n) => {
                assert_eq!(n.to_string(), "1267650600228229401496703205376");
            }
            other => panic!("Expected BigInteger, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_negative_beyond_i64_min() {
        // i64::MIN - 1 should parse as BigInteger
        let mut parser = Parser::new("-9223372036854775809").unwrap();
        let result = parser.parse().unwrap();
        match result {
            Value::BigInteger(n) => {
                assert_eq!(n.to_string(), "-9223372036854775809");
            }
            other => panic!("Expected BigInteger, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_bigint_in_expression() {
        // Large integers in expressions should parse as BigInteger
        let mut parser = Parser::new("(+ 10000000000000000000 1)").unwrap();
        let result = parser.parse().unwrap();

        // The result should be a list (pair chain)
        if let Value::Pair(pair) = result {
            let borrowed = pair.borrow();
            let (car, cdr) = (&borrowed.0, &borrowed.1);
            assert!(matches!(car, Value::Symbol(_))); // The '+'

            if let Value::Pair(pair2) = cdr {
                let borrowed2 = pair2.borrow();
                let (car2, _) = (&borrowed2.0, &borrowed2.1);
                // The first argument should be BigInteger
                match car2 {
                    Value::BigInteger(n) => {
                        assert_eq!(n.to_string(), "10000000000000000000");
                    }
                    other => panic!("Expected BigInteger, got {:?}", other),
                }
            } else {
                panic!("Expected pair for arguments");
            }
        } else {
            panic!("Expected list");
        }
    }

    #[test]
    fn test_parse_positive_infinity() {
        let mut parser = Parser::new("+inf.0").unwrap();
        let result = parser.parse().unwrap();
        match result {
            Value::Real(f) => {
                assert!(f.is_infinite() && f.is_sign_positive());
            }
            other => panic!("Expected positive infinity, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_negative_infinity() {
        let mut parser = Parser::new("-inf.0").unwrap();
        let result = parser.parse().unwrap();
        match result {
            Value::Real(f) => {
                assert!(f.is_infinite() && f.is_sign_negative());
            }
            other => panic!("Expected negative infinity, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_nan() {
        let mut parser = Parser::new("+nan.0").unwrap();
        let result = parser.parse().unwrap();
        match result {
            Value::Real(f) => {
                assert!(f.is_nan());
            }
            other => panic!("Expected NaN, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_special_floats_case_insensitive() {
        // Test case-insensitive parsing of inf.0 and nan.0
        let test_cases = [
            ("+INF.0", f64::INFINITY),
            ("+Inf.0", f64::INFINITY),
            ("+iNf.0", f64::INFINITY),
            ("-INF.0", f64::NEG_INFINITY),
            ("-Inf.0", f64::NEG_INFINITY),
        ];

        for (input, expected) in test_cases {
            let mut parser = Parser::new(input).unwrap();
            let result = parser.parse().unwrap();
            match result {
                Value::Real(f) => {
                    assert_eq!(
                        f.is_infinite(),
                        expected.is_infinite(),
                        "For input '{}': expected infinite",
                        input
                    );
                    assert_eq!(
                        f.is_sign_positive(),
                        expected.is_sign_positive(),
                        "For input '{}': sign mismatch",
                        input
                    );
                }
                other => panic!("For input '{}': expected Real, got {:?}", input, other),
            }
        }

        // Test NaN separately (can't use == for NaN)
        let nan_cases = ["+NAN.0", "+Nan.0", "+nAn.0", "-NAN.0", "-nan.0"];
        for input in nan_cases {
            let mut parser = Parser::new(input).unwrap();
            let result = parser.parse().unwrap();
            match result {
                Value::Real(f) => {
                    assert!(f.is_nan(), "For input '{}': expected NaN, got {}", input, f);
                }
                other => panic!(
                    "For input '{}': expected Real (NaN), got {:?}",
                    input, other
                ),
            }
        }
    }

    #[test]
    fn test_parse_infinity_in_expression() {
        let mut parser = Parser::new("(+ +inf.0 1)").unwrap();
        let result = parser.parse().unwrap();

        if let Value::Pair(pair) = result {
            let borrowed = pair.borrow();
            let (car, cdr) = (&borrowed.0, &borrowed.1);
            assert!(matches!(car, Value::Symbol(_))); // The '+'

            if let Value::Pair(pair2) = cdr {
                let borrowed2 = pair2.borrow();
                let (car2, _) = (&borrowed2.0, &borrowed2.1);
                match car2 {
                    Value::Real(f) => {
                        assert!(f.is_infinite() && f.is_sign_positive());
                    }
                    other => panic!("Expected positive infinity, got {:?}", other),
                }
            } else {
                panic!("Expected pair for arguments");
            }
        } else {
            panic!("Expected list");
        }
    }

    // ========== Datum Comment Tests ==========

    #[test]
    fn test_datum_comment_simple() {
        // #; abc def -> def
        let mut parser = Parser::new("#; abc def").unwrap();
        let result = parser.parse().unwrap();
        assert!(matches!(result, Value::Symbol(s) if &*s == "def"));
    }

    #[test]
    fn test_datum_comment_in_list() {
        // (#;sqrt abs -16) -> (abs -16)
        let mut parser = Parser::new("(#;sqrt abs -16)").unwrap();
        let result = parser.parse().unwrap();

        // Should be a list with two elements: abs and -16
        if let Value::Pair(pair) = result {
            let borrowed = pair.borrow();
            assert!(matches!(&borrowed.0, Value::Symbol(s) if &**s == "abs"));
        } else {
            panic!("Expected list");
        }
    }

    #[test]
    fn test_datum_comment_multiple() {
        // (a #; #;b c d) -> (a d)
        // First #; skips b, second #; skips c
        let mut parser = Parser::new("(a #; #;b c d)").unwrap();
        let result = parser.parse().unwrap();

        // Should be a list (a d)
        if let Value::Pair(pair) = result {
            let borrowed = pair.borrow();
            assert!(matches!(&borrowed.0, Value::Symbol(s) if &**s == "a"));

            if let Value::Pair(pair2) = &borrowed.1 {
                let borrowed2 = pair2.borrow();
                assert!(matches!(&borrowed2.0, Value::Symbol(s) if &**s == "d"));
                assert!(matches!(&borrowed2.1, Value::Null));
            } else {
                panic!("Expected second element");
            }
        } else {
            panic!("Expected list");
        }
    }

    #[test]
    fn test_datum_comment_nested_list() {
        // (a #;(b #;c d) e) -> (a e)
        let mut parser = Parser::new("(a #;(b #;c d) e)").unwrap();
        let result = parser.parse().unwrap();

        if let Value::Pair(pair) = result {
            let borrowed = pair.borrow();
            assert!(matches!(&borrowed.0, Value::Symbol(s) if &**s == "a"));

            if let Value::Pair(pair2) = &borrowed.1 {
                let borrowed2 = pair2.borrow();
                assert!(matches!(&borrowed2.0, Value::Symbol(s) if &**s == "e"));
                assert!(matches!(&borrowed2.1, Value::Null));
            } else {
                panic!("Expected second element");
            }
        } else {
            panic!("Expected list");
        }
    }

    #[test]
    fn test_datum_comment_dotted_list() {
        // (a . #;b c) -> (a . c)
        let mut parser = Parser::new("(a . #;b c)").unwrap();
        let result = parser.parse().unwrap();

        if let Value::Pair(pair) = result {
            let borrowed = pair.borrow();
            assert!(matches!(&borrowed.0, Value::Symbol(s) if &**s == "a"));
            assert!(matches!(&borrowed.1, Value::Symbol(s) if &**s == "c"));
        } else {
            panic!("Expected pair");
        }
    }

    #[test]
    fn test_datum_comment_before_tail() {
        // (a . b #;c) -> This should skip c after the dotted tail
        // Actually R7RS says: (a . b #;c) means the #;c is skipped after b
        // But b is already the tail... let's verify the chibi behavior
        // According to tests: this should return (a . b) with c skipped
        let mut parser = Parser::new("(a . b #;c)").unwrap();
        let result = parser.parse().unwrap();

        if let Value::Pair(pair) = result {
            let borrowed = pair.borrow();
            assert!(matches!(&borrowed.0, Value::Symbol(s) if &**s == "a"));
            assert!(matches!(&borrowed.1, Value::Symbol(s) if &**s == "b"));
        } else {
            panic!("Expected pair");
        }
    }

    #[test]
    fn test_datum_comment_with_line_comment() {
        // #; ; comment\n def ghi -> ghi
        // The #; comments out the first datum after it, which is 'def'
        // (the ; comment is just a line comment consumed as whitespace)
        let mut parser = Parser::new("#; ; comment\n def ghi").unwrap();
        let result = parser.parse().unwrap();
        assert!(matches!(result, Value::Symbol(s) if &*s == "ghi"));
    }

    #[test]
    fn test_datum_comment_vector() {
        // #;#(1 2 3) 42 -> 42
        let mut parser = Parser::new("#;#(1 2 3) 42").unwrap();
        let result = parser.parse().unwrap();
        assert!(matches!(result, Value::Integer(42)));
    }

    #[test]
    fn test_datum_comment_quoted() {
        // #;'foo bar -> bar
        let mut parser = Parser::new("#;'foo bar").unwrap();
        let result = parser.parse().unwrap();
        assert!(matches!(result, Value::Symbol(s) if &*s == "bar"));
    }

    // ========== Alternate Exponent Marker Tests ==========

    #[test]
    fn test_alternate_exponent_markers() {
        // R7RS allows s, f, d, l as exponent markers (all treated as f64)
        let test_cases = [
            ("1s2", 100.0),
            ("1S2", 100.0),
            ("1f2", 100.0),
            ("1F2", 100.0),
            ("1d2", 100.0),
            ("1D2", 100.0),
            ("1l2", 100.0),
            ("1L2", 100.0),
            ("1.5s1", 15.0),
            ("2.5f-1", 0.25),
        ];

        for (input, expected) in test_cases {
            let mut parser = Parser::new(input).unwrap();
            let result = parser.parse().unwrap();
            match result {
                Value::Real(f) => {
                    assert!(
                        (f - expected).abs() < 1e-10,
                        "For input '{}': expected {}, got {}",
                        input,
                        expected,
                        f
                    );
                }
                other => panic!("For input '{}': expected Real, got {:?}", input, other),
            }
        }
    }

    #[test]
    fn test_standard_exponent_still_works() {
        // Make sure we didn't break standard 'e' exponent
        let mut parser = Parser::new("1e2").unwrap();
        let result = parser.parse().unwrap();
        assert!(matches!(result, Value::Real(f) if (f - 100.0).abs() < 1e-10));
    }

    // ========== Non-Decimal Radix Rational Tests ==========

    #[test]
    fn test_hex_rational() {
        // #x1/10 = 1/16 in decimal
        let mut parser = Parser::new("#x1/10").unwrap();
        let result = parser.parse().unwrap();
        match result {
            Value::Rational(r) => {
                assert_eq!(r.numer().to_i64(), Some(1));
                assert_eq!(r.denom().to_i64(), Some(16));
            }
            other => panic!("Expected Rational, got {:?}", other),
        }
    }

    #[test]
    fn test_hex_rational_simplifies_to_integer() {
        // #x10/2 = 16/2 = 8
        let mut parser = Parser::new("#x10/2").unwrap();
        let result = parser.parse().unwrap();
        assert!(matches!(result, Value::Integer(8)));
    }

    #[test]
    fn test_hex_rational_simplifies() {
        // #x11/2 = 17/2
        let mut parser = Parser::new("#x11/2").unwrap();
        let result = parser.parse().unwrap();
        match result {
            Value::Rational(r) => {
                assert_eq!(r.numer().to_i64(), Some(17));
                assert_eq!(r.denom().to_i64(), Some(2));
            }
            other => panic!("Expected Rational, got {:?}", other),
        }
    }

    #[test]
    fn test_octal_rational() {
        // #o11/2 = 9/2 in decimal
        let mut parser = Parser::new("#o11/2").unwrap();
        let result = parser.parse().unwrap();
        match result {
            Value::Rational(r) => {
                assert_eq!(r.numer().to_i64(), Some(9));
                assert_eq!(r.denom().to_i64(), Some(2));
            }
            other => panic!("Expected Rational, got {:?}", other),
        }
    }

    #[test]
    fn test_binary_rational() {
        // #b11/10 = 3/2 in decimal
        let mut parser = Parser::new("#b11/10").unwrap();
        let result = parser.parse().unwrap();
        match result {
            Value::Rational(r) => {
                assert_eq!(r.numer().to_i64(), Some(3));
                assert_eq!(r.denom().to_i64(), Some(2));
            }
            other => panic!("Expected Rational, got {:?}", other),
        }
    }

    #[test]
    fn test_hex_rational_with_letters() {
        // #xa/b = 10/11
        let mut parser = Parser::new("#xa/b").unwrap();
        let result = parser.parse().unwrap();
        match result {
            Value::Rational(r) => {
                assert_eq!(r.numer().to_i64(), Some(10));
                assert_eq!(r.denom().to_i64(), Some(11));
            }
            other => panic!("Expected Rational, got {:?}", other),
        }
    }

    #[test]
    fn test_hex_rational_zero_denominator_error() {
        // #x1/0 should error
        let mut parser = Parser::new("#x1/0").unwrap();
        let result = parser.parse();
        assert!(result.is_err());
    }
}
