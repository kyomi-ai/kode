use std::sync::{Arc, Mutex};

use kode_core::{Editor, Marker, Position, Selection};
use leptos::prelude::*;

/// A handle to programmatically interact with a `CodeEditor` instance.
///
/// Obtained via the `on_ready` callback prop on `CodeEditor`.
#[derive(Clone)]
pub struct EditorHandle {
    editor: Arc<Mutex<Editor>>,
    on_change: Option<Arc<dyn Fn(String) + Send + Sync>>,
    text_version: RwSignal<u64>,
    cursor_version: RwSignal<u64>,
    markers: RwSignal<Vec<Marker>>,
    marker_version: RwSignal<u64>,
}

impl EditorHandle {
    pub fn new(
        editor: Arc<Mutex<Editor>>,
        on_change: Option<Arc<dyn Fn(String) + Send + Sync>>,
        text_version: RwSignal<u64>,
        cursor_version: RwSignal<u64>,
        markers: RwSignal<Vec<Marker>>,
        marker_version: RwSignal<u64>,
    ) -> Self {
        Self {
            editor,
            on_change,
            text_version,
            cursor_version,
            markers,
            marker_version,
        }
    }

    /// Insert text at the current cursor position, replacing any active selection.
    pub fn insert_at_cursor(&self, text: &str) {
        let mut ed = self.editor.lock().unwrap();
        ed.insert(text);
        let new_text = ed.text();
        drop(ed);
        self.text_version.update(|v| *v += 1);
        self.cursor_version.update(|v| *v += 1);
        if let Some(cb) = &self.on_change {
            cb(new_text);
        }
    }

    /// Return the currently selected text, or `None` if there is no selection.
    pub fn selected_text(&self) -> Option<String> {
        let ed = self.editor.lock().unwrap();
        let text = ed.selected_text();
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }

    /// Return the current cursor position (the selection head).
    pub fn cursor(&self) -> Position {
        self.editor.lock().unwrap().cursor()
    }

    /// Return the current selection (anchor and head).
    pub fn selection(&self) -> Selection {
        self.editor.lock().unwrap().selection()
    }

    /// Set error/warning markers on the editor content.
    pub fn set_markers(&self, markers: Vec<Marker>) {
        self.markers.set(markers);
        self.marker_version.update(|v| *v += 1);
    }

    /// Clear all markers from the editor.
    pub fn clear_markers(&self) {
        self.markers.set(Vec::new());
        self.marker_version.update(|v| *v += 1);
    }
}
