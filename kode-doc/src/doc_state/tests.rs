mod main_tests {
    use crate::doc_state::*;
    use crate::attrs::{empty_attrs, heading_attrs};
    use crate::fragment::Fragment;
    use crate::mark::{Mark, MarkType};
    use crate::node::Node;
    use crate::node_type::NodeType;
    // ── Helper ─────────────────────────────────────────────────────────

    /// Build: <doc><p>Hello</p></doc>
    fn simple_doc() -> Node {
        let p = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Hello")),
        );
        Node::branch(NodeType::Doc, Fragment::from_node(p))
    }

    /// Build: <doc><p>Hello</p><p>World</p></doc>
    fn two_para_doc() -> Node {
        let p1 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Hello")),
        );
        let p2 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("World")),
        );
        Node::branch(NodeType::Doc, Fragment::from_vec(vec![p1, p2]))
    }

    // ── Selection tests ────────────────────────────────────────────────

    #[test]
    fn selection_cursor_is_collapsed() {
        let sel = Selection::cursor(5);
        assert!(sel.is_cursor());
        assert_eq!(sel.anchor, 5);
        assert_eq!(sel.head, 5);
        assert_eq!(sel.from(), 5);
        assert_eq!(sel.to(), 5);
    }

    #[test]
    fn selection_range_forward() {
        let sel = Selection::range(2, 8);
        assert!(!sel.is_cursor());
        assert_eq!(sel.from(), 2);
        assert_eq!(sel.to(), 8);
    }

    #[test]
    fn selection_range_backward() {
        let sel = Selection::range(8, 2);
        assert!(!sel.is_cursor());
        assert_eq!(sel.from(), 2);
        assert_eq!(sel.to(), 8);
    }

    // ── Constructor tests ──────────────────────────────────────────────

    #[test]
    fn from_markdown_creates_doc() {
        let state = DocState::from_markdown("Hello world");
        assert_eq!(state.doc.child(0).text_content(), "Hello world");
        // Cursor at pos 1 (inside first textblock), not pos 0 (before it).
        assert_eq!(state.selection, Selection::cursor(1));
    }

    #[test]
    fn from_doc_creates_state() {
        let doc = simple_doc();
        let state = DocState::from_doc(doc);
        assert_eq!(state.doc.child(0).text_content(), "Hello");
        assert_eq!(state.selection, Selection::cursor(1));
    }

    #[test]
    fn from_doc_empty_doc_gets_default_paragraph() {
        // An empty doc is bootstrapped to have one empty paragraph, so the
        // cursor lands at pos 1 (inside the paragraph), not pos 0.
        let doc = Node::branch(NodeType::Doc, Fragment::empty());
        let state = DocState::from_doc(doc);
        assert_eq!(state.doc.child_count(), 1);
        assert_eq!(state.doc.child(0).node_type, NodeType::Paragraph);
        assert_eq!(state.selection, Selection::cursor(1));
    }

    // ── to_markdown round-trip ─────────────────────────────────────────

    #[test]
    fn to_markdown_round_trip() {
        let md = "Hello world";
        let state = DocState::from_markdown(md);
        let output = state.to_markdown();
        assert_eq!(output, md);
    }

    // ── set_from_markdown ──────────────────────────────────────────────

    #[test]
    fn set_from_markdown_replaces_doc() {
        let mut state = DocState::from_markdown("Original");
        state.set_from_markdown("Replaced");
        assert_eq!(state.doc.child(0).text_content(), "Replaced");
        // Cursor at pos 1 (inside first textblock).
        assert_eq!(state.selection, Selection::cursor(1));
        // Should be undoable.
        assert!(state.undo());
        assert_eq!(state.doc.child(0).text_content(), "Original");
    }

    // ── insert_text tests ──────────────────────────────────────────────

    #[test]
    fn insert_text_at_cursor() {
        // <doc><p>Hello</p></doc>
        // Positions: <p>=0, H=1, e=2, l=3, l=4, o=5, </p>=6, after=7
        // Place cursor at pos 3 (between "He" and "llo"), insert "XY".
        let mut state = DocState::from_doc(simple_doc());
        state.set_selection(Selection::cursor(3));
        state.insert_text("XY");

        assert_eq!(state.doc.child(0).text_content(), "HeXYllo");
        assert_eq!(state.selection, Selection::cursor(5)); // After "XY".
    }

    #[test]
    fn insert_text_replaces_selection() {
        // Select "ell" (pos 2..5), replace with "XY".
        let mut state = DocState::from_doc(simple_doc());
        state.set_selection(Selection::range(2, 5));
        state.insert_text("XY");

        assert_eq!(state.doc.child(0).text_content(), "HXYo");
        assert_eq!(state.selection, Selection::cursor(4)); // After "XY".
    }

    #[test]
    fn insert_text_inside_bold_preserves_mark_boundaries() {
        let mut ds = DocState::from_markdown("Hello **bold text** world");
        let initial = ds.to_markdown();
        assert!(initial.contains("**bold text**"), "setup: {initial}");

        // "Hello " = 6 chars, "bold text" = 9 chars. Paragraph starts at 1.
        // Position inside "bold" at offset 4 (after "bold"): 1 + 6 + 4 = 11
        ds.set_selection(Selection::cursor(11));
        ds.insert_text("X");

        let md = ds.to_markdown();
        assert_eq!(md, "Hello **boldX text** world", "after insert: {md}");
    }

    #[test]
    fn insert_text_inside_bold_after_heading() {
        // Simulate the demo document structure
        let mut ds = DocState::from_markdown(
            "# Dashboard Documentation\n\nThis dashboard tracks **monthly revenue** across all regions."
        );
        let md0 = ds.to_markdown();
        assert!(md0.contains("**monthly revenue**"), "setup: {md0}");

        // Heading: pos 0, node_size = 1 + 23 + 1 = 25
        // Paragraph: pos 25, content_start = 26
        // "This dashboard tracks " = 22 chars (positions 26-47)
        // "monthly revenue" = 15 chars (positions 48-62, bold)
        // " across all regions." = 20 chars (positions 63-82)
        // Insert 'x' at position 55 (after "monthly", offset 29 in paragraph)
        ds.set_selection(Selection::cursor(55));
        ds.insert_text("x");

        let md = ds.to_markdown();
        assert!(
            md.contains("**monthlyx revenue**"),
            "bold should wrap only 'monthlyx revenue', got: {md}"
        );
        assert!(
            md.contains("across all regions."),
            "trailing text should NOT be bold, got: {md}"
        );
    }

    #[test]
    fn insert_empty_text_is_noop() {
        let mut state = DocState::from_doc(simple_doc());
        state.set_selection(Selection::cursor(3));
        state.insert_text("");

        assert_eq!(state.doc.child(0).text_content(), "Hello");
        assert!(state.undo_stack.is_empty());
    }

    #[test]
    fn insert_text_into_empty_doc() {
        // from_doc bootstraps an empty paragraph, so cursor starts at 1.
        // Typing inserts into that paragraph.
        let doc = Node::branch(NodeType::Doc, Fragment::empty());
        let mut state = DocState::from_doc(doc);
        assert_eq!(state.selection, Selection::cursor(1));

        state.insert_text("Hello");

        // Should produce Doc[Paragraph[Text["Hello"]]]
        assert_eq!(state.doc.child_count(), 1);
        assert_eq!(state.doc.child(0).node_type, NodeType::Paragraph);
        assert_eq!(state.doc.child(0).text_content(), "Hello");
        // Cursor should be after the inserted text (pos 1 + 5 = 6)
        assert_eq!(state.selection, Selection::cursor(6));
    }

    #[test]
    fn insert_text_into_empty_doc_from_markdown() {
        // Same test but via from_markdown("") to match the real scenario.
        let mut state = DocState::from_markdown("");
        state.insert_text("Hi");

        assert_eq!(state.doc.child_count(), 1);
        assert_eq!(state.doc.child(0).node_type, NodeType::Paragraph);
        assert_eq!(state.doc.child(0).text_content(), "Hi");
    }

    #[test]
    fn empty_doc_has_default_paragraph() {
        let state = DocState::from_markdown("");
        assert_eq!(state.doc.child_count(), 1);
        assert_eq!(state.doc.child(0).node_type, NodeType::Paragraph);
        assert_eq!(state.doc.child(0).text_content(), "");
        assert_eq!(state.selection, Selection::cursor(1));
    }

    // ── backspace tests ────────────────────────────────────────────────

    #[test]
    fn backspace_within_text() {
        // Cursor at pos 3 (after "He"), backspace removes "e".
        let mut state = DocState::from_doc(simple_doc());
        state.set_selection(Selection::cursor(3));
        state.backspace();

        assert_eq!(state.doc.child(0).text_content(), "Hllo");
        assert_eq!(state.selection, Selection::cursor(2));
    }

    #[test]
    fn backspace_deletes_selection() {
        // Select "ell" (pos 2..5), backspace removes the selection.
        let mut state = DocState::from_doc(simple_doc());
        state.set_selection(Selection::range(2, 5));
        state.backspace();

        assert_eq!(state.doc.child(0).text_content(), "Ho");
        assert_eq!(state.selection, Selection::cursor(2));
    }

    #[test]
    fn backspace_at_start_of_doc_is_noop() {
        let mut state = DocState::from_doc(simple_doc());
        state.set_selection(Selection::cursor(0));
        state.backspace();

        assert_eq!(state.doc.child(0).text_content(), "Hello");
    }

    #[test]
    fn backspace_at_start_of_paragraph_joins() {
        // <doc><p>Hello</p><p>World</p></doc>
        // p1: pos 0..7 (open=0, H=1..5=o, close=6, after=7)
        // p2: pos 7..14 (open=7, W=8..12=d, close=13, after=14)
        // Cursor at pos 8 (start of p2 content).
        let mut state = DocState::from_doc(two_para_doc());
        state.set_selection(Selection::cursor(8));
        state.backspace();

        // Should join into one paragraph.
        assert_eq!(state.doc.child_count(), 1);
        assert_eq!(state.doc.child(0).text_content(), "HelloWorld");
    }

    // ── delete_forward tests ───────────────────────────────────────────

    #[test]
    fn delete_forward_within_text() {
        // Cursor at pos 3 (after "He"), delete forward removes "l".
        let mut state = DocState::from_doc(simple_doc());
        state.set_selection(Selection::cursor(3));
        state.delete_forward();

        assert_eq!(state.doc.child(0).text_content(), "Helo");
        assert_eq!(state.selection, Selection::cursor(3));
    }

    #[test]
    fn delete_forward_deletes_selection() {
        let mut state = DocState::from_doc(simple_doc());
        state.set_selection(Selection::range(2, 5));
        state.delete_forward();

        assert_eq!(state.doc.child(0).text_content(), "Ho");
        assert_eq!(state.selection, Selection::cursor(2));
    }

    #[test]
    fn delete_forward_at_end_of_doc_is_noop() {
        let mut state = DocState::from_doc(simple_doc());
        let doc_size = state.doc.content.size();
        state.set_selection(Selection::cursor(doc_size));
        state.delete_forward();

        assert_eq!(state.doc.child(0).text_content(), "Hello");
    }

    // ── split_block tests ──────────────────────────────────────────────

    #[test]
    fn split_block_at_cursor() {
        // <doc><p>Hello</p></doc>
        // Cursor at pos 3 (between "He" and "llo").
        let mut state = DocState::from_doc(simple_doc());
        state.set_selection(Selection::cursor(3));
        state.split_block();

        assert_eq!(state.doc.child_count(), 2);
        assert_eq!(state.doc.child(0).text_content(), "He");
        assert_eq!(state.doc.child(1).text_content(), "llo");
        // Cursor should be at start of second paragraph content.
        // After split: <p>He</p><p>llo</p>
        // p1: 0..4, p2: 4..9
        // Cursor at p2 content start = 5
        assert_eq!(state.selection, Selection::cursor(5));
    }

    #[test]
    fn split_block_with_range_selection() {
        // <doc><p>Hello</p></doc>
        // Select "ell" (pos 2..5), then split.
        // Expected: delete "ell" first → "Ho", then split at pos 2 → <p>H</p><p>o</p>
        let mut state = DocState::from_doc(simple_doc());
        state.set_selection(Selection::range(2, 5));
        state.split_block();

        assert_eq!(state.doc.child_count(), 2);
        assert_eq!(state.doc.child(0).text_content(), "H");
        assert_eq!(state.doc.child(1).text_content(), "o");

        // Cursor should be at start of second paragraph content.
        // After split: <p>H</p><p>o</p>
        // p1: 0..3, p2: 3..6, cursor at p2 content start = 4
        assert_eq!(state.selection, Selection::cursor(4));

        // Undo should restore the original document in one step.
        assert!(state.undo());
        assert_eq!(state.doc.child_count(), 1);
        assert_eq!(state.doc.child(0).text_content(), "Hello");
    }

    // ── toggle_mark tests ──────────────────────────────────────────────

    #[test]
    fn toggle_mark_adds_bold() {
        // Select "ell" (pos 2..5), toggle bold ON.
        let mut state = DocState::from_doc(simple_doc());
        state.set_selection(Selection::range(2, 5));
        state.toggle_mark(MarkType::Strong);

        let p = state.doc.child(0);
        // Should have 3 children: "H" (no mark), "ell" (strong), "o" (no mark).
        assert_eq!(p.child_count(), 3);
        assert_eq!(p.child(0).text(), Some("H"));
        assert!(p.child(0).marks.is_empty());
        assert_eq!(p.child(1).text(), Some("ell"));
        assert_eq!(p.child(1).marks[0].mark_type, MarkType::Strong);
        assert_eq!(p.child(2).text(), Some("o"));
        assert!(p.child(2).marks.is_empty());
    }

    #[test]
    fn toggle_mark_removes_bold() {
        // Start with all-bold text, toggle bold OFF.
        let bold_text = Node::new_text_with_marks("Hello", vec![Mark::new(MarkType::Strong)]);
        let p = Node::branch(NodeType::Paragraph, Fragment::from_node(bold_text));
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(p));

        let mut state = DocState::from_doc(doc);
        state.set_selection(Selection::range(1, 6)); // Entire text.
        state.toggle_mark(MarkType::Strong);

        let p = state.doc.child(0);
        assert_eq!(p.child_count(), 1);
        assert_eq!(p.child(0).text(), Some("Hello"));
        assert!(p.child(0).marks.is_empty());
    }

    #[test]
    fn toggle_mark_on_cursor_is_noop() {
        let mut state = DocState::from_doc(simple_doc());
        state.set_selection(Selection::cursor(3));
        state.toggle_mark(MarkType::Strong);

        // Doc should be unchanged.
        assert_eq!(state.doc.child(0).text_content(), "Hello");
        assert!(state.undo_stack.is_empty());
    }

    // ── set_block_type tests ───────────────────────────────────────────

    #[test]
    fn set_block_type_paragraph_to_heading() {
        // Cursor inside the paragraph. Change to heading.
        let mut state = DocState::from_doc(simple_doc());
        state.set_selection(Selection::cursor(3));
        state.set_block_type(NodeType::Heading, heading_attrs(2));

        let h = state.doc.child(0);
        assert_eq!(h.node_type, NodeType::Heading);
        assert_eq!(h.text_content(), "Hello");
    }

    #[test]
    fn set_block_type_heading_to_paragraph() {
        let h = Node::branch_with_attrs(
            NodeType::Heading,
            heading_attrs(1),
            Fragment::from_node(Node::new_text("Title")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(h));

        let mut state = DocState::from_doc(doc);
        state.set_selection(Selection::cursor(3));
        state.set_block_type(NodeType::Paragraph, empty_attrs());

        let p = state.doc.child(0);
        assert_eq!(p.node_type, NodeType::Paragraph);
        assert_eq!(p.text_content(), "Title");
    }

    // ── Undo/Redo tests ───────────────────────────────────────────────

    #[test]
    fn undo_reverts_insert() {
        let mut state = DocState::from_doc(simple_doc());
        state.set_selection(Selection::cursor(3));
        state.insert_text("XY");

        assert_eq!(state.doc.child(0).text_content(), "HeXYllo");

        assert!(state.undo());
        assert_eq!(state.doc.child(0).text_content(), "Hello");
        assert_eq!(state.selection, Selection::cursor(3));
    }

    #[test]
    fn redo_restores_insert() {
        let mut state = DocState::from_doc(simple_doc());
        state.set_selection(Selection::cursor(3));
        state.insert_text("XY");

        assert!(state.undo());
        assert_eq!(state.doc.child(0).text_content(), "Hello");

        assert!(state.redo());
        assert_eq!(state.doc.child(0).text_content(), "HeXYllo");
        assert_eq!(state.selection, Selection::cursor(5));
    }

    #[test]
    fn undo_empty_returns_false() {
        let mut state = DocState::from_doc(simple_doc());
        assert!(!state.undo());
    }

    #[test]
    fn redo_empty_returns_false() {
        let mut state = DocState::from_doc(simple_doc());
        assert!(!state.redo());
    }

    #[test]
    fn new_edit_clears_redo_stack() {
        let mut state = DocState::from_doc(simple_doc());
        state.set_selection(Selection::cursor(3));
        state.insert_text("X");
        state.undo();

        // Now insert something else — redo stack should be cleared.
        state.insert_text("Y");
        assert!(!state.redo());
    }

    // ── formatting_at_cursor tests ─────────────────────────────────────

    #[test]
    fn formatting_at_cursor_in_bold_text() {
        let bold_text = Node::new_text_with_marks("Hello", vec![Mark::new(MarkType::Strong)]);
        let p = Node::branch(NodeType::Paragraph, Fragment::from_node(bold_text));
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(p));

        let mut state = DocState::from_doc(doc);
        state.set_selection(Selection::cursor(3));

        let fmt = state.formatting_at_cursor();
        assert!(fmt.bold);
        assert!(!fmt.italic);
        assert!(!fmt.code);
        assert!(!fmt.strikethrough);
        assert_eq!(fmt.heading_level, 0);
    }

    #[test]
    fn formatting_at_cursor_on_heading() {
        let h = Node::branch_with_attrs(
            NodeType::Heading,
            heading_attrs(2),
            Fragment::from_node(Node::new_text("Title")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(h));

        let mut state = DocState::from_doc(doc);
        state.set_selection(Selection::cursor(3));

        let fmt = state.formatting_at_cursor();
        assert_eq!(fmt.heading_level, 2);
    }

    #[test]
    fn formatting_at_cursor_in_blockquote() {
        let p = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("quoted")),
        );
        let bq = Node::branch(NodeType::Blockquote, Fragment::from_node(p));
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(bq));

        let mut state = DocState::from_doc(doc);
        // pos 3 is inside the paragraph inside the blockquote.
        state.set_selection(Selection::cursor(3));

        let fmt = state.formatting_at_cursor();
        assert!(fmt.blockquote);
    }

    // ── resolve_cursor test ────────────────────────────────────────────

    #[test]
    fn resolve_cursor_returns_resolved_pos() {
        let mut state = DocState::from_doc(simple_doc());
        state.set_selection(Selection::cursor(3));

        let rp = state.resolve_cursor();
        assert_eq!(rp.pos, 3);
        assert_eq!(rp.depth, 1);
        assert_eq!(rp.parent().node_type, NodeType::Paragraph);
    }

    // ── backspace at pos 1 (start of first paragraph) ─────────────────

    #[test]
    fn backspace_at_pos_1_first_paragraph_is_noop() {
        // <doc><p>Hello</p></doc>
        // pos 1 = start of paragraph content (parent_offset=0, textblock).
        // This is the first block, so join_backward returns false.
        let mut state = DocState::from_doc(simple_doc());
        state.set_selection(Selection::cursor(1));
        state.backspace();

        // Doc unchanged — no previous block to join with.
        assert_eq!(state.doc.child_count(), 1);
        assert_eq!(state.doc.child(0).text_content(), "Hello");
        // No undo entry pushed since nothing happened.
        assert!(state.undo_stack.is_empty());
    }

    // ── delete_forward at textblock boundary ──────────────────────────

    #[test]
    fn delete_forward_at_end_of_textblock_joins_with_next() {
        // <doc><p>Hello</p><p>World</p></doc>
        // p1: pos 0..7 (open=0, H=1..5=o, close=6)
        // p2: pos 7..14 (open=7, W=8..12=d, close=13)
        // Cursor at pos 6 (end of p1 content).
        let mut state = DocState::from_doc(two_para_doc());
        state.set_selection(Selection::cursor(6));
        state.delete_forward();

        // Should join into one paragraph.
        assert_eq!(state.doc.child_count(), 1);
        assert_eq!(state.doc.child(0).text_content(), "HelloWorld");
        // Cursor stays at pos 6 (between "Hello" and "World" in the joined block).
        assert_eq!(state.selection, Selection::cursor(6));
    }

    #[test]
    fn delete_forward_at_end_of_last_block_is_noop() {
        // <doc><p>Hello</p></doc>
        // Cursor at pos 6 (end of p content). No next block to join.
        let mut state = DocState::from_doc(simple_doc());
        state.set_selection(Selection::cursor(6));
        state.delete_forward();

        assert_eq!(state.doc.child_count(), 1);
        assert_eq!(state.doc.child(0).text_content(), "Hello");
        assert!(state.undo_stack.is_empty());
    }

    // ── toggle_mark on partial-mark range ─────────────────────────────

    #[test]
    fn toggle_mark_on_partial_mark_range_adds_to_all() {
        // <doc><p><strong>He</strong>llo</p></doc>
        // "He" is bold, "llo" is not. Select full text (pos 1..6).
        // Since not ALL text has bold, toggle should ADD bold to the range.
        let bold_he = Node::new_text_with_marks("He", vec![Mark::new(MarkType::Strong)]);
        let plain_llo = Node::new_text("llo");
        let p = Node::branch(
            NodeType::Paragraph,
            Fragment::from_vec(vec![bold_he, plain_llo]),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(p));

        let mut state = DocState::from_doc(doc);
        state.set_selection(Selection::range(1, 6));
        state.toggle_mark(MarkType::Strong);

        // All text should now be bold.
        let p = state.doc.child(0);
        assert_eq!(p.text_content(), "Hello");
        // Every child text node should have the Strong mark.
        for i in 0..p.child_count() {
            assert!(
                p.child(i).marks.iter().any(|m| m.mark_type == MarkType::Strong),
                "child {} should have Strong mark",
                i
            );
        }
    }

    // ── range_has_mark text-free selection ─────────────────────────────

    #[test]
    fn range_has_mark_returns_false_on_empty_paragraph() {
        // <doc><p></p></doc>
        // Select the entire paragraph range (pos 0..2). No text nodes exist.
        let p = Node::branch(NodeType::Paragraph, Fragment::empty());
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(p));

        let state = DocState::from_doc(doc);
        // range_has_mark should return false (no text nodes found).
        assert!(!state.range_has_mark(0, 2, MarkType::Strong));
    }

    // ── select_word tests ─────────────────────────────────────────────

    #[test]
    fn select_word_middle_of_word() {
        // <doc><p>Hello world</p></doc>
        // Positions: <p>=0, H=1, e=2, l=3, l=4, o=5, ' '=6, w=7, o=8, r=9, l=10, d=11, </p>=12
        // Cursor at pos 3 (inside "Hello"), should select "Hello" -> pos 1..6.
        let mut state = DocState::from_markdown("Hello world");
        state.set_selection(Selection::cursor(3));
        state.select_word();

        assert_eq!(state.selection, Selection::range(1, 6));
    }

    #[test]
    fn select_word_second_word() {
        // Cursor at pos 9 (inside "world"), should select "world" -> pos 7..12.
        let mut state = DocState::from_markdown("Hello world");
        state.set_selection(Selection::cursor(9));
        state.select_word();

        assert_eq!(state.selection, Selection::range(7, 12));
    }

    #[test]
    fn select_word_single_word() {
        // Cursor inside a single-word paragraph.
        let mut state = DocState::from_markdown("Hello");
        state.set_selection(Selection::cursor(3));
        state.select_word();

        assert_eq!(state.selection, Selection::range(1, 6));
    }

    #[test]
    fn select_word_at_space() {
        // Cursor at the space (pos 6). Space is whitespace, so start=6, end=6.
        // No word found — selection should remain unchanged.
        let mut state = DocState::from_markdown("Hello world");
        state.set_selection(Selection::cursor(6));
        let original = state.selection.clone();
        state.select_word();

        // start == end, so selection is not updated.
        assert_eq!(state.selection, original);
    }

    // ── select_line tests ─────────────────────────────────────────────

    #[test]
    fn select_line_simple() {
        // <doc><p>Hello</p></doc>
        // Paragraph content runs from pos 1 to pos 6.
        let mut state = DocState::from_markdown("Hello");
        state.set_selection(Selection::cursor(3));
        state.select_line();

        assert_eq!(state.selection, Selection::range(1, 6));
    }

    #[test]
    fn select_line_two_paragraphs() {
        // <doc><p>Hello</p><p>World</p></doc>
        // p1 content: pos 1..6, p2 content: pos 8..13
        // Cursor inside p2 (pos 10), should select p2 content.
        let mut state = DocState::from_markdown("Hello\n\nWorld");
        state.set_selection(Selection::cursor(10));
        state.select_line();

        assert_eq!(state.selection, Selection::range(8, 13));
    }

    // ── wrap / lift / blockquote tests ────────────────────────────────

    /// Build: <doc><blockquote><p>Text</p></blockquote></doc>
    fn blockquote_doc() -> Node {
        let p = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Text")),
        );
        let bq = Node::branch(NodeType::Blockquote, Fragment::from_node(p));
        Node::branch(NodeType::Doc, Fragment::from_node(bq))
    }

    #[test]
    fn wrap_paragraph_in_blockquote() {
        // <doc><p>Hello</p></doc>
        // Wrap the paragraph in a blockquote.
        let mut state = DocState::from_doc(simple_doc());
        state.set_selection(Selection::cursor(3));
        state.toggle_blockquote();

        // Should be: <doc><blockquote><p>Hello</p></blockquote></doc>
        assert_eq!(state.doc.child_count(), 1);
        let bq = state.doc.child(0);
        assert_eq!(bq.node_type, NodeType::Blockquote);
        assert_eq!(bq.child_count(), 1);
        assert_eq!(bq.child(0).node_type, NodeType::Paragraph);
        assert_eq!(bq.child(0).text_content(), "Hello");
    }

    #[test]
    fn lift_from_blockquote() {
        // <doc><blockquote><p>Text</p></blockquote></doc>
        // Lift the paragraph out of blockquote.
        let mut state = DocState::from_doc(blockquote_doc());
        // Cursor inside the paragraph: pos 3 (bq open=0, p open=1, T=2, e=3)
        state.set_selection(Selection::cursor(3));
        state.toggle_blockquote();

        // Should be: <doc><p>Text</p></doc>
        assert_eq!(state.doc.child_count(), 1);
        let p = state.doc.child(0);
        assert_eq!(p.node_type, NodeType::Paragraph);
        assert_eq!(p.text_content(), "Text");
    }

    #[test]
    fn toggle_blockquote_on_off() {
        let mut state = DocState::from_doc(simple_doc());
        state.set_selection(Selection::cursor(3));

        // Toggle ON.
        state.toggle_blockquote();
        assert!(state.is_in_node(NodeType::Blockquote));

        // Toggle OFF.
        // Cursor position shifted by the blockquote wrapper (+1 token).
        state.toggle_blockquote();
        assert!(!state.is_in_node(NodeType::Blockquote));

        // Verify structure restored.
        assert_eq!(state.doc.child_count(), 1);
        assert_eq!(state.doc.child(0).node_type, NodeType::Paragraph);
        assert_eq!(state.doc.child(0).text_content(), "Hello");
    }

    #[test]
    fn backspace_empty_para_in_blockquote_preserves_blockquote() {
        // <doc><blockquote><p>Line one</p><p></p><p>Line three</p></blockquote></doc>
        // Cursor at start of the empty middle paragraph.
        // Backspace should delete the empty paragraph, NOT unwrap the blockquote.
        let p1 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Line one")),
        );
        let p2 = Node::branch(NodeType::Paragraph, Fragment::empty());
        let p3 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Line three")),
        );
        let bq = Node::branch(
            NodeType::Blockquote,
            Fragment::from_vec(vec![p1, p2, p3]),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(bq));

        let mut state = DocState::from_doc(doc);

        // Position inside the empty paragraph:
        // doc(0) > bq(1) > p1(2)..."Line one"(3-10) > /p1(11) > p2(12) > content_start(13)
        // p2 is empty so its content start = 12 (bq_open + p1_size + p2_open)
        // bq_open = 1, p1 size = "Line one".len() + 2 = 10, p2_open at 11, content at 12
        // Actually: doc_open not counted in content positions.
        // pos 0 = before bq, pos 1 = inside bq before p1
        // p1: open=1, content 2..10 ("Line one" = 8 chars), close=10
        // Wait, bq open = position 0 in doc content? No...
        // Let me compute: doc has one child (bq). bq starts at position 0 in doc content.
        // Inside bq: position 1 is inside bq content.
        // p1 starts at position 1 (inside bq). p1 content starts at position 2.
        // "Line one" = 8 chars, positions 2-9. p1 ends at position 10.
        // p2 starts at position 10. p2 content starts at position 11. p2 is empty. p2 ends at 11.
        // Wait, empty paragraph: open + close = 2 tokens, 0 content. So p2 occupies positions 10-11 (size 2).
        // p2 content start = 11. That's where the cursor should be.
        state.set_selection(Selection::cursor(12));

        state.backspace();

        // The blockquote should still exist with p1 and p3.
        assert_eq!(state.doc.child_count(), 1, "doc should have 1 child");
        let bq = state.doc.child(0);
        assert_eq!(bq.node_type, NodeType::Blockquote, "child should be blockquote");
        assert_eq!(bq.child_count(), 2, "blockquote should have 2 paragraphs");
        assert_eq!(bq.child(0).text_content(), "Line one");
        assert_eq!(bq.child(1).text_content(), "Line three");
    }

    // ── list tests ───────────────────────────────────────────────────

    #[test]
    fn wrap_in_bullet_list() {
        // <doc><p>Hello</p></doc>
        let mut state = DocState::from_doc(simple_doc());
        state.set_selection(Selection::cursor(3));
        state.toggle_bullet_list();

        // Should be: <doc><ul><li><p>Hello</p></li></ul></doc>
        assert_eq!(state.doc.child_count(), 1);
        let ul = state.doc.child(0);
        assert_eq!(ul.node_type, NodeType::BulletList);
        assert_eq!(ul.child_count(), 1);
        let li = ul.child(0);
        assert_eq!(li.node_type, NodeType::ListItem);
        assert_eq!(li.child_count(), 1);
        assert_eq!(li.child(0).node_type, NodeType::Paragraph);
        assert_eq!(li.child(0).text_content(), "Hello");
    }

    #[test]
    fn lift_from_bullet_list() {
        // Start with a bullet list, then toggle it off.
        let mut state = DocState::from_doc(simple_doc());
        state.set_selection(Selection::cursor(3));
        state.toggle_bullet_list();
        assert!(state.is_in_node(NodeType::BulletList));

        state.toggle_bullet_list();
        assert!(!state.is_in_node(NodeType::BulletList));
        assert!(!state.is_in_node(NodeType::ListItem));

        assert_eq!(state.doc.child_count(), 1);
        assert_eq!(state.doc.child(0).node_type, NodeType::Paragraph);
        assert_eq!(state.doc.child(0).text_content(), "Hello");
    }

    #[test]
    fn toggle_bullet_list_off_extracts_first_item_only() {
        // Build: <doc><p>Hello</p><p>World</p></doc>
        let mut state = DocState::from_doc(two_para_doc());

        // Select across both paragraphs and toggle bullet list ON.
        // Positions: 0(doc) 1(p) H(2) e(3) l(4) l(5) o(6) 7(/p) 8(p) W(9) ...
        state.set_selection(Selection::range(2, 9));
        state.toggle_bullet_list();

        // Should be: <doc><ul><li><p>Hello</p></li><li><p>World</p></li></ul></doc>
        assert_eq!(state.doc.child_count(), 1);
        let ul = state.doc.child(0);
        assert_eq!(ul.node_type, NodeType::BulletList);
        assert_eq!(ul.child_count(), 2);
        assert_eq!(ul.child(0).child(0).text_content(), "Hello");
        assert_eq!(ul.child(1).child(0).text_content(), "World");

        // Place a collapsed cursor inside item 0 and toggle OFF.
        // Wrapped positions: UL(0) LI1(1) P1(2) H(3) e(4) l(5) l(6) o(7) /P1(8) /LI1(9)
        //                    LI2(10) P2(11) W(12) o(13) r(14) l(15) d(16) /P2(17) /LI2(18) /UL(19)
        state.set_selection(Selection::cursor(4));
        state.toggle_bullet_list();

        // Only item 0 extracted; item 1 stays in the list.
        assert!(!state.is_in_node(NodeType::BulletList));
        assert_eq!(
            state.doc.child_count(),
            2,
            "After toggle off, doc should have Paragraph + BulletList but got: {:#?}",
            state.doc
        );
        assert_eq!(state.doc.child(0).node_type, NodeType::Paragraph);
        assert_eq!(state.doc.child(0).text_content(), "Hello");
        assert_eq!(state.doc.child(1).node_type, NodeType::BulletList);
        assert_eq!(state.doc.child(1).child_count(), 1);
        assert_eq!(state.doc.child(1).child(0).child(0).text_content(), "World");
        // Cursor shifted -2 (removed UL_open + LI_open).
        assert_eq!(state.selection.anchor, 2);
    }

    #[test]
    fn toggle_bullet_to_ordered_list() {
        let mut state = DocState::from_doc(simple_doc());
        state.set_selection(Selection::cursor(3));

        // First wrap in bullet list.
        state.toggle_bullet_list();
        assert!(state.is_in_node(NodeType::BulletList));

        // Toggle ordered list — should change type, not nest.
        state.toggle_ordered_list();
        assert!(state.is_in_node(NodeType::OrderedList));
        assert!(!state.is_in_node(NodeType::BulletList));

        // Content preserved.
        let ol = state.doc.child(0);
        assert_eq!(ol.node_type, NodeType::OrderedList);
        assert_eq!(ol.child(0).child(0).text_content(), "Hello");
    }

    // ── toggle list off: single-item extraction tests ─────────────────

    /// Build: <doc><ul><li><p>aaa</p></li><li><p>bbb</p></li><li><p>ccc</p></li></ul></doc>
    ///
    /// Positions:
    /// 0:UL  1:LI1  2:P1  3:a 4:a 5:a  6:/P1  7:/LI1
    /// 8:LI2  9:P2  10:b 11:b 12:b  13:/P2  14:/LI2
    /// 15:LI3  16:P3  17:c 18:c 19:c  20:/P3  21:/LI3  22:/UL
    fn three_item_bullet_list_doc() -> Node {
        let items: Vec<Node> = ["aaa", "bbb", "ccc"]
            .iter()
            .map(|text| {
                Node::branch(
                    NodeType::ListItem,
                    Fragment::from_node(Node::branch(
                        NodeType::Paragraph,
                        Fragment::from_node(Node::new_text(text)),
                    )),
                )
            })
            .collect();
        let list = Node::branch(NodeType::BulletList, Fragment::from_vec(items));
        Node::branch(NodeType::Doc, Fragment::from_node(list))
    }

    #[test]
    fn toggle_list_off_only_item() {
        // Single-item list: toggling off extracts the paragraph.
        let mut state = DocState::from_doc(simple_doc());
        state.set_selection(Selection::cursor(3));
        state.toggle_bullet_list();
        assert!(state.is_in_node(NodeType::BulletList));

        state.toggle_bullet_list();
        assert!(!state.is_in_node(NodeType::BulletList));
        assert_eq!(state.doc.child_count(), 1);
        assert_eq!(state.doc.child(0).node_type, NodeType::Paragraph);
        assert_eq!(state.doc.child(0).text_content(), "Hello");
        assert_eq!(state.selection.anchor, 3); // 5 - 2 (UL_open + LI_open)
    }

    #[test]
    fn toggle_list_off_first_item() {
        // 3-item list, cursor in item 0 (position 4).
        // Extracting item 0 should produce: P("aaa") + BulletList("bbb", "ccc").
        let mut state = DocState::from_doc(three_item_bullet_list_doc());
        state.set_selection(Selection::cursor(4));
        assert!(state.is_in_node(NodeType::BulletList));

        state.toggle_bullet_list();

        assert_eq!(
            state.doc.child_count(),
            2,
            "Expected Paragraph + BulletList, got: {:#?}",
            state.doc
        );
        assert_eq!(state.doc.child(0).node_type, NodeType::Paragraph);
        assert_eq!(state.doc.child(0).text_content(), "aaa");

        let ul = state.doc.child(1);
        assert_eq!(ul.node_type, NodeType::BulletList);
        assert_eq!(ul.child_count(), 2);
        assert_eq!(ul.child(0).child(0).text_content(), "bbb");
        assert_eq!(ul.child(1).child(0).text_content(), "ccc");

        // Cursor should shift by -2 (UL_open + LI_open removed).
        assert_eq!(state.selection.anchor, 2); // 4 - 2
    }

    #[test]
    fn toggle_list_off_last_item() {
        // 3-item list, cursor in item 2 (position 18).
        // Extracting last item should produce: BulletList("aaa", "bbb") + P("ccc").
        let mut state = DocState::from_doc(three_item_bullet_list_doc());
        state.set_selection(Selection::cursor(18));
        assert!(state.is_in_node(NodeType::BulletList));

        state.toggle_bullet_list();

        assert_eq!(
            state.doc.child_count(),
            2,
            "Expected BulletList + Paragraph, got: {:#?}",
            state.doc
        );
        let ul = state.doc.child(0);
        assert_eq!(ul.node_type, NodeType::BulletList);
        assert_eq!(ul.child_count(), 2);
        assert_eq!(ul.child(0).child(0).text_content(), "aaa");
        assert_eq!(ul.child(1).child(0).text_content(), "bbb");

        assert_eq!(state.doc.child(1).node_type, NodeType::Paragraph);
        assert_eq!(state.doc.child(1).text_content(), "ccc");

        // Cursor position unchanged for last item.
        assert_eq!(state.selection.anchor, 18);
    }

    #[test]
    fn toggle_list_off_middle_item() {
        // 3-item list, cursor in item 1 (position 11).
        // Extracting middle item should produce: BulletList("aaa") + P("bbb") + BulletList("ccc").
        let mut state = DocState::from_doc(three_item_bullet_list_doc());
        state.set_selection(Selection::cursor(11));
        assert!(state.is_in_node(NodeType::BulletList));

        state.toggle_bullet_list();

        assert_eq!(
            state.doc.child_count(),
            3,
            "Expected BulletList + Paragraph + BulletList, got: {:#?}",
            state.doc
        );
        assert_eq!(state.doc.child(0).node_type, NodeType::BulletList);
        assert_eq!(state.doc.child(0).child_count(), 1);
        assert_eq!(state.doc.child(0).child(0).child(0).text_content(), "aaa");

        assert_eq!(state.doc.child(1).node_type, NodeType::Paragraph);
        assert_eq!(state.doc.child(1).text_content(), "bbb");

        assert_eq!(state.doc.child(2).node_type, NodeType::BulletList);
        assert_eq!(state.doc.child(2).child_count(), 1);
        assert_eq!(state.doc.child(2).child(0).child(0).text_content(), "ccc");

        // Cursor position unchanged for middle item.
        assert_eq!(state.selection.anchor, 11);
    }

    #[test]
    fn toggle_ordered_list_off_middle_item() {
        // Same test but with ordered list to verify both list types work.
        let items: Vec<Node> = ["aaa", "bbb", "ccc"]
            .iter()
            .map(|text| {
                Node::branch(
                    NodeType::ListItem,
                    Fragment::from_node(Node::branch(
                        NodeType::Paragraph,
                        Fragment::from_node(Node::new_text(text)),
                    )),
                )
            })
            .collect();
        let list = Node::branch_with_attrs(
            NodeType::OrderedList,
            crate::attrs::ordered_list_attrs(1),
            Fragment::from_vec(items),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(list));

        let mut state = DocState::from_doc(doc);
        state.set_selection(Selection::cursor(11));
        assert!(state.is_in_node(NodeType::OrderedList));

        state.toggle_ordered_list();

        assert_eq!(
            state.doc.child_count(),
            3,
            "Expected OrderedList + Paragraph + OrderedList, got: {:#?}",
            state.doc
        );
        assert_eq!(state.doc.child(0).node_type, NodeType::OrderedList);
        assert_eq!(state.doc.child(0).child_count(), 1);
        assert_eq!(state.doc.child(0).child(0).child(0).text_content(), "aaa");

        assert_eq!(state.doc.child(1).node_type, NodeType::Paragraph);
        assert_eq!(state.doc.child(1).text_content(), "bbb");

        assert_eq!(state.doc.child(2).node_type, NodeType::OrderedList);
        assert_eq!(state.doc.child(2).child_count(), 1);
        assert_eq!(state.doc.child(2).child(0).child(0).text_content(), "ccc");

        // Cursor position unchanged for middle item.
        assert_eq!(state.selection.anchor, 11);
    }

    // ── split_block in list tests ────────────────────────────────────

    #[test]
    fn split_block_in_bullet_list_creates_new_item() {
        // Create: <doc><ul><li><p>Hello</p></li></ul></doc>
        let mut state = DocState::from_doc(simple_doc());
        state.set_selection(Selection::cursor(3));
        state.toggle_bullet_list();

        // Structure after wrap: Doc > BulletList > ListItem > Paragraph > "Hello"
        // Positions: 0(doc_open) 1(ul_open) 2(li_open) 3(p_open) H(4) e(5) l(6) l(7) o(8)
        //            9(p_close) 10(li_close) 11(ul_close) 12(doc_close)
        // Text positions: H=3, e=4, l=5, l=6, o=7.
        // Split at pos 5 gives "He" | "llo".
        state.set_selection(Selection::cursor(5));
        state.split_block();

        // Should produce:
        // <doc><ul><li><p>He</p></li><li><p>llo</p></li></ul></doc>
        let ul = state.doc.child(0);
        assert_eq!(ul.node_type, NodeType::BulletList);
        assert_eq!(ul.child_count(), 2, "BulletList should have 2 ListItems");
        let li0 = ul.child(0);
        assert_eq!(li0.node_type, NodeType::ListItem);
        assert_eq!(li0.child(0).text_content(), "He");
        let li1 = ul.child(1);
        assert_eq!(li1.node_type, NodeType::ListItem);
        assert_eq!(li1.child(0).text_content(), "llo");
    }

    #[test]
    fn split_block_in_ordered_list_creates_new_item() {
        let mut state = DocState::from_doc(simple_doc());
        state.set_selection(Selection::cursor(3));
        state.toggle_ordered_list();

        // Same position layout as bullet list. Split at "He" | "llo".
        state.set_selection(Selection::cursor(5));
        state.split_block();

        let ol = state.doc.child(0);
        assert_eq!(ol.node_type, NodeType::OrderedList);
        assert_eq!(ol.child_count(), 2, "OrderedList should have 2 ListItems");
        assert_eq!(ol.child(0).child(0).text_content(), "He");
        assert_eq!(ol.child(1).child(0).text_content(), "llo");
    }

    // ── insert_horizontal_rule tests ─────────────────────────────────

    #[test]
    fn insert_horizontal_rule_at_cursor() {
        // <doc><p>Hello</p></doc>
        // Insert HR at cursor pos 3 (inside "Hello").
        let mut state = DocState::from_doc(simple_doc());
        state.set_selection(Selection::cursor(3));
        state.insert_horizontal_rule();

        // Should produce: <doc><p>He</p><hr><p>llo</p></doc>
        assert_eq!(state.doc.child_count(), 3);
        assert_eq!(state.doc.child(0).node_type, NodeType::Paragraph);
        assert_eq!(state.doc.child(0).text_content(), "He");
        assert_eq!(state.doc.child(1).node_type, NodeType::HorizontalRule);
        assert_eq!(state.doc.child(2).node_type, NodeType::Paragraph);
        assert_eq!(state.doc.child(2).text_content(), "llo");
    }

    // ── insert_link tests ────────────────────────────────────────────

    #[test]
    fn insert_link_at_cursor_inserts_text() {
        let mut state = DocState::from_doc(simple_doc());
        state.set_selection(Selection::cursor(3));
        state.insert_link("https://example.com");

        // Should insert the URL as linked text at pos 3.
        let p = state.doc.child(0);
        let text = p.text_content();
        assert!(text.contains("https://example.com"));
    }

    #[test]
    fn insert_link_on_selection_adds_mark() {
        let mut state = DocState::from_doc(simple_doc());
        state.set_selection(Selection::range(2, 5));
        state.insert_link("https://example.com");

        // "ell" should have a Link mark.
        let p = state.doc.child(0);
        let mut found_link = false;
        for i in 0..p.child_count() {
            if p.child(i)
                .marks
                .iter()
                .any(|m| m.mark_type == MarkType::Link)
            {
                found_link = true;
            }
        }
        assert!(found_link, "expected a child with Link mark");
    }

    // ── formatting_at_cursor for lists and blockquote ─────────────────

    #[test]
    fn formatting_at_cursor_inside_blockquote_returns_true() {
        let mut state = DocState::from_doc(blockquote_doc());
        state.set_selection(Selection::cursor(3));
        let fmt = state.formatting_at_cursor();
        assert!(fmt.blockquote);
    }

    #[test]
    fn formatting_at_cursor_inside_bullet_list_returns_true() {
        let mut state = DocState::from_doc(simple_doc());
        state.set_selection(Selection::cursor(3));
        state.toggle_bullet_list();

        let fmt = state.formatting_at_cursor();
        assert!(fmt.bullet_list);
    }
}

