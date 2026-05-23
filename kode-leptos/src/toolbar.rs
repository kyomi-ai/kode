use std::sync::{Arc, Mutex};

use kode_markdown::{FormattingState, MarkdownCommands, MarkdownEditor};
use leptos::prelude::*;
use leptos::tachys::view::any_view::AnyView;

use crate::extension::ExtensionToolbarItem;

// ── Inject commands (for programmatic content insertion) ────────────────────

/// Commands that can be injected into the editor from outside (e.g., modals).
/// Write to the `inject` signal prop on `TreeWysiwygEditor`.
#[derive(Clone, Debug)]
pub enum InjectCommand {
    /// Insert plain text at the current cursor position.
    Text(String),
    /// Insert a link with display text and URL at the current cursor position.
    Link { text: String, url: String },
}

// ── Toolbar configuration types ─────────────────────────────────────────────

/// A single item in the toolbar. Applications compose a `Vec<ToolbarItem>`
/// to control which buttons appear, their order, and custom content.
pub enum ToolbarItem {
    /// A built-in formatting button — kode handles the action and active state.
    Builtin(BuiltinButton),
    /// A built-in formatting button with a custom view label instead of the
    /// default text. kode still handles the action and active state tracking.
    /// Use this to render SVG icons for built-in formatting actions.
    BuiltinWithView(BuiltinButton, AnyView),
    /// A visual separator between button groups.
    Separator,
    /// A flexible spacer that pushes subsequent items to the right.
    Spacer,
    /// A custom button provided by the application.
    Custom(CustomToolbarButton),
    /// An arbitrary Leptos view (e.g., mode toggle, dropdown).
    Slot(AnyView),
    /// An extension-provided button (used internally when appending extension
    /// items to the default toolbar). Applications using custom `toolbar_items`
    /// should include extension buttons via `Custom` instead.
    ExtensionButton(ExtensionToolbarItem),
}

/// Built-in formatting buttons. kode knows how to execute these actions
/// and track their active state via the document's formatting state.
#[derive(Clone, Copy, Debug)]
pub enum BuiltinButton {
    Bold,
    Italic,
    InlineCode,
    Strikethrough,
    H1,
    H2,
    H3,
    BulletList,
    OrderedList,
    TaskList,
    Blockquote,
    Link,
    CodeBlock,
    HorizontalRule,
    Table,
}

