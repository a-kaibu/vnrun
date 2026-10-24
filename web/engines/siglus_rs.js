// SiglusEngine, RealLive, AVG32 and UK2 through siglus_rs (third_party/siglus_rs), built by
// engines/siglus_rs/build.sh into ./siglus_rs/.
import init, { start_siglus_from_directory, launcherAddFont, WebGame } from "./siglus_rs/game_launcher.js";

// siglus_rs reads files through these globals (crates/game_fs, crates/siglus_scene_vm/src/wasm_vfs.rs).
function bind(fs) {
  globalThis.siglusFileExists = (path) => fs.exists(path);
  globalThis.siglusReadFile = (path) => fs.read(path);
  globalThis.siglusListDir = (path) => fs.list(path);
  globalThis.siglusKnownFileCount = () => fs.fileCount();
  globalThis.gameFsWrite = (path, bytes) => fs.write(path, bytes);
  globalThis.gameFsRemove = (path) => fs.remove(path);
}

export async function start({ fs, manifest, ui, fonts }) {
  bind(fs);
  await init();
  for (const font of fonts) launcherAddFont(font);
  ui.hide();
  ui.canvas.focus();
  if (manifest.engine === "siglus") {
    if (!fs.exists("Scene.pck")) throw new Error("Scene.pck がありません");
    // The Siglus host runs its own event loop on the canvas.
    await start_siglus_from_directory(ui.canvas.id, JSON.stringify(fs.fileList()));
    return;
  }
  framebuffer(new WebGame("", manifest.engine, manifest.nls || undefined), ui);
}

// Key codes of the siglus_rs web API (crates/game_launcher/src/ffi.rs).
const KEY_CODES = {
  Enter: 1, NumpadEnter: 1, Escape: 2, Space: 3,
  ArrowUp: 4, ArrowDown: 5, ArrowLeft: 6, ArrowRight: 7,
  PageUp: 8, PageDown: 9, Home: 10, End: 11, Backspace: 12, Tab: 13,
  ControlLeft: 14, ControlRight: 14, ShiftLeft: 15, ShiftRight: 15,
};
const KEY_CTRL = 14;
const KEY_SHIFT = 15;

function keyCode(event) {
  if (event.code in KEY_CODES) return KEY_CODES[event.code];
  const f = /^F(\d{1,2})$/.exec(event.code);
  if (f && Number(f[1]) >= 1 && Number(f[1]) <= 12) return 0x100 + Number(f[1]);
  if (event.key && [...event.key].length === 1) return 0x10000 + event.key.codePointAt(0);
  return 0;
}

