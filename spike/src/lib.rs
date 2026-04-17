use std::cell::RefCell;
use wasm_bindgen::prelude::*;

thread_local! {
    static HIGHLIGHTER: RefCell<arborium::Highlighter> = RefCell::new(arborium::Highlighter::new());
}

#[wasm_bindgen]
pub fn test_editor_core() -> String {
    let mut executor = editor_core::CommandExecutor::empty(80);

    executor
        .execute(editor_core::Command::Edit(
            editor_core::EditCommand::Insert {
                offset: 0,
                text: "SELECT * FROM users\nWHERE id = 1;\n".to_string(),
            },
        ))
        .unwrap();

    executor
        .execute(editor_core::Command::Cursor(
            editor_core::CursorCommand::MoveTo {
                line: 0,
                column: 7,
            },
        ))
        .unwrap();

    let pos = executor.editor().cursor_position();
    format!("cursor at line={}, col={}", pos.line, pos.column)
}

#[wasm_bindgen]
pub fn highlight(code: &str, language: &str) -> Result<String, JsValue> {
    HIGHLIGHTER.with(|h| {
        let mut highlighter = h.borrow_mut();
        let result = highlighter
            .highlight(language, code)
            .map_err(|e| JsValue::from_str(&format!("Error: {:?}", e)))?;
        Ok(result.to_string())
    })
}
