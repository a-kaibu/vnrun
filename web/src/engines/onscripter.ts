// NScripter through ONScripter-Yuri (third_party/onscripter-yuri), built by
// engines/onscripter/build.sh into public/engines/onscripter/. The engine is an Emscripten
// module running its SDL loop under Asyncify, with game files and saves from ../emfs. It hands
// each frame to vnrunPresent (patches/onscripter-yuri), which the presenter shows; pointer input
// goes back in through vnrun_mouse, and SDL reads the keyboard from the window itself.
import { importEngine, type EngineContext } from "../engine";
import { mountGame, Saves, type EmFS } from "../emfs";
import { Presenter } from "../present";

interface OnscripterModule {
  FS: EmFS;
  callMain(args: string[]): void;
  _vnrun_mouse(x: number, y: number, button: number, down: number): void;
  _vnrun_wheel(dy: number): void;
}
type CreateOnscripter = (options: object) => Promise<OnscripterModule>;

const SAVES = "onscripter-save";
// SDL_BUTTON_* for the DOM's MouseEvent.button.
const BUTTONS = [1, 2, 3];

export async function start({ fs, manifest, ui, fonts }: EngineContext): Promise<void> {
  if (fonts.length === 0) throw new Error("NScripter の作品には文字のフォントが要ります (fonts/fonts.json)");
  const presenter = await Presenter.create(ui.canvas);
  const create = (await importEngine<{ default: CreateOnscripter }>("onscripter", "onscripter.js")).default;
  let saves: Saves | null = null;
  // Globals the upstream web build reads (src/onsyuri/onscripter_main.cpp, ONScripter_file.cpp).
  // Files are already local (lazy files), so its own fetching stays off; video is not supported.
  Object.assign(globalThis, {
    g_onsyuri_module: null,
    g_onsyuri_index: null,
    g_onsyuri_filemap: null,
    fetch_file: null,
    playVideo: () => console.info("onscripter: skipping video"),
    flush_save: () => saves?.scan(),
  });
  const Module = await create({
    // SDL needs a canvas for its window, but frames go to the presenter instead.
    canvas: document.createElement("canvas"),
    print: (text: string) => console.info(`onscripter: ${text}`),
    printErr: (text: string) => console.warn(`onscripter: ${text}`),
    vnrunPresent: (pixels: Uint8Array, pitch: number, width: number, height: number) =>
      presenter.show(pixels, width, height, "bgra8unorm", pitch),
    onExit: () => {
      saves?.scan();
      void ui.exit();
    },
  });
  // Its video stub reads the module from here (and waits while `wait_video` is set).
  Object.assign(globalThis, { g_onsyuri_module: Module });
  mountGame(Module.FS, fs, manifest, true);
  saves = new Saves(Module.FS, fs, SAVES);
  Module.FS.mkdirTree("/fonts");
  Module.FS.writeFile("/fonts/default.ttf", fonts[0]);
  // Global data and settings are written outside of saving a slot too.
  setInterval(() => saves?.scan(), 2000);

  const { canvas } = ui;
  const mouse = (e: PointerEvent | MouseEvent, button: number, down: boolean) => {
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
  // Japanese releases are Shift_JIS; manifest.nls can say gbk or utf8.
  const encoding = manifest.nls === "gbk" || manifest.nls === "utf8" ? manifest.nls : "sjis";
  Module.callMain([
    "--root",
    "/game",
    "--save-dir",
    "/saves",
    "--font",
    "/fonts/default.ttf",
    `--enc:${encoding}`,
  ]);
}
