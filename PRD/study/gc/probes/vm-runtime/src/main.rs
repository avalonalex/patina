use std::mem::size_of;
use patina_vm::types::*;
use patina_core::heap::HeapObjectData;
use patina_core::environment::Environment;
use patina_core::tagged_value::TaggedValue;
fn main() {
    println!("TaggedValue {}", size_of::<TaggedValue>());
    println!("CallFrame {}", size_of::<CallFrame>());
    println!("Instruction {}", size_of::<Instruction>());
    println!("CodeObject {}", size_of::<CodeObject>());
    println!("GlobalCacheEntry {}", size_of::<GlobalCacheEntry>());
    println!("PromptFrame {}", size_of::<PromptFrame>());
    println!("ExceptionHandler {}", size_of::<ExceptionHandler>());
    println!("DynamicWindRecord {}", size_of::<DynamicWindRecord>());
    println!("VmContinuation {}", size_of::<VmContinuation>());
    println!("VmDelimitedContinuation {}", size_of::<VmDelimitedContinuation>());
    println!("HeapObjectData {}", size_of::<HeapObjectData>());
    println!("Environment {}", size_of::<Environment>());
    println!("VmClosure(types) {}", size_of::<VmClosure>());
    println!("VmState {}", size_of::<patina_vm::runtime::VmState>());
}
