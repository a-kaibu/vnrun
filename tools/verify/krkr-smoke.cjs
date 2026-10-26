// Run after building the player and KiriKiri wasm. All game data is synthetic.
const { chromium } = require('playwright');
const { spawn, execFileSync } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');
const root = path.resolve(__dirname, '../..');
const fixtures = path.join(root, 'target/krkr-smoke');
execFileSync('python3', [path.join(root, 'engines/krkr/tests/fixtures.py'), fixtures]);

(async () => {
  const browser = await chromium.launch({ executablePath: process.env.VNRUN_CHROMIUM, args: [
    '--enable-unsafe-webgpu', '--enable-features=Vulkan,SkiaGraphite', '--use-vulkan=swiftshader',
    '--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--autoplay-policy=no-user-gesture-required',
  ] });
  try {
    for (const [index, name] of ['mado', 'akabei', 'tlg', 'plaintext', 'pointer'].entries()) {
      const port = 8260 + index;
      const server = spawn(path.join(root, 'target/debug/vnrun-devserver'), [
        '--game', path.join(fixtures, name), '--saves', path.join(fixtures, name, 'saves'),
        '--addr', `127.0.0.1:${port}`,
      ], { cwd: root, stdio: ['ignore', 'pipe', 'pipe'] });
      const page = await browser.newPage({ viewport: { width: 640, height: 520 } });
      const log = [];
      let timer;
      try {
        await new Promise((resolve, reject) => {
          timer = setTimeout(() => reject(Error('server startup timeout')), 10000);
          server.stdout.on('data', data => { if (data.toString().includes('http://')) resolve(); });
          server.stderr.on('data', data => { log.push(data.toString()); if (data.toString().includes('http://')) resolve(); });
          server.once('exit', code => reject(Error(`server exited ${code}: ${log.join('\n')}`)));
        });
        clearTimeout(timer);
        const done = new Promise((resolve, reject) => {
          timer = setTimeout(() => reject(Error(`${name}: smoke timeout`)), 120000);
          page.on('console', message => {
            const text = message.text(); log.push(text);
            if (text === 'krkr: VNRUN_KRKR_SMOKE_OK' || text === 'krkr: VNRUN_KRKR_POINTER_OK') resolve();
            if (text === 'krkr: VNRUN_KRKR_POINTER_READY') {
              page.waitForFunction(() => document.querySelector('canvas')?.width === 640)
                .then(() => page.waitForTimeout(500)).then(() => page.mouse.click(80, 80)).catch(reject);
            }
            if (text.includes('Script exception raised')) reject(Error(`${name}: script exception`));
          });
          page.on('pageerror', reject);
        });
        await page.goto(`http://127.0.0.1:${port}/`);
        await done;
        console.log(`${name}: ${name === "pointer" ? "pointer reaches the visible window after creating a hidden helper" : "archive detection, decompression, script decoding and KAG conditions"} passed`);
      } finally {
        clearTimeout(timer);
        fs.writeFileSync(path.join(fixtures, `${name}.log`), log.join('\n'));
        await page.close();
        server.kill();
      }
    }
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
