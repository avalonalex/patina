//! What a host sets of an interpreter's memory before it is built (#907,
//! `PRD/GC_PRD.md` §14): today the main stack's cap, to which 5a adds the
//! collector's policy, 5e the heap's limits and 5g a shared budget.
//!
//! A [`HeapConfig`] is taken at construction, by reference, and is not kept:
//! the limits it names size or reserve memory, so there is no later moment
//! at which setting one could mean the same thing. Its fields are private
//! and it is built by methods, so a later stage adds a setting without
//! breaking a host that builds one today.
//!
//! Precedence, for the stack's cap: what the configuration says, held to
//! the machine's memory budget ([`MemoryLimits::stack_cap_for`], #929);
//! where it says nothing, the default fitted to the machine
//! ([`MemoryLimits::default_stack_cap`]). The environment is read only by
//! [`HeapConfig::from_env`], which the constructors that take no
//! configuration use: a host that passes its own configuration gets what it
//! wrote, held to the memory budget, whatever the process's environment
//! holds.

use super::budget::MemoryLimits;

/// The variable [`HeapConfig::from_env`] reads the main stack's cap from,
/// in the grammar of [`parse_size`]. `--stack-max` is the same setting on
/// the command line, and wins over it.
pub const STACK_MAX_VAR: &str = "PATINA_STACK_MAX";

/// An interpreter's memory settings (the module comment says how one is
/// used). `HeapConfig::new()` is the default configuration: every setting
/// left to its default, and nothing read from the environment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeapConfig {
    max_stack: Option<u64>,
}

impl HeapConfig {
    /// The default configuration: nothing set, nothing read from the
    /// environment.
    pub fn new() -> Self {
        Self::default()
    }

    /// The configuration the environment asks for: [`STACK_MAX_VAR`] where
    /// it is set and not empty, and the defaults otherwise. What the
    /// constructors that take no configuration use.
    ///
    /// # Panics
    ///
    /// Where a variable is set to something that is not a size
    /// ([`parse_size`]): a limit quietly ignored is a limit not held. A
    /// program that would rather report it calls
    /// [`HeapConfig::try_from_env`].
    pub fn from_env() -> Self {
        match Self::try_from_env() {
            Ok(config) => config,
            Err(error) => panic!("{STACK_MAX_VAR}: {error}"),
        }
    }

    /// [`HeapConfig::from_env`], answering a malformed variable as an error
    /// where that panics.
    pub fn try_from_env() -> Result<Self, SizeError> {
        let mut config = Self::new();
        if let Some(text) = std::env::var_os(STACK_MAX_VAR).filter(|text| !text.is_empty()) {
            config.max_stack(parse_size(&text.to_string_lossy())?);
        }
        Ok(config)
    }

    /// Set the main stack's cap, in bytes: the most the VM's frames and
    /// registers may take before a call raises `&stack-exhausted`. A value
    /// under [`STACK_CAP_FLOOR`](super::budget::STACK_CAP_FLOOR) (2 MiB) is
    /// raised to it, since the last MiB of the cap is the room a handler
    /// for the condition runs in. A value above the memory budget, physical
    /// memory or the cgroup's limit, is held to it (#929):
    /// [`HeapConfig::stack_cap`] answers what a machine built from this
    /// configuration holds. The tree-walker keeps no cap and ignores the
    /// setting.
    pub fn max_stack(&mut self, bytes: u64) -> &mut Self {
        self.max_stack = Some(bytes);
        self
    }

    /// The main stack's cap under this configuration, in bytes: what
    /// [`HeapConfig::max_stack`] set, raised to the floor and held to the
    /// memory budget ([`MemoryLimits::stack_cap_for`], #929), or the
    /// default fitted to the machine's memory, its cgroup's limit and
    /// `RLIMIT_AS` ([`MemoryLimits::default_stack_cap`]). The limits are
    /// read at this call.
    pub fn stack_cap(&self) -> u64 {
        let limits = MemoryLimits::detect();
        match self.max_stack {
            Some(bytes) => limits.stack_cap_for(bytes),
            None => limits.default_stack_cap(),
        }
    }
}

/// Why a text is not a size ([`parse_size`]): the text, and what is wrong
/// with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SizeError {
    text: String,
    reason: &'static str,
}

impl std::fmt::Display for SizeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "`{}` is not a size ({}): a size is a number of bytes, or a number followed by K, M or G \
             for binary kilobytes, megabytes or gigabytes, as 64M is",
            self.text, self.reason
        )
    }
}

