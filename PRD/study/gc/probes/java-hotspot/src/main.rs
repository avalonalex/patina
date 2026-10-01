use std::mem::size_of;
fn main() {
    println!("TaggedValue {}", size_of::<patina_core::TaggedValue>());
    println!("HeapObjectData {}", size_of::<patina_core::heap::HeapObjectData>());
    println!("(TV,TV) pair slot {}", size_of::<(patina_core::TaggedValue, patina_core::TaggedValue)>());
    println!("Vec<TV> vector slot {}", size_of::<Vec<patina_core::TaggedValue>>());
    println!("Vec<char> string slot {}", size_of::<Vec<char>>());
    println!("CallFrame {}", size_of::<patina_vm::types::CallFrame>());
}
