// FVP through rfvp (third_party/rfvp), built by engines/rfvp/build.sh into public/engines/rfvp/.
// The engine runs its own winit loop on the canvas, reads game files by id and range, and
// keeps saves through the rfvpSave* globals (patches/rfvp/0001), which put them in the GameFS.
import { importEngine, type EngineContext } from "../engine";
import type { GameFS } from "../gamefs";

interface Rfvp {
  default(): Promise<unknown>;
  start_rfvp_from_directory(canvasId: string, nls: string, files: string): Promise<void>;
}

// Saves get their own folder so they never shadow a game file of the same name.
const SAVES = "rfvp-save";

function bind(fs: GameFS, paths: string[]): void {
  const save = (path: string) => `${SAVES}/${path}`;
  Object.assign(globalThis, {
    rfvpReadFileRange: (id: number, offset: number | bigint, length: number | bigint) =>
      fs.readRange(paths[id], Number(offset), Number(length)),
    rfvpSaveRead: (path: string) => (fs.exists(save(path)) ? fs.read(save(path)) : undefined),
    rfvpSaveWrite: (path: string, bytes: Uint8Array) => fs.write(save(path), bytes),
    rfvpSaveRemove: (path: string) => {
      if (!fs.exists(save(path))) return false;
      fs.remove(save(path));
      return true;
    },
    rfvpSaveList: (dir: string) => fs.list(dir ? save(dir) : SAVES),
  });
}

export async function start({ fs, manifest, ui }: EngineContext): Promise<void> {
  const files = (manifest.files || []).map((f, id) => ({ path: f.p, id, size: f.s || 0 }));
  bind(
    fs,
    files.map((f) => f.path),
  );
  const rfvp = await importEngine<Rfvp>("rfvp", "rfvp.js");
  await rfvp.default();
  ui.hide();
  ui.canvas.focus();
  await rfvp.start_rfvp_from_directory(ui.canvas.id, manifest.nls || "sjis", JSON.stringify(files));
}
