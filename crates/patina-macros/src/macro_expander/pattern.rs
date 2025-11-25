//! PVREF-based pattern representation
//!
//! This module implements the new PVREF (Pattern Variable Reference) based
//! pattern representation for the macro system. This is a complete rewrite
//! inspired by Gauche Scheme's macro.c implementation.
//!
//! Key improvements over the original Pattern:
//! - Uses PVREF encoding instead of string-based HashMap lookups
//! - Precomputes metadata (num_following, vars list) for optimization
//! - Clear separation between compilation and matching phases
//!
//! Inspired by Gauche's pattern compilation (macro.c:400+)
//! Reference: https://github.com/shirok/Gauche/blob/master/src/macro.c

use patina_runtime::{PVRef, Value};

/// Compiled pattern for PVREF-based macro system
///
/// This is the second-generation pattern representation using PVREF encoding.
/// It's compiled once at macro definition time and reused for all expansions.
///
/// Based on Gauche's pattern compilation approach (macro.c:400+).
#[derive(Clone, Debug)]
pub enum Pattern {
    /// Wildcard (_) - matches anything, binds nothing
    Wildcard,

    /// Literal value that must match exactly
    Literal(Value),

    /// Pattern variable - binds to matched expression
    ///
    /// Uses PVREF (Pattern Variable Reference) for O(1) lookup instead of string-based HashMap.
    Var(PVRef),

    /// List pattern: (p1 p2 p3)
    List(Vec<Pattern>),

    /// Vector pattern: #(p1 p2 p3)
    Vector(Vec<Pattern>),

    /// Dotted list pattern: (p1 p2 . rest)
    ///
    /// Matches an improper list where the tail is bound to a variable.
    DottedList {
        patterns: Vec<Pattern>,
        tail: Box<Pattern>,
    },

    /// Ellipsis pattern: (p1 ... p2)
    ///
    /// Matches zero or more repetitions of the subpattern.
    /// Includes precomputed metadata for optimization.
    Ellipsis {
        /// Pattern to repeat
        subpattern: Box<Pattern>,

        /// Ellipsis nesting level (1 for first ..., 2 for nested, etc.)
        level: u8,

        /// Number of items after this ellipsis (excluding final CDR)
        ///
        /// This is Gauche's clever optimization (macro.c:138-145) that avoids backtracking.
        /// Example: From (x ... y z), x ... has num_following = 2
        /// Example: From (x ... . y), x ... has num_following = 0
        num_following: usize,

        /// Pattern variables used in this subpattern
        ///
        /// Precomputed list of PVREFs for efficient matching.
        /// All these variables will be bound as Multiple values.
        vars: Vec<PVRef>,
    },
}

impl Pattern {
    /// Check if this pattern is a wildcard
    pub fn is_wildcard(&self) -> bool {
        matches!(self, Pattern::Wildcard)
    }

    /// Check if this pattern is a literal
    pub fn is_literal(&self) -> bool {
        matches!(self, Pattern::Literal(_))
    }

    /// Check if this pattern is a variable
    pub fn is_var(&self) -> bool {
        matches!(self, Pattern::Var(_))
    }

    /// Check if this pattern is an ellipsis
    pub fn is_ellipsis(&self) -> bool {
        matches!(self, Pattern::Ellipsis { .. })
    }

    /// Get the PVREF if this is a variable pattern
    pub fn as_var(&self) -> Option<PVRef> {
        match self {
            Pattern::Var(pvref) => Some(*pvref),
            _ => None,
        }
    }
}

impl std::fmt::Display for Pattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Pattern::Wildcard => write!(f, "_"),
            Pattern::Literal(v) => write!(f, "{}", v),
            Pattern::Var(pvref) => write!(f, "?{}", pvref),
            Pattern::List(patterns) => {
                write!(f, "(")?;
                for (i, p) in patterns.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", p)?;
                }
                write!(f, ")")
            }
            Pattern::Vector(patterns) => {
                write!(f, "#(")?;
                for (i, p) in patterns.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", p)?;
                }
                write!(f, ")")
            }
            Pattern::DottedList { patterns, tail } => {
                write!(f, "(")?;
                for (i, p) in patterns.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", p)?;
                }
                write!(f, " . {})", tail)
            }
            Pattern::Ellipsis {
                subpattern,
                level,
                num_following,
                vars,
            } => {
                write!(
                    f,
                    "(... {} @level={} following={} vars={:?})",
                    subpattern, level, num_following, vars
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pattern_wildcard() {
        let pat = Pattern::Wildcard;
        assert!(pat.is_wildcard());
        assert!(!pat.is_var());
        assert_eq!(format!("{}", pat), "_");
    }

    #[test]
    fn test_pattern_var() {
        let pvref = PVRef::new(0, 0);
        let pat = Pattern::Var(pvref);
        assert!(pat.is_var());
        assert_eq!(pat.as_var(), Some(pvref));
        assert_eq!(format!("{}", pat), "?PVRef(level=0, index=0)");
    }

    #[test]
    fn test_pattern_list() {
        let pat = Pattern::List(vec![
            Pattern::Var(PVRef::new(0, 0)),
            Pattern::Var(PVRef::new(0, 1)),
        ]);
        assert_eq!(
            format!("{}", pat),
            "(?PVRef(level=0, index=0) ?PVRef(level=0, index=1))"
        );
    }

    #[test]
    fn test_pattern_ellipsis() {
        let pat = Pattern::Ellipsis {
            subpattern: Box::new(Pattern::Var(PVRef::new(1, 0))),
            level: 1,
            num_following: 2,
            vars: vec![PVRef::new(1, 0)],
        };
        assert!(pat.is_ellipsis());
        assert!(format!("{}", pat).contains("@level=1"));
        assert!(format!("{}", pat).contains("following=2"));
    }
}
