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
    Blockquote,
    Link,
    CodeBlock,
    HorizontalRule,
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
            Self::Blockquote => ">",
            Self::Link => "\u{1F517}",
            Self::CodeBlock => "```",
            Self::HorizontalRule => "\u{2015}",
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
            Self::Blockquote => "Blockquote",
            Self::Link => "Insert Link",
            Self::CodeBlock => "Code Block",
            Self::HorizontalRule => "Horizontal Rule",
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
            Self::Blockquote => fmt.blockquote,
            Self::Link | Self::CodeBlock | Self::HorizontalRule => false,
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
        BuiltinButton::Blockquote => ds.toggle_blockquote(),
        BuiltinButton::Link => ds.insert_link("https://"),
        BuiltinButton::CodeBlock => ds.set_block_type(NodeType::CodeBlock, code_block_attrs("")),
        BuiltinButton::HorizontalRule => ds.insert_horizontal_rule(),
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
        ToolbarItem::Builtin(BuiltinButton::Blockquote),
        ToolbarItem::Separator,
        ToolbarItem::Builtin(BuiltinButton::Link),
        ToolbarItem::Builtin(BuiltinButton::CodeBlock),
        ToolbarItem::Builtin(BuiltinButton::HorizontalRule),
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
        10 => MarkdownCommands::insert_link(ed.editor_mut(), "https://"),
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

    view! {
        <div class="kode-toolbar">
            {items}
        </div>
    }
}
