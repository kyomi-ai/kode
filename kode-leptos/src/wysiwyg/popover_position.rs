//! Popover positioning utility — compute absolute positions for floating UI elements.

use web_sys::DomRect;

/// Computed position for a popover element.
pub struct PopoverPosition {
    /// Absolute top position in pixels (relative to the viewport).
    pub top: f64,
    /// Absolute left position in pixels (relative to the viewport).
    pub left: f64,
    /// Maximum height before scrolling, respecting viewport bounds.
    pub max_height: f64,
    /// Whether the popover flipped to show above the anchor instead of below.
    pub flipped: bool,
}

/// Compute the position for a popover anchored to the given element's bounding rect.
///
/// The popover is placed below the anchor by default, flipping above if
/// insufficient space below. Horizontal position is set to the anchor's left edge.
///
/// `min_space` is the minimum vertical space required to show the popover
/// below the anchor before flipping (e.g. 200.0 for menus, 60.0 for compact popovers).
pub fn compute_position(anchor_rect: &DomRect, min_space: f64) -> Option<PopoverPosition> {
    let window = web_sys::window()?;
    let vh = window.inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(800.0);
    let vw = window.inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(1200.0);
    let gap = 4.0;
    let pad = 8.0;

    let space_below = vh - anchor_rect.bottom() - pad;
    let space_above = anchor_rect.top() - pad;
    let flip = space_below < min_space && space_above > space_below;

    let (top, max_height) = if flip {
        (anchor_rect.top() - gap, (space_above - gap).max(100.0))
    } else {
        (anchor_rect.bottom() + gap, (space_below - gap).max(100.0))
    };

    // Clamp left to stay within viewport
    let left = anchor_rect.left().max(pad).min(vw - 300.0 - pad);

    Some(PopoverPosition {
        top,
        left,
        max_height,
        flipped: flip,
    })
}

/// Compute position relative to a container element instead of viewport.
///
/// Returns position in the container's coordinate space. This is useful
/// for absolutely-positioned elements within a scroll container.
pub fn compute_position_relative(
    anchor_rect: &DomRect,
    container_rect: &DomRect,
    min_space: f64,
) -> Option<PopoverPosition> {
    let window = web_sys::window()?;
    let vh = window.inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(800.0);
    let gap = 4.0;
    let pad = 8.0;

    let space_below = vh - anchor_rect.bottom() - pad;
    let space_above = anchor_rect.top() - pad;
    let flip = space_below < min_space && space_above > space_below;

    let (top, max_height) = if flip {
        let abs_top = anchor_rect.top() - gap;
        (abs_top - container_rect.top(), (space_above - gap).max(100.0))
    } else {
        let abs_top = anchor_rect.bottom() + gap;
        (abs_top - container_rect.top(), (space_below - gap).max(100.0))
    };

    let left = anchor_rect.left() - container_rect.left();

    Some(PopoverPosition {
        top,
        left,
        max_height,
        flipped: flip,
    })
}
