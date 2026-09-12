//! VM runtime: `VmState`, the execution loop, and continuation machinery.
//!
//! `control` owns callable dispatch, resumable control transfers, and the
//! synchronous callback escape protocol. [`vm_state`] owns storage, driver
//! loops, instruction dispatch, GC safe points, and host evaluation services.
//!
//! See `docs/VM_RUNTIME.md` for the full specification.

pub(crate) mod control;
mod gc_roots;
pub mod vm_state;

pub use vm_state::{VmState, execute, execute_nested};
