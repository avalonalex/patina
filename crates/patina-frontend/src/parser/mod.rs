use crate::lexer::{LexError, Lexer, Token};
use crate::source_map::SourceMap;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{ToPrimitive, Zero};
use patina_core::error::SourceLocation;
use patina_core::heap::HeapObjectData;
use patina_core::{SharedHeap, TaggedValue};

use std::cell::RefCell;
use std::collections::HashMap;
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

    #[error("Undefined datum label: #{0}#")]
    UndefinedLabel(usize),

    #[error("Duplicate datum label: #{0}=")]
    DuplicateLabel(usize),

    /// A `define-library` reached an `include-shared` declaration: its
    /// implementation is a compiled shared object, which Patina cannot load.
    ///
    /// Its own variant rather than an `InvalidSyntax` string because the
    /// compatibility harness classifies on it — the library is out of scope
    /// pending FFI, not broken — and because a caller that wants to say so in
    /// its own words needs to be able to tell it apart. See
    /// `LibraryError::NativeExtensionRequired`.
    // The wording deliberately does not restate
    // `patina_runtime::NATIVE_EXTENSION_MARKER`: that phrase is a contract the
    // compat harness links against, and a second copy here would read like one
    // without being checked. `LibraryError::NativeExtensionRequired` is where
    // the marker belongs, and it is what every caller renders.
    #[error("include-shared \"{0}\"")]
    NativeExtensionRequired(String),
}

pub struct Parser {
    lexer: Lexer,
    current_token: Token,
    /// Line of the current token (1-based)
    current_token_line: u32,
    /// Column of the current token (1-based)
    current_token_column: u32,
    /// Shared heap for allocating pairs, vectors, strings, etc.
    heap: SharedHeap,
    /// Datum labels for shared/cyclic structure support (R7RS Section 2.4)
    /// Maps label number to the labelled value
    labels: HashMap<usize, TaggedValue>,
    /// Optional source map for recording source positions of parsed forms
    source_map: Option<Rc<RefCell<SourceMap>>>,
    /// Name of the source being parsed (e.g., file path, "<repl>", "<eval>")
    source_name: Rc<str>,
}

impl Parser {
    /// Create a new parser with the given heap.
    /// This is the preferred constructor when a heap is available.
    pub fn new_with_heap(input: &str, heap: SharedHeap) -> Result<Self, ParseError> {
        let mut lexer = Lexer::new(input);
        let spanned = lexer.next_token()?;
        Ok(Parser {
            lexer,
            current_token: spanned.token,
            current_token_line: spanned.line,
            current_token_column: spanned.column,
            heap,
            labels: HashMap::new(),
            source_map: None,
            source_name: Rc::from("<unknown>"),
        })
    }

    /// Create a new parser with a fresh heap.
    /// This is provided for backward compatibility and testing.
    pub fn new(input: &str) -> Result<Self, ParseError> {
        let heap = patina_core::new_shared_heap();
        Self::new_with_heap(input, heap)
    }

    /// Create a new parser with source map tracking.
    ///
    /// Source positions of list forms, vectors, and quote abbreviations are
    /// recorded in the source map, keyed by the TaggedValue's raw bits.
    pub fn new_with_source_map(
        input: &str,
        heap: SharedHeap,
        source_name: Rc<str>,
        source_map: Rc<RefCell<SourceMap>>,
    ) -> Result<Self, ParseError> {
        // Store source text for caret-style error display
        source_map.borrow_mut().set_source_text(input.to_string());
        // The map is keyed by raw bits, so slots the GC reclaims must be
        // pruned from it (§9.1). Recording is enabled here — only
        // source-mapped sessions pay for it — and the run loops drain via
        // `prune_freed_locations` at form boundaries. Draining now starts
        // this session clean of any earlier session's leftover bits (the
        // map is empty, so the prune itself is a no-op).
        heap.borrow_mut().enable_gc_freed_tracking();
        crate::source_map::prune_freed_locations(&heap, &source_map);
        let mut lexer = Lexer::new(input);
        let spanned = lexer.next_token()?;
        Ok(Parser {
            lexer,
            current_token: spanned.token,
            current_token_line: spanned.line,
            current_token_column: spanned.column,
            heap,
            labels: HashMap::new(),
            source_map: Some(source_map),
            source_name,
        })
    }

    /// Create a parser with case-folding enabled and the given heap.
    ///
    /// Used by `include-ci` to read files in case-insensitive mode.
    /// All identifiers will be folded to lowercase.
    pub fn new_case_insensitive_with_heap(
        input: &str,
        heap: SharedHeap,
    ) -> Result<Self, ParseError> {
        let mut lexer = Lexer::new_case_insensitive(input);
        let spanned = lexer.next_token()?;
        Ok(Parser {
            lexer,
            current_token: spanned.token,
            current_token_line: spanned.line,
            current_token_column: spanned.column,
            heap,
            labels: HashMap::new(),
            source_map: None,
            source_name: Rc::from("<unknown>"),
        })
    }

    /// Create a parser with case-folding enabled and a fresh heap.
    pub fn new_case_insensitive(input: &str) -> Result<Self, ParseError> {
        let heap = patina_core::new_shared_heap();
        Self::new_case_insensitive_with_heap(input, heap)
    }

    /// Get a reference to the parser's heap
    pub fn heap(&self) -> &SharedHeap {
        &self.heap
    }

    /// Char offset just past the last token consumed by the most recent
    /// `parse()` call. The parser keeps a one-token lookahead, so after
    /// `parse()` returns a datum this is the end of that datum's final
    /// token; everything from this offset onward (including whitespace
    /// before the lookahead token) is unconsumed input. Used by `read` to
    /// preserve the remainder for subsequent input operations.
    pub fn consumed_end(&self) -> usize {
        self.lexer.prev_token_end()
    }

    /// Record a source location for a TaggedValue in the source map (if present)
    fn record_source(&self, tv: TaggedValue, line: u32, col: u32) {
        if let Some(ref sm) = self.source_map {
            sm.borrow_mut()
                .record(tv, SourceLocation::new(self.source_name.clone(), line, col));
        }
    }

    fn advance(&mut self) -> Result<(), ParseError> {
        let spanned = self.lexer.next_token()?;
        self.current_token = spanned.token;
        self.current_token_line = spanned.line;
        self.current_token_column = spanned.column;
        Ok(())
    }

    pub fn parse(&mut self) -> Result<TaggedValue, ParseError> {
        let result = self.parse_expr()?;
        // After parsing the outermost datum, resolve any label placeholders
        Ok(self.resolve_labels(result))
    }

    /// Parse all expressions from the input until EOF.
    ///
    /// Returns a vector of all parsed expressions. Useful for parsing
    /// files that contain multiple top-level expressions (like included files).
    pub fn parse_all(&mut self) -> Result<Vec<TaggedValue>, ParseError> {
        let mut exprs = Vec::new();
        while self.current_token != Token::Eof {
            let expr = self.parse_expr()?;
            // Resolve labels for each top-level expression
            exprs.push(self.resolve_labels(expr));
            // Clear labels between top-level expressions
            self.labels.clear();
        }
        Ok(exprs)
    }

    /// Consume any run of `#;` datum comments, skipping each commented datum.
    ///
    /// Datum comments may appear anywhere a datum may — including
    /// immediately before a closing delimiter, the position the previously
    /// scattered inline copies of this loop missed. Every datum-position
    /// loop calls this rather than open-coding the skip.
    fn skip_datum_comments(&mut self) -> Result<(), ParseError> {
        while self.current_token == Token::DatumComment {
            self.advance()?; // consume #;
            self.skip_datum()?; // skip the commented datum
        }
        Ok(())
    }