mod end_of_line_tests {
    use crate::doc_state::*;
    use crate::node::Node;
    use crate::node_type::NodeType;
    use crate::fragment::Fragment;

    #[test]
    fn insert_text_at_end_of_heading() {
        // <doc><h1>Hello</h1></doc>
        // Positions: <h1>=0, H=1, e=2, l=3, l=4, o=5, </h1>=6
        let h1 = Node::branch(
            NodeType::Heading,
            Fragment::from_node(Node::new_text("Hello")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(h1));
        let mut state = DocState::from_doc(doc);

        state.set_selection(Selection::cursor(6));

        let resolved = state.doc().resolve(6);
        eprintln!("At pos 6: parent={:?}, depth={}", resolved.parent().node_type, resolved.depth);

        state.insert_text("X");
        eprintln!("After insert: text={}", state.doc().child(0).text_content());

        assert_eq!(state.doc().child(0).text_content(), "HelloX");
    }

    #[test]
    fn split_block_at_end_of_heading() {
        let h1 = Node::branch(
            NodeType::Heading,
            Fragment::from_node(Node::new_text("Hello")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(h1));
        let mut state = DocState::from_doc(doc);

        state.set_selection(Selection::cursor(6));

        let before_count = state.doc().content.children().len();
        state.split_block();
        let after_count = state.doc().content.children().len();

        eprintln!("Blocks: {} -> {}", before_count, after_count);
        eprintln!("Block 0: {:?} text={}", state.doc().child(0).node_type, state.doc().child(0).text_content());
        if after_count > 1 {
            eprintln!("Block 1: {:?} text={}", state.doc().child(1).node_type, state.doc().child(1).text_content());
        }

        assert!(after_count > before_count, "split_block should create a new block");
    }

    #[test]
    fn insert_text_at_end_of_paragraph() {
        // <doc><p>ABC</p></doc>
        // <p>=0, A=1, B=2, C=3, </p>=4
        let p = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("ABC")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(p));
        let mut state = DocState::from_doc(doc);

        state.set_selection(Selection::cursor(4));
        state.insert_text("D");

        assert_eq!(state.doc().child(0).text_content(), "ABCD");
    }

    #[test]
    fn insert_text_at_end_of_heading_with_following_paragraph() {
        // <doc><h1>Hello</h1><p>World</p></doc>
        // <doc>=0, <h1>=1, H=2, e=3, l=4, l=5, o=6, </h1>=7, <p>=8, W=9...
        // Wait — pos 7 is the closing token of h1 AND pos 7 is between blocks.
        // What does resolve(7) return?
        let h1 = Node::branch(
            NodeType::Heading,
            Fragment::from_node(Node::new_text("Hello")),
        );
        let p = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("World")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_vec(vec![h1, p]));
        let mut state = DocState::from_doc(doc);

        // Position 7 = closing token of h1
        let resolved = state.doc().resolve(7);
        eprintln!("Multi-block: pos 7 parent={:?}, depth={}", resolved.parent().node_type, resolved.depth);

        // Try inserting at pos 7
        state.set_selection(Selection::cursor(7));
        state.insert_text("X");
        eprintln!("After insert at 7: h1={}, p={}", state.doc().child(0).text_content(), state.doc().child(1).text_content());
    }
}

