//! Token-driven datum construction. Pending containers and prefixes live on
//! the heap, not on the Rust call stack, including inside datum comments.

use super::{ParseError, Parser};
use crate::lexer::{Lexer, Token};
use patina_core::{SourceLocation, TaggedValue};

enum Tail {
    Elements,
    Needed,
    Complete(TaggedValue),
}

#[derive(Default)]
struct ContainerSources {
    elements: Vec<Option<SourceLocation>>,
    tail: Option<SourceLocation>,
}

// Specialize the same grammar for syntax and ordinary Scheme data. The latter
// carries no source fields in its stack frames and does no provenance work.
trait SourceMode {
    const TRACK: bool;
    type Sources: Default;
    fn take(sources: Self::Sources) -> Option<ContainerSources>;
    fn get(sources: &mut Self::Sources) -> Option<&mut ContainerSources>;
}

struct DatumMode;

impl SourceMode for DatumMode {
    const TRACK: bool = false;
    type Sources = ();
    fn take(_: ()) -> Option<ContainerSources> {
        None
    }
    fn get(_: &mut ()) -> Option<&mut ContainerSources> {
        None
    }
}

struct ProgramMode;

impl SourceMode for ProgramMode {
    const TRACK: bool = true;
    type Sources = Box<ContainerSources>;
    fn take(sources: Self::Sources) -> Option<ContainerSources> {
        Some(*sources)
    }
    fn get(sources: &mut Self::Sources) -> Option<&mut ContainerSources> {
        Some(sources)
    }
}

enum Frame<M: SourceMode> {
    List {
        elements: Vec<TaggedValue>,
        sources: M::Sources,
        tail: Tail,
        at: (u32, u32),
    },
    Vector {
        elements: Vec<TaggedValue>,
        sources: M::Sources,
        at: (u32, u32),
    },
    Bytes(Vec<u8>, (u32, u32)),
    Prefix {
        name: &'static str,
        at: (u32, u32),
    },
    Label(usize),
    Comment {
        was_discarding: bool,
    },
    // Discarded datums need their structure checked, but allocate no Scheme
    // values and neither define nor reference labels in the surrounding datum.
    SkippedList(Tail, (u32, u32)),
    SkippedPrefix,
}

impl<M: SourceMode> Frame<M> {
    fn opening(&self) -> Option<(u32, u32)> {
        match self {
            Self::List { at, .. }
            | Self::Vector { at, .. }
            | Self::Bytes(_, at)
            | Self::SkippedList(_, at) => Some(*at),
            _ => None,
        }
    }
}

impl Parser {
    pub(super) fn parse_expr(&mut self) -> Result<TaggedValue, ParseError> {
        self.read_datum(false)
    }

    pub(super) fn skip_datum(&mut self) -> Result<(), ParseError> {
        self.read_datum(true).map(|_| ())
    }

    fn read_datum(&mut self, discarding: bool) -> Result<TaggedValue, ParseError> {
        if self.source_map.is_some() {
            self.read_datum_in::<ProgramMode>(discarding)
        } else {
            self.read_datum_in::<DatumMode>(discarding)
        }
    }

    fn read_datum_in<M: SourceMode>(
        &mut self,
        discarding: bool,
    ) -> Result<TaggedValue, ParseError> {
        let mut frames = Vec::new();
        self.read_with_frames::<M>(discarding, &mut frames)
            .map_err(|error| {
                error
                    .at(self.current_span())
                    .within(frames.iter().rev().find_map(Frame::opening))
            })
    }

