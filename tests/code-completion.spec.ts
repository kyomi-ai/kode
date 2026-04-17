import { test, expect } from '@playwright/test';

const BASE = 'http://localhost:8090';

test.describe('Code Completion', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(BASE);
    // Switch to Code Editor tab
    await page.click('button:has-text("Code Editor")');
    // Wait for the code editor to render
    await page.waitForSelector('.kode-editor', { timeout: 10000 });
    // Focus the editor
    await page.click('.kode-editor');
    await page.waitForFunction(() => document.activeElement?.tagName === 'TEXTAREA');
  });

  test('typing triggers autocomplete popup', async ({ page }) => {
    // Select all and replace with our test text
    await page.keyboard.press('Control+a');
    await page.keyboard.type('SEL', { delay: 50 });

    // Wait for the popup to appear (debounced at 100ms)
    const popup = page.locator('.kode-completion-popup');
    await expect(popup).toBeVisible({ timeout: 2000 });

    // Should have completion items
    const items = page.locator('.kode-completion-label');
    expect(await items.count()).toBeGreaterThan(0);

    // Should include SELECT
    await expect(page.locator('.kode-completion-label:has-text("SELECT")')).toBeVisible();
  });

  test('arrow keys navigate completion items', async ({ page }) => {
    await page.keyboard.press('Control+a');
    await page.keyboard.type('SEL', { delay: 50 });

    const popup = page.locator('.kode-completion-popup');
    await expect(popup).toBeVisible({ timeout: 2000 });

    // First item should be selected initially
    const selectedBefore = page.locator('.kode-completion-item--selected .kode-completion-label');
    const initialLabel = await selectedBefore.textContent();

    // Press ArrowDown to move selection
    await page.keyboard.press('ArrowDown');
    await page.waitForTimeout(100);

    const selectedAfter = page.locator('.kode-completion-item--selected .kode-completion-label');
    const newLabel = await selectedAfter.textContent();

    // The selected item should have changed
    expect(newLabel).not.toBe(initialLabel);
  });

  test('Enter accepts selected completion', async ({ page }) => {
    await page.keyboard.press('Control+a');
    await page.keyboard.type('SEL', { delay: 50 });

    const popup = page.locator('.kode-completion-popup');
    await expect(popup).toBeVisible({ timeout: 2000 });

    // Accept the completion with Enter
    await page.keyboard.press('Enter');
    await page.waitForTimeout(200);

    // Popup should disappear
    await expect(popup).not.toBeVisible({ timeout: 2000 });

    // The editor should contain the completed text (SELECT)
    const editorContent = await page.locator('.kode-editor').textContent();
    expect(editorContent).toContain('SELECT');
  });

  test('Escape dismisses completion popup', async ({ page }) => {
    await page.keyboard.press('Control+a');
    await page.keyboard.type('SEL', { delay: 50 });

    const popup = page.locator('.kode-completion-popup');
    await expect(popup).toBeVisible({ timeout: 2000 });

    // Dismiss with Escape
    await page.keyboard.press('Escape');
    await page.waitForTimeout(200);

    // Popup should disappear
    await expect(popup).not.toBeVisible({ timeout: 2000 });
  });

  test('Ctrl+Space invokes completion', async ({ page }) => {
    await page.keyboard.press('Control+a');
    await page.keyboard.type('S', { delay: 50 });

    // Wait for any automatic popup to settle
    await page.waitForTimeout(300);

    // Dismiss any existing popup first
    await page.keyboard.press('Escape');
    await page.waitForTimeout(200);

    const popup = page.locator('.kode-completion-popup');
    await expect(popup).not.toBeVisible({ timeout: 1000 });

    // Invoke with Ctrl+Space
    await page.keyboard.press('Control+Space');

    // Popup should appear
    await expect(popup).toBeVisible({ timeout: 2000 });
  });

  test('filter narrows results as user types', async ({ page }) => {
    await page.keyboard.press('Control+a');
    await page.keyboard.type('SE', { delay: 50 });

    const popup = page.locator('.kode-completion-popup');
    await expect(popup).toBeVisible({ timeout: 2000 });

    // Count items with prefix "SE" (should include SELECT and multiple others)
    const initialCount = await page.locator('.kode-completion-item').count();
    expect(initialCount).toBeGreaterThan(0);

    // Type more to narrow down
    await page.keyboard.type('L', { delay: 50 });
    await page.waitForTimeout(300);

    // Should still be visible but with fewer or equal items
    await expect(popup).toBeVisible({ timeout: 2000 });
    const narrowedCount = await page.locator('.kode-completion-item').count();
    expect(narrowedCount).toBeLessThanOrEqual(initialCount);

    // SELECT should still be visible
    await expect(page.locator('.kode-completion-label:has-text("SELECT")')).toBeVisible();
  });

  test('backspace widens filter then dismisses when past word start', async ({ page }) => {
    await page.keyboard.press('Control+a');
    await page.keyboard.type('SELE', { delay: 50 });

    const popup = page.locator('.kode-completion-popup');
    await expect(popup).toBeVisible({ timeout: 2000 });

    // Wait for filter to stabilise, then count items for "SELE"
    await page.waitForTimeout(200);
    const countBefore = await page.locator('.kode-completion-item').count();

    // Backspace to "SEL" — should widen the filter
    await page.keyboard.press('Backspace');
    await page.waitForTimeout(200);

    // Popup should still be visible with same or more items
    await expect(popup).toBeVisible({ timeout: 2000 });
    const countAfter = await page.locator('.kode-completion-item').count();
    expect(countAfter).toBeGreaterThanOrEqual(countBefore);

    // Backspace through all remaining chars — popup should dismiss
    await page.keyboard.press('Backspace'); // "SE"
    await page.waitForTimeout(100);
    await page.keyboard.press('Backspace'); // "S"
    await page.waitForTimeout(100);
    await page.keyboard.press('Backspace'); // ""
    await page.waitForTimeout(200);

    // Popup should be gone — empty prefix or cursor before word_start
    await expect(popup).not.toBeVisible({ timeout: 2000 });
  });

  test('click on completion item accepts it', async ({ page }) => {
    await page.keyboard.press('Control+a');
    await page.keyboard.type('SEL', { delay: 50 });

    const popup = page.locator('.kode-completion-popup');
    await expect(popup).toBeVisible({ timeout: 2000 });

    // Capture the label of the item we're about to click
    const firstItem = page.locator('.kode-completion-item').first();
    const itemLabel = await firstItem.locator('.kode-completion-label').textContent();
    expect(itemLabel).toBeTruthy();

    // Click the item to accept it
    await firstItem.click();
    await page.waitForTimeout(200);

    // Popup should disappear
    await expect(popup).not.toBeVisible({ timeout: 2000 });

    // The completed keyword (not just the "SEL" prefix) should be in the editor
    const editorContent = await page.locator('.kode-editor').textContent();
    expect(editorContent).toContain(itemLabel!);
  });
});
