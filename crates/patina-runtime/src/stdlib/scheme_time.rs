//! (scheme time) - R7RS Time Library
//!
//! Provides time-related procedures:
//! - current-second: Returns current time as TAI seconds since epoch
//! - current-jiffy: Returns elapsed jiffies since program start
//! - jiffies-per-second: Returns the jiffy resolution (1,000,000 = microseconds)

use crate::environment::Environment;
use crate::value::{Arity, Procedure, Value};
use std::rc::Rc;

/// Build the (scheme time) library
pub fn build_scheme_time(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec!["scheme".to_string(), "time".to_string()];

    let primitives = [
        ("current-second", Arity::Exact(0)),
        ("current-jiffy", Arity::Exact(0)),
        ("jiffies-per-second", Arity::Exact(0)),
    ];

    for (name, arity) in &primitives {
        env.define(
            name.to_string(),
            Value::Procedure(Rc::new(Procedure::Primitive {
                name,
                arity: arity.clone(),
                library: library_name.clone(),
            })),
        );
    }

    primitives
        .iter()
        .map(|(name, _)| name.to_string())
        .collect()
}
