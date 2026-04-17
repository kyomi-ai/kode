//! Selection highlight rendering for the tree-based WYSIWYG editor.
//!
//! Renders visual selection highlight divs over the document content
//! for non-collapsed selections.

use kode_doc::Selection;

use super::cursor::measure_char_offset_position;
use super::dom_helpers::parse_data_attr;

use wasm_bindgen::JsCast;

/// Render selection highlight divs for a non-collapsed selection.
pub(crate) fn render_selection_highlights(
    document: &web_sys::Document,
    container: &web_sys::Element,
    overlay_id: &str,
    sel: &Selection,
) {
    let Some(overlay_el) = document.get_element_by_id(overlay_id) else {
        return;
    };

    let sel_from = sel.from();
    let sel_to = sel.to();

    let container_rect = container.get_bounding_client_rect();
    let scroll_top_px = container.scroll_top() as f64;

    let elements = match container.query_selector_all("[data-pos-start][data-pos-end]").ok() {
        Some(list) => list,
        None => return,
    };

    for i in 0..elements.length() {
        let Some(node) = elements.item(i) else { continue };
        let Ok(el) = node.dyn_into::<web_sys::Element>() else { continue };
        let el_start = parse_data_attr(&el, "data-pos-start").unwrap_or(0);
        let el_end = parse_data_attr(&el, "data-pos-end").unwrap_or(0);

        // Skip elements outside selection range.
        if el_end <= sel_from || el_start >= sel_to {
            continue;
        }

        // Skip container elements that have children with data-pos-start.
        // Only draw highlights for leaf textblock elements (p, h1-h6, span
        // with pos attrs that contain text, not UL/OL/LI/BLOCKQUOTE wrappers).
        if el.query_selector("[data-pos-start]").ok().flatten().is_some() {
            continue;
        }

        let el_rect = el.get_bounding_client_rect();
        let el_height = el_rect.height();
        if el_height <= 0.0 {
            continue;
        }

        let fully_selected = sel_from <= el_start && sel_to >= el_end;

        if fully_selected {
            if let Ok(div) = document.create_element("div") {
                let el_left = el_rect.left() - container_rect.left();
                let el_top = el_rect.top() - container_rect.top() + scroll_top_px;
                let _ = div.set_attribute("class", "kode-selection");
                let _ = div.set_attribute("style", &format!(
                    "position:absolute;top:{}px;left:{}px;width:{}px;height:{}px;background:var(--kode-selection);pointer-events:none;",
                    el_top, el_left, el_rect.width(), el_height
                ));
                let _ = overlay_el.append_child(&div);
            }
        } else {
            // Partial selection: compute char offsets within the element.
            let local_start = sel_from.max(el_start).saturating_sub(el_start);
            let local_end = sel_to.min(el_end).saturating_sub(el_start);

            let start_px = measure_char_offset_position(&el, local_start);
            let end_px = measure_char_offset_position(&el, local_end);

            match (start_px, end_px) {
                (Some((sx, sy)), Some((ex, ey))) => {
                    if (sy - ey).abs() < 2.0 {
                        // Same line — use the element's actual height so
                        // the highlight covers the full text for all block
                        // types (headings have larger font than body text).
                        let left = sx - container_rect.left();
                        let top = el_rect.top() - container_rect.top() + scroll_top_px;
                        let width = ex - sx;
                        if let Ok(div) = document.create_element("div") {
                            let _ = div.set_attribute("class", "kode-selection");
                            let _ = div.set_attribute("style", &format!(
                                "position:absolute;top:{}px;left:{}px;width:{}px;height:{}px;background:var(--kode-selection);pointer-events:none;",
                                top, left, width.max(2.0), el_height
                            ));
                            let _ = overlay_el.append_child(&div);
                        }
                    } else {
                        // Multi-line partial selection: draw up to 3 rectangles:
                        // 1. First line: from selection start to right edge
                        // 2. Middle lines: full width
                        // 3. Last line: from left edge to selection end
                        let el_left = el_rect.left() - container_rect.left();

                        // Line height for each row
                        let line_h = (ey - sy).abs() / ((ey - sy).abs() / 20.0).ceil().max(1.0);
                        let line_h = if line_h > 5.0 && line_h < 50.0 { line_h } else { 22.0 };

                        // First line: from start_x to right edge of element
                        let first_top = sy - container_rect.top() + scroll_top_px;
                        let first_left = sx - container_rect.left();
                        let first_width = el_rect.right() - sx;
                        if let Ok(div) = document.create_element("div") {
                            let _ = div.set_attribute("class", "kode-selection");
                            let _ = div.set_attribute("style", &format!(
                                "position:absolute;top:{}px;left:{}px;width:{}px;height:{}px;background:var(--kode-selection);pointer-events:none;",
                                first_top, first_left, first_width.max(2.0), line_h
                            ));
                            let _ = overlay_el.append_child(&div);
                        }

                        // Middle lines: full width between first and last line
                        let middle_top = first_top + line_h;
                        let last_top = ey - container_rect.top() + scroll_top_px;
                        if last_top - middle_top > 1.0 {
                            let mid_height = last_top - middle_top;
                            if let Ok(div) = document.create_element("div") {
                                let _ = div.set_attribute("class", "kode-selection");
                                let _ = div.set_attribute("style", &format!(
                                    "position:absolute;top:{}px;left:{}px;width:{}px;height:{}px;background:var(--kode-selection);pointer-events:none;",
                                    middle_top, el_left, el_rect.width(), mid_height
                                ));
                                let _ = overlay_el.append_child(&div);
                            }
                        }

                        // Last line: from left edge to end_x
                        let last_left = el_left;
                        let last_width = ex - el_rect.left();
                        if let Ok(div) = document.create_element("div") {
                            let _ = div.set_attribute("class", "kode-selection");
                            let _ = div.set_attribute("style", &format!(
                                "position:absolute;top:{}px;left:{}px;width:{}px;height:{}px;background:var(--kode-selection);pointer-events:none;",
                                last_top, last_left, last_width.max(2.0), line_h
                            ));
                            let _ = overlay_el.append_child(&div);
                        }
                    }
                }
                _ => {
                    // Fallback: full element highlight.
                    let el_left = el_rect.left() - container_rect.left();
                    let el_top = el_rect.top() - container_rect.top() + scroll_top_px;
                    if let Ok(div) = document.create_element("div") {
                        let _ = div.set_attribute("class", "kode-selection");
                        let _ = div.set_attribute("style", &format!(
                            "position:absolute;top:{}px;left:{}px;width:{}px;height:{}px;background:var(--kode-selection);pointer-events:none;",
                            el_top, el_left, el_rect.width(), el_height
                        ));
                        let _ = overlay_el.append_child(&div);
                    }
                }
            }
        }
    }
}
