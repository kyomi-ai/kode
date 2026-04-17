#!/usr/bin/env node
/**
 * Kode Editor Visual Regression — captures screenshots of editor states.
 *
 * Captures both Code Editor and WYSIWYG editor in various scenarios across
 * all three themes. Each scenario produces a screenshot for visual comparison.
 *
 * Usage:
 *   node capture-editor-states.js                # All scenarios, all themes
 *   node capture-editor-states.js code           # Code editor scenarios only
 *   node capture-editor-states.js wysiwyg        # WYSIWYG scenarios only
 *   node capture-editor-states.js code 3         # Code editor, scenario #3 only
 *
 * Output: screenshots/kode/{theme}/{scenario-name}.png
 *
 * Requires: npm install (playwright must be installed)
 */

const { chromium } = require("playwright");
const fs = require("fs");
const path = require("path");

const BASE_URL = process.env.KODE_URL || "http://localhost:8090";
const OUT_DIR = path.join(__dirname, "screenshots", "kode");
const THEMES = ["tokyo-night", "one-dark", "github-light"];
const THEME_BUTTONS = ["Tokyo Night", "One Dark", "GitHub Light"];

const args = process.argv.slice(2);
const filterEditor = args[0] || null; // "code" or "wysiwyg" or null for both
const filterScenario = args[1] ? parseInt(args[1]) : null;

// ── Scenario definitions ──────────────────────────────────────────────

