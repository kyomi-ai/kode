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

test.describe('WYSIWYG with CSS grid parent', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(BASE);
    await page.waitForSelector('.wysiwyg-container', { timeout: 10000 });
  });

  test('typing works correctly when scroll container uses CSS grid', async ({ page }) => {
    await setContent(page, 'Hello world');

    // Apply CSS grid to the scroll container (mimics Kyomi's layout)
    await page.evaluate(() => {
      const scrollContainer = document.querySelector('.tree-wysiwyg-scroll-container');
      if (scrollContainer) {
        (scrollContainer as HTMLElement).style.display = 'grid';
        (scrollContainer as HTMLElement).style.gridTemplateColumns = '1fr';
        const children = scrollContainer.children;
        for (let i = 0; i < children.length; i++) {
          (children[i] as HTMLElement).style.gridColumn = '1 / -1';
        }
      }
    });
    await page.waitForTimeout(200);

    // Click on the paragraph and type
    const p = page.locator('.wysiwyg-container p').first();
    await p.click();
    await page.waitForFunction(() => document.activeElement?.getAttribute?.('contenteditable') === 'true');

    await page.keyboard.press('End');
    await page.keyboard.type(' added');
    await page.waitForTimeout(200);

    const text = await p.textContent();
    expect(text).toContain('Hello world added');
  });
});