// RealLive / AVG32 / UK2 draw into an RGBA frame that is scaled onto the canvas.
function framebuffer(web, ui) {
  const { canvas } = ui;
  const ctx = canvas.getContext("2d");
  const back = document.createElement("canvas");
  const backCtx = back.getContext("2d");
  let image = null;
  let rect = { x: 0, y: 0, w: 1, h: 1 };
  let skipping = false;
  let last = 0;
  // Input waits for the next frame, and a release never lands in the same frame as its
  // press: the engine reads button and key state once per frame, so a quick click or key
  // tap would otherwise come and go between two reads.
  const queue = [];
  const input = (fn, edge) => queue.push({ fn, edge });
  function applyInput() {
    const pressed = new Set();
    while (queue.length > 0) {
      const { fn, edge } = queue[0];
      if (edge && !edge.down && pressed.has(edge.id)) break;
      queue.shift();
      fn();
      if (edge && edge.down) pressed.add(edge.id);
    }
  }
  ui.menu.hidden = false;
  ui.skip.hidden = false;

  function layout() {
    const dpr = window.devicePixelRatio || 1;
    const cw = Math.max(1, Math.floor(canvas.clientWidth * dpr));
    const ch = Math.max(1, Math.floor(canvas.clientHeight * dpr));
    if (canvas.width !== cw || canvas.height !== ch) {
      canvas.width = cw;
      canvas.height = ch;
    }
    let scale = Math.min(cw / back.width, ch / back.height);
    const integer = Math.floor(scale);
    if (integer >= 1 && integer / scale > 0.92) scale = integer;
    const w = Math.round(back.width * scale);
    const h = Math.round(back.height * scale);
    rect = { x: Math.floor((cw - w) / 2), y: Math.floor((ch - h) / 2), w, h };
  }

  function toFrame(event) {
    const bounds = canvas.getBoundingClientRect();
    const dpr = canvas.width / Math.max(1, bounds.width);
    const x = Math.floor((((event.clientX - bounds.left) * dpr - rect.x) * back.width) / Math.max(1, rect.w));
    const y = Math.floor((((event.clientY - bounds.top) * dpr - rect.y) * back.height) / Math.max(1, rect.h));
    return [Math.max(0, Math.min(back.width - 1, x)), Math.max(0, Math.min(back.height - 1, y))];
  }

  function frame(now) {
    const dt = last ? Math.max(0, Math.min(100, Math.round(now - last))) : 0;
    last = now;
    applyInput();
    if (!web.step(dt)) {
      web.free();
      ui.exit();
      return;
    }
    const w = web.width();
    const h = web.height();
    if (w > 0 && h > 0) {
      if (!image || image.width !== w || image.height !== h) {
        back.width = w;
        back.height = h;
        image = backCtx.createImageData(w, h);
      }
      image.data.set(web.frame());
      backCtx.putImageData(image, 0, 0);
      layout();
      ctx.imageSmoothingEnabled = false;
      ctx.fillStyle = "#000";
      ctx.fillRect(0, 0, canvas.width, canvas.height);
      ctx.drawImage(back, rect.x, rect.y, rect.w, rect.h);
      canvas.style.cursor = web.cursorVisible() ? "default" : "none";
    }
    requestAnimationFrame(frame);
  }

  const button = (down) => (e) => {
    if (down) canvas.focus();
    const at = toFrame(e);
    input(() => web.pointerMove(...at));
    if (e.button === 0 || e.button === 2) {
      const id = e.button === 2 ? 1 : 0;
      input(() => web.pointerButton(id, down), { id: `b${id}`, down });
    }
    e.preventDefault();
  };
  canvas.addEventListener("pointermove", (e) => {
    const at = toFrame(e);
    input(() => web.pointerMove(...at));
  });
  canvas.addEventListener("pointerdown", button(true));
  canvas.addEventListener("pointerup", button(false));
  canvas.addEventListener("contextmenu", (e) => e.preventDefault());
  canvas.addEventListener("wheel", (e) => {
    web.wheel(e.deltaY < 0);
    e.preventDefault();
  }, { passive: false });
  window.addEventListener("keydown", (e) => {
    if (e.metaKey) return;
    const code = keyCode(e);
    if (code) input(() => web.key(code, true), { id: `k${code}`, down: true });
    if (!e.ctrlKey && !e.altKey && e.key && [...e.key].length === 1) {
      const { key } = e;
      input(() => web.text(key));
    }
    if (code) e.preventDefault();
  });
  window.addEventListener("keyup", (e) => {
    const code = keyCode(e);
    if (code) {
      input(() => web.key(code, false), { id: `k${code}`, down: false });
      e.preventDefault();
    }
  });
  window.addEventListener("blur", () => {
    web.key(KEY_CTRL, skipping);
    web.key(KEY_SHIFT, false);
  });
  ui.menu.addEventListener("click", () => {
    input(() => web.pointerButton(1, true), { id: "b1", down: true });
    input(() => web.pointerButton(1, false), { id: "b1", down: false });
    canvas.focus();
  });
  ui.skip.addEventListener("click", () => {
    skipping = !skipping;
    web.key(KEY_CTRL, skipping);
    ui.skip.classList.toggle("active", skipping);
    canvas.focus();
  });
  requestAnimationFrame(frame);
}
