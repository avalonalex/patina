//! Time-related primitives
//!
//! R7RS (scheme time) library procedures:
//! - current-second: Current time as TAI seconds since epoch
//! - current-jiffy: Elapsed jiffies since program start
//! - jiffies-per-second: Jiffy resolution constant

use super::super::EvalResult;
use super::super::Evaluator;
use super::super::error::EvalError;
use super::registry::PrimitiveFn;
use super::registry::PrimitiveRegistry;
use patina_runtime::value::{Arity, Value};
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Program start time for jiffy calculation
static PROGRAM_START: OnceLock<Instant> = OnceLock::new();

/// Number of jiffies per second (microsecond resolution)
const JIFFIES_PER_SECOND: i64 = 1_000_000;

/// Get or initialize the program start time
fn program_start() -> Instant {
    *PROGRAM_START.get_or_init(Instant::now)
}

/// Register all time primitives in the registry
pub(super) fn register(registry: &mut PrimitiveRegistry) {
    registry.register(PrimitiveFn::new(
        "scheme.time",
        "current-second",
        Arity::Exact(0),
        "Return current time as inexact seconds since Jan 1, 1970 TAI",
        |eval, args, _| current_second(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.time",
        "current-jiffy",
        Arity::Exact(0),
        "Return elapsed jiffies (microseconds) since program start as exact integer",
        |eval, args, _| current_jiffy(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.time",
        "jiffies-per-second",
        Arity::Exact(0),
        "Return the number of jiffies per second (1000000 for microsecond resolution)",
        |eval, args, _| jiffies_per_second(eval, args).map(EvalResult::Value),
    ));
}

/// Return current time as inexact seconds since Jan 1, 1970.
///
/// R7RS specifies TAI (International Atomic Time), which differs from UTC
/// by leap seconds (~37 seconds as of 2024). However, R7RS also says
/// "returning Coordinated Universal Time plus a suitable constant might be
/// the best an implementation can do" - so we return UTC which is standard
/// for system time.
fn current_second(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 0, "current-second")?;

    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| EvalError::InternalError(format!("System time error: {}", e)))?;

    // Return as inexact (f64) per R7RS spec
    let seconds = duration.as_secs_f64();
    Ok(Value::Real(seconds))
}

/// Return elapsed jiffies since program start as exact integer.
///
/// A jiffy is an implementation-defined fraction of a second.
/// We use microseconds (1,000,000 jiffies per second) which provides
/// good resolution while fitting in a compact integer for most durations.
fn current_jiffy(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 0, "current-jiffy")?;

    let elapsed = program_start().elapsed();
    let micros = elapsed.as_micros() as i64;

    Ok(Value::Integer(micros))
}

/// Return the number of jiffies per SI second.
///
/// This is an implementation-specified constant. We use 1,000,000
/// (microsecond resolution) which is a good balance between precision
/// and compact integer representation.
fn jiffies_per_second(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 0, "jiffies-per-second")?;

    Ok(Value::Integer(JIFFIES_PER_SECOND))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_current_second_returns_positive() {
        let eval = Evaluator::new();
        let result = current_second(&eval, vec![]).unwrap();

        match result {
            Value::Real(secs) => {
                // Should be after year 2020 (roughly 50 years after epoch)
                assert!(secs > 1577836800.0, "current-second should be after 2020");
            }
            _ => panic!("current-second should return Real"),
        }
    }

    #[test]
    fn test_current_jiffy_returns_non_negative() {
        let eval = Evaluator::new();
        let result = current_jiffy(&eval, vec![]).unwrap();

        match result {
            Value::Integer(jiffies) => {
                assert!(jiffies >= 0, "current-jiffy should be non-negative");
            }
            _ => panic!("current-jiffy should return Integer"),
        }
    }

    #[test]
    fn test_current_jiffy_increases() {
        let eval = Evaluator::new();

        let result1 = current_jiffy(&eval, vec![]).unwrap();
        // Small delay
        std::thread::sleep(std::time::Duration::from_micros(100));
        let result2 = current_jiffy(&eval, vec![]).unwrap();

        match (result1, result2) {
            (Value::Integer(j1), Value::Integer(j2)) => {
                assert!(j2 >= j1, "current-jiffy should not decrease");
            }
            _ => panic!("current-jiffy should return Integer"),
        }
    }

    #[test]
    fn test_jiffies_per_second() {
        let eval = Evaluator::new();
        let result = jiffies_per_second(&eval, vec![]).unwrap();

        match result {
            Value::Integer(jps) => {
                assert_eq!(jps, 1_000_000, "jiffies-per-second should be 1000000");
            }
            _ => panic!("jiffies-per-second should return Integer"),
        }
    }
}