mod cursor_accuracy_tests {
    use crate::doc_state::*;
    use crate::node::Node;
    use crate::node_type::NodeType;
    use crate::fragment::Fragment;

    #[test]
    fn insert_at_position_7_in_multiblock_doc() {
        // Simulate: <doc><h1>Dashboard Documentation</h1><p>...</p></doc>
        let h1 = Node::branch(
            NodeType::Heading,
            Fragment::from_node(Node::new_text("Dashboard Documentation")),
        );
        let p = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Some text here")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_vec(vec![h1, p]));
        // pos 0 = <doc>, pos 1 = <h1>, pos 2 = 'D', pos 7 = 'o', pos 25 = </h1>, pos 26 = <p>
        let mut state = DocState::from_doc(doc);

        state.set_selection(Selection::cursor(7));
        let resolved = state.doc().resolve(7);
        eprintln!("pos 7: parent={:?} parent_offset={}", resolved.parent().node_type, resolved.parent_offset);

        state.insert_text("|");
        let h1_text = state.doc().child(0).text_content();
        let marker_idx = h1_text.find('|').unwrap();
        eprintln!("H1 after insert at 7: \"{}\" marker at idx {}", h1_text, marker_idx);

        // Position 7 = offset 6 in heading content = before 'a' in "Dashboard"
        // Expected: "Dashbo|ard Documentation" with marker at index 6
        assert_eq!(marker_idx, 6, "marker should be at index 6 (before 'a' in Dashboard), got {}", marker_idx);
    }
}

mod list_backspace_tests {
    use crate::doc_state::*;
    use crate::node::Node;
    use crate::node_type::NodeType;
    use crate::fragment::Fragment;

    #[test]
    fn backspace_at_start_of_empty_list_item() {
        // <doc><ul><li><p></p></li><li><p>World</p></li></ul></doc>
        // Structure: Doc > BulletList > ListItem > Paragraph(empty)
        let empty_p = Node::branch(NodeType::Paragraph, Fragment::empty());
        let li1 = Node::branch(NodeType::ListItem, Fragment::from_node(empty_p));
        let p2 = Node::branch(NodeType::Paragraph, Fragment::from_node(Node::new_text("World")));
        let li2 = Node::branch(NodeType::ListItem, Fragment::from_node(p2));
        let ul = Node::branch(NodeType::BulletList, Fragment::from_vec(vec![li1, li2]));
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(ul));

        eprintln!("Doc size: {}", doc.content.size());
        // Positions: <ul>=0, <li1>=1, <p>=2, </p>=3, </li1>=4, <li2>=5, ...
        // Empty paragraph content is at position 3 (content_start=3, size=0)
        // Actually: <ul>0 <li1>1 <p>2 </p>3 </li1>4 <li2>5 <p>6 W7 o8 r9 l10 d11 </p>12 </li2>13 </ul>14

        let mut state = DocState::from_doc(doc);
        // Cursor at start of empty paragraph inside first LI
        state.set_selection(Selection::cursor(3));

        let resolved = state.doc().resolve(3);
        eprintln!("pos 3: parent={:?} depth={} parent_offset={}",
            resolved.parent().node_type, resolved.depth, resolved.parent_offset);
        eprintln!("depth-1 node: {:?}", resolved.node(resolved.depth.saturating_sub(1)).node_type);

        let before_li = state.doc().child(0).child_count();
        eprintln!("LI count before: {}", before_li);

        state.backspace();

        let after_li = state.doc().child(0).child_count();
        eprintln!("LI count after: {}", after_li);
        eprintln!("First child type: {:?}", state.doc().child(0).node_type);

        assert!(after_li < before_li || state.doc().child(0).node_type != NodeType::BulletList,
            "Backspace should have removed or lifted the empty LI");
    }

    #[test]
    fn backspace_at_start_of_second_ol_item_merges() {
        // <doc><ol><li><p>First item</p></li><li><p>Second item</p></li></ol></doc>
        let p1 = Node::branch(NodeType::Paragraph, Fragment::from_node(Node::new_text("First item")));
        let li1 = Node::branch(NodeType::ListItem, Fragment::from_node(p1));
        let p2 = Node::branch(NodeType::Paragraph, Fragment::from_node(Node::new_text("Second item")));
        let li2 = Node::branch(NodeType::ListItem, Fragment::from_node(p2));
        let ol = Node::branch(NodeType::OrderedList, Fragment::from_vec(vec![li1, li2]));
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(ol));

        // Positions: <ol>0 <li1>1 <p>2 F3..m12 </p>13 </li1>14 <li2>15 <p>16 S17..m27 </p>28 </li2>29 </ol>30
        // Cursor at start of second LI's paragraph content = 17
        let mut state = DocState::from_doc(doc);
        state.set_selection(Selection::cursor(17));

        let before_li = state.doc().child(0).child_count();
        assert_eq!(before_li, 2);

        state.backspace();

        // After merge: OL should have 1 LI with combined text
        let ol_node = state.doc().child(0);
        assert_eq!(ol_node.node_type, NodeType::OrderedList);
        assert_eq!(ol_node.child_count(), 1,
            "Backspace at start of second OL item should merge into one LI");
        assert_eq!(ol_node.text_content(), "First itemSecond item");
    }

    #[test]
    fn backspace_merges_li_when_previous_item_has_nested_list() {
        // Previous LI contains a paragraph AND a nested list.
        // <ol>
        //   <li><p>Top</p><ul><li><p>Nested</p></li></ul></li>
        //   <li><p>Bottom</p></li>
        // </ol>
        // The hardcoded -2 would corrupt the tree here because the previous
        // LI's last child is a <ul>, not a <p>.
        let nested_p = Node::branch(NodeType::Paragraph, Fragment::from_node(Node::new_text("Nested")));
        let nested_li = Node::branch(NodeType::ListItem, Fragment::from_node(nested_p));
        let nested_ul = Node::branch(NodeType::BulletList, Fragment::from_node(nested_li));

        let top_p = Node::branch(NodeType::Paragraph, Fragment::from_node(Node::new_text("Top")));
        let li1 = Node::branch(NodeType::ListItem, Fragment::from_vec(vec![top_p, nested_ul]));

        let bottom_p = Node::branch(NodeType::Paragraph, Fragment::from_node(Node::new_text("Bottom")));
        let li2 = Node::branch(NodeType::ListItem, Fragment::from_node(bottom_p));

        let ol = Node::branch(NodeType::OrderedList, Fragment::from_vec(vec![li1, li2]));
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(ol));

        // Structure positions:
        // <ol>0 <li1>1 <p>2 Top3..5 </p>6 <ul>7 <li>8 <p>9 Nested10..15 </p>16 </li>17 </ul>18 </li1>19
        //   <li2>20 <p>21 Bottom22..27 </p>28 </li2>29 </ol>30
        // Cursor at start of "Bottom" = 22
        let mut state = DocState::from_doc(doc);
        state.set_selection(Selection::cursor(22));

        state.backspace();

        // After merge: "Nested" and "Bottom" should be combined in the nested
        // list's last paragraph. The OL should now have 1 LI.
        let ol_node = state.doc().child(0);
        assert_eq!(ol_node.node_type, NodeType::OrderedList);
        assert_eq!(ol_node.child_count(), 1,
            "Backspace should merge second LI into first");
        let text = ol_node.text_content();
        assert!(text.contains("Top"), "Top should be preserved, got: {text}");
        assert!(text.contains("Nested"), "Nested should be preserved, got: {text}");
        assert!(text.contains("Bottom"), "Bottom should be preserved, got: {text}");
    }

    #[test]
    fn backspace_at_start_of_code_block_converts_to_paragraph() {
        // <doc><p>Hello</p><code_block>let x = 1;</code_block></doc>
        let p = Node::branch(NodeType::Paragraph, Fragment::from_node(Node::new_text("Hello")));
        let cb = Node::branch(NodeType::CodeBlock, Fragment::from_node(Node::new_text("let x = 1;")));
        let doc = Node::branch(NodeType::Doc, Fragment::from_vec(vec![p, cb]));

        // Positions: <p>0 H1..o5 </p>6 <cb>7 l8..;18 </cb>19
        // Cursor at start of code block content = 8
        let mut state = DocState::from_doc(doc);
        state.set_selection(Selection::cursor(8));

        state.backspace();

        // After: code block should be converted to a paragraph
        let second_block = state.doc().child(1);
        assert_eq!(second_block.node_type, NodeType::Paragraph,
            "Backspace at start of code block should convert to paragraph, got {:?}",
            second_block.node_type);
        assert_eq!(second_block.text_content(), "let x = 1;");
    }
}

