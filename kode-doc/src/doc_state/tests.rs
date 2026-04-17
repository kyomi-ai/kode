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
    fn from_doc_empty_doc_cursor_at_zero() {
        let doc = Node::branch(NodeType::Doc, Fragment::empty());
        let state = DocState::from_doc(doc);
        assert_eq!(state.selection, Selection::cursor(0));
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
    fn insert_empty_text_is_noop() {
        let mut state = DocState::from_doc(simple_doc());
        state.set_selection(Selection::cursor(3));
        state.insert_text("");

        assert_eq!(state.doc.child(0).text_content(), "Hello");
        assert!(state.undo_stack.is_empty());
    }

    #[test]
    fn insert_text_into_empty_doc() {
        // Empty document: Doc[] with cursor at 0.
        // Typing should bootstrap a paragraph and insert text.
        let doc = Node::branch(NodeType::Doc, Fragment::empty());
        let mut state = DocState::from_doc(doc);
        assert_eq!(state.selection, Selection::cursor(0));

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