    fn parse_expr(&mut self) -> Result<TaggedValue, ParseError> {
        self.skip_datum_comments()?;

        match &self.current_token.clone() {
            Token::Boolean(b) => {
                let val = if *b {
                    TaggedValue::TRUE
                } else {
                    TaggedValue::FALSE
                };
                self.advance()?;
                Ok(val)
            }
            Token::Number(s) => {
                let val = self.parse_number(s)?;
                self.advance()?;
                Ok(val)
            }
            Token::Character(c) => {
                let val = TaggedValue::character(*c);
                self.advance()?;
                Ok(val)
            }
            Token::String(s) => {
                // Allocate string on heap
                let val = self.heap.borrow_mut().alloc_string(s.clone());
                self.advance()?;
                Ok(val)
            }
            Token::Identifier(s) => {
                // Intern symbol in heap
                let val = self.heap.borrow_mut().intern_symbol(s);
                self.advance()?;
                Ok(val)
            }
            Token::Quote => {
                let abbr_line = self.current_token_line;
                let abbr_col = self.current_token_column;
                self.advance()?;
                let quoted = self.parse_expr()?;
                let quote_sym = self.heap.borrow_mut().intern_symbol("quote");
                let result = self.make_list(vec![quote_sym, quoted]);
                self.record_source(result, abbr_line, abbr_col);
                Ok(result)
            }
            Token::Quasiquote => {
                let abbr_line = self.current_token_line;
                let abbr_col = self.current_token_column;
                self.advance()?;
                let quoted = self.parse_expr()?;
                let sym = self.heap.borrow_mut().intern_symbol("quasiquote");
                let result = self.make_list(vec![sym, quoted]);
                self.record_source(result, abbr_line, abbr_col);
                Ok(result)
            }
            Token::Unquote => {
                let abbr_line = self.current_token_line;
                let abbr_col = self.current_token_column;
                self.advance()?;
                let quoted = self.parse_expr()?;
                let sym = self.heap.borrow_mut().intern_symbol("unquote");
                let result = self.make_list(vec![sym, quoted]);
                self.record_source(result, abbr_line, abbr_col);
                Ok(result)
            }
            Token::UnquoteSplicing => {
                let abbr_line = self.current_token_line;
                let abbr_col = self.current_token_column;
                self.advance()?;
                let quoted = self.parse_expr()?;
                let sym = self.heap.borrow_mut().intern_symbol("unquote-splicing");
                let result = self.make_list(vec![sym, quoted]);
                self.record_source(result, abbr_line, abbr_col);
                Ok(result)
            }
            Token::LeftParen => self.parse_list(),
            Token::VectorOpen => self.parse_vector(),
            Token::BytevectorOpen => self.parse_bytevector(),
            // R7RS datum labels: #n= defines a label, #n# references it
            Token::DatumLabel(n) => {
                let label = *n;
                self.advance()?; // consume #n=

                // Check for duplicate label
                if self.labels.contains_key(&label) {
                    return Err(ParseError::DuplicateLabel(label));
                }

                // Parse the datum being labelled
                let datum = self.parse_expr()?;

                // Store the labelled datum
                self.labels.insert(label, datum);

                Ok(datum)
            }
            Token::DatumRef(n) => {
                let label = *n;
                self.advance()?; // consume #n#

                // If the label is already resolved, return a reference to it
                // Otherwise, return a placeholder that will be resolved later
                if let Some(value) = self.labels.get(&label) {
                    Ok(*value)
                } else {
                    // Return a placeholder - will be resolved after parsing completes
                    Ok(self.heap.borrow_mut().alloc_label_placeholder(label))
                }
            }
            Token::Eof => Err(ParseError::UnexpectedEof),
            token => Err(ParseError::UnexpectedToken(token.clone())),
        }
    }

