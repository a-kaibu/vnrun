// FVP through rfvp (third_party/rfvp), built by engines/rfvp/build.sh into ./rfvp/. The engine
// runs its own winit loop on the canvas (WebGL2), reads game files by id and range, and keeps
// saves through the rfvpSave* globals (patches/rfvp/0001), which put them in the GameFS.
import init, { start_rfvp_from_directory } from "./rfvp/rfvp.js";

// Saves get their own folder so they never shadow a game file of the same name.
const SAVES = "rfvp-save";

function bind(fs, paths) {
  globalThis.rfvpReadFileRange = (id, offset, length) => fs.readRange(paths[id], Number(offset), Number(length));
  const save = (path) => `${SAVES}/${path}`;
  globalThis.rfvpSaveRead = (path) => (fs.exists(save(path)) ? fs.read(save(path)) : undefined);
  globalThis.rfvpSaveWrite = (path, bytes) => fs.write(save(path), bytes);
  globalThis.rfvpSaveRemove = (path) => {
    if (!fs.exists(save(path))) return false;
    fs.remove(save(path));
    return true;
  };
  globalThis.rfvpSaveList = (dir) => fs.list(dir ? save(dir) : SAVES);
}

export async function start({ fs, manifest, ui }) {
  const files = (manifest.files || []).map((f, id) => ({ path: f.p, id, size: f.s || 0 }));
  bind(fs, files.map((f) => f.path));
  await init();
  ui.hide();
  ui.canvas.focus();
  await start_rfvp_from_directory(ui.canvas.id, manifest.nls || "sjis", JSON.stringify(files));
}
