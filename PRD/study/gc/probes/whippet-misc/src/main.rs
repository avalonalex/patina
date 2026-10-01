use std::mem::size_of;
use patina_core::heap::HeapObjectData;
use patina_core::tagged_value::TaggedValue;
fn main() {
    println!("TaggedValue = {}", size_of::<TaggedValue>());
    println!("(TV,TV) pair slot = {}", size_of::<(TaggedValue, TaggedValue)>());
    println!("Vec<TV> vector slot header = {}", size_of::<Vec<TaggedValue>>());
    println!("Vec<char> string slot header = {}", size_of::<Vec<char>>());
    println!("HeapObjectData = {}", size_of::<HeapObjectData>());
}
