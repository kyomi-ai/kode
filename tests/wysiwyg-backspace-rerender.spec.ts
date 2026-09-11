import { test, expect } from '@playwright/test';

// The WYSIWYG editor's scroll container — one DOM child per top-level block.
const EDITOR = '.tree-wysiwyg-scroll-container';
const CHART = '.chart-demo-block';

/**
 * Plain text input is applied to the contenteditable DOM by the browser and
 * synced back to the document model by a MutationObserver, with no re-render.
 * The first model-driven edit after that — pressing Backspace, in practice —
 * used to clear the segment cache and every child of the container, forcing a
 * full rebuild: every extension block was remounted and the editor scrolled
 * back to the top.
 *
 * The probe is DOM identity. Every top-level child is tagged before the edit;
 * a rebuilt child is a fresh element and has lost its tag.
 */
test('first backspace after typing patches one block instead of rebuilding', async ({ page }) => {
  const consoleErrors: string[] = [];
  page.on('console', (msg) => {
    if (msg.type() === 'error') consoleErrors.push(msg.text());
  });

  await page.goto('/');
  await page.waitForSelector(EDITOR, { timeout: 60_000 });
  await page.waitForSelector(CHART, { timeout: 60_000 });

  // Edit the last block, so the container is genuinely scrolled while typing.
  const target = page.locator(`${EDITOR} > .kode-text-segment`).last();
  await target.scrollIntoViewIfNeeded();

  const before = await page.evaluate(
    ({ sel, chart }) => {
      const el = document.querySelector(sel) as HTMLElement;
      Array.from(el.children).forEach((c, i) => c.setAttribute('data-e2e-id', String(i)));
      document.querySelectorAll(chart).forEach((c, i) => c.setAttribute('data-e2e-chart', String(i)));
      return {
        children: el.children.length,
        charts: document.querySelectorAll(chart).length,
        scrollTop: el.scrollTop,
        text: el.textContent ?? '',
      };
    },
    { sel: EDITOR, chart: CHART },
  );

  expect(before.charts, 'demo document renders chart blocks').toBeGreaterThan(0);
  expect(before.scrollTop, 'container is scrolled before the edit').toBeGreaterThan(0);

  // Type one character at the end of the heading, then delete it again.
  await target.click();
  await page.keyboard.press('End');
  await page.keyboard.type('Z');
  await page.waitForTimeout(150);

  const typed = await page.evaluate((sel) => (document.querySelector(sel) as HTMLElement).textContent ?? '', EDITOR);
  expect(typed, 'the typed character reaches the DOM').toContain('2026Z');

  await page.keyboard.press('Backspace');
  await page.waitForTimeout(250);

  const after = await page.evaluate(
    ({ sel, chart }) => {
      const el = document.querySelector(sel) as HTMLElement;
      return {
        children: el.children.length,
        tagged: el.querySelectorAll(':scope > [data-e2e-id]').length,
        charts: document.querySelectorAll(chart).length,
        taggedCharts: document.querySelectorAll(`${chart}[data-e2e-chart]`).length,
        scrollTop: el.scrollTop,
        text: el.textContent ?? '',
      };
    },
    { sel: EDITOR, chart: CHART },
  );

  // The defect: a full rebuild replaces every child, so none keep their tag.
  expect(after.tagged, 'every top-level block survives the backspace').toBe(before.children);
  expect(after.children).toBe(before.children);
  expect(after.charts, 'no chart is lost').toBe(before.charts);
  expect(after.taggedCharts, 'charts keep their original DOM element').toBe(before.charts);
  expect(after.scrollTop, 'scroll position is kept').toBe(before.scrollTop);

  // The edit itself must still be correct: the typed character is gone, which
  // is exactly what the old full rebuild existed to guarantee.
  expect(after.text).toContain('Last updated: March 2026');
  expect(after.text).not.toContain('2026Z');

  expect(consoleErrors, 'no console errors').toEqual([]);
});
