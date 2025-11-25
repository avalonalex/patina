//! Re-export Pattern from patina-core
//!
//! The Pattern type is now defined in patina-core for shared use across crates.
//! This module re-exports it for backwards compatibility.

pub use patina_runtime::Pattern;

#[cfg(test)]
mod tests {
    use super::*;
    use patina_runtime::PVRef;

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
