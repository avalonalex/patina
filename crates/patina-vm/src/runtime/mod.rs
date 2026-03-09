//! VM runtime: `VmState`, the execution loop, and continuation machinery.
//!
//! See VM_RUNTIME.md for the full specification.

pub mod vm_state;

pub use vm_state::VmState;
