//! Primitive procedure implementations

mod arithmetic;
mod bitwise;
mod bytevectors;
mod characters;
mod continuations;
mod conversion;
mod debug;
mod ephemeron;
pub(crate) mod equality;
mod eval;
mod exceptions;
mod gc;
pub mod io;
mod lazy;
mod lists;
mod parameters;
mod predicates;
mod process_context;
mod records;
mod strings;
mod symbols;
mod system;
mod time;
mod values;
mod vectors;

use crate::registry::PrimitiveRegistry;

pub fn register_all(registry: &mut PrimitiveRegistry) {
    bitwise::register(registry);
    arithmetic::register(registry);
    bytevectors::register(registry);
    characters::register(registry);
    continuations::register(registry);
    conversion::register(registry);
    lists::register(registry);
    predicates::register(registry);
    equality::register(registry);
    strings::register(registry);
    symbols::register(registry);
    vectors::register(registry);
    values::register(registry);
    io::register(registry);
    ephemeron::register(registry);
    lazy::register(registry);
    parameters::register(registry);
    system::register(registry);
    time::register(registry);
    process_context::register(registry);
    records::register(registry);
    eval::register(registry);
    exceptions::register(registry);
    gc::register(registry);
    // Last, where the tree-walker registered these before #742, so that every
    // other primitive keeps its index. Registered after `io`, they moved the
    // tree-walker's instructions by 0.3% on `tw-fib` and `tw-nboyer`.
    debug::register(registry);
    // Primitives added since go after them, for the same reason: an index
    // shift of one cost `string-ports` about 1% in cycles (#608).
    parameters::register_last(registry);
}
