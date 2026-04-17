//! JSON Schema diagnostic provider for kode.
//!
//! Validates YAML or JSON editor content against a JSON Schema and returns
//! diagnostics with source-accurate line/col positions.
//!
//! Gated behind the `schema` feature flag.

use std::collections::HashMap;
use std::sync::Arc;

use kode_core::{Diagnostic, DiagnosticSeverity, Position};
use saphyr_parser::{Event, Parser as YamlParser, Span};

use crate::DiagnosticProvider;

/// A compiled JSON Schema ready for repeated validation.
///
/// Created once at construction time, shared across invocations via `Arc`.
struct CompiledSchema {
    schemas: boon::Schemas,
    index: boon::SchemaIndex,
}

/// Create a [`DiagnosticProvider`] that validates YAML or JSON content against
/// the given JSON Schema.
///
/// Schema compilation happens once at construction time. Each invocation
/// parses the editor text, validates it, and maps errors to source positions.
///
/// Auto-detects JSON vs YAML by the first non-whitespace character.
///
/// # Panics
///
/// Panics if `schema_json` is not valid JSON or not a valid JSON Schema.
/// The schema is a programmer-provided constant, not user input.
///
/// # Example
///
/// ```ignore
/// let provider = json_schema_provider(include_str!("my.schema.json"));
/// view! {
///     <CodeEditor
///         diagnostic_providers=Signal::stored(vec![provider])
///     />
/// }
/// ```
pub fn json_schema_provider(schema_json: &str) -> DiagnosticProvider {
    let schema_value: serde_json::Value =
        serde_json::from_str(schema_json).expect("json_schema_provider: invalid JSON schema");

    let mut schemas = boon::Schemas::new();
    let mut compiler = boon::Compiler::new();
    compiler
        .add_resource("schema.json", schema_value)
        .expect("json_schema_provider: failed to add schema resource");
    let index = compiler
        .compile("schema.json", &mut schemas)
        .expect("json_schema_provider: failed to compile schema");

    let compiled = Arc::new(CompiledSchema { schemas, index });

    Arc::new(move |text: String, _version: u64| {
        let compiled = Arc::clone(&compiled);
        Box::pin(async move { validate_against_schema(&text, &compiled) })
    })
}

// ── Validation pipeline ──────────────────────────────────────────────────

fn validate_against_schema(text: &str, compiled: &CompiledSchema) -> Vec<Diagnostic> {
    let trimmed = text.trim_start();
    if trimmed.is_empty() {
        return vec![];
    }

    let is_json = matches!(trimmed.as_bytes().first(), Some(b'{' | b'['));

    // Step 1: Parse to serde_json::Value
    let value: serde_json::Value = if is_json {
        match serde_json::from_str(text) {
            Ok(v) => v,
            Err(e) => return vec![json_parse_error(e)],
        }
    } else {
        match serde_saphyr::from_str(text) {
            Ok(v) => v,
            Err(e) => return vec![yaml_parse_error(e)],
        }
    };

    // Step 2: Build position map (JSON pointer → source position)
    let position_map = if is_json {
        build_json_position_map(text)
    } else {
        build_yaml_position_map(text)
    };

    // Step 3: Validate against schema
    match compiled.schemas.validate(&value, compiled.index) {
        Ok(()) => vec![],
        Err(err) => collect_diagnostics(&err, &position_map),
    }
}

// ── Parse error helpers ──────────────────────────────────────────────────

fn json_parse_error(e: serde_json::Error) -> Diagnostic {
    // serde_json lines are 1-indexed, kode is 0-indexed
    let line = e.line().saturating_sub(1);
    let col = e.column().saturating_sub(1);
    Diagnostic {
        start: Position::new(line, col),
        end: Position::new(line, col + 1),
        severity: DiagnosticSeverity::Error,
        message: format!("JSON parse error: {e}"),
        source: Some("json-schema".into()),
    }
}

fn yaml_parse_error(e: serde_saphyr::Error) -> Diagnostic {
    // TODO: extract line/col from serde-saphyr error instead of falling back to (0,0).
    // The error's Display includes location info but the struct doesn't expose it.
    Diagnostic {
        start: Position::zero(),
        end: Position::new(0, 1),
        severity: DiagnosticSeverity::Error,
        message: format!("YAML parse error: {e}"),
        source: Some("json-schema".into()),
    }
}

