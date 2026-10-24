// Softpal (PAL) through sena-rs (third_party/sena-rs), built by engines/sena/build.sh into
// public/engines/sena/. The engine hands each RGBA frame to senaPresent, which the presenter
// shows, and asks senaToFrame for pointer positions in frame pixels (patches/sena-rs/0002).
import { importEngine, type EngineContext } from "../engine";
import type { GameFS } from "../gamefs";
import { Presenter } from "../present";

interface PalVm {
  default(): Promise<unknown>;
  start_sena_from_directory(canvasId: string, root: string, nls: string): void;
}

// sena-rs reads files through these globals (crates/pal-asset/src/vfs.rs).
function bind(fs: GameFS, presenter: Presenter): void {
  Object.assign(globalThis, {
    senaPresent: (rgba: Uint8Array, width: number, height: number) => presenter.show(rgba, width, height),
    senaToFrame: (clientX: number, clientY: number) =>
      Float64Array.from(presenter.toFrame({ clientX, clientY })),
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
  bind(fs, await Presenter.create(ui.canvas));
  const vm = await importEngine<PalVm>("sena", "pal_vm.js");
  await vm.default();
  ui.hide();
  ui.canvas.focus();
  vm.start_sena_from_directory(ui.canvas.id, "", manifest.nls || "sjis");
}
