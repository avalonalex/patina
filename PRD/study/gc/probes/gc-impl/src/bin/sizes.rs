use std::mem::size_of;
fn main() {
    println!("ScopeSet {}", size_of::<patina_core::ScopeSet>());
    println!("ExceptionKind {}", size_of::<patina_core::ExceptionKind>());
    println!("String {}", size_of::<String>());
    println!("Rc<str> {}", size_of::<std::rc::Rc<str>>());
    println!("RefCell<Option<(TV,TV)>> {}", size_of::<std::cell::RefCell<Option<(patina_core::TaggedValue, patina_core::TaggedValue)>>>());
    println!("VmDelimitedContinuation {}", size_of::<patina_vm::types::continuation::VmDelimitedContinuation>());
    println!("DynamicWindRecord(vm) {}", size_of::<patina_vm::types::continuation::DynamicWindRecord>());
    println!("CompiledMacro {}", size_of::<patina_core::CompiledMacro>());
    println!("Library {}", size_of::<patina_core::Library>());
}