// ── Schema error collection ──────────────────────────────────────────────

/// Position range in source text for a given JSON pointer path.
type PositionMap = HashMap<String, (Position, Position)>;

fn collect_diagnostics(
    err: &boon::ValidationError<'_, '_>,
    position_map: &PositionMap,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    collect_leaf_errors(err, position_map, &mut out);
    out
}

fn collect_leaf_errors(
    err: &boon::ValidationError<'_, '_>,
    position_map: &PositionMap,
    out: &mut Vec<Diagnostic>,
) {
    // Skip structural wrappers that just group sub-errors
    if !err.causes.is_empty() {
        for cause in &err.causes {
            collect_leaf_errors(cause, position_map, out);
        }
        return;
    }

    let pointer = err.instance_location.to_string();
    let (start, end) = position_map
        .get(&pointer)
        .copied()
        .unwrap_or((Position::zero(), Position::new(0, 1)));

    let message = format_error(err, &pointer);
    out.push(Diagnostic {
        start,
        end,
        severity: DiagnosticSeverity::Error,
        message,
        source: Some("json-schema".into()),
    });
}

fn format_error(err: &boon::ValidationError<'_, '_>, pointer: &str) -> String {
    let kind_msg = format!("{}", err.kind);
    if pointer.is_empty() {
        kind_msg
    } else {
        let readable = pointer_to_dot_notation(pointer);
        format!("{readable}: {kind_msg}")
    }
}

/// Convert a JSON pointer like `/panels/0/type` to dot notation `panels[0].type`.
fn pointer_to_dot_notation(pointer: &str) -> String {
    let mut result = String::new();
    for segment in pointer.split('/').filter(|s| !s.is_empty()) {
        // Unescape JSON pointer: ~1 → /, ~0 → ~
        let unescaped = segment.replace("~1", "/").replace("~0", "~");
        if let Ok(idx) = unescaped.parse::<usize>() {
            result.push_str(&format!("[{idx}]"));
        } else {
            if !result.is_empty() {
                result.push('.');
            }
            result.push_str(&unescaped);
        }
    }
    result
}

// ── YAML position mapping ────────────────────────────────────────────────

/// Walk the saphyr-parser event stream to build a map from JSON pointer paths
/// to source positions.
///
/// The parser emits events like `MappingStart`, `Scalar`, `SequenceStart` etc.
/// with `Span` containing start/end `Marker`s (line/col, 1-indexed).
///
/// We maintain a path stack to track the current JSON pointer and record the
/// position of each key/value.
fn build_yaml_position_map(text: &str) -> PositionMap {
    let mut map = PositionMap::new();
    let parser = YamlParser::new_from_str(text);

    let events: Vec<(Event<'_>, Span)> = parser
        .into_iter()
        .filter_map(|r| r.ok())
        .collect();

    let mut ctx = YamlWalkContext {
        events: &events,
        pos: 0,
        map: &mut map,
    };
    // Skip StreamStart
    if matches!(ctx.peek_event(), Some(Event::StreamStart)) {
        ctx.pos += 1;
    }
    // Skip DocumentStart
    if matches!(ctx.peek_event(), Some(Event::DocumentStart(_))) {
        ctx.pos += 1;
    }
    // Walk the root value
    let mut path = Vec::new();
    walk_yaml_value(&mut ctx, &mut path);

    map
}

struct YamlWalkContext<'a, 'e> {
    events: &'a [(Event<'e>, Span)],
    pos: usize,
    map: &'a mut PositionMap,
}

impl<'a, 'e> YamlWalkContext<'a, 'e> {
    fn peek_event(&self) -> Option<&Event<'e>> {
        self.events.get(self.pos).map(|(e, _)| e)
    }

    fn peek_span(&self) -> Option<&Span> {
        self.events.get(self.pos).map(|(_, s)| s)
    }

    fn advance(&mut self) {
        self.pos += 1;
    }
}

