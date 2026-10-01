use std::mem::size_of;
use patina_core::heap::HeapObjectData;
use patina_core::tagged_value::TaggedValue;
use patina_core::environment::Environment;
fn main() {
    println!("TaggedValue = {}", size_of::<TaggedValue>());
    println!("HeapObjectData = {}", size_of::<HeapObjectData>());
    println!("pair slot (TV,TV) = {}", size_of::<(TaggedValue, TaggedValue)>());
    println!("vector slot Vec<TV> = {}", size_of::<Vec<TaggedValue>>());
    println!("string slot Vec<char> = {}", size_of::<Vec<char>>());
    println!("Environment = {}", size_of::<Environment>());
    println!("Option<TaggedValue> = {}", size_of::<Option<TaggedValue>>());
}
