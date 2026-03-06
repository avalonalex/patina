//! Primitive procedure implementations

mod arithmetic;
mod bytevectors;
mod characters;
mod continuations;
mod conversion;
mod debug;
pub(crate) mod equality;
mod eval;
mod exceptions;
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
mod test;
mod time;
mod values;
mod vectors;

use crate::registry::PrimitiveRegistry;

pub fn register_all(registry: &mut PrimitiveRegistry) {
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
    test::register(registry);
    lazy::register(registry);
    parameters::register(registry);
    system::register(registry);
    time::register(registry);
    process_context::register(registry);
    records::register(registry);
    eval::register(registry);
    exceptions::register(registry);
}
