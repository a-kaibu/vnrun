// KiriKiri (KAG) through KiriKiri SDL2 (third_party/krkrsdl2), built by engines/krkr/build.sh
// into public/engines/krkr/. The engine is an Emscripten module with game files and saves from
// ../emfs. It hands each frame of its window surface to vnrunPresent (patches/krkrsdl2), which
// the presenter shows; pointer input goes back in through vnrun_mouse, and SDL reads the keyboard
// from the window itself.
import { importEngine, type EngineContext } from "../engine";
import { mountGame, Saves, type EmFS } from "../emfs";
import type { Manifest } from "../gamefs";
import { Presenter } from "../present";

interface KrkrModule {
  FS: EmFS & { syncfs(populate: boolean, done: (error: unknown) => void): void };
  callMain(args: string[]): void;
  _vnrun_mouse(x: number, y: number, button: number, down: number): void;
  _vnrun_wheel(dy: number): void;
  _emscripten_syncfs_is_finished(): void;
}
type CreateKrkr = (options: object) => Promise<KrkrModule>;

const SAVES = "krkr-save";
// Where KiriKiri SDL2 keeps System.dataPath (savedata) in the browser.
const DATA_PATH = "/libsdl/krkrsdl2";
// SDL_BUTTON_* for the DOM's MouseEvent.button.
const BUTTONS = [1, 2, 3];

// The project to start: data.xp3 (most releases), else a data folder, else the game folder.
function project(manifest: Manifest): string {
  const files = (manifest.files || []).map((f) => f.p);
  const xp3 = files.find((p) => p.toLowerCase() === "data.xp3");
  if (xp3) return `/game/${xp3}`;
  const data = files.find((p) => p.toLowerCase().startsWith("data/"));
  if (data) return `/game/${data.slice(0, data.indexOf("/"))}/`;
  return "/game/";
}

export async function start({ fs, manifest, ui }: EngineContext): Promise<void> {
  const presenter = await Presenter.create(ui.canvas);
  const create = (await importEngine<{ default: CreateKrkr }>("krkr", "krkr.js")).default;
  const Module = await create({
    // SDL needs a canvas for its window, but frames go to the presenter instead.
    canvas: document.createElement("canvas"),
    print: (text: string) => console.info(`krkr: ${text}`),
    printErr: (text: string) => console.warn(`krkr: ${text}`),
    vnrunPresent: (pixels: Uint8Array, pitch: number, width: number, height: number) =>
      presenter.show(pixels, width, height, "rgba8unorm", pitch),
  });
  // KiriKiri matches names case-insensitively itself (OPTION_ENABLE_CASEINSENSITIVITY).
  mountGame(Module.FS, fs, manifest);
  const saves = new Saves(Module.FS, fs, SAVES, DATA_PATH);
  // After writing save data the engine runs `FS.syncfs(false, ...)`
  // (src/core/environ/sdl2/Application.cpp): that is when saves go to the server.
  Module.FS.syncfs = (_populate, done) => {
    saves.scan();
    done(null);
  };

  const { canvas } = ui;
  const mouse = (e: PointerEvent, button: number, down: boolean) => {
    const [x, y] = presenter.toFrame(e);
    Module._vnrun_mouse(Math.round(x), Math.round(y), button, down ? 1 : 0);
  };
  canvas.addEventListener("pointermove", (e) => mouse(e, 0, false));
  canvas.addEventListener("pointerdown", (e) => {
    canvas.focus();
    mouse(e, BUTTONS[e.button] ?? 0, true);
    e.preventDefault();
  });
  canvas.addEventListener("pointerup", (e) => mouse(e, BUTTONS[e.button] ?? 0, false));
  canvas.addEventListener("contextmenu", (e) => e.preventDefault());
  canvas.addEventListener(
    "wheel",
    (e) => {
      Module._vnrun_wheel(e.deltaY < 0 ? 1 : -1);
      e.preventDefault();
    },
    { passive: false },
  );

  ui.hide();
  canvas.focus();
  // Scripts without a BOM are Shift_JIS in KiriKiri 2/Z games; KiriKiri SDL2 reads them as UTF-8
  // unless told otherwise.
  Module.callMain([project(manifest), "-readencoding=Shift_JIS"]);
}
