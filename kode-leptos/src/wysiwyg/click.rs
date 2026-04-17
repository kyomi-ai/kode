//! Click position calculation for the tree-based WYSIWYG editor.
//!
//! Converts browser click coordinates to document tree positions using
//! the browser's caret position APIs (`caretRangeFromPoint` for Chrome/Safari,
//! `caretPositionFromPoint` for Firefox).

use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;

/// Get the character offset from a click point within an element, using
/// the browser's caret position APIs.
pub(crate) fn get_char_offset_from_point(
    document: &web_sys::Document,
    x: f64,
    y: f64,
    pos_el: &web_sys::Element,
) -> Option<usize> {
    let container_node: &web_sys::Node = pos_el.as_ref();

    // Try Chrome/Safari: caretRangeFromPoint.
    if let Some(range) = js_caret_range_from_point(document, x, y) {
        let target_node: Option<web_sys::Node> = range.start_container().ok();
        let offset_in_node = range.start_offset().unwrap_or(0) as usize;

        if let Some(target) = target_node {
            let mut count = 0usize;
            if count_chars_before_node(container_node, &target, offset_in_node, &mut count) {
                return Some(count);
            }
        }
    }

    // Try Firefox: caretPositionFromPoint.
    if let Some((offset_node, offset_in_node)) = js_caret_position_from_point(document, x, y) {
        let mut count = 0usize;
        if count_chars_before_node(container_node, &offset_node, offset_in_node, &mut count) {
            return Some(count);
        }
    }

    None
}

/// Call `document.caretRangeFromPoint(x, y)` via JS reflection (Chrome/Safari).
pub(crate) fn js_caret_range_from_point(
    document: &web_sys::Document,
    x: f64,
    y: f64,
) -> Option<web_sys::Range> {
    let func = js_sys::Reflect::get(document, &JsValue::from_str("caretRangeFromPoint")).ok()?;
    let func: js_sys::Function = func.dyn_into().ok()?;
    let result = func
        .call2(document, &JsValue::from_f64(x), &JsValue::from_f64(y))
        .ok()?;
    if result.is_null() || result.is_undefined() {
        return None;
    }
    result.dyn_into().ok()
}

/// Call `document.caretPositionFromPoint(x, y)` via JS reflection (Firefox).
pub(crate) fn js_caret_position_from_point(
    document: &web_sys::Document,
    x: f64,
    y: f64,
) -> Option<(web_sys::Node, usize)> {
    let func = js_sys::Reflect::get(document, &JsValue::from_str("caretPositionFromPoint")).ok()?;
    if func.is_undefined() || func.is_null() {
        return None;
    }
    let func: js_sys::Function = func.dyn_into().ok()?;
    let result = func
        .call2(document, &JsValue::from_f64(x), &JsValue::from_f64(y))
        .ok()?;
    if result.is_null() || result.is_undefined() {
        return None;
    }
    let offset_node_val = js_sys::Reflect::get(&result, &JsValue::from_str("offsetNode")).ok()?;
    if offset_node_val.is_null() || offset_node_val.is_undefined() {
        return None;
    }
    let offset_node: web_sys::Node = offset_node_val.dyn_into().ok()?;
    let offset_val = js_sys::Reflect::get(&result, &JsValue::from_str("offset")).ok()?;
    let offset = offset_val.as_f64()? as usize;
    Some((offset_node, offset))
}

/// Count visible characters in text nodes before (and partially within) the
/// target node. Returns true if we found and counted the target node.
pub(crate) fn count_chars_before_node(
    node: &web_sys::Node,
    target: &web_sys::Node,
    offset_in_target: usize,
    count: &mut usize,
) -> bool {
    if node.is_same_node(Some(target)) {
        if node.node_type() == web_sys::Node::TEXT_NODE {
            let text = node.text_content().unwrap_or_default();
            // offset_in_target is in UTF-16 code units; convert to char count.
            let mut char_count = 0;
            let mut utf16_count = 0;
            for c in text.chars() {
                if utf16_count >= offset_in_target {
                    break;
                }
                utf16_count += c.len_utf16();
                char_count += 1;
            }
            *count += char_count;
        }
        return true;
    }

    if node.node_type() == web_sys::Node::TEXT_NODE {
        let text = node.text_content().unwrap_or_default();
        *count += text.chars().count();
        return false;
    }

    let children = node.child_nodes();
    for i in 0..children.length() {
        if let Some(child) = children.item(i) {
            if count_chars_before_node(&child, target, offset_in_target, count) {
                return true;
            }
        }
    }
    false
}

