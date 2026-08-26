//! Feature Registry for R7RS cond-expand
//!
//! R7RS §4.2.1 and §5.6.1 define `cond-expand` which uses feature identifiers
//! to conditionally include code. The `(features)` procedure returns the list
//! of features supported by the implementation.
//!
//! Standard features from R7RS Appendix B:
//! - `r7rs` - This implementation supports R7RS
//! - `exact-closed` - Exact arithmetic is closed under +, -, *, expt
//! - `exact-complex` - Exact complex numbers are supported
//! - `ieee-float` - Inexact numbers are IEEE 754 floats
//! - `full-unicode` - All Unicode characters are supported
//! - `ratios` - Exact ratios are supported
//! - Platform identifiers: `posix`, `windows`, `unix`, `darwin`, `gnu-linux`
//! - Architecture: `i386`, `x86-64`, `ppc`, `sparc`, `arm`, `aarch64`
//! - Endianness: `big-endian`, `little-endian`

use std::collections::HashSet;

/// Registry of implementation features for cond-expand
#[derive(Debug, Clone)]
pub struct FeatureRegistry {
    features: HashSet<String>,
}

impl FeatureRegistry {
    /// Create a new feature registry with Patina's default features
    pub fn new() -> Self {
        let mut features = HashSet::new();

        // R7RS required
        features.insert("r7rs".to_string());

        // Implementation identifier
        features.insert("patina".to_string());

        // Numeric capabilities
        features.insert("ratios".to_string()); // We support exact rationals
        features.insert("exact-closed".to_string()); // Exact arithmetic stays exact
        features.insert("ieee-float".to_string()); // f64 is IEEE 754

        // Unicode support
        features.insert("full-unicode".to_string()); // Rust strings are UTF-8
        // R7RS's `full-unicode` says the *reader* and char procedures cover
        // Unicode; it does not promise strings can hold every scalar value,
        // which is why SRFI 135's and Larceny's suites gate their non-ASCII
        // data on this second name instead. Patina qualifies — a string is a
        // Vec<char>, so "\x3b1;\x1F600;" is two characters and `string-ref`
        // answers with the astral one — and until this was advertised both
        // suites ran their ASCII-only branch and skipped 41 assertions.
        features.insert("full-unicode-strings".to_string());

        // Platform detection (compile-time)
        #[cfg(target_os = "macos")]
        {
            features.insert("darwin".to_string());
            features.insert("macosx".to_string()); // Common alias
            features.insert("posix".to_string());
            features.insert("unix".to_string());
        }

        #[cfg(target_os = "linux")]
        {
            features.insert("linux".to_string());
            features.insert("gnu-linux".to_string());
            features.insert("posix".to_string());
            features.insert("unix".to_string());
        }

        #[cfg(target_os = "freebsd")]
        {
            features.insert("freebsd".to_string());
            features.insert("bsd".to_string());
            features.insert("posix".to_string());
            features.insert("unix".to_string());
        }

        #[cfg(windows)]
        {
            features.insert("windows".to_string());
        }

        // Architecture detection
        #[cfg(target_arch = "x86_64")]
        features.insert("x86-64".to_string());

        #[cfg(target_arch = "x86")]
        features.insert("i386".to_string());

        #[cfg(target_arch = "aarch64")]
        features.insert("aarch64".to_string());

        #[cfg(target_arch = "arm")]
        features.insert("arm".to_string());

        // Endianness
        #[cfg(target_endian = "little")]
        features.insert("little-endian".to_string());

        #[cfg(target_endian = "big")]
        features.insert("big-endian".to_string());

        Self { features }
    }

    /// Check if a feature is supported
    pub fn has_feature(&self, name: &str) -> bool {
        self.features.contains(name)
    }

    /// Get all supported features as a sorted vector
    pub fn all_features(&self) -> Vec<String> {
        let mut features: Vec<_> = self.features.iter().cloned().collect();
        features.sort();
        features
    }

    /// Add a custom feature (for testing or extensions)
    pub fn add_feature(&mut self, name: &str) {
        self.features.insert(name.to_string());
    }

    /// Remove a feature (for testing)
    #[cfg(test)]
    pub fn remove_feature(&mut self, name: &str) {
        self.features.remove(name);
    }
}

impl Default for FeatureRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Global feature registry instance
/// Using a function to avoid lazy_static dependency
pub fn default_features() -> FeatureRegistry {
    FeatureRegistry::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_features() {
        let features = FeatureRegistry::new();

        // Must have r7rs
        assert!(features.has_feature("r7rs"));

        // Must have patina
        assert!(features.has_feature("patina"));

        // Numeric features
        assert!(features.has_feature("ratios"));
        assert!(features.has_feature("exact-closed"));
        assert!(features.has_feature("ieee-float"));

        // Unicode
        assert!(features.has_feature("full-unicode"));
    }

    #[test]
    fn test_platform_features() {
        let features = FeatureRegistry::new();

        // At least one platform should be detected
        let has_platform = features.has_feature("darwin")
            || features.has_feature("linux")
            || features.has_feature("windows")
            || features.has_feature("freebsd");
        assert!(has_platform, "No platform detected");

        // At least one architecture should be detected
        let has_arch = features.has_feature("x86-64")
            || features.has_feature("aarch64")
            || features.has_feature("arm")
            || features.has_feature("i386");
        assert!(has_arch, "No architecture detected");

        // Endianness should be detected
        let has_endian =
            features.has_feature("little-endian") || features.has_feature("big-endian");
        assert!(has_endian, "No endianness detected");
    }

    #[test]
    fn test_custom_features() {
        let mut features = FeatureRegistry::new();

        assert!(!features.has_feature("my-extension"));

        features.add_feature("my-extension");
        assert!(features.has_feature("my-extension"));

        features.remove_feature("my-extension");
        assert!(!features.has_feature("my-extension"));
    }

    #[test]
    fn test_all_features_sorted() {
        let features = FeatureRegistry::new();
        let all = features.all_features();

        // Should be sorted
        let mut sorted = all.clone();
        sorted.sort();
        assert_eq!(all, sorted);
    }
}
