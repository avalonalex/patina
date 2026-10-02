use std::marker::PhantomData;
struct W<T: ?Sized>(PhantomData<T>);
trait NotSend { const SEND: bool = false; }
impl<T: ?Sized> NotSend for W<T> {}
trait NotSync { const SYNC: bool = false; }
impl<T: ?Sized> NotSync for W<T> {}
struct S<T: ?Sized>(PhantomData<T>);
impl<T: ?Sized> NotSync for S<T> {}
impl<T: ?Sized + Send> W<T> { const SEND: bool = true; }
impl<T: ?Sized + Sync> S<T> { const SYNC: bool = true; }
macro_rules! check { ($($t:ty),* $(,)?) => { $( println!("{:<70} Send={:<5} Sync={}", stringify!($t), W::<$t>::SEND, S::<$t>::SYNC); )* } }
fn main() {
    check!(
        patina_core::TaggedValue,
        patina_core::Heap,
        patina_core::SharedHeap,
        patina_core::heap::HeapObjectData,
        patina_runtime::Environment,
        patina_core::Port,
        patina_core::CompiledMacro,
        patina_core::Library,
        patina_core::Procedure,
        patina_core::CpsExpr,
        patina_ir::CoreExpr,
        patina_core::GcController,
        patina_core::error::SourceLocation,
        patina_runtime::LibraryRegistry,
        patina_runtime::LibraryLoaderRegistry,
        patina_primitives::PrimitiveRegistry,
        patina_primitives::Step,
        patina_frontend::Parser,
        patina_frontend::Desugarer,
        patina_frontend::SourceMap,
        patina_vm::types::CodeObject,
        patina_vm::runtime::VmState,
        patina_vm::VmBackend,
        patina_vm::VmBackendError,
        patina_tree_walker::Evaluator,
        patina_tree_walker::TreeWalker,
        patina_interpreter::Interpreter<patina_vm::VmBackend>,
        patina_interpreter::Interpreter<patina_tree_walker::TreeWalker>,
    );
}
