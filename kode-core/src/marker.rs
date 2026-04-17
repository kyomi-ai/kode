use crate::Position;

#[derive(Debug, Clone, PartialEq)]
pub enum MarkerSeverity {
    Error,
    Warning,
    Info,
    Hint,
}

#[derive(Debug, Clone)]
pub struct Marker {
    pub start: Position,
    pub end: Position,
    pub message: String,
    pub severity: MarkerSeverity,
}
