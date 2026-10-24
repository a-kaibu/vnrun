// Softpal (PAL) through sena-rs (third_party/sena-rs), built by engines/sena/build.sh into
// public/engines/sena/. The engine draws its frame into the canvas at the game's own size and
// reads the pointer against the canvas's on-screen box, so the canvas is kept at the game's
// aspect ratio here instead of being stretched over the page.
import { fitCanvas, importEngine, type EngineContext } from "../engine";
import type { GameFS } from "../gamefs";

interface PalVm {
  default(): Promise<unknown>;
  start_sena_from_directory(canvasId: string, root: string, nls: string): void;
}

// sena-rs reads files through these globals (crates/pal-asset/src/vfs.rs).
function bind(fs: GameFS): void {
  Object.assign(globalThis, {
    senaFileExists: (path: string) => fs.exists(path),
    senaFileSize: (path: string) => fs.size(path),
    senaReadFile: (path: string) => fs.read(path),
    senaReadRange: (path: string, offset: number | bigint, length: number | bigint) =>
      fs.readRange(path, Number(offset), Number(length)),
    senaListDir: (path: string) => fs.list(path),
    senaKnownFileCount: () => fs.fileCount(),
  });
}

export async function start({ fs, manifest, ui }: EngineContext): Promise<void> {
  bind(fs);
  const vm = await importEngine<PalVm>("sena", "pal_vm.js");
  await vm.default();
  const { canvas } = ui;
  // The engine sets the canvas size to the game's size on its first frame.
  new MutationObserver(() => fitCanvas(canvas)).observe(canvas, {
    attributes: true,
    attributeFilter: ["width", "height"],
  });
  window.addEventListener("resize", () => fitCanvas(canvas));
  ui.hide();
  canvas.focus();
  vm.start_sena_from_directory(canvas.id, "", manifest.nls || "sjis");
}
