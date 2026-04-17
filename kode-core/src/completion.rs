use crate::selection::Position;

/// The kind of a completion item, used for icon display and sorting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompletionKind {
    #[default]
    Text,
    Keyword,
    Variable,
    Function,
    Field,
    Property,
    Method,
    Module,
    Snippet,
    Other,
}

/// A single completion suggestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionItem {
    pub label: String,
    pub insert_text: Option<String>,
    pub detail: Option<String>,
    pub sort_order: i32,
    pub kind: CompletionKind,
}

impl Default for CompletionItem {
    fn default() -> Self {
        Self {
            label: String::new(),
            insert_text: None,
            detail: None,
            sort_order: 0,
            kind: CompletionKind::Text,
        }
    }
}

/// Context passed to completion providers.
#[derive(Debug, Clone)]
pub struct CompletionContext {
    pub text: String,
    pub cursor: Position,
    pub version: u64,
    pub trigger: CompletionTrigger,
}

/// What triggered the completion request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionTrigger {
    Invoke,
    TriggerCharacter(char),
    Typing,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_item_all_fields() {
        let item = CompletionItem {
            label: "my_func".to_string(),
            insert_text: Some("my_func()".to_string()),
            detail: Some("fn my_func()".to_string()),
            sort_order: 5,
            kind: CompletionKind::Function,
        };
        assert_eq!(item.label, "my_func");
        assert_eq!(item.insert_text.as_deref(), Some("my_func()"));
        assert_eq!(item.detail.as_deref(), Some("fn my_func()"));
        assert_eq!(item.sort_order, 5);
        assert_eq!(item.kind, CompletionKind::Function);
    }

    #[test]
    fn completion_item_default() {
        let item = CompletionItem::default();
        assert_eq!(item.label, "");
        assert!(item.insert_text.is_none());
        assert!(item.detail.is_none());
        assert_eq!(item.sort_order, 0);
        assert_eq!(item.kind, CompletionKind::Text);
    }

    #[test]
    fn completion_context_holds_position_and_version() {
        let ctx = CompletionContext {
            text: "hello world".to_string(),
            cursor: Position::new(0, 5),
            version: 42,
            trigger: CompletionTrigger::Typing,
        };
        assert_eq!(ctx.cursor, Position::new(0, 5));
        assert_eq!(ctx.version, 42);
        assert_eq!(ctx.text, "hello world");
        assert_eq!(ctx.trigger, CompletionTrigger::Typing);
    }

    #[test]
    fn completion_kind_variants_compare_equal() {
        let variants = [
            CompletionKind::Text,
            CompletionKind::Keyword,
            CompletionKind::Variable,
            CompletionKind::Function,
            CompletionKind::Field,
            CompletionKind::Property,
            CompletionKind::Method,
            CompletionKind::Module,
            CompletionKind::Snippet,
            CompletionKind::Other,
        ];
        for (i, a) in variants.iter().enumerate() {
            for (j, b) in variants.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b);
                }
            }
        }
    }
}
