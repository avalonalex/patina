//! The types every carrier shares are `Send + Sync` (C11, #711;
//! `PRD/GC_PRD.md` §18.6).
//!
//! Under N carriers a poster on another thread holds the `InterruptHandle`,
//! and one primitive registry serves every carrier. Both are `Send + Sync`
//! already, and this keeps them so: a field that is not, such as an `Rc` or a
//! `Cell`, fails the build. The registry is also asserted where it is
//! defined (`patina-primitives/src/registry.rs`).

use patina_core::InterruptHandle;
use patina_primitives::PrimitiveRegistry;

const fn send_and_sync<T: Send + Sync>() {}

const _: () = {
    send_and_sync::<InterruptHandle>();
    send_and_sync::<PrimitiveRegistry>();
};

#[test]
fn the_types_every_carrier_shares_are_send_and_sync() {
    // The assertions above are checked as the test compiles; this names
    // them in the test list.
    send_and_sync::<InterruptHandle>();
    send_and_sync::<PrimitiveRegistry>();
}
