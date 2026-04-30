import { test, expect } from '@playwright/test';

const BASE = 'http://localhost:8090';

async function setContent(page, markdown: string) {
  const sourceBtn = page.getByText('Source', { exact: true });
  await sourceBtn.click();
  await page.waitForTimeout(300);
  await page.waitForSelector('.kode-editor');

  const editor = page.locator('.kode-editor');
  await editor.click();
  await page.waitForFunction(() => document.activeElement?.tagName === 'TEXTAREA');
  await page.keyboard.press('Control+a');
  await page.keyboard.type(markdown);
  await page.waitForTimeout(200);

  const wysiwygBtn = page.getByText('WYSIWYG', { exact: true });
  await wysiwygBtn.click();
  await page.waitForTimeout(300);
  await page.waitForSelector('.wysiwyg-container');
}

test.describe('WYSIWYG cursor with CSS grid parent', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(BASE);
    await page.waitForSelector('.wysiwyg-container', { timeout: 10000 });
  });

  test('cursor aligns correctly when scroll container uses CSS grid', async ({ page }) => {
    await setContent(page, 'Hello world this is a test');

    // Simulate Kyomi's CSS: add grid layout to the scroll container
    // This mimics: .tree-wysiwyg-scroll-container { display: grid; }
    // and: .tree-wysiwyg-scroll-container > * { grid-column: 1 / -1; }
    await page.evaluate(() => {
      const scrollContainer = document.querySelector('.tree-wysiwyg-scroll-container');
      if (scrollContainer) {
        (scrollContainer as HTMLElement).style.display = 'grid';
        (scrollContainer as HTMLElement).style.gridTemplateColumns = '1fr';
        // Apply grid-column to all direct children
        const children = scrollContainer.children;
        for (let i = 0; i < children.length; i++) {
          (children[i] as HTMLElement).style.gridColumn = '1 / -1';
        }
      }
    });
    await page.waitForTimeout(200);

    // Click on the paragraph to focus
    const p = page.locator('.wysiwyg-container p').first();
    await p.click();
    await page.waitForFunction(() => document.activeElement?.tagName === 'TEXTAREA');

    // Move to position 5
    await page.keyboard.press('Home');
    for (let i = 0; i < 5; i++) {
      await page.keyboard.press('ArrowRight');
    }
    await page.waitForTimeout(300);

    // Measure: cursor's ACTUAL rendered position vs character position
    const positions = await page.evaluate(() => {
      const cursor = document.querySelector('.kode-cursor') as HTMLElement;
      if (!cursor) return null;

      // The cursor's actual rendered position (viewport-relative)
      const cursorRect = cursor.getBoundingClientRect();

      // Find the text-containing element
      const posEl = document.querySelector('[data-pos-start]');
      if (!posEl) return null;

      // Walk text nodes to find character offset 5
      let remaining = 5;
      function findNode(node: Node): { node: Node, offset: number } | null {
        if (node.nodeType === Node.TEXT_NODE) {
          const len = (node.textContent || '').length;
          if (remaining <= len) return { node, offset: remaining };
          remaining -= len;
          return null;
        }
        for (let i = 0; i < node.childNodes.length; i++) {
          const r = findNode(node.childNodes[i]);
          if (r) return r;
        }
        return null;
      }

      const found = findNode(posEl);
      if (!found) return null;

      const range = document.createRange();
      range.setStart(found.node, found.offset);
      range.setEnd(found.node, found.offset);
      const rects = range.getClientRects();
      if (rects.length === 0) return null;

      // Compare viewport-relative positions directly
      const charLeftViewport = rects[0].left;
      const cursorLeftViewport = cursorRect.left;

      // Also capture diagnostic info
      const overlay = cursor.parentElement;
      const overlayRect = overlay ? overlay.getBoundingClientRect() : null;
      const scrollContainer = document.querySelector('.tree-wysiwyg-scroll-container');
      const containerRect = scrollContainer ? scrollContainer.getBoundingClientRect() : null;

      return {
        cursorLeftViewport,
        charLeftViewport,
        diff: Math.abs(cursorLeftViewport - charLeftViewport),
        cursorStyleLeft: cursor.style.left,
        overlayLeft: overlayRect?.left,
        containerLeft: containerRect?.left,
        overlayOffset: overlayRect && containerRect ? overlayRect.left - containerRect.left : null,
      };
    });

    console.log('Grid cursor positions:', JSON.stringify(positions, null, 2));

    expect(positions).not.toBeNull();
    // Cursor should be within 2px of the actual character position
    expect(positions!.diff).toBeLessThanOrEqual(2);
  });
});