impl std::error::Error for SizeError {}

/// A size in bytes from its text, the grammar `--stack-max` and
/// [`STACK_MAX_VAR`] share: decimal digits, then optionally one of `K`, `M`
/// or `G` in either case, binary (1024, 1024², 1024³), as `java -Xss` and
/// GHC's `-K` read theirs. Nothing else: no sign, no fraction, no space, no
/// `B`.
pub fn parse_size(text: &str) -> Result<u64, SizeError> {
    let error = |reason| SizeError {
        text: text.to_string(),
        reason,
    };
    let (digits, shift) = match text.as_bytes().last() {
        Some(b'K' | b'k') => (&text[..text.len() - 1], 10),
        Some(b'M' | b'm') => (&text[..text.len() - 1], 20),
        Some(b'G' | b'g') => (&text[..text.len() - 1], 30),
        _ => (text, 0),
    };
    if digits.is_empty() {
        return Err(error("no number"));
    }
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(error("not a whole number"));
    }
    let number: u64 = digits.parse().map_err(|_| error("too large"))?;
    number
        .checked_mul(1 << shift)
        .ok_or_else(|| error("too large"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_size_is_bytes_with_a_binary_suffix() {
        assert_eq!(parse_size("0"), Ok(0));
        assert_eq!(parse_size("4096"), Ok(4096));
        assert_eq!(parse_size("2K"), Ok(2 << 10));
        assert_eq!(parse_size("2k"), Ok(2 << 10));
        assert_eq!(parse_size("64M"), Ok(64 << 20));
        assert_eq!(parse_size("64m"), Ok(64 << 20));
        assert_eq!(parse_size("8G"), Ok(8 << 30));
        assert_eq!(parse_size("8g"), Ok(8 << 30));
        assert_eq!(parse_size("17179869183G"), Ok(17_179_869_183 << 30));
    }

    #[test]
    fn what_is_not_a_size_says_why() {
        for (text, reason) in [
            ("", "no number"),
            ("M", "no number"),
            ("-1", "not a whole number"),
            ("+1", "not a whole number"),
            ("1.5G", "not a whole number"),
            ("1 M", "not a whole number"),
            ("1MB", "not a whole number"),
            ("1MiB", "not a whole number"),
            ("0x10", "not a whole number"),
            ("١٢", "not a whole number"),
            ("18446744073709551616", "too large"),
            ("17179869184G", "too large"),
        ] {
            let error = parse_size(text).expect_err(text);
            assert_eq!(error.reason, reason, "{text}");
            assert!(error.to_string().contains(&format!("`{text}`")), "{error}");
        }
    }

    /// A configuration's cap is what it was given, never under the floor;
    /// one that was given none answers the machine's default, which is
    /// between the floor and the ceiling whatever the machine.
    #[test]
    fn the_cap_is_what_was_set_or_the_default() {
        use crate::heap::budget::{STACK_CAP_CEILING, STACK_CAP_FLOOR};
        assert_eq!(HeapConfig::new().max_stack(64 << 20).stack_cap(), 64 << 20);
        assert_eq!(HeapConfig::new().max_stack(1).stack_cap(), STACK_CAP_FLOOR);
        assert_eq!(HeapConfig::new().max_stack(0).stack_cap(), STACK_CAP_FLOOR);
        // More than the default's ceiling may be asked for, and is held to
        // the machine's memory budget where that is known (#929).
        let budget = MemoryLimits::detect().budget();
        assert_eq!(
            HeapConfig::new().max_stack(u64::MAX).stack_cap(),
            budget.unwrap_or(u64::MAX).max(STACK_CAP_FLOOR),
        );
        if let Some(budget) = budget.filter(|&budget| budget > STACK_CAP_CEILING) {
            assert_eq!(
                HeapConfig::new().max_stack(budget - 4096).stack_cap(),
                budget - 4096,
                "a cap above the default's ceiling and under the budget is held as asked"
            );
        }
        let default = HeapConfig::new().stack_cap();
        assert_eq!(default, MemoryLimits::detect().default_stack_cap());
        assert!((STACK_CAP_FLOOR..=STACK_CAP_CEILING).contains(&default));
        // A later setting replaces an earlier one, and a clone is its own.
        let mut config = HeapConfig::new();
        config.max_stack(4 << 20).max_stack(8 << 20);
        let copy = config.clone();
        config.max_stack(16 << 20);
        assert_eq!((copy.stack_cap(), config.stack_cap()), (8 << 20, 16 << 20));
    }
}