mod toolbar_cursor_tests {
    use crate::doc_state::*;
    use crate::attrs::{AttrValue, Attrs};
    use crate::fragment::Fragment;
    use crate::node::Node;
    use crate::node_type::NodeType;

    fn heading_attrs(level: i64) -> Attrs {
        Attrs::from(vec![("level".to_string(), AttrValue::Int(level))])
    }

    #[test]
    fn set_block_type_preserves_cursor_position() {
        // <doc><h1 level=1>Dashboard Documentation</h1><p>Some text</p></doc>
        let h1 = Node::branch_with_attrs(
            NodeType::Heading,
            heading_attrs(1),
            Fragment::from_node(Node::new_text("Dashboard Documentation")),
        );
        let p = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Some text")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_vec(vec![h1, p]));
        let mut state = DocState::from_doc(doc);

        // Place cursor mid-heading (position 10)
        state.set_selection(Selection::cursor(10));
        eprintln!("Before: selection={:?}", state.selection());

        // Change H1 to H2
        state.set_block_type(NodeType::Heading, heading_attrs(2));

        eprintln!("After: selection={:?}", state.selection());
        eprintln!("Doc child 0 type: {:?}", state.doc().child(0).node_type);

        // Cursor should still be around position 10, not at end of doc
        let sel = state.selection();
        assert!(sel.head < 25, "cursor should stay in heading area, not jump to {} (doc size={})",
            sel.head, state.doc().content.size());
    }
}

mod clipboard_tests {
    use crate::doc_state::*;
    use crate::parse::parse_markdown;

    // ── selected_markdown ─────────────────────────────────────────────

    #[test]
    fn selected_markdown_cursor_returns_empty() {
        let mut state = DocState::from_markdown("Hello world");
        state.set_selection(Selection::cursor(3));
        assert_eq!(state.selected_markdown(), "");
    }

    #[test]
    fn selected_markdown_returns_plain_text() {
        let mut state = DocState::from_markdown("Hello world");
        // Select "ello" (positions 2..6 in <doc><p>Hello world</p></doc>)
        // Pos 1 = H, pos 2 = e, pos 3 = l, pos 4 = l, pos 5 = o
        state.set_selection(Selection::range(2, 6));
        let md = state.selected_markdown();
        assert!(
            md.contains("ello"),
            "expected 'ello' in selected markdown, got: {md:?}"
        );
    }

    #[test]
    fn selected_markdown_preserves_heading() {
        let mut state = DocState::from_markdown("# Hello\n\nWorld");
        // Select the entire heading: <doc><h1>Hello</h1><p>World</p></doc>
        // h1 opens at 0, content 1..6, closes at 6
        // p opens at 7, content 8..13, closes at 13
        // Select range 1..6 = "Hello" inside heading
        state.set_selection(Selection::range(0, 7));
        let md = state.selected_markdown();
        assert!(
            md.contains("# Hello") || md.contains("#"),
            "expected heading syntax in selected markdown, got: {md:?}"
        );
    }

    #[test]
    fn selected_markdown_across_paragraphs() {
        let mut state = DocState::from_markdown("Hello\n\nWorld");
        // Select everything
        let doc_size = state.doc().content.size();
        state.set_selection(Selection::range(0, doc_size));
        let md = state.selected_markdown();
        assert!(md.contains("Hello"), "got: {md:?}");
        assert!(md.contains("World"), "got: {md:?}");
    }

    #[test]
    fn selected_markdown_mid_paragraph_through_heading() {
        // Selecting from mid-paragraph through a heading should produce
        // valid markdown, not garbled content with stray marks.
        let mut state = DocState::from_markdown("Hello world\n\n## Title\n\nAfter");
        // <doc><p>Hello world</p><h2>Title</h2><p>After</p></doc>
        // p1 opens at 0, content 1..12, closes at 12
        // h2 opens at 13, content 14..19, closes at 19
        // Select from pos 6 ("world") through end of heading (pos 19)
        state.set_selection(Selection::range(6, 20));
        let md = state.selected_markdown();
        // Should contain the heading marker and not have garbled inline marks
        assert!(
            md.contains("Title"),
            "heading content should be in selection, got: {md:?}"
        );
        // The partial first paragraph should not produce stray marks
        assert!(
            !md.contains("*") && !md.contains("_"),
            "should not have stray inline marks, got: {md:?}"
        );
    }

    #[test]
    fn selected_markdown_with_inline_marks_mid_block() {
        // Select across bold text and a heading — the roundtrip through
        // markdown should not garble content.
        let mut state = DocState::from_markdown("Hello **bold** text\n\n## Heading");
        let doc_size = state.doc().content.size();
        // Select from position 8 (inside "bold") through end of heading
        state.set_selection(Selection::range(8, doc_size));
        let md = state.selected_markdown();
        // Roundtrip: parse back and check structure
        let roundtrip_doc = parse_markdown(&md);
        let roundtrip_text = roundtrip_doc.text_content();
        assert!(
            roundtrip_text.contains("Heading"),
            "heading should survive roundtrip, got: {roundtrip_text:?} from md: {md:?}"
        );
    }

    // ── insert_from_markdown ──────────────────────────────────────────

    #[test]
    fn insert_from_markdown_preserves_heading() {
        let mut state = DocState::from_markdown("Before\n\nAfter");
        // Place cursor at end of "Before" (pos 7 = end of first paragraph content)
        // <doc><p>Before</p><p>After</p></doc>
        // p1 opens at 0, "Before" at 1..7, closes at 7
        state.set_selection(Selection::cursor(7));
        state.insert_from_markdown("\n\n## Heading\n\n");
        let md = state.to_markdown();
        assert!(
            md.contains("## Heading"),
            "heading should be preserved in: {md:?}"
        );
        assert!(md.contains("Before"), "original content should remain: {md:?}");
        assert!(md.contains("After"), "original content should remain: {md:?}");
    }

    #[test]
    fn insert_from_markdown_preserves_list() {
        let mut state = DocState::from_markdown("Hello");
        // Place cursor at end of "Hello"
        state.set_selection(Selection::cursor(6));
        state.insert_from_markdown("\n\n- Item 1\n- Item 2\n\n");
        let md = state.to_markdown();
        assert!(
            md.contains("- Item 1"),
            "bullet list should be preserved in: {md:?}"
        );
        assert!(
            md.contains("- Item 2"),
            "second item should be present in: {md:?}"
        );
    }

    #[test]
    fn insert_from_markdown_empty_is_noop() {
        let mut state = DocState::from_markdown("Hello");
        let before = state.to_markdown();
        state.insert_from_markdown("");
        assert_eq!(state.to_markdown(), before);
    }

    #[test]
    fn insert_from_markdown_external_paste_with_formatting() {
        let mut state = DocState::from_markdown("Existing content");
        state.set_selection(Selection::cursor(17)); // end of "Existing content"
        // Simulates external paste containing markdown syntax
        state.insert_from_markdown("# Title\n\nSome **bold** text\n\n- item 1\n- item 2\n\n```rust\nfn main() {}\n```");
        let md = state.to_markdown();
        assert!(md.contains("# Title"), "heading should render: {md:?}");
        assert!(md.contains("**bold**"), "bold should render: {md:?}");
        assert!(md.contains("- item 1"), "list should render: {md:?}");
        assert!(md.contains("```rust"), "code block should render: {md:?}");
    }

    #[test]
    fn insert_from_markdown_plain_text_without_syntax() {
        let mut state = DocState::from_markdown("Hello");
        state.set_selection(Selection::cursor(6)); // end of "Hello"
        // Plain text with no markdown syntax still works correctly
        state.insert_from_markdown("world");
        let md = state.to_markdown();
        assert!(
            md.contains("Helloworld"),
            "plain text should insert inline: {md:?}"
        );
    }

    // ── insert_text_multiline ─────────────────────────────────────────

    #[test]
    fn insert_text_multiline_single_line() {
        let mut state = DocState::from_markdown("Hello");
        state.set_selection(Selection::cursor(6)); // end of "Hello"
        state.insert_text_multiline(" World");
        let md = state.to_markdown();
        assert!(
            md.contains("Hello World"),
            "single-line paste should insert inline: {md:?}"
        );
    }

    #[test]
    fn insert_text_multiline_creates_new_paragraphs() {
        let mut state = DocState::from_markdown("Start");
        state.set_selection(Selection::cursor(6)); // end of "Start"
        state.insert_text_multiline("\nLine 2\nLine 3");
        let md = state.to_markdown();
        // After splitting, each line becomes a new block.
        assert!(md.contains("Start"), "got: {md:?}");
        assert!(md.contains("Line 2"), "got: {md:?}");
        assert!(md.contains("Line 3"), "got: {md:?}");
    }

    #[test]
    fn insert_text_multiline_in_code_block_preserves_newlines() {
        let mut state = DocState::from_markdown("```\nhello\n```");
        // Place cursor inside code block content.
        // <doc><codeblock>hello</codeblock></doc>
        // codeblock opens at 0, "hello" at 1..6, closes at 6
        state.set_selection(Selection::cursor(6)); // end of "hello"
        state.insert_text_multiline("\nworld\nfoo");
        let md = state.to_markdown();
        // In a code block, newlines are literal — no block splitting.
        assert!(
            md.contains("hello\nworld\nfoo"),
            "code block should preserve literal newlines: {md:?}"
        );
    }

    #[test]
    fn split_block_in_code_block_inserts_newline_and_advances_cursor() {
        // Parser includes trailing \n, so content is "hello\n" (6 chars).
        let mut state = DocState::from_markdown("```\nhello\n```");
        let initial_content = state.doc.child(0).text_content();

        // Cursor at position 6 = at the trailing \n (after "hello").
        state.set_selection(Selection::cursor(6));
        state.split_block();

        let cb = state.doc.child(0);
        assert_eq!(cb.node_type, NodeType::CodeBlock);
        // A new \n is inserted before the existing trailing \n.
        assert_eq!(
            cb.text_content(),
            format!("{}{}", &initial_content[..initial_content.len() - 1], "\n\n"),
        );

        // Cursor must advance past the inserted newline.
        assert_eq!(
            state.selection().head, 7,
            "cursor should advance past the newline to the new line"
        );
    }

    #[test]
    fn insert_text_multiline_in_list_item() {
        let mut state = DocState::from_markdown("- Item 1");
        // Place cursor at end of "Item 1"
        // <doc><bulletlist><listitem><p>Item 1</p></listitem></bulletlist></doc>
        // bulletlist opens at 0, listitem opens at 1, p opens at 2,
        // "Item 1" at 3..9, p closes at 9, listitem closes at 10, bulletlist closes at 11
        state.set_selection(Selection::cursor(9));
        state.insert_text_multiline("\nItem 2");
        let md = state.to_markdown();
        assert!(md.contains("Item 1"), "got: {md:?}");
        assert!(md.contains("Item 2"), "got: {md:?}");
    }

    #[test]
    fn text_between_ol_items_has_newlines() {
        let state = DocState::from_markdown("1. First\n2. Second\n3. Third");
        // Select all OL content
        // <doc><ol><li><p>First</p></li><li><p>Second</p></li><li><p>Third</p></li></ol></doc>
        // ol:0 li:1 p:2 "First":3..8 p:8 li:9 li:10 p:11 "Second":12..18 p:18 li:19 li:20 p:21 "Third":22..27 p:27 li:28 ol:29
        let text = state.text_between(3, 27);
        eprintln!("text_between for OL items: {:?}", text);
        assert!(text.contains('\n'), "should have newlines between items");
        let lines: Vec<&str> = text.split('\n').collect();
        eprintln!("lines: {:?}", lines);
        assert!(
            lines.len() >= 3,
            "should have at least 3 lines, got {} from {:?}",
            lines.len(),
            text
        );
    }

    #[test]
    fn insert_from_markdown_ol_items() {
        let mut state = DocState::from_markdown("1. First\n2. Second\n3. Third");
        // Navigate to end of "Third" and create a new list item
        state.set_selection(Selection::cursor(27)); // end of "Third"
        state.split_block();
        // Now insert markdown that represents OL items
        state.insert_from_markdown("1. Line A\n2. Line B\n3. Line C");
        let md = state.to_markdown();
        eprintln!("after insert_from_markdown: {md:?}");
        assert!(md.contains("Line A"), "got: {md:?}");
        assert!(md.contains("Line B"), "got: {md:?}");
        assert!(md.contains("Line C"), "got: {md:?}");
    }

    #[test]
    fn insert_text_multiline_in_ol_creates_separate_items() {
        let mut state = DocState::from_markdown("1. First\n2. Second\n3. Third");
        // Move to end of "Third" and press Enter (split_block) to create item 4
        let doc_size = state.doc().content.size();
        // Find end of "Third" content. OL structure:
        // <doc><ol><li><p>First</p></li><li><p>Second</p></li><li><p>Third</p></li></ol></doc>
        // We want end of "Third" content
        let md_before = state.to_markdown();
        eprintln!("before: {md_before:?}, doc_size={doc_size}");

        // Navigate to end of Third: ol opens at 0, li1 opens 1, p opens 2,
        // "First" 3..8, p closes 8, li1 closes 9, li2 opens 10, p opens 11,
        // "Second" 12..18, p closes 18, li2 closes 19, li3 opens 20, p opens 21,
        // "Third" 22..27, p closes 27, li3 closes 28, ol closes 29
        state.set_selection(Selection::cursor(27)); // end of "Third"
        state.split_block(); // creates new empty list item

        // Now paste multiline text into the new (empty) list item
        state.insert_text_multiline("Line A\nLine B\nLine C");
        let md = state.to_markdown();
        eprintln!("after: {md:?}");

        // Should have separate items for Line A, B, C
        assert!(md.contains("Line A"), "got: {md:?}");
        assert!(md.contains("Line B"), "got: {md:?}");
        assert!(md.contains("Line C"), "got: {md:?}");
        // The key assertion: Line B and C should be on separate lines
        // (as separate list items), not all merged into one
        assert!(
            md.contains("Line A\n") || md.contains("Line A\r"),
            "Line A should be followed by a newline in: {md:?}"
        );
    }
}

mod paste_replace_test {
    use crate::doc_state::*;
    use crate::node::Node;
    use crate::node_type::NodeType;
    use crate::fragment::Fragment;

    #[test]
    fn paste_replaces_selection_in_heading() {
        // <doc><h1>Dashboard Documentation</h1></doc>
        let h1 = Node::branch(
            NodeType::Heading,
            Fragment::from_node(Node::new_text("Dashboard Documentation")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(h1));
        let mut state = DocState::from_doc(doc);

        // Select " Documentation" (positions 10..24 — space + 13 chars)
        // H1 content: D(1) a(2) s(3) h(4) b(5) o(6) a(7) r(8) d(9) (10) D(11)...n(23)
        state.set_selection(Selection::range(10, 24));
        eprintln!("Before: h1={}", state.doc().child(0).text_content());

        state.insert_from_markdown("Dashboard");
        eprintln!("After: h1={}", state.doc().child(0).text_content());

        let h1_text = state.doc().child(0).text_content();
        assert!(h1_text.contains("Dashboard"), "Should contain Dashboard");
        // Should have "Dashboard" twice (or at least "DashboardDashboard")
        assert!(h1_text.len() > 9, "Should be longer than just 'Dashboard', got: {}", h1_text);
    }
}

mod atom_tests {
    use std::collections::HashSet;

    use crate::attrs::code_block_attrs;
    use crate::doc_state::*;
    use crate::fragment::Fragment;
    use crate::node::Node;
    use crate::node_type::NodeType;

    /// Helper: create a set of atomic languages from string slices.
    fn atomic_set(langs: &[&str]) -> HashSet<String> {
        langs.iter().map(|s| s.to_string()).collect()
    }

    // ── is_atom defaults ──────────────────────────────────────────────

    #[test]
    fn node_is_not_atom_by_default() {
        let text = Node::new_text("hello");
        assert!(!text.is_atom());

        let p = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("hello")),
        );
        assert!(!p.is_atom());

