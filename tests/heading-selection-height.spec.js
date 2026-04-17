/**
 * Test: Selection highlight height on headings (especially H1).
 *
 * Bug: When partially selecting text inside a heading, the selection
 * highlight only covers the top half of the text. Full-line selections
 * are fine because they use the element's bounding rect height, but
 * partial selections use a hardcoded `1.4em` height that is relative
 * to the overlay container's font size — not the heading's larger
 * font size.
 *
 * This test verifies that the selection highlight height is at least
 * 80% of the heading element's actual height.
 */

const { chromium } = require("playwright");

const BASE_URL = process.env.KODE_URL || "http://localhost:8090";

async function focusWysiwyg(page) {
  const ta = await page.$(".tree-wysiwyg-container textarea");
  if (ta) {
    await ta.focus();
    return;
  }
  const container = await page.$(".tree-wysiwyg-scroll-container");
  if (container) await container.click();
}

async function run() {
  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  await page.goto(BASE_URL);
  await page.waitForTimeout(1000);

  // Switch to WYSIWYG mode if needed
  const wysiwygBtn = await page.$('button:has-text("WYSIWYG")');
  if (wysiwygBtn) {
    await wysiwygBtn.click();
    await page.waitForTimeout(500);
  }

  await focusWysiwyg(page);
  await page.waitForTimeout(300);

  // Find the H1 element
  const h1Info = await page.evaluate(() => {
    const scroll = document.querySelector(".tree-wysiwyg-scroll-container");
    const h1 = scroll?.querySelector("h1");
    if (!h1) return null;
    const r = h1.getBoundingClientRect();
    const style = window.getComputedStyle(h1);
    return {
      left: r.left,
      top: r.top,
      width: r.width,
      height: r.height,
      fontSize: parseFloat(style.fontSize),
      lineHeight: parseFloat(style.lineHeight) || r.height,
    };
  });

  if (!h1Info) {
    console.log("SKIP: No H1 found in the editor");
    await browser.close();
    process.exit(0);
  }

  console.log(`H1 element: ${Math.round(h1Info.width)}x${Math.round(h1Info.height)}px, fontSize=${Math.round(h1Info.fontSize)}px`);

  // Create a partial selection within the H1 by clicking at 10% then
  // shift-clicking at 50%.
  const y = h1Info.top + h1Info.height / 2;
  const startX = h1Info.left + h1Info.width * 0.1;
  const endX = h1Info.left + h1Info.width * 0.5;

  await page.mouse.click(startX, y);
  await page.waitForTimeout(300);
  await page.mouse.click(endX, y, { modifiers: ["Shift"] });
  await page.waitForTimeout(500);

  // Measure the selection highlight dimensions
  const result = await page.evaluate(() => {
    const selections = document.querySelectorAll(".kode-selection");
    if (selections.length === 0) return { error: "no selection highlights found" };

    const scroll = document.querySelector(".tree-wysiwyg-scroll-container");
    const h1 = scroll?.querySelector("h1");
    const h1Rect = h1?.getBoundingClientRect();
    const h1Style = h1 ? window.getComputedStyle(h1) : null;

    // Collect all selection highlight rects
    const highlights = Array.from(selections).map((sel) => {
      const r = sel.getBoundingClientRect();
      return { width: r.width, height: r.height, top: r.top, left: r.left };
    });

    return {
      highlightCount: highlights.length,
      highlights,
      h1Height: h1Rect?.height || 0,
      h1FontSize: h1Style ? parseFloat(h1Style.fontSize) : 0,
      h1LineHeight: h1Style
        ? parseFloat(h1Style.lineHeight) || h1Rect?.height || 0
        : 0,
    };
  });

  if (result.error) {
    console.log(`FAIL: ${result.error}`);
    await browser.close();
    process.exit(1);
  }

  console.log(`Found ${result.highlightCount} selection highlight(s)`);
  console.log(`H1 height: ${Math.round(result.h1Height)}px, fontSize: ${Math.round(result.h1FontSize)}px`);

  // Check that each selection highlight's height covers at least 80%
  // of the heading's visual height. The bug causes the highlight to
  // be only ~50% of the heading height because it uses 1.4em relative
  // to the overlay's font size, not the heading's.
  let passed = true;
  for (let i = 0; i < result.highlights.length; i++) {
    const h = result.highlights[i];
    const ratio = h.height / result.h1Height;
    const ok = ratio >= 0.8;
    console.log(
      `  highlight[${i}]: ${Math.round(h.width)}x${Math.round(h.height)}px, ` +
      `height ratio: ${(ratio * 100).toFixed(1)}% of H1 — ${ok ? "PASS" : "FAIL"}`
    );
    if (!ok) passed = false;
  }

  // Take a screenshot for visual reference
  await page.screenshot({ path: "tests/screenshots/heading-selection-height.png" });

  await browser.close();

  if (!passed) {
    console.log("\nFAIL: Selection highlight height does not cover full heading text.");
    console.log("Expected: highlight height >= 80% of H1 element height");
    console.log("This is the known bug: partial selection uses hardcoded 1.4em height");
    process.exit(1);
  } else {
    console.log("\nPASS: Selection highlight height covers heading text correctly.");
    process.exit(0);
  }
}

run().catch((err) => {
  console.error("Test error:", err.message);
  process.exit(1);
});
