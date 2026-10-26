// Regression test against `cd web && vp dev`. Requires Playwright like tools/shot.js.
// No game data required. Exercise the real Pads + GameFS with HTTP save responses.
const assert = require('node:assert/strict');
const { chromium } = require('playwright');

(async () => {
  const base = process.argv[2] || 'http://127.0.0.1:5173';
  const browser = await chromium.launch({ executablePath: process.env.VNRUN_CHROMIUM });
  try {
    const page = await browser.newPage();
    // An empty document avoids starting a game while preserving the Vite origin.
    await page.route('**/pad-test', route => route.fulfill({ contentType: 'text/html', body: '<canvas tabindex="0"></canvas>' }));
    let status = 503;
    const writes = [];
    await page.route('**/saves/**', route => {
      writes.push({ url: route.request().url(), body: route.request().postDataBuffer() });
      return route.fulfill({ status, body: '' });
    });
    await page.goto(`${base}/pad-test`);
    await page.evaluate(async () => {
      const { Pads } = await import('/src/engines/krkr-pad.ts');
      const { GameFS } = await import('/src/gamefs.ts');
      const canvas = document.querySelector('canvas');
      const fs = new GameFS('/test/', { engine: 'kirikiri', files: [{ p: 'Config.tjs', s: 2 }] });
      window.padTest = { pads: new Pads(fs, canvas), fs };
      window.padTest.pads.call(1, 'set', 'initial', '', '');
      window.padTest.pads.call(1, 'show', '', '設定の編集', 'file://./game/Config.tjs');
      window.padKeys = 0;
      window.addEventListener('keydown', () => window.padKeys++);
    });
    const dialog = page.getByRole('dialog', { name: '設定の編集' });
    const editor = dialog.getByRole('textbox', { name: '内容' });
    await editor.fill('設定の更新\n// 日本語');
    await editor.press('End');
    assert.equal(await page.evaluate(() => window.padKeys), 0, 'editor input reached the game');
    const save = dialog.getByRole('button', { name: '保存', exact: true });
    await save.click();
    await dialog.getByRole('status').getByText(/保存できませんでした/).waitFor();
    status = 200;
    await save.click();
    await dialog.getByRole('status').getByText('保存しました。', { exact: true }).waitFor();
    assert.equal(writes.length, 2);
    assert.ok(writes[1].url.endsWith('/saves/Config.tjs'));
    assert.deepEqual(writes[1].body, Buffer.from('\ufeff設定の更新\n// 日本語', 'utf16le'));
    assert.equal(await page.evaluate(() => window.padTest.fs.size('Config.tjs')), writes[1].body.length);
    await editor.press('Escape');
    await dialog.waitFor({ state: 'hidden' });
    await page.waitForFunction(() => document.activeElement.tagName === 'CANVAS');
    await page.evaluate(() => {
      window.padTest.pads.call(1, 'show', '', '設定の編集', 'file://./game/../outside.tjs');
    });
    await save.click();
    await dialog.getByRole('status').getByText(/ゲームフォルダ内/).waitFor();
    assert.equal(writes.length, 2, 'invalid path was sent to the server');
    console.log('Pad: UTF-16 save, failure/retry, saved size, input isolation, Escape/focus and path validation passed');
  } finally {
    await browser.close();
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
