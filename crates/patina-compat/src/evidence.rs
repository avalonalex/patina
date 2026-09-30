//! Bounded excerpts for failed measurements; never used to choose a verdict.

use std::io::Read;
use std::path::Path;

const MAX_LINES: usize = 32;
const MAX_BYTES: usize = 8 * 1024;
const MAX_LINE_BYTES: usize = 512;
const MAX_LOG_BYTES: u64 = 1024 * 1024;
const TRUNCATED: &str = "[evidence truncated]";

#[derive(Default)]
pub(crate) struct Evidence {
    lines: Vec<String>,
    bytes: usize,
    truncated: bool,
}

impl Evidence {
    pub(crate) fn push(&mut self, text: &str) {
        for line in text.lines() {
            let clean = plain(line);
            let line = clean.trim();
            if line.is_empty() {
                continue;
            }
            // Reserve a line and bytes for an explicit truncation marker.
            let available = (MAX_BYTES - TRUNCATED.len()).saturating_sub(self.bytes);
            if self.lines.len() >= MAX_LINES - 1 || available == 0 {
                self.truncated = true;
                continue;
            }
            let end = line.floor_char_boundary(line.len().min(MAX_LINE_BYTES).min(available));
            if end < line.len() {
                self.truncated = true;
            }
            if end > 0 {
                self.lines.push(line[..end].to_string());
                self.bytes += end;
            }
        }
    }

    pub(crate) fn finish(mut self) -> Vec<String> {
        if self.truncated {
            self.lines.push(TRUNCATED.into());
        }
        self.lines
    }
}

pub(crate) fn bounded(text: &str) -> Vec<String> {
    let mut evidence = Evidence::default();
    evidence.push(text);
    evidence.finish()
}

/// Drop terminal CSI sequences (including colour) and nonprinting controls.
fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
        } else if !c.is_control() || c == '\t' {
            out.push(c);
        }
    }
    out
}

/// Keep failure tallies first, then SRFI 64 records and console context. The
/// existing classifier supplies its predicate, so recognizing evidence cannot
/// silently widen or narrow the set of failing suites.
pub(crate) fn test_output(
    stdout: &str,
    scratch: Option<&Path>,
    is_failure: fn(&str) -> bool,
) -> Vec<String> {
    let lines: Vec<String> = stdout.lines().map(plain).collect();
    let mut included = vec![false; lines.len()];
    let mut evidence = Evidence::default();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if is_failure(line)
            && (trimmed.starts_with("# of ")
                || trimmed.starts_with("; *** checks ***")
                || trimmed
                    .split_whitespace()
                    .next()
                    .is_some_and(|n| n.parse::<u64>().is_ok()))
        {
            evidence.push(line);
            included[i] = true;
        }
    }
    if let Some(scratch) = scratch {
        for line in &lines {
            if !line.starts_with("%%%% Starting test ") {
                continue;
            }
            if let Some((_, rest)) = line.split_once("Writing full log to \"")
                && let Some((name, _)) = rest.rsplit_once("\")")
            {
                srfi64_log(scratch, name, &mut evidence);
            }
        }
    }
    for (i, line) in lines.iter().enumerate() {
        if is_failure(line) || line.trim_start().starts_with("ERROR:") {
            for j in i.saturating_sub(2)..(i + 5).min(lines.len()) {
                if included[j] {
                    continue;
                }
                included[j] = true;
                let context = &lines[j];
                // Framework durations are not failure evidence. Retain the
                // tally and any skipped-test suffix, omitting just elapsed time.
                if let Some((tally, duration)) = context.split_once(" tests passed in ")
                    && let Some((_, suffix)) = duration.split_once(" seconds")
                {
                    evidence.push(&format!("{tally} tests passed{suffix}"));
                } else {
                    evidence.push(context);
                }
            }
        }
    }
    let lines = evidence.finish();
    if lines.is_empty() {
        bounded("Suite reported failures without per-case details on stdout")
    } else {
        lines
    }
}

