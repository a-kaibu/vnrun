// KiriKiri (KAG) through KiriKiri SDL2 (third_party/krkrsdl2), built by engines/krkr/build.sh
// into public/engines/krkr/. The engine is an Emscripten module with game files and saves from
// ../emfs. It hands each frame of its window surface to vnrunPresent (patches/krkrsdl2), which
// the presenter shows; pointer input goes back in through vnrun_mouse, and SDL reads the keyboard
// from the window itself.
import { engineUrl, importEngine, type EngineContext } from "../engine";
import { mountGame, Saves, type EmFS } from "../emfs";
import type { Manifest } from "../gamefs";
import { Presenter } from "../present";
import { Pads } from "./krkr-pad";

interface KrkrModule {
  FS: EmFS & {
    syncfs(populate: boolean, done: (error: unknown) => void): void;
    symlink(target: string, path: string): void;
  };
  callMain(args: string[]): void;
  _vnrun_mouse(x: number, y: number, button: number, down: number): void;
  _vnrun_wheel(dy: number): void;
  _vnrun_save_state(): void;
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
  "Pad.tjs",
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
    "layerExRasterMP",
    "layerExImage",
    "extrans",
    "saveStruct",
    "psbFile",
    "textRender",
    "alphaMovie",
    "proxyFs",
    "motionPlayer",
  ].map((name) => `plugins/${name}.tjs`),
];
const STARTUP = "/vnrun/startup.tjs";
// ASCII only: the engine reads BOM-less scripts as Shift_JIS.
const STARTUP_SCRIPT = `global.KirikiriPolyfillRoot = "${POLYFILL}/";
Scripts.execStorage(global.KirikiriPolyfillRoot + "PolyfillInitialize.tjs");
Scripts.execStorage(global.KirikiriPolyfillRoot + "plugins/Plugins.tjs");
Scripts.execStorage(global.KirikiriPolyfillRoot + "Compat.tjs");
Scripts.execStorage(global.KirikiriPolyfillRoot + "Pad.tjs");
Scripts.execStorage(global.KirikiriPolyfillRoot + "SelfRestart.tjs");
Scripts.execStorage(global.KirikiriPolyfillRoot + "ModalDialog.tjs");
Scripts.execStorage("startup.tjs");
`;

