//! Marks — metadata applied to text nodes.
//!
//! Marks represent inline formatting (bold, italic, code, etc.) applied to text
//! spans. Unlike wrapper nodes, marks are flat metadata attached to text nodes,
//! allowing multiple marks to coexist on the same text.
//!
//! Mark sets are maintained in sorted order by [`MarkType`] for determinism.
//! The [`Code`](MarkType::Code) mark type excludes all other marks — a code
//! span cannot contain bold, italic, or other formatting.

use crate::attrs::{empty_attrs, Attrs};

/// The type of a mark (inline formatting).
///
/// Variants are ordered by their sort rank for deterministic mark set ordering.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MarkType {
    /// Bold text.
    Strong,
    /// Italic text.
    Em,
    /// Inline code. Excludes all other marks.
    Code,
    /// Hyperlink (attrs: href, title).
    Link,
    /// Strikethrough text.
    Strike,
}

impl MarkType {
    /// Returns `true` if this mark type excludes the `other` mark type.
    ///
    /// Code marks exclude all other marks. All other marks coexist.
    pub fn excludes(self, other: MarkType) -> bool {
        self == MarkType::Code || other == MarkType::Code
    }
}

/// A mark instance — a mark type with optional attributes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark {
    /// The type of this mark.
    pub mark_type: MarkType,
    /// Attributes for this mark (e.g. href for links).
    pub attrs: Attrs,
}

impl Mark {
    /// Creates a new mark with the given type and no attributes.
    pub fn new(mark_type: MarkType) -> Self {
        Mark {
            mark_type,
            attrs: empty_attrs(),
        }
    }

    /// Creates a new mark with the given type and attributes.
    pub fn with_attrs(mark_type: MarkType, attrs: Attrs) -> Self {
        Mark { mark_type, attrs }
    }

    /// Adds this mark to a sorted mark set, returning a new set.
    ///
    /// If a mark of the same type already exists, it is replaced. If this mark
    /// excludes other marks (or vice versa), those marks are removed.
    /// The returned set maintains sorted order by [`MarkType`].
    pub fn add_to_set(&self, marks: &[Mark]) -> Vec<Mark> {
        // If self is NOT Code but the set already contains Code, reject self —
        // return the set unchanged. Code excludes all other marks.
        if self.mark_type != MarkType::Code {
            for existing in marks {
                if existing.mark_type == MarkType::Code {
                    return marks.to_vec();
                }
            }
        }

        let mut result: Vec<Mark> = Vec::with_capacity(marks.len() + 1);
        let mut inserted = false;

        for existing in marks {
            // If self is Code, skip all non-Code marks (Code excludes everything).
            if self.mark_type == MarkType::Code && existing.mark_type != MarkType::Code {
                continue;
            }

            // If existing mark is the same type, replace it.
            if existing.mark_type == self.mark_type {
                result.push(self.clone());
                inserted = true;
                continue;
            }

            // If we haven't inserted yet and self should come before existing
            // (by sort order), insert now.
            if !inserted && self.mark_type < existing.mark_type {
                result.push(self.clone());
                inserted = true;
            }

            result.push(existing.clone());
        }

        if !inserted {
            result.push(self.clone());
        }

        result
    }

    /// Removes marks of this mark's type from a set, returning a new set.
    pub fn remove_from_set(&self, marks: &[Mark]) -> Vec<Mark> {
        marks
            .iter()
            .filter(|m| m.mark_type != self.mark_type)
            .cloned()
            .collect()
    }

