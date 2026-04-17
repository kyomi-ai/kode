//! Attribute storage for nodes and marks.
//!
//! Attributes are key-value pairs stored in a `SmallVec` optimized for the
//! common case of 0–2 attributes per node.

use smallvec::SmallVec;

/// A single attribute value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttrValue {
    /// A string value (e.g. link href, code block language).
    String(String),
    /// An integer value (e.g. heading level, list start number).
    Int(i64),
    /// A boolean value.
    Bool(bool),
}

/// Attribute collection — a list of `(key, value)` pairs.
///
/// Uses `SmallVec` with inline capacity of 2 to avoid heap allocation
/// for the common case of nodes with few attributes.
pub type Attrs = SmallVec<[(String, AttrValue); 2]>;

/// Returns an empty attribute collection.
pub fn empty_attrs() -> Attrs {
    SmallVec::new()
}

/// Returns attributes for a heading node with the given level (1–6).
pub fn heading_attrs(level: u8) -> Attrs {
    let mut attrs = SmallVec::new();
    attrs.push(("level".to_string(), AttrValue::Int(level as i64)));
    attrs
}

/// Returns attributes for a code block node with the given language.
pub fn code_block_attrs(language: &str) -> Attrs {
    let mut attrs = SmallVec::new();
    attrs.push(("language".to_string(), AttrValue::String(language.to_string())));
    attrs
}

/// Returns attributes for a link mark.
pub fn link_attrs(href: &str, title: Option<&str>) -> Attrs {
    let mut attrs = SmallVec::new();
    attrs.push(("href".to_string(), AttrValue::String(href.to_string())));
    if let Some(t) = title {
        attrs.push(("title".to_string(), AttrValue::String(t.to_string())));
    }
    attrs
}

/// Returns attributes for an image node.
pub fn image_attrs(src: &str, alt: &str, title: Option<&str>) -> Attrs {
    let mut attrs = SmallVec::new();
    attrs.push(("src".to_string(), AttrValue::String(src.to_string())));
    attrs.push(("alt".to_string(), AttrValue::String(alt.to_string())));
    if let Some(t) = title {
        attrs.push(("title".to_string(), AttrValue::String(t.to_string())));
    }
    attrs
}

/// Returns attributes for an ordered list node with the given start number.
pub fn ordered_list_attrs(start: i64) -> Attrs {
    let mut attrs = SmallVec::new();
    attrs.push(("start".to_string(), AttrValue::Int(start)));
    attrs
}

/// Looks up an attribute by key, returning a reference to its value if found.
pub fn get_attr<'a>(attrs: &'a Attrs, key: &str) -> Option<&'a AttrValue> {
    attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_attrs_is_empty() {
        let attrs = empty_attrs();
        assert!(attrs.is_empty());
    }

    #[test]
    fn heading_attrs_stores_level() {
        let attrs = heading_attrs(3);
        assert_eq!(get_attr(&attrs, "level"), Some(&AttrValue::Int(3)));
    }

    #[test]
    fn code_block_attrs_stores_language() {
        let attrs = code_block_attrs("rust");
        assert_eq!(
            get_attr(&attrs, "language"),
            Some(&AttrValue::String("rust".to_string()))
        );
    }

    #[test]
    fn link_attrs_without_title() {
        let attrs = link_attrs("https://example.com", None);
        assert_eq!(
            get_attr(&attrs, "href"),
            Some(&AttrValue::String("https://example.com".to_string()))
        );
        assert_eq!(get_attr(&attrs, "title"), None);
    }

    #[test]
    fn link_attrs_with_title() {
        let attrs = link_attrs("https://example.com", Some("Example"));
        assert_eq!(
            get_attr(&attrs, "title"),
            Some(&AttrValue::String("Example".to_string()))
        );
    }

    #[test]
    fn image_attrs_stores_all_fields() {
        let attrs = image_attrs("img.png", "An image", Some("Title"));
        assert_eq!(
            get_attr(&attrs, "src"),
            Some(&AttrValue::String("img.png".to_string()))
        );
        assert_eq!(
            get_attr(&attrs, "alt"),
            Some(&AttrValue::String("An image".to_string()))
        );
        assert_eq!(
            get_attr(&attrs, "title"),
            Some(&AttrValue::String("Title".to_string()))
        );
    }

    #[test]
    fn ordered_list_attrs_stores_start() {
        let attrs = ordered_list_attrs(5);
        assert_eq!(get_attr(&attrs, "start"), Some(&AttrValue::Int(5)));
    }

    #[test]
    fn get_attr_returns_none_for_missing_key() {
        let attrs = heading_attrs(1);
        assert_eq!(get_attr(&attrs, "nonexistent"), None);
    }
}
