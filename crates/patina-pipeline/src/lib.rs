//! Compatibility facade for the historical tree-walker pipeline.
//!
//! New hosts should depend on `patina-interpreter` and explicitly select
//! `VmInterpreter` (recommended) or `TreeWalkInterpreter`. All evaluation is
//! delegated to `Interpreter<B>`; this crate contains no parser or evaluator.
//! Existing module paths, constructors, traits, and error variants remain
//! available during migration. `StandardPipeline` always uses the tree-walker.
#![allow(deprecated)]

pub mod error;
pub mod pipeline;
pub mod standard;

pub use error::{PipelineError, PipelineResult};
pub use pipeline::{EvaluationStrategy, Pipeline};
pub use standard::StandardPipeline;
