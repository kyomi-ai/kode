import { test, expect } from '@playwright/test';

const EDITOR = '.tree-wysiwyg-scroll-container';
const TEXT_BLOCK = `${EDITOR} > .kode-text-segment`;

/**
 * Text typed natively into the contenteditable is reconciled by invalidating
 * only the blocks the browser touched. These cases exercise the ways that
 * block-to-slot mapping can go stale: editing several blocks before any
 * re-render, and edits that change how many blocks exist.
 */

async function ready(page: import('@playwright/test').Page) {
  await page.goto('/');
  await page.waitForSelector(EDITOR, { timeout: 60_000 });
  await page.waitForSelector('.chart-demo-block', { timeout: 60_000 });
}

const editorText = (page: import('@playwright/test').Page) =>
  page.evaluate((sel) => (document.querySelector(sel) as HTMLElement).textContent ?? '', EDITOR);

test('backspace is applied to the right block when two blocks were typed in', async ({ page }) => {
  await ready(page);

  const first = page.locator(TEXT_BLOCK).first();
  const last = page.locator(TEXT_BLOCK).last();

  // Type into two different blocks with no re-render in between, so both
  // carry browser-applied text the cached segments know nothing about.
  await first.click();
  await page.keyboard.press('End');
  await page.keyboard.type('AAA');
  await last.scrollIntoViewIfNeeded();
  await last.click();
  await page.keyboard.press('End');
  await page.keyboard.type('BBB');
  await page.waitForTimeout(150);

  await page.keyboard.press('Backspace');
  await page.waitForTimeout(250);

  const text = await editorText(page);
  expect(text, 'the untouched-by-backspace block keeps its typed text').toContain('DocumentationAAA');
  expect(text, 'the backspace lands on the block that has focus').toContain('2026BB');
  expect(text).not.toContain('2026BBB');
});

test('typing then pressing Enter splits the block without losing text', async ({ page }) => {
  await ready(page);

  const before = await page.locator(TEXT_BLOCK).count();
  const last = page.locator(TEXT_BLOCK).last();
  await last.scrollIntoViewIfNeeded();
  await last.click();
  await page.keyboard.press('End');
  await page.keyboard.type('CCC');
  await page.waitForTimeout(150);

  await page.keyboard.press('Enter');
  await page.keyboard.type('DDD');
  await page.waitForTimeout(250);

  expect(await page.locator(TEXT_BLOCK).count(), 'a block was added').toBe(before + 1);
  const text = await editorText(page);
  expect(text).toContain('2026CCC');
  expect(text).toContain('DDD');
});

test('backspace at the start of a block merges it into the one before', async ({ page }) => {
  await ready(page);

  const before = await page.locator(TEXT_BLOCK).count();
  // The paragraph under the H1 — its preceding block is an ordinary heading,
  // so Backspace at its start is an unambiguous block merge.
  const para = page.locator(TEXT_BLOCK).nth(1);
  await para.click();
  await page.keyboard.press('End');
  await page.keyboard.type('EEE');
  await page.waitForTimeout(150);

  // Home, then Backspace — a structural edit that shifts every later block.
  // The caret move has to land before the delete, or the delete just removes
  // the character it is sitting on.
  await page.keyboard.press('Home');
  await page.waitForTimeout(100);
  await page.keyboard.press('Backspace');
  await page.waitForTimeout(250);

  expect(await page.locator(TEXT_BLOCK).count(), 'two blocks became one').toBe(before - 1);
  const text = await editorText(page);
  expect(text, 'the heading and the paragraph are now one block').toContain(
    'Dashboard DocumentationThis dashboard tracks',
  );
  expect(text, 'the typed text survives the merge').toContain('regions.EEE');
});

test('repeated type-and-delete cycles keep the document and charts intact', async ({ page }) => {
  await ready(page);

  const charts = await page.locator('.chart-demo-block').count();
  const blocks = await page.locator(TEXT_BLOCK).count();
  const last = page.locator(TEXT_BLOCK).last();
  await last.scrollIntoViewIfNeeded();
  await last.click();
  await page.keyboard.press('End');

  for (let i = 0; i < 5; i++) {
    await page.keyboard.type('xy');
    await page.waitForTimeout(80);
    await page.keyboard.press('Backspace');
    await page.keyboard.press('Backspace');
    await page.waitForTimeout(80);
  }

  const text = await editorText(page);
  expect(text, 'the document returns to its original text').toContain('Last updated: March 2026');
  expect(text).not.toContain('x');
  expect(await page.locator('.chart-demo-block').count(), 'charts still mounted').toBe(charts);
  expect(await page.locator(TEXT_BLOCK).count(), 'block count unchanged').toBe(blocks);
});
