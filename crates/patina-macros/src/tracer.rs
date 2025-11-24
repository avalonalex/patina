//! Structured macro expansion tracing
//!
//! This module provides selective tracing of macro expansions for debugging.
//! Unlike MACRO_DEBUG which traces everything, MacroTracer allows you to:
//! - Trace specific macros by name
//! - Control trace depth
//! - Record expansion history for error reporting

use crate::error::ExpansionStep;
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

thread_local! {
    /// Global macro tracer instance
    static TRACER: RefCell<MacroTracer> = RefCell::new(MacroTracer::new());
}

/// Selective macro expansion tracer
///
/// # Usage
///
/// ```ignore
/// use patina_macros::MacroTracer;
///
/// // Enable tracing for specific macros
/// MacroTracer::enable_for(&["let-values", "my-macro"]);
///
/// // Set maximum trace depth
/// MacroTracer::set_max_depth(10);
///
/// // Run your code - only specified macros will be traced
/// interpreter.eval_str("(let-values ...)").unwrap();
///
/// // Get the expansion history
/// let history = MacroTracer::get_history();
/// for step in history {
///     println!("{}", step);
/// }
///
/// // Clear and disable
/// MacroTracer::clear();
/// MacroTracer::disable();
/// ```
#[derive(Debug, Clone)]
pub struct MacroTracer {
    /// Set of macro names to trace (empty means trace none)
    enabled_macros: HashSet<Rc<str>>,

    /// Maximum expansion depth to trace (0 means unlimited)
    max_depth: usize,

    /// Current expansion depth
    current_depth: usize,

    /// History of expansion steps
    history: Vec<ExpansionStep>,

    /// Whether tracing is globally enabled
    enabled: bool,
}

impl MacroTracer {
    /// Create a new tracer (disabled by default)
    pub fn new() -> Self {
        Self {
            enabled_macros: HashSet::new(),
            max_depth: 0,
            current_depth: 0,
            history: Vec::new(),
            enabled: false,
        }
    }

    /// Enable tracing for specific macros
    ///
    /// # Example
    /// ```ignore
    /// MacroTracer::enable_for(&["let-values", "cond"]);
    /// ```
    pub fn enable_for(macro_names: &[&str]) {
        TRACER.with(|t| {
            let mut tracer = t.borrow_mut();
            tracer.enabled = true;
            tracer.enabled_macros.clear();
            for name in macro_names {
                tracer.enabled_macros.insert(Rc::from(*name));
            }
        });
    }

    /// Enable tracing for all macros
    pub fn enable_all() {
        TRACER.with(|t| {
            let mut tracer = t.borrow_mut();
            tracer.enabled = true;
            tracer.enabled_macros.clear();
        });
    }

    /// Disable tracing
    pub fn disable() {
        TRACER.with(|t| {
            t.borrow_mut().enabled = false;
        });
    }

    /// Set maximum trace depth (0 = unlimited)
    pub fn set_max_depth(depth: usize) {
        TRACER.with(|t| {
            t.borrow_mut().max_depth = depth;
        });
    }

    /// Clear the expansion history
    pub fn clear() {
        TRACER.with(|t| {
            let mut tracer = t.borrow_mut();
            tracer.history.clear();
            tracer.current_depth = 0;
        });
    }

    /// Check if a macro should be traced
    pub fn should_trace(macro_name: &Rc<str>) -> bool {
        TRACER.with(|t| {
            let tracer = t.borrow();
            if !tracer.enabled {
                return false;
            }

            // Check depth limit
            if tracer.max_depth > 0 && tracer.current_depth >= tracer.max_depth {
                return false;
            }

            // If enabled_macros is empty, trace all
            // Otherwise, only trace if macro is in the set
            tracer.enabled_macros.is_empty() || tracer.enabled_macros.contains(macro_name)
        })
    }

    /// Record an expansion step
    pub fn record_step(step: ExpansionStep) {
        TRACER.with(|t| {
            let mut tracer = t.borrow_mut();
            if tracer.enabled {
                tracer.history.push(step);
            }
        });
    }

    /// Enter a macro expansion (increment depth)
    pub fn enter_expansion() {
        TRACER.with(|t| {
            t.borrow_mut().current_depth += 1;
        });
    }

    /// Exit a macro expansion (decrement depth)
    pub fn exit_expansion() {
        TRACER.with(|t| {
            let mut tracer = t.borrow_mut();
            if tracer.current_depth > 0 {
                tracer.current_depth -= 1;
            }
        });
    }

    /// Get the current expansion depth
    pub fn current_depth() -> usize {
        TRACER.with(|t| t.borrow().current_depth)
    }

    /// Get a copy of the expansion history
    pub fn get_history() -> Vec<ExpansionStep> {
        TRACER.with(|t| t.borrow().history.clone())
    }

    /// Print the expansion history
    pub fn print_history() {
        TRACER.with(|t| {
            let tracer = t.borrow();
            if tracer.history.is_empty() {
                println!("No macro expansions traced.");
                return;
            }

            println!("Macro expansion history:");
            for (i, step) in tracer.history.iter().enumerate() {
                println!("{}. {}", i + 1, step);
            }
        });
    }
}

impl Default for MacroTracer {
    fn default() -> Self {
        Self::new()
    }
}
