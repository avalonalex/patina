//! #366: build data independently of the reader, then write/read/equal?.
//! Trees are bounded to depth 5 and about 64 nodes. Graphs contain 1..8 nodes
//! with pair/vector cycles and repeated references; write-simple gets trees
//! only. NaNs use the canonical payload: Scheme text cannot encode payloads.

use super::*;
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::ToPrimitive;
use patina_core::{SharedHeap, new_shared_heap};
use patina_frontend::Parser;
use proptest::prelude::*;
use proptest::test_runner::RngSeed;

#[path = "../../../../patina-frontend/tests/support/reader_checks.rs"]
mod reader_checks;

pub(super) fn config() -> ProptestConfig {
    ProptestConfig {
        rng_seed: RngSeed::Fixed(366),
        max_shrink_iters: 4096,
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

#[derive(Clone, Debug)]
enum Number {
    Integer(i128),
    Rational(i64, u32),
    Real(f64),
}

fn integer(heap: &mut Heap, value: BigInt) -> TaggedValue {
    match value.to_i64().filter(|&n| TaggedValue::fits_fixnum(n)) {
        Some(n) => TaggedValue::fixnum(n),
        None => heap.alloc_bigint(value),
    }
}

impl Number {
    fn allocate(&self, heap: &mut Heap) -> TaggedValue {
        match *self {
            Self::Integer(n) => integer(heap, BigInt::from(n)),
            Self::Rational(n, d) => {
                let r = BigRational::new(BigInt::from(n), BigInt::from(d));
                if r.is_integer() {
                    integer(heap, r.to_integer())
                } else {
                    heap.alloc_rational(r)
                }
            }
            Self::Real(f) => heap.alloc_real(f),
        }
    }
}

fn numbers() -> impl Strategy<Value = Number> {
    prop_oneof![
        any::<i128>().prop_map(Number::Integer),
        prop::sample::select(vec![
            0,
            1,
            -1,
            TaggedValue::FIXNUM_MIN as i128,
            TaggedValue::FIXNUM_MAX as i128
        ])
        .prop_map(Number::Integer),
        (any::<i64>(), 1u32..=u32::MAX).prop_map(|(n, d)| Number::Rational(n, d)),
        any::<u64>().prop_map(|bits| {
            let f = f64::from_bits(bits);
            Number::Real(if f.is_nan() { f64::NAN } else { f })
        }),
        prop::sample::select(vec![
            0.0,
            -0.0,
            1.0,
            -1.0,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NAN,
            f64::MIN_POSITIVE,
            f64::from_bits(1),
            f64::MAX
        ])
        .prop_map(Number::Real),
    ]
}

#[derive(Clone, Debug)]
enum Datum {
    Null,
    Bool(bool),
    Char(char),
    Symbol(String),
    String(String),
    Number(Number),
    Complex(Number, Number),
    Bytes(Vec<u8>),
    List(Vec<Datum>, Box<Datum>),
    Vector(Vec<Datum>),
}

impl Datum {
    fn allocate(&self, heap: &mut Heap) -> TaggedValue {
        match self {
            Self::Null => TaggedValue::NULL,
            Self::Bool(b) => TaggedValue::boolean(*b),
            Self::Char(c) => TaggedValue::character(*c),
            Self::Symbol(s) => heap.intern_symbol(s),
            Self::String(s) => heap.alloc_string(s.clone()),
            Self::Number(n) => n.allocate(heap),
            Self::Complex(r, i) => {
                let real = r.allocate(heap);
                let imag = i.allocate(heap);
                // Canonical exact-zero imaginary parts are real numbers.
                // Keep inexact signed zero: read can produce that complex.
                if heap.is_exact_zero(imag) {
                    real
                } else {
                    heap.alloc_complex(real, imag)
                }
            }
            Self::Bytes(b) => heap.alloc_bytevector(b.clone()),
            Self::List(values, tail) => {
                let mut result = tail.allocate(heap);
                for value in values.iter().rev() {
                    let car = value.allocate(heap);
                    result = heap.alloc_pair(car, result);
                }
                result
            }
            Self::Vector(values) => {
                let elements = values.iter().map(|v| v.allocate(heap)).collect();
                heap.alloc_vector(elements)
            }
        }
    }
}

fn leaves() -> impl Strategy<Value = Datum> {
    prop_oneof![
        Just(Datum::Null),
        any::<bool>().prop_map(Datum::Bool),
        any::<char>().prop_map(Datum::Char),
        prop::sample::select(vec![
            '\0',
            '\x07',
            '\x08',
            '\t',
            '\n',
            '\r',
            ' ',
            '\x1b',
            '\x7f',
            'λ',
            '\u{10ffff}'
        ])
        .prop_map(Datum::Char),
        super::tests::symbol_names().prop_map(Datum::Symbol),
        prop::collection::vec(any::<char>(), 0..80)
            .prop_map(|s| Datum::String(s.into_iter().collect())),
        Just(Datum::String("\0\x07\x08\t\n\r\x1b\"\\|λ\u{10ffff}".into())),
        numbers().prop_map(Datum::Number),
        (numbers(), numbers()).prop_map(|(r, i)| Datum::Complex(r, i)),
        prop::collection::vec(any::<u8>(), 0..80).prop_map(Datum::Bytes),
    ]
}

fn data() -> impl Strategy<Value = Datum> {
    leaves().prop_recursive(5, 64, 8, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..8).prop_map(Datum::Vector),
            prop::collection::vec(inner.clone(), 0..8)
                .prop_map(|v| Datum::List(v, Box::new(Datum::Null))),
            (prop::collection::vec(inner.clone(), 1..8), inner)
                .prop_map(|(v, tail)| Datum::List(v, Box::new(tail))),
        ]
    })
}