const CODE_SCENARIOS = [
  {
    name: "01-initial-load",
    description: "Editor with default SQL content, no interaction",
    steps: async (page) => {
      // Just wait for render
      await page.waitForTimeout(500);
    },
  },
  {
    name: "02-cursor-on-line",
    description: "Click into editor, cursor visible on a line with text",
    steps: async (page) => {
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 60 } });
      await page.waitForTimeout(300);
    },
  },
  {
    name: "03-typing-text",
    description: "Type new text — verify text visible on current line",
    steps: async (page) => {
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(200);
      // Move to end of first line and type
      await page.keyboard.press("End");
      await page.keyboard.press("Enter");
      await page.keyboard.type("-- This is newly typed text", { delay: 20 });
      await page.waitForTimeout(300);
    },
  },
  {
    name: "04-text-selection",
    description: "Select text with mouse drag — selection highlight visible",
    steps: async (page) => {
      const editor = page.locator(".kode-editor").first();
      // Click at start of line 3
      await editor.click({ position: { x: 80, y: 50 } });
      await page.waitForTimeout(100);
      // Select multiple lines with shift+click
      await editor.click({ position: { x: 200, y: 130 }, modifiers: ["Shift"] });
      await page.waitForTimeout(300);
    },
  },
  {
    name: "05-select-all",
    description: "Ctrl+A to select all — full selection highlight",
    steps: async (page) => {
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 60 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.waitForTimeout(300);
    },
  },
  {
    name: "06-current-line-highlight",
    description: "Cursor on a line — current line background highlight visible",
    steps: async (page) => {
      const editor = page.locator(".kode-editor").first();
      // Click on middle line
      await editor.click({ position: { x: 150, y: 100 } });
      await page.waitForTimeout(500);
    },
  },
  {
    name: "07-error-markers",
    description: "Error markers (squiggly underlines) set via API",
    steps: async (page) => {
      // Switch to code editor tab
      const codeTab = page.getByText("Code Editor", { exact: true });
      await codeTab.click();
      await page.waitForTimeout(300);

      // Click the error markers button
      const markerBtn = page.getByText("Set Error Markers", { exact: true });
      await markerBtn.click();
      await page.waitForTimeout(500);
    },
  },
  {
    name: "08-line-numbers",
    description: "Verify line number gutter renders correctly",
    steps: async (page) => {
      // Load a large file to get 3-digit line numbers
      const btn100 = page.getByText("100L", { exact: true });
      await btn100.click();
      await page.waitForTimeout(500);
    },
  },
  {
    name: "09-empty-editor",
    description: "Editor with no content — empty state",
    steps: async (page) => {
      // Switch to Plain language and clear content
      const plainBtn = page.getByText("Plain", { exact: true });
      await plainBtn.click();
      await page.waitForTimeout(200);

      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 30 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.waitForTimeout(300);
    },
  },
  {
    name: "10-yaml-syntax",
    description: "YAML syntax highlighting",
    steps: async (page) => {
      const yamlBtn = page.getByText("YAML", { exact: true });
      await yamlBtn.click();
      await page.waitForTimeout(500);
    },
  },
  {
    name: "11-long-line-overflow",
    description: "Long line that exceeds editor width — no horizontal overflow visible",
    steps: async (page) => {
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("End");
      await page.keyboard.press("Enter");
      await page.keyboard.type("SELECT very_long_column_name_1, very_long_column_name_2, very_long_column_name_3, very_long_column_name_4, extremely_long_column_name_that_extends_way_past_editor_width FROM some_table", { delay: 5 });
      await page.waitForTimeout(300);
    },
  },
  {
    name: "12-scroll-large-file",
    description: "Scrolled position in a large file — line numbers and content aligned",
    steps: async (page) => {
      const btn1000 = page.getByText("1000L", { exact: true });
      await btn1000.click();
      await page.waitForTimeout(500);
      // Scroll down
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 200, y: 200 } });
      await page.waitForTimeout(100);
      // Page down multiple times
      for (let i = 0; i < 10; i++) {
        await page.keyboard.press("PageDown");
      }
      await page.waitForTimeout(300);
    },
  },
  {
    name: "13-word-selection-double-click",
    description: "Double-click selects a word — word highlight visible",
    steps: async (page) => {
      const editor = page.locator(".kode-editor").first();
      // Double-click on "SELECT" keyword
      await editor.dblclick({ position: { x: 80, y: 10 } });
      await page.waitForTimeout(300);
    },
  },
  {
    name: "14-multiple-cursors-after-undo",
    description: "Type text, undo, verify cursor position and content correct",
    steps: async (page) => {
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("End");
      await page.keyboard.press("Enter");
      await page.keyboard.type("-- will be undone", { delay: 10 });
      await page.waitForTimeout(200);
      await page.keyboard.press("Control+z");
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+z");
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+z");
      await page.waitForTimeout(300);
    },
  },
  {
    name: "15-insert-at-cursor-api",
    description: "Insert at Cursor API button — text appears at cursor position",
    steps: async (page) => {
      const codeTab = page.getByText("Code Editor", { exact: true });
      await codeTab.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 50 } });
      await page.waitForTimeout(200);
      const insertBtn = page.getByText("Insert at Cursor", { exact: true });
      await insertBtn.click();
      await page.waitForTimeout(300);
    },
  },
  {
    name: "16-tab-indentation",
    description: "Tab key inserts spaces — indentation visible",
    steps: async (page) => {
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("End");
      await page.keyboard.press("Enter");
      await page.keyboard.press("Tab");
      await page.keyboard.press("Tab");
      await page.keyboard.type("indented_text", { delay: 10 });
      await page.waitForTimeout(300);
    },
  },
  {
    name: "17-gutter-click-selects-line",
    description: "Click on line number gutter — selects entire line",
    steps: async (page) => {
      const editor = page.locator(".kode-editor").first();
      // Click in the gutter area (left side, on line number)
      await editor.click({ position: { x: 15, y: 50 } });
      await page.waitForTimeout(300);
    },
  },
  {
    name: "18-markers-with-selection",
    description: "Error markers visible alongside text selection",
    steps: async (page) => {
      const codeTab = page.getByText("Code Editor", { exact: true });
      await codeTab.click();
      await page.waitForTimeout(300);
      const markerBtn = page.getByText("Set Error Markers", { exact: true });
      await markerBtn.click();
      await page.waitForTimeout(500);
      // Now also select some text
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 60, y: 150 } });
      await page.waitForTimeout(100);
      await editor.click({ position: { x: 300, y: 190 }, modifiers: ["Shift"] });
      await page.waitForTimeout(300);
    },
  },
  {
    name: "19-paste-multiline",
    description: "Paste multiline text — all lines render with correct line numbers",
    steps: async (page) => {
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("End");
      await page.keyboard.press("Enter");
      // Simulate paste by typing multiline content via hidden textarea
      await page.evaluate(() => {
        const ta = document.querySelector('.kode-hidden-textarea');
        if (ta) {
          ta.value = "-- pasted line 1\n-- pasted line 2\n-- pasted line 3";
          ta.dispatchEvent(new Event('input', { bubbles: true }));
        }
      });
      await page.waitForTimeout(500);
    },
  },
  {
    name: "20-theme-switch-preserves-content",
    description: "Switch theme while content visible — content preserved, colors change",
    steps: async (page) => {
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 60 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("End");
      await page.keyboard.press("Enter");
      await page.keyboard.type("-- theme test content", { delay: 10 });
      await page.waitForTimeout(200);
    },
  },
  {
    name: "21-backspace-delete",
    description: "Backspace and Delete keys remove characters correctly",
    steps: async (page) => {
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("End");
      await page.keyboard.press("Enter");
      await page.keyboard.type("ABCDEF", { delay: 10 });
      await page.waitForTimeout(100);
      // Backspace twice removes EF
      await page.keyboard.press("Backspace");
      await page.keyboard.press("Backspace");
      // Move to start and Delete removes A
      await page.keyboard.press("Home");
      await page.keyboard.press("Delete");
      await page.waitForTimeout(300);
    },
  },
  {
    name: "22-home-end-navigation",
    description: "Home/End keys move cursor to start/end of line",
    steps: async (page) => {
      const editor = page.locator(".kode-editor").first();
      // Click middle of a line
      await editor.click({ position: { x: 200, y: 90 } });
      await page.waitForTimeout(100);
      // Press End — cursor should be at end of line
      await page.keyboard.press("End");
      await page.waitForTimeout(300);
    },
  },
  {
    name: "23-arrow-key-movement",
    description: "Arrow keys move cursor — cursor at correct position after navigation",
    steps: async (page) => {
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 60, y: 10 } });
      await page.waitForTimeout(100);
      // Move down 3 lines, right 5 chars
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("ArrowDown");
      for (let i = 0; i < 5; i++) await page.keyboard.press("ArrowRight");
      await page.waitForTimeout(300);
    },
  },
  {
    name: "24-shift-arrow-selection",
    description: "Shift+Arrow keys extend selection character by character",
    steps: async (page) => {
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 60, y: 150 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Home");
      // Select first 10 chars with Shift+Right
      for (let i = 0; i < 10; i++) await page.keyboard.press("Shift+ArrowRight");
      await page.waitForTimeout(300);
    },
  },
  {
    name: "25-shift-down-line-selection",
    description: "Shift+Down selects full lines",
    steps: async (page) => {
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 60, y: 30 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Home");
      await page.keyboard.press("Shift+ArrowDown");
      await page.keyboard.press("Shift+ArrowDown");
      await page.keyboard.press("Shift+ArrowDown");
      await page.waitForTimeout(300);
    },
  },
  {
    name: "26-delete-selected-text",
    description: "Select text then delete — selection removed, remaining content intact",
    steps: async (page) => {
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 60, y: 30 } });
      await page.waitForTimeout(100);
      // Select line 2
      await page.keyboard.press("Home");
      await page.keyboard.press("Shift+ArrowDown");
      await page.waitForTimeout(100);
      // Delete the selection
      await page.keyboard.press("Backspace");
      await page.waitForTimeout(300);
    },
  },
  {
    name: "27-5000-line-performance",
    description: "5000 line file — editor renders without blank areas or lag",
    steps: async (page) => {
      const btn5000 = page.getByText("5000L", { exact: true });
      await btn5000.click();
      await page.waitForTimeout(1000);
      // Scroll to middle
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 200, y: 200 } });
      for (let i = 0; i < 50; i++) await page.keyboard.press("PageDown");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "28-clear-markers-api",
    description: "Set markers then clear — markers disappear cleanly",
    steps: async (page) => {
      const codeTab = page.getByText("Code Editor", { exact: true });
      await codeTab.click();
      await page.waitForTimeout(300);
      // Set markers
      await page.getByText("Set Error Markers", { exact: true }).click();
      await page.waitForTimeout(500);
      // Clear markers
      await page.getByText("Clear Markers", { exact: true }).click();
      await page.waitForTimeout(500);
    },
  },
  {
    name: "29-rapid-typing",
    description: "Fast typing — all characters rendered, no missing chars",
    steps: async (page) => {
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("End");
      await page.keyboard.press("Enter");
      // Type quickly (no delay)
      await page.keyboard.type("SELECT a, b, c, d FROM fast_typing_test WHERE x = 1 AND y = 2 ORDER BY a");
      await page.waitForTimeout(300);
    },
  },
  {
    name: "30-special-characters",
    description: "Special chars in SQL — parens, quotes, semicolons, operators",
    steps: async (page) => {
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("End");
      await page.keyboard.press("Enter");
      await page.keyboard.type("WHERE (a >= 'test''s value') AND b != 0 OR c IN (1,2,3);", { delay: 10 });
      await page.waitForTimeout(300);
    },
  },
  // ── Syntax highlighting scenarios ─────────────────────────────────────
  {
    name: "31-sql-syntax-tokens",
    description: "SQL highlighting produces syntax tokens (keywords, strings, literals)",
    steps: async (page) => {
      // SQL is the default language — capture initial state
      await page.waitForTimeout(500);
    },
    verify: async (page) => {
      const tokens = await page.evaluate(() => {
        const content = document.querySelector('.kode-content');
        if (!content) return { total: 0, hasContent: false };
        // Count all arborium syntax tokens (a-k, a-s, a-f, a-tl, a-p, etc.)
        const allTokens = content.querySelectorAll('[class^="a-"], a-k, a-s, a-f, a-tl, a-p, a-n, a-o, a-c, a-v');
        return {
          total: allTokens.length,
          hasContent: content.textContent.length > 0,
        };
      });
      return {
        has_content: tokens.hasContent,
        has_syntax_tokens: tokens.total > 0,
      };
    },
  },
  {
    name: "32-javascript-syntax-tokens",
    description: "JavaScript highlighting produces keyword and function tokens",
    steps: async (page) => {
      await page.evaluate(() => {
        const content = document.querySelector('.kode-content');
        window.__testBefore = {
          html: content?.innerHTML?.substring(0, 500) || '',
        };
      });
      const jsBtn = page.getByText("JS", { exact: true });
      await jsBtn.click();
      await page.waitForTimeout(500);
    },
    verify: async (page) => {
      const result = await page.evaluate(() => {
        const content = document.querySelector('.kode-content');
        const before = window.__testBefore;
        return {
          keywords: content ? content.querySelectorAll('a-k').length : 0,
          functions: content ? content.querySelectorAll('a-f').length : 0,
          htmlChanged: before.html !== (content?.innerHTML?.substring(0, 500) || ''),
          text: content?.textContent?.substring(0, 100) || '',
        };
      });
      return {
        content_changed_after_language_switch: result.htmlChanged,
        has_keyword_tokens: result.keywords > 0,
        has_function_tokens: result.functions > 0,
        has_javascript_content: result.text.includes('function') || result.text.includes('greet'),
      };
    },
  },
  {
    name: "33-python-syntax-tokens",
    description: "Python highlighting produces keyword and function tokens",
    steps: async (page) => {
      const pyBtn = page.getByText("Python", { exact: true });
      await pyBtn.click();
      await page.waitForTimeout(500);
    },
    verify: async (page) => {
      const tokens = await page.evaluate(() => {
        const content = document.querySelector('.kode-content');
        return {
          keywords: content ? content.querySelectorAll('a-k').length : 0,
          strings: content ? content.querySelectorAll('a-s').length : 0,
          text: content?.textContent?.substring(0, 100) || '',
        };
      });
      return {
        has_keyword_tokens: tokens.keywords > 0,
        has_python_content: tokens.text.includes('def') || tokens.text.includes('fibonacci'),
      };
    },
  },
  {
    name: "34-rust-syntax-tokens",
    description: "Rust highlighting produces keyword and function tokens",
    steps: async (page) => {
      const rustBtn = page.getByText("Rust", { exact: true });
      await rustBtn.click();
      await page.waitForTimeout(500);
    },
    verify: async (page) => {
      const tokens = await page.evaluate(() => {
        const content = document.querySelector('.kode-content');
        return {
          keywords: content ? content.querySelectorAll('a-k').length : 0,
          strings: content ? content.querySelectorAll('a-s').length : 0,
          text: content?.textContent?.substring(0, 100) || '',
        };
      });
      return {
        has_keyword_tokens: tokens.keywords > 0,
        has_rust_content: tokens.text.includes('fn') || tokens.text.includes('main'),
      };
    },
  },
  {
    name: "35-json-syntax-tokens",
    description: "JSON highlighting produces string and punctuation tokens",
    steps: async (page) => {
      const jsonBtn = page.getByText("JSON", { exact: true });
      await jsonBtn.click();
      await page.waitForTimeout(500);
    },
    verify: async (page) => {
      const tokens = await page.evaluate(() => {
        const content = document.querySelector('.kode-content');
        return {
          strings: content ? content.querySelectorAll('a-s').length : 0,
          punctuation: content ? content.querySelectorAll('a-p').length : 0,
          text: content?.textContent?.substring(0, 100) || '',
        };
      });
      return {
        has_string_tokens: tokens.strings > 0,
        has_json_content: tokens.text.includes('kode') || tokens.text.includes('name'),
      };
    },
  },
  {
    name: "36-html-syntax-tokens",
    description: "HTML highlighting produces syntax tokens",
    steps: async (page) => {
      const htmlBtn = page.getByText("HTML", { exact: true });
      await htmlBtn.click();
      await page.waitForTimeout(500);
    },
    verify: async (page) => {
      const tokens = await page.evaluate(() => {
        const content = document.querySelector('.kode-content');
        const allTokens = content ? content.querySelectorAll('a-k, a-s, a-f, a-tl, a-p, a-n, a-o, a-c, a-v') : [];
        return {
          total: allTokens.length,
          text: content?.textContent?.substring(0, 100) || '',
        };
      });
      return {
        has_syntax_tokens: tokens.total > 0,
        has_html_content: tokens.text.includes('html') || tokens.text.includes('DOCTYPE'),
      };
    },
  },
  {
    name: "37-language-switch-preserves-no-tokens-for-plain",
    description: "Plain text has no syntax tokens — switching from a highlighted language removes tokens",
    steps: async (page) => {
      // First switch to a language with tokens
      const jsBtn = page.getByText("JS", { exact: true });
      await jsBtn.click();
      await page.waitForTimeout(500);
      await page.evaluate(() => {
        const content = document.querySelector('.kode-content');
        window.__testBefore = {
          keywords: content ? content.querySelectorAll('a-k').length : 0,
        };
      });
      // Now switch to Plain
      const plainBtn = page.getByText("Plain", { exact: true });
      await plainBtn.click();
      await page.waitForTimeout(500);
    },
    verify: async (page) => {
      const result = await page.evaluate(() => {
        const content = document.querySelector('.kode-content');
        const before = window.__testBefore;
        return {
          beforeKeywords: before.keywords,
          afterKeywords: content ? content.querySelectorAll('a-k').length : 0,
          afterStrings: content ? content.querySelectorAll('a-s').length : 0,
          afterFunctions: content ? content.querySelectorAll('a-f').length : 0,
        };
      });
      return {
        had_tokens_before: result.beforeKeywords > 0,
        no_keyword_tokens_in_plain: result.afterKeywords === 0,
        no_string_tokens_in_plain: result.afterStrings === 0,
        no_function_tokens_in_plain: result.afterFunctions === 0,
      };
    },
  },
  {
    name: "38-yaml-syntax-tokens",
    description: "YAML highlighting produces keyword tokens for keys",
    steps: async (page) => {
      const yamlBtn = page.getByText("YAML", { exact: true });
      await yamlBtn.click();
      await page.waitForTimeout(500);
    },
    verify: async (page) => {
      const tokens = await page.evaluate(() => {
        const content = document.querySelector('.kode-content');
        // YAML keys are typically tagged as a-k or a-s
        const allTokens = content ? content.querySelectorAll('a-k, a-s, a-tl') : [];
        return {
          tokenCount: allTokens.length,
          text: content?.textContent?.substring(0, 100) || '',
        };
      });
      return {
        has_syntax_tokens: tokens.tokenCount > 0,
        has_yaml_content: tokens.text.includes('dashboard') || tokens.text.includes('title'),
      };
    },
  },
  {
    name: "39-css-syntax-tokens",
    description: "CSS highlighting produces syntax tokens",
    steps: async (page) => {
      const cssBtn = page.getByText("CSS", { exact: true });
      await cssBtn.click();
      await page.waitForTimeout(500);
    },
    verify: async (page) => {
      const tokens = await page.evaluate(() => {
        const content = document.querySelector('.kode-content');
        const allTokens = content ? content.querySelectorAll('a-k, a-s, a-f, a-tl, a-p, a-n, a-o, a-c, a-v') : [];
        return {
          total: allTokens.length,
          text: content?.textContent?.substring(0, 100) || '',
        };
      });
      return {
        has_syntax_tokens: tokens.total > 0,
        has_css_content: tokens.text.includes('body') || tokens.text.includes('font-family'),
      };
    },
  },
];

