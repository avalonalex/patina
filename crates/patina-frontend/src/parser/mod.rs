use crate::lexer::{LexError, Lexer, Token};
use patina_runtime::Value;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{ToPrimitive, Zero};
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

    fn parse_expr(&mut self) -> Result<Value, ParseError> {
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
                let val = Value::Symbol(Rc::from(s.as_str()));
                self.advance()?;
                Ok(val)
            }
            Token::Quote => {
                self.advance()?;
                let quoted = self.parse_expr()?;
                Ok(self.make_list(vec![Value::Symbol(Rc::from("quote")), quoted]))
            }
            Token::Quasiquote => {
                self.advance()?;
                let quoted = self.parse_expr()?;
                Ok(self.make_list(vec![Value::Symbol(Rc::from("quasiquote")), quoted]))
            }
            Token::Unquote => {
                self.advance()?;
                let quoted = self.parse_expr()?;
                Ok(self.make_list(vec![Value::Symbol(Rc::from("unquote")), quoted]))
            }
            Token::UnquoteSplicing => {
                self.advance()?;
                let quoted = self.parse_expr()?;
                Ok(self.make_list(vec![Value::Symbol(Rc::from("unquote-splicing")), quoted]))
            }
            Token::LeftParen => self.parse_list(),
            Token::VectorOpen => self.parse_vector(),
            Token::BytevectorOpen => self.parse_bytevector(),
            Token::Eof => Err(ParseError::UnexpectedEof),
            token => Err(ParseError::UnexpectedToken(token.clone())),
        }
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
                let byte: u8 = s
                    .parse()
                    .map_err(|_| ParseError::InvalidSyntax("Invalid byte value".to_string()))?;
                bytes.push(byte);
                self.advance()?;
            } else {
                return Err(ParseError::InvalidSyntax(
                    "Bytevector must contain only bytes (0-255)".to_string(),
                ));
            }
        }

        self.advance()?; // consume )
        Ok(Value::Bytevector(Rc::new(bytes)))
    }

    fn parse_number(&self, s: &str) -> Result<Value, ParseError> {
        // Parse numbers following the R7RS numeric tower

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
                        return Ok(Value::Integer(n));
                    } else {
                        return Ok(Value::BigInteger(numer.clone()));
                    }
                }

                return Ok(Value::Rational(ratio));
            }
        }

        // Try i64 first (fast path for small integers)
        if let Ok(n) = s.parse::<i64>() {
            return Ok(Value::Integer(n));
        }

        // If it doesn't fit in i64, try BigInt (for large integers)
        if let Ok(n) = BigInt::from_str(s) {
            return Ok(Value::BigInteger(n));
        }

        // If it's not an integer, try floating point
        if let Ok(f) = s.parse::<f64>() {
            return Ok(Value::Real(f));
        }

        // Nothing worked - invalid number
        Err(ParseError::InvalidSyntax(format!("Invalid number: {}", s)))
    }

    fn parse_rectangular(&self, s: &str) -> Result<Value, ParseError> {
        // Remove the trailing 'i' or 'I'
        let s_no_i = &s[..s.len() - 1];

        // Handle special cases: +i, -i
        if s_no_i == "+" || s_no_i.is_empty() {
            return Ok(Value::Complex(0.0, 1.0));
        }
        if s_no_i == "-" {
            return Ok(Value::Complex(0.0, -1.0));
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
            let real_part = &s_no_i[..real_sep_pos];
            let imag_part = &s_no_i[real_sep_pos..];

            // Handle empty imaginary part (like "3+i" or "3-i")
            let imag_str = if imag_part == "+" || imag_part == "-" {
                format!("{}1", imag_part)
            } else {
                imag_part.to_string()
            };

            // Parse both parts as real numbers (could be int or float)
            let real_val = if real_part.is_empty() {
                0.0
            } else {
                self.parse_real_component(real_part)?
            };
            let imag_val = self.parse_real_component(&imag_str)?;

            Ok(Value::Complex(real_val, imag_val))
        } else {
            // No separator found - this is pure imaginary like "+5i" or "-3i"
            let imag_val = self.parse_real_component(s_no_i)?;
            Ok(Value::Complex(0.0, imag_val))
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
        let real = magnitude * angle.cos();
        let imag = magnitude * angle.sin();

        Ok(Value::Complex(real, imag))
    }

    /// Parse a component that should be a real number (int, bigint, rational, or float)
    fn parse_real_component(&self, s: &str) -> Result<f64, ParseError> {
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
        elements
            .into_iter()
            .rev()
            .fold(Value::Null, |acc, elem| Value::Pair(Rc::new((elem, acc))))
    }

    fn make_dotted_list(&self, elements: Vec<Value>, tail: Value) -> Value {
        elements
            .into_iter()
            .rev()
            .fold(tail, |acc, elem| Value::Pair(Rc::new((elem, acc))))
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
            let (car, cdr) = pair.as_ref();
            assert!(matches!(car, Value::Symbol(_))); // The '+'

            if let Value::Pair(pair2) = cdr {
                let (car2, _) = pair2.as_ref();
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
}