fn span_to_positions(span: &Span) -> (Position, Position) {
    // saphyr Marker is 1-indexed, kode Position is 0-indexed
    let start = Position::new(
        span.start.line().saturating_sub(1),
        span.start.col().saturating_sub(1),
    );
    let end = if span.end.line() > 0 && span.end.col() > 0 {
        Position::new(
            span.end.line().saturating_sub(1),
            span.end.col().saturating_sub(1),
        )
    } else {
        Position::new(start.line, start.col + 1)
    };
    // Ensure end is at least 1 char past start for visible underlines
    if start == end {
        (start, Position::new(end.line, end.col + 1))
    } else {
        (start, end)
    }
}

fn build_pointer(path: &[PathSegment]) -> String {
    let mut pointer = String::new();
    for seg in path {
        pointer.push('/');
        match seg {
            PathSegment::Key(k) => {
                for ch in k.chars() {
                    match ch {
                        '~' => pointer.push_str("~0"),
                        '/' => pointer.push_str("~1"),
                        c => pointer.push(c),
                    }
                }
            }
            PathSegment::Index(idx) => {
                pointer.push_str(&idx.to_string());
            }
        }
    }
    pointer
}

enum PathSegment {
    Key(String),
    Index(usize),
}

fn walk_yaml_value(ctx: &mut YamlWalkContext<'_, '_>, path: &mut Vec<PathSegment>) {
    let Some(event) = ctx.peek_event().cloned() else {
        return;
    };

    match event {
        Event::MappingStart(_, _) => walk_yaml_mapping(ctx, path),
        Event::SequenceStart(_, _) => walk_yaml_sequence(ctx, path),
        Event::Scalar(_, _, _, _) | Event::Alias(_) => {
            // Record scalar value position
            if let Some(span) = ctx.peek_span() {
                let pointer = build_pointer(path);
                let positions = span_to_positions(span);
                ctx.map.insert(pointer, positions);
            }
            ctx.advance();
        }
        _ => {
            ctx.advance();
        }
    }
}

fn walk_yaml_mapping(ctx: &mut YamlWalkContext<'_, '_>, path: &mut Vec<PathSegment>) {
    // Record the mapping itself at the current path
    if let Some(span) = ctx.peek_span() {
        let pointer = build_pointer(path);
        let positions = span_to_positions(span);
        ctx.map.insert(pointer, positions);
    }
    ctx.advance(); // consume MappingStart

    loop {
        match ctx.peek_event() {
            Some(Event::MappingEnd) => {
                ctx.advance();
                return;
            }
            None => return,
            _ => {}
        }

        // Read key
        let key_name = match ctx.peek_event() {
            Some(Event::Scalar(s, _, _, _)) => {
                let name = s.to_string();
                ctx.advance(); // consume key scalar
                name
            }
            _ => {
                // Non-scalar key (unusual) — skip
                ctx.advance();
                continue;
            }
        };

        // Walk value with key on the path
        path.push(PathSegment::Key(key_name));
        walk_yaml_value(ctx, path);
        path.pop();
    }
}

fn walk_yaml_sequence(ctx: &mut YamlWalkContext<'_, '_>, path: &mut Vec<PathSegment>) {
    // Record the sequence itself at the current path
    if let Some(span) = ctx.peek_span() {
        let pointer = build_pointer(path);
        let positions = span_to_positions(span);
        ctx.map.insert(pointer, positions);
    }
    ctx.advance(); // consume SequenceStart

    let mut index = 0usize;
    loop {
        match ctx.peek_event() {
            Some(Event::SequenceEnd) => {
                ctx.advance();
                return;
            }
            None => return,
            _ => {}
        }

        path.push(PathSegment::Index(index));
        walk_yaml_value(ctx, path);
        path.pop();
        index += 1;
    }
}

// ── JSON position mapping ────────────────────────────────────────────────

/// Scan JSON text to build a position map for keys.
///
/// Tracks line/col while walking through the text, recording positions
/// of object keys and array indices.
fn build_json_position_map(text: &str) -> PositionMap {
    let mut ctx = JsonWalkContext {
        chars: text.chars().collect(),
        pos: 0,
        line: 0,
        col: 0,
        path: Vec::new(),
        map: PositionMap::new(),
    };

    // Record root position
    ctx.map.insert(String::new(), (Position::zero(), Position::new(0, 1)));

    ctx.walk_value();
    ctx.map
}