fn read_back(value: TaggedValue, text: &str, heap: &SharedHeap) -> TaggedValue {
    let mut parser = Parser::new_with_heap(text, heap.clone()).unwrap();
    let result = parser
        .parse_next()
        .unwrap_or_else(|e| panic!("reading {text:?}: {e}"))
        .expect("writer produced no datum");
    assert_eq!(
        parser.parse_next().unwrap(),
        None,
        "extra datum in {text:?}"
    );
    assert!(
        heap.borrow().tagged_values_equal(value, result),
        "round trip changed {text:?}"
    );
    result
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn generated_data_round_trips(datum in data(), chunks in prop::collection::vec(any::<u8>(), 0..16)) {
        let heap = new_shared_heap();
        let value = datum.allocate(&mut heap.borrow_mut());
        for writer in [format_write_tagged, format_write_simple_tagged, format_write_shared_tagged] {
            let text = writer(value, &heap);
            read_back(value, &text, &heap);

            // Writer output supplies valid nested inputs to the incremental
            // property as well as the malformed/random inputs in frontend.
            reader_checks::check_reader(&text, &chunks);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 32, ..config() })]

    #[test]
    fn shared_and_cyclic_graphs_round_trip(nodes in prop::collection::vec((any::<bool>(), any::<bool>(), any::<u8>(), any::<i32>()), 1..9)) {
        let heap = new_shared_heap();
        let value = {
            let mut h = heap.borrow_mut();
            let refs: Vec<_> = nodes.iter().map(|(vector, _, _, _)| {
                if *vector { h.alloc_vector_fill(3, TaggedValue::NULL) }
                else { h.alloc_pair(TaggedValue::NULL, TaggedValue::NULL) }
            }).collect();
            for (index, &(vector, through_car, edge, n)) in nodes.iter().enumerate() {
                let next = refs[(index + 1) % refs.len()];
                if vector {
                    h.vector_set(refs[index], 0, TaggedValue::fixnum(n.into()));
                    h.vector_set(refs[index], 1, next);
                    h.vector_set(refs[index], 2, refs[usize::from(edge) % refs.len()]);
                } else if through_car {
                    h.set_car(refs[index], next);
                    h.set_cdr(refs[index], TaggedValue::fixnum(n.into()));
                } else {
                    h.set_car(refs[index], TaggedValue::fixnum(n.into()));
                    h.set_cdr(refs[index], next);
                }
            }
            let shared = h.alloc_vector(vec![TaggedValue::fixnum(17)]);
            h.alloc_vector(vec![refs[0], refs[0], shared, shared])
        };
        for writer in [format_write_tagged, format_write_shared_tagged] {
            let text = writer(value, &heap);
            let result = read_back(value, &text, &heap);
            reader_checks::check_reader(&text, &[0, 2, 7]);
            let h = heap.borrow();
            // Both writers must preserve the reference into the cycle.
            prop_assert_eq!(h.vector_ref(result, 0), h.vector_ref(result, 1));
        }
        // write-shared additionally preserves repeated acyclic containers.
        let text = format_write_shared_tagged(value, &heap);
        let result = read_back(value, &text, &heap);
        let h = heap.borrow();
        prop_assert_eq!(h.vector_ref(result, 2), h.vector_ref(result, 3));
    }
}
