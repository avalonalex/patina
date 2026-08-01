//! VM runtime: `VmState`, the execution loop, and continuation machinery.
//!
//! See VM_RUNTIME.md for the full specification.

mod gc_roots;
pub mod vm_state;

pub use vm_state::{VmState, execute, execute_nested};
