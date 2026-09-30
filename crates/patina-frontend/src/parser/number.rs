//! One grammar scan for program literals, `read` and `string->number` (#369).
//!
//! Scan borrowed parts before allocating Scheme numbers. Conversion routines
//! cannot broaden the language: they see only validated digits. The grammar is
//! R7RS 7.1.1, with the existing SRFI 169 digit separators (#364) and optional
//! s/f/d/l exponent markers. In particular, a denominator has no sign, radix
//! prefixes cannot repeat, and exponent signs are not complex separators.

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{ToPrimitive, Zero};
use patina_core::{Heap, SharedHeap, TaggedValue};
use std::borrow::Cow;

#[derive(Debug)]
pub(super) struct NumberError {
    /// Byte offset in the original spelling, never in normalized digits.
    pub offset: usize,
    reason: &'static str,
}

impl NumberError {
    fn new(offset: usize, reason: &'static str) -> Self {
        Self { offset, reason }
    }

    pub fn describe(&self, input: &str) -> String {
        let position = input[..self.offset].chars().count() + 1;
        let found = input[self.offset..]
            .chars()
            .next()
            .map_or_else(|| "end of number".to_owned(), |ch| format!("{ch:?}"));
        format!(
            "Invalid number {input:?}: {} at character {position}, found {found}",
            self.reason
        )
    }
}

type Result<T> = std::result::Result<T, NumberError>;

#[derive(Clone, Copy, Debug)]
struct Digits<'a> {
    text: &'a str,
    offset: usize,
    count: usize,
    separated: bool,
}

impl Digits<'_> {
    fn normalized(&self) -> Cow<'_, str> {
        if self.separated {
            Cow::Owned(self.text.replace('_', ""))
        } else {
            Cow::Borrowed(self.text)
        }
    }

    fn integer(&self, radix: u32) -> BigInt {
        // Only used after the complete grammar has been accepted.
        BigInt::parse_bytes(self.normalized().as_bytes(), radix).expect("scanned digits")
    }
}

#[derive(Debug)]
struct Exponent<'a> {
    negative: bool,
    digits: Digits<'a>,
}

#[derive(Debug)]
enum RealKind<'a> {
    Integer,
    Rational(Digits<'a>, Digits<'a>),
    Decimal {
        whole: Digits<'a>,
        fraction: Digits<'a>,
        exponent: Option<Exponent<'a>>,
    },
    Infinity,
    Nan,
    Unit,
}

#[derive(Debug)]
struct Real<'a> {
    text: &'a str,
    offset: usize,
    signed: bool,
    negative: bool,
    normalize: bool,
    kind: RealKind<'a>,
}

#[derive(Debug)]
enum Form<'a> {
    Real(Real<'a>),
    Rectangular(Option<Real<'a>>, Real<'a>),
    Polar(Real<'a>, Real<'a>),
}

#[derive(Debug)]
struct Number<'a> {
    radix: u32,
    exactness: Option<bool>,
    form: Form<'a>,
}

struct Scanner<'a> {
    input: &'a str,
    at: usize,
    radix: u32,
}

impl<'a> Scanner<'a> {
    fn peek(&self) -> Option<u8> {
        self.input.as_bytes().get(self.at).copied()
    }

    fn error(&self, reason: &'static str) -> NumberError {
        NumberError::new(self.at, reason)
    }

    fn sign(&mut self) -> (bool, bool) {
        match self.peek() {
            Some(b'+' | b'-') => {
                let negative = self.peek() == Some(b'-');
                self.at += 1;
                (true, negative)
            }
            _ => (false, false),
        }
    }

