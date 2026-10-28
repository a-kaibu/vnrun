// AliceSoft System4 through xsystem4 (third_party/xsystem4, GPL-2.0-or-later), built by
// engines/system4/build.sh into public/engines/system4/. The engine is an Emscripten module that
// runs its VM under Asyncify and draws with WebGL2 on a canvas of its own that is never shown;
// each frame is read back and handed to vnrunPresent (patches/xsystem4), which the presenter
// shows with WebGPU. Pointer input goes in through vnrun_mouse; SDL reads the keyboard from the
// window itself. Game files and saves come from ../emfs.
import { engineUrl, importEngine, type EngineContext } from "../engine";
import { mountGame, Saves, type EmFS } from "../emfs";
import { Presenter } from "../present";

interface System4Module {
  FS: EmFS;
  callMain(args: string[]): void;
  _vnrun_mouse(x: number, y: number, button: number, down: number): void;
  _vnrun_wheel(dy: number): void;
}
type CreateSystem4 = (options: object) => Promise<System4Module>;

const SAVES = "system4-save";
// SDL_BUTTON_* for the DOM's MouseEvent.button.
const BUTTONS = [1, 2, 3];

export async function start({ fs, manifest, ui, fonts }: EngineContext): Promise<void> {
  const presenter = await Presenter.create(ui.canvas);
  const create = (await importEngine<{ default: CreateSystem4 }>("system4", "system4.js")).default;
  let saves: Saves | null = null;
  const Module = await create({
    // SDL makes its window and the WebGL2 context here; the page never shows it.
    canvas: document.createElement("canvas"),
    print: (text: string) => console.info(`system4: ${text}`),
    printErr: (text: string) => console.warn(`system4: ${text}`),
    vnrunPresent: (pixels: Uint8Array, pitch: number, width: number, height: number) =>
      presenter.show(pixels, width, height, "rgba8unorm", pitch),
    onExit: () => {
      saves?.scan();
      void ui.exit();
    },
  });
  mountGame(Module.FS, fs, manifest);
  saves = new Saves(Module.FS, fs, SAVES);
  setInterval(() => saves?.scan(), 2000);

  // xsystem4 always loads a gothic and a mincho TrueType font, for text the game's own .fnl
  // font library does not cover: the page's fonts, else the VL Gothic that build.sh ships.
  let gothic = fonts[0];
  if (!gothic) {
    const res = await fetch(engineUrl("system4", "fonts/VL-Gothic-Regular.ttf"));
    if (!res.ok) throw new Error(`system4: no font (${res.status})`);
    gothic = new Uint8Array(await res.arrayBuffer());
  }
  Module.FS.mkdirTree("/fonts");
  Module.FS.writeFile("/fonts/gothic.ttf", gothic);
  Module.FS.writeFile("/fonts/mincho.ttf", fonts[1] ?? gothic);
  const args = [
    "--save-folder",
    "/saves",
    "--font-gothic",
    "/fonts/gothic.ttf",
    "--font-mincho",
    "/fonts/mincho.ttf",
  ];

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
  Module.callMain([...args, "/game"]);
}