    /// Returns `true` if two mark sets are equal.
    ///
    /// Two mark sets are equal if they have the same length and each mark
    /// at the same position is equal.
    pub fn same_set(a: &[Mark], b: &[Mark]) -> bool {
        a.len() == b.len() && a.iter().zip(b.iter()).all(|(ma, mb)| ma == mb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attrs::link_attrs;

    #[test]
    fn new_mark_has_empty_attrs() {
        let m = Mark::new(MarkType::Strong);
        assert_eq!(m.mark_type, MarkType::Strong);
        assert!(m.attrs.is_empty());
    }

    #[test]
    fn with_attrs_stores_attrs() {
        let attrs = link_attrs("https://example.com", None);
        let m = Mark::with_attrs(MarkType::Link, attrs.clone());
        assert_eq!(m.attrs, attrs);
    }

    #[test]
    fn add_to_empty_set() {
        let m = Mark::new(MarkType::Strong);
        let set = m.add_to_set(&[]);
        assert_eq!(set.len(), 1);
        assert_eq!(set[0].mark_type, MarkType::Strong);
    }

    #[test]
    fn add_maintains_sort_order() {
        let strong = Mark::new(MarkType::Strong);
        let strike = Mark::new(MarkType::Strike);
        let em = Mark::new(MarkType::Em);

        // Add in reverse order; result should be sorted.
        let set = strike.add_to_set(&[]);
        let set = em.add_to_set(&set);
        let set = strong.add_to_set(&set);

        assert_eq!(set.len(), 3);
        assert_eq!(set[0].mark_type, MarkType::Strong);
        assert_eq!(set[1].mark_type, MarkType::Em);
        assert_eq!(set[2].mark_type, MarkType::Strike);
    }

    #[test]
    fn add_replaces_same_type() {
        let link1 = Mark::with_attrs(MarkType::Link, link_attrs("https://a.com", None));
        let link2 = Mark::with_attrs(MarkType::Link, link_attrs("https://b.com", None));

        let set = link1.add_to_set(&[]);
        let set = link2.add_to_set(&set);

        assert_eq!(set.len(), 1);
        assert_eq!(set[0].mark_type, MarkType::Link);
        assert_eq!(set[0].attrs, link_attrs("https://b.com", None));
    }

    #[test]
    fn code_excludes_all_other_marks() {
        let strong = Mark::new(MarkType::Strong);
        let em = Mark::new(MarkType::Em);
        let code = Mark::new(MarkType::Code);

        let set = strong.add_to_set(&[]);
        let set = em.add_to_set(&set);
        assert_eq!(set.len(), 2);

        // Adding code should remove strong and em.
        let set = code.add_to_set(&set);
        assert_eq!(set.len(), 1);
        assert_eq!(set[0].mark_type, MarkType::Code);
    }

    #[test]
    fn adding_mark_to_set_with_code_is_rejected() {
        let code = Mark::new(MarkType::Code);
        let strong = Mark::new(MarkType::Strong);

        let set = code.add_to_set(&[]);
        // Adding strong to a set that has code: code excludes strong,
        // so strong is rejected and the set is returned unchanged.
        let set = strong.add_to_set(&set);

        assert_eq!(set.len(), 1);
        assert_eq!(set[0].mark_type, MarkType::Code);
    }

    #[test]
    fn remove_from_set() {
        let strong = Mark::new(MarkType::Strong);
        let em = Mark::new(MarkType::Em);

        let set = strong.add_to_set(&[]);
        let set = em.add_to_set(&set);
        assert_eq!(set.len(), 2);

        let set = strong.remove_from_set(&set);
        assert_eq!(set.len(), 1);
        assert_eq!(set[0].mark_type, MarkType::Em);
    }

    #[test]
    fn remove_nonexistent_mark_is_noop() {
        let strong = Mark::new(MarkType::Strong);
        let em = Mark::new(MarkType::Em);

        let set = strong.add_to_set(&[]);
        let result = em.remove_from_set(&set);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].mark_type, MarkType::Strong);
    }

    #[test]
    fn same_set_equal() {
        let a = vec![Mark::new(MarkType::Strong), Mark::new(MarkType::Em)];
        let b = vec![Mark::new(MarkType::Strong), Mark::new(MarkType::Em)];
        assert!(Mark::same_set(&a, &b));
    }

    #[test]
    fn same_set_different_length() {
        let a = vec![Mark::new(MarkType::Strong)];
        let b = vec![Mark::new(MarkType::Strong), Mark::new(MarkType::Em)];
        assert!(!Mark::same_set(&a, &b));
    }

    #[test]
    fn same_set_different_types() {
        let a = vec![Mark::new(MarkType::Strong)];
        let b = vec![Mark::new(MarkType::Em)];
        assert!(!Mark::same_set(&a, &b));
    }

    #[test]
    fn same_set_empty() {
        assert!(Mark::same_set(&[], &[]));
    }

    #[test]
    fn exclusion_is_symmetric() {
        assert!(MarkType::Code.excludes(MarkType::Strong));
        assert!(MarkType::Strong.excludes(MarkType::Code));
    }

    #[test]
    fn non_code_marks_dont_exclude_each_other() {
        assert!(!MarkType::Strong.excludes(MarkType::Em));
        assert!(!MarkType::Em.excludes(MarkType::Strike));
        assert!(!MarkType::Link.excludes(MarkType::Strong));
    }

    #[test]
    fn adding_code_to_set_with_strong_removes_strong() {
        let strong = Mark::new(MarkType::Strong);
        let code = Mark::new(MarkType::Code);

        let set = strong.add_to_set(&[]);
        assert_eq!(set.len(), 1);
        assert_eq!(set[0].mark_type, MarkType::Strong);

        // Adding Code should remove Strong and keep only Code.
        let set = code.add_to_set(&set);
        assert_eq!(set.len(), 1);
        assert_eq!(set[0].mark_type, MarkType::Code);
    }
}