    /// Skip a single datum (used for #; datum comments)
    /// This parses the datum but discards the result
    fn skip_datum(&mut self) -> Result<(), ParseError> {
        // Handle nested datum comments within the skipped datum
        self.skip_datum_comments()?;

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
            // Datum labels: #n= followed by datum, #n# is just the reference
            Token::DatumLabel(_) => {
                self.advance()?; // consume #n=
                self.skip_datum() // skip the labelled datum
            }
            Token::DatumRef(_) => {
                self.advance()?; // consume #n#
                Ok(())
            }
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

    fn parse_list(&mut self) -> Result<TaggedValue, ParseError> {
        // Capture position of the opening `(`
        let open_line = self.current_token_line;
        let open_col = self.current_token_column;
        self.advance()?; // consume (

        let mut elements = Vec::new();
        let mut dotted_tail = None;

        while self.current_token != Token::RightParen {
            if self.current_token == Token::Eof {
                return Err(ParseError::UnexpectedEof);
            }

            // A datum comment may sit immediately before the closing paren
            // — `(a b #;c)` — so consume comments here rather than letting
            // parse_expr skip them and then face the bare `)`.
            self.skip_datum_comments()?;
            if self.current_token == Token::RightParen {
                break;
            }

            if self.current_token == Token::Dot {
                // `( . b)` — including `(#;a . b)` after comment stripping —
                // has no head and is not a valid dotted list.
                if elements.is_empty() {
                    return Err(ParseError::UnexpectedToken(Token::Dot));
                }
                self.advance()?;
                dotted_tail = Some(self.parse_expr()?);
                // After the tail, skip any datum comments before )
                self.skip_datum_comments()?;
                break;
            }

            elements.push(self.parse_expr()?);
        }

        if self.current_token != Token::RightParen {
            return Err(ParseError::UnexpectedToken(self.current_token.clone()));
        }
        self.advance()?; // consume )

        let result = self
            .heap
            .borrow_mut()
            .list_from_iter_with_tail(elements, dotted_tail.unwrap_or(TaggedValue::NULL));
        self.record_source(result, open_line, open_col);
        Ok(result)
    }

    fn parse_vector(&mut self) -> Result<TaggedValue, ParseError> {
        // Capture position of the opening `#(`
        let open_line = self.current_token_line;
        let open_col = self.current_token_column;
        self.advance()?; // consume #(

        let mut elements = Vec::new();

        while self.current_token != Token::RightParen {
            if self.current_token == Token::Eof {
                return Err(ParseError::UnexpectedEof);
            }
            // As in parse_list: `#(a #;b)` — a datum comment may precede `)`.
            self.skip_datum_comments()?;
            if self.current_token == Token::RightParen {
                break;
            }
            elements.push(self.parse_expr()?);
        }

        self.advance()?; // consume )
        // Allocate vector on heap
        let result = self.heap.borrow_mut().alloc_vector(elements);
        self.record_source(result, open_line, open_col);
        Ok(result)
    }

    fn parse_bytevector(&mut self) -> Result<TaggedValue, ParseError> {
        self.advance()?; // consume #u8(

        let mut bytes = Vec::new();

        while self.current_token != Token::RightParen {
            if self.current_token == Token::Eof {
                return Err(ParseError::UnexpectedEof);
            }

            // As in parse_list: a datum comment may appear between bytes.
            self.skip_datum_comments()?;
            if self.current_token == Token::RightParen {
                break;
            }

            if let Token::Number(s) = &self.current_token.clone() {
                // Parse the number (handles decimal, hex #x, binary #b, octal #o)
                let tv = self.parse_number(s)?;

                // Extract integer value and validate it's a valid byte (0-255)
                let byte = if let Some(n) = tv.as_fixnum() {
                    if (0..=255).contains(&n) {
                        n as u8
                    } else {
                        return Err(ParseError::InvalidSyntax(format!(
                            "Byte value out of range (0-255): {}",
                            n
                        )));
                    }
                } else {
                    return Err(ParseError::InvalidSyntax(
                        "Bytevector must contain only integer bytes (0-255)".to_string(),
                    ));
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
        // Allocate bytevector on heap
        Ok(self.heap.borrow_mut().alloc_bytevector(bytes))
    }

    /// Create a TaggedValue integer from i64 (fixnum if it fits, BigInt otherwise)
    fn tagged_integer(&self, n: i64) -> TaggedValue {
        if TaggedValue::fits_fixnum(n) {
            TaggedValue::fixnum(n)
        } else {
            self.heap
                .borrow_mut()
                .alloc_bigint(num_bigint::BigInt::from(n))
        }
    }

    /// Parse a number string and return a TaggedValue
    fn parse_number(&self, s: &str) -> Result<TaggedValue, ParseError> {
        // Parse numbers following the R7RS numeric tower

        // Handle R7RS numeric prefixes: #e #i #b #o #d #x
        if s.starts_with('#') {
            return self.parse_number_with_prefix(s);
        }

        // Check for special R7RS floating-point literals (case-insensitive)
        match s.to_lowercase().as_str() {
            "+inf.0" => return Ok(self.heap.borrow_mut().alloc_real(f64::INFINITY)),
            "-inf.0" => return Ok(self.heap.borrow_mut().alloc_real(f64::NEG_INFINITY)),
            "+nan.0" | "-nan.0" => return Ok(self.heap.borrow_mut().alloc_real(f64::NAN)),
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
            return self.parse_rational(s);
        }

        // Try i64 first (fast path for small integers)
        if let Ok(n) = s.parse::<i64>() {
            return Ok(self.tagged_integer(n));
        }

        // If it doesn't fit in i64, try BigInt (for large integers)
        if let Ok(n) = BigInt::from_str(s) {
            return Ok(self.heap.borrow_mut().alloc_bigint(n));
        }

        // If it's not an integer, try floating point
        // R7RS allows alternate exponent markers: s (short), f (single), d (double), l (long)
        // We normalize all to 'e' and parse as f64.
        let normalized = Self::normalize_exponent_markers(s);
        if let Ok(f) = normalized.parse::<f64>() {
            return Ok(self.heap.borrow_mut().alloc_real(f));
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

    fn parse_number_with_prefix(&self, s: &str) -> Result<TaggedValue, ParseError> {
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
        let tv = if radix == 10 {
            // For decimal, use the main parse_number logic (handles floats, rationals, complex, etc.)
            // Note: rest doesn't start with '#' at this point, so this won't recurse
            self.parse_number(rest)?
        } else if rest.contains('/') {
            // Parse rational with given radix: e.g., #x1/10 -> 1/16
            self.parse_rational_with_radix(rest, radix)?
        } else {
            // For non-decimal radix, parse as integer only
            self.parse_integer_with_radix(rest, radix)?
        };

        // Apply exactness conversion if specified
        self.apply_exactness(tv, exactness, s)
    }

    /// Parse a rational number string: "numerator/denominator"
    fn parse_rational(&self, s: &str) -> Result<TaggedValue, ParseError> {
        let parts: Vec<&str> = s.split('/').collect();
        if parts.len() == 2 {
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

            Ok(self.tagged_rational(BigRational::new(numer, denom)))
        } else {
            Err(ParseError::InvalidSyntax(format!(
                "Invalid rational: {}",
                s
            )))
        }
    }

    /// Parse a rational number with a non-decimal radix
    fn parse_rational_with_radix(&self, rest: &str, radix: u32) -> Result<TaggedValue, ParseError> {
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
            ParseError::InvalidSyntax(format!("Invalid {} denominator: {}", radix_name, parts[1]))
        })?;

        if denom.is_zero() {
            return Err(ParseError::InvalidSyntax(
                "Rational denominator cannot be zero".to_string(),
            ));
        }

        Ok(self.tagged_rational(BigRational::new(numer, denom)))
    }

    /// Parse an integer with a non-decimal radix
    fn parse_integer_with_radix(&self, rest: &str, radix: u32) -> Result<TaggedValue, ParseError> {
        match i64::from_str_radix(rest, radix) {
            Ok(n) => Ok(self.tagged_integer(n)),
            Err(_) => {
                // Try BigInt if it doesn't fit in i64
                BigInt::parse_bytes(rest.as_bytes(), radix)
                    .map(|n| self.heap.borrow_mut().alloc_bigint(n))
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
                    })
            }
        }
    }

    /// Create a TaggedValue from a BigRational, simplifying to integer if denominator is 1
    fn tagged_rational(&self, ratio: BigRational) -> TaggedValue {
        if ratio.denom() == &BigInt::from(1) {
            let numer = ratio.numer();
            if let Some(n) = numer.to_i64() {
                self.tagged_integer(n)
            } else {
                self.heap.borrow_mut().alloc_bigint(numer.clone())
            }
        } else {
            self.heap.borrow_mut().alloc_rational(ratio)
        }
    }

    /// Apply exactness prefix (#e or #i) to a parsed TaggedValue
    fn apply_exactness(
        &self,
        tv: TaggedValue,
        exactness: Option<bool>,
        original: &str,
    ) -> Result<TaggedValue, ParseError> {
        let heap = &self.heap;
        match exactness {
            Some(true) => {
                // Force exact: fixnums/bigint/rational stay; reals convert if whole
                if tv.is_fixnum() {
                    Ok(tv)
                } else {
                    let heap_ref = heap.borrow();
                    if heap_ref.get_bigint(tv).is_some() || heap_ref.get_rational(tv).is_some() {
                        Ok(tv)
                    } else if let Some(f) = heap_ref.get_real(tv) {
                        if f.fract() == 0.0 && f.is_finite() {
                            drop(heap_ref);
                            Ok(self.tagged_integer(f as i64))
                        } else {
                            Err(ParseError::InvalidSyntax(format!(
                                "#e prefix on inexact number not yet fully supported: {}",
                                original
                            )))
                        }
                    } else {
                        Ok(tv)
                    }
                }
            }
            Some(false) => {
                // Force inexact: convert to real
                if let Some(n) = tv.as_fixnum() {
                    Ok(heap.borrow_mut().alloc_real(n as f64))
                } else {
                    let heap_ref = heap.borrow();
                    if let Some(b) = heap_ref.get_bigint(tv) {
                        let f = b.to_f64().unwrap_or(f64::INFINITY);
                        drop(heap_ref);
                        Ok(heap.borrow_mut().alloc_real(f))
                    } else if let Some(r) = heap_ref.get_rational(tv) {
                        let f = r.to_f64().unwrap_or(f64::NAN);
                        drop(heap_ref);
                        Ok(heap.borrow_mut().alloc_real(f))
                    } else {
                        // Already inexact (real) or other type
                        Ok(tv)
                    }
                }
            }
            None => Ok(tv),
        }
    }

    /// Helper to create a Complex value from two real components
    /// Allocate a complex number on the heap from two TaggedValue components
    fn alloc_complex(&self, real: TaggedValue, imag: TaggedValue) -> TaggedValue {
        self.heap.borrow_mut().alloc_complex(real, imag)
    }

    fn parse_rectangular(&self, s: &str) -> Result<TaggedValue, ParseError> {
        // Remove the trailing 'i' or 'I'
        let s_no_i = &s[..s.len() - 1];

        // Handle special cases: +i, -i
        let zero = TaggedValue::fixnum(0);
        if s_no_i == "+" || s_no_i.is_empty() {
            // +i means 0+1i (both exact integers)
            return Ok(self.alloc_complex(zero, TaggedValue::fixnum(1)));
        }
        if s_no_i == "-" {
            // -i means 0-1i (both exact integers)
            return Ok(self.alloc_complex(zero, TaggedValue::fixnum(-1)));
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

            // Parse both parts as TaggedValues (preserving exactness)
            let real_tv = if real_part_str.is_empty() {
                zero
            } else {
                self.parse_real_component_as_tagged(real_part_str)?
            };
            let imag_tv = self.parse_real_component_as_tagged(&imag_str)?;

            Ok(self.alloc_complex(real_tv, imag_tv))
        } else {
            // No separator found - this is pure imaginary like "+5i" or "-3i"
            let imag_tv = self.parse_real_component_as_tagged(s_no_i)?;
            Ok(self.alloc_complex(zero, imag_tv))
        }
    }

    fn parse_polar(&self, s: &str) -> Result<TaggedValue, ParseError> {
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

        let mut heap = self.heap.borrow_mut();
        let real_tv = heap.alloc_real(real);
        let imag_tv = heap.alloc_real(imag);
        Ok(heap.alloc_complex(real_tv, imag_tv))
    }

    /// Parse a component as a TaggedValue, preserving exactness
    /// This is key for R7RS complex number semantics: `1+0i` has exact 0 imaginary,
    /// while `1+0.0i` has inexact 0.0 imaginary.
    fn parse_real_component_as_tagged(&self, s: &str) -> Result<TaggedValue, ParseError> {
        // Check for special R7RS floating-point literals (case-insensitive, always inexact)
        match s.to_lowercase().as_str() {
            "+inf.0" => return Ok(self.heap.borrow_mut().alloc_real(f64::INFINITY)),
            "-inf.0" => return Ok(self.heap.borrow_mut().alloc_real(f64::NEG_INFINITY)),
            "+nan.0" | "-nan.0" => return Ok(self.heap.borrow_mut().alloc_real(f64::NAN)),
            _ => {}
        }

        // Check if it's a float (has decimal point or exponent marker) -> inexact
        // Include alternate exponent markers: s, f, d, l (R7RS 7.1.1)
        let is_float = s.contains('.')
            || s.contains('e')
            || s.contains('E')
            || s.chars()
                .any(|c| matches!(c, 's' | 'S' | 'f' | 'F' | 'd' | 'D' | 'l' | 'L'));

        // Handle sign prefix for parsing
        let (sign, num_str) = if let Some(stripped) = s.strip_prefix('+') {
            (1i64, stripped)
        } else if let Some(stripped) = s.strip_prefix('-') {
            (-1i64, stripped)
        } else {
            (1i64, s)
        };

        if is_float {
            // Normalize alternate exponent markers before parsing
            let normalized = Self::normalize_exponent_markers(s);
            let val = normalized
                .parse::<f64>()
                .map_err(|_| ParseError::InvalidSyntax(format!("Invalid real number: {}", s)))?;
            return Ok(self.heap.borrow_mut().alloc_real(val));
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
                return Ok(self.heap.borrow_mut().alloc_rational(ratio));
            }
        }

        // Try i64 first (exact)
        if let Ok(n) = num_str.parse::<i64>() {
            return Ok(self.tagged_integer(sign * n));
        }

        // Try BigInt (exact)
        if let Ok(n) = BigInt::from_str(num_str) {
            let n = if sign < 0 { -n } else { n };
            return Ok(self.heap.borrow_mut().alloc_bigint(n));
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

        // Try float (normalize alternate exponent markers first)
        let normalized = Self::normalize_exponent_markers(s);
        normalized
            .parse::<f64>()
            .map_err(|_| ParseError::InvalidSyntax(format!("Invalid real number: {}", s)))
    }

    fn make_list(&self, elements: Vec<TaggedValue>) -> TaggedValue {
        self.heap.borrow_mut().list_from_iter(elements)
    }

    /// Resolve all LabelPlaceholder values in a parsed datum.
    ///
    /// After parsing completes, any #n# references that appeared before
    /// the corresponding #n= label was fully parsed will be LabelPlaceholder
    /// values. This function walks the structure and replaces them with
    /// the actual labelled values.
    ///
    /// For cyclic structures, this mutates pairs/vectors in place to create
    /// actual cycles (rather than copies).
    fn resolve_labels(&self, tv: TaggedValue) -> TaggedValue {
        use std::collections::HashSet;
        let mut visited = HashSet::new();
        self.resolve_labels_inner(tv, &mut visited)
    }

    fn resolve_labels_inner(
        &self,
        tv: TaggedValue,
        visited: &mut std::collections::HashSet<u64>,
    ) -> TaggedValue {
        // Check for LabelPlaceholder
        if tv.is_object() {
            let heap = self.heap.borrow();
            if let HeapObjectData::LabelPlaceholder(n) = heap.get_object(tv) {
                let n = *n;
                drop(heap);
                if let Some(&resolved) = self.labels.get(&n) {
                    return resolved;
                }
                return tv;
            }
        }

        // Handle native heap pairs
        if tv.is_pair() {
            let addr = tv.raw();
            if visited.contains(&addr) {
                // Already visited - return as-is to avoid infinite loop
                return tv;
            }
            visited.insert(addr);

            // Resolve car and cdr
            let (car, cdr) = {
                let heap = self.heap.borrow();
                (heap.car(tv), heap.cdr(tv))
            };

            let new_car = self.resolve_labels_inner(car, visited);
            let new_cdr = self.resolve_labels_inner(cdr, visited);

            // Mutate in place to preserve identity
            {
                let mut heap = self.heap.borrow_mut();
                heap.set_car(tv, new_car);
                heap.set_cdr(tv, new_cdr);
            }

            return tv;
        }

        // Handle native heap vectors
        if tv.is_vector() {
            let addr = tv.raw();
            if visited.contains(&addr) {
                return tv;
            }
            visited.insert(addr);

            // Get vector length and resolve all elements
            let len = self.heap.borrow().vector_len(tv);
            for i in 0..len {
                let elem = self.heap.borrow().vector_ref(tv, i);
                let resolved = self.resolve_labels_inner(elem, visited);
                if resolved.raw() != elem.raw() {
                    self.heap.borrow_mut().vector_set(tv, i, resolved);
                }
            }

            return tv;
        }

        // Other value types don't contain nested values that could be placeholders
        tv
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_atom() {
        let mut parser = Parser::new("42").unwrap();
        let result = parser.parse().unwrap();
        // Should be a fixnum
        assert!(result.is_fixnum());
        assert_eq!(result.as_fixnum_unchecked(), 42);
    }

    #[test]
    fn test_parse_list() {
        let mut parser = Parser::new("(+ 1 2)").unwrap();
        let result = parser.parse().unwrap();
        // Should be a pair
        assert!(result.is_pair());
    }

    #[test]
    fn test_consumed_end_after_atom() {
        // "5 40": parsing one datum consumes exactly the "5"; the space
        // and the "40" remain unconsumed
        let mut parser = Parser::new("5 40").unwrap();
        let result = parser.parse().unwrap();
        assert_eq!(result.as_fixnum_unchecked(), 5);
        assert_eq!(parser.consumed_end(), 1);

        // A second parse from the same parser picks up the next datum
        let result = parser.parse().unwrap();
        assert_eq!(result.as_fixnum_unchecked(), 40);
        assert_eq!(parser.consumed_end(), 4);
    }

    #[test]
    fn test_consumed_end_after_list() {
        let mut parser = Parser::new("(a b) (c)").unwrap();
        let result = parser.parse().unwrap();
        assert!(result.is_pair());
        assert_eq!(parser.consumed_end(), 5); // just past the closing paren
    }

    #[test]
    fn test_consumed_end_with_leading_whitespace_and_comments() {
        let mut parser = Parser::new("  ; note\n  7 8").unwrap();
        let result = parser.parse().unwrap();
        assert_eq!(result.as_fixnum_unchecked(), 7);
        assert_eq!(parser.consumed_end(), 12); // just past the "7"
    }

    #[test]
    fn test_parse_small_integer() {
        // Small integers should parse as fixnum
        let mut parser = Parser::new("42").unwrap();
        let result = parser.parse().unwrap();
        assert!(result.is_fixnum());
        assert_eq!(result.as_fixnum_unchecked(), 42);
    }

    #[test]
    fn test_parse_i64_max() {
        // i64::MAX is larger than fixnum range, should be BigInteger
        let mut parser = Parser::new("9223372036854775807").unwrap();
        let result = parser.parse().unwrap();
        // This is larger than fixnum range, so it's a BigInteger on heap
        assert!(result.is_object());
    }

    #[test]
    fn test_parse_i64_min() {
        // i64::MIN is larger magnitude than fixnum range
        let mut parser = Parser::new("-9223372036854775808").unwrap();
        let result = parser.parse().unwrap();
        // This is outside fixnum range, so it's a BigInteger on heap
        assert!(result.is_object());
    }

    #[test]
    fn test_parse_beyond_i64_max() {
        // i64::MAX + 1 should parse as BigInteger on heap
        let mut parser = Parser::new("9223372036854775808").unwrap();
        let result = parser.parse().unwrap();
        assert!(result.is_object());
        // Check it's a BigInt
        assert!(parser.heap().borrow().is_bigint(result));
    }

    #[test]
    fn test_parse_large_bigint() {
        // Very large number should parse as BigInteger
        let mut parser = Parser::new("10000000000000000000").unwrap();
        let result = parser.parse().unwrap();
        assert!(result.is_object());
        assert!(parser.heap().borrow().is_bigint(result));
    }

    #[test]
    fn test_parse_huge_bigint() {
        // Astronomically large number (2^100) should parse as BigInteger
        let mut parser = Parser::new("1267650600228229401496703205376").unwrap();
        let result = parser.parse().unwrap();
        assert!(result.is_object());
        assert!(parser.heap().borrow().is_bigint(result));
    }

    #[test]
    fn test_parse_negative_beyond_i64_min() {
        // i64::MIN - 1 should parse as BigInteger
        let mut parser = Parser::new("-9223372036854775809").unwrap();
        let result = parser.parse().unwrap();
        assert!(result.is_object());
        assert!(parser.heap().borrow().is_bigint(result));
    }

    #[test]
    fn test_parse_bigint_in_expression() {
        // Large integers in expressions should parse as BigInteger
        let mut parser = Parser::new("(+ 10000000000000000000 1)").unwrap();
        let result = parser.parse().unwrap();

        // The result should be a list (pair chain)
        assert!(result.is_pair());
        let heap = parser.heap();
        let heap_ref = heap.borrow();
        let car = heap_ref.car(result);
        let cdr = heap_ref.cdr(result);
        assert!(heap_ref.get_symbol_name(car).is_some()); // The '+'

        assert!(cdr.is_pair());
        let arg1 = heap_ref.car(cdr);
        // The first argument should be BigInteger
        let bigint = heap_ref
            .get_bigint(arg1)
            .expect("Expected BigInteger for first arg");
        assert_eq!(bigint.to_string(), "10000000000000000000");
    }

    #[test]
    fn test_parse_positive_infinity() {
        let mut parser = Parser::new("+inf.0").unwrap();
        let result = parser.parse().unwrap();
        let f = parser
            .heap()
            .borrow()
            .get_real(result)
            .expect("Expected Real");
        assert!(f.is_infinite() && f.is_sign_positive());
    }

    #[test]
    fn test_parse_negative_infinity() {
        let mut parser = Parser::new("-inf.0").unwrap();
        let result = parser.parse().unwrap();
        let f = parser
            .heap()
            .borrow()
            .get_real(result)
            .expect("Expected Real");
        assert!(f.is_infinite() && f.is_sign_negative());
    }

    #[test]
    fn test_parse_nan() {
        let mut parser = Parser::new("+nan.0").unwrap();
        let result = parser.parse().unwrap();
        let f = parser
            .heap()
            .borrow()
            .get_real(result)
            .expect("Expected Real");
        assert!(f.is_nan());
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
            let f = parser
                .heap()
                .borrow()
                .get_real(result)
                .unwrap_or_else(|| panic!("For input '{}': expected Real", input));
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

        // Test NaN separately (can't use == for NaN)
        let nan_cases = ["+NAN.0", "+Nan.0", "+nAn.0", "-NAN.0", "-nan.0"];
        for input in nan_cases {
            let mut parser = Parser::new(input).unwrap();
            let result = parser.parse().unwrap();
            let f = parser
                .heap()
                .borrow()
                .get_real(result)
                .unwrap_or_else(|| panic!("For input '{}': expected Real (NaN)", input));
            assert!(f.is_nan(), "For input '{}': expected NaN, got {}", input, f);
        }
    }

    #[test]
    fn test_parse_infinity_in_expression() {
        let mut parser = Parser::new("(+ +inf.0 1)").unwrap();
        let result = parser.parse().unwrap();

        assert!(result.is_pair());
        let heap = parser.heap();
        let heap_ref = heap.borrow();
        let car = heap_ref.car(result);
        let cdr = heap_ref.cdr(result);
        assert!(heap_ref.get_symbol_name(car).is_some()); // The '+'

        assert!(cdr.is_pair());
        let arg1 = heap_ref.car(cdr);
        let f = heap_ref
            .get_real(arg1)
            .expect("Expected Real for first arg");
        assert!(f.is_infinite() && f.is_sign_positive());
    }

    // ========== Datum Comment Tests ==========

    #[test]
    fn test_datum_comment_simple() {
        // #; abc def -> def
        let mut parser = Parser::new("#; abc def").unwrap();
        let result = parser.parse().unwrap();
        let heap_ref = parser.heap().borrow();
        assert_eq!(heap_ref.get_symbol_name(result), Some("def"));
    }

    #[test]
    fn test_datum_comment_in_list() {
        // (#;sqrt abs -16) -> (abs -16)
        let mut parser = Parser::new("(#;sqrt abs -16)").unwrap();
        let result = parser.parse().unwrap();

        // Should be a list with two elements: abs and -16
        assert!(result.is_pair());
        let heap_ref = parser.heap().borrow();
        let car = heap_ref.car(result);
        assert_eq!(heap_ref.get_symbol_name(car), Some("abs"));
    }

    #[test]
    fn test_datum_comment_multiple() {
        // (a #; #;b c d) -> (a d)
        // First #; skips b, second #; skips c
        let mut parser = Parser::new("(a #; #;b c d)").unwrap();
        let result = parser.parse().unwrap();

        // Should be a list (a d)
        assert!(result.is_pair());
        let heap_ref = parser.heap().borrow();
        assert_eq!(heap_ref.get_symbol_name(heap_ref.car(result)), Some("a"));

        let cdr = heap_ref.cdr(result);
        assert!(cdr.is_pair());
        assert_eq!(heap_ref.get_symbol_name(heap_ref.car(cdr)), Some("d"));
        assert!(heap_ref.cdr(cdr).is_null());
    }

    #[test]
    fn test_datum_comment_before_closing_paren() {
        // (a b #;c) -> (a b) — found by the compat corpus: srfi-14's
        // reference implementation comments out a trailing argument, which
        // transitively blocked 19 vendored packages.
        let mut parser = Parser::new("(a b #;c)").unwrap();
        let result = parser.parse().unwrap();

        let heap_ref = parser.heap().borrow();
        assert_eq!(heap_ref.get_symbol_name(heap_ref.car(result)), Some("a"));
        let cdr = heap_ref.cdr(result);
        assert_eq!(heap_ref.get_symbol_name(heap_ref.car(cdr)), Some("b"));
        assert!(heap_ref.cdr(cdr).is_null());
    }

    #[test]
    fn test_datum_comment_only_list_is_empty() {
        // (#;a) -> ()
        let mut parser = Parser::new("(#;a)").unwrap();
        let result = parser.parse().unwrap();
        assert!(result.is_null());
    }

    #[test]
    fn test_datum_comment_before_vector_close() {
        // #(1 #;2) -> #(1)
        let mut parser = Parser::new("#(1 #;2)").unwrap();
        let result = parser.parse().unwrap();
        let heap_ref = parser.heap().borrow();
        assert_eq!(heap_ref.vector_len(result), 1);
    }

    #[test]
    fn test_datum_comment_in_bytevector() {
        // #u8(1 #;2 3) -> #u8(1 3)
        let mut parser = Parser::new("#u8(1 #;2 3)").unwrap();
        let result = parser.parse().unwrap();
        let heap_ref = parser.heap().borrow();
        let bytes = heap_ref.get_bytevector(result).unwrap();
        assert_eq!(bytes, &[1, 3]);
    }

    #[test]
    fn test_datum_comment_nested_list() {
        // (a #;(b #;c d) e) -> (a e)
        let mut parser = Parser::new("(a #;(b #;c d) e)").unwrap();
        let result = parser.parse().unwrap();

        assert!(result.is_pair());
        let heap_ref = parser.heap().borrow();
        assert_eq!(heap_ref.get_symbol_name(heap_ref.car(result)), Some("a"));

        let cdr = heap_ref.cdr(result);
        assert!(cdr.is_pair());
        assert_eq!(heap_ref.get_symbol_name(heap_ref.car(cdr)), Some("e"));
        assert!(heap_ref.cdr(cdr).is_null());
    }

    #[test]
    fn test_datum_comment_dotted_list() {
        // (a . #;b c) -> (a . c)
        let mut parser = Parser::new("(a . #;b c)").unwrap();
        let result = parser.parse().unwrap();

        assert!(result.is_pair());
        let heap_ref = parser.heap().borrow();
        assert_eq!(heap_ref.get_symbol_name(heap_ref.car(result)), Some("a"));
        assert_eq!(heap_ref.get_symbol_name(heap_ref.cdr(result)), Some("c"));
    }

    #[test]
    fn test_datum_comment_before_tail() {
        // (a . b #;c) -> This should skip c after the dotted tail
        // According to tests: this should return (a . b) with c skipped
        let mut parser = Parser::new("(a . b #;c)").unwrap();
        let result = parser.parse().unwrap();

        assert!(result.is_pair());
        let heap_ref = parser.heap().borrow();
        assert_eq!(heap_ref.get_symbol_name(heap_ref.car(result)), Some("a"));
        assert_eq!(heap_ref.get_symbol_name(heap_ref.cdr(result)), Some("b"));
    }

    #[test]
    fn test_datum_comment_with_line_comment() {
        // #; ; comment\n def ghi -> ghi
        // The #; comments out the first datum after it, which is 'def'
        // (the ; comment is just a line comment consumed as whitespace)
        let mut parser = Parser::new("#; ; comment\n def ghi").unwrap();
        let result = parser.parse().unwrap();
        let heap_ref = parser.heap().borrow();
        assert_eq!(heap_ref.get_symbol_name(result), Some("ghi"));
    }

    #[test]
    fn test_datum_comment_vector() {
        // #;#(1 2 3) 42 -> 42
        let mut parser = Parser::new("#;#(1 2 3) 42").unwrap();
        let result = parser.parse().unwrap();
        assert!(result.is_fixnum());
        assert_eq!(result.as_fixnum_unchecked(), 42);
    }

    #[test]
    fn test_datum_comment_quoted() {
        // #;'foo bar -> bar
        let mut parser = Parser::new("#;'foo bar").unwrap();
        let result = parser.parse().unwrap();
        let heap_ref = parser.heap().borrow();
        assert_eq!(heap_ref.get_symbol_name(result), Some("bar"));
    }

    // ========== Datum Comment Error Cases ==========
    // These test cases correspond to R7RS read errors that require guard to test
    // at the Scheme level. By testing at the Rust level, we ensure correct behavior.

    #[test]
    fn test_datum_comment_error_dotted_head() {
        // (#;a . b) -> error: no datum before dot
        let mut parser = Parser::new("(#;a . b)").unwrap();
        let result = parser.parse();
        assert!(
            result.is_err(),
            "Expected error for (#;a . b), got {:?}",
            result
        );
    }

    #[test]
    fn test_datum_comment_error_dotted_tail() {
        // (a . #;b) -> error: no datum after dot
        let mut parser = Parser::new("(a . #;b)").unwrap();
        let result = parser.parse();
        assert!(
            result.is_err(),
            "Expected error for (a . #;b), got {:?}",
            result
        );
    }

    #[test]
    fn test_datum_comment_error_commenting_dot() {
        // (a #;. b) -> error: dot is not a datum
        let mut parser = Parser::new("(a #;. b)").unwrap();
        let result = parser.parse();
        assert!(
            result.is_err(),
            "Expected error for (a #;. b), got {:?}",
            result
        );
    }

    #[test]
    fn test_datum_comment_error_multiple_before_dot() {
        // (#;x #;y . z) -> error: no datum before dot
        let mut parser = Parser::new("(#;x #;y . z)").unwrap();
        let result = parser.parse();
        assert!(
            result.is_err(),
            "Expected error for (#;x #;y . z), got {:?}",
            result
        );
    }

    #[test]
    fn test_datum_comment_error_nested_before_dot() {
        // (#; #;x #;y . z) -> error: no datum before dot
        let mut parser = Parser::new("(#; #;x #;y . z)").unwrap();
        let result = parser.parse();
        assert!(
            result.is_err(),
            "Expected error for (#; #;x #;y . z), got {:?}",
            result
        );
    }

    #[test]
    fn test_datum_comment_error_nested_dotted() {
        // (#; #;x . z) -> error: missing datum after nested #;
        let mut parser = Parser::new("(#; #;x . z)").unwrap();
        let result = parser.parse();
        assert!(
            result.is_err(),
            "Expected error for (#; #;x . z), got {:?}",
            result
        );
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
            let f = parser
                .heap()
                .borrow()
                .get_real(result)
                .unwrap_or_else(|| panic!("For input '{}': expected Real", input));
            assert!(
                (f - expected).abs() < 1e-10,
                "For input '{}': expected {}, got {}",
                input,
                expected,
                f
            );
        }
    }

    #[test]
    fn test_standard_exponent_still_works() {
        // Make sure we didn't break standard 'e' exponent
        let mut parser = Parser::new("1e2").unwrap();
        let result = parser.parse().unwrap();
        let f = parser
            .heap()
            .borrow()
            .get_real(result)
            .expect("Expected Real");
        assert!((f - 100.0).abs() < 1e-10);
    }

    // ========== Non-Decimal Radix Rational Tests ==========

    #[test]
    fn test_hex_rational() {
        // #x1/10 = 1/16 in decimal
        let mut parser = Parser::new("#x1/10").unwrap();
        let result = parser.parse().unwrap();
        let heap_ref = parser.heap().borrow();
        let r = heap_ref.get_rational(result).expect("Expected Rational");
        assert_eq!(r.numer().to_i64(), Some(1));
        assert_eq!(r.denom().to_i64(), Some(16));
    }

    #[test]
    fn test_hex_rational_simplifies_to_integer() {
        // #x10/2 = 16/2 = 8
        let mut parser = Parser::new("#x10/2").unwrap();
        let result = parser.parse().unwrap();
        assert!(result.is_fixnum());
        assert_eq!(result.as_fixnum_unchecked(), 8);
    }

    #[test]
    fn test_hex_rational_simplifies() {
        // #x11/2 = 17/2
        let mut parser = Parser::new("#x11/2").unwrap();
        let result = parser.parse().unwrap();
        let heap_ref = parser.heap().borrow();
        let r = heap_ref.get_rational(result).expect("Expected Rational");
        assert_eq!(r.numer().to_i64(), Some(17));
        assert_eq!(r.denom().to_i64(), Some(2));
    }

    #[test]
    fn test_octal_rational() {
        // #o11/2 = 9/2 in decimal
        let mut parser = Parser::new("#o11/2").unwrap();
        let result = parser.parse().unwrap();
        let heap_ref = parser.heap().borrow();
        let r = heap_ref.get_rational(result).expect("Expected Rational");
        assert_eq!(r.numer().to_i64(), Some(9));
        assert_eq!(r.denom().to_i64(), Some(2));
    }

    #[test]
    fn test_binary_rational() {
        // #b11/10 = 3/2 in decimal
        let mut parser = Parser::new("#b11/10").unwrap();
        let result = parser.parse().unwrap();
        let heap_ref = parser.heap().borrow();
        let r = heap_ref.get_rational(result).expect("Expected Rational");
        assert_eq!(r.numer().to_i64(), Some(3));
        assert_eq!(r.denom().to_i64(), Some(2));
    }

    #[test]
    fn test_hex_rational_with_letters() {
        // #xa/b = 10/11
        let mut parser = Parser::new("#xa/b").unwrap();
        let result = parser.parse().unwrap();
        let heap_ref = parser.heap().borrow();
        let r = heap_ref.get_rational(result).expect("Expected Rational");
        assert_eq!(r.numer().to_i64(), Some(10));
        assert_eq!(r.denom().to_i64(), Some(11));
    }

    #[test]
    fn test_hex_rational_zero_denominator_error() {
        // #x1/0 should error
        let mut parser = Parser::new("#x1/0").unwrap();
        let result = parser.parse();
        assert!(result.is_err());
    }

    // ========== Complex Numbers with Alternate Exponent Markers ==========

    #[test]
    fn test_complex_with_short_exponent_marker() {
        // 1s2+1.0i = 100.0+1.0i
        let mut parser = Parser::new("1s2+1.0i").unwrap();
        let result = parser.parse().unwrap();
        let heap_ref = parser.heap().borrow();
        let (real_tv, imag_tv) = heap_ref.get_complex(result).expect("Expected Complex");
        let real = heap_ref.get_real(real_tv).expect("Expected real part");
        let imag = heap_ref.get_real(imag_tv).expect("Expected imag part");
        assert!(
            (real - 100.0).abs() < 1e-10,
            "Expected real 100.0, got {}",
            real
        );
        assert!(
            (imag - 1.0).abs() < 1e-10,
            "Expected imag 1.0, got {}",
            imag
        );
    }

    #[test]
    fn test_complex_with_exponent_in_imag() {
        // 1.0+1s2i = 1.0+100.0i
        let mut parser = Parser::new("1.0+1s2i").unwrap();
        let result = parser.parse().unwrap();
        let heap_ref = parser.heap().borrow();
        let (real_tv, imag_tv) = heap_ref.get_complex(result).expect("Expected Complex");
        let real = heap_ref.get_real(real_tv).expect("Expected real part");
        let imag = heap_ref.get_real(imag_tv).expect("Expected imag part");
        assert!(
            (real - 1.0).abs() < 1e-10,
            "Expected real 1.0, got {}",
            real
        );
        assert!(
            (imag - 100.0).abs() < 1e-10,
            "Expected imag 100.0, got {}",
            imag
        );
    }

    #[test]
    fn test_complex_with_double_exponent_marker() {
        // 1d2+1.0i = 100.0+1.0i (double precision marker)
        let mut parser = Parser::new("1d2+1.0i").unwrap();
        let result = parser.parse().unwrap();
        let heap_ref = parser.heap().borrow();
        let (real_tv, imag_tv) = heap_ref.get_complex(result).expect("Expected Complex");
        let real = heap_ref.get_real(real_tv).expect("Expected real part");
        let imag = heap_ref.get_real(imag_tv).expect("Expected imag part");
        assert!(
            (real - 100.0).abs() < 1e-10,
            "Expected real 100.0, got {}",
            real
        );
        assert!(
            (imag - 1.0).abs() < 1e-10,
            "Expected imag 1.0, got {}",
            imag
        );
    }

    #[test]
    fn test_complex_with_long_exponent_marker() {
        // 1l2+3.0i = 100.0+3.0i (long precision marker)
        let mut parser = Parser::new("1l2+3.0i").unwrap();
        let result = parser.parse().unwrap();
        let heap_ref = parser.heap().borrow();
        let (real_tv, imag_tv) = heap_ref.get_complex(result).expect("Expected Complex");
        let real = heap_ref.get_real(real_tv).expect("Expected real part");
        let imag = heap_ref.get_real(imag_tv).expect("Expected imag part");
        assert!(
            (real - 100.0).abs() < 1e-10,
            "Expected real 100.0, got {}",
            real
        );
        assert!(
            (imag - 3.0).abs() < 1e-10,
            "Expected imag 3.0, got {}",
            imag
        );
    }

    #[test]
    fn test_polar_with_exponent_marker() {
        // 1s2@0 = 100.0+0.0i (polar notation with exponent marker)
        let mut parser = Parser::new("1s2@0").unwrap();
        let result = parser.parse().unwrap();
        let heap_ref = parser.heap().borrow();
        let (real_tv, imag_tv) = heap_ref.get_complex(result).expect("Expected Complex");
        let real = heap_ref.get_real(real_tv).expect("Expected real part");
        let imag = heap_ref.get_real(imag_tv).expect("Expected imag part");
        assert!(
            (real - 100.0).abs() < 1e-10,
            "Expected real 100.0, got {}",
            real
        );
        assert!(imag.abs() < 1e-10, "Expected imag ~0, got {}", imag);
    }

    // ========== Datum Label Tests (R7RS Section 2.4) ==========

    #[test]
    fn test_datum_label_simple() {
        // #0=42 -> just returns 42, label is stored but not needed
        let mut parser = Parser::new("#0=42").unwrap();
        let result = parser.parse().unwrap();
        assert!(result.is_fixnum());
        assert_eq!(result.as_fixnum_unchecked(), 42);
    }

    #[test]
    fn test_datum_label_shared_structure() {
        // (#0=(a b) #0#) -> two references to the same list
        let mut parser = Parser::new("(#0=(a b) #0#)").unwrap();
        let result = parser.parse().unwrap();

        // HeapPair structure - shared structure via same TaggedValue (raw pointer)
        assert!(result.is_pair());
        let heap = parser.heap();
        let heap_ref = heap.borrow();
        let first = heap_ref.car(result);
        let second_cell = heap_ref.cdr(result);

        // First element should be (a b)
        assert!(first.is_pair());
        assert_eq!(heap_ref.get_symbol_name(heap_ref.car(first)), Some("a"));

        // Second element of outer list should also be the same (a b)
        assert!(second_cell.is_pair());
        let second = heap_ref.car(second_cell);
        assert!(second.is_pair());

        // Check pointer equality - both should be the same TaggedValue
        assert_eq!(
            first.raw(),
            second.raw(),
            "Shared structure should use same heap pointer"
        );
    }

    #[test]
    fn test_datum_label_cyclic() {
        // #0=(1 . #0#) -> creates a cycle
        let mut parser = Parser::new("#0=(1 . #0#)").unwrap();
        let result = parser.parse().unwrap();

        assert!(result.is_pair());
        let heap = parser.heap();
        let heap_ref = heap.borrow();
        let car = heap_ref.car(result);
        let cdr = heap_ref.cdr(result);

        assert!(car.is_fixnum());
        assert_eq!(car.as_fixnum_unchecked(), 1);

        // The cdr should be eq? to the pair itself (cycle)
        assert!(cdr.is_pair());
        assert_eq!(
            result.raw(),
            cdr.raw(),
            "Cyclic structure: cdr should point to self"
        );
    }

    #[test]
    fn test_datum_label_forward_reference() {
        // (#0# #0=(a b)) -> forward reference resolved
        let mut parser = Parser::new("(#0# #0=(a b))").unwrap();
        let result = parser.parse().unwrap();

        assert!(result.is_pair());
        let heap = parser.heap();
        let heap_ref = heap.borrow();
        let first = heap_ref.car(result);
        let rest = heap_ref.cdr(result);

        // Both elements should be the same (a b) list
        assert!(first.is_pair());
        assert!(rest.is_pair());
        let second = heap_ref.car(rest);
        assert!(second.is_pair());

        assert_eq!(
            first.raw(),
            second.raw(),
            "Forward reference should resolve to same object"
        );
    }

    #[test]
    fn test_datum_label_multiple() {
        // (#0=a #1=b #0# #1#) -> (a b a b)
        let mut parser = Parser::new("(#0=a #1=b #0# #1#)").unwrap();
        let result = parser.parse().unwrap();

        // Collect all symbols from the list
        let heap = parser.heap();
        let heap_ref = heap.borrow();
        let mut symbols = Vec::new();
        let mut current = result;
        while current.is_pair() {
            if let Some(name) = heap_ref.get_symbol_name(heap_ref.car(current)) {
                symbols.push(name.to_string());
            }
            current = heap_ref.cdr(current);
        }

        assert_eq!(symbols, vec!["a", "b", "a", "b"]);
    }

    #[test]
    fn test_datum_label_duplicate_error() {
        // #0=a #0=b -> error: duplicate label
        let mut parser = Parser::new("(#0=a #0=b)").unwrap();
        let result = parser.parse();
        assert!(
            matches!(result, Err(ParseError::DuplicateLabel(0))),
            "Expected DuplicateLabel error, got {:?}",
            result
        );
    }

    #[test]
    fn test_datum_label_undefined() {
        // #99# -> undefined label (no error at parse time, placeholder left)
        // Note: Our implementation leaves placeholders for undefined labels
        // which is technically valid per R7RS (implementation-defined)
        let mut parser = Parser::new("#99#").unwrap();
        let result = parser.parse().unwrap();
        // Should return a placeholder since label 99 is never defined
        let display = patina_core::format_tagged(result, &parser.heap().borrow());
        assert!(
            display.contains("label-placeholder") && display.contains("99"),
            "Expected LabelPlaceholder(99), got {}",
            display
        );
    }

    #[test]
    fn test_datum_label_in_vector() {
        // #(#0=1 #0# #0#) -> vector with shared elements
        let mut parser = Parser::new("#(#0=1 #0# #0#)").unwrap();
        let result = parser.parse().unwrap();

        assert!(result.is_vector());
        let heap_ref = parser.heap().borrow();
        let len = heap_ref.vector_len(result);
        assert_eq!(len, 3);
        for i in 0..len {
            let elem = heap_ref.vector_ref(result, i);
            assert!(elem.is_fixnum());
            assert_eq!(elem.as_fixnum_unchecked(), 1);
        }
    }

    #[test]
    fn test_datum_label_nested() {
        // #0=(a #1=(b c) #1#) -> nested shared structure
        let mut parser = Parser::new("#0=(a #1=(b c) #1#)").unwrap();
        let result = parser.parse().unwrap();

        // Verify the nested list (b c) is shared
        assert!(result.is_pair());
        let heap = parser.heap();
        let heap_ref = heap.borrow();
        let rest1 = heap_ref.cdr(result);

        // Get second element (should be (b c))
        assert!(rest1.is_pair());
        let inner1 = heap_ref.car(rest1);
        assert!(inner1.is_pair());

        // Get third element (should also be (b c))
        let rest2 = heap_ref.cdr(rest1);
        assert!(rest2.is_pair());
        let inner2 = heap_ref.car(rest2);
        assert!(inner2.is_pair());

        assert_eq!(
            inner1.raw(),
            inner2.raw(),
            "Nested shared structure should use same heap pointer"
        );
    }

    #[test]
    fn test_source_map_list() {
        let heap = patina_core::new_shared_heap();
        let sm = Rc::new(RefCell::new(crate::source_map::SourceMap::new()));
        let mut parser =
            Parser::new_with_source_map("(+ 1 2)", heap.clone(), Rc::from("test.scm"), sm.clone())
                .unwrap();
        let result = parser.parse().unwrap();
        let loc = sm.borrow().get(result).cloned();
        assert!(loc.is_some(), "list form should have source location");
        let loc = loc.unwrap();
        assert_eq!(loc.line, 1);
        assert_eq!(loc.column, 1);
        assert_eq!(&*loc.source, "test.scm");
    }

    #[test]
    fn test_source_map_nested_lists() {
        let heap = patina_core::new_shared_heap();
        let sm = Rc::new(RefCell::new(crate::source_map::SourceMap::new()));
        let mut parser = Parser::new_with_source_map(
            "(define (f x) (+ x 1))",
            heap.clone(),
            Rc::from("test.scm"),
            sm.clone(),
        )
        .unwrap();
        let result = parser.parse().unwrap();
        // Outer list starts at column 1
        let loc = sm.borrow().get(result).cloned().unwrap();
        assert_eq!(loc.line, 1);
        assert_eq!(loc.column, 1);

        // Find inner (+ x 1) — it's the third element: (define (f x) (+ x 1))
        let heap_ref = heap.borrow();
        let cdr1 = heap_ref.cdr(result); // ((f x) (+ x 1))
        let cdr2 = heap_ref.cdr(cdr1); // ((+ x 1))
        let inner = heap_ref.car(cdr2); // (+ x 1)
        drop(heap_ref);
        let inner_loc = sm.borrow().get(inner).cloned().unwrap();
        assert_eq!(inner_loc.line, 1);
        assert_eq!(inner_loc.column, 15); // position of `(` in `(+ x 1)`
    }

    #[test]
    fn test_source_map_quote_abbreviation() {
        let heap = patina_core::new_shared_heap();
        let sm = Rc::new(RefCell::new(crate::source_map::SourceMap::new()));
        let mut parser =
            Parser::new_with_source_map("'(1 2 3)", heap.clone(), Rc::from("test.scm"), sm.clone())
                .unwrap();
        let result = parser.parse().unwrap();
        // The quote abbreviation should record position of the `'`
        let loc = sm.borrow().get(result).cloned();
        assert!(loc.is_some(), "quote form should have source location");
        let loc = loc.unwrap();
        assert_eq!(loc.line, 1);
        assert_eq!(loc.column, 1);
    }

    #[test]
    fn test_source_map_vector() {
        let heap = patina_core::new_shared_heap();
        let sm = Rc::new(RefCell::new(crate::source_map::SourceMap::new()));
        let mut parser =
            Parser::new_with_source_map("#(1 2 3)", heap.clone(), Rc::from("test.scm"), sm.clone())
                .unwrap();
        let result = parser.parse().unwrap();
        let loc = sm.borrow().get(result).cloned();
        assert!(loc.is_some(), "vector should have source location");
        let loc = loc.unwrap();
        assert_eq!(loc.line, 1);
        assert_eq!(loc.column, 1);
    }

    #[test]
    fn test_source_map_multiline() {
        let heap = patina_core::new_shared_heap();
        let sm = Rc::new(RefCell::new(crate::source_map::SourceMap::new()));
        let mut parser = Parser::new_with_source_map(
            "(define x 1)\n(+ x 2)",
            heap.clone(),
            Rc::from("test.scm"),
            sm.clone(),
        )
        .unwrap();
        let expr1 = parser.parse().unwrap();
        let expr2 = parser.parse().unwrap();
        let loc1 = sm.borrow().get(expr1).cloned().unwrap();
        let loc2 = sm.borrow().get(expr2).cloned().unwrap();
        assert_eq!(loc1.line, 1);
        assert_eq!(loc1.column, 1);
        assert_eq!(loc2.line, 2);
        assert_eq!(loc2.column, 1);
    }

    #[test]
    fn test_no_source_map_backward_compat() {
        // Without source map, everything still works
        let mut parser = Parser::new("(+ 1 2)").unwrap();
        let result = parser.parse().unwrap();
        assert!(result.is_pair());
    }
}
