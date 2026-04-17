mod completion;
mod diagnostics;
mod editor;
#[cfg(feature = "schema")]
mod schema_diagnostics;
pub mod extension;
mod handle;
mod highlight;
mod keys;
mod markdown_editor_component;
mod theme;
mod toolbar;
pub mod wysiwyg;

pub use completion::{CompletionItemRenderer, CompletionProviderConfig, CompletionProviderFn};
pub use diagnostics::{DiagnosticProvider, tree_sitter_provider};
#[cfg(feature = "schema")]
pub use schema_diagnostics::json_schema_provider;
pub use editor::CodeEditor;
pub use extension::{
    Extension, ExtensionEditorContext, ExtensionKeyboardShortcut, ExtensionToolbarItem,
};
pub use handle::EditorHandle;
pub use highlight::{Language, FenceTracker, language_from_info_string, line_languages};
pub use kode_core::{CompletionContext, CompletionItem, CompletionKind, CompletionTrigger, Diagnostic, DiagnosticSeverity, Marker, MarkerSeverity, Position};
pub use markdown_editor_component::{EditorMode, MarkdownEditorComponent};
pub use theme::{SyntaxTheme, Theme};
pub use kode_markdown::FormattingState;
pub use toolbar::{BuiltinButton, CustomToolbarButton, InjectCommand, ToolbarItem, Toolbar, default_toolbar_items};
pub use wysiwyg::TreeWysiwygEditor;
