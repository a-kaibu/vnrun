// KiriKiri (KAG) through KiriKiri SDL2 (third_party/krkrsdl2), built by engines/krkr/build.sh
// into public/engines/krkr/. The engine is an Emscripten module with game files and saves from
// ../emfs. It hands each frame of its window surface to vnrunPresent (patches/krkrsdl2), which
// the presenter shows; pointer input goes back in through vnrun_mouse, and SDL reads the keyboard
// from the window itself.
import { engineUrl, importEngine, type EngineContext } from "../engine";
import { mountGame, Saves, type EmFS } from "../emfs";
import type { Manifest } from "../gamefs";
import { Presenter } from "../present";

interface KrkrModule {
  FS: EmFS & {
    syncfs(populate: boolean, done: (error: unknown) => void): void;
    symlink(target: string, path: string): void;
  };
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
const POLYFILL = "/vnrun/polyfill";
const POLYFILL_FILES = [
  "PolyfillInitialize.tjs",
  "MenuItem_stub.tjs",
  "KAGParser.tjs",
  "SelfRestart.tjs",
  "ModalDialog.tjs",
  "Compat.tjs",
  "compat/k2compat.tjs",
  ...[
    "Plugins",
    "csvParser",
    "fstat",
    "windowEx",
    "layerExDraw",
    "scriptsEx",
    "KAGParserEx",
    "addFont",
    "win32dialog",
    "sigcheck",
    "getSample",
    "gamepad",
    "layerExAlpha",
    "extrans",
    "saveStruct",
    "psbFile",
    "textRender",
    "alphaMovie",
  ].map((name) => `plugins/${name}.tjs`),
];
const STARTUP = "/vnrun/startup.tjs";
// ASCII only: the engine reads BOM-less scripts as Shift_JIS.
const STARTUP_SCRIPT = `global.KirikiriPolyfillRoot = "${POLYFILL}/";
Scripts.execStorage(global.KirikiriPolyfillRoot + "PolyfillInitialize.tjs");
Scripts.execStorage(global.KirikiriPolyfillRoot + "plugins/Plugins.tjs");
Scripts.execStorage(global.KirikiriPolyfillRoot + "Compat.tjs");
Scripts.execStorage(global.KirikiriPolyfillRoot + "SelfRestart.tjs");
Scripts.execStorage(global.KirikiriPolyfillRoot + "ModalDialog.tjs");
Scripts.execStorage("startup.tjs");
`;

// The project to start: data.xp3 (most releases), else a data folder, else the game folder.
function project(manifest: Manifest): string {
  const files = (manifest.files || []).map((f) => f.p);
  const xp3 = files.find((p) => p.toLowerCase() === "data.xp3");
  if (xp3) return `/game/${xp3}`;
  const data = files.find((p) => p.toLowerCase().startsWith("data/"));
  if (data) return `/game/${data.slice(0, data.indexOf("/"))}/`;
  return "/game/";
}

// The settings of the game's exe in the game folder: the .cf named like an exe, else any .cf.
function configFile(manifest: Manifest): string | undefined {
  const files = (manifest.files || []).map((f) => f.p).filter((p) => !p.includes("/"));
  const cfs = files.filter((p) => p.toLowerCase().endsWith(".cf"));
  const exes = new Set(
    files.filter((p) => p.toLowerCase().endsWith(".exe")).map((p) => p.slice(0, -4).toLowerCase()),
  );
  return cfs.find((p) => exes.has(p.slice(0, -3).toLowerCase())) ?? cfs[0];
}

// The version of the game's exe (its FileVersion resource, "2.26.0.915"), if it has one.
async function exeVersion(fs: EngineContext["fs"], manifest: Manifest): Promise<number[] | undefined> {
  const exes = (manifest.files || [])
    .map((f) => f.p)
    .filter((p) => !p.includes("/") && p.toLowerCase().endsWith(".exe"));
  for (const exe of exes) {
    const res = await fetch(fs.fileUrl(exe));
    if (!res.ok) continue;
    const bytes = new Uint8Array(await res.arrayBuffer());
    // Version resources are UTF-16 and 32-bit aligned.
    const text = new TextDecoder("utf-16le").decode(bytes.subarray(0, bytes.length & ~1));
    const m = text.match(/FileVersion\0+(\d+)\.(\d+)\./);
    if (m) return [Number(m[1]), Number(m[2])];
  }
  return undefined;
}

export async function start({ fs, manifest, ui, fonts }: EngineContext): Promise<void> {
  const presenter = await Presenter.create(ui.canvas);
  const create = (await importEngine<{ default: CreateKrkr }>("krkr", "krkr.js")).default;
  const Module = await create({
    // SDL needs a canvas for its window, but frames go to the presenter instead.
    canvas: document.createElement("canvas"),
    print: (text: string) => console.info(`krkr: ${text}`),
    printErr: (text: string) => console.warn(`krkr: ${text}`),
    vnrunPresent: (pixels: Uint8Array, pitch: number, width: number, height: number) =>
      presenter.show(pixels, width, height, "bgra8unorm", pitch),
  });
  // KiriKiri matches names case-insensitively itself (OPTION_ENABLE_CASEINSENSITIVITY).
  mountGame(Module.FS, fs, manifest);
  const saves = new Saves(Module.FS, fs, SAVES, DATA_PATH);
  // The exe is in the game folder (patches/krkrsdl2), and KAG writes to savedata/ beside it:
  // that goes to the data path, which is kept as saves.
  Module.FS.mkdirTree(`${DATA_PATH}/savedata`);
  if (!Module.FS.analyzePath("/game/savedata").exists)
    Module.FS.symlink(`${DATA_PATH}/savedata`, "/game/savedata");
  // Many startup.tjs read the exe's settings, <exe name>.cf, and quit without it.
  const cf = configFile(manifest);
  if (cf && !Module.FS.analyzePath("/game/game.cf").exists) Module.FS.symlink(`/game/${cf}`, "/game/game.cf");
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

  // KiriKiri SDL2 has no menus, plugins or default font. Its own stand-ins and ours for common
  // plugins (engines/krkr/polyfill, with the player's font) run before the game's startup.tjs.
  Module.FS.mkdirTree(`${POLYFILL}/plugins`);
  Module.FS.mkdirTree(`${POLYFILL}/compat`);
  for (const name of POLYFILL_FILES) {
    const res = await fetch(engineUrl("krkr", `polyfill/${name}`));
    if (!res.ok) throw new Error(`polyfill/${name}: HTTP ${res.status}`);
    Module.FS.writeFile(`${POLYFILL}/${name}`, new Uint8Array(await res.arrayBuffer()));
  }
  if (fonts[0]) Module.FS.writeFile(`${POLYFILL}/font.ttf`, fonts[0]);
  Module.FS.writeFile(STARTUP, new TextEncoder().encode(STARTUP_SCRIPT));

  // KiriKiri 2.26 (grand_cru 2010) let a class var keep a native property of the same name and
  // set it, where later TJS hides the property (patches/krkrz)
  const version = await exeVersion(fs, manifest);
  const oldClassVars = version !== undefined && version[0] === 2 && version[1] < 28;
  ui.hide();
  canvas.focus();
  // Scripts without a BOM are Shift_JIS in KiriKiri 2/Z games; KiriKiri SDL2 reads them as UTF-8
  // unless told otherwise.
  Module.callMain([
    project(manifest),
    "-readencoding=Shift_JIS",
    // No debug console in the page (KiriKiri Z's k2compat builds Windows dialogs for it).
    "-debugwin=no",
    `-startup=${STARTUP}`,
    ...(oldClassVars ? ["-vnrunk2classvars=yes"] : []),
  ]);
}