    fn read_with_frames<M: SourceMode>(
        &mut self,
        mut discarding: bool,
        frames: &mut Vec<Frame<M>>,
    ) -> Result<TaggedValue, ParseError> {
        'tokens: loop {
            if self.current_token == Token::DatumComment {
                frames.push(Frame::Comment {
                    was_discarding: discarding,
                });
                discarding = true;
                self.advance()?;
                continue;
            }
            if self.current_token == Token::Eof {
                return Err(self.incomplete_datum());
            }
            // A dotted tail is exactly one datum, followed by comments or ).
            if self.current_token != Token::RightParen
                && matches!(
                    frames.last(),
                    Some(
                        Frame::List {
                            tail: Tail::Complete(_),
                            ..
                        } | Frame::SkippedList(Tail::Complete(_), _)
                    )
                )
            {
                return Err(ParseError::UnexpectedToken(self.current_token.clone()));
            }
            // Bytevectors accept numeric tokens only, not arbitrary datums
            // that happen to produce numbers (labels or quote prefixes).
            if let Some(Frame::Bytes(bytes, _)) = frames.last_mut()
                && self.current_token != Token::RightParen
            {
                let Token::Number(number) = &self.current_token else {
                    return Err(ParseError::InvalidSyntax(
                        "Bytevector must contain only bytes (0-255)".to_string(),
                    ));
                };
                let value = self.parse_number(number)?;
                let Some(byte) = value.as_fixnum() else {
                    return Err(ParseError::InvalidSyntax(
                        "Bytevector must contain only integer bytes (0-255)".to_string(),
                    ));
                };
                if !(0..=255).contains(&byte) {
                    return Err(ParseError::InvalidSyntax(format!(
                        "Byte value out of range (0-255): {byte}"
                    )));
                }
                bytes.push(byte as u8);
                self.advance()?;
                continue;
            }

            let at = (self.current_token_line, self.current_token_column);
            let mut location = if M::TRACK {
                self.source_location(at.0, at.1)
            } else {
                None
            };
            let mut value = match self.current_token.clone() {
                Token::LeftParen | Token::VectorOpen | Token::BytevectorOpen => {
                    let frame = if discarding {
                        Frame::SkippedList(Tail::Elements, at)
                    } else {
                        match self.current_token {
                            Token::LeftParen => Frame::List {
                                elements: Vec::new(),
                                sources: M::Sources::default(),
                                tail: Tail::Elements,
                                at,
                            },
                            Token::VectorOpen => Frame::Vector {
                                elements: Vec::new(),
                                sources: M::Sources::default(),
                                at,
                            },
                            _ => Frame::Bytes(Vec::new(), at),
                        }
                    };
                    self.advance()?;
                    if !discarding {
                        self.nesting += 1;
                    }
                    frames.push(frame);
                    continue;
                }
                Token::RightParen => {
                    if matches!(
                        frames.last(),
                        Some(
                            Frame::List {
                                tail: Tail::Needed,
                                ..
                            } | Frame::SkippedList(Tail::Needed, _)
                        )
                    ) {
                        return Err(ParseError::UnexpectedToken(Token::RightParen));
                    }
                    let value = match frames.pop() {
                        Some(Frame::List {
                            elements,
                            sources,
                            tail,
                            at,
                        }) => {
                            let tail = match tail {
                                Tail::Elements => TaggedValue::NULL,
                                Tail::Complete(tail) => tail,
                                Tail::Needed => {
                                    return Err(ParseError::UnexpectedToken(Token::RightParen));
                                }
                            };
                            if M::TRACK {
                                location = self.source_location(at.0, at.1);
                            }
                            let mut heap = self.heap.borrow_mut();
                            let value = if let Some(sources) = M::take(sources) {
                                let mut out = tail;
                                let mut rest_source = sources.tail;
                                for (element, source) in
                                    elements.into_iter().zip(sources.elements).rev()
                                {
                                    out = heap.alloc_pair(element, out);
                                    heap.record_source_children(out, vec![source, rest_source]);
                                    rest_source = None;
                                }
                                out
                            } else {
                                heap.list_from_iter_with_tail(elements, tail)
                            };
                            drop(heap);
                            value
                        }
                        Some(Frame::Vector {
                            elements,
                            sources,
                            at,
                        }) => {
                            if M::TRACK {
                                location = self.source_location(at.0, at.1);
                            }
                            let value = self.heap.borrow_mut().alloc_vector(elements);
                            if let Some(sources) = M::take(sources) {
                                self.heap
                                    .borrow_mut()
                                    .record_source_children(value, sources.elements);
                            }
                            value
                        }
                        Some(Frame::Bytes(bytes, at)) => {
                            if M::TRACK {
                                location = self.source_location(at.0, at.1);
                            }
                            self.heap.borrow_mut().alloc_bytevector(bytes)
                        }
                        Some(Frame::SkippedList(Tail::Elements | Tail::Complete(_), _)) => {
                            TaggedValue::UNSPECIFIED
                        }
                        _ => return Err(ParseError::UnexpectedToken(Token::RightParen)),
                    };
                    if !discarding {
                        self.nesting -= 1;
                    }
                    value
                }
                Token::Dot => {
                    match frames.last_mut() {
                        Some(Frame::List { elements, tail, .. })
                            if !elements.is_empty() && matches!(tail, Tail::Elements) =>
                        {
                            *tail = Tail::Needed;
                        }
                        // Keep the existing skip reader's structural-only
                        // treatment of lists, vectors and bytevectors.
                        Some(Frame::SkippedList(tail, _)) if matches!(tail, Tail::Elements) => {
                            *tail = Tail::Needed;
                        }
                        _ => return Err(ParseError::UnexpectedToken(Token::Dot)),
                    }
                    self.advance()?;
                    continue;
                }
                Token::Quote | Token::Quasiquote | Token::Unquote | Token::UnquoteSplicing => {
                    frames.push(if discarding {
                        Frame::SkippedPrefix
                    } else {
                        let name = match self.current_token {
                            Token::Quote => "quote",
                            Token::Quasiquote => "quasiquote",
                            Token::Unquote => "unquote",
                            _ => "unquote-splicing",
                        };
                        Frame::Prefix { name, at }
                    });
                    self.advance()?;
                    continue;
                }
                Token::DatumLabel(label) => {
                    let span = self.current_span();
                    self.advance()?;
                    if discarding {
                        frames.push(Frame::SkippedPrefix);
                    } else {
                        if self.labels.contains_key(&label) {
                            return Err(ParseError::DuplicateLabel(label).at(span));
                        }
                        frames.push(Frame::Label(label));
                    }
                    continue;
                }
                // Even numeric conversion is unnecessary when discarding.
                _ if discarding => TaggedValue::UNSPECIFIED,
                Token::Boolean(value) => {
                    if value {
                        TaggedValue::TRUE
                    } else {
                        TaggedValue::FALSE
                    }
                }
                Token::Number(number) => match self.parse_number(&number) {
                    Ok(value) => value,
                    Err(_) if Lexer::is_peculiar_identifier(&number) => {
                        let name = Lexer::identifier_name(number, self.current_token_fold_case);
                        self.identifier(&name)
                    }
                    Err(error) => return Err(error),
                },
                Token::Character(value) => TaggedValue::character(value),
                Token::String(value) => self.heap.borrow_mut().alloc_string(value),
                Token::Identifier(value) => self.identifier(&value),
                Token::DatumRef(label) => {
                    if let Some(value) = self.labels.get(&label) {
                        *value
                    } else {
                        self.pending_refs.push((label, self.current_span()));
                        self.heap.borrow_mut().alloc_label_placeholder(label)
                    }
                }
                token => return Err(ParseError::UnexpectedToken(token)),
            };
            if M::TRACK
                && !discarding
                && let Some(loc) = &location
            {
                // A label reference is another edge to existing syntax. Keep
                // the identifier/container's original spelling location;
                // its parent's slot records the #n# occurrence separately.
                if !matches!(self.current_token, Token::DatumRef(_))
                    || self.heap.borrow().source(value).is_none()
                {
                    self.record_location(value, loc.clone());
                }
            }
            if discarding {
                self.advance()?;
            } else {
                self.advance_past_datum()?;
            }

            // Hand a finished datum to its parent. Prefixes finish another
            // datum; a comment drops one and still leaves its parent waiting.
            loop {
                match frames.last_mut() {
                    None => return Ok(value),
                    Some(Frame::List {
                        elements,
                        sources,
                        tail,
                        ..
                    }) => {
                        if matches!(tail, Tail::Needed) {
                            *tail = Tail::Complete(value);
                            if let Some(sources) = M::get(sources) {
                                sources.tail = location.clone();
                            }
                        } else {
                            elements.push(value);
                            if let Some(sources) = M::get(sources) {
                                sources.elements.push(location.clone());
                            }
                        }
                        continue 'tokens;
                    }
                    Some(Frame::Vector {
                        elements, sources, ..
                    }) => {
                        elements.push(value);
                        if let Some(sources) = M::get(sources) {
                            sources.elements.push(location.clone());
                        }
                        continue 'tokens;
                    }
                    Some(Frame::SkippedList(tail, _)) => {
                        if matches!(tail, Tail::Needed) {
                            *tail = Tail::Complete(value);
                        }
                        continue 'tokens;
                    }
                    _ => {}
                }
                match frames.pop().expect("a pending prefix") {
                    Frame::Prefix { name, at } => {
                        let symbol = self.heap.borrow_mut().intern_symbol(name);
                        value = self.make_list(vec![symbol, value]);
                        if M::TRACK
                            && let Some(loc) = &mut location
                        {
                            loc.line = at.0;
                            loc.column = at.1;
                            loc.length = loc.span.as_ref().map(|span| {
                                if span.end_line == at.0 {
                                    span.end_column.saturating_sub(at.1).max(1)
                                } else {
                                    1
                                }
                            });
                            self.record_location(value, loc.clone());
                        }
                    }
                    Frame::Label(label) => {
                        self.labels.insert(label, value);
                    }
                    Frame::SkippedPrefix => {}
                    Frame::Comment { was_discarding } => {
                        discarding = was_discarding;
                        continue 'tokens;
                    }
                    _ => unreachable!("containers receive values above"),
                }
            }
        }
    }
}