        let cb = Node::branch_with_attrs(
            NodeType::CodeBlock,
            code_block_attrs("rust"),
            Fragment::from_node(Node::new_text("let x = 1;")),
        );
        assert!(!cb.is_atom());
    }

    // ── Matching language marks CodeBlock as atomic ────────────────────

    #[test]
    fn code_block_with_matching_language_is_atomic() {
        let md = "```chartml\ntype: bar\n```";
        let state = DocState::from_markdown_with_atoms(md, atomic_set(&["chartml"]));

        let cb = state.doc().child(0);
        assert_eq!(cb.node_type, NodeType::CodeBlock);
        assert!(cb.is_atom(), "CodeBlock with matching language should be atomic");
    }

    #[test]
    fn code_block_with_non_matching_language_is_not_atomic() {
        let md = "```rust\nlet x = 1;\n```";
        let state = DocState::from_markdown_with_atoms(md, atomic_set(&["chartml"]));

        let cb = state.doc().child(0);
        assert_eq!(cb.node_type, NodeType::CodeBlock);
        assert!(!cb.is_atom(), "CodeBlock with non-matching language should not be atomic");
    }

    #[test]
    fn code_block_without_language_is_not_atomic() {
        let md = "```\nsome content\n```";
        let state = DocState::from_markdown_with_atoms(md, atomic_set(&["chartml"]));

        let cb = state.doc().child(0);
        assert_eq!(cb.node_type, NodeType::CodeBlock);
        assert!(!cb.is_atom(), "CodeBlock without language should not be atomic");
    }

    // ── Multiple atomic languages ────────────────────────────────────

    #[test]
    fn multiple_atomic_languages_all_marked() {
        let md = "```chartml\ntype: bar\n```\n\n```mermaid\ngraph TD\n```\n\n```rust\nfn main() {}\n```";
        let state = DocState::from_markdown_with_atoms(md, atomic_set(&["chartml", "mermaid"]));

        let chartml_block = state.doc().child(0);
        assert_eq!(chartml_block.node_type, NodeType::CodeBlock);
        assert!(chartml_block.is_atom(), "chartml block should be atomic");

        let mermaid_block = state.doc().child(1);
        assert_eq!(mermaid_block.node_type, NodeType::CodeBlock);
        assert!(mermaid_block.is_atom(), "mermaid block should be atomic");

        let rust_block = state.doc().child(2);
        assert_eq!(rust_block.node_type, NodeType::CodeBlock);
        assert!(!rust_block.is_atom(), "rust block should not be atomic");
    }

    // ── Non-CodeBlock nodes are never atomic ─────────────────────────

    #[test]
    fn paragraph_is_never_atomic() {
        let md = "Hello world";
        let state = DocState::from_markdown_with_atoms(md, atomic_set(&["chartml"]));

        let p = state.doc().child(0);
        assert_eq!(p.node_type, NodeType::Paragraph);
        assert!(!p.is_atom(), "Paragraphs should never be atomic");
    }

    #[test]
    fn heading_is_never_atomic() {
        let h = Node::branch_with_attrs(
            NodeType::Heading,
            crate::attrs::heading_attrs(1),
            Fragment::from_node(Node::new_text("Title")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(h));
        let state = DocState::from_doc_with_atoms(doc, atomic_set(&["chartml"]));

        let heading = state.doc().child(0);
        assert_eq!(heading.node_type, NodeType::Heading);
        assert!(!heading.is_atom(), "Headings should never be atomic");
    }

    // ── set_from_markdown re-applies atom marking ────────────────────

    #[test]
    fn set_from_markdown_re_marks_atoms() {
        let md1 = "Hello world";
        let mut state = DocState::from_markdown_with_atoms(md1, atomic_set(&["chartml"]));

        // Initially no CodeBlock, nothing atomic.
        assert!(!state.doc().child(0).is_atom());

        // Replace with markdown that has a matching code block.
        let md2 = "```chartml\ntype: line\n```";
        state.set_from_markdown(md2);

        let cb = state.doc().child(0);
        assert_eq!(cb.node_type, NodeType::CodeBlock);
        assert!(cb.is_atom(), "set_from_markdown should re-mark atoms");
    }

    #[test]
    fn set_from_markdown_clears_atoms_when_language_changes() {
        let md1 = "```chartml\ntype: bar\n```";
        let mut state = DocState::from_markdown_with_atoms(md1, atomic_set(&["chartml"]));
        assert!(state.doc().child(0).is_atom());

        // Replace with a non-matching code block.
        let md2 = "```rust\nlet x = 1;\n```";
        state.set_from_markdown(md2);

        let cb = state.doc().child(0);
        assert_eq!(cb.node_type, NodeType::CodeBlock);
        assert!(!cb.is_atom(), "atom flag should be cleared for non-matching language");
    }

    // ── Empty atomic_languages set marks nothing ─────────────────────

    #[test]
    fn empty_atomic_languages_marks_nothing() {
        let md = "```chartml\ntype: bar\n```";
        let state = DocState::from_markdown_with_atoms(md, HashSet::new());

        let cb = state.doc().child(0);
        assert!(!cb.is_atom(), "empty atomic_languages should mark nothing");
    }

    // ── from_markdown (no atoms) still works ─────────────────────────

    #[test]
    fn from_markdown_without_atoms_never_marks_atomic() {
        let md = "```chartml\ntype: bar\n```";
        let state = DocState::from_markdown(md);

        let cb = state.doc().child(0);
        assert!(!cb.is_atom(), "from_markdown should never mark anything atomic");
    }

    // ── atom field does not affect PartialEq ─────────────────────────

    #[test]
    fn atom_flag_does_not_affect_equality() {
        let a = Node::branch_with_attrs(
            NodeType::CodeBlock,
            code_block_attrs("chartml"),
            Fragment::from_node(Node::new_text("type: bar")),
        );
        let mut b = Node::branch_with_attrs(
            NodeType::CodeBlock,
            code_block_attrs("chartml"),
            Fragment::from_node(Node::new_text("type: bar")),
        );
        b.atom = true;

        assert_eq!(a, b, "atom flag should not affect structural equality");
    }

    // ── node_size is unchanged by atom flag ──────────────────────────

    #[test]
    fn atom_flag_does_not_change_node_size() {
        let mut cb = Node::branch_with_attrs(
            NodeType::CodeBlock,
            code_block_attrs("chartml"),
            Fragment::from_node(Node::new_text("type: bar")),
        );
        let size_before = cb.node_size();
        cb.atom = true;
        let size_after = cb.node_size();

        assert_eq!(size_before, size_after, "atom flag should not change node_size");
    }

    // ── Gap cursor: adjust_into_textblock with atomic blocks ─────────

    /// Helper: build a doc with two atomic code blocks.
    ///
    /// <doc><codeblock[chartml]>chart1</codeblock><codeblock[mermaid]>graph</codeblock></doc>
    ///
    /// Positions:
    ///   0: before first codeblock
    ///   1: inside first codeblock content start
    ///   7: inside first codeblock content end
    ///   8: between the two codeblocks (gap)
    ///   9: inside second codeblock content start
    ///  14: inside second codeblock content end
    ///  15: after second codeblock (end of doc)
    fn two_atomic_blocks_doc() -> (Node, HashSet<String>) {
        let cb1 = Node::branch_with_attrs(
            NodeType::CodeBlock,
            code_block_attrs("chartml"),
            Fragment::from_node(Node::new_text("chart1")),
        );
        let cb2 = Node::branch_with_attrs(
            NodeType::CodeBlock,
            code_block_attrs("mermaid"),
            Fragment::from_node(Node::new_text("graph")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_vec(vec![cb1, cb2]));
        (doc, atomic_set(&["chartml", "mermaid"]))
    }

    /// Helper: build a doc with an atomic block followed by a normal paragraph.
    ///
    /// <doc><codeblock[chartml]>chart</codeblock><p>Hello</p></doc>
    ///
    /// Positions:
    ///   0: before codeblock
    ///   1..6: inside codeblock content ("chart")
    ///   7: between codeblock and paragraph (gap)
    ///   8: inside paragraph content start
    ///  13: inside paragraph content end
    ///  14: after paragraph
    fn atomic_then_paragraph_doc() -> (Node, HashSet<String>) {
        let cb = Node::branch_with_attrs(
            NodeType::CodeBlock,
            code_block_attrs("chartml"),
            Fragment::from_node(Node::new_text("chart")),
        );
        let p = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Hello")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_vec(vec![cb, p]));
        (doc, atomic_set(&["chartml"]))
    }

    /// Helper: build a doc with a normal paragraph followed by an atomic block.
    ///
    /// <doc><p>Hello</p><codeblock[chartml]>chart</codeblock></doc>
    fn paragraph_then_atomic_doc() -> (Node, HashSet<String>) {
        let p = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Hello")),
        );
        let cb = Node::branch_with_attrs(
            NodeType::CodeBlock,
            code_block_attrs("chartml"),
            Fragment::from_node(Node::new_text("chart")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_vec(vec![p, cb]));
        (doc, atomic_set(&["chartml"]))
    }

    #[test]
    fn gap_cursor_between_two_atomic_blocks_stays_at_gap() {
        let (doc, atoms) = two_atomic_blocks_doc();
        let state = DocState::from_doc_with_atoms(doc, atoms);
        // Position 8 is between the two atomic code blocks.
        let adjusted = state.adjust_into_textblock(8);
        assert_eq!(adjusted, 8, "gap position between two atomic blocks should stay at gap");
    }

    #[test]
    fn gap_cursor_before_first_atomic_block_stays_at_gap() {
        let (doc, atoms) = two_atomic_blocks_doc();
        let state = DocState::from_doc_with_atoms(doc, atoms);
        // Position 0 is before the first atomic code block (doc boundary).
        let adjusted = state.adjust_into_textblock(0);
        assert_eq!(adjusted, 0, "gap position at doc start before atomic block should stay");
    }

    #[test]
    fn gap_cursor_after_last_atomic_block_stays_at_gap() {
        let (doc, atoms) = two_atomic_blocks_doc();
        let state = DocState::from_doc_with_atoms(doc, atoms);
        // Position 15 is after the second atomic code block (doc end).
        let doc_size = state.doc().content.size();
        let adjusted = state.adjust_into_textblock(doc_size);
        assert_eq!(adjusted, doc_size, "gap position at doc end after atomic block should stay");
    }

    #[test]
    fn gap_cursor_atomic_before_paragraph_adjusts_into_paragraph() {
        let (doc, atoms) = atomic_then_paragraph_doc();
        let state = DocState::from_doc_with_atoms(doc, atoms);
        // Position 7 is between the atomic codeblock and the paragraph.
        // node_before is atomic, node_after is non-atomic paragraph.
        // Should adjust into the paragraph (pos + 1 = 8).
        let adjusted = state.adjust_into_textblock(7);
        assert_eq!(adjusted, 8, "should adjust into the non-atomic paragraph");
    }

    #[test]
    fn gap_cursor_paragraph_before_atomic_adjusts_into_paragraph() {
        let (doc, atoms) = paragraph_then_atomic_doc();
        let state = DocState::from_doc_with_atoms(doc, atoms);
        // Position 7 is between the paragraph and the atomic codeblock.
        // node_before is non-atomic paragraph, node_after is atomic.
        // Should adjust into the paragraph (pos - 1 = 6).
        let adjusted = state.adjust_into_textblock(7);
        assert_eq!(adjusted, 6, "should adjust into the non-atomic paragraph");
    }

    #[test]
    fn gap_cursor_at_doc_start_before_atomic_stays() {
        let (doc, atoms) = atomic_then_paragraph_doc();
        let state = DocState::from_doc_with_atoms(doc, atoms);
        // Position 0 is before the atomic codeblock. node_before is None,
        // node_after is atomic. Should stay at 0 (gap cursor at doc boundary).
        let adjusted = state.adjust_into_textblock(0);
        assert_eq!(adjusted, 0, "gap at doc start before atomic block should stay");
    }

    #[test]
    fn gap_cursor_at_doc_end_after_atomic_stays() {
        let (doc, atoms) = paragraph_then_atomic_doc();
        let state = DocState::from_doc_with_atoms(doc, atoms);
        let doc_size = state.doc().content.size();
        // Last position is after the atomic codeblock. node_before is
        // atomic, node_after is None. Should stay (gap cursor at doc end).
        let adjusted = state.adjust_into_textblock(doc_size);
        assert_eq!(adjusted, doc_size, "gap at doc end after atomic block should stay");
    }

    #[test]
    fn adjust_still_works_between_two_non_atomic_textblocks() {
        // Verify that the original behavior is preserved when no atoms are
        // involved: position between two paragraphs adjusts into the first.
        let p1 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Hi")),
        );
        let p2 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("World")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_vec(vec![p1, p2]));
        let state = DocState::from_doc(doc);
        // Position 4 is between the two paragraphs.
        // node_before is p1 (textblock), so adjust to pos - 1 = 3.
        let adjusted = state.adjust_into_textblock(4);
        assert_eq!(adjusted, 3, "should adjust into the first paragraph (end of content)");
    }

    #[test]
    fn adjust_into_textblock_inside_textblock_unchanged() {
        // Position already inside a textblock should be returned as-is.
        let (doc, atoms) = atomic_then_paragraph_doc();
        let state = DocState::from_doc_with_atoms(doc, atoms);
        // Position 10 is inside the paragraph's content.
        let adjusted = state.adjust_into_textblock(10);
        assert_eq!(adjusted, 10, "position inside textblock should be unchanged");
    }

    #[test]
    fn set_selection_respects_atomic_gap() {
        let (doc, atoms) = two_atomic_blocks_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);
        // Setting selection to the gap between two atomic blocks should stay.
        state.set_selection(Selection::cursor(8));
        assert_eq!(state.selection().head, 8, "set_selection should preserve gap cursor");
        assert_eq!(state.selection().anchor, 8);
    }

    // ── Selection expansion around atomic blocks ─────────────────────

    #[test]
    fn expand_selection_partially_inside_atomic_block_expands() {
        // <doc><p>Hello</p><codeblock[chartml]>chart data</codeblock><p>World</p></doc>
        //   p: 0..7 (content 1..6)
        //   cb: 7..19 (content 8..18, "chart data" = 10 chars)
        //   p: 19..26 (content 20..25)
        let p1 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Hello")),
        );
        let cb = Node::branch_with_attrs(
            NodeType::CodeBlock,
            code_block_attrs("chartml"),
            Fragment::from_node(Node::new_text("chart data")),
        );
        let p2 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("World")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_vec(vec![p1, cb, p2]));
        let state = DocState::from_doc_with_atoms(doc, atomic_set(&["chartml"]));

        // Select from inside paragraph into atomic block: from=3, to=10
        // to=10 is inside the atomic code block (content position).
        // Should expand to include the full code block: to -> 19.
        let sel = Selection::range(3, 10);
        let expanded = state.expand_selection_around_atoms(&sel);
        assert_eq!(expanded.from(), 3, "from should not change (inside paragraph)");
        assert_eq!(expanded.to(), 19, "to should expand to end of atomic block");
    }

    #[test]
    fn expand_selection_fully_outside_atomic_block_unchanged() {
        let (doc, atoms) = atomic_then_paragraph_doc();
        let state = DocState::from_doc_with_atoms(doc, atoms);
        // Select within the paragraph only (pos 8..11).
        let sel = Selection::range(8, 11);
        let expanded = state.expand_selection_around_atoms(&sel);
        assert_eq!(expanded, sel, "selection not touching atomic block should be unchanged");
    }

    #[test]
    fn expand_selection_spanning_multiple_atomic_blocks() {
        // <doc><codeblock[chartml]>abc</codeblock><p>mid</p><codeblock[mermaid]>xyz</codeblock></doc>
        //   cb1: 0..5 (content 1..4, "abc")
        //   p:   5..10 (content 6..9, "mid")
        //   cb2: 10..15 (content 11..14, "xyz")
        let cb1 = Node::branch_with_attrs(
            NodeType::CodeBlock,
            code_block_attrs("chartml"),
            Fragment::from_node(Node::new_text("abc")),
        );
        let p = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("mid")),
        );
        let cb2 = Node::branch_with_attrs(
            NodeType::CodeBlock,
            code_block_attrs("mermaid"),
            Fragment::from_node(Node::new_text("xyz")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_vec(vec![cb1, p, cb2]));
        let state = DocState::from_doc_with_atoms(doc, atomic_set(&["chartml", "mermaid"]));

        // Select from inside first atomic block to inside second:
        // from=2 (inside cb1), to=12 (inside cb2)
        let sel = Selection::range(2, 12);
        let expanded = state.expand_selection_around_atoms(&sel);
        assert_eq!(expanded.from(), 0, "from should expand to start of first atomic block");
        assert_eq!(expanded.to(), 15, "to should expand to end of second atomic block");
    }

    #[test]
    fn expand_selection_preserves_direction_forward() {
        let (doc, atoms) = atomic_then_paragraph_doc();
        let state = DocState::from_doc_with_atoms(doc, atoms);
        // Forward selection partially inside atomic block: anchor=3, head=5
        // Both are inside the atomic code block.
        let sel = Selection::range(3, 5);
        let expanded = state.expand_selection_around_atoms(&sel);
        // Anchor should be <= head (forward direction preserved).
        assert!(expanded.anchor <= expanded.head, "forward direction should be preserved");
        assert_eq!(expanded.from(), 0, "from should be start of atomic block");
        assert_eq!(expanded.to(), 7, "to should be end of atomic block");
    }

    #[test]
    fn expand_selection_preserves_direction_backward() {
        let (doc, atoms) = atomic_then_paragraph_doc();
        let state = DocState::from_doc_with_atoms(doc, atoms);
        // Backward selection partially inside atomic block: anchor=5, head=3
        let sel = Selection::range(5, 3);
        let expanded = state.expand_selection_around_atoms(&sel);
        // Anchor should be > head (backward direction preserved).
        assert!(expanded.anchor >= expanded.head, "backward direction should be preserved");
        assert_eq!(expanded.from(), 0);
        assert_eq!(expanded.to(), 7);
    }

    #[test]
    fn expand_selection_non_atomic_code_block_unchanged() {
        // Non-atomic code block should not trigger expansion.
        let cb = Node::branch_with_attrs(
            NodeType::CodeBlock,
            code_block_attrs("rust"),
            Fragment::from_node(Node::new_text("let x = 1;")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(cb));
        let state = DocState::from_doc_with_atoms(doc, atomic_set(&["chartml"]));

        let sel = Selection::range(3, 8);
        let expanded = state.expand_selection_around_atoms(&sel);
        assert_eq!(expanded, sel, "non-atomic code block should not trigger expansion");
    }

    // ── Backspace with atomic blocks ────────────────────────────────

    #[test]
    fn backspace_at_gap_after_atomic_deletes_atomic_block() {
        // <doc><codeblock[chartml]>chart1</codeblock><codeblock[mermaid]>graph</codeblock></doc>
        //   cb1: 0..8, cb2: 8..15
        // Cursor at gap 8 (between the two atomic blocks).
        // Backspace should delete cb1 (node before the cursor).
        let (doc, atoms) = two_atomic_blocks_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);
        state.selection = Selection::cursor(8);
        state.backspace();

        // Only cb2 should remain.
        assert_eq!(state.doc.child_count(), 1);
        assert_eq!(state.doc.child(0).node_type, NodeType::CodeBlock);
        assert_eq!(state.doc.child(0).text_content(), "graph");
    }

    #[test]
    fn backspace_at_start_of_paragraph_after_atomic_deletes_atomic() {
        // <doc><codeblock[chartml]>chart</codeblock><p>Hello</p></doc>
        //   cb: 0..7, p: 7..14
        // Cursor at pos 8 (start of paragraph content).
        // Backspace at start of paragraph — previous block is atomic — delete it.
        let (doc, atoms) = atomic_then_paragraph_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);
        state.selection = Selection::cursor(8);
        state.backspace();

        // Only the paragraph should remain.
        assert_eq!(state.doc.child_count(), 1);
        assert_eq!(state.doc.child(0).node_type, NodeType::Paragraph);
        assert_eq!(state.doc.child(0).text_content(), "Hello");
    }

    #[test]
    fn backspace_at_gap_between_two_atomic_blocks_deletes_one_before() {
        // <doc><codeblock[chartml]>abc</codeblock><codeblock[mermaid]>xyz</codeblock></doc>
        //   cb1: 0..5 (content "abc"), cb2: 5..10 (content "xyz")
        // Cursor at gap 5.
        let cb1 = Node::branch_with_attrs(
            NodeType::CodeBlock,
            code_block_attrs("chartml"),
            Fragment::from_node(Node::new_text("abc")),
        );
        let cb2 = Node::branch_with_attrs(
            NodeType::CodeBlock,
            code_block_attrs("mermaid"),
            Fragment::from_node(Node::new_text("xyz")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_vec(vec![cb1, cb2]));
        let mut state = DocState::from_doc_with_atoms(doc, atomic_set(&["chartml", "mermaid"]));
        state.selection = Selection::cursor(5);
        state.backspace();

        // cb1 deleted, cb2 remains.
        assert_eq!(state.doc.child_count(), 1);
        assert_eq!(state.doc.child(0).text_content(), "xyz");
    }

    #[test]
    fn backspace_inside_non_atomic_code_block_deletes_char() {
        // <doc><codeblock[rust]>let x;</codeblock></doc>
        // Non-atomic code block: backspace inside should delete a character.
        let cb = Node::branch_with_attrs(
            NodeType::CodeBlock,
            code_block_attrs("rust"),
            Fragment::from_node(Node::new_text("let x;")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_node(cb));
        let mut state = DocState::from_doc_with_atoms(doc, atomic_set(&["chartml"]));
        // Cursor at pos 5 (inside "let x;" → after "let ")
        state.selection = Selection::cursor(5);
        state.backspace();

        assert_eq!(state.doc.child(0).text_content(), "letx;");
    }

    // ── Delete forward with atomic blocks ───────────────────────────

    #[test]
    fn delete_at_gap_before_atomic_deletes_atomic_block() {
        // <doc><codeblock[chartml]>chart1</codeblock><codeblock[mermaid]>graph</codeblock></doc>
        //   cb1: 0..8, cb2: 8..15
        // Cursor at gap 8. Delete forward should delete cb2 (node after).
        let (doc, atoms) = two_atomic_blocks_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);
        state.selection = Selection::cursor(8);
        state.delete_forward();

        // Only cb1 should remain.
        assert_eq!(state.doc.child_count(), 1);
        assert_eq!(state.doc.child(0).node_type, NodeType::CodeBlock);
        assert_eq!(state.doc.child(0).text_content(), "chart1");
    }

    #[test]
    fn delete_at_end_of_paragraph_before_atomic_deletes_atomic() {
        // <doc><p>Hello</p><codeblock[chartml]>chart</codeblock></doc>
        //   p: 0..7, cb: 7..14
        // Cursor at pos 6 (end of paragraph content).
        // Delete forward — next block is atomic — delete it.
        let (doc, atoms) = paragraph_then_atomic_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);
        state.selection = Selection::cursor(6);
        state.delete_forward();

        // Only the paragraph should remain.
        assert_eq!(state.doc.child_count(), 1);
        assert_eq!(state.doc.child(0).node_type, NodeType::Paragraph);
        assert_eq!(state.doc.child(0).text_content(), "Hello");
    }

    #[test]
    fn delete_at_gap_between_two_atomic_blocks_deletes_one_after() {
        // <doc><codeblock[chartml]>abc</codeblock><codeblock[mermaid]>xyz</codeblock></doc>
        //   cb1: 0..5, cb2: 5..10
        // Cursor at gap 5. Delete forward should delete cb2.
        let cb1 = Node::branch_with_attrs(
            NodeType::CodeBlock,
            code_block_attrs("chartml"),
            Fragment::from_node(Node::new_text("abc")),
        );
        let cb2 = Node::branch_with_attrs(
            NodeType::CodeBlock,
            code_block_attrs("mermaid"),
            Fragment::from_node(Node::new_text("xyz")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_vec(vec![cb1, cb2]));
        let mut state = DocState::from_doc_with_atoms(doc, atomic_set(&["chartml", "mermaid"]));
        state.selection = Selection::cursor(5);
        state.delete_forward();

        // cb2 deleted, cb1 remains.
        assert_eq!(state.doc.child_count(), 1);
        assert_eq!(state.doc.child(0).text_content(), "abc");
    }

    // ── Insert text at gap positions ────────────────────────────────

    #[test]
    fn insert_text_at_gap_creates_paragraph_with_text() {
        // <doc><codeblock[chartml]>chart1</codeblock><codeblock[mermaid]>graph</codeblock></doc>
        //   cb1: 0..8, cb2: 8..15
        // Cursor at gap 8. Insert text should create a new paragraph.
        let (doc, atoms) = two_atomic_blocks_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);
        state.selection = Selection::cursor(8);
        state.insert_text("Hello");

        // Should now be: cb1, paragraph("Hello"), cb2
        assert_eq!(state.doc.child_count(), 3);
        assert_eq!(state.doc.child(0).node_type, NodeType::CodeBlock);
        assert_eq!(state.doc.child(0).text_content(), "chart1");
        assert_eq!(state.doc.child(1).node_type, NodeType::Paragraph);
        assert_eq!(state.doc.child(1).text_content(), "Hello");
        assert_eq!(state.doc.child(2).node_type, NodeType::CodeBlock);
        assert_eq!(state.doc.child(2).text_content(), "graph");
    }

    #[test]
    fn insert_text_at_gap_between_two_atomic_blocks() {
        // <doc><codeblock[chartml]>abc</codeblock><codeblock[mermaid]>xyz</codeblock></doc>
        //   cb1: 0..5, cb2: 5..10
        // Cursor at gap 5.
        let cb1 = Node::branch_with_attrs(
            NodeType::CodeBlock,
            code_block_attrs("chartml"),
            Fragment::from_node(Node::new_text("abc")),
        );
        let cb2 = Node::branch_with_attrs(
            NodeType::CodeBlock,
            code_block_attrs("mermaid"),
            Fragment::from_node(Node::new_text("xyz")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_vec(vec![cb1, cb2]));
        let mut state = DocState::from_doc_with_atoms(doc, atomic_set(&["chartml", "mermaid"]));
        state.selection = Selection::cursor(5);
        state.insert_text("Hi");

        // Should be: cb1, paragraph("Hi"), cb2
        assert_eq!(state.doc.child_count(), 3);
        assert_eq!(state.doc.child(1).node_type, NodeType::Paragraph);
        assert_eq!(state.doc.child(1).text_content(), "Hi");
    }

    #[test]
    fn insert_text_inside_normal_paragraph_unchanged_behavior() {
        // <doc><codeblock[chartml]>chart</codeblock><p>Hello</p></doc>
        // Cursor at pos 8 (inside paragraph), adjusted by set_selection.
        // Insert text should go into the paragraph normally.
        let (doc, atoms) = atomic_then_paragraph_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);
        state.set_selection(Selection::cursor(10));
        state.insert_text("XY");

        assert_eq!(state.doc.child(1).node_type, NodeType::Paragraph);
        assert_eq!(state.doc.child(1).text_content(), "HeXYllo");
    }

    // ── Split block (Enter) at gap positions ────────────────────────

    #[test]
    fn enter_at_gap_creates_empty_paragraph() {
        // <doc><codeblock[chartml]>chart1</codeblock><codeblock[mermaid]>graph</codeblock></doc>
        //   cb1: 0..8, cb2: 8..15
        // Cursor at gap 8. Enter should insert an empty paragraph.
        let (doc, atoms) = two_atomic_blocks_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);
        state.selection = Selection::cursor(8);
        state.split_block();

        // Should be: cb1, empty paragraph, cb2
        assert_eq!(state.doc.child_count(), 3);
        assert_eq!(state.doc.child(0).node_type, NodeType::CodeBlock);
        assert_eq!(state.doc.child(1).node_type, NodeType::Paragraph);
        assert_eq!(state.doc.child(1).text_content(), "");
        assert_eq!(state.doc.child(2).node_type, NodeType::CodeBlock);
        // Cursor should be inside the new paragraph.
        assert_eq!(state.selection, Selection::cursor(9));
    }

    #[test]
    fn enter_at_gap_between_two_atomic_blocks() {
        // <doc><codeblock[chartml]>abc</codeblock><codeblock[mermaid]>xyz</codeblock></doc>
        //   cb1: 0..5, cb2: 5..10
        // Cursor at gap 5.
        let cb1 = Node::branch_with_attrs(
            NodeType::CodeBlock,
            code_block_attrs("chartml"),
            Fragment::from_node(Node::new_text("abc")),
        );
        let cb2 = Node::branch_with_attrs(
            NodeType::CodeBlock,
            code_block_attrs("mermaid"),
            Fragment::from_node(Node::new_text("xyz")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_vec(vec![cb1, cb2]));
        let mut state = DocState::from_doc_with_atoms(doc, atomic_set(&["chartml", "mermaid"]));
        state.selection = Selection::cursor(5);
        state.split_block();

        // Should be: cb1, empty paragraph, cb2
        assert_eq!(state.doc.child_count(), 3);
        assert_eq!(state.doc.child(1).node_type, NodeType::Paragraph);
        assert_eq!(state.doc.child(1).text_content(), "");
        // Cursor inside the new paragraph.
        assert_eq!(state.selection, Selection::cursor(6));
    }

    #[test]
    fn enter_inside_normal_paragraph_unchanged_behavior() {
        // <doc><codeblock[chartml]>chart</codeblock><p>Hello</p></doc>
        // Cursor at pos 10 (inside "Hello" at "He|llo").
        // Enter should split the paragraph normally.
        let (doc, atoms) = atomic_then_paragraph_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);
        state.set_selection(Selection::cursor(10));
        state.split_block();

        // cb stays, paragraph splits into two.
        assert_eq!(state.doc.child_count(), 3);
        assert_eq!(state.doc.child(0).node_type, NodeType::CodeBlock);
        assert_eq!(state.doc.child(1).node_type, NodeType::Paragraph);
        assert_eq!(state.doc.child(1).text_content(), "He");
        assert_eq!(state.doc.child(2).node_type, NodeType::Paragraph);
        assert_eq!(state.doc.child(2).text_content(), "llo");
    }

    // ── Edge cases ──────────────────────────────────────────────────

    #[test]
    fn backspace_at_doc_start_before_atomic_is_noop() {
        // <doc><codeblock[chartml]>chart</codeblock><p>Hello</p></doc>
        // Cursor at position 0. Backspace should do nothing.
        let (doc, atoms) = atomic_then_paragraph_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);
        state.selection = Selection::cursor(0);
        state.backspace();

        assert_eq!(state.doc.child_count(), 2);
        assert!(state.undo_stack.is_empty());
    }

    #[test]
    fn delete_at_doc_end_after_atomic_is_noop() {
        // <doc><p>Hello</p><codeblock[chartml]>chart</codeblock></doc>
        // Cursor at doc end. Delete should do nothing.
        let (doc, atoms) = paragraph_then_atomic_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);
        let doc_size = state.doc.content.size();
        state.selection = Selection::cursor(doc_size);
        state.delete_forward();

        assert_eq!(state.doc.child_count(), 2);
        assert!(state.undo_stack.is_empty());
    }

    #[test]
    fn backspace_after_atomic_is_undoable() {
        // Verify that deleting an atomic block via backspace can be undone.
        let (doc, atoms) = two_atomic_blocks_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);
        state.selection = Selection::cursor(8);
        state.backspace();

        assert_eq!(state.doc.child_count(), 1);

        // Undo should restore both blocks.
        assert!(state.undo());
        assert_eq!(state.doc.child_count(), 2);
        assert_eq!(state.doc.child(0).text_content(), "chart1");
        assert_eq!(state.doc.child(1).text_content(), "graph");
    }

    #[test]
    fn delete_forward_of_atomic_is_undoable() {
        // Verify that deleting an atomic block via delete forward can be undone.
        let (doc, atoms) = two_atomic_blocks_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);
        state.selection = Selection::cursor(8);
        state.delete_forward();

        assert_eq!(state.doc.child_count(), 1);

        // Undo should restore both blocks.
        assert!(state.undo());
        assert_eq!(state.doc.child_count(), 2);
    }

    #[test]
    fn insert_text_at_gap_at_doc_start() {
        // <doc><codeblock[chartml]>chart</codeblock><p>Hello</p></doc>
        // Cursor at position 0 (before the atomic block).
        // Insert text should create a paragraph before the atomic block.
        let (doc, atoms) = atomic_then_paragraph_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);
        state.selection = Selection::cursor(0);
        state.insert_text("Hi");

        // Should be: paragraph("Hi"), codeblock, paragraph
        assert_eq!(state.doc.child_count(), 3);
        assert_eq!(state.doc.child(0).node_type, NodeType::Paragraph);
        assert_eq!(state.doc.child(0).text_content(), "Hi");
        assert_eq!(state.doc.child(1).node_type, NodeType::CodeBlock);
    }

    #[test]
    fn enter_at_gap_at_doc_end() {
        // <doc><p>Hello</p><codeblock[chartml]>chart</codeblock></doc>
        // Cursor at doc end (after atomic block).
        // Enter should create an empty paragraph at the end.
        let (doc, atoms) = paragraph_then_atomic_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);
        let doc_size = state.doc.content.size();
        state.selection = Selection::cursor(doc_size);
        state.split_block();

        // Should be: paragraph, codeblock, empty paragraph
        assert_eq!(state.doc.child_count(), 3);
        assert_eq!(state.doc.child(2).node_type, NodeType::Paragraph);
        assert_eq!(state.doc.child(2).text_content(), "");
    }

    // ── Clipboard roundtrip preserves atomicity ──────────────────────

    #[test]
    fn insert_from_markdown_marks_pasted_atomic_block() {
        let mut state = DocState::from_markdown_with_atoms("Hello", atomic_set(&["chartml"]));
        state.selection = Selection::cursor(state.doc.content.size());
        state.insert_from_markdown("```chartml\ntype: line\n```");

        let cb = state.doc.content.iter()
            .find(|n| n.node_type == NodeType::CodeBlock)
            .expect("pasted CodeBlock not found");
        assert!(cb.is_atom(), "pasted chartml block should be marked atomic");
    }

    #[test]
    fn insert_from_markdown_preserves_existing_atomic_blocks() {
        let md = "```chartml\ntype: bar\n```";
        let mut state = DocState::from_markdown_with_atoms(md, atomic_set(&["chartml"]));

        assert!(state.doc.child(0).is_atom(), "initial chartml block must be atomic");

        state.selection = Selection::cursor(state.doc.content.size());
        state.insert_from_markdown("More text");

        assert!(state.doc.child(0).is_atom(), "pre-existing chartml block must stay atomic after paste");
    }
}

