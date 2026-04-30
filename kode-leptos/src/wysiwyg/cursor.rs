//! Cursor positioning and blink management for the tree-based WYSIWYG editor.
//!
//! Functions for finding DOM elements at document positions, measuring character
//! offsets for cursor placement, and vertical cursor movement.

use wasm_bindgen::JsCast;

use kode_doc::Node as DocNode;

use super::click::{get_char_offset_from_point, js_caret_range_from_point};
use super::dom_helpers::{find_ancestor_with_attr, find_ancestor_with_pos_attrs, parse_data_attr};

/// Fraction of line height used as the threshold for determining whether
/// a cursor movement constitutes a visual line change vs horizontal movement.
const LINE_CHANGE_THRESHOLD: f64 = 0.25;

/// Skip over structural boundary tokens (block open/close) to land on the
/// next position inside a textblock. If `forward` is true, move right;
/// otherwise move left. This prevents the cursor from resting at invisible
/// structural boundaries between paragraphs/blocks.
///
/// Atomic blocks (extension-rendered code blocks) are also skipped — the
/// cursor cannot enter them. When moving forward past an atomic block, the
/// cursor lands after the block's closing token (a gap position). When
/// moving backward, it lands before the block's opening token.
///
/// Resolves the position once and computes the target directly instead of
/// stepping one position at a time.
pub(crate) fn next_text_pos(doc: &DocNode, pos: usize, forward: bool) -> usize {
    let max = doc.content.size();
    if forward && pos >= max {
        return max;
    }
    if !forward && pos == 0 {
        return 0;
    }

    let target = if forward { pos + 1 } else { pos - 1 };
    let resolved = doc.resolve(target);

    // Already inside a textblock — check it's not inside an atomic block.
    if resolved.parent().node_type.is_textblock() {
        if !has_atomic_ancestor(&resolved) {
            return target;
        }
        // Inside an atomic block: jump past it.
        return jump_past_atomic_ancestor(doc, &resolved, forward);
    }

    // Not inside a textblock — `target` is at a structural boundary between
    // blocks. Check if this is a valid gap cursor position (next to an atomic
    // block). If so, stop here.
    let before = resolved.node_before();
    let after = resolved.node_after();
    let before_is_atom = before.is_some_and(|n| n.is_atom());
    let after_is_atom = after.is_some_and(|n| n.is_atom());
    if before_is_atom || after_is_atom {
        return target;
    }

    // Not a gap position — find the nearest non-atomic textblock.
    if forward {
        // Look for the next textblock: check node_after at each depth.
        if let Some(after_node) = after {
            return jump_into_first_textblock(after_node, target + 1);
        }
        // No node after at this depth — walk up to find one.
        for d in (0..resolved.depth).rev() {
            let parent_end = resolved.end(d);
            if parent_end + 1 < max {
                let next_resolved = doc.resolve(parent_end + 1);
                if let Some(next_after) = next_resolved.node_after() {
                    if next_after.is_atom() {
                        // Gap position before an atomic block.
                        return parent_end + 1;
                    }
                    return jump_into_first_textblock(next_after, parent_end + 2);
                }
            }
        }
        max
    } else {
        // Look for the previous textblock: check node_before at each depth.
        if let Some(before_node) = before {
            return jump_into_last_textblock(before_node, target);
        }
        // No node before at this depth — walk up.
        for d in (0..resolved.depth).rev() {
            let parent_start = resolved.start(d);
            if parent_start > 0 {
                let prev_resolved = doc.resolve(parent_start - 1);
                if let Some(prev_before) = prev_resolved.node_before() {
                    let before_end = parent_start - 1;
                    if prev_before.is_atom() {
                        // Gap position after an atomic block.
                        return before_end;
                    }
                    return jump_into_last_textblock(prev_before, before_end);
                }
            }
        }
        0
    }
}

/// Check if any ancestor of the resolved position is an atomic node.
fn has_atomic_ancestor(resolved: &kode_doc::ResolvedPos) -> bool {
    for d in (1..=resolved.depth).rev() {
        if resolved.node(d).is_atom() {
            return true;
        }
    }
    false
}

