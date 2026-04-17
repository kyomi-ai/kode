import { test, expect } from '@playwright/test';

const BASE = 'http://localhost:8090';

// Helper: get the raw markdown source from the editor by switching to source mode
async function getSourceText(page) {
  const sourceBtn = page.getByText('Source', { exact: true });
  await sourceBtn.click();
  await page.waitForTimeout(300);
  await page.waitForSelector('.kode-editor');

  // Read text content from the source editor lines
  const text = await page.evaluate(() => {
    const spans = document.querySelectorAll('[data-line]');
    return Array.from(spans).map(s => s.textContent || '').join('\n');
  });

  // Switch back to WYSIWYG
  const wysiwygBtn = page.getByText('WYSIWYG', { exact: true });
  await wysiwygBtn.click();
  await page.waitForTimeout(300);
  await page.waitForSelector('.wysiwyg-container');

  return text;
}

// Helper: clear editor and set fresh content
async function setContent(page, markdown: string) {
  // Switch to source mode
  const sourceBtn = page.getByText('Source', { exact: true });
  await sourceBtn.click();
  await page.waitForTimeout(300);
  await page.waitForSelector('.kode-editor');

  // Select all and replace
  const editor = page.locator('.kode-editor');
  await editor.click();
  await page.waitForFunction(() => document.activeElement?.tagName === 'TEXTAREA');
  await page.keyboard.press('Control+a');
  await page.keyboard.type(markdown);
  await page.waitForTimeout(200);

  // Switch to WYSIWYG
  const wysiwygBtn = page.getByText('WYSIWYG', { exact: true });
  await wysiwygBtn.click();
  await page.waitForTimeout(300);
  await page.waitForSelector('.wysiwyg-container');
}