mod move_block_tests {
    use crate::attrs::code_block_attrs;
    use crate::doc_state::*;
    use crate::fragment::Fragment;
    use crate::node::Node;
    use crate::node_type::NodeType;

    /// Build: <doc><p>Hello</p><p>World</p></doc>
    ///
    /// Positions:
    ///   p1: 0..7  (open=0, H=1..5, o=6, close=7 — wait, 5 chars)
    ///   Actually: <p>=0, H=1, e=2, l=3, l=4, o=5, </p>=6
    ///   p1 node_size = 1 + 5 + 1 = 7, so p1 occupies 0..7
    ///   p2: 7..14 (<p>=7, W=8, o=9, r=10, l=11, d=12, </p>=13)
    ///   p2 node_size = 1 + 5 + 1 = 7, so p2 occupies 7..14
    fn two_para_doc() -> Node {
        let p1 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Hello")),
        );
        let p2 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("World")),
        );
        Node::branch(NodeType::Doc, Fragment::from_vec(vec![p1, p2]))
    }

    /// Build: <doc><p>Alpha</p><p>Bravo</p><p>Charlie</p></doc>
    fn three_para_doc() -> Node {
        let p1 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Alpha")),
        );
        let p2 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Bravo")),
        );
        let p3 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Charlie")),
        );
        Node::branch(NodeType::Doc, Fragment::from_vec(vec![p1, p2, p3]))
    }

    #[test]
    fn move_block_forward() {
        // Move first paragraph past the second.
        // Before: <doc><p>Hello</p><p>World</p></doc>
        // p1: 0..7, p2: 7..14
        // Move p1 (0..7) to target_pos=14 (end of doc).
        // After: <doc><p>World</p><p>Hello</p></doc>
        let mut state = DocState::from_doc(two_para_doc());
        state.move_block(0, 7, 14);

        assert_eq!(state.doc.child_count(), 2);
        assert_eq!(state.doc.child(0).text_content(), "World");
        assert_eq!(state.doc.child(1).text_content(), "Hello");
    }

    #[test]
    fn move_block_backward() {
        // Move second paragraph before the first.
        // Before: <doc><p>Hello</p><p>World</p></doc>
        // p1: 0..7, p2: 7..14
        // Move p2 (7..14) to target_pos=0 (start of doc).
        // After: <doc><p>World</p><p>Hello</p></doc>
        let mut state = DocState::from_doc(two_para_doc());
        state.move_block(7, 14, 0);

        assert_eq!(state.doc.child_count(), 2);
        assert_eq!(state.doc.child(0).text_content(), "World");
        assert_eq!(state.doc.child(1).text_content(), "Hello");
    }

    #[test]
    fn move_block_noop_target_within_range() {
        // Target inside the block range should be a no-op.
        let mut state = DocState::from_doc(two_para_doc());
        let doc_before = state.doc.clone();
        let sel_before = state.selection.clone();

        state.move_block(0, 7, 3);

        assert_eq!(state.doc, doc_before);
        assert_eq!(state.selection, sel_before);
        assert!(state.undo_stack.is_empty(), "no-op should not push undo");
    }

    #[test]
    fn move_block_noop_target_at_block_start() {
        // target_pos == block_start is within [block_start, block_end].
        let mut state = DocState::from_doc(two_para_doc());
        let doc_before = state.doc.clone();

        state.move_block(0, 7, 0);

        assert_eq!(state.doc, doc_before);
        assert!(state.undo_stack.is_empty(), "no-op should not push undo");
    }

    #[test]
    fn move_block_noop_target_at_block_end() {
        // target_pos == block_end maps back to block_start after deletion,
        // producing an identical document — must be treated as no-op.
        let mut state = DocState::from_doc(two_para_doc());
        let doc_before = state.doc.clone();

        state.move_block(0, 7, 7);

        assert_eq!(state.doc, doc_before);
        assert!(state.undo_stack.is_empty(), "no-op should not push undo");
    }

    #[test]
    fn move_atomic_block() {
        // Move an atomic code block forward past a paragraph.
        // <doc><codeblock[chartml]>chart</codeblock><p>Hello</p></doc>
        //   cb: 0..7 (1+5+1), p: 7..14 (1+5+1)
        let mut cb = Node::branch_with_attrs(
            NodeType::CodeBlock,
            code_block_attrs("chartml"),
            Fragment::from_node(Node::new_text("chart")),
        );
        cb.atom = true;
        let p = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("Hello")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_vec(vec![cb, p]));
        let mut state = DocState::from_doc(doc);

        // Move the atomic block (0..7) to after the paragraph (target=14).
        state.move_block(0, 7, 14);

        assert_eq!(state.doc.child_count(), 2);
        assert_eq!(state.doc.child(0).node_type, NodeType::Paragraph);
        assert_eq!(state.doc.child(0).text_content(), "Hello");
        assert_eq!(state.doc.child(1).node_type, NodeType::CodeBlock);
        assert_eq!(state.doc.child(1).text_content(), "chart");
        assert!(state.doc.child(1).is_atom(), "atom flag must be preserved after move");
    }

    #[test]
    fn move_block_undo_reverses() {
        // Verify that undo restores the original document.
        let original_doc = two_para_doc();
        let mut state = DocState::from_doc(original_doc.clone());
        let sel_before = state.selection.clone();

        state.move_block(0, 7, 14);

        // Verify the move happened.
        assert_eq!(state.doc.child(0).text_content(), "World");
        assert_eq!(state.doc.child(1).text_content(), "Hello");

        // Undo.
        assert!(state.undo());

        assert_eq!(state.doc, original_doc);
        assert_eq!(state.selection, sel_before);
    }

    #[test]
    fn move_block_markdown_correct_after_move() {
        // Verify markdown serialization is correct after moving.
        let mut state = DocState::from_doc(three_para_doc());

        // Move first paragraph (0..7) to end of doc.
        // p1: 0..7 "Alpha", p2: 7..14 "Bravo", p3: 14..23 "Charlie"
        let doc_size = state.doc.content.size();
        state.move_block(0, 7, doc_size);

        let md = state.to_markdown();
        // After move: Bravo, Charlie, Alpha
        assert!(
            md.starts_with("Bravo"),
            "expected markdown to start with 'Bravo', got: {md:?}"
        );
        assert!(
            md.contains("Charlie"),
            "expected 'Charlie' in markdown, got: {md:?}"
        );
        assert!(
            md.ends_with("Alpha"),
            "expected markdown to end with 'Alpha', got: {md:?}"
        );
    }

    #[test]
    fn move_block_forward_middle_to_end() {
        // Move middle paragraph to end in a three-paragraph doc.
        // Before: Alpha, Bravo, Charlie
        // Move Bravo (7..14) to end (23).
        // After: Alpha, Charlie, Bravo
        let mut state = DocState::from_doc(three_para_doc());
        let doc_size = state.doc.content.size();
        state.move_block(7, 14, doc_size);

        assert_eq!(state.doc.child_count(), 3);
        assert_eq!(state.doc.child(0).text_content(), "Alpha");
        assert_eq!(state.doc.child(1).text_content(), "Charlie");
        assert_eq!(state.doc.child(2).text_content(), "Bravo");
    }

    #[test]
    fn move_block_clears_redo_stack() {
        // Verify that redo stack is cleared after a move.
        let mut state = DocState::from_doc(two_para_doc());

        // Do an edit, then undo to populate redo stack.
        state.insert_text("x");
        state.undo();
        assert!(!state.redo_stack.is_empty(), "redo stack should have an entry");

        // Now move a block — redo should be cleared.
        state.move_block(0, 7, 14);
        assert!(state.redo_stack.is_empty(), "redo stack should be cleared after move_block");
    }
}