/// When the cursor has landed inside an atomic block, jump past it.
///
/// If `forward`, jump to the gap position after the outermost atomic
/// ancestor. If backward, jump before it. Then recursively resolve to
/// a valid text or gap position.
fn jump_past_atomic_ancestor(
    doc: &DocNode,
    resolved: &kode_doc::ResolvedPos,
    forward: bool,
) -> usize {
    // Find the outermost atomic ancestor.
    let mut atom_depth = resolved.depth;
    for d in 1..=resolved.depth {
        if resolved.node(d).is_atom() {
            atom_depth = d;
            break;
        }
    }

    let gap_pos = if forward {
        resolved.after(atom_depth)
    } else {
        resolved.before(atom_depth)
    };

    next_text_pos_or_gap(doc, gap_pos, forward)
}

/// Move to the next valid cursor position (textblock or gap position next to
/// an atomic block). If `pos` is already valid, return it. Otherwise
/// continue searching in the given direction.
fn next_text_pos_or_gap(doc: &DocNode, pos: usize, forward: bool) -> usize {
    let max = doc.content.size();
    if pos > max {
        return max;
    }
    if pos == 0 && !forward {
        return 0;
    }

    let resolved = doc.resolve(pos);

    // Inside a non-atomic textblock — valid position.
    if resolved.parent().node_type.is_textblock() && !has_atomic_ancestor(&resolved) {
        return pos;
    }

    // At a gap position (between blocks at Doc/container level)?
    if !resolved.parent().node_type.is_textblock() {
        let before = resolved.node_before();
        let after = resolved.node_after();

        // Valid gap position: at least one neighbor is atomic.
        let before_is_atom = before.is_some_and(|n| n.is_atom());
        let after_is_atom = after.is_some_and(|n| n.is_atom());
        if before_is_atom || after_is_atom {
            return pos;
        }

        // Not a gap position — try to enter the adjacent non-atomic textblock.
        if forward {
            if let Some(after_node) = after {
                if !after_node.is_atom() {
                    return jump_into_first_textblock(after_node, pos + 1);
                }
            }
        } else if let Some(before_node) = before {
            if !before_node.is_atom() {
                return jump_into_last_textblock(before_node, pos);
            }
        }
    }

    // Keep searching.
    if forward && pos < max {
        next_text_pos_or_gap(doc, pos + 1, true)
    } else if !forward && pos > 0 {
        next_text_pos_or_gap(doc, pos - 1, false)
    } else if forward {
        max
    } else {
        0
    }
}

/// Jump into a node's first textblock, returning the content start position.
/// `node_start` is the position of the node's opening token.
///
/// Atomic nodes are never entered — the caller must check `is_atom()`
/// before calling this function.
fn jump_into_first_textblock(node: &DocNode, node_start: usize) -> usize {
    if node.node_type.is_textblock() {
        return node_start; // Content starts right after the opening token.
    }
    // Walk into first child recursively.
    let mut current = node;
    let mut pos = node_start;
    loop {
        if current.node_type.is_textblock() {
            return pos;
        }
        match current.first_child() {
            Some(child) => {
                pos += 1; // Skip opening token of current, enter content.
                current = child;
            }
            None => return pos,
        }
    }
}

/// Jump into a node's last textblock, returning the content end position.
/// `after_node` is the position just after the node's closing token.
///
/// Atomic nodes are never entered — the caller must check `is_atom()`
/// before calling this function.
fn jump_into_last_textblock(node: &DocNode, after_node: usize) -> usize {
    if node.node_type.is_textblock() {
        // Content end = after_node - 1 (step back from closing token).
        return after_node - 1;
    }
    let mut current = node;
    let mut pos = after_node;
    loop {
        if current.node_type.is_textblock() {
            return pos - 1; // Content end is before the closing token.
        }
        match current.last_child() {
            Some(child) => {
                pos -= 1; // Step back from closing token of current.
                current = child;
            }
            None => return pos.saturating_sub(1),
        }
    }
}

/// Find the deepest element with `data-pos-start`/`data-pos-end` that contains
/// the given token position.
///
/// Walks the DOM tree top-down, pruning branches whose position range doesn't
/// contain `pos`. This is O(depth) instead of O(total elements).
///
/// When `pos` equals an element's end, prefers the element ending there
/// (end-of-content) over one starting there (start-of-next-block).
pub(crate) fn find_element_for_pos(
    container: &web_sys::Element,
    pos: usize,
) -> Option<web_sys::Element> {
    find_element_for_pos_recursive(container, pos)
}

