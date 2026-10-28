// Games that are web pages already: TyranoScript, RPG Maker MV/MZ and other NW.js games. The
// page runs as it is in an iframe and draws itself; the player only keeps its saves. The
// server starts each page of the game with a call to vnrunFrame (docs/protocol.md), which
// swaps the page's localStorage for one kept in the GameFS, before any script of the game runs.
// IndexedDB is left as it is.
import type { EngineContext } from "../engine";
import { normalizePath, type GameFS, type Manifest } from "../gamefs";

const SAVES = "localStorage";
const STUDIO = "studio";

// The page to open: package.json's main (NW.js), else www/index.html (RPG Maker MV), else
// index.html.
function startPage(fs: GameFS, manifest: Manifest): string {
  if (fs.exists("package.json")) {
    try {
      const main = JSON.parse(new TextDecoder().decode(fs.read("package.json"))).main;
      const page = typeof main === "string" ? main.replace(/^app:\/\/[^/]*\//, "").replace(/^\.\//, "") : "";
      if (/\.html?$/i.test(page) && fs.exists(page)) return page;
    } catch (error) {
      console.warn("package.json:", error);
    }
  }
  const pages = (manifest.files || []).map((f) => f.p);
  return pages.find((p) => p.toLowerCase() === "www/index.html") || "index.html";
}

// A Storage whose items are save files (one per key) in the GameFS.
function storage(fs: GameFS): Storage {
  const path = (key: string) => `${SAVES}/${encodeURIComponent(key)}`;
  const keys = () =>
    Array.from(fs.saves.keys())
      .filter((p) => p.startsWith(`${SAVES}/`))
      .map((p) => decodeURIComponent(p.slice(SAVES.length + 1)));
  const api: Storage = {
    get length() {
      return keys().length;
    },
    key: (i: number) => keys()[i] ?? null,
    getItem: (key: string) => {
      const bytes = fs.saves.get(path(String(key)));
      return bytes ? new TextDecoder().decode(bytes) : null;
    },
    setItem: (key: string, value: string) =>
      fs.write(path(String(key)), new TextEncoder().encode(String(value))),
    removeItem: (key: string) => {
      if (fs.saves.has(path(String(key)))) fs.remove(path(String(key)));
    },
    clear: () => keys().forEach((k) => fs.remove(path(k))),
  };
  // Pages also use storage["key"] and Object.keys(storage).
  return new Proxy(api, {
    get: (t, k) => (typeof k === "symbol" || k in t ? Reflect.get(t, k) : (t.getItem(k) ?? undefined)),
    set: (t, k, v) => (typeof k === "symbol" || k in t ? false : (t.setItem(k, v), true)),
    deleteProperty: (t, k) => (typeof k === "string" && t.removeItem(k), true),
    has: (t, k) => typeof k === "string" && (k in t || t.getItem(k) !== null),
    ownKeys: () => keys(),
    getOwnPropertyDescriptor: (t, k) =>
      typeof k === "string" && t.getItem(k) !== null
        ? { value: t.getItem(k), writable: true, enumerable: true, configurable: true }
        : undefined,
  });
}

// Node's path, on the GameFS's forward-slash paths.
const join = (...parts: string[]) => normalizePath(parts.join("/"));
const nodePath = {
  sep: "/",
  join,
  resolve: join,
  dirname: (p: string) => join(p).split("/").slice(0, -1).join("/") || ".",
  basename: (p: string, ext = "") => {
    const base = join(p).split("/").pop() || "";
    return ext && base.endsWith(ext) ? base.slice(0, -ext.length) : base;
  },
  extname: (p: string) => /\.[^./]*$/.exec(join(p).split("/").pop() || "")?.[0] ?? "",
};

// TyranoScript's Electron build (configSave=file) keeps its saves as files next to the exe,
// through the `studio_api` its preload script exposes (fs-extra and an ipcRenderer that answers
// getAppPath and getProcess). This one keeps them as save files named after the file.
function studioApi(fs: GameFS) {
  const path = (p: string) => `${STUDIO}/${encodeURIComponent(String(p).split(/[\\/]/).pop() || "")}`;
  const process = {
    platform: "win32",
    execPath: "C:\\game\\game.exe",
    env: {},
    __dirname: "C:\\game\\resources\\app",
  };
  return {
    fs: {
      // Folders exist; of the files, only the saves (a .tpatch was applied by vnrun-prep).
      existsSync: (p: string) => (/\.sav$/i.test(p) ? fs.saves.has(path(p)) : !/\.[^./\\]*$/.test(String(p))),
      mkdirSync: () => undefined,
      readFileSync: (p: string) => new TextDecoder().decode(fs.saves.get(path(p)) ?? new Uint8Array()),
      writeFileSync: (p: string, data: string) => fs.write(path(p), new TextEncoder().encode(String(data))),
      unlinkSync: (p: string) => fs.saves.has(path(p)) && fs.remove(path(p)),
    },
    path: nodePath,
    ipcRenderer: {
      sendSync: (channel: string) =>
        channel === "getAppPath" ? process.__dirname : channel === "getProcess" ? process : undefined,
    },
  };
}

// NW.js games that edit the RPG Maker core can call Node's require at the top level (an
// RPG Maker MV game reads its language from lng.txt with fs). This require has the synchronous
// fs calls (reads from the game, writes as saves) and path; other modules throw as in Node.
function nodeRequire(fs: GameFS) {
  const file = (p: string) => normalizePath(p);
  const encoding = (opt: unknown) =>
    typeof opt === "string" ? opt : (opt as { encoding?: string } | undefined)?.encoding;
  const bytes = (data: unknown) =>
    typeof data === "string" ? new TextEncoder().encode(data) : new Uint8Array(data as ArrayBuffer);
  const nodeFs = {
    existsSync: (p: string) => fs.exists(file(p)) || fs.list(file(p)).length > 0,
    readFileSync(p: string, opt?: unknown) {
      const data = fs.read(file(p));
      const text = () => new TextDecoder().decode(data);
      // A Buffer turns into its text, as with unescape(fs.readFileSync(p)).
      return encoding(opt) ? text() : Object.assign(data, { toString: text });
    },
    writeFileSync: (p: string, data: unknown) => fs.write(file(p), bytes(data)),
    unlinkSync: (p: string) => fs.saves.has(fs.resolve(file(p))) && fs.remove(file(p)),
    readdirSync: (p: string) => fs.list(file(p)),
    mkdirSync: () => undefined,
    createWriteStream(p: string) {
      let text = "";
      return {
        write: (s: string) => ((text += s), true),
        end: (s = "") => nodeFs.writeFileSync(p, text + s),
      };
    },
  };
  // fs-extra: TyranoScript checks for an update patch with it at startup.
  const modules: Record<string, unknown> = { fs: nodeFs, "fs-extra": nodeFs, path: nodePath };
  return (name: string) => {
    if (name in modules) return modules[name];
    throw new Error(`Cannot find module '${name}'`);
  };
}

// Plugins that see `require` take NW.js for granted and read process.mainModule.filename for
// the game folder (Text2Frame). This `process` is a function, so the RPG Maker cores' NW.js
// check (typeof process === "object") still fails and they keep their web storage.
function nodeProcess() {
  return Object.assign(function process() {}, {
    mainModule: { filename: "index.html" },
    // TyranoScript keeps configSave=file saves in the exe's folder; with no folder in this
    // path, they land in the game root (as saves).
    execPath: "game.exe",
    platform: "win32",
    env: {},
    argv: [],
    versions: {},
    cwd: () => ".",
    exit: () => undefined,
    on: () => undefined,
  });
}

export async function start({ fs, manifest, ui }: EngineContext): Promise<void> {
  const localStorage = storage(fs);
  const studio = studioApi(fs);
  const require = nodeRequire(fs);
  Object.assign(window, {
    vnrunFrame(win: Window) {
      Object.defineProperty(win, "localStorage", { value: localStorage });
      // RPG Maker MZ refuses to start without IndexedDB and keeps its saves there (localforage,
      // keys named after the game id), so it stays the browser's own.
      Object.assign(win, { studio_api: studio, require, process: nodeProcess() });
      // An RPG Maker plugin that throws while it loads (Last12's JSON.parse of an empty
      // parameter) makes main.js show the error instead of the game, which the release runs past.
      // Until the page has loaded, errors from plugin scripts only go to the console.
      const plugin = (e: ErrorEvent) => {
        // The stack tells where it came from (filename can name the page).
        if (!/\/js\/plugins\//.test(`${e.filename} ${(e.error as Error | undefined)?.stack}`)) return;
        console.warn("plugin error while loading:", e.error ?? e.message);
        e.stopImmediatePropagation();
        e.preventDefault();
      };
      win.addEventListener("error", plugin, true);
      win.addEventListener("load", () => win.removeEventListener("error", plugin, true));
      // RPG Maker fits the screen to the window only in NW.js (and on phones); a 1920x1080 MV
      // game would overflow the frame. Its core has loaded by the window's load event, where
      // this listener runs before the game's own, which starts it.
      win.addEventListener("load", () => {
        const graphics = (win as { Graphics?: { _defaultStretchMode?: () => boolean } }).Graphics;
        if (graphics?._defaultStretchMode) graphics._defaultStretchMode = () => true;
      });
    },
  });

  const frame = document.createElement("iframe");
  frame.id = "frame";
  frame.allow = "autoplay; fullscreen";
  frame.src = fs.fileUrl(startPage(fs, manifest));
  ui.canvas.hidden = true;
  document.body.append(frame);
  frame.addEventListener("load", () => {
    ui.hide();
    frame.focus();
  });
}