    fn digits(&mut self, radix: u32, required: bool) -> Result<Digits<'a>> {
        let start = self.at;
        let mut count = 0;
        let mut separated = false;
        let digit = |byte: u8| char::from(byte).is_digit(radix);
        loop {
            match self.peek() {
                Some(ch) if digit(ch) => {
                    count += 1;
                    self.at += 1;
                }
                Some(b'_') => {
                    // The preceding digit is in this very component; an e
                    // exponent marker in decimal can never count as one.
                    if count == 0
                        || !self
                            .input
                            .as_bytes()
                            .get(self.at + 1)
                            .is_some_and(|b| digit(*b))
                    {
                        return Err(self.error("expected a single underscore between digits"));
                    }
                    separated = true;
                    self.at += 1;
                }
                _ => break,
            }
        }
        if required && count == 0 {
            return Err(self.error("expected a digit in the number's radix"));
        }
        Ok(Digits {
            text: &self.input[start..self.at],
            offset: start,
            count,
            separated,
        })
    }

    fn real(&mut self, allow_unit: bool) -> Result<Real<'a>> {
        let start = self.at;
        let (signed, negative) = self.sign();
        let mut normalize = false;
        let kind = if signed && matches!(self.peek(), Some(b'i' | b'I' | b'n' | b'N')) {
            // Infinity and NaN are legal in every radix. A unit is permitted
            // only as a signed imaginary component, never a polar component.
            if allow_unit
                && matches!(self.peek(), Some(b'i' | b'I'))
                && self.at + 1 == self.input.len()
            {
                RealKind::Unit
            } else {
                let nan = matches!(self.peek(), Some(b'n' | b'N'));
                for expected in if nan { b"nan.0" } else { b"inf.0" } {
                    if !self
                        .peek()
                        .is_some_and(|ch| ch.eq_ignore_ascii_case(expected))
                    {
                        return Err(self.error("expected inf.0 or nan.0 after the sign"));
                    }
                    self.at += 1;
                }
                if nan {
                    RealKind::Nan
                } else {
                    RealKind::Infinity
                }
            }
        } else {
            let whole = self.digits(self.radix, false)?;
            normalize |= whole.separated;
            if self.peek() == Some(b'/') && whole.count > 0 {
                self.at += 1;
                let denominator = self.digits(self.radix, true)?;
                RealKind::Rational(whole, denominator)
            } else {
                let dot = self.peek() == Some(b'.') && self.radix == 10;
                let fraction = if dot {
                    self.at += 1;
                    self.digits(10, whole.count == 0)?
                } else {
                    Digits {
                        text: "",
                        offset: self.at,
                        count: 0,
                        separated: false,
                    }
                };
                if whole.count == 0 && fraction.count == 0 {
                    return Err(self.error("expected a digit in the number's radix"));
                }
                normalize |= fraction.separated;
                let exponent = if self.radix == 10
                    && matches!(
                        self.peek(),
                        Some(b'e' | b'E' | b's' | b'S' | b'f' | b'F' | b'd' | b'D' | b'l' | b'L')
                    ) {
                    normalize |= !matches!(self.peek(), Some(b'e' | b'E'));
                    self.at += 1;
                    let (_, negative) = self.sign();
                    let digits = self.digits(10, true)?;
                    normalize |= digits.separated;
                    Some(Exponent { negative, digits })
                } else {
                    None
                };
                if dot || exponent.is_some() {
                    RealKind::Decimal {
                        whole,
                        fraction,
                        exponent,
                    }
                } else {
                    RealKind::Integer
                }
            }
        };
        Ok(Real {
            text: &self.input[start..self.at],
            offset: start,
            signed,
            negative,
            normalize,
            kind,
        })
    }

    fn scan(mut self) -> Result<Number<'a>> {
        let mut exactness = None;
        let mut radix_seen = false;
        while self.peek() == Some(b'#') {
            self.at += 1;
            match self.peek().map(|ch| ch.to_ascii_lowercase()) {
                Some(b'e' | b'i') => {
                    if exactness.is_some() {
                        return Err(self.error("duplicate exactness prefix"));
                    }
                    exactness = Some(self.peek().unwrap().eq_ignore_ascii_case(&b'e'));
                }
                Some(ch @ (b'b' | b'o' | b'd' | b'x')) => {
                    if radix_seen {
                        return Err(self.error("duplicate radix prefix"));
                    }
                    radix_seen = true;
                    self.radix = match ch {
                        b'b' => 2,
                        b'o' => 8,
                        b'x' => 16,
                        _ => 10,
                    };
                }
                _ => return Err(self.error("expected a radix or exactness prefix")),
            }
            self.at += 1;
        }
        let first = self.real(true)?;
        let form = match self.peek() {
            Some(b'i' | b'I') => {
                if !first.signed {
                    return Err(self.error("a pure imaginary number requires a sign"));
                }
                self.at += 1;
                Form::Rectangular(None, first)
            }
            Some(b'+' | b'-') => {
                let imag = self.real(true)?;
                if !matches!(self.peek(), Some(b'i' | b'I')) {
                    return Err(self.error("expected i after the imaginary component"));
                }
                self.at += 1;
                Form::Rectangular(Some(first), imag)
            }
            Some(b'@') => {
                self.at += 1;
                Form::Polar(first, self.real(false)?)
            }
            _ => Form::Real(first),
        };
        if self.peek().is_some() {
            return Err(self.error("expected end of number"));
        }
        Ok(Number {
            radix: self.radix,
            exactness,
            form,
        })
    }
}

