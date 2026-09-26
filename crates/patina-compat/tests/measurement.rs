use std::process::Command;

/// Re-rendering is often done long after a measurement. Its date and backend
/// must come from the snapshot, including an explicit unknown date for old
/// snapshots. Exercise the CLI so it cannot accidentally stamp reports anew.
#[test]
fn report_preserves_the_measurement_time_or_its_absence() {
    let dir = tempfile::tempdir().unwrap();
    let results = dir.path().join("results.scm");
    for (metadata, measured) in [
        (
            "(measured-at \"2000-02-29T23:59:59Z\")",
            "2000-02-29T23:59:59Z",
        ),
        ("", "unknown (not recorded in this snapshot)"),
    ] {
        let source = format!(
            "(patina-compat-results (version 1) (backend \"vm\") {metadata}
             (results ((slug \"example\") (mode probe) (status pass))))"
        );
        std::fs::write(&results, &source).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_patina-compat"))
            .args([
                "report",
                "--tree-walker",
                "--exclusions",
                "none",
                "--results",
            ])
            .arg(&results)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report = String::from_utf8(output.stdout).unwrap();
        assert!(report.contains("(vm backend)"), "{report}");
        assert!(
            report.contains(&format!("**Measured:** {measured}\n")),
            "{report}"
        );
        assert!(report.contains("**1 of 1 packages pass.**"), "{report}");
        assert_eq!(std::fs::read_to_string(&results).unwrap(), source);
    }
}
