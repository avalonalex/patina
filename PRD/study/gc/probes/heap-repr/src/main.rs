use std::cell::RefCell;
use std::mem::{size_of, align_of};
use std::rc::Rc;
use patina_core::heap::{HeapObjectData, PromiseState};
use patina_core::*;
use patina_vm::types::*;

macro_rules! sz { ($t:ty) => { println!("{:<60} size={:>4} align={}", stringify!($t), size_of::<$t>(), align_of::<$t>()); } }

fn main() {
    println!("rustc via toolchain; target {}", std::env::consts::ARCH);
    sz!(TaggedValue);
    sz!(HeapObjectData);
    sz!(Option<HeapObjectData>);
    sz!((TaggedValue, TaggedValue));
    sz!(Vec<TaggedValue>);
    sz!(Vec<char>);
    sz!(Vec<u8>);
    sz!(Rc<str>);
    sz!(String);
    println!("-- variant payloads --");
    sz!(num_bigint::BigInt);
    sz!(num_rational::BigRational);
    sz!(f64);
    sz!(ExceptionKind);
    sz!((ExceptionKind, String, Vec<TaggedValue>));
    sz!(Rc<Procedure>);
    sz!(Procedure);
    sz!(Port);
    sz!(CompiledMacro);
    sz!(RecordTypeDescriptor);
    sz!(Rc<RefCell<Vec<TaggedValue>>>);
    sz!(RefCell<Vec<TaggedValue>>);
    sz!(ScopeSet);
    sz!((Rc<str>, ScopeSet, bool));
    sz!(CpsContinuation);
    sz!((Rc<RefCell<Vec<TaggedValue>>>, Option<TaggedValue>));
    sz!(RefCell<PromiseState>);
    sz!(Library);
    sz!(Environment);
    sz!(PromptTag);
    sz!(RefCell<TaggedValue>);
    sz!(RefCell<Option<(TaggedValue, TaggedValue)>>);
    sz!((u64, Vec<TaggedValue>, Rc<Environment>));
    sz!(CoreForm);
    sz!(Heap);
    sz!(SourceLocation);
    sz!(CpsExpr);
    sz!(CoreExpr);
    println!("-- VM --");
    sz!(CallFrame);
    sz!(CodeObject);
    sz!(Instruction);
    sz!(VmContinuation);
    sz!(VmDelimitedContinuation);
    sz!(PromptFrame);
    sz!(ExceptionHandler);
    sz!(patina_vm::types::DynamicWindRecord);
    sz!(GlobalCacheEntry);

    // Per-object footprint estimates: allocate typical objects and report.
    let mut heap = Heap::new();
    let p = heap.alloc_pair(TaggedValue::fixnum(1), TaggedValue::NULL);
    let _ = p;
    let v = heap.alloc_vector(vec![TaggedValue::fixnum(1); 3]);
    let _ = v;
    let s = heap.alloc_str("hello");
    let _ = s;
    let r = heap.alloc_real(1.5);
    let _ = r;
    println!("stats {:?}", heap.stats());
    replica_sizes();
}

#[allow(dead_code)]
mod replica {
    use super::*;
    use std::cell::RefCell;
    // HeapObjectData with Exception / Rational / Identifier boxed.
    pub enum Slim48 {
        BigInt(num_bigint::BigInt),
        Rational(Box<num_rational::BigRational>),
        Real(f64),
        Complex { real: TaggedValue, imag: TaggedValue },
        Symbol(Rc<str>),
        Bytevector(Vec<u8>),
        Exception(Box<(ExceptionKind, String, Vec<TaggedValue>)>),
        Procedure(Rc<Procedure>),
        Record { record_type: Rc<RecordTypeDescriptor>, fields: Rc<RefCell<Vec<TaggedValue>>> },
        Identifier(Box<(Rc<str>, ScopeSet, bool)>),
        Parameter { values: Rc<RefCell<Vec<TaggedValue>>>, converter: Option<TaggedValue> },
        Values(Vec<TaggedValue>),
        MutableCell(RefCell<TaggedValue>),
        Ephemeron(RefCell<Option<(TaggedValue, TaggedValue)>>),
        VmClosure { code_id: u64, free_vars: Vec<TaggedValue>, globals: Rc<Environment> },
        Free,
    }
    pub enum Slim40 {
        BigInt(num_bigint::BigInt),
        Rational(Box<num_rational::BigRational>),
        Real(f64),
        Symbol(Rc<str>),
        Bytevector(Vec<u8>),
        Exception(Box<(ExceptionKind, String, Vec<TaggedValue>)>),
        Identifier(Box<(Rc<str>, ScopeSet, bool)>),
        Ephemeron(RefCell<Option<(TaggedValue, TaggedValue)>>),
        VmClosure(Box<(u64, Vec<TaggedValue>, Rc<Environment>)>),
        Free,
    }
}
pub fn replica_sizes() {
    println!("replica Slim48 (box Exception/Rational/Identifier) = {}", std::mem::size_of::<replica::Slim48>());
    println!("replica Slim40 (also box VmClosure)                = {}", std::mem::size_of::<replica::Slim40>());
}
