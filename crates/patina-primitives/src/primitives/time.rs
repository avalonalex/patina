//! Time-related primitives
//!
//! R7RS (scheme time) library procedures:
//! - current-second: Current time as TAI seconds since epoch
//! - current-jiffy: Elapsed jiffies since program start
//! - jiffies-per-second: Jiffy resolution constant
use crate::registry::PrimitiveFn;
use crate::registry::PrimitiveRegistry;
use patina_core::TaggedValue;
use patina_runtime::Arity;
use patina_runtime::EvalError;
use patina_runtime::SharedHeap;
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
    registry.register(PrimitiveFn::new_heap(
        "scheme.time",
        "current-second",
        Arity::Exact(0),
        "Return current time as inexact seconds since Jan 1, 1970 TAI",
        current_second,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.time",
        "current-jiffy",
        Arity::Exact(0),
        "Return elapsed jiffies (microseconds) since program start as exact integer",
        |_heap, args| current_jiffy(args),
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.time",
        "jiffies-per-second",
        Arity::Exact(0),
        "Return the number of jiffies per second (1000000 for microsecond resolution)",
        |_heap, args| jiffies_per_second(args),
    ));
}

/// Return current time as inexact seconds since Jan 1, 1970.
///
/// R7RS specifies TAI (International Atomic Time), which differs from UTC
/// by leap seconds (~37 seconds as of 2024). However, R7RS also says
/// "returning Coordinated Universal Time plus a suitable constant might be
/// the best an implementation can do" - so we return UTC which is standard
/// for system time.
fn current_second(heap: &SharedHeap, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "0".to_string(),
            actual: args.len(),
        });
    }

    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| EvalError::InternalError(format!("System time error: {}", e)))?;

    // Return as inexact (f64) per R7RS spec
    let seconds = duration.as_secs_f64();
    Ok(heap.borrow_mut().alloc_real(seconds))
}

/// Return elapsed jiffies since program start as exact integer.
///
/// A jiffy is an implementation-defined fraction of a second.
/// We use microseconds (1,000,000 jiffies per second) which provides
/// good resolution while fitting in a compact integer for most durations.
fn current_jiffy(args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "0".to_string(),
            actual: args.len(),
        });
    }

    let elapsed = program_start().elapsed();
    let micros = elapsed.as_micros() as i64;

    Ok(TaggedValue::fixnum(micros))
}

/// Return the number of jiffies per SI second.
///
/// This is an implementation-specified constant. We use 1,000,000
/// (microsecond resolution) which is a good balance between precision
/// and compact integer representation.
fn jiffies_per_second(args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "0".to_string(),
            actual: args.len(),
        });
    }

    Ok(TaggedValue::fixnum(JIFFIES_PER_SECOND))
}
