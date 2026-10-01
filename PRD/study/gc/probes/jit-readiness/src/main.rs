use std::mem::size_of;
fn main() {
    println!("TaggedValue {}", size_of::<patina_core::TaggedValue>());
    println!("HeapObjectData {}", size_of::<patina_core::heap::HeapObjectData>());
    println!("Instruction {}", size_of::<patina_vm::types::Instruction>());
    println!("CallFrame {}", size_of::<patina_vm::types::CallFrame>());
    println!("CodeObject {}", size_of::<patina_vm::types::CodeObject>());
    println!("PromptFrame {}", size_of::<patina_vm::types::PromptFrame>());
    println!("ExceptionHandler {}", size_of::<patina_vm::types::ExceptionHandler>());
    println!("DynamicWindRecord {}", size_of::<patina_vm::types::DynamicWindRecord>());
    println!("VmContinuation {}", size_of::<patina_vm::types::VmContinuation>());
    println!("(TV,TV) pair {}", size_of::<(patina_core::TaggedValue, patina_core::TaggedValue)>());
    println!("Vec<TV> {}", size_of::<Vec<patina_core::TaggedValue>>());
    println!("Vec<char> {}", size_of::<Vec<char>>());
}
