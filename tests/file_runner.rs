//! File-based integration tests
//!
//! This test runner:
//! 1. Runs .scm files through chibi-scheme to get reference outputs
//! 2. Compares with Patina's output (when features are implemented)
//! 3. Documents compliance gaps

use std::fs;
use std::path::Path;
use std::process::Command;

/// Check if chibi-scheme is available in PATH
fn is_chibi_scheme_available() -> bool {
    Command::new("chibi-scheme")
        .arg("-V")
        .output()
        .map(|out| out.status.success() || !out.stderr.is_empty())
        .unwrap_or(false)
}

/// Run a .scm file through chibi-scheme and capture output
fn run_chibi_scheme(file_path: &Path) -> Result<String, String> {
    if !is_chibi_scheme_available() {
        return Err(
            "chibi-scheme not found in PATH. Install it or set SKIP_CHIBI_TESTS=1".to_string(),
        );
    }

    let output = Command::new("chibi-scheme")
        .arg(file_path)
        .output()
        .map_err(|e| format!("Failed to execute chibi-scheme: {}", e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

/// Skip test if chibi-scheme is not available and SKIP_CHIBI_TESTS is set
fn skip_if_chibi_unavailable() {
    if !is_chibi_scheme_available() {
        if std::env::var("SKIP_CHIBI_TESTS").is_ok() {
            println!("Skipping test: chibi-scheme not available (SKIP_CHIBI_TESTS=1)");
            return;
        }
        panic!(
            "chibi-scheme not found. Install it or set SKIP_CHIBI_TESTS=1 to skip these tests.\n\
             Installation:\n\
             - macOS: brew install chibi-scheme\n\
             - Ubuntu: apt-get install chibi-scheme\n\
             - Source: https://github.com/ashinn/chibi-scheme"
        );
    }
}

/// Generate snapshot files from chibi-scheme output
#[test]
#[ignore] // Run with --ignored to regenerate snapshots
fn generate_snapshots() {
    let test_files = [
        "tests/schemes/arithmetic/basic.scm",
        "tests/schemes/arithmetic/comparisons.scm",
        "tests/schemes/lists/basic.scm",
        "tests/schemes/control/if.scm",
        "tests/schemes/control/begin.scm",
    ];

    for file in &test_files {
        let path = Path::new(file);
        if !path.exists() {
            println!("Skipping {}: file not found", file);
            continue;
        }

        match run_chibi_scheme(path) {
            Ok(output) => {
                let snapshot_path = path.with_extension("expected");
                fs::write(&snapshot_path, &output).expect("Failed to write snapshot");
                println!("Generated snapshot: {}", snapshot_path.display());
                println!("Output:\n{}", output);
            }
            Err(e) => {
                println!("Failed to run {}: {}", file, e);
            }
        }
    }
}

#[test]
fn verify_chibi_scheme_available() {
    if std::env::var("SKIP_CHIBI_TESTS").is_ok() {
        println!("Skipping chibi-scheme check (SKIP_CHIBI_TESTS=1)");
        return;
    }

    if is_chibi_scheme_available() {
        println!("✓ chibi-scheme is available");
    } else {
        panic!(
            "chibi-scheme not found in PATH.\n\
             Installation instructions:\n\
             - macOS: brew install chibi-scheme\n\
             - Ubuntu/Debian: apt-get install chibi-scheme\n\
             - Arch: pacman -S chibi-scheme\n\
             - From source: https://github.com/ashinn/chibi-scheme\n\n\
             To skip these tests: SKIP_CHIBI_TESTS=1 cargo test"
        );
    }
}

// TODO: Add Patina comparison tests once we implement:
// - import statements (or skip them)
// - write procedure
// - display procedure
// - (+) returning 0 and (*) returning 1
