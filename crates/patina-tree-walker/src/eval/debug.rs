//! Debug tracing infrastructure for the evaluator
//!
//! Provides configurable debug output for different evaluation stages:
//! - Lexing (future)
//! - Parsing (future)
//! - Evaluation
//! - Procedure application
//! - Environment operations
//! - Macro expansion (future)
//!
//! Uses the `tracing` crate for structured logging. Enable with:
//! - `RUST_LOG=patina_tree_walker=debug` for debug-level traces
//! - `RUST_LOG=patina_tree_walker=trace` for trace-level (verbose) output

use std::cell::RefCell;
use std::collections::HashSet;

/// Log an evaluation entry point
#[macro_export]
macro_rules! trace_eval {
    ($debug:expr, $($arg:tt)*) => {
        if $debug.is_enabled($crate::eval::debug::DebugStage::Eval) {
            tracing::debug!(target: "patina_tree_walker::eval", $($arg)*);
        }
    };
}

/// Log an evaluation result
#[macro_export]
macro_rules! trace_eval_result {
    ($debug:expr, $($arg:tt)*) => {
        if $debug.is_enabled($crate::eval::debug::DebugStage::Eval) {
            tracing::debug!(target: "patina_tree_walker::eval", $($arg)*);
        }
    };
}

/// Log a procedure application
#[macro_export]
macro_rules! trace_apply {
    ($debug:expr, $($arg:tt)*) => {
        if $debug.is_enabled($crate::eval::debug::DebugStage::Apply) {
            tracing::debug!(target: "patina_tree_walker::apply", $($arg)*);
        }
    };
}

/// Log an environment lookup
#[macro_export]
macro_rules! trace_env {
    ($debug:expr, $($arg:tt)*) => {
        if $debug.is_enabled($crate::eval::debug::DebugStage::Env) {
            tracing::trace!(target: "patina_tree_walker::env", $($arg)*);
        }
    };
}

/// Log a warning (always emitted, not controlled by debug stages)
#[macro_export]
macro_rules! trace_warn {
    ($($arg:tt)*) => {
        tracing::warn!(target: "patina_tree_walker", $($arg)*);
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DebugStage {
    Lex,
    Parse,
    Eval,
    Apply,
    Env,
    Expand,
}

/// Configuration for debug output
#[allow(dead_code)]
pub struct DebugConfig {
    enabled_stages: RefCell<HashSet<DebugStage>>,
    indent_level: RefCell<usize>,
}

impl DebugConfig {
    /// Create a new debug configuration with all stages disabled
    pub fn new() -> Self {
        Self {
            enabled_stages: RefCell::new(HashSet::new()),
            indent_level: RefCell::new(0),
        }
    }

    /// Enable a specific debug stage
    pub fn enable(&self, stage: DebugStage) {
        self.enabled_stages.borrow_mut().insert(stage);
    }

    /// Disable a specific debug stage
    pub fn disable(&self, stage: DebugStage) {
        self.enabled_stages.borrow_mut().remove(&stage);
    }

    /// Check if a stage is enabled
    pub fn is_enabled(&self, stage: DebugStage) -> bool {
        self.enabled_stages.borrow().contains(&stage)
    }

    /// Disable all debug stages
    pub fn clear(&self) {
        self.enabled_stages.borrow_mut().clear();
    }

    /// Enable all debug stages
    pub fn enable_all(&self) {
        let mut stages = self.enabled_stages.borrow_mut();
        stages.insert(DebugStage::Lex);
        stages.insert(DebugStage::Parse);
        stages.insert(DebugStage::Eval);
        stages.insert(DebugStage::Apply);
        stages.insert(DebugStage::Env);
        stages.insert(DebugStage::Expand);
    }

    /// Get currently enabled stages as a list
    #[allow(dead_code)]
    pub fn enabled_list(&self) -> Vec<DebugStage> {
        self.enabled_stages.borrow().iter().copied().collect()
    }

    /// Increase indentation level
    pub fn indent(&self) {
        *self.indent_level.borrow_mut() += 1;
    }

    /// Decrease indentation level
    pub fn dedent(&self) {
        let mut level = self.indent_level.borrow_mut();
        if *level > 0 {
            *level -= 1;
        }
    }

    /// Get current indentation as a string
    pub fn current_indent(&self) -> String {
        "  ".repeat(*self.indent_level.borrow())
    }
}

impl Default for DebugConfig {
    fn default() -> Self {
        Self::new()
    }
}