// The project to start: data.xp3 (most releases), else a data folder, else the game folder.
function project(fs: EngineContext["fs"], manifest: Manifest, exe: GameExe): string {
  const files = (manifest.files || []).map((f) => f.p);
  const xp3 = files.find((p) => p.toLowerCase() === "data.xp3");
  if (xp3) return `/game/${xp3}`;
  const bin = files.find((p) => p.toLowerCase() === "data.bin");
  if (bin && isXp3(fs.readRange(bin, 0, 11))) return `/game/${bin}`;
  if (exe.archive) return `/game/${exe.archive}`;
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

interface GameExe {
  /** The KiriKiri 2 minor version (31 for FileVersion "2.31.2013.411"). */
  krkr2Minor?: number;
  /** Options embedded in it ("-key=value"), which KiriKiri reads as defaults for its command line. */
  options: string[];
  archive?: string;
}

// The game's KiriKiri exe: the first exe with embedded options or a KiriKiri 2 version. Other exes
// beside it (engine settings, file checkers) have neither.
async function gameExe(fs: EngineContext["fs"], manifest: Manifest): Promise<GameExe> {
  const exes = (manifest.files || [])
    .map((f) => f.p)
    .filter((p) => !p.includes("/") && p.toLowerCase().endsWith(".exe"));
  for (const exe of exes) {
    const res = await fetch(fs.fileUrl(exe));
    if (!res.ok) continue;
    const bytes = new Uint8Array(await res.arrayBuffer());
    // Version resources are UTF-16 and 32-bit aligned.
    const text = new TextDecoder("utf-16le").decode(bytes.subarray(0, bytes.length & ~1));
    const m = text.match(/FileVersion\0+2\.(\d+)\./);
    const options = embeddedOptions(bytes);
    const archive = hasEmbeddedXp3(bytes) ? exe : undefined;
    if (m || options.length || archive) return { krkr2Minor: m ? Number(m[1]) : undefined, options, archive };
  }
  return { options: [] };
}

const XP3_MAGIC = [0x58, 0x50, 0x33, 0x0d, 0x0a, 0x20, 0x0a, 0x1a, 0x8b, 0x67, 0x01];
function isXp3(bytes: Uint8Array, at = 0): boolean {
  return XP3_MAGIC.every((b, i) => bytes[at + i] === b);
}
function hasEmbeddedXp3(bytes: Uint8Array): boolean {
  // KiriKiri's embedded XP3 archives begin on a 16-byte boundary.
  for (let at = 16; at + 19 <= bytes.length; at += 16)
    if (bytes[at] === XP3_MAGIC[0] && isXp3(bytes, at)) {
      const view = new DataView(bytes.buffer, bytes.byteOffset + at + 11, 8);
      const index = Number(view.getBigUint64(0, true));
      if (index >= 19 && index < bytes.length - at) return true;
    }
  return false;
}

// "XOPT_EMBED_AREA_", a 32-bit size, then lines of key="value" (values TJS-escaped, "\x7b"), as
// KiriKiri's engine settings tool writes them into the exe.
function embeddedOptions(bytes: Uint8Array): string[] {
  const mark = new TextEncoder().encode("XOPT_EMBED_AREA_");
  let at = -1;
  for (let i = bytes.indexOf(mark[0]); i >= 0 && at < 0; i = bytes.indexOf(mark[0], i + 1))
    if (mark.every((c, j) => bytes[i + j] === c)) at = i + mark.length;
  if (at < 0) return [];
  const size = new DataView(bytes.buffer, bytes.byteOffset + at, 4).getUint32(0, true);
  const area = bytes.subarray(at + 4, at + 4 + size);
  const end = area.indexOf(0);
  const text = new TextDecoder("shift_jis").decode(end < 0 ? area : area.subarray(0, end));
  const options = [];
  for (const line of text.split(/\r?\n/)) {
    const kv = line.match(/^([\w-]+)="(.*)"$/);
    if (!kv) continue;
    const value = kv[2].replace(/\\(x[0-9a-fA-F]{2}|.)/g, (_, e: string) =>
      e[0] === "x" ? String.fromCharCode(parseInt(e.slice(1), 16)) : e,
    );
    options.push(`-${kv[1]}=${value}`);
  }
  return options;
}

export async function start({ fs, manifest, ui, fonts }: EngineContext): Promise<void> {
  const presenter = await Presenter.create(ui.canvas);
  const create = (await importEngine<{ default: CreateKrkr }>("krkr", "krkr.js")).default;
  const pads = new Pads(fs, ui.canvas);
  const Module = await create({
    // SDL needs a canvas for its window, but frames go to the presenter instead.
    canvas: document.createElement("canvas"),
    print: (text: string) => console.info(`krkr: ${text}`),
    printErr: (text: string) => console.warn(`krkr: ${text}`),
    vnrunPad: (id: number, action: string, value: string, title: string, filename: string) =>
      pads.call(id, action, value, title, filename),
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
  // What KAG keeps until a normal quit (system variables) is saved when the tab is hidden and
  // before the player's exit sends the saves (polyfill/Compat.tjs vnrunSaveState).
  const saveState = () => {
    try {
      Module._vnrun_save_state();
      saves.scan();
    } catch (error) {
      console.error("krkr: saving state", error);
    }
  };
  window.addEventListener("vnrun-before-exit", saveState);
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "hidden") saveState();
  });

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

  const exe = await gameExe(fs, manifest);
  // KiriKiri 2.26 (grand_cru 2010) let a class var keep a native property of the same name and
  // set it, where later TJS hides the property (patches/krkrz)
  const oldClassVars = exe.krkr2Minor !== undefined && exe.krkr2Minor < 28;
  ui.hide();
  canvas.focus();
  // Scripts without a BOM are Shift_JIS in KiriKiri 2/Z games; KiriKiri SDL2 reads them as UTF-8
  // unless told otherwise.
  Module.callMain([
    project(fs, manifest, exe),
    "-readencoding=Shift_JIS",
    // No debug console in the page (KiriKiri Z's k2compat builds Windows dialogs for it).
    "-debugwin=no",
    `-startup=${STARTUP}`,
    ...(oldClassVars ? ["-vnrunk2classvars=yes"] : []),
    // Earlier options win, so the exe's own come last.
    ...exe.options,
  ]);
}
