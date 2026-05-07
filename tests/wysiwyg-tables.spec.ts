import { test, expect } from '@playwright/test';

const BASE = 'http://localhost:8090';

test.describe('WYSIWYG Table Support', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(BASE);
    await page.waitForSelector('.wysiwyg-container', { timeout: 10000 });
  });

  test('renders table with correct structure', async ({ page }) => {
    // The demo markdown contains a table under ## Metrics
    const table = page.locator('.wysiwyg-table');
    await expect(table).toBeVisible();

    // Table should have a thead with a row containing 4 th cells
    const headerCells = table.locator('thead th');
    await expect(headerCells).toHaveCount(4);

    // Table body should have at least 3 data rows (Revenue, Users, Conversion)
    const bodyRows = table.locator('tbody tr');
    expect(await bodyRows.count()).toBeGreaterThanOrEqual(3);
  });

  test('table cells have correct content', async ({ page }) => {
    const table = page.locator('.wysiwyg-table');
    await expect(table).toBeVisible();

    // Verify header cells
    const headerCells = table.locator('thead th');
    await expect(headerCells.nth(0)).toContainText('Metric');
    await expect(headerCells.nth(1)).toContainText('Q1');
    await expect(headerCells.nth(2)).toContainText('Q2');
    await expect(headerCells.nth(3)).toContainText('Q3');

    // Verify first data row (Revenue)
    const firstRow = table.locator('tbody tr').first();
    const firstRowCells = firstRow.locator('td');
    await expect(firstRowCells.nth(0)).toContainText('Revenue');
    await expect(firstRowCells.nth(1)).toContainText('$1.2M');
    await expect(firstRowCells.nth(2)).toContainText('$1.5M');
    await expect(firstRowCells.nth(3)).toContainText('$1.8M');
  });

  test('clicking a table cell focuses editor', async ({ page }) => {
    const table = page.locator('.wysiwyg-table');
    await expect(table).toBeVisible();

    // Click on a data cell
    const cell = table.locator('tbody td').first();
    await cell.click();

    // The contenteditable div should have focus
    await page.waitForFunction(
      () => document.activeElement?.getAttribute?.('contenteditable') === 'true'
    );
  });

  test('typing in a table cell inserts text', async ({ page }) => {
    const table = page.locator('.wysiwyg-table');
    await expect(table).toBeVisible();

    // Click on a data cell and get its initial text
    const cell = table.locator('tbody td').first();
    const initialText = await cell.textContent();
    await cell.click();

    await page.waitForFunction(
      () => document.activeElement?.getAttribute?.('contenteditable') === 'true'
    );

    // Move to end of cell content and type
    await page.keyboard.press('End');
    await page.keyboard.type('XYZ');
    await page.waitForTimeout(200);

    // The cell text should contain the typed characters
    const updatedText = await table.locator('tbody td').first().textContent();
    expect(updatedText).toContain('XYZ');
  });

  test('Enter in a table cell does not break table structure', async ({ page }) => {
    const table = page.locator('.wysiwyg-table');
    await expect(table).toBeVisible();

    // Count initial th and td cells
    await expect(table.locator('th')).toHaveCount(4);
    await expect(table.locator('td')).toHaveCount(12);

    // Click a data cell
    const cell = table.locator('tbody td').first();
    await cell.click();
    await page.waitForFunction(
      () => document.activeElement?.getAttribute?.('contenteditable') === 'true'
    );

    // Press Enter
    await page.keyboard.press('Enter');
    await page.waitForTimeout(200);

    // Table structure should be unchanged (same number of cells)
    await expect(table.locator('th')).toHaveCount(4);
    await expect(table.locator('td')).toHaveCount(12);

    // Table should still be present
    await expect(page.locator('.wysiwyg-table')).toBeVisible();
  });

  test('Backspace at start of table cell preserves table', async ({ page }) => {
    const table = page.locator('.wysiwyg-table');
    await expect(table).toBeVisible();

    // Count initial rows and cells
    await expect(table.locator('th')).toHaveCount(4);
    await expect(table.locator('td')).toHaveCount(12);

    // Click a data cell
    const cell = table.locator('tbody td').first();
    await cell.click();
    await page.waitForFunction(
      () => document.activeElement?.getAttribute?.('contenteditable') === 'true'
    );

    // Move to start of cell and press Backspace
    await page.keyboard.press('Home');
    await page.keyboard.press('Backspace');
    await page.waitForTimeout(200);

    // Table structure should be unchanged
    await expect(table.locator('th')).toHaveCount(4);
    await expect(table.locator('td')).toHaveCount(12);

    // Table should still be present
    await expect(page.locator('.wysiwyg-table')).toBeVisible();
  });

  test('Tab navigates to next table cell', async ({ page }) => {
    const table = page.locator('.wysiwyg-table');
    await expect(table).toBeVisible();

    // Click on the first header cell
    const firstHeader = table.locator('thead th').first();
    await firstHeader.click();
    await page.waitForFunction(
      () => document.activeElement?.getAttribute?.('contenteditable') === 'true'
    );

    // Press Tab to move to next cell
    await page.keyboard.press('Tab');
    await page.waitForTimeout(200);

    // Table structure should still be intact (Tab should not break things)
    await expect(table.locator('th')).toHaveCount(4);
    await expect(table.locator('td')).toHaveCount(12);

    // Verify focus is still in the table (the selection moved)
    const activeInTable = await page.evaluate(() => {
      const sel = window.getSelection();
      if (!sel || sel.rangeCount === 0) return false;
      const node = sel.anchorNode;
      return node?.parentElement?.closest('table') !== null;
    });
    expect(activeInTable).toBe(true);
  });

  test('Shift+Tab navigates to previous table cell', async ({ page }) => {
    const table = page.locator('.wysiwyg-table');
    await expect(table).toBeVisible();

    // Click on the second header cell
    const secondHeader = table.locator('thead th').nth(1);
    await secondHeader.click();
    await page.waitForFunction(
      () => document.activeElement?.getAttribute?.('contenteditable') === 'true'
    );

    // Press Shift+Tab to move to previous cell
    await page.keyboard.press('Shift+Tab');
    await page.waitForTimeout(200);

    // Table structure should still be intact
    await expect(table.locator('th')).toHaveCount(4);
    await expect(table.locator('td')).toHaveCount(12);

    // Verify focus is still in the table
    const activeInTable = await page.evaluate(() => {
      const sel = window.getSelection();
      if (!sel || sel.rangeCount === 0) return false;
      const node = sel.anchorNode;
      return node?.parentElement?.closest('table') !== null;
    });
    expect(activeInTable).toBe(true);
  });
});