mod cursor_movement_tests {
    use std::collections::HashSet;

    use crate::attrs::code_block_attrs;
    use crate::doc_state::*;
    use crate::fragment::Fragment;
    use crate::node::Node;
    use crate::node_type::NodeType;

    fn atomic_set(langs: &[&str]) -> HashSet<String> {
        langs.iter().map(|s| s.to_string()).collect()
    }

    /// Helper: <doc><codeblock[chartml]>chart1</codeblock><codeblock[mermaid]>graph</codeblock></doc>
    fn two_atomic_blocks_doc() -> (Node, HashSet<String>) {
        let cb1 = Node::branch_with_attrs(
            NodeType::CodeBlock,
            code_block_attrs("chartml"),
            Fragment::from_node(Node::new_text("chart1")),
        );
        let cb2 = Node::branch_with_attrs(
            NodeType::CodeBlock,
            code_block_attrs("mermaid"),
            Fragment::from_node(Node::new_text("graph")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_vec(vec![cb1, cb2]));
        (doc, atomic_set(&["chartml", "mermaid"]))
    }

    /// Helper: <doc><p>abc</p><codeblock[chartml]>chart</codeblock><p>def</p></doc>
    ///
    /// Positions:
    ///   0: before p1
    ///   1..4: inside p1 ("abc")
    ///   5: between p1 and codeblock (gap)
    ///   6..11: inside codeblock content ("chart") — opaque
    ///  12: between codeblock and p2 (gap)
    ///  13..16: inside p2 ("def")
    ///  17: after p2
    fn para_atom_para_doc() -> (Node, HashSet<String>) {
        let p1 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("abc")),
        );
        let cb = Node::branch_with_attrs(
            NodeType::CodeBlock,
            code_block_attrs("chartml"),
            Fragment::from_node(Node::new_text("chart")),
        );
        let p2 = Node::branch(
            NodeType::Paragraph,
            Fragment::from_node(Node::new_text("def")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_vec(vec![p1, cb, p2]));
        (doc, atomic_set(&["chartml"]))
    }

    #[test]
    fn move_right_within_text() {
        let (doc, atoms) = para_atom_para_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);
        state.set_selection(Selection::cursor(1)); // start of "abc"
        state.move_right();
        assert_eq!(state.selection().head, 2); // after 'a'
        state.move_right();
        assert_eq!(state.selection().head, 3); // after 'b'
        state.move_right();
        assert_eq!(state.selection().head, 4); // after 'c'
    }

    #[test]
    fn move_right_from_end_of_text_to_gap_cursor() {
        let (doc, atoms) = para_atom_para_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);
        state.set_selection(Selection::cursor(4)); // end of "abc"
        state.move_right();
        // Should land at gap position before the atomic block.
        assert_eq!(state.selection().head, 5);
        assert!(state.is_gap_cursor(5), "pos 5 should be a valid gap cursor");
    }

#[test]
    fn move_right_from_gap_skips_atomic_block() {
        let (doc, atoms) = para_atom_para_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);
        // Navigate to the gap before the atomic block naturally.
        state.set_selection(Selection::cursor(4)); // end of "abc"
        state.move_right(); // -> gap at 5
        assert_eq!(state.selection().head, 5);
        // Now skip the atomic block.
        state.move_right();
        assert_eq!(state.selection().head, 12);
    }

    #[test]
    fn move_right_from_gap_after_atom_enters_textblock() {
        let (doc, atoms) = para_atom_para_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);
        // Navigate to gap after the atomic block.
        state.set_selection(Selection::cursor(4)); // end of "abc"
        state.move_right(); // -> gap at 5 (before atom)
        state.move_right(); // -> gap at 12 (after atom)
        assert_eq!(state.selection().head, 12);
        // Now move right — should enter next textblock.
        state.move_right();
        assert_eq!(state.selection().head, 13); // start of "def"
    }

    #[test]
    fn move_left_within_text() {
        let (doc, atoms) = para_atom_para_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);
        state.set_selection(Selection::cursor(16)); // end of "def"
        state.move_left();
        assert_eq!(state.selection().head, 15);
        state.move_left();
        assert_eq!(state.selection().head, 14);
        state.move_left();
        assert_eq!(state.selection().head, 13);
    }

    #[test]
    fn move_left_from_start_of_text_to_gap_cursor() {
        let (doc, atoms) = para_atom_para_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);
        state.set_selection(Selection::cursor(13)); // start of "def"
        state.move_left();
        assert_eq!(state.selection().head, 12);
        assert!(state.is_gap_cursor(12));
    }

    #[test]
    fn move_left_from_gap_skips_atomic_block() {
        let (doc, atoms) = para_atom_para_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);
        state.set_selection(Selection::cursor(13)); // start of "def"
        state.move_left(); // -> gap at 12 (after atom)
        assert_eq!(state.selection().head, 12);
        state.move_left(); // -> gap at 5 (before atom)
        assert_eq!(state.selection().head, 5);
        assert!(state.is_gap_cursor(5));
    }

    #[test]
    fn move_left_from_gap_before_atom_enters_textblock() {
        let (doc, atoms) = para_atom_para_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);
        state.set_selection(Selection::cursor(13)); // start of "def"
        state.move_left(); // -> gap at 12
        state.move_left(); // -> gap at 5
        state.move_left(); // -> end of "abc" = 4
        assert_eq!(state.selection().head, 4);
    }

    #[test]
    fn move_right_full_traversal_through_atomic_block() {
        let (doc, atoms) = para_atom_para_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);
        state.set_selection(Selection::cursor(1));

        // Move through "abc"
        state.move_right(); // 2
        state.move_right(); // 3
        state.move_right(); // 4 (end of "abc")

        // Hit gap cursor left of block
        state.move_right(); // 5 (gap before atom)
        assert_eq!(state.selection().head, 5);

        // Skip entire block
        state.move_right(); // 12 (gap after atom)
        assert_eq!(state.selection().head, 12);

        // Enter next paragraph
        state.move_right(); // 13 (start of "def")
        assert_eq!(state.selection().head, 13);

        // Continue through "def"
        state.move_right(); // 14
        state.move_right(); // 15
        state.move_right(); // 16
        assert_eq!(state.selection().head, 16);
    }

    #[test]
    fn move_left_full_traversal_through_atomic_block() {
        let (doc, atoms) = para_atom_para_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);
        // "def" is at positions 13-16. set_selection(16) adjusts into textblock
        // which keeps it at 16 (inside p2).
        state.set_selection(Selection::cursor(16));
        assert_eq!(state.selection().head, 16);

        state.move_left(); // 15
        state.move_left(); // 14
        state.move_left(); // 13 (start of "def")

        state.move_left(); // 12 (gap after atom)
        assert_eq!(state.selection().head, 12);

        state.move_left(); // 5 (gap before atom — skips block)
        assert_eq!(state.selection().head, 5);

        state.move_left(); // 4 (end of "abc")
        assert_eq!(state.selection().head, 4);

        state.move_left(); // 3
        state.move_left(); // 2
        state.move_left(); // 1
        assert_eq!(state.selection().head, 1);
    }

    #[test]
    fn move_right_between_two_adjacent_atoms() {
        // Doc: <codeblock[chartml]>chart1</codeblock><codeblock[mermaid]>graph</codeblock>
        //   cb1: node_size = 1+6+1=8, positions 0..8
        //   cb2: node_size = 1+5+1=7, positions 8..15
        //   doc size = 15
        //
        // But from_doc_with_atoms adds an empty paragraph if no children...
        // No, ensure_min_content only adds if child_count==0 — our doc has 2 children.
        //
        // Initial cursor from_doc_with_atoms is at position 1 (inside the first
        // textblock). But cb1 is atomic, so adjust_into_textblock at pos 1 would
        // resolve inside cb1 (a textblock-like CodeBlock marked as atom).
        // Actually CodeBlock IS_textblock... Let me check.
        let (doc, atoms) = two_atomic_blocks_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);
        let doc_size = state.doc().content.size();

        // The initial cursor is at 1, which is inside cb1 content.
        // But cb1 is atomic, so we need to navigate from a known position.
        // pos 0 is gap before cb1. set_selection(cursor(0)) adjusts: node_before
        // is None, node_after is cb1 (atom) → gap cursor stays at 0 because
        // neither neighbor is a non-atomic textblock. Actually wait — set_selection
        // calls adjust_into_textblock... at pos 0, parent is Doc, node_before is
        // None, node_after is cb1 (atom, not non-atomic textblock). So
        // before_is_textblock=false, after_is_textblock=false → returns 0.
        state.set_selection(Selection::cursor(0));
        assert_eq!(state.selection().head, 0);
        assert!(state.is_gap_cursor(0));

        state.move_right(); // skip cb1 -> gap at 8
        assert_eq!(state.selection().head, 8);
        assert!(state.is_gap_cursor(8));

        state.move_right(); // skip cb2 -> gap at 15
        assert_eq!(state.selection().head, doc_size);
    }

    #[test]
    fn move_left_between_two_adjacent_atoms() {
        let (doc, atoms) = two_atomic_blocks_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);
        let doc_size = state.doc().content.size();

        // Navigate to end of doc. set_selection at doc_size: parent is Doc,
        // node_before is cb2 (atom), no node_after → gap cursor. Both neighbors
        // are atom/None so adjust_into_textblock returns as-is.
        state.set_selection(Selection::cursor(doc_size));
        assert_eq!(state.selection().head, doc_size);

        state.move_left(); // skip cb2 -> gap at 8
        assert_eq!(state.selection().head, 8);

        state.move_left(); // skip cb1 -> gap at 0
        assert_eq!(state.selection().head, 0);
    }

    #[test]
    fn move_right_at_document_end_is_noop() {
        let (doc, atoms) = para_atom_para_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);
        let doc_size = state.doc().content.size();
        state.set_selection(Selection::cursor(doc_size));
        state.move_right();
        assert_eq!(state.selection().head, doc_size);
    }

    #[test]
    fn move_left_at_document_start_is_noop() {
        let (doc, atoms) = para_atom_para_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);
        state.set_selection(Selection::cursor(0));
        state.move_left();
        assert_eq!(state.selection().head, 0);
    }

    #[test]
    fn is_gap_cursor_inside_textblock_is_false() {
        let (doc, atoms) = para_atom_para_doc();
        let state = DocState::from_doc_with_atoms(doc, atoms);
        assert!(!state.is_gap_cursor(1));
        assert!(!state.is_gap_cursor(3));
        assert!(!state.is_gap_cursor(14));
    }

    #[test]
    fn is_gap_cursor_at_atom_boundary_is_true() {
        let (doc, atoms) = para_atom_para_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);
        // Navigate to gap positions to verify they're valid gap cursors.
        state.set_selection(Selection::cursor(4));
        state.move_right(); // pos 5 (gap before atom)
        assert!(state.is_gap_cursor(state.selection().head));

        state.move_right(); // pos 12 (gap after atom)
        assert!(state.is_gap_cursor(state.selection().head));
    }

    #[test]
    fn gap_cursor_info_returns_correct_side() {
        let (doc, atoms) = para_atom_para_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);

        // Navigate to gap before atom.
        state.set_selection(Selection::cursor(4)); // end of "abc"
        state.move_right(); // -> gap at 5
        assert_eq!(state.selection().head, 5);
        let info = state.gap_cursor_info();
        assert!(info.is_some());
        let (side, start, end) = info.unwrap();
        assert_eq!(side, GapSide::Before);
        assert_eq!(start, 5);
        assert_eq!(end, 12);

        // Navigate to gap after atom.
        state.move_right(); // -> gap at 12
        assert_eq!(state.selection().head, 12);
        let info = state.gap_cursor_info();
        assert!(info.is_some());
        let (side, start, end) = info.unwrap();
        assert_eq!(side, GapSide::After);
        assert_eq!(start, 5);
        assert_eq!(end, 12);
    }

    #[test]
    fn snap_out_of_atom_inside_atomic_block() {
        let (doc, atoms) = para_atom_para_doc();
        let state = DocState::from_doc_with_atoms(doc, atoms);
        // Position 8 is inside the atomic block content.
        // Block is at 5..12. pos 8 is closer to 5 (distance 3) than 12 (distance 4).
        assert_eq!(state.snap_out_of_atom(8), 5);
        // Position 10 is closer to 12 (distance 2) than 5 (distance 5).
        assert_eq!(state.snap_out_of_atom(10), 12);
    }

    #[test]
    fn snap_out_of_atom_outside_atom_unchanged() {
        let (doc, atoms) = para_atom_para_doc();
        let state = DocState::from_doc_with_atoms(doc, atoms);
        assert_eq!(state.snap_out_of_atom(1), 1);
        assert_eq!(state.snap_out_of_atom(4), 4);
        assert_eq!(state.snap_out_of_atom(14), 14);
    }

    #[test]
    fn enter_then_delete_at_gap_removes_empty_para() {
        let (doc, atoms) = para_atom_para_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);
        // Navigate to gap before atom.
        state.set_selection(Selection::cursor(4));
        state.move_right(); // gap at 5
        assert!(state.gap_cursor_info().is_some());

        let children_before = state.doc().child_count();
        // Enter creates a new empty paragraph.
        state.split_block();
        assert_eq!(state.doc().child_count(), children_before + 1);

        // Delete should remove the empty paragraph, not the atom.
        state.delete_forward();
        assert_eq!(state.doc().child_count(), children_before,
            "delete should remove the empty paragraph, not the atom");
    }

    #[test]
    fn delete_forward_in_empty_para_before_atom_removes_para() {
        // <doc><atom>chart</atom><p></p><atom>chart2</atom></doc>
        // Cursor in the empty paragraph — Delete should remove the paragraph,
        // not delete the next atomic block.
        let cb1 = Node::branch_with_attrs(
            NodeType::CodeBlock,
            code_block_attrs("chartml"),
            Fragment::from_node(Node::new_text("chart")),
        );
        let empty_p = Node::branch(NodeType::Paragraph, Fragment::empty());
        let cb2 = Node::branch_with_attrs(
            NodeType::CodeBlock,
            code_block_attrs("mermaid"),
            Fragment::from_node(Node::new_text("chart2")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_vec(vec![cb1, empty_p, cb2]));
        let mut state = DocState::from_doc_with_atoms(doc, atomic_set(&["chartml", "mermaid"]));

        // cb1: size 7 (0..7), empty_p: size 2 (7..9), cb2: size 8 (9..17)
        // Cursor inside empty paragraph = position 8
        state.set_selection(Selection::cursor(8));

        let child_count_before = state.doc().child_count();
        state.delete_forward();

        // The empty paragraph should be removed, both atoms should remain.
        assert_eq!(state.doc().child_count(), child_count_before - 1,
            "empty paragraph should be deleted, not the atomic block");
        assert_eq!(state.doc().child(0).node_type, NodeType::CodeBlock);
        assert_eq!(state.doc().child(1).node_type, NodeType::CodeBlock);
    }

    #[test]
    fn backspace_in_empty_para_between_atoms_removes_para() {
        // Reproduces the Mac Delete key bug: user navigates between two
        // side-by-side atomic charts, presses Enter to create a paragraph,
        // then presses Backspace (Mac Delete). The backspace should remove
        // the empty paragraph, not destroy an atomic block.
        let (doc, atoms) = para_atom_para_doc();
        let mut state = DocState::from_doc_with_atoms(doc, atoms);

        // Navigate to gap between the atom and p2.
        state.set_selection(Selection::cursor(4)); // end of "abc"
        state.move_right(); // gap at 5 (before atom)
        state.move_right(); // gap at 12 (after atom, before p2)

        // Enter creates a paragraph at the gap.
        state.split_block();
        let children_after_enter = state.doc().child_count();
        assert_eq!(children_after_enter, 4, "should be p1, atom, new_p, p2");

        // Backspace should remove the empty paragraph, not the atom.
        state.backspace();
        assert_eq!(state.doc().child_count(), 3,
            "backspace should remove the empty paragraph, not the atom");

        // Verify all original nodes are intact.
        assert_eq!(state.doc().child(0).node_type, NodeType::Paragraph);
        assert_eq!(state.doc().child(0).text_content(), "abc");
        assert!(state.doc().child(1).is_atom());
        assert_eq!(state.doc().child(2).node_type, NodeType::Paragraph);
        assert_eq!(state.doc().child(2).text_content(), "def");
    }

    #[test]
    fn backspace_in_empty_para_between_two_atoms_removes_para() {
        // Two adjacent atomic blocks with an empty paragraph between them.
        let cb1 = Node::branch_with_attrs(
            NodeType::CodeBlock,
            code_block_attrs("chartml"),
            Fragment::from_node(Node::new_text("chart1")),
        );
        let empty_p = Node::branch(NodeType::Paragraph, Fragment::empty());
        let cb2 = Node::branch_with_attrs(
            NodeType::CodeBlock,
            code_block_attrs("mermaid"),
            Fragment::from_node(Node::new_text("chart2")),
        );
        let doc = Node::branch(NodeType::Doc, Fragment::from_vec(vec![cb1, empty_p, cb2]));
        let mut state = DocState::from_doc_with_atoms(doc, atomic_set(&["chartml", "mermaid"]));

        // Cursor inside the empty paragraph.
        // cb1: size 8 (0..8), empty_p: size 2 (8..10), cb2: size 8 (10..18)
        // Position 9 is inside empty_p content.
        state.set_selection(Selection::cursor(9));

        state.backspace();

        assert_eq!(state.doc().child_count(), 2,
            "backspace should remove the empty paragraph");
        assert_eq!(state.doc().child(0).node_type, NodeType::CodeBlock);
        assert_eq!(state.doc().child(1).node_type, NodeType::CodeBlock);
        assert_eq!(state.doc().child(0).text_content(), "chart1");
        assert_eq!(state.doc().child(1).text_content(), "chart2");
    }
}