struct JsonWalkContext {
    chars: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
    path: Vec<PathSegment>,
    map: PositionMap,
}

impl JsonWalkContext {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn advance(&mut self) {
        if let Some(ch) = self.chars.get(self.pos) {
            self.pos += 1;
            if *ch == '\n' {
                self.line += 1;
                self.col = 0;
            } else {
                self.col += 1;
            }
        }
    }

    fn position(&self) -> Position {
        Position::new(self.line, self.col)
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek() {
            match ch {
                ' ' | '\t' | '\r' => self.advance(),
                '\n' => self.advance(),
                _ => return,
            }
        }
    }

    fn read_string(&mut self) -> String {
        let mut result = String::new();
        self.advance(); // skip opening '"'
        loop {
            match self.peek() {
                Some('"') => {
                    self.advance();
                    return result;
                }
                Some('\\') => {
                    self.advance();
                    if let Some(escaped) = self.peek() {
                        result.push(escaped);
                        self.advance();
                    }
                }
                Some(ch) => {
                    result.push(ch);
                    self.advance();
                }
                None => return result,
            }
        }
    }

    fn skip_scalar(&mut self) {
        match self.peek() {
            Some('"') => {
                self.read_string();
            }
            Some('{') => self.skip_balanced('{', '}'),
            Some('[') => self.skip_balanced('[', ']'),
            _ => {
                // number, bool, null — skip until delimiter
                while let Some(ch) = self.peek() {
                    if ch == ',' || ch == '}' || ch == ']' || ch == '\n' {
                        return;
                    }
                    self.advance();
                }
            }
        }
    }

    fn skip_balanced(&mut self, open: char, close: char) {
        let mut depth = 0;
        loop {
            match self.peek() {
                Some(ch) if ch == open => {
                    depth += 1;
                    self.advance();
                }
                Some(ch) if ch == close => {
                    depth -= 1;
                    self.advance();
                    if depth == 0 {
                        return;
                    }
                }
                Some('"') => {
                    self.read_string();
                }
                Some(_) => self.advance(),
                None => return,
            }
        }
    }

    fn walk_value(&mut self) {
        self.skip_whitespace();
        match self.peek() {
            Some('{') => self.walk_object(),
            Some('[') => self.walk_array(),
            _ => {
                let start = self.position();
                self.skip_scalar();
                let end = self.position();
                let pointer = build_pointer(&self.path);
                self.map.insert(pointer, (start, end));
            }
        }
    }

    fn walk_object(&mut self) {
        let start = self.position();
        let pointer = build_pointer(&self.path);
        self.map.entry(pointer).or_insert((start, Position::new(start.line, start.col + 1)));

        self.advance(); // skip '{'

        loop {
            self.skip_whitespace();
            match self.peek() {
                Some('}') => {
                    self.advance();
                    return;
                }
                Some(',') => {
                    self.advance();
                    self.skip_whitespace();
                }
                None => return,
                _ => {}
            }

            // Read key
            self.skip_whitespace();
            if self.peek() != Some('"') {
                return;
            }
            let key_start = self.position();
            let key = self.read_string();
            let key_end = self.position();

            // Skip colon
            self.skip_whitespace();
            if self.peek() == Some(':') {
                self.advance();
            }

            // Record key position and walk value
            self.path.push(PathSegment::Key(key));
            let value_pointer = build_pointer(&self.path);
            self.map.insert(value_pointer, (key_start, key_end));
            self.walk_value();
            self.path.pop();
        }
    }