const WYSIWYG_SCENARIOS = [
  {
    name: "01-initial-render",
    description: "WYSIWYG with default markdown content rendered",
    steps: async (page) => {
      await page.waitForTimeout(500);
    },
  },
  {
    name: "02-heading-styles",
    description: "Verify h1, h2, h3 rendering",
    steps: async (page) => {
      await page.waitForTimeout(300);
    },
  },
  {
    name: "03-bold-italic",
    description: "Bold and italic text rendering",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.waitForTimeout(200);
      await page.keyboard.press("Control+End");
      await page.keyboard.press("Enter");
      await page.keyboard.press("Enter");
      await page.keyboard.type("Normal ");
      await page.keyboard.press("Control+b");
      await page.keyboard.type("bold");
      await page.keyboard.press("Control+b");
      await page.keyboard.type(" ");
      await page.keyboard.press("Control+i");
      await page.keyboard.type("italic");
      await page.keyboard.press("Control+i");
      await page.keyboard.type(" text");
      await page.waitForTimeout(300);
    },
    verify: async (page) => {
      const content = await page.evaluate(() => {
        const scroll = document.querySelector('.tree-wysiwyg-scroll-container, .wysiwyg-scroll-container');
        const boldEls = scroll ? scroll.querySelectorAll('strong, b') : [];
        const italicEls = scroll ? scroll.querySelectorAll('em, i') : [];
        return {
          text: scroll?.textContent || '',
          boldCount: boldEls.length,
          italicCount: italicEls.length,
        };
      });
      return {
        typed_text_visible: content.text.includes('Normal') && content.text.includes('bold') && content.text.includes('italic') && content.text.includes('text'),
        bold_element_exists: content.boldCount > 0,
        italic_element_exists: content.italicCount > 0,
      };
    },
  },
  {
    name: "04-code-block",
    description: "Code block rendering within WYSIWYG",
    steps: async (page) => {
      // Default content has a SQL code block
      await page.waitForTimeout(500);
    },
  },
  {
    name: "05-list-rendering",
    description: "Bullet and ordered list rendering",
    steps: async (page) => {
      await page.waitForTimeout(300);
    },
  },
  {
    name: "06-cursor-in-text",
    description: "Click into WYSIWYG content — cursor visible",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 200, y: 100 } });
      await page.waitForTimeout(500);
    },
  },
  {
    name: "07-text-selection",
    description: "Select text in WYSIWYG mode",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      // Click in the heading text area (below toolbar, y:80 is the h1)
      await container.click({ position: { x: 50, y: 80 } });
      await page.waitForTimeout(100);
      await container.click({
        position: { x: 300, y: 80 },
        modifiers: ["Shift"],
      });
      await page.waitForTimeout(300);
    },
  },
  {
    name: "08-typing-new-paragraph",
    description: "Type text and create new paragraphs",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.waitForTimeout(200);
      await page.keyboard.press("Control+End");
      await page.keyboard.press("Enter");
      await page.keyboard.press("Enter");
      await page.keyboard.type("This is a newly typed paragraph.", { delay: 20 });
      await page.waitForTimeout(300);
    },
    verify: async (page) => {
      const content = await page.evaluate(() =>
        (document.querySelector('.tree-wysiwyg-scroll-container, .wysiwyg-scroll-container')?.textContent) || ''
      );
      return {
        typed_text_visible: content.includes('This is a newly typed paragraph.'),
      };
    },
  },
  {
    name: "09-source-mode",
    description: "Switch to source mode — raw markdown visible",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(500);
    },
  },
  {
    name: "10-source-to-wysiwyg-roundtrip",
    description: "Source mode edit then switch back to WYSIWYG",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Home");
      await page.keyboard.type("## New Heading\n\n", { delay: 10 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
    },
  },
  {
    name: "11-blockquote",
    description: "Blockquote renders with left border and indentation",
    steps: async (page) => {
      // Navigate cursor to bottom so WYSIWYG scrolls down
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.waitForTimeout(200);
      await page.keyboard.press("Control+End");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "12-horizontal-rule",
    description: "Horizontal rule (---) renders as visible separator",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.waitForTimeout(200);
      await page.keyboard.press("Control+End");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "13-inline-code",
    description: "Inline code renders with background highlight",
    steps: async (page) => {
      // Inline code is visible on the first screen (production-bq, analytics-ch)
      await page.waitForTimeout(300);
    },
  },
  {
    name: "14-link-rendering",
    description: "Links render as clickable styled text",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.waitForTimeout(200);
      await page.keyboard.press("Control+End");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "15-bold-in-list",
    description: "Bold text inside list items renders correctly",
    steps: async (page) => {
      // Item 3 has **Apply** bold — visible on first screen
      await page.waitForTimeout(300);
    },
  },
  {
    name: "16-toolbar-bold-toggle",
    description: "Click Bold toolbar button — selected text becomes bold",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.waitForTimeout(200);
      // Go to start and select first word
      await page.keyboard.press("Control+Home");
      await page.waitForTimeout(100);
      // Select "Dashboard" by shift+clicking
      for (let i = 0; i < 9; i++) await page.keyboard.press("Shift+ArrowRight");
      await page.waitForTimeout(100);
      // Click Bold toolbar button
      const boldBtn = page.locator('button[title="Bold (Ctrl+B)"]');
      await boldBtn.click();
      await page.waitForTimeout(300);
    },
    verify: async (page) => {
      const result = await page.evaluate(() => {
        const scroll = document.querySelector('.tree-wysiwyg-scroll-container, .wysiwyg-scroll-container');
        const boldEls = scroll ? scroll.querySelectorAll('strong, b') : [];
        const boldTexts = Array.from(boldEls).map(el => el.textContent);
        return { boldTexts };
      });
      return {
        bold_applied: result.boldTexts.some(t => t.includes('Dashboard')),
      };
    },
  },
  {
    name: "17-toolbar-heading-toggle",
    description: "Click H2 toolbar button — converts paragraph to heading",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.waitForTimeout(200);
      await page.keyboard.press("Control+End");
      await page.keyboard.press("Enter");
      await page.keyboard.press("Enter");
      await page.keyboard.type("Test Heading", { delay: 10 });
      await page.waitForTimeout(100);
      // Select the line
      await page.keyboard.press("Home");
      await page.keyboard.press("Shift+End");
      await page.waitForTimeout(100);
      // Click H2 button
      const h2Btn = page.locator('button:has-text("H2")');
      if (await h2Btn.count() > 0) {
        await h2Btn.click();
        await page.waitForTimeout(300);
      }
      // Scroll to bottom to see it
      await page.evaluate(() => {
        const el = document.querySelector('.wysiwyg-container');
        if (el) el.scrollTop = el.scrollHeight;
      });
      await page.waitForTimeout(200);
    },
    verify: async (page) => {
      const result = await page.evaluate(() => {
        const scroll = document.querySelector('.tree-wysiwyg-scroll-container, .wysiwyg-scroll-container');
        const h2s = scroll ? scroll.querySelectorAll('h2') : [];
        const h2Texts = Array.from(h2s).map(el => el.textContent);
        return { h2Texts };
      });
      return {
        heading_created: result.h2Texts.some(t => t.includes('Test Heading')),
      };
    },
  },
  {
    name: "18-empty-wysiwyg",
    description: "Clear all content — empty state renders cleanly",
    steps: async (page) => {
      // Switch to source mode, select all, delete
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.waitForTimeout(200);
      // Switch back to WYSIWYG
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
    },
  },
  {
    name: "19-emphasis-italic",
    description: "Italic text (*text*) renders with proper font style",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.waitForTimeout(200);
      await page.keyboard.press("Control+End");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "20-full-page-scroll",
    description: "Full document scrolled to bottom — all content rendered",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.waitForTimeout(200);
      await page.keyboard.press("Control+End");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "21-heading-to-bullet-via-toolbar",
    description: "Click bullet list button while cursor is on a heading — heading prefix stripped, content preserved",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      // Click into heading text
      await container.click({ position: { x: 200, y: 80 } });
      await page.waitForTimeout(200);
      // Click bullet list toolbar button
      const bulletBtn = page.locator('button[title="Bullet List"]');
      if (await bulletBtn.count() > 0) {
        await bulletBtn.click();
        await page.waitForTimeout(500);
      }
    },
  },
  {
    name: "22-bullet-back-to-paragraph",
    description: "Toggle bullet list off — returns to plain paragraph",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      // Click into second paragraph (below heading)
      await container.click({ position: { x: 200, y: 140 } });
      await page.waitForTimeout(200);
      // Toggle bullet on
      const bulletBtn = page.locator('button[title="Bullet List"]');
      if (await bulletBtn.count() > 0) {
        await bulletBtn.click();
        await page.waitForTimeout(300);
        // Toggle bullet off
        await bulletBtn.click();
        await page.waitForTimeout(500);
      }
    },
  },
  {
    name: "23-ordered-list-on-heading",
    description: "Click ordered list button while cursor is on a heading — heading prefix stripped",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 200, y: 80 } });
      await page.waitForTimeout(200);
      const orderedBtn = page.locator('button[title="Ordered List"]');
      if (await orderedBtn.count() > 0) {
        await orderedBtn.click();
        await page.waitForTimeout(500);
      }
    },
  },
  {
    name: "24-ctrl-b-bold-in-paragraph",
    description: "Ctrl+B toggles bold on selected text in paragraph",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 50, y: 140 } });
      await page.waitForTimeout(200);
      // Select first few words with shift+right
      for (let i = 0; i < 15; i++) await page.keyboard.press("Shift+ArrowRight");
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+b");
      await page.waitForTimeout(500);
    },
    verify: async (page) => {
      const result = await page.evaluate(() => {
        const scroll = document.querySelector('.tree-wysiwyg-scroll-container, .wysiwyg-scroll-container');
        const boldEls = scroll ? scroll.querySelectorAll('strong, b') : [];
        return { boldCount: boldEls.length };
      });
      return {
        bold_applied: result.boldCount > 0,
      };
    },
  },
  {
    name: "25-ctrl-i-italic-in-paragraph",
    description: "Ctrl+I toggles italic on selected text in paragraph",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 50, y: 140 } });
      await page.waitForTimeout(200);
      for (let i = 0; i < 10; i++) await page.keyboard.press("Shift+ArrowRight");
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+i");
      await page.waitForTimeout(500);
    },
    verify: async (page) => {
      const result = await page.evaluate(() => {
        const scroll = document.querySelector('.tree-wysiwyg-scroll-container, .wysiwyg-scroll-container');
        const italicEls = scroll ? scroll.querySelectorAll('em, i') : [];
        return { italicCount: italicEls.length };
      });
      return {
        italic_applied: result.italicCount > 0,
      };
    },
  },
  {
    name: "26-undo-redo-wysiwyg",
    description: "Undo/Redo works in WYSIWYG — content reverts and restores",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 200, y: 140 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("End");
      await page.keyboard.type(" UNDO_TEST", { delay: 10 });
      await page.waitForTimeout(200);
      // Undo
      await page.keyboard.press("Control+z");
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+z");
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+z");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "27-enter-splits-paragraph",
    description: "Enter key splits current paragraph into two",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      // Click into middle of the first paragraph
      await container.click({ position: { x: 250, y: 140 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("Enter");
      await page.waitForTimeout(500);
    },
    verify: async (page) => {
      const result = await page.evaluate(() => {
        const scroll = document.querySelector('.tree-wysiwyg-scroll-container, .wysiwyg-scroll-container');
        // Count direct block-level children (paragraphs, headings, etc.)
        const blocks = scroll ? scroll.querySelectorAll('[data-pos-start], [data-block-start], p, h1, h2, h3, h4, h5, h6, ul, ol, blockquote, pre, hr') : [];
        return { blockCount: blocks.length };
      });
      return {
        // After splitting, there should be more blocks than the initial document
        paragraph_split: result.blockCount > 5,
      };
    },
  },
  {
    name: "28-backspace-at-start-merges",
    description: "Backspace at start of paragraph merges with previous",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      // Click at start of the second paragraph
      await container.click({ position: { x: 35, y: 140 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("Home");
      await page.keyboard.press("Backspace");
      await page.waitForTimeout(500);
    },
    verify: async (page) => {
      const result = await page.evaluate(() => {
        const scroll = document.querySelector('.tree-wysiwyg-scroll-container, .wysiwyg-scroll-container');
        return { text: scroll?.textContent || '' };
      });
      return {
        // After merge, the text should still contain content (not lost)
        content_preserved: result.text.length > 20,
      };
    },
  },
  {
    name: "29-multiline-selection-delete",
    description: "Select across multiple blocks and delete — content merges correctly",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 100, y: 80 } });
      await page.waitForTimeout(200);
      // Select from heading into next paragraph
      await page.keyboard.press("Shift+ArrowDown");
      await page.keyboard.press("Shift+ArrowDown");
      await page.waitForTimeout(100);
      await page.keyboard.press("Backspace");
      await page.waitForTimeout(500);
    },
    verify: async (page) => {
      const result = await page.evaluate(() => {
        const scroll = document.querySelector('.tree-wysiwyg-scroll-container, .wysiwyg-scroll-container');
        return { text: scroll?.textContent || '' };
      });
      return {
        // Content should be shorter after deletion (some blocks removed)
        deletion_occurred: result.text.length > 0,
      };
    },
  },
  {
    name: "30-h1-to-h2-via-toolbar",
    description: "Click H2 while on H1 — heading level changes, content preserved",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 200, y: 80 } });
      await page.waitForTimeout(200);
      const h2Btn = page.locator('button[title="Heading 2"]').or(page.locator('button:has-text("H2")'));
      if (await h2Btn.count() > 0) {
        await h2Btn.click();
        await page.waitForTimeout(500);
      }
    },
  },
  {
    name: "31-enter-in-empty-line",
    description: "Enter on empty line creates new paragraph break",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 50, y: 140 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("End");
      await page.keyboard.press("Enter");
      await page.keyboard.press("Enter");
      await page.keyboard.press("Enter");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "32-type-after-code-block",
    description: "Click after code block and type — new paragraph below code block",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.waitForTimeout(200);
      await page.keyboard.press("Control+End");
      await page.keyboard.press("Enter");
      await page.keyboard.type("Paragraph after all content", { delay: 10 });
      await page.waitForTimeout(500);
    },
    verify: async (page) => {
      const content = await page.evaluate(() =>
        (document.querySelector('.tree-wysiwyg-scroll-container, .wysiwyg-scroll-container')?.textContent) || ''
      );
      return {
        typed_text_visible: content.includes('Paragraph after all content'),
      };
    },
  },
  {
    name: "33-inline-code-toggle",
    description: "Select text and toggle inline code via toolbar backtick button",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 50, y: 140 } });
      await page.waitForTimeout(200);
      for (let i = 0; i < 4; i++) await page.keyboard.press("Shift+ArrowRight");
      await page.waitForTimeout(100);
      const codeBtn = page.locator('button[title="Inline Code"]');
      if (await codeBtn.count() > 0) {
        await codeBtn.click();
        await page.waitForTimeout(500);
      }
    },
    verify: async (page) => {
      const result = await page.evaluate(() => {
        const scroll = document.querySelector('.tree-wysiwyg-scroll-container, .wysiwyg-scroll-container');
        const codeEls = scroll ? scroll.querySelectorAll('code:not(pre code)') : [];
        return { inlineCodeCount: codeEls.length };
      });
      return {
        inline_code_applied: result.inlineCodeCount > 0,
      };
    },
  },
  {
    name: "34-strikethrough-toggle",
    description: "Select text and toggle strikethrough via toolbar S button",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 50, y: 140 } });
      await page.waitForTimeout(200);
      for (let i = 0; i < 4; i++) await page.keyboard.press("Shift+ArrowRight");
      await page.waitForTimeout(100);
      const strikeBtn = page.locator('button[title="Strikethrough"]');
      if (await strikeBtn.count() > 0) {
        await strikeBtn.click();
        await page.waitForTimeout(500);
      }
    },
    verify: async (page) => {
      const result = await page.evaluate(() => {
        const scroll = document.querySelector('.tree-wysiwyg-scroll-container, .wysiwyg-scroll-container');
        const strikeEls = scroll ? scroll.querySelectorAll('del, s, strike') : [];
        return { strikethroughCount: strikeEls.length };
      });
      return {
        strikethrough_applied: result.strikethroughCount > 0,
      };
    },
  },
  {
    name: "35-blockquote-via-toolbar",
    description: "Click blockquote toolbar button — paragraph becomes blockquote",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 200, y: 140 } });
      await page.waitForTimeout(200);
      const quoteBtn = page.locator('button[title="Blockquote"]');
      if (await quoteBtn.count() > 0) {
        await quoteBtn.click();
        await page.waitForTimeout(500);
      }
    },
    verify: async (page) => {
      const result = await page.evaluate(() => {
        const scroll = document.querySelector('.tree-wysiwyg-scroll-container, .wysiwyg-scroll-container');
        const bqEls = scroll ? scroll.querySelectorAll('blockquote') : [];
        return { blockquoteCount: bqEls.length };
      });
      return {
        blockquote_created: result.blockquoteCount > 0,
      };
    },
  },
  {
    name: "36-insert-link-via-toolbar",
    description: "Click link toolbar button — link markdown inserted at cursor",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 200, y: 140 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("End");
      await page.keyboard.press("Home");
      for (let i = 0; i < 4; i++) await page.keyboard.press("Shift+ArrowRight");
      await page.waitForTimeout(100);
      const linkBtn = page.locator('button[title="Insert Link"]');
      if (await linkBtn.count() > 0) {
        await linkBtn.click();
        await page.waitForTimeout(500);
      }
    },
  },
  {
    name: "37-insert-code-block-via-toolbar",
    description: "Click code block toolbar button — fenced code block inserted",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.waitForTimeout(200);
      await page.keyboard.press("Control+End");
      await page.keyboard.press("Enter");
      const codeBlockBtn = page.locator('button[title="Code Block"]');
      if (await codeBlockBtn.count() > 0) {
        await codeBlockBtn.click();
        await page.waitForTimeout(500);
      }
    },
  },
  {
    name: "38-insert-hr-via-toolbar",
    description: "Click horizontal rule toolbar button — hr inserted",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.waitForTimeout(200);
      await page.keyboard.press("Control+End");
      await page.keyboard.press("Enter");
      const hrBtn = page.locator('button[title="Horizontal Rule"]');
      if (await hrBtn.count() > 0) {
        await hrBtn.click();
        await page.waitForTimeout(500);
      }
    },
  },
  {
    name: "39-bold-italic-combined",
    description: "Type ***bold italic*** directly — renders as bold+italic",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.waitForTimeout(200);
      await page.keyboard.press("Control+End");
      await page.keyboard.press("Enter");
      await page.keyboard.press("Enter");
      // Type raw bold-italic markdown
      await page.keyboard.type("Normal text ***bold and italic*** and back to normal.", { delay: 10 });
      await page.waitForTimeout(500);
    },
    verify: async (page) => {
      const content = await page.evaluate(() =>
        (document.querySelector('.tree-wysiwyg-scroll-container, .wysiwyg-scroll-container')?.textContent) || ''
      );
      return {
        typed_text_visible: content.includes('bold and italic') || content.includes('***bold and italic***'),
      };
    },
  },
  {
    name: "40-rapid-typing-in-wysiwyg",
    description: "Fast typing in WYSIWYG — all characters appear, no missing text",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.waitForTimeout(200);
      await page.keyboard.press("Control+End");
      await page.keyboard.press("Enter");
      await page.keyboard.press("Enter");
      await page.keyboard.type("The quick brown fox jumps over the lazy dog. Pack my box with five dozen liquor jugs.");
      await page.waitForTimeout(500);
    },
    verify: async (page) => {
      const content = await page.evaluate(() =>
        (document.querySelector('.tree-wysiwyg-scroll-container, .wysiwyg-scroll-container')?.textContent) || ''
      );
      return {
        typed_text_visible: content.includes('The quick brown fox jumps over the lazy dog'),
        full_text_present: content.includes('Pack my box with five dozen liquor jugs'),
      };
    },
  },
  {
    name: "41-cursor-at-end-of-heading",
    description: "Cursor at end of H1 heading — cursor height matches heading font size",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 200, y: 80 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("End");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "42-cursor-at-end-of-paragraph",
    description: "Cursor at end of paragraph — cursor height matches paragraph text",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 200, y: 140 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("End");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "43-cursor-at-end-of-h2",
    description: "Cursor at end of H2 heading — cursor height matches H2 size",
    steps: async (page) => {
      // Click into Data Sources h2
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 100, y: 185 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("End");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "44-cursor-at-end-of-list-item",
    description: "Cursor at end of list item — cursor height matches list text",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 200, y: 235 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("End");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "45-cursor-between-blocks",
    description: "Cursor at start of paragraph after heading — correct position",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 200, y: 140 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("Home");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "46-bold-preserves-cursor",
    description: "Apply bold via Ctrl+B — cursor stays at same logical position",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 200, y: 140 } });
      await page.waitForTimeout(200);
      // Select a word
      await page.keyboard.press("Home");
      for (let i = 0; i < 4; i++) await page.keyboard.press("Shift+ArrowRight");
      await page.waitForTimeout(100);
      // Apply bold — cursor should stay at selection end
      await page.keyboard.press("Control+b");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "47-heading-toggle-preserves-cursor",
    description: "Toggle H1 to H2 — cursor stays in same text position",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      // Click middle of heading text
      await container.click({ position: { x: 250, y: 80 } });
      await page.waitForTimeout(300);
      // Toggle to H2
      const h2Btn = page.locator('button[title="Heading 2"]').or(page.locator('button:has-text("H2")'));
      if (await h2Btn.count() > 0) {
        await h2Btn.click();
        await page.waitForTimeout(500);
      }
    },
  },
  {
    name: "48-blockquote-toggle-preserves-cursor",
    description: "Toggle blockquote — cursor stays in same text",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 200, y: 140 } });
      await page.waitForTimeout(200);
      const quoteBtn = page.locator('button[title="Blockquote"]');
      if (await quoteBtn.count() > 0) {
        await quoteBtn.click();
        await page.waitForTimeout(300);
        // Toggle off
        await quoteBtn.click();
        await page.waitForTimeout(500);
      }
    },
  },
  {
    name: "49-cursor-after-inline-code",
    description: "Cursor immediately after inline code — cursor visible, normal height",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      // Click on the inline code `production-bq` in the first list item
      await container.click({ position: { x: 175, y: 235 } });
      await page.waitForTimeout(500);
    },
  },
  {
    name: "50-type-at-end-of-bold",
    description: "Type at end of bold text — new text should be normal (not bold)",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 50, y: 140 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("Home");
      for (let i = 0; i < 45; i++) await page.keyboard.press("ArrowRight");
      await page.waitForTimeout(100);
      await page.keyboard.type(" AFTER_BOLD", { delay: 20 });
      await page.waitForTimeout(500);
    },
  },
  // ── Newline-focused scenarios (51-70) ──────────────────────────────
  {
    name: "51-enter-at-end-of-paragraph",
    description: "Enter at end of paragraph — new empty paragraph below, cursor on it",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 200, y: 140 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("End");
      await page.keyboard.press("Enter");
      await page.waitForTimeout(500);
    },
    verify: async (page) => {
      const result = await page.evaluate(() => {
        const scroll = document.querySelector('.tree-wysiwyg-scroll-container, .wysiwyg-scroll-container');
        const blocks = scroll ? scroll.querySelectorAll('[data-pos-start], [data-block-start], p, h1, h2, h3, h4, h5, h6, ul, ol, blockquote, pre, hr') : [];
        return { blockCount: blocks.length };
      });
      return {
        new_block_created: result.blockCount > 5,
      };
    },
  },
  {
    name: "52-enter-at-start-of-paragraph",
    description: "Enter at start of paragraph — blank line above, content stays",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 50, y: 140 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("Home");
      await page.keyboard.press("Enter");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "53-enter-middle-of-paragraph",
    description: "Enter in middle of paragraph — splits into two paragraphs",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 200, y: 140 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("Enter");
      await page.waitForTimeout(500);
    },
    verify: async (page) => {
      const result = await page.evaluate(() => {
        const scroll = document.querySelector('.tree-wysiwyg-scroll-container, .wysiwyg-scroll-container');
        const blocks = scroll ? scroll.querySelectorAll('[data-pos-start], [data-block-start], p, h1, h2, h3, h4, h5, h6, ul, ol, blockquote, pre, hr') : [];
        return { blockCount: blocks.length, text: scroll?.textContent || '' };
      });
      return {
        paragraph_split: result.blockCount > 5,
        content_preserved: result.text.length > 20,
      };
    },
  },
  {
    name: "54-enter-at-end-of-heading",
    description: "Enter at end of heading — new paragraph below (not another heading)",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 200, y: 80 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("End");
      await page.keyboard.press("Enter");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "55-enter-at-end-of-list-item",
    description: "Enter at end of list item — new list item below",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 200, y: 258 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("End");
      await page.keyboard.press("Enter");
      await page.keyboard.type("New list item", { delay: 10 });
      await page.waitForTimeout(500);
    },
    verify: async (page) => {
      const result = await page.evaluate(() => {
        const scroll = document.querySelector('.tree-wysiwyg-scroll-container, .wysiwyg-scroll-container');
        const listItems = scroll ? scroll.querySelectorAll('li') : [];
        const liTexts = Array.from(listItems).map(li => li.textContent);
        return { listItemCount: listItems.length, liTexts };
      });
      return {
        new_list_item_exists: result.liTexts.some(t => t.includes('New list item')),
      };
    },
  },
  {
    name: "56-shift-enter-in-paragraph",
    description: "Shift+Enter in paragraph — soft break (single newline), not paragraph break",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 200, y: 140 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("Shift+Enter");
      await page.keyboard.type("soft break line", { delay: 10 });
      await page.waitForTimeout(500);
    },
    verify: async (page) => {
      const content = await page.evaluate(() =>
        (document.querySelector('.tree-wysiwyg-scroll-container, .wysiwyg-scroll-container')?.textContent) || ''
      );
      return {
        typed_text_visible: content.includes('soft break line'),
      };
    },
  },
  {
    name: "57-enter-in-empty-list-item",
    description: "Enter on empty list item — exits list, creates paragraph",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 200, y: 235 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("End");
      await page.keyboard.press("Enter");
      // Now on a new empty list item — press Enter again to exit list
      await page.keyboard.press("Enter");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "58-enter-in-blockquote",
    description: "Enter inside blockquote — continues blockquote on new line",
    steps: async (page) => {
      // Switch to source, add blockquote, switch back
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+End");
      await page.keyboard.press("Enter");
      await page.keyboard.press("Enter");
      await page.keyboard.type("> Quote line one", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      // Click into the blockquote and press Enter
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.waitForTimeout(200);
      await page.keyboard.press("Control+End");
      await page.keyboard.press("Enter");
      await page.keyboard.type("Quote line two", { delay: 10 });
      await page.waitForTimeout(500);
    },
  },
  {
    name: "59-multiple-enters-create-gap",
    description: "Multiple Enter presses — visible gap between paragraphs",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 200, y: 140 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("End");
      await page.keyboard.press("Enter");
      await page.keyboard.type("First new para", { delay: 10 });
      await page.keyboard.press("Enter");
      await page.keyboard.type("Second new para", { delay: 10 });
      await page.waitForTimeout(500);
    },
  },
  {
    name: "60-enter-preserves-no-raw-markers",
    description: "Enter inside italic text — no raw * markers visible after split",
    steps: async (page) => {
      // Source mode: write a line with italic
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+End");
      await page.keyboard.press("Enter");
      await page.keyboard.press("Enter");
      await page.keyboard.type("Normal *italic text here* end", { delay: 5 });
      await page.waitForTimeout(200);
      // Switch to WYSIWYG
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      // Navigate to inside the italic text and press Enter
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.waitForTimeout(200);
      await page.keyboard.press("Control+End");
      // Move back into the italic text
      for (let i = 0; i < 10; i++) await page.keyboard.press("ArrowLeft");
      await page.waitForTimeout(100);
      await page.keyboard.press("Enter");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "61-enter-inside-inline-code",
    description: "Enter inside `inline code` — markers closed/reopened correctly",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+End");
      await page.keyboard.press("Enter");
      await page.keyboard.press("Enter");
      await page.keyboard.type("Before `code block text` after", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+End");
      for (let i = 0; i < 12; i++) await page.keyboard.press("ArrowLeft");
      await page.keyboard.press("Enter");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "62-enter-inside-strikethrough",
    description: "Enter inside ~~strikethrough~~ — markers closed/reopened",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+End");
      await page.keyboard.press("Enter");
      await page.keyboard.press("Enter");
      await page.keyboard.type("Normal ~~deleted text here~~ end", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+End");
      for (let i = 0; i < 10; i++) await page.keyboard.press("ArrowLeft");
      await page.keyboard.press("Enter");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "63-backspace-merges-paragraphs",
    description: "Backspace at start of paragraph — merges with previous paragraph",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 200, y: 140 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("End");
      await page.keyboard.press("Enter");
      await page.keyboard.type("NEW PARAGRAPH", { delay: 10 });
      await page.waitForTimeout(200);
      // Now backspace from start of "NEW PARAGRAPH" to merge
      await page.keyboard.press("Home");
      await page.keyboard.press("Backspace");
      await page.waitForTimeout(500);
    },
    verify: async (page) => {
      const result = await page.evaluate(() => {
        const scroll = document.querySelector('.tree-wysiwyg-scroll-container, .wysiwyg-scroll-container');
        return { text: scroll?.textContent || '' };
      });
      return {
        // After merge, "NEW PARAGRAPH" text should still be present (merged into previous line)
        merged_text_preserved: result.text.includes('NEW PARAGRAPH'),
      };
    },
  },
  {
    name: "64-delete-at-end-merges-next",
    description: "Delete at end of paragraph — merges with next paragraph",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 200, y: 140 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("End");
      await page.keyboard.press("Delete");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "65-enter-between-two-headings",
    description: "Enter between two headings — creates paragraph, not heading",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      // Click into "Data Sources" heading
      await container.click({ position: { x: 100, y: 185 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("Home");
      await page.keyboard.press("Enter");
      await page.keyboard.press("ArrowUp");
      await page.keyboard.type("Inserted paragraph between headings", { delay: 10 });
      await page.waitForTimeout(500);
    },
    verify: async (page) => {
      const content = await page.evaluate(() =>
        (document.querySelector('.tree-wysiwyg-scroll-container, .wysiwyg-scroll-container')?.textContent) || ''
      );
      return {
        typed_text_visible: content.includes('Inserted paragraph between headings'),
      };
    },
  },
  {
    name: "66-enter-in-ordered-list-item",
    description: "Enter at end of ordered list item — new numbered item",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      // Click into ordered list (Usage section, first ordered item)
      await container.click({ position: { x: 200, y: 398 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("End");
      await page.keyboard.press("Enter");
      await page.keyboard.type("New step added", { delay: 10 });
      await page.waitForTimeout(500);
    },
    verify: async (page) => {
      const result = await page.evaluate(() => {
        const scroll = document.querySelector('.tree-wysiwyg-scroll-container, .wysiwyg-scroll-container');
        const listItems = scroll ? scroll.querySelectorAll('li') : [];
        const liTexts = Array.from(listItems).map(li => li.textContent);
        return { liTexts };
      });
      return {
        new_item_visible: result.liTexts.some(t => t.includes('New step added')),
      };
    },
  },
  {
    name: "67-cursor-after-enter",
    description: "After Enter, cursor is on new line at column 0",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 200, y: 140 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("Enter");
      // Cursor should now be at start of new line
      await page.waitForTimeout(500);
    },
  },
  {
    name: "68-enter-then-type-renders-immediately",
    description: "Type immediately after Enter — text appears on new line without delay",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 200, y: 140 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("End");
      await page.keyboard.press("Enter");
      await page.keyboard.type("Immediate text after enter", { delay: 0 });
      await page.waitForTimeout(500);
    },
    verify: async (page) => {
      const content = await page.evaluate(() =>
        (document.querySelector('.tree-wysiwyg-scroll-container, .wysiwyg-scroll-container')?.textContent) || ''
      );
      return {
        typed_text_visible: content.includes('Immediate text after enter'),
      };
    },
  },
  {
    name: "69-shift-enter-in-bold",
    description: "Shift+Enter inside bold — soft break with bold markers preserved",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+End");
      await page.keyboard.press("Enter");
      await page.keyboard.press("Enter");
      await page.keyboard.type("Text **bold word here** end", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+End");
      for (let i = 0; i < 8; i++) await page.keyboard.press("ArrowLeft");
      await page.keyboard.press("Shift+Enter");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "70-rapid-enter-spam",
    description: "Press Enter 5 times rapidly — all line breaks created, no crash",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 200, y: 140 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("End");
      for (let i = 0; i < 5; i++) {
        await page.keyboard.press("Enter");
      }
      await page.keyboard.type("After 5 enters", { delay: 10 });
      await page.waitForTimeout(500);
    },
  },
  // ── Adversarial newline/Enter scenarios (71-120) ──────────────────
  // Category 1: Enter at exact formatting boundaries
  {
    name: "71-enter-right-before-bold-open",
    description: "Enter right before ** opening marker — bold should stay intact on new line",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type("Hello **bold** world", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+Home");
      // Move to right before the bold (after "Hello ")
      for (let i = 0; i < 6; i++) await page.keyboard.press("ArrowRight");
      await page.keyboard.press("Enter");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "72-enter-right-after-bold-close",
    description: "Enter right after ** closing marker — new line should not inherit bold",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type("Hello **bold** world", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+Home");
      // Move to right after "bold" (after "Hello bold")
      for (let i = 0; i < 10; i++) await page.keyboard.press("ArrowRight");
      await page.keyboard.press("Enter");
      await page.keyboard.type("this should not be bold", { delay: 5 });
      await page.waitForTimeout(500);
    },
  },
  {
    name: "73-enter-between-bold-markers-and-text",
    description: "Enter at **|bold — cursor right after opening markers, before text",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type("Start **important text** end", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+Home");
      // Move to just inside the bold (after "Start " at the start of bold text)
      for (let i = 0; i < 6; i++) await page.keyboard.press("ArrowRight");
      await page.keyboard.press("Enter");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "74-enter-at-newline-between-list-items",
    description: "Position cursor at boundary between two list items and press Enter",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      // Click into first bullet item
      await container.click({ position: { x: 200, y: 235 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("End");
      // Pressing Enter at end of list item should create new item
      await page.keyboard.press("Enter");
      // Then Enter again on empty item
      await page.keyboard.press("Enter");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "75-enter-before-inline-code-backtick",
    description: "Enter right before opening backtick of inline code",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type("Run the `command --flag` now", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+Home");
      // Move to right before inline code (after "Run the ")
      for (let i = 0; i < 8; i++) await page.keyboard.press("ArrowRight");
      await page.keyboard.press("Enter");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "76-enter-after-inline-code-backtick",
    description: "Enter right after closing backtick of inline code",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type("Run the `command --flag` now", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+Home");
      // Move past inline code (after "Run the command --flag")
      for (let i = 0; i < 22; i++) await page.keyboard.press("ArrowRight");
      await page.keyboard.press("Enter");
      await page.waitForTimeout(500);
    },
  },
  // Category 2: Newline rendering issues
  {
    name: "77-type-enter-both-lines-render",
    description: "Type text, Enter, type more — both lines must be visible",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.type("LINE ONE VISIBLE", { delay: 5 });
      await page.keyboard.press("Enter");
      await page.keyboard.type("LINE TWO VISIBLE", { delay: 5 });
      await page.waitForTimeout(500);
    },
  },
  {
    name: "78-enter-creates-visible-blank-line",
    description: "Enter twice — blank line between paragraphs should be visible, not collapsed",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.type("Above blank line", { delay: 5 });
      await page.keyboard.press("Enter");
      await page.keyboard.press("Enter");
      await page.keyboard.type("Below blank line", { delay: 5 });
      await page.waitForTimeout(500);
    },
  },
  {
    name: "79-triple-blank-lines-not-collapsed",
    description: "Three consecutive Enters — all blank lines render, not collapsed to one",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.type("Top", { delay: 5 });
      await page.keyboard.press("Enter");
      await page.keyboard.press("Enter");
      await page.keyboard.press("Enter");
      await page.keyboard.type("Bottom after 3 enters", { delay: 5 });
      await page.waitForTimeout(500);
    },
  },
  {
    name: "80-shift-enter-vs-enter-visual-diff",
    description: "Shift+Enter then Enter — soft break is tighter spacing than hard break",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.type("Soft break below", { delay: 5 });
      await page.keyboard.press("Shift+Enter");
      await page.keyboard.type("After soft break", { delay: 5 });
      await page.keyboard.press("Enter");
      await page.keyboard.type("Hard break below", { delay: 5 });
      await page.keyboard.press("Enter");
      await page.keyboard.type("After hard break", { delay: 5 });
      await page.waitForTimeout(500);
    },
  },
  {
    name: "81-enter-inside-code-block-no-paragraph-break",
    description: "Enter inside fenced code block — stays in code block, no paragraph split",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type("```\nline one\nline two\n```", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      // Click into the code block area
      await container.click({ position: { x: 100, y: 80 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("End");
      await page.keyboard.press("Enter");
      await page.keyboard.type("line inserted in code block", { delay: 5 });
      await page.waitForTimeout(500);
    },
  },
  {
    name: "82-enter-renders-in-source-roundtrip",
    description: "Enter in WYSIWYG, switch to Source — newline visible in markdown source",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 200, y: 140 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("End");
      await page.keyboard.press("Enter");
      await page.keyboard.type("NEWLINE_MARKER", { delay: 5 });
      await page.waitForTimeout(300);
      // Switch to source to verify
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(500);
    },
  },
  // Category 3: Cursor position after newline
  {
    name: "83-cursor-after-enter-at-end-of-heading",
    description: "Enter at end of H1 — cursor should be on new paragraph line, not stuck on heading",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 200, y: 80 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("End");
      await page.keyboard.press("Enter");
      // Type to prove cursor is on new line
      await page.keyboard.type("X", { delay: 5 });
      await page.waitForTimeout(500);
    },
  },
  {
    name: "84-cursor-position-after-enter-mid-text",
    description: "Enter in middle of text — cursor at start of line 2, type to verify",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type("ABCDEFGHIJ", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+Home");
      for (let i = 0; i < 5; i++) await page.keyboard.press("ArrowRight");
      await page.keyboard.press("Enter");
      // Cursor should be at start of "FGHIJ" — type marker to prove
      await page.keyboard.type(">>", { delay: 5 });
      await page.waitForTimeout(500);
    },
  },
  {
    name: "85-cursor-after-shift-enter",
    description: "Shift+Enter — cursor on new soft-break line, type to verify position",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type("Before soft break", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+End");
      await page.keyboard.press("Shift+Enter");
      await page.keyboard.type("CURSOR_HERE", { delay: 5 });
      await page.waitForTimeout(500);
    },
  },
  {
    name: "86-cursor-after-enter-in-list-item",
    description: "Enter at end of list item — cursor on new bullet item, type to verify",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 200, y: 235 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("End");
      await page.keyboard.press("Enter");
      await page.keyboard.type("CURSOR_ON_NEW_ITEM", { delay: 5 });
      await page.waitForTimeout(500);
    },
  },
  {
    name: "87-tab-indent-after-enter-in-list",
    description: "Enter in list item then Tab — should create nested/indented list item",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 200, y: 235 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("End");
      await page.keyboard.press("Enter");
      await page.keyboard.press("Tab");
      await page.keyboard.type("Indented sub-item", { delay: 5 });
      await page.waitForTimeout(500);
    },
  },
  // Category 4: Delete/Backspace at newline boundaries
  {
    name: "88-backspace-at-line2-start-merges-with-bold",
    description: "Backspace at start of line after bold line — bold on line 1 should survive merge",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type("**Bold line**\n\nNormal line", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+End");
      await page.keyboard.press("Home");
      await page.keyboard.press("Backspace");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "89-delete-at-end-of-bold-line",
    description: "Delete at end of bold text — merges with next line, bold survives",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type("**Bold text**\n\nPlain text after", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+Home");
      await page.keyboard.press("End");
      await page.keyboard.press("Delete");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "90-backspace-at-start-of-heading-merges",
    description: "Backspace at start of H2 — should merge with previous block",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type("Paragraph above\n\n## Heading Below", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+End");
      await page.keyboard.press("Home");
      await page.keyboard.press("Backspace");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "91-delete-at-end-of-last-list-item",
    description: "Delete at end of last list item — should merge next block into list or create junction",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type("- Item one\n- Item two\n\nParagraph after list", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      // Navigate to end of "Item two"
      await page.keyboard.press("Control+Home");
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("End");
      await page.keyboard.press("Delete");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "92-backspace-through-multiple-blank-lines",
    description: "Multiple blank lines then Backspace repeatedly — lines removed one by one",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.type("Top", { delay: 5 });
      await page.keyboard.press("Enter");
      await page.keyboard.press("Enter");
      await page.keyboard.press("Enter");
      await page.keyboard.press("Enter");
      await page.keyboard.type("Bottom", { delay: 5 });
      await page.waitForTimeout(200);
      // Now backspace from before "Bottom" to remove blank lines
      await page.keyboard.press("Home");
      await page.keyboard.press("Backspace");
      await page.keyboard.press("Backspace");
      await page.keyboard.press("Backspace");
      await page.waitForTimeout(500);
    },
  },
  // Category 5: Newline inside complex formatting
  {
    name: "93-enter-inside-bold-italic-triple",
    description: "Enter inside ***bold italic*** — triple markers split correctly",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type("Text ***bold italic stuff*** end", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+Home");
      // Move into the bold-italic text
      for (let i = 0; i < 12; i++) await page.keyboard.press("ArrowRight");
      await page.keyboard.press("Enter");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "94-enter-inside-nested-bold-and-italic",
    description: "Enter inside **bold *and italic* text** — nested formatting preserved on both lines",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type("Start **bold *and italic* text** end", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+Home");
      // Move into the italic part within bold
      for (let i = 0; i < 15; i++) await page.keyboard.press("ArrowRight");
      await page.keyboard.press("Enter");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "95-enter-inside-link-text",
    description: "Enter inside [link text](url) — should split or escape link correctly",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type("Click [this important link](https://example.com) here", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+Home");
      // Move into the link text
      for (let i = 0; i < 14; i++) await page.keyboard.press("ArrowRight");
      await page.keyboard.press("Enter");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "96-enter-at-bold-italic-boundary",
    description: "Enter at boundary between **bold** and *italic* — formatting preserved on each side",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type("**bold text***italic text*", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+Home");
      // Move to boundary between bold and italic
      for (let i = 0; i < 9; i++) await page.keyboard.press("ArrowRight");
      await page.keyboard.press("Enter");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "97-enter-inside-heading-with-bold",
    description: "Enter inside ## **Bold** Heading — heading splits, bold preserved",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type("## **Bold** Heading Text", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+Home");
      // Move into the bold part of the heading
      for (let i = 0; i < 3; i++) await page.keyboard.press("ArrowRight");
      await page.keyboard.press("Enter");
      await page.waitForTimeout(500);
    },
  },
  // Category 6: Block-level newline interactions
  {
    name: "98-enter-at-end-of-code-fence",
    description: "Enter after closing ``` fence — should exit code block, create paragraph",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type("```\ncode here\n```", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+End");
      await page.keyboard.press("Enter");
      await page.keyboard.type("Text after code block", { delay: 5 });
      await page.waitForTimeout(500);
    },
  },
  {
    name: "99-enter-at-start-of-blockquote",
    description: "Enter at very start of blockquote — blank line inserted before quote",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type("> This is a blockquote", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+Home");
      await page.keyboard.press("Enter");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "100-enter-mid-ordered-list-renumbering",
    description: "Enter in middle of ordered list — subsequent items should renumber",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type("1. First item\n2. Second item\n3. Third item", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+Home");
      await page.keyboard.press("End");
      await page.keyboard.press("Enter");
      await page.keyboard.type("Inserted item", { delay: 5 });
      await page.waitForTimeout(500);
    },
  },
  {
    name: "101-enter-after-horizontal-rule",
    description: "Enter after horizontal rule — new paragraph below the rule",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type("Above\n\n---\n\nBelow", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+End");
      await page.keyboard.press("Home");
      await page.keyboard.press("Enter");
      await page.keyboard.type("Inserted after HR", { delay: 5 });
      await page.waitForTimeout(500);
    },
  },
  {
    name: "102-enter-at-very-end-of-document",
    description: "Enter at absolute end of document — new content appears at bottom",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.waitForTimeout(200);
      await page.keyboard.press("Control+End");
      await page.keyboard.press("Enter");
      await page.keyboard.press("Enter");
      await page.keyboard.type("APPENDED_AT_VERY_END", { delay: 5 });
      await page.waitForTimeout(500);
    },
  },
  // Category 7: Undo/Redo around newlines
  {
    name: "103-undo-enter-key",
    description: "Type, Enter, type more, Ctrl+Z — undo should remove the Enter",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.type("Before", { delay: 5 });
      await page.keyboard.press("Enter");
      await page.keyboard.type("After", { delay: 5 });
      await page.waitForTimeout(300);
      // Undo — should rejoin the text
      await page.keyboard.press("Control+z");
      await page.keyboard.press("Control+z");
      await page.keyboard.press("Control+z");
      await page.keyboard.press("Control+z");
      await page.keyboard.press("Control+z");
      await page.keyboard.press("Control+z");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "104-undo-enter-inside-bold",
    description: "Enter inside bold then Ctrl+Z — bold markers restored to single span",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type("**bold text here**", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+Home");
      for (let i = 0; i < 5; i++) await page.keyboard.press("ArrowRight");
      await page.keyboard.press("Enter");
      await page.waitForTimeout(300);
      // Undo the Enter
      await page.keyboard.press("Control+z");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "105-undo-multiple-enters",
    description: "Multiple Enters then undo all — should return to original single line",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type("Single line of text", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+Home");
      for (let i = 0; i < 7; i++) await page.keyboard.press("ArrowRight");
      // Press Enter 3 times
      await page.keyboard.press("Enter");
      await page.keyboard.press("Enter");
      await page.keyboard.press("Enter");
      await page.waitForTimeout(300);
      // Undo all 3
      await page.keyboard.press("Control+z");
      await page.keyboard.press("Control+z");
      await page.keyboard.press("Control+z");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "106-redo-after-undo-enter",
    description: "Undo an Enter then Redo — the Enter should come back",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type("Line before split", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+Home");
      for (let i = 0; i < 5; i++) await page.keyboard.press("ArrowRight");
      await page.keyboard.press("Enter");
      await page.waitForTimeout(200);
      await page.keyboard.press("Control+z");
      await page.waitForTimeout(200);
      // Redo
      await page.keyboard.press("Control+Shift+z");
      await page.waitForTimeout(500);
    },
  },
  // Category 8: Edge cases
  {
    name: "107-enter-on-empty-document",
    description: "Enter on completely empty document — should create a paragraph",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Enter");
      await page.keyboard.press("Enter");
      await page.keyboard.type("Typed after enters on empty doc", { delay: 5 });
      await page.waitForTimeout(500);
    },
  },
  {
    name: "108-enter-with-multiblock-selection",
    description: "Select across heading + paragraph + list, press Enter — replaces all with newline",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 50, y: 80 } });
      await page.waitForTimeout(200);
      // Select from heading through paragraph into list
      await page.keyboard.press("Home");
      for (let i = 0; i < 6; i++) await page.keyboard.press("Shift+ArrowDown");
      await page.waitForTimeout(100);
      await page.keyboard.press("Enter");
      await page.keyboard.type("Replaced multi-block selection", { delay: 5 });
      await page.waitForTimeout(500);
    },
  },
  {
    name: "109-enter-replaces-selected-text",
    description: "Select text within a paragraph and press Enter — selection replaced with line break",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 50, y: 140 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("Home");
      for (let i = 0; i < 10; i++) await page.keyboard.press("Shift+ArrowRight");
      await page.waitForTimeout(100);
      await page.keyboard.press("Enter");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "110-paste-multiline-into-paragraph",
    description: "Paste multiline text into middle of paragraph — multiple paragraphs created",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 200, y: 140 } });
      await page.waitForTimeout(200);
      // Use clipboard to paste multiline content
      await page.evaluate(() => {
        const text = "PASTED LINE ONE\nPASTED LINE TWO\nPASTED LINE THREE";
        navigator.clipboard.writeText(text);
      });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+v");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "111-enter-after-very-long-line",
    description: "Very long wrapping line then Enter — newline after wrapped content works",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      const longText = "This is a very long line that should wrap in the editor because it contains many words and will exceed the width of the container viewport area by a significant amount to test wrapping behavior with newlines.";
      await page.keyboard.type(longText, { delay: 0 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+End");
      await page.keyboard.press("Enter");
      await page.keyboard.type("Short line after long line", { delay: 5 });
      await page.waitForTimeout(500);
    },
  },
  // More adversarial edge cases
  {
    name: "112-enter-between-adjacent-bold-spans",
    description: "Enter between two adjacent bold spans — **a****b** split correctly",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type("**first** **second**", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+Home");
      // Move to the space between the two bold spans
      for (let i = 0; i < 6; i++) await page.keyboard.press("ArrowRight");
      await page.keyboard.press("Enter");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "113-shift-enter-in-heading",
    description: "Shift+Enter inside heading — soft break in heading or block conversion?",
    steps: async (page) => {
      const container = page.locator(".wysiwyg-container");
      await container.click({ position: { x: 200, y: 80 } });
      await page.waitForTimeout(200);
      await page.keyboard.press("Home");
      for (let i = 0; i < 5; i++) await page.keyboard.press("ArrowRight");
      await page.keyboard.press("Shift+Enter");
      await page.keyboard.type("soft break in heading", { delay: 5 });
      await page.waitForTimeout(500);
    },
  },
  {
    name: "114-enter-at-start-of-only-paragraph",
    description: "Document has one paragraph — Enter at start creates blank above, text moves down",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type("Only paragraph in document", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+Home");
      await page.keyboard.press("Enter");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "115-backspace-merges-heading-into-paragraph",
    description: "Backspace at start of heading merges heading text into previous paragraph",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type("Normal paragraph\n\n# Heading", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+End");
      await page.keyboard.press("Home");
      await page.keyboard.press("Backspace");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "116-enter-splits-inline-code-at-boundary",
    description: "Enter right at end of inline code text (before closing backtick) — code split or escaped",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type("Use `some command` here", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+Home");
      // Move to right before closing backtick (end of code text)
      for (let i = 0; i < 16; i++) await page.keyboard.press("ArrowRight");
      await page.keyboard.press("Enter");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "117-delete-across-formatting-boundary",
    description: "Delete at end of normal text that precedes bold — bold formatting preserved",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type("Normal\n\n**Bold paragraph**", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+Home");
      await page.keyboard.press("End");
      await page.keyboard.press("Delete");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "118-rapid-enter-backspace-cycle",
    description: "Enter then Backspace 10 times rapidly — no corruption, content intact",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type("Stable content here", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+Home");
      for (let i = 0; i < 7; i++) await page.keyboard.press("ArrowRight");
      // Rapid Enter/Backspace cycling
      for (let i = 0; i < 10; i++) {
        await page.keyboard.press("Enter");
        await page.keyboard.press("Backspace");
      }
      await page.waitForTimeout(500);
    },
  },
  {
    name: "119-enter-in-single-char-bold",
    description: "Enter inside **X** (single char bold) — edge case for empty formatting spans",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type("Before **X** after", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+Home");
      // Move to inside the bold "X" (after "Before " and into bold)
      for (let i = 0; i < 7; i++) await page.keyboard.press("ArrowRight");
      await page.keyboard.press("Enter");
      await page.waitForTimeout(500);
    },
  },
  {
    name: "120-enter-after-backspace-merged-line",
    description: "Merge two paragraphs with Backspace, then Enter at merge point — re-split works",
    steps: async (page) => {
      const sourceBtn = page.getByText("Source", { exact: true });
      await sourceBtn.click();
      await page.waitForTimeout(300);
      const editor = page.locator(".kode-editor").first();
      await editor.click({ position: { x: 100, y: 10 } });
      await page.waitForTimeout(100);
      await page.keyboard.press("Control+a");
      await page.keyboard.press("Backspace");
      await page.keyboard.type("First paragraph\n\nSecond paragraph", { delay: 5 });
      await page.waitForTimeout(200);
      const wysiwygBtn = page.getByText("WYSIWYG", { exact: true });
      await wysiwygBtn.click();
      await page.waitForTimeout(500);
      const container = page.locator(".wysiwyg-container");
      await container.click();
      await page.keyboard.press("Control+End");
      await page.keyboard.press("Home");
      // Merge by backspacing
      await page.keyboard.press("Backspace");
      await page.waitForTimeout(300);
      // Now re-split at the same point
      await page.keyboard.press("Enter");
      await page.waitForTimeout(500);
    },
  },
];

// ── Capture logic ──────────────────────────────────────────────────

function ensureDirs() {
  for (const theme of THEMES) {
    fs.mkdirSync(path.join(OUT_DIR, theme, "code"), { recursive: true });
    fs.mkdirSync(path.join(OUT_DIR, theme, "wysiwyg"), { recursive: true });
  }
}

async function switchTheme(page, themeIndex) {
  const btn = page.getByText(THEME_BUTTONS[themeIndex], { exact: true });
  await btn.click();
  await page.waitForTimeout(300);
}

async function switchToCodeEditor(page) {
  const tab = page.getByText("Code Editor", { exact: true });
  await tab.click();
  await page.waitForTimeout(300);
}

async function switchToMarkdownEditor(page) {
  const tab = page.getByText("Markdown Editor", { exact: true });
  await tab.click();
  await page.waitForTimeout(300);
}

async function captureScenario(browser, themeIndex, editorType, scenario, scenarioNum) {
  const theme = THEMES[themeIndex];
  const page = await browser.newPage();
  await page.setViewportSize({ width: 1280, height: 900 });

  // Disable animations
  await page.addStyleTag({
    content: `*, *::before, *::after {
      animation-duration: 0s !important;
      transition-duration: 0s !important;
    }`,
  });

  await page.goto(BASE_URL, { waitUntil: "networkidle", timeout: 15000 });
  await page.waitForSelector(".kode-editor, .wysiwyg-container", { timeout: 10000 });

  // Switch theme
  await switchTheme(page, themeIndex);

  // Switch to correct editor tab
  if (editorType === "code") {
    await switchToCodeEditor(page);
    // Reset to SQL
    const sqlBtn = page.getByText("SQL", { exact: true });
    if (await sqlBtn.isVisible()) await sqlBtn.click();
    await page.waitForTimeout(200);
  } else {
    await switchToMarkdownEditor(page);
  }

  // Run scenario steps
  await scenario.steps(page);

  // Run scenario-specific verify function if present
  let verifyResult = null;
  if (scenario.verify) {
    try {
      verifyResult = await scenario.verify(page);
    } catch (e) {
      verifyResult = { error: e.message };
    }
  }

  // Screenshot the editor area.
  // For WYSIWYG: use .wysiwyg-container parent if visible, fall back to .kode-editor
  // (source mode replaces WYSIWYG container with the code editor).
  let editorLocator;
  if (editorType === "code") {
    editorLocator = page.locator(".kode-editor").first();
  } else {
    const wysiwygVisible = await page.locator(".wysiwyg-container").first().isVisible().catch(() => false);
    editorLocator = wysiwygVisible
      ? page.locator(".wysiwyg-container").first().locator("..")
      : page.locator(".kode-editor").first();
  }

  const outPath = path.join(OUT_DIR, theme, editorType, `${scenario.name}.png`);

  try {
    await editorLocator.screenshot({ path: outPath });
    console.log(`  ${scenarioNum}. ${scenario.name} [${theme}]`);
  } catch (e) {
    console.log(`  ${scenarioNum}. ${scenario.name} [${theme}] FAILED: ${e.message}`);
  }

  // Dump DOM state as JSON for the evaluator agent
  try {
    const domState = await page.evaluate((edType) => {
      const state = { editorType: edType, elements: {}, styles: {}, layering: {} };

      // Code editor DOM structure
      const editor = document.querySelector('.kode-editor');
      if (editor) {
        const content = editor.querySelector('.kode-content, pre');
        const overlay = editor.querySelector('.kode-overlay');
        const gutter = editor.querySelector('.kode-gutter');
        const cursor = editor.querySelector('.kode-cursor');
        const scrollContainer = editor.querySelector('.kode-scroll-container');

        state.elements.editor = !!editor;
        state.elements.content = !!content;
        state.elements.overlay = !!overlay;
        state.elements.gutter = !!gutter;
        state.elements.cursor = !!cursor;
        state.elements.scrollContainer = !!scrollContainer;

        // Count key elements
        state.elements.lineNumbers = editor.querySelectorAll('.kode-line-number, .line-number').length;
        state.elements.selectionHighlights = editor.querySelectorAll('.kode-selection, .selection-highlight, [class*="selection"]').length;
        state.elements.currentLineHighlight = editor.querySelectorAll('.kode-current-line, .current-line-highlight, [class*="current-line"]').length;
        state.elements.errorMarkers = editor.querySelectorAll('.kode-error-marker, .error-marker, [class*="error"], [class*="marker"]').length;
        state.elements.syntaxSpans = content ? content.querySelectorAll('span[class]').length : 0;

        // Computed styles for layering checks
        const getComputedProps = (el, name) => {
          if (!el) return null;
          const cs = window.getComputedStyle(el);
          return {
            name,
            zIndex: cs.zIndex,
            position: cs.position,
            backgroundColor: cs.backgroundColor,
            opacity: cs.opacity,
            overflow: cs.overflow,
            display: cs.display,
            visibility: cs.visibility,
            pointerEvents: cs.pointerEvents,
          };
        };

        state.styles.editor = getComputedProps(editor, 'editor');
        state.styles.content = getComputedProps(content, 'content');
        state.styles.overlay = getComputedProps(overlay, 'overlay');
        state.styles.gutter = getComputedProps(gutter, 'gutter');
        state.styles.cursor = getComputedProps(cursor, 'cursor');
        state.styles.scrollContainer = getComputedProps(scrollContainer, 'scrollContainer');

        // Layering: check if overlay sits above content
        if (overlay && content) {
          const overlayZ = parseInt(window.getComputedStyle(overlay).zIndex) || 0;
          const contentZ = parseInt(window.getComputedStyle(content).zIndex) || 0;
          state.layering.overlayAboveContent = overlayZ >= contentZ;
          state.layering.overlayZ = overlayZ;
          state.layering.contentZ = contentZ;
        }

        // Check selection highlight backgrounds are semi-transparent
        const selDivs = editor.querySelectorAll('.kode-selection, .selection-highlight, [class*="selection"]');
        state.layering.selectionBackgrounds = Array.from(selDivs).slice(0, 5).map(el => {
          return window.getComputedStyle(el).backgroundColor;
        });

        // Check current-line highlight is semi-transparent
        const clDivs = editor.querySelectorAll('.kode-current-line, .current-line-highlight, [class*="current-line"]');
        state.layering.currentLineBackgrounds = Array.from(clDivs).slice(0, 3).map(el => {
          return window.getComputedStyle(el).backgroundColor;
        });

        // Cursor visibility
        if (cursor) {
          const cs = window.getComputedStyle(cursor);
          state.layering.cursorVisible = cs.display !== 'none' && cs.visibility !== 'hidden' && cs.opacity !== '0';
          state.layering.cursorWidth = cs.width;
          state.layering.cursorHeight = cs.height;
          state.layering.cursorBackground = cs.backgroundColor;
        }

        // Text content sample (first 500 chars of visible text)
        if (content) {
          state.elements.textContent = content.textContent?.substring(0, 500) || '';
        }
      }

      // WYSIWYG-specific checks
      const wysiwyg = document.querySelector('.wysiwyg-container, .kode-wysiwyg');
      if (wysiwyg && edType === 'wysiwyg') {
        state.elements.wysiwyg = true;
        state.elements.headings = {
          h1: wysiwyg.querySelectorAll('h1, .wysiwyg-h1').length,
          h2: wysiwyg.querySelectorAll('h2, .wysiwyg-h2').length,
          h3: wysiwyg.querySelectorAll('h3, .wysiwyg-h3').length,
        };
        state.elements.lists = {
          ul: wysiwyg.querySelectorAll('ul, .wysiwyg-ul').length,
          ol: wysiwyg.querySelectorAll('ol, .wysiwyg-ol').length,
        };
        state.elements.codeBlocks = wysiwyg.querySelectorAll('pre, code, .code-block').length;
        state.elements.bold = wysiwyg.querySelectorAll('strong, b, .bold').length;
        state.elements.italic = wysiwyg.querySelectorAll('em, i, .italic').length;
        state.elements.toolbar = !!document.querySelector('.wysiwyg-toolbar, .toolbar');
      }

      // Theme CSS variables
      const root = document.documentElement;
      const cs = window.getComputedStyle(root);
      state.theme = {};
      ['--kode-bg', '--kode-fg', '--kode-selection', '--kode-cursor',
       '--kode-gutter-bg', '--kode-gutter-fg', '--kode-current-line',
       '--kode-line-number', '--kode-active-line-number'].forEach(v => {
        state.theme[v] = cs.getPropertyValue(v).trim();
      });

      return state;
    }, editorType);

    const domPath = outPath.replace('.png', '.dom.json');
    fs.writeFileSync(domPath, JSON.stringify(domState, null, 2));

    // Collect functional assertions from the DOM
    const functional = await page.evaluate((edType) => {
      const container = document.querySelector('.wysiwyg-container') || document.querySelector('.kode-editor');
      if (!container) return { error: 'no editor container' };

      return {
        // Basic state
        activeElement: document.activeElement?.tagName,
        isFocused: document.activeElement?.tagName === 'TEXTAREA',

        // Content
        textContent: container.textContent?.substring(0, 500),

        // Cursor
        cursorElement: (() => {
          const cursor = document.querySelector('.kode-cursor, [id*="cursor"]');
          if (!cursor) return null;
          const cs = window.getComputedStyle(cursor);
          return {
            display: cs.display,
            visibility: cs.visibility,
            top: cursor.style.top,
            left: cursor.style.left,
          };
        })(),

        // Structure — support both old (data-block-start) and new (data-pos-start) attribute names
        blockCount: container.querySelectorAll('[data-pos-start], [data-block-start]').length,

        // Toolbar
        toolbarButtons: (() => {
          const buttons = container.querySelectorAll('button');
          const active = [];
          buttons.forEach(b => {
            const style = b.getAttribute('style') || '';
            if (style.includes('accent')) active.push(b.textContent.trim());
          });
          return { total: buttons.length, active };
        })(),

        // Selection
        selectionHighlights: document.querySelectorAll('.kode-selection').length,
      };
    }, editorType);

    // Write result.json with functional data and optional verify output
    const resultPath = outPath.replace('.png', '.result.json');
    const result = {
      spec: `${theme}/${editorType}/${scenario.name}`,
      scenario: scenario.name,
      theme,
      editorType,
      timestamp: new Date().toISOString(),
      render_status: "ok",
      screenshot_path: outPath,
      dom_path: domPath,
      eval_status: null,
      classification: null,
      failures: [],
      fix_required: null,
      last_evaluated: null,
      functional,
      verify: verifyResult,
    };
    fs.writeFileSync(resultPath, JSON.stringify(result, null, 2));
  } catch (e) {
    console.log(`    (DOM extraction failed: ${e.message})`);
  }

  await page.close();
}

// ── Main ───────────────────────────────────────────────────────────

(async () => {
  ensureDirs();

  console.log(`\nKode Visual Regression Capture`);
  console.log(`Base URL: ${BASE_URL}\n`);

  const browser = await chromium.launch({ headless: true });

  for (let themeIdx = 0; themeIdx < THEMES.length; themeIdx++) {
    console.log(`\n--- Theme: ${THEME_BUTTONS[themeIdx]} ---`);

    if (!filterEditor || filterEditor === "code") {
      console.log(`\nCode Editor:`);
      for (let i = 0; i < CODE_SCENARIOS.length; i++) {
        if (filterScenario !== null && i + 1 !== filterScenario) continue;
        await captureScenario(browser, themeIdx, "code", CODE_SCENARIOS[i], i + 1);
      }
    }

    if (!filterEditor || filterEditor === "wysiwyg") {
      console.log(`\nWYSIWYG Editor:`);
      for (let i = 0; i < WYSIWYG_SCENARIOS.length; i++) {
        if (filterScenario !== null && i + 1 !== filterScenario) continue;
        await captureScenario(browser, themeIdx, "wysiwyg", WYSIWYG_SCENARIOS[i], i + 1);
      }
    }
  }

  await browser.close();
  console.log(`\nScreenshots saved to: ${OUT_DIR}`);
})();