fn integer(heap: &mut Heap, n: BigInt) -> TaggedValue {
    if let Some(n) = n.to_i64().filter(|n| TaggedValue::fits_fixnum(*n)) {
        TaggedValue::fixnum(n)
    } else {
        heap.alloc_bigint(n)
    }
}

fn rational(heap: &mut Heap, value: BigRational) -> TaggedValue {
    if value.is_integer() {
        integer(heap, value.to_integer())
    } else {
        heap.alloc_rational(value)
    }
}

impl Real<'_> {
    fn normalized(&self) -> Cow<'_, str> {
        if !self.normalize {
            return Cow::Borrowed(self.text);
        }
        Cow::Owned(
            self.text
                .chars()
                .filter(|ch| *ch != '_')
                .map(|ch| {
                    if matches!(self.kind, RealKind::Decimal { .. })
                        && matches!(ch, 's' | 'S' | 'f' | 'F' | 'd' | 'D' | 'l' | 'L')
                    {
                        'e'
                    } else {
                        ch
                    }
                })
                .collect(),
        )
    }

    fn exact_decimal(
        &self,
        whole: &Digits<'_>,
        fraction: &Digits<'_>,
        exponent: &Option<Exponent<'_>>,
    ) -> Result<BigRational> {
        // Retain the existing exact-decimal allocation limit. In particular,
        // overflowing an exponent must not fall through to a rounded float.
        let too_large = || {
            NumberError::new(
                exponent.as_ref().map_or(self.offset, |e| e.digits.offset),
                "exponent is too large for an exact number",
            )
        };
        let exp = match exponent {
            Some(e) => {
                let n = e
                    .digits
                    .normalized()
                    .parse::<i64>()
                    .map_err(|_| too_large())?;
                if e.negative { -n } else { n }
            }
            None => 0,
        };
        let frac = i64::try_from(fraction.count).map_err(|_| too_large())?;
        let scale = exp
            .checked_sub(frac)
            .filter(|s| s.unsigned_abs() <= 20_000)
            .ok_or_else(too_large)?;
        let mut digits = whole.normalized().into_owned();
        digits.push_str(&fraction.normalized());
        let mut mantissa =
            BigInt::parse_bytes(digits.as_bytes(), 10).expect("scanned decimal digits");
        if self.negative {
            mantissa = -mantissa;
        }
        let power = BigInt::from(10).pow(scale.unsigned_abs() as u32);
        Ok(if scale >= 0 {
            BigRational::from_integer(mantissa * power)
        } else {
            BigRational::new(mantissa, power)
        })
    }

    fn convert(&self, heap: &mut Heap, radix: u32, exactness: Option<bool>) -> Result<TaggedValue> {
        let value = match &self.kind {
            RealKind::Unit => TaggedValue::fixnum(if self.negative { -1 } else { 1 }),
            RealKind::Infinity | RealKind::Nan => {
                if exactness == Some(true) {
                    return Err(NumberError::new(
                        self.offset,
                        "infinity or NaN has no exact representation",
                    ));
                }
                let f = if matches!(self.kind, RealKind::Nan) {
                    f64::NAN
                } else if self.negative {
                    f64::NEG_INFINITY
                } else {
                    f64::INFINITY
                };
                heap.alloc_real(f)
            }
            RealKind::Integer => {
                let digits = self.normalized();
                if exactness == Some(false) && radix == 10 {
                    // Keep Rust's correctly rounded decimal conversion, also
                    // preserving the sign of #i-0 and overflowing integers.
                    return Ok(heap.alloc_real(digits.parse().expect("scanned decimal integer")));
                }
                match i64::from_str_radix(&digits, radix) {
                    Ok(n) if TaggedValue::fits_fixnum(n) => TaggedValue::fixnum(n),
                    _ => integer(
                        heap,
                        BigInt::parse_bytes(digits.as_bytes(), radix).expect("scanned integer"),
                    ),
                }
            }
            RealKind::Rational(numerator, denominator) => {
                let denom = denominator.integer(radix);
                if denom.is_zero() {
                    return Err(NumberError::new(
                        denominator.offset,
                        "rational denominator must be nonzero",
                    ));
                }
                let numer = numerator.integer(radix);
                rational(
                    heap,
                    BigRational::new(if self.negative { -numer } else { numer }, denom),
                )
            }
            RealKind::Decimal {
                whole,
                fraction,
                exponent,
            } => {
                if exactness == Some(true) {
                    rational(heap, self.exact_decimal(whole, fraction, exponent)?)
                } else {
                    heap.alloc_real(self.normalized().parse().expect("scanned decimal"))
                }
            }
        };
        Ok(if exactness == Some(false) {
            if self.negative && heap.is_numeric_zero_tv(value) {
                heap.alloc_real(-0.0)
            } else {
                heap.to_inexact(value)
            }
        } else {
            value
        })
    }
}