    fn walk_array(&mut self) {
        let start = self.position();
        let pointer = build_pointer(&self.path);
        self.map.entry(pointer).or_insert((start, Position::new(start.line, start.col + 1)));

        self.advance(); // skip '['
        let mut index = 0usize;

        loop {
            self.skip_whitespace();
            match self.peek() {
                Some(']') => {
                    self.advance();
                    return;
                }
                Some(',') => {
                    self.advance();
                }
                None => return,
                _ => {}
            }

            self.skip_whitespace();
            if self.peek() == Some(']') {
                self.advance();
                return;
            }

            self.path.push(PathSegment::Index(index));
            self.walk_value();
            self.path.pop();
            index += 1;
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_schema(json: &str) -> Arc<CompiledSchema> {
        let schema_value: serde_json::Value = serde_json::from_str(json).unwrap();
        let mut schemas = boon::Schemas::new();
        let mut compiler = boon::Compiler::new();
        compiler.add_resource("test.json", schema_value).unwrap();
        let index = compiler.compile("test.json", &mut schemas).unwrap();
        Arc::new(CompiledSchema { schemas, index })
    }

    const SIMPLE_SCHEMA: &str = r#"{
        "type": "object",
        "required": ["name", "version"],
        "properties": {
            "name": { "type": "string" },
            "version": { "type": "number" },
            "tags": {
                "type": "array",
                "items": { "type": "string" }
            }
        },
        "additionalProperties": false
    }"#;

    #[test]
    fn valid_yaml_produces_no_diagnostics() {
        let compiled = make_schema(SIMPLE_SCHEMA);
        let yaml = "name: hello\nversion: 1\n";
        let diags = validate_against_schema(yaml, &compiled);
        assert!(diags.is_empty(), "Expected no diagnostics, got: {diags:?}");
    }

    #[test]
    fn missing_required_field_produces_diagnostic() {
        let compiled = make_schema(SIMPLE_SCHEMA);
        let yaml = "name: hello\n";
        let diags = validate_against_schema(yaml, &compiled);
        assert!(!diags.is_empty(), "Expected diagnostics for missing 'version'");
        let msg = &diags[0].message;
        assert!(
            msg.contains("version") || msg.contains("required"),
            "Expected error about missing 'version', got: {msg}"
        );
    }

    #[test]
    fn wrong_type_produces_diagnostic() {
        let compiled = make_schema(SIMPLE_SCHEMA);
        let yaml = "name: hello\nversion: not_a_number\n";
        let diags = validate_against_schema(yaml, &compiled);
        assert!(!diags.is_empty(), "Expected diagnostics for wrong type");
        let has_type_error = diags.iter().any(|d| {
            d.message.contains("type") || d.message.contains("number") || d.message.contains("version")
        });
        assert!(has_type_error, "Expected type error, got: {diags:?}");
    }

    #[test]
    fn wrong_type_points_to_correct_line() {
        let compiled = make_schema(SIMPLE_SCHEMA);
        // "version" is on line 1 (0-indexed)
        let yaml = "name: hello\nversion: not_a_number\n";
        let diags = validate_against_schema(yaml, &compiled);
        let version_diag = diags
            .iter()
            .find(|d| d.message.contains("version"))
            .expect("Expected a diagnostic about 'version'");
        assert_eq!(
            version_diag.start.line, 1,
            "Expected diagnostic on line 1, got line {}",
            version_diag.start.line
        );
    }

    #[test]
    fn additional_property_produces_diagnostic() {
        let compiled = make_schema(SIMPLE_SCHEMA);
        let yaml = "name: hello\nversion: 1\nunknown_field: oops\n";
        let diags = validate_against_schema(yaml, &compiled);
        assert!(!diags.is_empty(), "Expected diagnostics for additional property");
        let has_additional = diags.iter().any(|d| {
            d.message.contains("additional") || d.message.contains("unknown_field")
        });
        assert!(has_additional, "Expected additional property error, got: {diags:?}");
    }

    #[test]
    fn json_input_works() {
        let compiled = make_schema(SIMPLE_SCHEMA);
        let json = r#"{"name": "hello", "version": 1}"#;
        let diags = validate_against_schema(json, &compiled);
        assert!(diags.is_empty(), "Expected no diagnostics for valid JSON, got: {diags:?}");
    }

    #[test]
    fn json_wrong_type_produces_diagnostic() {
        let compiled = make_schema(SIMPLE_SCHEMA);
        let json = r#"{"name": "hello", "version": "not_a_number"}"#;
        let diags = validate_against_schema(json, &compiled);
        assert!(!diags.is_empty(), "Expected diagnostics for wrong type in JSON");
    }

    #[test]
    fn yaml_parse_error_produces_diagnostic() {
        let compiled = make_schema(SIMPLE_SCHEMA);
        let yaml = "name: [invalid yaml\n";
        let diags = validate_against_schema(yaml, &compiled);
        assert!(!diags.is_empty(), "Expected diagnostic for YAML parse error");
        assert!(
            diags[0].message.contains("parse error") || diags[0].message.contains("YAML"),
            "Expected parse error message, got: {}",
            diags[0].message
        );
    }

    #[test]
    fn json_parse_error_produces_diagnostic() {
        let compiled = make_schema(SIMPLE_SCHEMA);
        let json = r#"{"name": }"#;
        let diags = validate_against_schema(json, &compiled);
        assert!(!diags.is_empty(), "Expected diagnostic for JSON parse error");
        assert!(
            diags[0].message.contains("parse error") || diags[0].message.contains("JSON"),
            "Expected parse error message, got: {}",
            diags[0].message
        );
    }

    #[test]
    fn empty_input_produces_no_diagnostics() {
        let compiled = make_schema(SIMPLE_SCHEMA);
        let diags = validate_against_schema("", &compiled);
        assert!(diags.is_empty());
        let diags = validate_against_schema("   \n  ", &compiled);
        assert!(diags.is_empty());
    }

    #[test]
    fn nested_path_maps_correctly() {
        let schema = r#"{
            "type": "object",
            "properties": {
                "panels": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["type"],
                        "properties": {
                            "type": { "type": "string" },
                            "width": { "type": "number" }
                        }
                    }
                }
            }
        }"#;
        let compiled = make_schema(schema);
        let yaml = "panels:\n  - type: chart\n    width: not_a_number\n";
        let diags = validate_against_schema(yaml, &compiled);
        assert!(!diags.is_empty(), "Expected diagnostic for nested wrong type");
        let width_diag = diags
            .iter()
            .find(|d| d.message.contains("width") || d.message.contains("panels[0]"))
            .expect("Expected a diagnostic about panels[0].width");
        // "width" is on line 2 (0-indexed)
        assert_eq!(
            width_diag.start.line, 2,
            "Expected diagnostic on line 2, got line {}",
            width_diag.start.line
        );
    }

    #[test]
    fn pointer_to_dot_notation_works() {
        assert_eq!(pointer_to_dot_notation("/panels/0/type"), "panels[0].type");
        assert_eq!(pointer_to_dot_notation("/name"), "name");
        assert_eq!(pointer_to_dot_notation(""), "");
        assert_eq!(pointer_to_dot_notation("/a/b/c"), "a.b.c");
        assert_eq!(pointer_to_dot_notation("/items/0"), "items[0]");
    }

    #[test]
    fn yaml_position_map_basic() {
        let yaml = "name: hello\nversion: 1\n";
        let map = build_yaml_position_map(yaml);
        // Root mapping should be recorded
        assert!(map.contains_key(""), "Missing root entry");
        // /name should point to line 0
        let (start, _) = map.get("/name").expect("Missing /name entry");
        assert_eq!(start.line, 0);
        // /version should point to line 1
        let (start, _) = map.get("/version").expect("Missing /version entry");
        assert_eq!(start.line, 1);
    }

    #[test]
    fn yaml_position_map_nested() {
        let yaml = "panels:\n  - type: chart\n    width: 100\n";
        let map = build_yaml_position_map(yaml);
        assert!(map.contains_key("/panels"), "Missing /panels");
        assert!(map.contains_key("/panels/0"), "Missing /panels/0");
        assert!(map.contains_key("/panels/0/type"), "Missing /panels/0/type");
        assert!(map.contains_key("/panels/0/width"), "Missing /panels/0/width");

        let (start, _) = map.get("/panels/0/width").expect("Missing /panels/0/width");
        assert_eq!(start.line, 2);
    }

    #[test]
    fn json_position_map_basic() {
        let json = r#"{"name": "hello", "version": 1}"#;
        let map = build_json_position_map(json);
        assert!(map.contains_key("/name"), "Missing /name, keys: {:?}", map.keys().collect::<Vec<_>>());
        assert!(map.contains_key("/version"), "Missing /version");
    }

    #[test]
    fn source_is_json_schema() {
        let compiled = make_schema(SIMPLE_SCHEMA);
        let yaml = "name: hello\n";
        let diags = validate_against_schema(yaml, &compiled);
        for d in &diags {
            assert_eq!(d.source.as_deref(), Some("json-schema"));
        }
    }
}
