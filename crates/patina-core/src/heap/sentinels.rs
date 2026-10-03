//! Sentinel values for the collector's field tests (#623).
//!
//! Every trace function names each field of its struct, so a new field does
//! not compile until someone decides how it is traced. That forces a
//! decision but cannot judge it: `resume: _ // aliased` compiles. A sentinel
//! test judges it. It builds the struct by a struct literal, so that a new
//! field breaks the test as well; puts a fresh value from here in every field
//! that can hold one; roots the struct and nothing else; collects; and asks
//! [`Sentinels::assert_survived`] whether each value is still there.
//!
//! The answer comes from the arenas' free lists, so it holds in every build
//! and names the field whose value was swept. The read that follows is the
//! stale-reference check's (#621) in a check build, and compares contents in
//! any build. A decoy, rooted nowhere, must be swept by the same collection,
//! so a test cannot pass because nothing was collected.
//!
//! Compiled for patina-core's own tests, and for other crates' tests under
//! the `test-support` feature, beside `collect_for_tests`.

use super::{Heap, HeapObjectData};
use crate::tagged_value::TaggedValue;

/// The fresh values a sentinel test hides in the structure under test.
pub struct Sentinels {
    made: Vec<Sentinel>,
    decoy: TaggedValue,
}

struct Sentinel {
    /// The field the value stands in, as the failure names it.
    field: String,
    value: TaggedValue,
}

impl Sentinels {
    /// Start a test's sentinels on `heap`, with the decoy the collection
    /// must reclaim.
    pub fn new(heap: &mut Heap) -> Self {
        let decoy = heap.alloc_pair(TaggedValue::fixnum(-1), TaggedValue::NULL);
        Sentinels {
            made: Vec::new(),
            decoy,
        }
    }

    /// The number the next sentinel carries, which says which one it is.
    fn next(&self) -> i64 {
        self.made.len() as i64
    }

    fn keep(&mut self, field: impl Into<String>, value: TaggedValue) -> TaggedValue {
        self.made.push(Sentinel {
            field: field.into(),
            value,
        });
        value
    }

    /// A fresh pair for `field`, whose car is its number.
    pub fn pair(&mut self, heap: &mut Heap, field: impl Into<String>) -> TaggedValue {
        let value = heap.alloc_pair(TaggedValue::fixnum(self.next()), TaggedValue::NULL);
        self.keep(field, value)
    }

    /// A fresh one-element vector for `field`, holding its number.
    pub fn vector(&mut self, heap: &mut Heap, field: impl Into<String>) -> TaggedValue {
        let value = heap.alloc_vector(vec![TaggedValue::fixnum(self.next())]);
        self.keep(field, value)
    }

    /// A fresh string for `field`, spelling its number.
    pub fn string(&mut self, heap: &mut Heap, field: impl Into<String>) -> TaggedValue {
        let value = heap.alloc_string(format!("sentinel {}", self.next()));
        self.keep(field, value)
    }

    /// A fresh object for `field`: a `values` object holding its number.
    pub fn object(&mut self, heap: &mut Heap, field: impl Into<String>) -> TaggedValue {
        let value = heap.alloc_values(vec![TaggedValue::fixnum(self.next())]);
        self.keep(field, value)
    }

    /// Assert, after a collection, that the decoy was swept and that every
    /// sentinel survived with its contents, naming the field of the first one
    /// that did not.
    pub fn assert_survived(&self, heap: &Heap) {
        assert!(
            on_a_free_list(heap, self.decoy),
            "the decoy survived the collection: nothing was collected, or the \
             test roots more than the structure under test"
        );
        assert!(!self.made.is_empty(), "a sentinel test with no sentinels");
        for (number, sentinel) in self.made.iter().enumerate() {
            let Sentinel { field, value } = sentinel;
            assert!(
                !on_a_free_list(heap, *value),
                "{field}: its sentinel was swept, so the trace does not reach this field"
            );
            let number = TaggedValue::fixnum(number as i64);
            let holds = if value.is_pair() {
                heap.car(*value)
            } else if value.is_vector() {
                heap.vector_ref(*value, 0)
            } else if value.is_string() {
                let spelled = heap.get_string_as_utf8(*value);
                assert_eq!(spelled, format!("sentinel {}", number.as_fixnum().unwrap()));
                number
            } else {
                match heap.get_object(*value) {
                    HeapObjectData::Values(values) => values[0],
                    other => panic!("{field}: its sentinel became {other:?}"),
                }
            };
            assert_eq!(holds, number, "{field}: its sentinel holds another value");
        }
    }
}

/// Whether `value`'s slot is on its arena's free list: swept by the last
/// collection and not reused since.
fn on_a_free_list(heap: &Heap, value: TaggedValue) -> bool {
    let index = value.heap_index();
    if value.is_pair() {
        heap.free_pairs.contains(&index)
    } else if value.is_vector() {
        heap.free_vectors.contains(&index)
    } else if value.is_string() {
        heap.free_strings.contains(&index)
    } else if value.is_object() {
        heap.free_objects.contains(&index)
    } else {
        false
    }
}