fn srfi64_log(scratch: &Path, name: &str, evidence: &mut Evidence) {
    let read = || -> Result<Vec<u8>, String> {
        let root = scratch.canonicalize().map_err(|e| e.to_string())?;
        let path = scratch
            .join(name)
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if !path.starts_with(&root) || !path.is_file() {
            return Err("not a regular file inside the run's scratch directory".into());
        }
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .and_then(|file| file.take(MAX_LOG_BYTES + 1).read_to_end(&mut bytes))
            .map_err(|e| e.to_string())?;
        Ok(bytes)
    };
    let bytes = match read() {
        Ok(bytes) => bytes,
        Err(error) => {
            evidence.push(&format!("SRFI 64 log {name}: {error}"));
            return;
        }
    };
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_LOG_BYTES as usize)]);
    for block in text.split("Test begin:").skip(1) {
        if block.lines().any(|line| {
            matches!(
                line.trim(),
                "result-kind: fail" | "result-kind: xpass" | "result-kind: error"
            )
        }) {
            evidence.push(&format!("SRFI 64 log {name}:"));
            for line in block
                .lines()
                .take_while(|line| !line.starts_with("Group end:") && !line.starts_with("# of "))
            {
                if line.trim() != "Test end:" {
                    evidence.push(line);
                }
            }
        }
    }
    if bytes.len() > MAX_LOG_BYTES as usize {
        evidence.push(&format!(
            "[SRFI 64 log {name}: scan truncated at {MAX_LOG_BYTES} bytes]"
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excerpts_bound_lines_bytes_and_unicode_without_terminal_controls() {
        assert_eq!(
            bounded("\u{1b}[31mFAIL\u{1b}[0m\0\n\n detail"),
            ["FAIL", "detail"]
        );
        for text in [
            "λ".repeat(1000),
            "failure\n".repeat(100),
            ("λ".repeat(250) + "\n").repeat(100),
        ] {
            let lines = bounded(&text);
            assert!(lines.len() <= MAX_LINES);
            assert!(lines.iter().map(String::len).sum::<usize>() <= MAX_BYTES);
            assert!(lines.iter().all(|line| line.len() <= MAX_LINE_BYTES));
            assert_eq!(lines.last().unwrap(), TRUNCATED);
        }
    }

    #[test]
    fn srfi64_keeps_failed_and_unexpected_pass_records_only() {
        let dir = tempfile::tempdir().unwrap();
        let log = ["pass", "xfail", "skip", "fail", "xpass", "error"]
            .map(|kind| format!("Test begin:\n  test-name: case-{kind}\nTest end:\n  result-kind: {kind}\n  actual-value: 4\n  expected-value: 5\n"))
            .join("");
        std::fs::write(dir.path().join("suite.log"), log).unwrap();
        let mut evidence = Evidence::default();
        srfi64_log(dir.path(), "suite.log", &mut evidence);
        let lines = evidence.finish();
        for kind in ["fail", "xpass", "error"] {
            assert!(
                lines.contains(&format!("test-name: case-{kind}")),
                "{lines:?}"
            );
        }
        for kind in ["pass", "xfail", "skip"] {
            assert!(
                !lines.contains(&format!("test-name: case-{kind}")),
                "{lines:?}"
            );
        }
        assert_eq!(
            lines
                .iter()
                .filter(|line| *line == "expected-value: 5")
                .count(),
            3
        );
    }

    #[test]
    fn missing_outside_and_oversize_logs_leave_an_explanation() {
        let dir = tempfile::tempdir().unwrap();
        let scratch = dir.path().join("scratch");
        std::fs::create_dir(&scratch).unwrap();
        std::fs::write(dir.path().join("outside.log"), "private contents").unwrap();
        std::fs::write(
            scratch.join("large.log"),
            vec![b'x'; MAX_LOG_BYTES as usize + 1],
        )
        .unwrap();
        let mut evidence = Evidence::default();
        for name in ["missing.log", "../outside.log", "large.log"] {
            srfi64_log(&scratch, name, &mut evidence);
        }
        let text = evidence.finish().join("\n");
        assert!(text.contains("SRFI 64 log missing.log:"));
        assert!(text.contains("not a regular file inside"));
        assert!(!text.contains("private contents"));
        assert!(text.contains("scan truncated at 1048576 bytes"));
    }
}
