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

/**
 * Capture the clipboard payload written by the editor's copy/cut handler.
 *
 * The editor intercepts the native copy/cut event and writes to
 * event.clipboardData synchronously (via preventDefault + setData).
 * We register a document-level listener that reads the data synchronously
 * during the same event dispatch — after the editor's handler has written it.
 *
 * Returns the { plainText, htmlText } captured from the event.
 */
async function captureClipboard(
  page,
  action: () => Promise<void>,
  eventType: 'copy' | 'cut' = 'copy',
): Promise<{ plainText: string; htmlText: string }> {
  // Register capture listener before the action fires the event.
  await page.evaluate((evType: string) => {
    (window as any).__kodeClipCapture = null;
    document.addEventListener(
      evType,
      (ev: ClipboardEvent) => {
        // Must read synchronously — clipboardData becomes empty after the event.
        const plainText = ev.clipboardData?.getData('text/plain') ?? '';
        const htmlText = ev.clipboardData?.getData('text/html') ?? '';
        (window as any).__kodeClipCapture = { plainText, htmlText };
      },
      { once: true },
    );
  }, eventType);

  await action();
  await page.waitForTimeout(200);

  return page.evaluate(() => (window as any).__kodeClipCapture) as Promise<{
    plainText: string;
    htmlText: string;
  }>;
}

/**
 * Dispatch a paste ClipboardEvent directly on the WYSIWYG editor element.
 *
 * The editor's `paste` fallback handler (tree_editor.rs) reads event.clipboardData
 * directly. We provide the same `<pre data-kode-md>` HTML format that the editor
 * writes on copy, causing the editor to call insert_from_markdown with the payload.
 */
async function dispatchPaste(
  page,
  clipboardData: { plainText: string; htmlText: string },
) {
  await page.evaluate(
    ({ plainText, htmlText }: { plainText: string; htmlText: string }) => {
      const editor = document.querySelector(
        '.wysiwyg-scroll-container',
      ) as HTMLElement | null;
      if (!editor) throw new Error('WYSIWYG editor element not found');

      const dt = new DataTransfer();
      dt.setData('text/plain', plainText);
      dt.setData('text/html', htmlText);

      const pasteEvent = new ClipboardEvent('paste', {
        bubbles: true,
        cancelable: true,
        clipboardData: dt,
      });
      editor.dispatchEvent(pasteEvent);
    },
    clipboardData,
  );
}

/**
 * Build the kode-md clipboard payload for a given markdown string.
 *
 * Produces the same format the editor's copy handler writes:
 *   text/html  = <pre data-kode-md>{html-escaped-markdown}</pre>
 *   text/plain = the markdown as-is
 */
function kodeClipboardPayload(markdown: string): {
  plainText: string;
  htmlText: string;
} {
  const htmlEscaped = markdown
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
  return {
    plainText: markdown,
    htmlText: `<pre data-kode-md>${htmlEscaped}</pre>`,
  };
}

