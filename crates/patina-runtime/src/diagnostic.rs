//! Machine-readable diagnostics, independent of user-facing error wording.
//!
//! The CLI optionally writes a versioned JSON-lines sidecar. Only errors
//! reported to the caller belong here; constructing or catching an error must
//! not emit a record. The separate stream cannot confuse Scheme output with
//! interpreter diagnostics. These categories describe failure causes for
//! tools; they do not determine Scheme condition types or catchability.

use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::fs::File;
use std::io::Write;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticKind {
    Parse,
    Syntax,
    MissingLibrary,
    UnboundIdentifier,
    Load,
    NativeExtension,
    Runtime,
    Io,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    /// Human-readable detail, never used to determine the kind.
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extension: Option<String>,
}

impl Diagnostic {
    pub fn new(kind: DiagnosticKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            path: None,
            library: None,
            identifier: None,
            extension: None,
        }
    }

    pub fn at_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// A body that parsed but failed to compile/evaluate is a load failure.
    /// Preserve a more specific dependency or reader failure from inside it.
    pub fn in_library(mut self, path: Option<&Path>) -> Self {
        if matches!(
            self.kind,
            DiagnosticKind::Runtime | DiagnosticKind::UnboundIdentifier
        ) {
            self.kind = DiagnosticKind::Load;
        }
        if self.path.is_none() {
            self.path = path.map(|p| p.display().to_string());
        }
        self
    }
}

/// Preserve a failure's kind and payload across display-oriented wrappers.
pub trait HasDiagnostic {
    fn diagnostic(&self) -> Diagnostic;
}

const HEADER: &str = r#"{"protocol":"patina-diagnostics","version":1}"#;
thread_local! {
    static OUTPUT: RefCell<Option<File>> = const { RefCell::new(None) };
}

/// Start a fresh stream, including a header even when the run has no errors.
pub fn start(path: &Path) -> std::io::Result<()> {
    let mut file = File::create(path)?;
    writeln!(file, "{HEADER}")?;
    file.flush()?;
    OUTPUT.with(|output| *output.borrow_mut() = Some(file));
    Ok(())
}

/// Record an error at the point it is reported, preserving ordinary stderr.
pub fn emit(diagnostic: Diagnostic) {
    OUTPUT.with(|output| {
        if let Some(file) = output.borrow_mut().as_mut() {
            let result = serde_json::to_writer(&mut *file, &diagnostic)
                .map_err(std::io::Error::other)
                .and_then(|()| writeln!(file))
                .and_then(|()| file.flush());
            if let Err(error) = result {
                eprintln!("Cannot write diagnostic stream: {error}");
                std::process::exit(1);
            }
        }
    });
}

/// Reject absent, truncated, unsupported or malformed streams, including a
/// stale binary that does not implement the CLI option. Never fall back to prose.
pub fn read_stream(text: &str) -> Result<Vec<Diagnostic>, String> {
    let mut lines = text.lines();
    let header: serde_json::Value =
        serde_json::from_str(lines.next().ok_or("missing diagnostic header")?)
            .map_err(|e| format!("invalid diagnostic header: {e}"))?;
    if header != serde_json::from_str::<serde_json::Value>(HEADER).unwrap() {
        return Err("unsupported diagnostic protocol/version".into());
    }
    let diagnostics: Vec<Diagnostic> = lines
        .map(|line| {
            serde_json::from_str(line).map_err(|e| format!("invalid diagnostic record: {e}"))
        })
        .collect::<Result<_, _>>()?;
    for d in &diagnostics {
        let valid = match d.kind {
            DiagnosticKind::MissingLibrary => {
                d.library.as_ref().is_some_and(|name| !name.is_empty())
            }
            DiagnosticKind::UnboundIdentifier => d.identifier.is_some(),
            DiagnosticKind::NativeExtension => d.extension.is_some(),
            _ => true,
        };
        if !valid {
            return Err(format!(
                "diagnostic {:?} lacks its required payload",
                d.kind
            ));
        }
    }
    Ok(diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_lines_preserve_raw_names_paths_and_multiline_messages() {
        let mut d = Diagnostic::new(
            DiagnosticKind::UnboundIdentifier,
            "new wording:\nsecond line",
        )
        .at_path("dir: 'quoted'/λ\nfile.scm");
        d.identifier = Some("odd name: `λ`\n".into());
        let encoded = format!("{HEADER}\n{}\n", serde_json::to_string(&d).unwrap());
        assert_eq!(encoded.lines().count(), 2);
        assert_eq!(read_stream(&encoded).unwrap(), [d]);
        assert!(read_stream(&format!("{HEADER}\n")).unwrap().is_empty());
    }

    #[test]
    fn absent_malformed_unknown_or_incomplete_protocols_are_rejected() {
        for stream in [
            String::new(),
            "human error text".into(),
            "{\"protocol\":\"patina-diagnostics\",\"version\":2}\n".into(),
            format!("{HEADER}\n{{\"kind\":\"parse\""),
            format!("{HEADER}\n{{\"kind\":\"future-kind\",\"message\":\"x\"}}\n"),
            format!("{HEADER}\n{{\"kind\":\"missing-library\",\"message\":\"x\"}}\n"),
            format!("{HEADER}\n{{\"kind\":\"unbound-identifier\",\"message\":\"x\"}}\n"),
            format!("{HEADER}\n{{\"kind\":\"native-extension\",\"message\":\"x\"}}\n"),
        ] {
            assert!(read_stream(&stream).is_err(), "accepted {stream:?}");
        }
    }
}
