import { test, expect } from '@playwright/test';

const BASE = 'http://localhost:8090';

// Helper: clear editor and set fresh content in WYSIWYG mode
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

// Helper: get the raw markdown source from the editor
async function getSourceText(page) {
  const sourceBtn = page.getByText('Source', { exact: true });
  await sourceBtn.click();
  await page.waitForTimeout(300);
  await page.waitForSelector('.kode-editor');

  const text = await page.evaluate(() => {
    const spans = document.querySelectorAll('[data-line]');
    return Array.from(spans).map(s => s.textContent || '').join('\n');
  });

  const wysiwygBtn = page.getByText('WYSIWYG', { exact: true });
  await wysiwygBtn.click();
  await page.waitForTimeout(300);
  await page.waitForSelector('.wysiwyg-container');

  return text;
}

test.describe('Atomic Extension Blocks', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(BASE);
    await page.waitForSelector('.wysiwyg-container', { timeout: 10000 });
  });

  test('chart blocks render as atomic extension blocks', async ({ page }) => {
    // The demo starts with chart blocks in the default content
    const chartBlocks = page.locator('[data-kode-extension="chart-demo"]');
    expect(await chartBlocks.count()).toBeGreaterThanOrEqual(1);

    // Chart blocks should contain the rendered content, not raw code
    const firstChart = chartBlocks.first();
    await expect(firstChart.locator('.chart-demo-block')).toBeVisible();
  });

  test('cursor cannot enter atomic block via arrow keys', async ({ page }) => {
    await setContent(page, 'Before\n\n```chart\ntitle: Test Chart\n```\n\nAfter');

    // Click on "Before" paragraph
    const p = page.locator('.wysiwyg-container p').first();
    await p.click();
    await page.waitForFunction(() => document.activeElement?.getAttribute?.('contenteditable') === 'true');

    // Move to end of "Before"
    await page.keyboard.press('End');
    await page.waitForTimeout(100);

    // Press ArrowRight — should skip over the atomic block entirely
    // and land in the "After" paragraph (or at the gap after the block)
    await page.keyboard.press('ArrowRight');
    await page.waitForTimeout(100);
    await page.keyboard.press('ArrowRight');
    await page.waitForTimeout(100);

    // Type something — it should NOT appear inside the chart block
    await page.keyboard.type('INSERTED');
    await page.waitForTimeout(200);

    const source = await getSourceText(page);
    // The text should NOT be inside the chart fence
    expect(source).not.toMatch(/title: Test ChartINSERTED/);
    expect(source).not.toMatch(/```chart\nINSERTED/);
    // The chart should still be intact
    expect(source).toContain('```chart');
    expect(source).toContain('title: Test Chart');
  });

  test('typing at gap between atomic blocks creates new paragraph', async ({ page }) => {
    await setContent(page, 'Start\n\n```chart\ntitle: Chart One\n```\n\n```chart\ntitle: Chart Two\n```');

    // There should be two chart blocks
    const chartBlocks = page.locator('[data-kode-extension="chart-demo"]');
    await expect(chartBlocks).toHaveCount(2);

    // Navigate to the gap using keyboard: click "Start", then arrow right
    // past the paragraph end, past the first chart, to the gap between charts
    const p = page.locator('.wysiwyg-container p').first();
    await p.click();
    await page.waitForFunction(() => document.activeElement?.getAttribute?.('contenteditable') === 'true');
    await page.keyboard.press('End');
    await page.waitForTimeout(100);

    // ArrowRight past paragraph end → gap before first chart → gap after first chart
    // (each ArrowRight skips the atomic block as a whole unit)
    await page.keyboard.press('ArrowRight');
    await page.waitForTimeout(100);
    await page.keyboard.press('ArrowRight');
    await page.waitForTimeout(100);

    // Now at gap between the two charts. Type text.
    await page.keyboard.type('Text between charts');
    await page.waitForTimeout(300);

    const source = await getSourceText(page);
    expect(source).toContain('Text between charts');
    // Both charts should still be intact
    expect(source).toContain('title: Chart One');
    expect(source).toContain('title: Chart Two');
  });

  test('backspace at gap after atomic block deletes the block', async ({ page }) => {
    await setContent(page, '```chart\ntitle: Delete Me\n```\n\nKeep this');

    // Verify chart exists
    const chartBlocks = page.locator('[data-kode-extension="chart-demo"]');
    await expect(chartBlocks).toHaveCount(1);

    // Click on "Keep this"
    const p = page.locator('.wysiwyg-container p').first();
    await p.click();
    await page.waitForFunction(() => document.activeElement?.getAttribute?.('contenteditable') === 'true');

    // Move to start of "Keep this"
    await page.keyboard.press('Home');
    await page.waitForTimeout(100);

    // Backspace should delete the atomic block
    await page.keyboard.press('Backspace');
    await page.waitForTimeout(300);

    const source = await getSourceText(page);
    // Chart should be gone
    expect(source).not.toContain('```chart');
    expect(source).not.toContain('title: Delete Me');
    // Text should remain
    expect(source).toContain('Keep this');
  });

  test('delete at gap before atomic block deletes the block', async ({ page }) => {
    await setContent(page, 'Keep this\n\n```chart\ntitle: Delete Me\n```');

    // Click on "Keep this"
    const p = page.locator('.wysiwyg-container p').first();
    await p.click();
    await page.waitForFunction(() => document.activeElement?.getAttribute?.('contenteditable') === 'true');

    // Move to end of "Keep this"
    await page.keyboard.press('End');
    await page.waitForTimeout(100);

    // Delete should delete the atomic block
    await page.keyboard.press('Delete');
    await page.waitForTimeout(300);

    const source = await getSourceText(page);
    // Chart should be gone
    expect(source).not.toContain('```chart');
    expect(source).not.toContain('title: Delete Me');
    // Text should remain
    expect(source).toContain('Keep this');
  });

  test('Enter at gap creates empty paragraph', async ({ page }) => {
    await setContent(page, 'Before\n\n```chart\ntitle: Test\n```\n\nAfter');

    // Click on "Before"
    const p = page.locator('.wysiwyg-container p').first();
    await p.click();
    await page.waitForFunction(() => document.activeElement?.getAttribute?.('contenteditable') === 'true');
    await page.keyboard.press('End');
    await page.waitForTimeout(100);

    // ArrowRight to move past the paragraph end to the gap before the chart
    await page.keyboard.press('ArrowRight');
    await page.waitForTimeout(100);

    // Press Enter — should create a new paragraph at the gap
    await page.keyboard.press('Enter');
    await page.waitForTimeout(200);

    // Type in the new paragraph
    await page.keyboard.type('New line');
    await page.waitForTimeout(300);

    const source = await getSourceText(page);
    expect(source).toContain('New line');
    // Chart should still be intact
    expect(source).toContain('```chart');
    expect(source).toContain('title: Test');
  });

  test('clicking on atomic block does not steal focus or move cursor inside', async ({ page }) => {
    await setContent(page, 'Before\n\n```chart\ntitle: Test\n```\n\nAfter');

    // First focus the editor
    const p = page.locator('.wysiwyg-container p').first();
    await p.click();
    await page.waitForFunction(() => document.activeElement?.getAttribute?.('contenteditable') === 'true');

    // Click directly on the chart block
    const chartBlock = page.locator('[data-kode-extension="chart-demo"]').first();
    await chartBlock.click();
    await page.waitForTimeout(200);

    // Type — should NOT corrupt the chart
    await page.keyboard.type('X');
    await page.waitForTimeout(200);

    const source = await getSourceText(page);
    // Chart content should be unchanged
    expect(source).toContain('title: Test');
    expect(source).not.toMatch(/title: TestX/);
    expect(source).not.toMatch(/Xtitle/);
  });

  test('non-atomic code blocks remain editable', async ({ page }) => {
    await setContent(page, '```sql\nSELECT 1\n```');

    // SQL code blocks are NOT handled by the chart extension, so they should be normal
    const chartBlocks = page.locator('[data-kode-extension]');
    expect(await chartBlocks.count()).toBe(0);

    // The SQL code block should be rendered as a normal syntax-highlighted block
    const codeBlock = page.locator('.wysiwyg-container .wysiwyg-code-block');
    await expect(codeBlock).toBeVisible();
  });

  test('source roundtrip preserves chart blocks', async ({ page }) => {
    const original = '```chart\ntitle: Roundtrip Test\ntype: line\n```';
    await setContent(page, original);

    // Verify it renders as atomic
    const chartBlock = page.locator('[data-kode-extension="chart-demo"]');
    await expect(chartBlock).toHaveCount(1);

    // Get source and verify it's preserved
    const source = await getSourceText(page);
    expect(source).toContain('```chart');
    expect(source).toContain('title: Roundtrip Test');
    expect(source).toContain('type: line');
  });

  test('no console errors with atomic blocks', async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', (err) => errors.push(err.message));

    await setContent(page, 'Text\n\n```chart\ntitle: Error Test\n```\n\nMore text');

    // Click around, navigate, type
    const p = page.locator('.wysiwyg-container p').first();
    await p.click();
    await page.waitForFunction(() => document.activeElement?.getAttribute?.('contenteditable') === 'true');

    await page.keyboard.press('End');
    for (let i = 0; i < 10; i++) {
      await page.keyboard.press('ArrowRight');
    }
    await page.keyboard.type('test');
    await page.waitForTimeout(300);

    const realErrors = errors.filter(e => !e.includes('ResizeObserver'));
    expect(realErrors).toHaveLength(0);
  });
});