test.describe('Atomic Block Clipboard Operations', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(BASE);
    await page.waitForSelector('.wysiwyg-container', { timeout: 10000 });
  });

  // ── Test 1: Copy a selection spanning an atomic block, paste elsewhere ───
  test('copy selection spanning atomic block then paste produces a second atomic block', async ({
    page,
  }) => {
    await setContent(
      page,
      'Alpha\n\n```chart\ntitle: Copy Test\ntype: bar\n```\n\nBeta\n\nGamma',
    );

    // The chart block renders as a non-editable div with data-kode-extension="chart"
    const chartBlocks = page.locator('[data-kode-extension="chart"]');
    await expect(chartBlocks).toHaveCount(1);

    // Click the first paragraph ("Alpha"), move to end, then extend the
    // selection rightward: cross the paragraph boundary → skip the atomic
    // chart block → land one character into "Beta".
    const firstPara = page.locator('.wysiwyg-container p').first();
    await firstPara.click();
    await page.waitForFunction(
      () => document.activeElement?.getAttribute?.('contenteditable') === 'true',
    );
    await page.keyboard.press('End');
    await page.waitForTimeout(100);

    // Shift+ArrowRight 3×:
    //   1st press → selection extends past the paragraph end (to gap before chart)
    //   2nd press → selection jumps over the full atomic chart block
    //   3rd press → selection extends into the first character of "Beta"
    await page.keyboard.press('Shift+ArrowRight');
    await page.waitForTimeout(50);
    await page.keyboard.press('Shift+ArrowRight');
    await page.waitForTimeout(50);
    await page.keyboard.press('Shift+ArrowRight');
    await page.waitForTimeout(50);

    // Capture the clipboard data written by the editor's copy handler
    const clipboard = await captureClipboard(
      page,
      () => page.keyboard.press('Control+c'),
      'copy',
    );

    // The editor writes kode-md HTML for spans that include atomic blocks
    expect(clipboard.htmlText).toContain('data-kode-md');
    expect(clipboard.htmlText).toContain('```chart');

    // Move cursor to the end of "Gamma" (the last paragraph) and paste
    const lastPara = page.locator('.wysiwyg-container p').last();
    await lastPara.click();
    await page.waitForFunction(
      () => document.activeElement?.getAttribute?.('contenteditable') === 'true',
    );
    await page.keyboard.press('End');
    await page.waitForTimeout(100);

    await dispatchPaste(page, clipboard);
    await page.waitForTimeout(300);

    // There should now be TWO atomic chart blocks in the document
    await expect(chartBlocks).toHaveCount(2);

    // The source markdown should contain the chart fenced block at least twice
    const source = await getSourceText(page);
    expect(source).toContain('title: Copy Test');
    const chartOccurrences = (source.match(/```chart/g) ?? []).length;
    expect(chartOccurrences).toBeGreaterThanOrEqual(2);
  });

  // ── Test 2: Cut an atomic block, verify removed, paste back ─────────────
  test('cut atomic block removes it from document and paste restores it as atomic', async ({
    page,
  }) => {
    await setContent(
      page,
      'Before\n\n```chart\ntitle: Cut Me\ntype: line\n```\n\nAfter',
    );

    const chartBlocks = page.locator('[data-kode-extension="chart"]');
    await expect(chartBlocks).toHaveCount(1);

    // Navigate to the start of "After", then extend the selection BACKWARD
    // two steps: the first Shift+ArrowLeft jumps to the gap after the chart
    // block, the second skips the full atomic chart block.
    const afterPara = page.locator('.wysiwyg-container p').last();
    await afterPara.click();
    await page.waitForFunction(
      () => document.activeElement?.getAttribute?.('contenteditable') === 'true',
    );
    await page.keyboard.press('Home');
    await page.waitForTimeout(100);

    await page.keyboard.press('Shift+ArrowLeft'); // to gap after chart block
    await page.waitForTimeout(50);
    await page.keyboard.press('Shift+ArrowLeft'); // select across the chart block
    await page.waitForTimeout(50);

    // Capture cut clipboard data and perform the cut
    const cutData = await captureClipboard(
      page,
      () => page.keyboard.press('Control+x'),
      'cut',
    );

    await page.waitForTimeout(300);

    // The cut clipboard should carry the chart markdown
    expect(cutData.htmlText).toContain('data-kode-md');
    expect(cutData.htmlText).toContain('```chart');

    // The chart block must be gone from the DOM
    await expect(chartBlocks).toHaveCount(0);

    // Source no longer contains the chart fence
    const sourceAfterCut = await getSourceText(page);
    expect(sourceAfterCut).not.toContain('```chart');
    expect(sourceAfterCut).not.toContain('title: Cut Me');

    // Move cursor to the end of the document and paste the cut data back
    const lastPara = page.locator('.wysiwyg-container p').last();
    await lastPara.click();
    await page.waitForFunction(
      () => document.activeElement?.getAttribute?.('contenteditable') === 'true',
    );
    await page.keyboard.press('End');
    await page.waitForTimeout(100);

    await dispatchPaste(page, cutData);
    await page.waitForTimeout(300);

    // Chart block should be back in the DOM as an atomic block
    await expect(chartBlocks).toHaveCount(1);

    const sourceAfterPaste = await getSourceText(page);
    expect(sourceAfterPaste).toContain('```chart');
    expect(sourceAfterPaste).toContain('title: Cut Me');

    // Verify the restored chart block is still atomic by confirming the cursor
    // skips over it. Navigate from the paragraph after the chart block backward
    // and type — it must not land inside the chart.
    const paras = page.locator('.wysiwyg-container p');
    const lastParaAgain = paras.last();
    await lastParaAgain.click();
    await page.waitForFunction(
      () => document.activeElement?.getAttribute?.('contenteditable') === 'true',
    );
    await page.keyboard.press('Home');
    await page.waitForTimeout(100);
    await page.keyboard.press('ArrowLeft'); // gap after restored chart block
    await page.waitForTimeout(100);
    await page.keyboard.press('ArrowLeft'); // skip over the atomic chart block
    await page.waitForTimeout(100);
    await page.keyboard.type('CHECK');
    await page.waitForTimeout(200);

    const finalSource = await getSourceText(page);
    // Typed text must NOT appear inside the chart fenced block
    expect(finalSource).not.toMatch(/title: Cut MeCHECK/);
    expect(finalSource).not.toMatch(/```chart\nCHECK/);
    // Chart content must remain intact
    expect(finalSource).toContain('title: Cut Me');
  });

  // ── Test 3: External paste with fenced code block matching atomic language
  test('pasting external kode-md markdown containing a chart block makes it atomic', async ({
    page,
  }) => {
    await setContent(page, 'Intro paragraph');

    // No chart blocks yet
    const chartBlocks = page.locator('[data-kode-extension="chart"]');
    await expect(chartBlocks).toHaveCount(0);

    // Place cursor at end of the intro paragraph
    const p = page.locator('.wysiwyg-container p').first();
    await p.click();
    await page.waitForFunction(
      () => document.activeElement?.getAttribute?.('contenteditable') === 'true',
    );
    await page.keyboard.press('End');
    await page.waitForTimeout(100);

    // Paste markdown that contains a chart fenced code block, using the kode-md
    // clipboard format so the editor calls insert_from_markdown.
    const externalMarkdown =
      '\n\n```chart\ntitle: External Chart\ntype: scatter\n```\n\nAppended text';
    await dispatchPaste(page, kodeClipboardPayload(externalMarkdown));
    await page.waitForTimeout(300);

    // The chart block should now be in the DOM as an atomic extension block
    await expect(chartBlocks).toHaveCount(1);

    // Source should include the fenced chart block and the appended text
    const source = await getSourceText(page);
    expect(source).toContain('```chart');
    expect(source).toContain('title: External Chart');
    expect(source).toContain('type: scatter');
    expect(source).toContain('Appended text');

    // Verify the pasted chart block is atomic: navigate to the paragraph after
    // it and arrow backward — the cursor should skip the chart block entirely.
    const lastPara = page.locator('.wysiwyg-container p').last();
    await lastPara.click();
    await page.waitForFunction(
      () => document.activeElement?.getAttribute?.('contenteditable') === 'true',
    );
    await page.keyboard.press('Home');
    await page.waitForTimeout(100);
    await page.keyboard.press('ArrowLeft'); // gap after chart block
    await page.waitForTimeout(100);
    await page.keyboard.press('ArrowLeft'); // skip the atomic chart block
    await page.waitForTimeout(100);
    await page.keyboard.type('OUTSIDE');
    await page.waitForTimeout(200);

    const sourceAfter = await getSourceText(page);
    // Typed text must NOT appear inside the chart block
    expect(sourceAfter).not.toMatch(/title: External ChartOUTSIDE/);
    expect(sourceAfter).not.toMatch(/```chart\nOUTSIDE/);
    // Chart content must remain intact
    expect(sourceAfter).toContain('title: External Chart');
  });

  // ── Test 4: Paste plain text (no kode-md wrapper) stays as regular text ──
  test('pasting plain text without kode-md wrapper does not create atomic blocks', async ({
    page,
  }) => {
    await setContent(page, 'Existing content');

    const p = page.locator('.wysiwyg-container p').first();
    await p.click();
    await page.waitForFunction(
      () => document.activeElement?.getAttribute?.('contenteditable') === 'true',
    );
    await page.keyboard.press('End');
    await page.waitForTimeout(100);

    // Paste via plain text/plain only (no text/html, no data-kode-md marker)
    await page.evaluate(() => {
      const editor = document.querySelector(
        '.wysiwyg-scroll-container',
      ) as HTMLElement | null;
      if (!editor) throw new Error('WYSIWYG editor element not found');

      const dt = new DataTransfer();
      dt.setData('text/plain', ' — appended plain text');
      // No text/html provided: the editor falls back to insert_text_multiline

      const pasteEvent = new ClipboardEvent('paste', {
        bubbles: true,
        cancelable: true,
        clipboardData: dt,
      });
      editor.dispatchEvent(pasteEvent);
    });
    await page.waitForTimeout(300);

    // No atomic chart blocks should have been created
    const chartBlocks = page.locator('[data-kode-extension="chart"]');
    await expect(chartBlocks).toHaveCount(0);

    // The plain text must appear in source
    const source = await getSourceText(page);
    expect(source).toContain('appended plain text');
  });
});