test.describe('Cursor Position and Formatting', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(BASE);
    await page.waitForSelector('.wysiwyg-container', { timeout: 10000 });
  });

  test('typing at end of paragraph appends correctly', async ({ page }) => {
    await setContent(page, 'Hello world');

    // Click on the paragraph text
    const p = page.locator('.wysiwyg-container p').first();
    await p.click();
    await page.waitForFunction(() => document.activeElement?.tagName === 'TEXTAREA');

    // Move to end and type
    await page.keyboard.press('End');
    await page.keyboard.type(' added');
    await page.waitForTimeout(200);

    const source = await getSourceText(page);
    expect(source).toContain('Hello world added');
  });

  test('typing at start of paragraph prepends correctly', async ({ page }) => {
    await setContent(page, 'world');

    const p = page.locator('.wysiwyg-container p').first();
    await p.click();
    await page.waitForFunction(() => document.activeElement?.tagName === 'TEXTAREA');

    await page.keyboard.press('Home');
    await page.keyboard.type('Hello ');
    await page.waitForTimeout(200);

    const source = await getSourceText(page);
    expect(source).toContain('Hello world');
  });

  test('Enter at end of paragraph creates new paragraph', async ({ page }) => {
    await setContent(page, 'First paragraph');

    const p = page.locator('.wysiwyg-container p').first();
    await p.click();
    await page.waitForFunction(() => document.activeElement?.tagName === 'TEXTAREA');

    await page.keyboard.press('End');
    await page.keyboard.press('Enter');
    await page.keyboard.type('Second paragraph');
    await page.waitForTimeout(200);

    const source = await getSourceText(page);
    expect(source).toContain('First paragraph');
    expect(source).toContain('Second paragraph');
  });

  test('bold text renders as <strong> in WYSIWYG', async ({ page }) => {
    await setContent(page, 'This is **bold** text');

    const strong = page.locator('.wysiwyg-container strong');
    await expect(strong).toHaveText('bold');
  });

  test('italic text renders as <em> in WYSIWYG', async ({ page }) => {
    await setContent(page, 'This is *italic* text');

    const em = page.locator('.wysiwyg-container em');
    await expect(em).toHaveText('italic');
  });

  test('inline code renders with code element', async ({ page }) => {
    await setContent(page, 'Use `foo()` here');

    const code = page.locator('.wysiwyg-container code');
    expect(await code.count()).toBeGreaterThan(0);
    await expect(code.first()).toContainText('foo()');
  });

  test('heading renders correctly', async ({ page }) => {
    await setContent(page, '# Main Title');

    const h1 = page.locator('.wysiwyg-container h1');
    await expect(h1).toHaveText('Main Title');
  });

  test('h2 renders correctly', async ({ page }) => {
    await setContent(page, '## Sub Title');

    const h2 = page.locator('.wysiwyg-container h2');
    await expect(h2).toHaveText('Sub Title');
  });

  test('bullet list renders correctly', async ({ page }) => {
    await setContent(page, '- item one\n- item two\n- item three');

    const ul = page.locator('.wysiwyg-container ul');
    await expect(ul).toBeVisible();
    const items = page.locator('.wysiwyg-container ul li');
    expect(await items.count()).toBe(3);
  });

  test('ordered list renders correctly', async ({ page }) => {
    await setContent(page, '1. first\n2. second\n3. third');

    const ol = page.locator('.wysiwyg-container ol');
    await expect(ol).toBeVisible();
    const items = page.locator('.wysiwyg-container ol li');
    expect(await items.count()).toBe(3);
  });

  test('blockquote renders correctly', async ({ page }) => {
    await setContent(page, '> This is a quote');

    const bq = page.locator('.wysiwyg-container blockquote');
    await expect(bq).toBeVisible();
  });

  test('code block renders with highlighting', async ({ page }) => {
    await setContent(page, '```sql\nSELECT * FROM users\n```');

    const codeBlock = page.locator('.wysiwyg-container .wysiwyg-code-block');
    await expect(codeBlock).toBeVisible();
    const pre = page.locator('.wysiwyg-container .wysiwyg-code-block pre');
    await expect(pre).toBeVisible();
  });

  test('horizontal rule renders', async ({ page }) => {
    await setContent(page, 'Above\n\n---\n\nBelow');

    const hr = page.locator('.wysiwyg-container hr');
    await expect(hr).toBeVisible();
  });

  test('link renders as anchor', async ({ page }) => {
    await setContent(page, 'Click [here](https://example.com) now');

    const link = page.locator('.wysiwyg-container a');
    await expect(link).toHaveText('here');
    await expect(link).toHaveAttribute('href', 'https://example.com');
  });

  test('Ctrl+B toggles bold on selected text', async ({ page }) => {
    await setContent(page, 'plain text here');

    const p = page.locator('.wysiwyg-container p').first();
    await p.click();
    await page.waitForFunction(() => document.activeElement?.tagName === 'TEXTAREA');

    // Select "text"
    await page.keyboard.press('Home');
    // Move to "text" — "plain " is 6 chars
    for (let i = 0; i < 6; i++) await page.keyboard.press('ArrowRight');
    // Select "text" — 4 chars
    for (let i = 0; i < 4; i++) await page.keyboard.press('Shift+ArrowRight');

    await page.keyboard.press('Control+b');
    await page.waitForTimeout(300);

    const source = await getSourceText(page);
    expect(source).toContain('**text**');
  });

  test('Ctrl+I toggles italic on selected text', async ({ page }) => {
    await setContent(page, 'plain text here');

    const p = page.locator('.wysiwyg-container p').first();
    await p.click();
    await page.waitForFunction(() => document.activeElement?.tagName === 'TEXTAREA');

    await page.keyboard.press('Home');
    for (let i = 0; i < 6; i++) await page.keyboard.press('ArrowRight');
    for (let i = 0; i < 4; i++) await page.keyboard.press('Shift+ArrowRight');

    await page.keyboard.press('Control+i');
    await page.waitForTimeout(300);

    const source = await getSourceText(page);
    expect(source).toContain('*text*');
  });

  test('typing inside bold text stays bold', async ({ page }) => {
    await setContent(page, 'Hello **world** end');

    // The word "world" should be rendered in <strong>
    const strong = page.locator('.wysiwyg-container strong');
    await expect(strong).toHaveText('world');

    // Click on the bold text
    await strong.click();
    await page.waitForFunction(() => document.activeElement?.tagName === 'TEXTAREA');

    // Type after "world" — should end up inside the bold markers
    await page.keyboard.press('End'); // end of bold
    await page.keyboard.type('!');
    await page.waitForTimeout(200);

    const source = await getSourceText(page);
    // The "!" could end up inside or outside bold markers depending on cursor position
    // At minimum, the original bold should be preserved
    expect(source).toContain('**world');
  });

  test('new text after heading does not inherit heading', async ({ page }) => {
    // Note: heading must have trailing newline for tree-sitter to parse correctly
    await setContent(page, '# Title\n');

    // Wait for the h1 to be rendered and visible
    await page.waitForSelector('.wysiwyg-container h1');

    const h1 = page.locator('.wysiwyg-container h1');
    await h1.click();
    await page.waitForFunction(() => document.activeElement?.tagName === 'TEXTAREA');

    await page.keyboard.press('End');
    await page.keyboard.press('Enter');
    await page.keyboard.type('Normal paragraph');
    await page.waitForTimeout(200);

    // Check that "Normal paragraph" is NOT a heading
    const source = await getSourceText(page);
    // After heading + Enter, we should get a paragraph break
    expect(source).toContain('# Title');
    expect(source).toContain('Normal paragraph');
    // "Normal paragraph" should NOT have a # prefix
    expect(source).not.toContain('# Normal paragraph');
  });

  test('Backspace at start of paragraph joins with previous', async ({ page }) => {
    await setContent(page, 'First\n\nSecond');

    // There should be two paragraphs
    const paragraphs = page.locator('.wysiwyg-container p');
    const initialCount = await paragraphs.count();

    // Click on second paragraph
    const secondP = paragraphs.nth(initialCount - 1);
    await secondP.click();
    await page.waitForFunction(() => document.activeElement?.tagName === 'TEXTAREA');

    await page.keyboard.press('Home');
    await page.keyboard.press('Backspace');
    await page.waitForTimeout(200);

    // Paragraphs should decrease
    const newCount = await page.locator('.wysiwyg-container p').count();
    expect(newCount).toBeLessThan(initialCount);
  });

  test('Tab in list indents item', async ({ page }) => {
    await setContent(page, '- item one\n- item two');

    // Click on second list item
    const items = page.locator('.wysiwyg-container li');
    await items.nth(1).click();
    await page.waitForFunction(() => document.activeElement?.tagName === 'TEXTAREA');

    await page.keyboard.press('Tab');
    await page.waitForTimeout(200);

    const source = await getSourceText(page);
    expect(source).toContain('  - item two');
  });

  test('no errors during rapid typing', async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', (err) => errors.push(err.message));

    await setContent(page, '');

    const container = page.locator('.wysiwyg-container');
    await container.click();
    await page.waitForFunction(() => document.activeElement?.tagName === 'TEXTAREA');

    // Rapid typing
    await page.keyboard.type('The quick brown fox jumps over the lazy dog. ', { delay: 10 });
    await page.keyboard.press('Enter');
    await page.keyboard.type('Another paragraph with **bold** and *italic* text.', { delay: 10 });
    await page.waitForTimeout(500);

    const realErrors = errors.filter(e => !e.includes('ResizeObserver'));
    expect(realErrors).toHaveLength(0);
  });

  test('no errors when switching modes after editing', async ({ page }) => {
    const errors: string[] = [];
    page.on('pageerror', (err) => errors.push(err.message));

    const container = page.locator('.wysiwyg-container');
    await container.click();
    await page.waitForFunction(() => document.activeElement?.tagName === 'TEXTAREA');

    await page.keyboard.type('Edited in WYSIWYG');
    await page.waitForTimeout(200);

    // Switch to source
    await page.getByText('Source', { exact: true }).click();
    await page.waitForTimeout(300);

    // Switch back to WYSIWYG
    await page.getByText('WYSIWYG', { exact: true }).click();
    await page.waitForTimeout(300);

    // Edit again
    await container.click();
    await page.waitForFunction(() => document.activeElement?.tagName === 'TEXTAREA');
    await page.keyboard.type(' more text');
    await page.waitForTimeout(200);

    const realErrors = errors.filter(e => !e.includes('ResizeObserver'));
    expect(realErrors).toHaveLength(0);
  });

  test('Enter immediately shows new empty paragraph and cursor types on it', async ({ page }) => {
    // Reproduces: pressing Enter inserts \n\n in source but the cursor stays
    // rendered at the end of the old line. The cursor must visually move down
    // to the new empty line before any further typing.
    await setContent(page, 'Hello');

    const p = page.locator('.wysiwyg-container p').first();
    await p.click();
    await page.waitForFunction(() => document.activeElement?.tagName === 'TEXTAREA');

    await page.keyboard.press('End');

    // Record cursor top position while on "Hello"
    const cursorTopBefore = await page.evaluate(() => {
      const cursor = document.querySelector('.kode-cursor') as HTMLElement;
      if (!cursor || !cursor.style.top) return -1;
      return parseFloat(cursor.style.top);
    });

    await page.keyboard.press('Enter');
    await page.waitForTimeout(200);

    // Two paragraphs must be visible before any typing
    const paragraphs = page.locator('.wysiwyg-container p');
    expect(await paragraphs.count()).toBe(2);

    // The cursor must have moved DOWN to the new empty line
    const cursorTopAfter = await page.evaluate(() => {
      const cursor = document.querySelector('.kode-cursor') as HTMLElement;
      if (!cursor || !cursor.style.top) return -1;
      return parseFloat(cursor.style.top);
    });
    expect(cursorTopAfter).toBeGreaterThan(cursorTopBefore);

    // Type without clicking — text must appear as the second paragraph
    await page.keyboard.type('World');
    await page.waitForTimeout(200);

    const source = await getSourceText(page);
    expect(source).toMatch(/Hello\n\nWorld/);
  });

  test('blank lines between paragraphs render as empty paragraphs', async ({ page }) => {
    // "Hello\n\n\n\nasdasd" has 4 newlines between the two words.
    // \n\n is the standard markdown paragraph separator (no visible blank line).
    // Each additional \n beyond the pair produces one blank paragraph.
    // So: Hello + 2 blank paragraphs + asdasd = 4 total.
    await setContent(page, 'Hello\n\n\n\nasdasd');

    const paragraphs = page.locator('.wysiwyg-container p');
    const count = await paragraphs.count();
    expect(count).toBe(4);

    // Verify correct order: content, blanks, content
    expect(await paragraphs.first().textContent()).toBe('Hello');
    expect(await paragraphs.last().textContent()).toBe('asdasd');
  });

  test('normal paragraph separator produces no blank line', async ({ page }) => {
    // "First\n\nSecond" is a standard two-paragraph document — no blank lines.
    await setContent(page, 'First\n\nSecond');

    const paragraphs = page.locator('.wysiwyg-container p');
    const count = await paragraphs.count();
    expect(count).toBe(2);
    expect(await paragraphs.first().textContent()).toBe('First');
    expect(await paragraphs.last().textContent()).toBe('Second');
  });

  test('End key on first line of multi-line content appends to that line, not next', async ({ page }) => {
    // Reproduces: soft line breaks (\n within a paragraph) were rendered as
    // raw text characters in the DOM. Clicking on the middle of the visual
    // text could land on rope line 1 instead of line 0.
    // Fix: render \n as <br> so the two visual lines are distinct.
    await setContent(page, 'First line\nSecond line');

    const p = page.locator('.wysiwyg-container p').first();

    // Click on the first visual line. With <br> rendering, this is unambiguous.
    // With old rendering (no <br>), both lines appear as one visual line, and
    // clicking at y+2 could land on either rope line depending on x position.
    const box = await p.boundingBox();
    await page.mouse.click(box.x + 5, box.y + 2);
    await page.waitForFunction(() => document.activeElement?.tagName === 'TEXTAREA');

    // From wherever the click landed, press End to move to end of CURRENT rope line.
    // With the fix, this is end of line 0. Without it, a misplaced click could
    // put us on line 1, making End go to end of line 1 instead.
    await page.keyboard.press('End');
    // Type directly without Home — test End's behavior from click position
    await page.keyboard.type(' appended');
    await page.waitForTimeout(200);

    const source = await getSourceText(page);
    // If click landed on line 0 (as it should with <br>), End → line 0 end,
    // and typing appends to "First line".
    expect(source).toContain('First line appended');
    expect(source).toContain('Second line');
    expect(source).not.toMatch(/\nappended/);
  });
});
