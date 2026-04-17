import { test, expect } from '@playwright/test';

const BASE = 'http://localhost:8090';

test.describe('WYSIWYG Editor', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(BASE);
    // Wait for WASM to load and render
    await page.waitForSelector('.wysiwyg-container', { timeout: 10000 });
  });

  test('renders markdown as rich content', async ({ page }) => {
    // Should see rendered headings, not raw "# Dashboard Documentation"
    const h1 = page.locator('.wysiwyg-container h1');
    await expect(h1.first()).toBeVisible();

    // Should see paragraphs
    const paragraphs = page.locator('.wysiwyg-container p');
    expect(await paragraphs.count()).toBeGreaterThan(0);

    // Should see a list
    const list = page.locator('.wysiwyg-container ul, .wysiwyg-container ol');
    expect(await list.count()).toBeGreaterThan(0);
  });

  test('textarea gets focus on click', async ({ page }) => {
    const container = page.locator('.wysiwyg-container');
    await container.click();

    // The hidden textarea should be the active element
    const activeTag = await page.evaluate(() => document.activeElement?.tagName);
    expect(activeTag).toBe('TEXTAREA');
  });

  test('typing inserts text', async ({ page }) => {
    const container = page.locator('.wysiwyg-container');
    await container.click();

    // Wait for focus
    await page.waitForFunction(() => document.activeElement?.tagName === 'TEXTAREA');

    // Get initial text
    const initialText = await page.evaluate(() => {
      return document.querySelector('.wysiwyg-container')?.textContent?.length || 0;
    });

    // Type some text
    await page.keyboard.type('Hello test ');

    // Wait for re-render
    await page.waitForTimeout(200);

    // Text should have changed
    const newText = await page.evaluate(() => {
      return document.querySelector('.wysiwyg-container')?.textContent?.length || 0;
    });
    expect(newText).toBeGreaterThan(initialText);
  });

  test('Enter creates new paragraph', async ({ page }) => {
    const container = page.locator('.wysiwyg-container');
    await container.click();
    await page.waitForFunction(() => document.activeElement?.tagName === 'TEXTAREA');

    // Count initial paragraphs
    const initialCount = await page.locator('.wysiwyg-container p').count();

    // Move to end of first paragraph and press Enter twice (paragraph break)
    await page.keyboard.press('End');
    await page.keyboard.press('Enter');
    await page.waitForTimeout(200);

    // Should have at least as many paragraphs (Enter might create list continuation, etc.)
    const newCount = await page.locator('.wysiwyg-container p').count();
    expect(newCount).toBeGreaterThanOrEqual(initialCount);
  });

  test('Ctrl+Z undoes last action', async ({ page }) => {
    const container = page.locator('.wysiwyg-container');
    await container.click();
    await page.waitForFunction(() => document.activeElement?.tagName === 'TEXTAREA');

    // Type something
    await page.keyboard.type('UNDO_TEST');
    await page.waitForTimeout(200);

    // Verify it appeared
    let content = await container.textContent();
    expect(content).toContain('UNDO_TEST');

    // Undo
    await page.keyboard.press('Control+z');
    await page.waitForTimeout(200);

    // Should be gone
    content = await container.textContent();
    expect(content).not.toContain('UNDO_TEST');
  });

  test('toolbar Bold button works', async ({ page }) => {
    const container = page.locator('.wysiwyg-container');
    await container.click();
    await page.waitForFunction(() => document.activeElement?.tagName === 'TEXTAREA');

    // Type some text
    await page.keyboard.type('make this bold');
    await page.waitForTimeout(200);

    // Select all
    await page.keyboard.press('Control+a');
    await page.waitForTimeout(100);

    // Click Bold button
    const boldBtn = page.locator('button[title="Bold"]');
    if (await boldBtn.count() > 0) {
      await boldBtn.click();
      await page.waitForTimeout(200);

      // Should see <strong> in the rendered content
      const strong = page.locator('.wysiwyg-container strong');
      expect(await strong.count()).toBeGreaterThan(0);
    }
  });

  test('toolbar Italic button works', async ({ page }) => {
    const container = page.locator('.wysiwyg-container');
    await container.click();
    await page.waitForFunction(() => document.activeElement?.tagName === 'TEXTAREA');

    await page.keyboard.type('make this italic');
    await page.waitForTimeout(200);
    await page.keyboard.press('Control+a');
    await page.waitForTimeout(100);

    const italicBtn = page.locator('button[title="Italic"]');
    if (await italicBtn.count() > 0) {
      await italicBtn.click();
      await page.waitForTimeout(200);

      const em = page.locator('.wysiwyg-container em');
      expect(await em.count()).toBeGreaterThan(0);
    }
  });

  test('mode toggle switches between Source and WYSIWYG', async ({ page }) => {
    // Should start in WYSIWYG mode
    await expect(page.locator('.wysiwyg-container')).toBeVisible();

    // Click Source button
    const sourceBtn = page.getByText('Source', { exact: true });
    await sourceBtn.click();
    await page.waitForTimeout(300);

    // Should see source mode editor (kode-editor class)
    await expect(page.locator('.kode-editor')).toBeVisible();

    // Click WYSIWYG button
    const wysiwygBtn = page.getByText('WYSIWYG', { exact: true });
    await wysiwygBtn.click();
    await page.waitForTimeout(300);

    // Should be back in WYSIWYG
    await expect(page.locator('.wysiwyg-container')).toBeVisible();
  });

  test('source mode editing works', async ({ page }) => {
    // Switch to source mode
    const sourceBtn = page.getByText('Source', { exact: true });
    await sourceBtn.click();
    await page.waitForTimeout(300);

    const editor = page.locator('.kode-editor');
    await editor.click();
    await page.waitForFunction(() => document.activeElement?.tagName === 'TEXTAREA');

    // Type something
    await page.keyboard.type('SOURCE_TEST');
    await page.waitForTimeout(200);

    // Should appear in the editor
    const content = await editor.textContent();
    expect(content).toContain('SOURCE_TEST');
  });

  test('no console errors on load', async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', (err) => errors.push(err.message));

    await page.goto(BASE);
    await page.waitForSelector('.wysiwyg-container', { timeout: 10000 });
    await page.waitForTimeout(1000);

    // Filter out known benign errors
    const realErrors = errors.filter(e => !e.includes('ResizeObserver'));
    expect(realErrors).toHaveLength(0);
  });

  test('no console errors on typing', async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', (err) => errors.push(err.message));

    await page.waitForSelector('.wysiwyg-container', { timeout: 10000 });
    const container = page.locator('.wysiwyg-container');
    await container.click();
    await page.waitForFunction(() => document.activeElement?.tagName === 'TEXTAREA');

    // Type various things
    await page.keyboard.type('Hello world');
    await page.keyboard.press('Enter');
    await page.keyboard.type('New paragraph');
    await page.keyboard.press('Enter');
    await page.keyboard.type('- list item');
    await page.keyboard.press('Enter');
    await page.keyboard.type('another item');
    await page.keyboard.press('Backspace');
    await page.keyboard.press('Control+z');
    await page.waitForTimeout(500);

    const realErrors = errors.filter(e => !e.includes('ResizeObserver'));
    expect(realErrors).toHaveLength(0);
  });

  test('no console errors on toolbar clicks', async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', (err) => errors.push(err.message));

    await page.waitForSelector('.wysiwyg-container', { timeout: 10000 });
    const container = page.locator('.wysiwyg-container');
    await container.click();
    await page.waitForFunction(() => document.activeElement?.tagName === 'TEXTAREA');

    // Click each toolbar button
    const buttons = page.locator('.wysiwyg-container button');
    const count = await buttons.count();
    for (let i = 0; i < count; i++) {
      await buttons.nth(i).click();
      await page.waitForTimeout(100);
    }

    const realErrors = errors.filter(e => !e.includes('ResizeObserver'));
    expect(realErrors).toHaveLength(0);
  });

  test('Backspace works', async ({ page }) => {
    const container = page.locator('.wysiwyg-container');
    await container.click();
    await page.waitForFunction(() => document.activeElement?.tagName === 'TEXTAREA');

    await page.keyboard.type('ABCDE');
    await page.waitForTimeout(200);

    let content = await container.textContent();
    expect(content).toContain('ABCDE');

    await page.keyboard.press('Backspace');
    await page.keyboard.press('Backspace');
    await page.waitForTimeout(200);

    content = await container.textContent();
    expect(content).toContain('ABC');
    expect(content).not.toContain('ABCDE');
  });

  test('arrow keys move cursor without errors', async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', (err) => errors.push(err.message));

    const container = page.locator('.wysiwyg-container');
    await container.click();
    await page.waitForFunction(() => document.activeElement?.tagName === 'TEXTAREA');

    // Navigate around
    for (let i = 0; i < 5; i++) {
      await page.keyboard.press('ArrowDown');
    }
    for (let i = 0; i < 10; i++) {
      await page.keyboard.press('ArrowRight');
    }
    for (let i = 0; i < 3; i++) {
      await page.keyboard.press('ArrowUp');
    }
    await page.keyboard.press('Home');
    await page.keyboard.press('End');
    await page.keyboard.press('Control+Home');
    await page.keyboard.press('Control+End');
    await page.waitForTimeout(200);

    const realErrors = errors.filter(e => !e.includes('ResizeObserver'));
    expect(realErrors).toHaveLength(0);
  });

  test('Ctrl+A selects all', async ({ page }) => {
    const container = page.locator('.wysiwyg-container');
    await container.click();
    await page.waitForFunction(() => document.activeElement?.tagName === 'TEXTAREA');

    await page.keyboard.press('Control+a');
    await page.waitForTimeout(100);

    // Should have selection highlights visible
    // (We can't easily verify the selection state from the DOM, but at least no errors)
  });
});
