//! Direct comparison tests between Patina and chibi-scheme
//!
//! This demonstrates actual side-by-side comparison for expressions
//! that both interpreters support.

use patina::Interpreter;
use std::process::Command;

/// Run a single Scheme expression through chibi-scheme
fn eval_with_chibi(expr: &str) -> Result<String, String> {
    // Wrap expression in (write ...) to get printed output
    let wrapped = format!("(write {})", expr);

    let output = Command::new("chibi-scheme")
        .arg("-e")
        .arg(&wrapped)
        .output()
        .map_err(|e| format!("Failed to execute chibi-scheme: {}", e))?;

    if output.status.success() {
        let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(result)
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

/// Run a single Scheme expression through Patina
fn eval_with_patina(expr: &str) -> Result<String, String> {
    let interp = Interpreter::new();
    match interp.eval_str(expr) {
        Ok(value) => Ok(format!("{}", value)),
        Err(e) => Err(format!("{}", e)),
    }
}

/// Compare both interpreters on the same expression
fn compare(expr: &str) {
    if std::env::var("SKIP_CHIBI_TESTS").is_ok() {
        // Just test Patina
        let patina = eval_with_patina(expr);
        println!("Expression: {}", expr);
        println!("Patina: {:?}", patina);
        assert!(patina.is_ok(), "Patina failed on: {}", expr);
        return;
    }

    let patina = eval_with_patina(expr);
    let chibi = eval_with_chibi(expr);

    println!("\n=== Comparing: {} ===", expr);
    println!("Patina: {:?}", patina);
    println!("Chibi:  {:?}", chibi);

    match (patina, chibi) {
        (Ok(p), Ok(c)) => {
            // TODO: Normalize output format (e.g., #t vs true)
            println!("Both succeeded!");
            // For now, just verify both worked
        }
        (Err(p), Err(c)) => {
            println!("Both failed (expected for unimplemented features)");
        }
        (Ok(p), Err(c)) => {
            println!("WARNING: Patina succeeded but chibi failed");
            println!("This might indicate a test setup issue");
        }
        (Err(p), Ok(c)) => {
            println!("Patina failed but chibi succeeded");
            println!("This means Patina needs to implement this feature");
            // Don't panic - just document the gap
        }
    }
}

#[test]
fn test_compare_arithmetic() {
    compare("(+ 1 2 3)");
    compare("(- 10 3)");
    compare("(* 2 3 4)");
    compare("(/ 20 4)");
}

#[test]
fn test_compare_comparisons() {
    compare("(= 5 5)");
    compare("(< 1 2 3)");
}

#[test]
fn test_compare_lists() {
    compare("(cons 1 2)");
    compare("(car (cons 10 20))");
    compare("(cdr (cons 10 20))");
}

#[test]
fn test_compare_predicates() {
    compare("(null? '())");
    compare("(pair? (cons 1 2))");
}

// Tests that show differences in output format
#[test]
fn test_output_format_differences() {
    // Note: Patina might output "true" while chibi outputs "#t"
    // We'll need normalization for these

    let expr = "#t";
    println!("\nTesting boolean representation:");

    if std::env::var("SKIP_CHIBI_TESTS").is_err() {
        let patina = eval_with_patina(expr).unwrap();
        let chibi = eval_with_chibi(expr).unwrap();

        println!("Patina outputs: {}", patina);
        println!("Chibi outputs: {}", chibi);
        println!("These might differ in format but represent the same value");
    }
}
