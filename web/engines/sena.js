// Softpal (PAL) through sena-rs (third_party/sena-rs), built by engines/sena/build.sh into
// ./sena/. The engine draws its frame into the canvas at the game's own size and reads the
// pointer against the canvas's on-screen box, so the canvas is kept at the game's aspect
// ratio here instead of being stretched over the page.
import init, { start_sena_from_directory } from "./sena/pal_vm.js";

// sena-rs reads files through these globals (crates/pal-asset/src/vfs.rs).
function bind(fs) {
  globalThis.senaFileExists = (path) => fs.exists(path);
  globalThis.senaFileSize = (path) => fs.size(path);
  globalThis.senaReadFile = (path) => fs.read(path);
  globalThis.senaReadRange = (path, offset, length) => fs.readRange(path, Number(offset), Number(length));
  globalThis.senaListDir = (path) => fs.list(path);
  globalThis.senaKnownFileCount = () => fs.fileCount();
}

// Fits the canvas into the area the page gives it, keeping the game's aspect ratio.
function fit(canvas) {
  const area = canvas.parentElement.getBoundingClientRect();
  const bar = document.getElementById("bar");
  const height = window.innerHeight - (bar ? bar.offsetHeight : 0);
  const scale = Math.min(area.width / canvas.width, height / canvas.height);
  const w = Math.floor(canvas.width * scale);
  const h = Math.floor(canvas.height * scale);
  canvas.style.width = `${w}px`;
  canvas.style.height = `${h}px`;
  canvas.style.left = `${Math.floor((area.width - w) / 2)}px`;
  canvas.style.top = `${Math.floor((height - h) / 2)}px`;
}

export async function start({ fs, manifest, ui }) {
  bind(fs);
  await init();
  const { canvas } = ui;
  // The engine sets the canvas size to the game's size on its first frame.
  new MutationObserver(() => fit(canvas)).observe(canvas, { attributes: true, attributeFilter: ["width", "height"] });
  window.addEventListener("resize", () => fit(canvas));
  ui.hide();
  canvas.focus();
  start_sena_from_directory(canvas.id, "", manifest.nls || "sjis");
}
