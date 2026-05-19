import { test, expect } from '@playwright/test';

const BASE = 'http://localhost:8090';

test.describe('WYSIWYG multiple space insertion', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(BASE);
    // Default tab is Markdown Editor in WYSIWYG mode
    await page.waitForTimeout(2000);
  });

  test('multiple spaces at end of line are visually distinct', async ({ page }) => {
    // Click into the heading text
    const heading = await page.evaluate(() => {
      const container = document.querySelector('.wysiwyg-scroll-container');
      const h1 = container?.querySelector('h1');
      if (!h1) return null;
      const rect = h1.getBoundingClientRect();
      return { x: rect.x + 50, y: rect.y + 10 };
    });
    expect(heading).not.toBeNull();
    await page.mouse.click(heading!.x, heading!.y);
    await page.waitForTimeout(300);

    // Go to end of heading
    await page.keyboard.press('End');
    await page.waitForTimeout(200);

    // Measure the rendered width of the heading BEFORE spaces
    const widthBefore = await page.evaluate(() => {
      const container = document.querySelector('.wysiwyg-scroll-container');
      const h1 = container?.querySelector('h1');
      if (!h1) return -1;
      const range = document.createRange();
      range.selectNodeContents(h1);
      return range.getBoundingClientRect().width;
    });

    // Type 5 spaces
    for (let i = 0; i < 5; i++) {
      await page.keyboard.press('Space');
      await page.waitForTimeout(100);
    }
    await page.waitForTimeout(300);

    // Measure the rendered width AFTER spaces
    const widthAfter = await page.evaluate(() => {
      const container = document.querySelector('.wysiwyg-scroll-container');
      const h1 = container?.querySelector('h1');
      if (!h1) return -1;
      const range = document.createRange();
      range.selectNodeContents(h1);
      return range.getBoundingClientRect().width;
    });

    console.log(`Width before: ${widthBefore}px, after: ${widthAfter}px, delta: ${widthAfter - widthBefore}px`);

    // The width should increase — trailing spaces must be visible
    // 5 spaces at ~8px each should add ~40px minimum
    expect(widthAfter - widthBefore).toBeGreaterThan(20);
  });

  test('cursor advances visually when typing spaces at end of line', async ({ page }) => {
    // Click into the heading
    const heading = await page.evaluate(() => {
      const container = document.querySelector('.wysiwyg-scroll-container');
      const h1 = container?.querySelector('h1');
      if (!h1) return null;
      const rect = h1.getBoundingClientRect();
      return { x: rect.x + 50, y: rect.y + 10 };
    });
    expect(heading).not.toBeNull();
    await page.mouse.click(heading!.x, heading!.y);
    await page.waitForTimeout(300);

    // Go to end of heading
    await page.keyboard.press('End');
    await page.waitForTimeout(200);

    // Get the caret position using Selection API
    const caretBefore = await page.evaluate(() => {
      const sel = window.getSelection();
      if (!sel || sel.rangeCount === 0) return null;
      const range = sel.getRangeAt(0);
      const rect = range.getBoundingClientRect();
      return { x: rect.x, y: rect.y };
    });

    // Type 3 spaces
    for (let i = 0; i < 3; i++) {
      await page.keyboard.press('Space');
      await page.waitForTimeout(150);
    }

    // Get the caret position after spaces
    const caretAfter = await page.evaluate(() => {
      const sel = window.getSelection();
      if (!sel || sel.rangeCount === 0) return null;
      const range = sel.getRangeAt(0);
      const rect = range.getBoundingClientRect();
      return { x: rect.x, y: rect.y };
    });

    console.log(`Caret before: ${JSON.stringify(caretBefore)}, after: ${JSON.stringify(caretAfter)}`);

    expect(caretBefore).not.toBeNull();
    expect(caretAfter).not.toBeNull();
    // Caret should have moved right by at least 15px (3 spaces)
    expect(caretAfter!.x - caretBefore!.x).toBeGreaterThan(15);
  });
});
