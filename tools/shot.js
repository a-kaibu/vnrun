// Opens the player in headless Chromium, runs a list of steps and saves screenshots and the
// console log, to check a game without a screen.
//
//   node tools/shot.js <url> <out prefix> [steps]
//
// steps (comma separated):
//   wait:<sec>            wait
//   shot:<name>           screenshot to <out prefix>_<name>.png
//   at:<x>;<y>            click at page pixels (the page is 1280x800)
//   hold:<x>;<y>[;<ms>]   like at, hovering and holding the button for ms (default 1000)
//   key:<Key>             press a key (Playwright names: Enter, Space, ArrowDown, ...)
//   wheel:<x>;<y>;<dy>    turn the mouse wheel over page pixels (dy < 0 is up)
//   type:<text>           type text into the focused element
//   files:<sel>=<path>    put a file or folder into an <input type=file> (upstream launcher pages)
//   eval:<js>             log the JSON of a page expression (no commas)
//   dialog:yes|no         how later alert/confirm dialogs are answered (no by default)
// The console log (page and workers) goes to <out prefix>.log.
const { chromium } = require("playwright");
const fs = require("fs");

const [url, out, steps = "wait:30,shot:a"] = process.argv.slice(2);
if (!url || !out) {
  console.error("usage: node tools/shot.js <url> <out prefix> [steps]");
  process.exit(2);
}

(async () => {
  // WebGL and WebGPU through SwiftShader on machines without a GPU. WebGPU needs the full
  // Chromium (the headless shell cannot present a WebGPU canvas) with Vulkan and Graphite.
  const browser = await chromium.launch({
    executablePath: process.env.VNRUN_CHROMIUM || "/opt/pw-browsers/chromium-1243/chrome-linux64/chrome",
    args: [
      "--headless=new",
      "--enable-unsafe-webgpu",
      "--enable-features=Vulkan,SkiaGraphite",
      "--use-vulkan=swiftshader",
      "--use-angle=swiftshader",
      "--enable-unsafe-swiftshader",
      "--autoplay-policy=no-user-gesture-required",
    ],
  });
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  const log = [];
  const t0 = Date.now();
  const at = () => `${((Date.now() - t0) / 1000).toFixed(0)}s`;
  page.on("console", (m) => log.push(`[${m.type()}] ${m.text()}`));
  page.on("pageerror", (e) => log.push(`[pageerror] ${e.stack || e.message}`));
  page.on("response", (r) => r.status() >= 400 && log.push(`[http ${r.status()}] ${r.url()}`));
  // alert/confirm from the game (KiriKiri's System.inform, yes/no dialogs): logged, answered no
  // (or yes after a dialog:yes step). prompt (AdvHD name entry): accepted with its default, or a
  // placeholder name when it has none.
  let answerYes = false;
  page.on("dialog", (d) => {
    log.push(`[dialog ${d.type()}] ${d.message()}`);
    if (d.type() === "prompt") d.accept(d.defaultValue() || "テスト").catch(() => {});
    else (answerYes ? d.accept() : d.dismiss()).catch(() => {});
  });
  page.on("worker", (w) => w.on("console", (m) => log.push(`[worker ${m.type()}] ${m.text()}`)));
  process.on("exit", () => fs.writeFileSync(`${out}.log`, log.join("\n")));
  // Whole stacks in the log (a Rust panic's caller is often past the default 10 frames).
  await page.addInitScript(() => (Error.stackTraceLimit = 50));
  await page.goto(url);
  for (const step of steps.split(",")) {
    const [cmd, ...rest] = step.split(":");
    const arg = rest.join(":");
    if (cmd === "wait") {
      await page.waitForTimeout(Number(arg) * 1000);
    } else if (cmd === "shot") {
      // A busy software renderer can hold the page for a while.
      await page.screenshot({ path: `${out}_${arg}.png`, timeout: 120000 });
      log.push(`[shot ${arg} @${at()}]`);
    } else if (cmd === "at") {
      const [x, y] = arg.split(";").map(Number);
      // Hover first and hold the button briefly, as a person would: engines highlight a
      // button on hover and read button state once per frame.
      await page.mouse.move(x, y);
      await page.waitForTimeout(500);
      await page.mouse.down();
      await page.waitForTimeout(150);
      await page.mouse.up();
    } else if (cmd === "hold") {
      // Like at, with a longer hover and hold (ms) for slow software-rendered frames.
      const [x, y, ms = 1000] = arg.split(";").map(Number);
      await page.mouse.move(x, y);
      await page.waitForTimeout(ms);
      await page.mouse.down();
      await page.waitForTimeout(ms);
      await page.mouse.up();
    } else if (cmd === "key") {
      await page.keyboard.press(arg);
    } else if (cmd === "wheel") {
      const [x, y, dy] = arg.split(";").map(Number);
      await page.mouse.move(x, y);
      await page.mouse.wheel(0, dy);
    } else if (cmd === "type") {
      await page.keyboard.type(arg);
    } else if (cmd === "files") {
      const [sel, path] = arg.split("=");
      await page.setInputFiles(sel, path);
    } else if (cmd === "dialog") {
      answerYes = arg === "yes";
    } else if (cmd === "eval") {
      log.push(`[eval] ${JSON.stringify(await page.evaluate(arg))}`);
    } else {
      throw new Error(`unknown step: ${step}`);
    }
  }
  await browser.close();
})();
