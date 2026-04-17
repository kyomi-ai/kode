use crate::{Marker, MarkerSeverity, Position};

/// Severity levels, matching LSP `DiagnosticSeverity` semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Information,
    Hint,
}

/// A single diagnostic finding from a [`DiagnosticProvider`].
///
/// Modelled after LSP's `Diagnostic` but simplified — no URI (the editor
/// owns the document), no code/tags (can be added later).
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub start: Position,
    pub end: Position,
    pub severity: DiagnosticSeverity,
    pub message: String,
    /// Identifies the provider that produced this diagnostic
    /// (e.g. `"bigquery"`, `"jsonschema"`, `"python"`).
    pub source: Option<String>,
}

impl Diagnostic {
    pub fn error(start: Position, end: Position, message: impl Into<String>) -> Self {
        Self {
            start,
            end,
            severity: DiagnosticSeverity::Error,
            message: message.into(),
            source: None,
        }
    }

    pub fn warning(start: Position, end: Position, message: impl Into<String>) -> Self {
        Self {
            start,
            end,
            severity: DiagnosticSeverity::Warning,
            message: message.into(),
            source: None,
        }
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }
}

impl From<DiagnosticSeverity> for MarkerSeverity {
    fn from(s: DiagnosticSeverity) -> Self {
        match s {
            DiagnosticSeverity::Error => MarkerSeverity::Error,
            DiagnosticSeverity::Warning => MarkerSeverity::Warning,
            DiagnosticSeverity::Information => MarkerSeverity::Info,
            DiagnosticSeverity::Hint => MarkerSeverity::Hint,
        }
    }
}

impl From<Diagnostic> for Marker {
    fn from(d: Diagnostic) -> Self {
        Marker {
            start: d.start,
            end: d.end,
            message: d.message,
            severity: d.severity.into(),
        }
    }
}
