//! Shared bounded greedy reduction for H1 and H3. The caller admits a candidate
//! only when it preserves that lane's failure signature; source is re-emitted
//! from the binding graph after every change.
use std::time::Instant;

pub fn minimize<C, F>(
    mut case: C,
    mut failure: F,
    limit: usize,
    deadline: Instant,
    reductions: impl Fn(&C) -> Vec<C>,
    mut preserves: impl FnMut(&C) -> Option<F>,
) -> (C, F, usize) {
    let mut attempts = 0;
    loop {
        let mut accepted = None;
        for candidate in reductions(&case) {
            if attempts >= limit || Instant::now() >= deadline {
                return (case, failure, attempts);
            }
            attempts += 1;
            if let Some(next) = preserves(&candidate) {
                accepted = Some((candidate, next));
                break;
            }
        }
        match accepted {
            Some((next_case, next_failure)) => {
                case = next_case;
                failure = next_failure;
            }
            None => return (case, failure, attempts),
        }
    }
}
