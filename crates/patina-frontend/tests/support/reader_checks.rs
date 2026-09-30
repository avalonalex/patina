//! Shared by the ordinary property tests and the libFuzzer target. No writer
//! is used here: formatting arbitrary-depth parsed data can itself recurse.

use patina_core::{Heap, Port, SharedHeap, TaggedValue, new_shared_heap};
use patina_frontend::{Parser, Reader, dialect};
use std::collections::HashSet;

#[derive(Default)]
struct Outcome {
    values: Vec<TaggedValue>,
    rejected: bool,
}

// Compare reader-produced graphs without recursion, and remember cycles from
// the first visit. The runtime's equal? deliberately defers this bookkeeping
// until a million visits, too expensive for a fuzz loop over tiny cycles.
fn equivalent(heap: &Heap, a: TaggedValue, b: TaggedValue) -> bool {
    let mut work = vec![(a, b)];
    let mut seen = HashSet::new();
    while let Some((a, b)) = work.pop() {
        if a == b || !seen.insert((a.raw(), b.raw())) {
            continue;
        }
        if a.is_pair() && b.is_pair() {
            work.extend([(heap.car(a), heap.car(b)), (heap.cdr(a), heap.cdr(b))]);
        } else if a.is_vector() && b.is_vector() {
            if heap.vector_len(a) != heap.vector_len(b) {
                return false;
            }
            work.extend(
                heap.vector_slice(a)
                    .iter()
                    .copied()
                    .zip(heap.vector_slice(b).iter().copied()),
            );
        } else if !heap.tagged_values_equal(a, b) {
            return false;
        }
    }
    true
}

fn compare(heap: &Heap, expected: &Outcome, actual: &Outcome, route: &str, text: &str) {
    assert_eq!(
        expected.rejected, actual.rejected,
        "{route}: rejection differs for {text:?}"
    );
    assert_eq!(
        expected.values.len(),
        actual.values.len(),
        "{route}: datum count differs for {text:?}"
    );
    for (index, (&a, &b)) in expected.values.iter().zip(&actual.values).enumerate() {
        assert!(
            equivalent(heap, a, b),
            "{route}: datum {index} differs for {text:?}"
        );
    }
}

fn drain(reader: &mut Reader, heap: &SharedHeap, result: &mut Outcome) {
    if result.rejected {
        return;
    }
    while let Some(value) = reader.next_datum(heap, |parser| parser) {
        match value {
            Ok(value) => result.values.push(value),
            Err(_) => {
                result.rejected = true;
                break;
            }
        }
    }
    // Also exercise callers that discard consumed input between feeds.
    reader.take_consumed();
}

/// Compare successful prefixes and accept/reject outcomes in whole-text,
/// chunk-fed and repeated port reads. Errors need not have identical wording.
/// Chunk lengths are 1..=256 *characters*, never invalid UTF-8 fragments.
pub fn check_reader(text: &str, chunks: &[u8]) {
    let heap = new_shared_heap();
    let mut whole = Outcome::default();
    match Parser::new_with_heap(text, heap.clone()) {
        Ok(mut parser) => loop {
            match parser.parse_next() {
                Ok(Some(value)) => whole.values.push(value),
                Ok(None) => break,
                Err(_) => {
                    whole.rejected = true;
                    break;
                }
            }
        },
        Err(_) => whole.rejected = true,
    }

    let all = Parser::new_with_heap(text, heap.clone()).and_then(|mut p| p.parse_all());
    assert_eq!(
        all.is_err(),
        whole.rejected,
        "parse_all: rejection differs for {text:?}"
    );
    if let Ok(values) = all {
        compare(
            &heap.borrow(),
            &whole,
            &Outcome {
                values,
                rejected: false,
            },
            "parse_all",
            text,
        );
    }

    let mut fed = Outcome::default();
    let mut reader = Reader::new(dialect::allow_r6rs());
    let mut rest = text;
    let mut piece = 0;
    while !rest.is_empty() && !fed.rejected {
        let size = usize::from(
            chunks
                .get(piece % chunks.len().max(1))
                .copied()
                .unwrap_or(0),
        ) + 1;
        let end = rest
            .char_indices()
            .nth(size)
            .map_or(rest.len(), |(byte, _)| byte);
        reader.feed(&rest[..end]);
        // Interactive callers trial EOF between feeds. It must not consume
        // lookahead or change folding, even inside an escape or comment.
        reader.inside_datum();
        drain(&mut reader, &heap, &mut fed);
        rest = &rest[end..];
        piece += 1;
    }
    if !fed.rejected {
        reader.no_more_text();
        drain(&mut reader, &heap, &mut fed);
    }
    compare(&heap.borrow(), &whole, &fed, "fed", text);

    let port = Port::new_input_string(text.to_owned());
    let mut pulled = Outcome::default();
    loop {
        match Parser::read_from_port(port.clone(), heap.clone()) {
            Ok(Some(value)) => pulled.values.push(value),
            Ok(None) => break,
            Err(_) => {
                pulled.rejected = true;
                break;
            }
        }
    }
    compare(&heap.borrow(), &whole, &pulled, "port", text);
}
