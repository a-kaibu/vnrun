// Opens the player in headless Chromium, runs a list of steps and saves screenshots and the
// console log, to check a game without a screen.
//
//   node tools/shot.js <url> <out prefix> [steps]
//
// steps (comma separated):
//   wait:<sec>            wait
//   shot:<name>           screenshot to <out prefix>_<name>.png
//   at:<x>;<y>            click at page pixels (the page is 1280x800)
//   key:<Key>             press a key (Playwright names: Enter, Space, ArrowDown, ...)
//   files:<sel>=<path>    put a file or folder into an <input type=file> (upstream launcher pages)
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
  page.on("pageerror", (e) => log.push(`[pageerror] ${e.message}`));
  page.on("worker", (w) => w.on("console", (m) => log.push(`[worker ${m.type()}] ${m.text()}`)));
  process.on("exit", () => fs.writeFileSync(`${out}.log`, log.join("\n")));
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
      await page.mouse.click(x, y);
    } else if (cmd === "key") {
      await page.keyboard.press(arg);
    } else if (cmd === "files") {
      const [sel, path] = arg.split("=");
      await page.setInputFiles(sel, path);
    } else {
      throw new Error(`unknown step: ${step}`);
    }
  }
  await browser.close();
})();