/// Recursive helper: walk children of `parent`, looking for the deepest
/// positioned element containing `pos`.
fn find_element_for_pos_recursive(
    parent: &web_sys::Element,
    pos: usize,
) -> Option<web_sys::Element> {
    let children = parent.children();
    let len = children.length();

    // Track best match at this level for boundary (pos == end) and strict
    // (pos in [start, end)) cases.
    let mut boundary_best: Option<web_sys::Element> = None;
    let mut boundary_best_size = usize::MAX;
    let mut strict_best: Option<web_sys::Element> = None;
    let mut strict_best_size = usize::MAX;

    for i in 0..len {
        let Some(child) = children.item(i) else { continue };

        let has_pos_attrs = child.has_attribute("data-pos-start")
            && child.has_attribute("data-pos-end");

        if has_pos_attrs {
            let start = parse_data_attr(&child, "data-pos-start").unwrap_or(0);
            let end = parse_data_attr(&child, "data-pos-end").unwrap_or(0);
            let size = end.saturating_sub(start);

            // Empty element: start == end == pos (e.g., empty paragraph)
            if start == end && pos == start {
                // Try to find a deeper match in children first.
                if let Some(deeper) = find_element_for_pos_recursive(&child, pos) {
                    return Some(deeper);
                }
                return Some(child);
            }

            // Position at the end boundary.
            if pos == end && size > 0 && size <= boundary_best_size {
                boundary_best_size = size;
                boundary_best = Some(child.clone());
            }

            // Position strictly inside the range.
            if pos >= start && pos < end {
                // This child contains pos — recurse into it for a deeper match.
                if let Some(deeper) = find_element_for_pos_recursive(&child, pos) {
                    return Some(deeper);
                }
                if size <= strict_best_size {
                    strict_best_size = size;
                    strict_best = Some(child);
                }
            }
        } else {
            // Element without pos attrs might contain positioned children
            // (e.g., wrapper divs). Recurse into it.
            if let Some(found) = find_element_for_pos_recursive(&child, pos) {
                return Some(found);
            }
        }
    }

    // Prefer boundary match (pos at end of element) for cursor-at-end behavior.
    if let Some(b) = boundary_best {
        // Try to find a deeper match within the boundary element.
        if let Some(deeper) = find_element_for_pos_recursive(&b, pos) {
            return Some(deeper);
        }
        return Some(b);
    }

    strict_best
}

/// Find the deepest positioned element at the end of the document.
///
/// Walks the DOM tree top-down, always following the last positioned child,
/// to find the deepest leaf element at the document's end. This is O(depth)
/// instead of the previous O(elements) two-pass scan.
pub(crate) fn find_last_positioned_element(container: &web_sys::Element) -> Option<web_sys::Element> {
    find_last_positioned_recursive(container)
}

fn find_last_positioned_recursive(parent: &web_sys::Element) -> Option<web_sys::Element> {
    let children = parent.children();
    let len = children.length();

    // Walk children in reverse to find the last one with position attrs.
    for i in (0..len).rev() {
        let Some(child) = children.item(i) else { continue };

        if child.has_attribute("data-pos-start") && child.has_attribute("data-pos-end") {
            // This is a positioned element. Try to find a deeper one inside it.
            if let Some(deeper) = find_last_positioned_recursive(&child) {
                return Some(deeper);
            }
            return Some(child);
        }

        // Unpositioned wrapper — check inside it.
        if let Some(found) = find_last_positioned_recursive(&child) {
            return Some(found);
        }
    }
    None
}

/// Find the deepest (most specific) descendant that has `data-pos-start`.
/// For a list item (LI) containing a paragraph span, returns the span.
pub(crate) fn find_deepest_pos_child(el: &web_sys::Element) -> Option<web_sys::Element> {
    // Look for a child element with data-pos-start
    let children = el.query_selector_all("[data-pos-start]").ok()?;
    if children.length() == 0 {
        return None;
    }
    // Return the last match (deepest in DOM order for a single branch)
    let last = children.item(children.length() - 1)?;
    last.dyn_into().ok()
}

