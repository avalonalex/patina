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
    debug::register(registry);
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
}