impl Number<'_> {
    fn convert(&self, heap: &mut Heap) -> Result<TaggedValue> {
        let part = |r: &Real<'_>, heap: &mut Heap| r.convert(heap, self.radix, self.exactness);
        match &self.form {
            Form::Real(real) => part(real, heap),
            Form::Rectangular(real, imag) => {
                // Preserve Patina/Chibi's exact implicit zero (#418). A #i
                // prefix alone asks for both components to become inexact.
                let re = match real {
                    Some(real) => part(real, heap)?,
                    None if self.exactness == Some(false) => heap.alloc_real(0.0),
                    None => TaggedValue::fixnum(0),
                };
                let im = part(imag, heap)?;
                Ok(heap.alloc_complex(re, im))
            }
            Form::Polar(magnitude, angle) => {
                // Test a known exact zero angle before #i coerces it, so
                // 1@0 and #i1@0 preserve the magnitude without trigonometry.
                let component_exactness = self.exactness.filter(|exact| *exact);
                let r = magnitude.convert(heap, self.radix, component_exactness)?;
                let theta = angle.convert(heap, self.radix, component_exactness)?;
                if heap.is_exact_zero(theta) {
                    return Ok(if self.exactness == Some(false) {
                        if magnitude.negative && heap.is_numeric_zero_tv(r) {
                            heap.alloc_real(-0.0)
                        } else {
                            heap.to_inexact(r)
                        }
                    } else {
                        r
                    });
                }
                let radius = heap.numeric_to_f64(r).expect("real magnitude");
                let radians = heap.numeric_to_f64(theta).expect("real angle");
                let re = heap.alloc_real(radius * radians.cos());
                let im = heap.alloc_real(radius * radians.sin());
                let value = heap.alloc_complex(re, im);
                if self.exactness == Some(true) {
                    // As with (exact (make-polar ...)), make the computed
                    // finite approximation exact; do not silently ignore #e.
                    heap.to_exact(value).map_err(|_| {
                        NumberError::new(angle.offset, "polar result has no exact representation")
                    })
                } else {
                    Ok(value)
                }
            }
        }
    }
}

pub(super) fn parse(input: &str, radix: u32, heap: &SharedHeap) -> Result<TaggedValue> {
    let number = Scanner {
        input,
        at: 0,
        radix,
    }
    .scan()?;
    number.convert(&mut heap.borrow_mut())
}