/// Use `Range` API to measure the pixel position of a character offset
/// within an element's visible text content.
///
/// `char_offset` is the number of visible characters from the start of the
/// element's text content. This walks text nodes depth-first and creates a
/// collapsed Range at the target position.
pub(crate) fn measure_char_offset_position(
    el: &web_sys::Element,
    char_offset: usize,
) -> Option<(f64, f64)> {
    let document = web_sys::window()?.document()?;

    let mut remaining = char_offset;
    let result = find_text_node_at_char_offset(el, &mut remaining);

    let (text_node, utf16_offset) = match result {
        Some(pair) => pair,
        None => {
            // Past end: use last text node at its end.
            let mut last_node = None;
            let mut last_len = 0usize;
            collect_last_text_node(el.as_ref(), &mut last_node, &mut last_len);
            match last_node {
                Some(node) => (node, last_len),
                None => return None,
            }
        }
    };

    let range = document.create_range().ok()?;
    let _ = range.set_start(&text_node, utf16_offset as u32);
    let _ = range.set_end(&text_node, utf16_offset as u32);

    let rect_list = range.get_client_rects()?;
    if rect_list.length() > 0 {
        let rect = rect_list.item(0)?;
        return Some((rect.left(), rect.top()));
    }

    let rect = range.get_bounding_client_rect();
    if !(rect.width() == 0.0 && rect.height() == 0.0 && rect.left() == 0.0 && rect.top() == 0.0) {
        return Some((rect.left(), rect.top()));
    }

    // Collapsed range at end of text (common after trailing whitespace) can
    // return empty rects in some browsers. Fall back to measuring a 1-char
    // range ending at the offset and using its right edge.
    if utf16_offset > 0 {
        let _ = range.set_start(&text_node, (utf16_offset - 1) as u32);
        let _ = range.set_end(&text_node, utf16_offset as u32);
        let rect = range.get_bounding_client_rect();
        if rect.width() > 0.0 || rect.height() > 0.0 {
            return Some((rect.right(), rect.top()));
        }
    }

    None
}

/// Walk text nodes depth-first to find the node and UTF-16 offset for a given
/// visible character position.
pub(crate) fn find_text_node_at_char_offset(
    el: &web_sys::Element,
    remaining: &mut usize,
) -> Option<(web_sys::Node, usize)> {
    find_text_node_in_subtree(el.as_ref(), remaining)
}

fn find_text_node_in_subtree(
    node: &web_sys::Node,
    remaining: &mut usize,
) -> Option<(web_sys::Node, usize)> {
    if node.node_type() == web_sys::Node::TEXT_NODE {
        let text = node.text_content().unwrap_or_default();
        let char_count = text.chars().count();
        if *remaining <= char_count {
            // Convert char offset to UTF-16 code unit offset (DOM Range API expects UTF-16).
            let utf16_offset: usize = text.chars().take(*remaining).map(|c| c.len_utf16()).sum();
            return Some((node.clone(), utf16_offset));
        }
        *remaining -= char_count;
        return None;
    }

    let children = node.child_nodes();
    for i in 0..children.length() {
        if let Some(child) = children.item(i) {
            if let Some(result) = find_text_node_in_subtree(&child, remaining) {
                return Some(result);
            }
        }
    }
    None
}

/// Collect the last text node in a subtree and its UTF-16 length.
pub(crate) fn collect_last_text_node(
    node: &web_sys::Node,
    last: &mut Option<web_sys::Node>,
    last_utf16_len: &mut usize,
) {
    if node.node_type() == web_sys::Node::TEXT_NODE {
        let text = node.text_content().unwrap_or_default();
        let utf16_len: usize = text.chars().map(|c| c.len_utf16()).sum();
        *last = Some(node.clone());
        *last_utf16_len = utf16_len;
        return;
    }
    let children = node.child_nodes();
    for i in 0..children.length() {
        if let Some(child) = children.item(i) {
            collect_last_text_node(&child, last, last_utf16_len);
        }
    }
}