mod table_editing_tests {
    use crate::doc_state::*;
    use crate::serialize::serialize_markdown;

    // Table markdown used for all tests:
    //   "| A | B |\n| --- | --- |\n| hello | world |"
    //
    // Parsed tree positions (verified via resolve):
    //   0:  Doc boundary
    //   1:  Table open
    //   2:  TableHeader open
    //   3:  TableCell "A" content start (parent_offset=0)
    //   4:  after "A" (parent_offset=1)
    //   5:  TableCell "B" open
    //   6:  TableCell "B" content start (parent_offset=0)
    //   7:  after "B" (parent_offset=1)
    //   8:  TableHeader close
    //   9:  Table second child boundary
    //  10:  TableRow open
    //  11:  TableCell "hello" content start (parent_offset=0)
    //  12:  after "h" (parent_offset=1)
    //  13:  after "he" (parent_offset=2)
    //  14:  after "hel" (parent_offset=3)
    //  15:  after "hell" (parent_offset=4)
    //  16:  after "hello" (parent_offset=5, end of content)
    //  17:  TableCell "world" open
    //  18:  TableCell "world" content start (parent_offset=0)
    //  19-23: "world" chars
    //  24:  TableRow close
    //  25:  Table close
    //  26:  Doc boundary

    const TABLE_MD: &str = "| A | B |\n| --- | --- |\n| hello | world |";

    #[test]
    fn table_insert_text_in_cell() {
        let mut state = DocState::from_markdown(TABLE_MD);
        // pos 11 = start of "hello" cell content (parent_offset=0)
        state.set_selection(Selection::cursor(11));
        state.insert_text("X");

        let md = serialize_markdown(state.doc());
        assert!(
            md.contains("Xhello"),
            "cell should contain 'Xhello', got: {md:?}"
        );
    }

    #[test]
    fn table_enter_inserts_newline() {
        let mut state = DocState::from_markdown(TABLE_MD);

        // pos 13 = after "he" in "hello" (parent_offset=2)
        state.set_selection(Selection::cursor(13));
        state.split_block();

        // Table structure must be preserved: still one table with header + row.
        let table = state.doc().child(0);
        assert_eq!(
            table.node_type,
            crate::node_type::NodeType::Table,
            "first child should still be a Table"
        );
        assert_eq!(
            table.child_count(),
            2,
            "table should still have 2 children (header + row)"
        );

        // The cell should now contain "he", a HardBreak, and "llo".
        let row = table.child(1);
        let cell = row.child(0);
        let has_hard_break = cell.content.iter().any(|c| c.node_type == crate::node_type::NodeType::HardBreak);
        assert!(
            has_hard_break,
            "cell should contain a HardBreak after split_block"
        );
        let cell_text = cell.text_content();
        assert!(
            cell_text.contains("he") && cell_text.contains("llo"),
            "cell should contain 'he' and 'llo' around the break, got: {cell_text:?}"
        );

        // Verify we didn't accidentally produce a second table or extra rows.
        assert_eq!(
            state.doc().child_count(),
            1,
            "doc should still have exactly 1 child (the table)"
        );

        // Round-trip: serialize → re-parse → verify HardBreak survives
        let md = crate::serialize_markdown(state.doc());
        assert!(md.contains("<br>"), "serialized markdown should contain <br>");
        let reparsed = DocState::from_markdown(&md);
        let rt_table = reparsed.doc().child(0);
        let rt_row = rt_table.child(1);
        let rt_cell = rt_row.child(0);
        let rt_has_br = rt_cell.content.iter().any(|c| c.node_type == crate::node_type::NodeType::HardBreak);
        assert!(rt_has_br, "HardBreak should survive round-trip through serialization");
    }

    #[test]
    fn table_backspace_at_cell_start_is_noop() {
        let mut state = DocState::from_markdown(TABLE_MD);
        let before_md = serialize_markdown(state.doc());

        // pos 11 = start of "hello" cell content (parent_offset=0)
        state.set_selection(Selection::cursor(11));
        state.backspace();

        let after_md = serialize_markdown(state.doc());
        assert_eq!(
            before_md, after_md,
            "backspace at cell start should be a no-op"
        );
    }

    #[test]
    fn table_backspace_mid_cell_deletes_char() {
        let mut state = DocState::from_markdown(TABLE_MD);

        // pos 13 = after "he" in "hello" (parent_offset=2)
        state.set_selection(Selection::cursor(13));
        state.backspace();

        let md = serialize_markdown(state.doc());
        assert!(
            md.contains("hllo"),
            "backspace should delete 'e', producing 'hllo', got: {md:?}"
        );
    }

    #[test]
    fn table_delete_at_cell_end_is_noop() {
        let mut state = DocState::from_markdown(TABLE_MD);
        let before_md = serialize_markdown(state.doc());

        // pos 16 = end of "hello" content (parent_offset=5)
        state.set_selection(Selection::cursor(16));
        state.delete_forward();

        let after_md = serialize_markdown(state.doc());
        assert_eq!(
            before_md, after_md,
            "delete at cell end should be a no-op"
        );
    }

    #[test]
    fn table_delete_mid_cell_deletes_char() {
        let mut state = DocState::from_markdown(TABLE_MD);

        // pos 13 = after "he" in "hello" (parent_offset=2)
        // delete_forward removes the character AFTER cursor = 'l' at offset 2
        state.set_selection(Selection::cursor(13));
        state.delete_forward();

        let md = serialize_markdown(state.doc());
        assert!(
            md.contains("helo"),
            "delete should remove first 'l', producing 'helo', got: {md:?}"
        );
    }

    // ── Navigation tests ──────────────────────────────────────────────

    #[test]
    fn table_move_to_next_cell() {
        let mut state = DocState::from_markdown(TABLE_MD);
        // pos 3 = first header cell "A" content start
        state.set_selection(Selection::cursor(3));
        let moved = state.move_to_next_cell();
        assert!(moved, "move_to_next_cell should return true");
        // pos 6 = second header cell "B" content start
        assert_eq!(
            state.selection.head, 6,
            "cursor should be in the second header cell"
        );
    }

    #[test]
    fn table_move_to_next_cell_wraps_row() {
        let mut state = DocState::from_markdown(TABLE_MD);
        // pos 6 = last header cell "B" content start
        state.set_selection(Selection::cursor(6));
        let moved = state.move_to_next_cell();
        assert!(moved, "move_to_next_cell should return true when wrapping");
        // pos 11 = first cell of data row "hello" content start
        assert_eq!(
            state.selection.head, 11,
            "cursor should wrap to first cell of next row"
        );
    }

    #[test]
    fn table_move_to_next_cell_returns_false_at_end() {
        let mut state = DocState::from_markdown(TABLE_MD);
        // pos 18 = last cell "world" content start (last row, last cell)
        state.set_selection(Selection::cursor(18));
        let moved = state.move_to_next_cell();
        assert!(!moved, "move_to_next_cell should return false at end of table");
    }

    #[test]
    fn table_move_to_prev_cell() {
        let mut state = DocState::from_markdown(TABLE_MD);
        // pos 6 = second header cell "B" content start
        state.set_selection(Selection::cursor(6));
        let moved = state.move_to_prev_cell();
        assert!(moved, "move_to_prev_cell should return true");
        // pos 3 = first header cell "A" content start
        assert_eq!(
            state.selection.head, 3,
            "cursor should be in the first header cell"
        );
    }

    #[test]
    fn table_move_to_prev_cell_wraps_row() {
        let mut state = DocState::from_markdown(TABLE_MD);
        // pos 11 = first cell of data row "hello" content start
        state.set_selection(Selection::cursor(11));
        let moved = state.move_to_prev_cell();
        assert!(moved, "move_to_prev_cell should return true when wrapping");
        // pos 6 = last cell of header row "B" content start
        assert_eq!(
            state.selection.head, 6,
            "cursor should wrap to last cell of previous row"
        );
    }

    #[test]
    fn table_move_to_prev_cell_returns_false_at_start() {
        let mut state = DocState::from_markdown(TABLE_MD);
        // pos 3 = first header cell "A" content start (first row, first cell)
        state.set_selection(Selection::cursor(3));
        let moved = state.move_to_prev_cell();
        assert!(!moved, "move_to_prev_cell should return false at start of table");
    }

    // ── Insert table test ─────────────────────────────────────────────

    #[test]
    fn table_insert_table_creates_structure() {
        let mut state = DocState::from_markdown("Hello");
        // pos 1 = inside the paragraph, at "H"
        state.set_selection(Selection::cursor(1));
        state.insert_table();

        let md = serialize_markdown(state.doc());
        // insert_table creates 3 columns, 1 header + 2 body rows.
        // Verify the output contains a pipe table with header + delimiter + 2 data rows.
        let lines: Vec<&str> = md.lines().collect();
        let table_lines: Vec<&&str> = lines.iter().filter(|l| l.starts_with('|')).collect();
        assert!(
            table_lines.len() >= 4,
            "should have at least 4 pipe rows (header + delimiter + 2 data), got: {md:?}"
        );
        // Check delimiter row exists
        assert!(
            md.contains("| --- | --- | --- |"),
            "should contain a 3-column delimiter row, got: {md:?}"
        );
    }

    // ── Row operation tests ───────────────────────────────────────────

    // Extended table: header + 2 data rows for row/column operation tests.
    //
    // "| A | B |\n| --- | --- |\n| c | d |\n| e | f |"
    //
    // Position map:
    //   0:  Doc boundary
    //   1:  Table content start
    //   2:  TableHeader content start
    //   3:  Cell "A" content start
    //   4:  after "A"
    //   5:  Cell "B" open
    //   6:  Cell "B" content start
    //   7:  after "B"
    //   8:  TableHeader close
    //   9:  TableRow 1 start
    //  10:  TableRow 1 content start
    //  11:  Cell "c" content start
    //  12:  after "c"
    //  13:  Cell "d" open
    //  14:  Cell "d" content start
    //  15:  after "d"
    //  16:  TableRow 1 close
    //  17:  TableRow 2 start
    //  18:  TableRow 2 content start
    //  19:  Cell "e" content start
    //  20:  after "e"
    //  21:  Cell "f" open
    //  22:  Cell "f" content start
    //  23:  after "f"
    //  24:  TableRow 2 close
    //  25:  Table close
    //  26:  Doc boundary
    const EXTENDED_TABLE_MD: &str =
        "| A | B |\n| --- | --- |\n| c | d |\n| e | f |";

    fn count_pipe_rows(md: &str) -> usize {
        md.lines().filter(|l| l.starts_with('|')).count()
    }

    fn count_cells_in_row(line: &str) -> usize {
        // Count pipe-delimited cells: "| a | b |" -> 2 cells.
        let trimmed = line.trim().trim_start_matches('|').trim_end_matches('|');
        trimmed.split('|').count()
    }

    #[test]
    fn table_insert_row_below() {
        let mut state = DocState::from_markdown(EXTENDED_TABLE_MD);
        // pos 11 = first cell of first data row "c" content start
        state.set_selection(Selection::cursor(11));
        state.insert_row_below();

        let md = serialize_markdown(state.doc());
        // Original: header + delim + 2 data = 4 pipe rows.
        // After insert: header + delim + 3 data = 5 pipe rows.
        assert_eq!(
            count_pipe_rows(&md),
            5,
            "should have 5 pipe rows after insert_row_below, got: {md:?}"
        );
        // Every pipe row should have 2 cells.
        for line in md.lines().filter(|l| l.starts_with('|') && !l.contains("---")) {
            assert_eq!(
                count_cells_in_row(line),
                2,
                "every row should have 2 cells, got line: {line:?}"
            );
        }
    }

    #[test]
    fn table_insert_row_above() {
        let mut state = DocState::from_markdown(EXTENDED_TABLE_MD);
        // pos 19 = first cell of second data row "e" content start
        state.set_selection(Selection::cursor(19));
        state.insert_row_above();

        let md = serialize_markdown(state.doc());
        // Original: 4 pipe rows. After insert above: 5 pipe rows.
        assert_eq!(
            count_pipe_rows(&md),
            5,
            "should have 5 pipe rows after insert_row_above, got: {md:?}"
        );
        // The new row should appear between "c | d" and "e | f".
        let data_lines: Vec<&str> = md
            .lines()
            .filter(|l| l.starts_with('|') && !l.contains("---"))
            .collect();
        // data_lines: header, row "c|d", new empty row, row "e|f"
        // The row with "e" should be at index 3 (0-indexed).
        assert!(
            data_lines.last().unwrap().contains('e'),
            "last data row should still contain 'e', got: {md:?}"
        );
    }

    #[test]
    fn table_insert_row_above_on_header_inserts_below() {
        let mut state = DocState::from_markdown(EXTENDED_TABLE_MD);
        // pos 3 = header cell "A" content start
        state.set_selection(Selection::cursor(3));
        state.insert_row_above();

        let md = serialize_markdown(state.doc());
        // Can't insert above header, so it inserts below. 5 pipe rows total.
        assert_eq!(
            count_pipe_rows(&md),
            5,
            "should have 5 pipe rows after insert_row_above on header, got: {md:?}"
        );
        // Header should still be first: "| A | B |"
        let first_line = md.lines().next().unwrap();
        assert!(
            first_line.contains('A') && first_line.contains('B'),
            "header should still be first row, got: {md:?}"
        );
    }

    #[test]
    fn table_delete_row() {
        let mut state = DocState::from_markdown(EXTENDED_TABLE_MD);
        // pos 11 = first cell of first data row "c" content start
        state.set_selection(Selection::cursor(11));
        state.delete_row();

        let md = serialize_markdown(state.doc());
        // Original: 4 pipe rows. After delete: 3 pipe rows.
        assert_eq!(
            count_pipe_rows(&md),
            3,
            "should have 3 pipe rows after delete_row, got: {md:?}"
        );
        // The deleted row's content ("c", "d") should be gone.
        assert!(
            !md.contains("| c |"),
            "deleted row's content should be gone, got: {md:?}"
        );
    }

    #[test]
    fn table_delete_row_noop_on_header() {
        let mut state = DocState::from_markdown(EXTENDED_TABLE_MD);
        let before_md = serialize_markdown(state.doc());
        // pos 3 = header cell "A" content start
        state.set_selection(Selection::cursor(3));
        state.delete_row();

        let after_md = serialize_markdown(state.doc());
        assert_eq!(
            before_md, after_md,
            "delete_row on header should be a no-op"
        );
    }

    // ── Column operation tests ────────────────────────────────────────

    #[test]
    fn table_insert_column_right() {
        let mut state = DocState::from_markdown(EXTENDED_TABLE_MD);
        // pos 3 = header cell "A" content start (column 0)
        state.set_selection(Selection::cursor(3));
        state.insert_column_right();

        let md = serialize_markdown(state.doc());
        // Every row should now have 3 cells (was 2).
        for line in md.lines().filter(|l| l.starts_with('|') && !l.contains("---")) {
            assert_eq!(
                count_cells_in_row(line),
                3,
                "every row should have 3 cells after insert_column_right, got line: {line:?}"
            );
        }
        // Delimiter row should also have 3 columns.
        let delim_line = md.lines().find(|l| l.contains("---")).unwrap();
        assert_eq!(
            count_cells_in_row(delim_line),
            3,
            "delimiter row should have 3 columns, got: {delim_line:?}"
        );
    }

    #[test]
    fn table_insert_column_left() {
        let mut state = DocState::from_markdown(EXTENDED_TABLE_MD);
        // pos 6 = header cell "B" content start (column 1)
        state.set_selection(Selection::cursor(6));
        state.insert_column_left();

        let md = serialize_markdown(state.doc());
        // Every row should now have 3 cells (was 2).
        for line in md.lines().filter(|l| l.starts_with('|') && !l.contains("---")) {
            assert_eq!(
                count_cells_in_row(line),
                3,
                "every row should have 3 cells after insert_column_left, got line: {line:?}"
            );
        }
    }

    #[test]
    fn table_delete_column() {
        let mut state = DocState::from_markdown(EXTENDED_TABLE_MD);
        // pos 3 = header cell "A" content start (column 0)
        state.set_selection(Selection::cursor(3));
        state.delete_column();

        let md = serialize_markdown(state.doc());
        // Every row should now have 1 cell (was 2).
        for line in md.lines().filter(|l| l.starts_with('|') && !l.contains("---")) {
            assert_eq!(
                count_cells_in_row(line),
                1,
                "every row should have 1 cell after delete_column, got line: {line:?}"
            );
        }
        // Column "A" content should be gone from header.
        let first_line = md.lines().next().unwrap();
        assert!(
            !first_line.contains('A'),
            "deleted column 'A' should be gone, got: {first_line:?}"
        );
    }

    #[test]
    fn table_delete_column_noop_on_single_column() {
        let single_col_md = "| X |\n| --- |\n| a |";
        let mut state = DocState::from_markdown(single_col_md);
        let before_md = serialize_markdown(state.doc());
        // pos 3 = cell "X" content start
        state.set_selection(Selection::cursor(3));
        state.delete_column();

        let after_md = serialize_markdown(state.doc());
        assert_eq!(
            before_md, after_md,
            "delete_column on single-column table should be a no-op"
        );
    }
}