impl BuiltinButton {
    /// Default label for this button (used in the default toolbar).
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Bold => "B",
            Self::Italic => "I",
            Self::InlineCode => "`",
            Self::Strikethrough => "S",
            Self::H1 => "H1",
            Self::H2 => "H2",
            Self::H3 => "H3",
            Self::BulletList => "\u{2022}",
            Self::OrderedList => "1.",
            Self::TaskList => "\u{2610}",
            Self::Blockquote => ">",
            Self::Link => "\u{1F517}",
            Self::CodeBlock => "```",
            Self::HorizontalRule => "\u{2015}",
            Self::Table => "\u{25A6}",
        }
    }

    /// Tooltip text for this button.
    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::Bold => "Bold (Ctrl+B)",
            Self::Italic => "Italic (Ctrl+I)",
            Self::InlineCode => "Inline Code",
            Self::Strikethrough => "Strikethrough",
            Self::H1 => "Heading 1",
            Self::H2 => "Heading 2",
            Self::H3 => "Heading 3",
            Self::BulletList => "Bullet List",
            Self::OrderedList => "Ordered List",
            Self::TaskList => "Task List",
            Self::Blockquote => "Blockquote",
            Self::Link => "Insert Link",
            Self::CodeBlock => "Code Block",
            Self::HorizontalRule => "Horizontal Rule",
            Self::Table => "Insert Table",
        }
    }

    /// SVG icon path for this button (Phosphor Icons, MIT license).
    /// Returns `None` for buttons that use text labels.
    pub(crate) fn icon_svg(self) -> Option<&'static str> {
        match self {
            Self::Bold => Some(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" fill="currentColor"><path d="M178.48,115.7A44,44,0,0,0,148,40H80a8,8,0,0,0-8,8V200a8,8,0,0,0,8,8h80a48,48,0,0,0,18.48-92.3ZM88,56h60a28,28,0,0,1,0,56H88Zm72,136H88V128h72a32,32,0,0,1,0,64Z"/></svg>"#),
            Self::Italic => Some(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" fill="currentColor"><path d="M200,56a8,8,0,0,1-8,8H157.77L115.1,192H144a8,8,0,0,1,0,16H64a8,8,0,0,1,0-16H98.23L140.9,64H112a8,8,0,0,1,0-16h80A8,8,0,0,1,200,56Z"/></svg>"#),
            Self::Strikethrough => Some(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" fill="currentColor"><path d="M224,128a8,8,0,0,1-8,8H175.93c9.19,7.11,16.07,17.2,16.07,32,0,13.34-7,25.7-19.75,34.79C160.33,211.31,144.61,216,128,216s-32.33-4.69-44.25-13.21C71,193.7,64,181.34,64,168a8,8,0,0,1,16,0c0,17.35,22,32,48,32s48-14.65,48-32c0-14.85-10.54-23.58-38.77-32H40a8,8,0,0,1,0-16H216A8,8,0,0,1,224,128ZM76.33,104a8,8,0,0,0,7.61-10.49A17.3,17.3,0,0,1,83.11,88c0-18.24,19.3-32,44.89-32,18.84,0,34.16,7.42,41,19.85a8,8,0,0,0,14-7.7C173.33,50.52,152.77,40,128,40,93.29,40,67.11,60.63,67.11,88a33.73,33.73,0,0,0,1.62,10.49A8,8,0,0,0,76.33,104Z"/></svg>"#),
            Self::InlineCode => Some(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" fill="currentColor"><path d="M93.31,70,28,128l65.27,58a8,8,0,1,1-10.62,12l-72-64a8,8,0,0,1,0-12l72-64A8,8,0,1,1,93.31,70Zm152,52-72-64a8,8,0,0,0-10.62,12L228,128l-65.27,58a8,8,0,1,0,10.62,12l72-64a8,8,0,0,0,0-12Z"/></svg>"#),
            Self::BulletList => Some(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" fill="currentColor"><path d="M80,64a8,8,0,0,1,8-8H216a8,8,0,0,1,0,16H88A8,8,0,0,1,80,64Zm136,56H88a8,8,0,0,0,0,16H216a8,8,0,0,0,0-16Zm0,64H88a8,8,0,0,0,0,16H216a8,8,0,0,0,0-16ZM44,52A12,12,0,1,0,56,64,12,12,0,0,0,44,52Zm0,64a12,12,0,1,0,12,12A12,12,0,0,0,44,116Zm0,64a12,12,0,1,0,12,12A12,12,0,0,0,44,180Z"/></svg>"#),
            Self::OrderedList => Some(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" fill="currentColor"><path d="M224,128a8,8,0,0,1-8,8H104a8,8,0,0,1,0-16H216A8,8,0,0,1,224,128ZM104,72H216a8,8,0,0,0,0-16H104a8,8,0,0,0,0,16ZM216,184H104a8,8,0,0,0,0,16H216a8,8,0,0,0,0-16ZM43.58,55.16,48,52.94V104a8,8,0,0,0,16,0V40a8,8,0,0,0-11.58-7.16l-16,8a8,8,0,0,0,7.16,14.32ZM79.77,156.72a23.73,23.73,0,0,0-9.6-15.95,24.86,24.86,0,0,0-34.11,4.7,23.63,23.63,0,0,0-3.57,6.46,8,8,0,1,0,15,5.47,7.84,7.84,0,0,1,1.18-2.13,8.76,8.76,0,0,1,12-1.59A7.91,7.91,0,0,1,63.93,159a7.64,7.64,0,0,1-1.57,5.78,1,1,0,0,0-.08.11L33.59,203.21A8,8,0,0,0,40,216H72a8,8,0,0,0,0-16H56l19.08-25.53A23.47,23.47,0,0,0,79.77,156.72Z"/></svg>"#),
            Self::Blockquote => Some(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" fill="currentColor"><path d="M100,56H40A16,16,0,0,0,24,72v64a16,16,0,0,0,16,16h60v8a32,32,0,0,1-32,32,8,8,0,0,0,0,16,48.05,48.05,0,0,0,48-48V72A16,16,0,0,0,100,56Zm0,80H40V72h60ZM216,56H156a16,16,0,0,0-16,16v64a16,16,0,0,0,16,16h60v8a32,32,0,0,1-32,32,8,8,0,0,0,0,16,48.05,48.05,0,0,0,48-48V72A16,16,0,0,0,216,56Zm0,80H156V72h60Z"/></svg>"#),
            Self::Link => Some(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" fill="currentColor"><path d="M240,88.23a54.43,54.43,0,0,1-16,37L189.25,160a54.27,54.27,0,0,1-38.63,16h-.05A54.63,54.63,0,0,1,96,119.84a8,8,0,0,1,16,.45A38.62,38.62,0,0,0,150.58,160h0a38.39,38.39,0,0,0,27.31-11.31l34.75-34.75a38.63,38.63,0,0,0-54.63-54.63l-11,11A8,8,0,0,1,135.7,59l11-11A54.65,54.65,0,0,1,224,48,54.86,54.86,0,0,1,240,88.23ZM109,185.66l-11,11A38.41,38.41,0,0,1,70.6,208h0a38.63,38.63,0,0,1-27.29-65.94L78,107.31A38.63,38.63,0,0,1,144,135.71a8,8,0,0,0,7.78,8.22H152a8,8,0,0,0,8-7.78A54.86,54.86,0,0,0,144,96a54.65,54.65,0,0,0-77.27,0L32,130.75A54.62,54.62,0,0,0,70.56,224h0a54.28,54.28,0,0,0,38.64-16l11-11A8,8,0,0,0,109,185.66Z"/></svg>"#),
            Self::CodeBlock => Some(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" fill="currentColor"><path d="M69.12,94.15,28.5,128l40.62,33.85a8,8,0,1,1-10.24,12.29l-48-40a8,8,0,0,1,0-12.29l48-40a8,8,0,0,1,10.24,12.3Zm176,27.7-48-40a8,8,0,1,0-10.24,12.3L227.5,128l-40.62,33.85a8,8,0,1,0,10.24,12.29l48-40a8,8,0,0,0,0-12.29ZM162.73,32.48a8,8,0,0,0-10.25,4.79l-64,176a8,8,0,0,0,4.79,10.26A8.14,8.14,0,0,0,96,224a8,8,0,0,0,7.52-5.27l64-176A8,8,0,0,0,162.73,32.48Z"/></svg>"#),
            Self::HorizontalRule => Some(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" fill="currentColor"><path d="M224,128a8,8,0,0,1-8,8H40a8,8,0,0,1,0-16H216A8,8,0,0,1,224,128Z"/></svg>"#),
            Self::TaskList => Some(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" fill="currentColor"><path d="M173.66,98.34a8,8,0,0,1,0,11.32l-56,56a8,8,0,0,1-11.32,0l-24-24a8,8,0,0,1,11.32-11.32L112,148.69l50.34-50.35A8,8,0,0,1,173.66,98.34ZM224,48V208a16,16,0,0,1-16,16H48a16,16,0,0,1-16-16V48A16,16,0,0,1,48,32H208A16,16,0,0,1,224,48Zm-16,0H48V208H208Z"/></svg>"#),
            Self::Table => Some(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" fill="currentColor"><path d="M224,48H32a8,8,0,0,0-8,8V192a16,16,0,0,0,16,16H216a16,16,0,0,0,16-16V56A8,8,0,0,0,224,48ZM40,112H80v32H40Zm56,0H216v32H96ZM216,64V96H40V64ZM40,160H80v32H40Zm176,32H96V160H216v32Z"/></svg>"#),
            _ => None,
        }
    }

    /// Check if this button is active given the current formatting state.
    /// Accepts `kode_doc::FormattingState` (used by `TreeWysiwygEditor`).
    pub(crate) fn is_active(self, fmt: &kode_doc::FormattingState) -> bool {
        match self {
            Self::Bold => fmt.bold,
            Self::Italic => fmt.italic,
            Self::InlineCode => fmt.code,
            Self::Strikethrough => fmt.strikethrough,
            Self::H1 => fmt.heading_level == 1,
            Self::H2 => fmt.heading_level == 2,
            Self::H3 => fmt.heading_level == 3,
            Self::BulletList => fmt.bullet_list,
            Self::OrderedList => fmt.ordered_list,
            Self::TaskList => fmt.task_list,
            Self::Blockquote => fmt.blockquote,
            Self::Link | Self::CodeBlock | Self::HorizontalRule | Self::Table => false,
        }
    }
}

/// A custom toolbar button provided by the consuming application.
pub struct CustomToolbarButton {
    /// Button content — text, icon, or any Leptos view.
    pub label: AnyView,
    /// Tooltip text.
    pub title: String,
    /// Click handler. For application-level actions (open modal, toggle mode).
    pub on_click: Arc<dyn Fn() + Send + Sync>,
    /// CSS class override. When set, this class is used instead of
    /// `kode-toolbar-button`. Useful for buttons that should look different
    /// (e.g., "Add Chart" with a secondary background).
    pub class: Option<String>,
}

/// Execute the formatting action for a built-in button on a `DocState`.
///
/// This is used by `TreeWysiwygEditor` which operates on `DocState` directly
/// (not `MarkdownEditor`). Each button maps to a `DocState` method.
pub(crate) fn dispatch_builtin_action(ds: &mut kode_doc::DocState, btn: BuiltinButton) {
    use kode_doc::attrs::{code_block_attrs, empty_attrs, heading_attrs};
    use kode_doc::mark::MarkType;
    use kode_doc::node_type::NodeType;

    match btn {
        BuiltinButton::Bold => ds.toggle_mark(MarkType::Strong),
        BuiltinButton::Italic => ds.toggle_mark(MarkType::Em),
        BuiltinButton::InlineCode => ds.toggle_mark(MarkType::Code),
        BuiltinButton::Strikethrough => ds.toggle_mark(MarkType::Strike),
        BuiltinButton::H1 => {
            if ds.formatting_at_cursor().heading_level == 1 {
                ds.set_block_type(NodeType::Paragraph, empty_attrs());
            } else {
                ds.set_block_type(NodeType::Heading, heading_attrs(1));
            }
        }
        BuiltinButton::H2 => {
            if ds.formatting_at_cursor().heading_level == 2 {
                ds.set_block_type(NodeType::Paragraph, empty_attrs());
            } else {
                ds.set_block_type(NodeType::Heading, heading_attrs(2));
            }
        }
        BuiltinButton::H3 => {
            if ds.formatting_at_cursor().heading_level == 3 {
                ds.set_block_type(NodeType::Paragraph, empty_attrs());
            } else {
                ds.set_block_type(NodeType::Heading, heading_attrs(3));
            }
        }
        BuiltinButton::BulletList => ds.toggle_bullet_list(),
        BuiltinButton::OrderedList => ds.toggle_ordered_list(),
        BuiltinButton::TaskList => ds.toggle_task_list(),
        BuiltinButton::Blockquote => ds.toggle_blockquote(),
        BuiltinButton::Link => ds.insert_link("https://"),
        BuiltinButton::CodeBlock => ds.set_block_type(NodeType::CodeBlock, code_block_attrs("")),
        BuiltinButton::HorizontalRule => ds.insert_horizontal_rule(),
        // Reached via slash menu (toolbar button intercepts to show picker instead).
        BuiltinButton::Table => ds.insert_table(3, 2),
    }
}

/// The default toolbar layout — used when no custom `toolbar_items` is provided.
pub fn default_toolbar_items() -> Vec<ToolbarItem> {
    vec![
        ToolbarItem::Builtin(BuiltinButton::Bold),
        ToolbarItem::Builtin(BuiltinButton::Italic),
        ToolbarItem::Builtin(BuiltinButton::InlineCode),
        ToolbarItem::Builtin(BuiltinButton::Strikethrough),
        ToolbarItem::Separator,
        ToolbarItem::Builtin(BuiltinButton::H1),
        ToolbarItem::Builtin(BuiltinButton::H2),
        ToolbarItem::Builtin(BuiltinButton::H3),
        ToolbarItem::Separator,
        ToolbarItem::Builtin(BuiltinButton::BulletList),
        ToolbarItem::Builtin(BuiltinButton::OrderedList),
        ToolbarItem::Builtin(BuiltinButton::TaskList),
        ToolbarItem::Builtin(BuiltinButton::Blockquote),
        ToolbarItem::Separator,
        ToolbarItem::Builtin(BuiltinButton::Link),
        ToolbarItem::Builtin(BuiltinButton::CodeBlock),
        ToolbarItem::Builtin(BuiltinButton::HorizontalRule),
        ToolbarItem::Builtin(BuiltinButton::Table),
    ]
}

// ── Slash menu ─────────────────────────────────────────────────────────────

pub enum SlashMenuItem {
    Builtin { button: BuiltinButton, label: &'static str, description: &'static str },
    Extension { label: String, description: String, ext_index: usize, icon_svg: Option<String> },
}

pub fn default_slash_menu_items() -> Vec<SlashMenuItem> {
    vec![
        SlashMenuItem::Builtin { button: BuiltinButton::H1, label: "Heading 1", description: "Large section heading" },
        SlashMenuItem::Builtin { button: BuiltinButton::H2, label: "Heading 2", description: "Medium section heading" },
        SlashMenuItem::Builtin { button: BuiltinButton::H3, label: "Heading 3", description: "Small section heading" },
        SlashMenuItem::Builtin { button: BuiltinButton::BulletList, label: "Bullet List", description: "Unordered list" },
        SlashMenuItem::Builtin { button: BuiltinButton::OrderedList, label: "Ordered List", description: "Numbered list" },
        SlashMenuItem::Builtin { button: BuiltinButton::TaskList, label: "Task List", description: "Checklist with checkboxes" },
        SlashMenuItem::Builtin { button: BuiltinButton::Blockquote, label: "Blockquote", description: "Quote block" },
        SlashMenuItem::Builtin { button: BuiltinButton::CodeBlock, label: "Code Block", description: "Fenced code block" },
        SlashMenuItem::Builtin { button: BuiltinButton::HorizontalRule, label: "Horizontal Rule", description: "Divider line" },
        SlashMenuItem::Builtin { button: BuiltinButton::Table, label: "Table", description: "Insert a table" },
    ]
}

// ── Helpers ─────────────────────────────────────────────────────────────────
// Used by the `Toolbar` component (for MarkdownEditorComponent).

fn button_class(active: bool) -> &'static str {
    if active {
        "kode-toolbar-button active"
    } else {
        "kode-toolbar-button"
    }
}

// ── Action dispatch ─────────────────────────────────────────────────────────

/// Map button index to the corresponding `MarkdownCommands` call.
fn dispatch_action(ed: &mut MarkdownEditor, idx: usize) {
    match idx {
        0 => MarkdownCommands::toggle_bold(ed.editor_mut()),
        1 => MarkdownCommands::toggle_italic(ed.editor_mut()),
        2 => MarkdownCommands::toggle_inline_code(ed.editor_mut()),
        3 => MarkdownCommands::toggle_strikethrough(ed.editor_mut()),
        4 => MarkdownCommands::set_heading(ed.editor_mut(), 1),
        5 => MarkdownCommands::set_heading(ed.editor_mut(), 2),
        6 => MarkdownCommands::set_heading(ed.editor_mut(), 3),
        7 => MarkdownCommands::toggle_bullet_list(ed.editor_mut()),
        8 => MarkdownCommands::toggle_ordered_list(ed.editor_mut()),
        9 => MarkdownCommands::toggle_blockquote(ed.editor_mut()),
        10 => return,
        11 => MarkdownCommands::insert_code_block(ed.editor_mut(), ""),
        12 => MarkdownCommands::insert_horizontal_rule(ed.editor_mut()),
        _ => return,
    }
    ed.sync_tree();
}

// ── Individual button component ─────────────────────────────────────────────

#[component]
fn ToolbarButton(
    idx: usize,
    label: &'static str,
    title: &'static str,
    #[prop(into)] active: Signal<bool>,
    editor: Arc<Mutex<MarkdownEditor>>,
    on_action: Arc<dyn Fn() + Send + Sync + 'static>,
) -> impl IntoView {
    let on_click = move |_: web_sys::MouseEvent| {
        {
            let mut ed = editor.lock().unwrap();
            dispatch_action(&mut ed, idx);
        } // lock dropped before on_action
        on_action();
    };

    view! {
        <button
            title={title}
            class=move || button_class(active.get())
            on:click={on_click}
            on:mousedown={move |ev: web_sys::MouseEvent| { ev.prevent_default(); }}
        >
            {label}
        </button>
    }
}

// ── Button definitions ──────────────────────────────────────────────────────

struct ButtonDef {
    label: &'static str,
    title: &'static str,
    group: u8,
}

const BUTTONS: &[ButtonDef] = &[
    ButtonDef { label: "B",   title: "Bold (Ctrl+B)",    group: 0 },
    ButtonDef { label: "I",   title: "Italic (Ctrl+I)",  group: 0 },
    ButtonDef { label: "`",   title: "Inline Code",      group: 0 },
    ButtonDef { label: "S",   title: "Strikethrough",    group: 0 },
    ButtonDef { label: "H1",  title: "Heading 1",        group: 1 },
    ButtonDef { label: "H2",  title: "Heading 2",        group: 1 },
    ButtonDef { label: "H3",  title: "Heading 3",        group: 1 },
    ButtonDef { label: "\u{2022}",  title: "Bullet List",       group: 2 },
    ButtonDef { label: "1.",  title: "Ordered List",     group: 2 },
    ButtonDef { label: ">",   title: "Blockquote",       group: 2 },
    ButtonDef { label: "\u{1F517}", title: "Insert Link",       group: 3 },
    ButtonDef { label: "```", title: "Code Block",       group: 3 },
    ButtonDef { label: "\u{2015}",  title: "Horizontal Rule",   group: 3 },
];

// ── Main Toolbar component ──────────────────────────────────────────────────

/// A composable markdown formatting toolbar.
///
/// Each button applies the corresponding `MarkdownCommands` method to the
/// editor, syncs the tree, then fires `on_action` so the parent can
/// trigger a re-render.
/// Map a button index to whether it's active given the current formatting state.
fn is_button_active(fmt: &FormattingState, idx: usize) -> bool {
    match idx {
        0 => fmt.bold,
        1 => fmt.italic,
        2 => fmt.code,
        3 => fmt.strikethrough,
        4 => fmt.heading_level == 1,
        5 => fmt.heading_level == 2,
        6 => fmt.heading_level == 3,
        7 => fmt.bullet_list,
        8 => fmt.ordered_list,
        9 => fmt.blockquote,
        _ => false, // Link, Code Block, HR are insert-only actions
    }
}

#[component]
pub fn Toolbar(
    editor: Arc<Mutex<MarkdownEditor>>,
    on_action: Arc<dyn Fn() + Send + Sync + 'static>,
    #[prop(into, default = Signal::stored(FormattingState::default()))]
    formatting: Signal<FormattingState>,
    /// Extension toolbar items appended after built-in buttons.
    #[prop(default = vec![])]
    extension_items: Vec<ExtensionToolbarItem>,
    /// Extension active state — a reactive signal of `(name, is_active)` pairs
    /// collected from all extensions. Used to highlight extension toolbar buttons.
    #[prop(into, default = Signal::stored(Vec::<(String, bool)>::new()))]
    extension_active_state: Signal<Vec<(String, bool)>>,
) -> impl IntoView {
    // ── Link popup state ────────────────────────────────────────────
    let link_popup_open = RwSignal::new(false);
    let link_popup_url = RwSignal::new(String::new());
    let link_input_ref = NodeRef::<leptos::html::Input>::new();

    // We build the children as a collected Vec of fragments.
    // Each item is either a separator or a button.
    let items: Vec<AnyView> = {
        let mut out = Vec::new();
        let mut prev_group: Option<u8> = None;

        // ── Built-in buttons ─────────────────────────────────────────
        for (idx, def) in BUTTONS.iter().enumerate() {
            if let Some(pg) = prev_group {
                if pg != def.group {
                    out.push(
                        view! { <span class="kode-toolbar-separator"></span> }.into_any()
                    );
                }
            }
            prev_group = Some(def.group);

            // Link button (idx=10) opens the popup instead of inserting directly
            if idx == 10 {
                let link_popup_tb = link_popup_open;
                out.push(view! {
                    <button
                        title={def.title}
                        class="kode-toolbar-button"
                        on:click=move |_: web_sys::MouseEvent| { link_popup_tb.set(true); }
                        on:mousedown=move |ev: web_sys::MouseEvent| { ev.prevent_default(); }
                    >
                        {def.label}
                    </button>
                }.into_any());
                continue;
            }

            let ed = Arc::clone(&editor);
            let cb = Arc::clone(&on_action);
            let active = Signal::derive(move || {
                is_button_active(&formatting.get(), idx)
            });
            out.push(
                view! {
                    <ToolbarButton
                        idx={idx}
                        label={def.label}
                        title={def.title}
                        active={active}
                        editor={ed}
                        on_action={cb}
                    />
                }
                .into_any(),
            );
        }

        // ── Extension buttons ────────────────────────────────────────
        for item in extension_items {
            if let Some(pg) = prev_group {
                if pg != item.group {
                    out.push(
                        view! { <span class="kode-toolbar-separator"></span> }.into_any()
                    );
                }
            }
            prev_group = Some(item.group);

            let ed = Arc::clone(&editor);
            let cb = Arc::clone(&on_action);
            let action = item.action;
            let label = item.label;
            let title = item.title;
            let active_name = item.active_name;

            let active = Signal::derive(move || {
                if let Some(ref name) = active_name {
                    let states = extension_active_state.get();
                    states.iter().any(|(n, a)| n == name && *a)
                } else {
                    false
                }
            });

            let on_click = move |_: web_sys::MouseEvent| {
                {
                    let mut ed = ed.lock().unwrap();
                    (action)(&mut ed);
                    ed.sync_tree();
                }
                cb();
            };

            out.push(
                view! {
                    <button
                        title={title}
                        class=move || button_class(active.get())
                        on:click={on_click}
                        on:mousedown={move |ev: web_sys::MouseEvent| { ev.prevent_default(); }}
                    >
                        {label}
                    </button>
                }
                .into_any(),
            );
        }

        out
    };

    // ── Link popup ──────────────────────────────────────────────────
    let ed_key = Arc::clone(&editor);
    let cb_key = Arc::clone(&on_action);
    let ed_apply = Arc::clone(&editor);
    let cb_apply = Arc::clone(&on_action);

    Effect::new(move || {
        if link_popup_open.get() {
            if let Some(input) = link_input_ref.get() {
                let _ = input.focus();
            }
        }
    });

    view! {
        <div class="kode-toolbar">
            {items}
        </div>
        <div class="kode-link-popup"
            style=move || if link_popup_open.get() { "display:flex;" } else { "display:none;" }
            on:mousedown=|ev: web_sys::MouseEvent| { ev.prevent_default(); }>
            <input
                node_ref=link_input_ref
                type="text"
                class="kode-link-popup-input"
                placeholder="https://example.com"
                prop:value=move || link_popup_url.get()
                on:input=move |ev| {
                    link_popup_url.set(event_target_value(&ev));
                }
                on:keydown=move |ev: web_sys::KeyboardEvent| {
                    if ev.key() == "Enter" {
                        ev.prevent_default();
                        let url = link_popup_url.get_untracked();
                        if !url.trim().is_empty() && url.trim() != "https://" {
                            let Ok(mut ed) = ed_key.lock() else {
                                link_popup_open.set(false);
                                link_popup_url.set(String::new());
                                return;
                            };
                            MarkdownCommands::insert_link(ed.editor_mut(), &url);
                            ed.sync_tree();
                            drop(ed);
                            cb_key();
                        }
                        link_popup_open.set(false);
                        link_popup_url.set(String::new());
                    } else if ev.key() == "Escape" {
                        ev.prevent_default();
                        link_popup_open.set(false);
                        link_popup_url.set(String::new());
                    }
                }
            />
            <button class="kode-link-popup-apply"
                on:click=move |_: web_sys::MouseEvent| {
                    let url = link_popup_url.get_untracked();
                    if !url.trim().is_empty() && url.trim() != "https://" {
                        let Ok(mut ed) = ed_apply.lock() else {
                            link_popup_open.set(false);
                            link_popup_url.set(String::new());
                            return;
                        };
                        MarkdownCommands::insert_link(ed.editor_mut(), &url);
                        ed.sync_tree();
                        drop(ed);
                        cb_apply();
                    }
                    link_popup_open.set(false);
                    link_popup_url.set(String::new());
                }>
                "Apply"
            </button>
            <button class="kode-link-popup-cancel"
                on:click=move |_: web_sys::MouseEvent| {
                    link_popup_open.set(false);
                    link_popup_url.set(String::new());
                }>
                "Cancel"
            </button>
        </div>
    }
}
