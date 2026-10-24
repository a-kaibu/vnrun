// SiglusEngine, RealLive, AVG32 and UK2 through siglus_rs (third_party/siglus_rs), built by
// engines/siglus_rs/build.sh into public/engines/siglus_rs/.
import { importEngine, type EngineContext, type Ui } from "../engine";
import type { GameFS } from "../gamefs";
import { Presenter } from "../present";

// The part of the wasm-bindgen module (crates/game_launcher) used here.
interface WebGame {
  step(dt: number): boolean;
  width(): number;
  height(): number;
  frame(): Uint8Array;
  cursorVisible(): boolean;
  pointerMove(x: number, y: number): void;
  pointerButton(id: number, down: boolean): void;
  wheel(up: boolean): void;
  key(code: number, down: boolean): void;
  text(text: string): void;
  free(): void;
}
interface GameLauncher {
  default(): Promise<unknown>;
  start_siglus_from_directory(canvasId: string, files: string): Promise<void>;
  launcherAddFont(font: Uint8Array): void;
  WebGame: new (root: string, engine: string, nls?: string) => WebGame;
}

// siglus_rs reads files through these globals (crates/game_fs, crates/siglus_scene_vm/src/wasm_vfs.rs).
function bind(fs: GameFS): void {
  Object.assign(globalThis, {
    siglusFileExists: (path: string) => fs.exists(path),
    siglusReadFile: (path: string) => fs.read(path),
    siglusListDir: (path: string) => fs.list(path),
    siglusKnownFileCount: () => fs.fileCount(),
    gameFsWrite: (path: string, bytes: Uint8Array) => fs.write(path, bytes),
    gameFsRemove: (path: string) => fs.remove(path),
  });
}

export async function start({ fs, manifest, ui, fonts }: EngineContext): Promise<void> {
  bind(fs);
  const launcher = await importEngine<GameLauncher>("siglus_rs", "game_launcher.js");
  await launcher.default();
  for (const font of fonts) launcher.launcherAddFont(font);
  ui.hide();
  ui.canvas.focus();
  if (manifest.engine === "siglus") {
    if (!fs.exists("Scene.pck")) throw new Error("Scene.pck がありません");
    // The Siglus host runs its own event loop on the canvas.
    await launcher.start_siglus_from_directory(ui.canvas.id, JSON.stringify(fs.fileList()));
    return;
  }
  const game = new launcher.WebGame("", manifest.engine, manifest.nls || undefined);
  framebuffer(game, ui, await Presenter.create(ui.canvas));
}

// Key codes of the siglus_rs web API (crates/game_launcher/src/ffi.rs).
const KEY_CODES: Record<string, number> = {
  Enter: 1,
  NumpadEnter: 1,
  Escape: 2,
  Space: 3,
  ArrowUp: 4,
  ArrowDown: 5,
  ArrowLeft: 6,
  ArrowRight: 7,
  PageUp: 8,
  PageDown: 9,
  Home: 10,
  End: 11,
  Backspace: 12,
  Tab: 13,
  ControlLeft: 14,
  ControlRight: 14,
  ShiftLeft: 15,
  ShiftRight: 15,
};
const KEY_CTRL = 14;
const KEY_SHIFT = 15;

function keyCode(event: KeyboardEvent): number {
  if (event.code in KEY_CODES) return KEY_CODES[event.code];
  const f = /^F(\d{1,2})$/.exec(event.code);
  if (f && Number(f[1]) >= 1 && Number(f[1]) <= 12) return 0x100 + Number(f[1]);
  if (event.key && Array.from(event.key).length === 1) return 0x10000 + event.key.codePointAt(0)!;
  return 0;
}

interface Queued {
  apply: () => void;
  edge?: { id: string; down: boolean };
}

// RealLive / AVG32 / UK2 draw into an RGBA frame that the presenter scales onto the canvas.
function framebuffer(web: WebGame, ui: Ui, presenter: Presenter): void {
  const { canvas } = ui;
  let skipping = false;
  let last = 0;
  // Input waits for the next frame, and a release never lands in the same frame as its
  // press: the engine reads button and key state once per frame, so a quick click or key
  // tap would otherwise come and go between two reads.
  const queue: Queued[] = [];
  const input = (apply: () => void, edge?: Queued["edge"]) => queue.push({ apply, edge });
  function applyInput(): void {
    const pressed = new Set<string>();
    while (queue.length > 0) {
      const { apply, edge } = queue[0];
      if (edge && !edge.down && pressed.has(edge.id)) break;
      queue.shift();
      apply();
      if (edge && edge.down) pressed.add(edge.id);
    }
  }
  ui.menu.hidden = false;
  ui.skip.hidden = false;

  function frame(now: number): void {
    const dt = last ? Math.max(0, Math.min(100, Math.round(now - last))) : 0;
    last = now;
    applyInput();
    if (!web.step(dt)) {
      web.free();
      void ui.exit();
      return;
    }
    const w = web.width();
    const h = web.height();
    if (w > 0 && h > 0) {
      presenter.show(web.frame(), w, h);
      canvas.style.cursor = web.cursorVisible() ? "default" : "none";
    }
    requestAnimationFrame(frame);
  }

  const button = (down: boolean) => (e: PointerEvent) => {
    if (down) canvas.focus();
    const [x, y] = presenter.toFrame(e);
    input(() => web.pointerMove(x, y));
    if (e.button === 0 || e.button === 2) {
      const id = e.button === 2 ? 1 : 0;
      input(() => web.pointerButton(id, down), { id: `b${id}`, down });
    }
    e.preventDefault();
  };
  canvas.addEventListener("pointermove", (e) => {
    const [x, y] = presenter.toFrame(e);
    input(() => web.pointerMove(x, y));
  });
  canvas.addEventListener("pointerdown", button(true));
  canvas.addEventListener("pointerup", button(false));
  canvas.addEventListener("contextmenu", (e) => e.preventDefault());
  canvas.addEventListener(
    "wheel",
    (e) => {
      web.wheel(e.deltaY < 0);
      e.preventDefault();
    },
    { passive: false },
  );
  window.addEventListener("keydown", (e) => {
    if (e.metaKey) return;
    const code = keyCode(e);
    if (code) input(() => web.key(code, true), { id: `k${code}`, down: true });
    if (!e.ctrlKey && !e.altKey && e.key && Array.from(e.key).length === 1) {
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