/// Move the cursor vertically by one visual line. Uses DOM measurement:
/// 1. Find the pixel position of the current cursor (from `head`).
/// 2. Offset y by one line-height up or down.
/// 3. Use `caretRangeFromPoint` to find the character at the new position.
/// 4. Convert back to a tree position.
///
/// If the target lands inside an atomic block (detected via `data-kode-extension`
/// attribute), snaps to the gap position before (when moving up) or after
/// (when moving down) the block.
///
/// Returns `Some(new_pos)` or `None` if movement is not possible.
pub(crate) fn vertical_cursor_move(
    document: &web_sys::Document,
    container_el: &web_sys::Element,
    head: usize,
    forward: bool, // true = down, false = up
) -> Option<usize> {

    // Find the DOM element for the current position.
    let target_el = find_element_for_pos(container_el, head)?;
    let el_start = parse_data_attr(&target_el, "data-pos-start")?;
    let char_offset = head.saturating_sub(el_start);

    // Check if the current position is at an atomic block (gap cursor).
    let at_atomic = is_atomic_element(&target_el);

    // Measure the pixel position of the cursor. For gap positions at atomic
    // blocks, we use the block's bounding rect since there is no text to
    // measure inside.
    let (cursor_x, cursor_y) = if at_atomic {
        let rect = target_el.get_bounding_client_rect();
        // Use the content left edge for x; for y, use top or bottom edge
        // depending on whether cursor is before or after the block.
        let el_end = parse_data_attr(&target_el, "data-pos-end").unwrap_or(el_start);
        let y = if head <= el_start {
            rect.top()
        } else if head >= el_end {
            rect.bottom()
        } else {
            rect.top()
        };
        (rect.left(), y)
    } else {
        measure_char_offset_position(&target_el, char_offset)?
    };

    // Get line-height from the target element.
    let line_height = web_sys::window()
        .and_then(|w| w.get_computed_style(&target_el).ok().flatten())
        .and_then(|cs| cs.get_property_value("line-height").ok())
        .and_then(|lh| lh.trim_end_matches("px").parse::<f64>().ok())
        .unwrap_or(24.0);

    let new_x = cursor_x;

    // Try multiple y offsets to find the target line. Block gaps and
    // margins can cause the first attempt to land in dead space.
    let mut pos_el: Option<web_sys::Element> = None;
    let mut hit_y = 0.0f64;
    let step = if forward { line_height } else { -line_height };
    for multiplier in [1.0, 1.5, 2.0, 2.5, 3.0] {
        let try_y = cursor_y + step * multiplier;
        if try_y < 0.0 { break; }

        let range = js_caret_range_from_point(document, new_x, try_y);
        let found_el = range.and_then(|r| {
            r.start_container().ok().and_then(|node| {
                let el: web_sys::Element = if node.node_type() == web_sys::Node::TEXT_NODE {
                    node.parent_element()?
                } else {
                    node.dyn_into().ok()?
                };
                find_ancestor_with_pos_attrs(&el)
            })
        });

        if let Some(ref el) = found_el {
            // If the target element is an atomic block, snap to a gap position.
            if is_atomic_element(el) {
                let atom_start = parse_data_attr(el, "data-pos-start").unwrap_or(0);
                let atom_end = parse_data_attr(el, "data-pos-end").unwrap_or(atom_start);
                let gap_pos = if forward { atom_end } else { atom_start };
                if gap_pos != head {
                    return Some(gap_pos);
                }
                // Already at this gap — keep probing.
                continue;
            }

            let found_start = parse_data_attr(el, "data-pos-start").unwrap_or(0);
            hit_y = try_y;
            if found_start != el_start {
                // Moved to a different element (different block).
                pos_el = found_el;
                break;
            } else {
                // Same element — could be a multi-line element (code block).
                // Verify the new position is on a truly different visual line,
                // not just a different character on the same line (which would
                // be horizontal movement, not vertical).
                let new_char = get_char_offset_from_point(document, new_x, try_y, el)
                    .unwrap_or(0);
                let el_end = parse_data_attr(el, "data-pos-end").unwrap_or(el_start);
                let new_pos = (el_start + new_char).min(el_end);
                if new_pos != head {
                    // Measure the y position of the candidate to confirm it's
                    // on a different visual line (not just a different x offset).
                    let new_char_offset = new_pos.saturating_sub(el_start);
                    if let Some((_, new_y)) = measure_char_offset_position(el, new_char_offset) {
                        let y_delta = (new_y - cursor_y).abs();
                        if y_delta > line_height * LINE_CHANGE_THRESHOLD {
                            return Some(new_pos);
                        }
                    }
                    // Same visual line — continue probing further offsets.
                }
            }
        }
    }

    if let Some(ref el) = pos_el {
        let new_el_start = parse_data_attr(el, "data-pos-start")?;
        let new_el_end = parse_data_attr(el, "data-pos-end").unwrap_or(new_el_start);
        let new_char = get_char_offset_from_point(document, new_x, hit_y, el)
            .unwrap_or(0);
        let new_tree_pos = (new_el_start + new_char).min(new_el_end);

        if new_tree_pos != head {
            return Some(new_tree_pos);
        }
    }

    // Fallback: if caretRangeFromPoint couldn't bridge the gap (e.g. large
    // heading margins), find the next/previous block element by DOM order.
    let all_pos_els = container_el
        .query_selector_all("[data-pos-start][data-pos-end]")
        .ok()?;

    // Collect all top-level block elements (those whose data-pos-start is
    // unique — i.e. not nested inside another element with the same start).
    // We iterate in DOM order and pick the adjacent block.
    let mut current_idx: Option<u32> = None;
    for i in 0..all_pos_els.length() {
        let Some(node) = all_pos_els.item(i) else { continue };
        let Ok(el) = node.dyn_into::<web_sys::Element>() else { continue };
        let start = parse_data_attr(&el, "data-pos-start").unwrap_or(0);
        let end = parse_data_attr(&el, "data-pos-end").unwrap_or(0);
        if start <= head && head <= end {
            current_idx = Some(i);
            // Don't break — we want the *last* (deepest) match, which
            // matches find_element_for_pos behavior.
        }
    }

    let cur_i = current_idx?;
    let neighbor_idx = if forward {
        // Find next element with a different (greater) data-pos-start.
        let mut found = None;
        for i in (cur_i + 1)..all_pos_els.length() {
            let Some(node) = all_pos_els.item(i) else { continue };
            let Ok(el) = node.dyn_into::<web_sys::Element>() else { continue };
            let s = parse_data_attr(&el, "data-pos-start").unwrap_or(0);
            if s > el_start {
                found = Some(i);
                break;
            }
        }
        found?
    } else {
        // Find previous element with a different (lesser) data-pos-start.
        let mut found = None;
        for i in (0..cur_i).rev() {
            let Some(node) = all_pos_els.item(i) else { continue };
            let Ok(el) = node.dyn_into::<web_sys::Element>() else { continue };
            let s = parse_data_attr(&el, "data-pos-start").unwrap_or(0);
            if s < el_start {
                found = Some(i);
                break;
            }
        }
        found?
    };

    let neighbor_node = all_pos_els.item(neighbor_idx)?;
    let neighbor_el: web_sys::Element = neighbor_node.dyn_into().ok()?;
    let nb_start = parse_data_attr(&neighbor_el, "data-pos-start")?;
    let nb_end = parse_data_attr(&neighbor_el, "data-pos-end").unwrap_or(nb_start);

    // If the neighbor is an atomic block, snap to its gap position.
    if is_atomic_element(&neighbor_el) {
        let gap_pos = if forward { nb_end } else { nb_start };
        return Some(gap_pos);
    }

    // Try to preserve horizontal position using caretRangeFromPoint on the
    // neighbor element's bounding rect.
    let nb_rect = neighbor_el.get_bounding_client_rect();
    let target_y = if forward {
        nb_rect.top() + 4.0 // just inside the top of the next block
    } else {
        nb_rect.bottom() - 4.0 // just inside the bottom of the previous block
    };
    let char_off = get_char_offset_from_point(document, new_x, target_y, &neighbor_el)
        .unwrap_or(0);
    let new_tree_pos = (nb_start + char_off).min(nb_end);
    Some(new_tree_pos)
}

/// Check if a DOM element is an atomic extension block by looking for the
/// `data-kode-extension` attribute on the element or its ancestors.
fn is_atomic_element(el: &web_sys::Element) -> bool {
    el.has_attribute("data-kode-extension")
        || find_ancestor_with_attr(el, "data-kode-extension").is_some()
}
