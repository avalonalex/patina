//! Compatibility for the historical tree-walker pipeline API.
//!
//! New embeddings should use `VmInterpreter` or `TreeWalkInterpreter`. This
//! module owns the legacy signatures so `patina-pipeline` can re-export them
//! without making the interpreter depend on that facade.
#![allow(deprecated)]

pub mod error;
pub mod pipeline;
pub mod standard;

pub use error::{PipelineError, PipelineResult};
pub use pipeline::{EvaluationStrategy, Pipeline};
pub use standard::StandardPipeline;
