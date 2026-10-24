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
      existsSync: (p: string) => !/\.sav$/i.test(p) || fs.saves.has(path(p)),
      mkdirSync: () => undefined,
      readFileSync: (p: string) => new TextDecoder().decode(fs.saves.get(path(p)) ?? new Uint8Array()),
      writeFileSync: (p: string, data: string) => fs.write(path(p), new TextEncoder().encode(String(data))),
      unlinkSync: (p: string) => fs.saves.has(path(p)) && fs.remove(path(p)),
    },
    ipcRenderer: {
      sendSync: (channel: string) =>
        channel === "getAppPath" ? process.__dirname : channel === "getProcess" ? process : undefined,
    },
  };
}

// NW.js games that edit the RPG Maker core can call Node's require at the top level (an
// RPG Maker MV game reads its language from lng.txt with fs). This require has the synchronous
// fs calls (reads from the game, writes as saves) and path; other modules throw as in Node.
// `process` stays undefined, so the core keeps its web storage.
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
      return encoding(opt) ? new TextDecoder().decode(data) : data;
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
  const modules: Record<string, unknown> = { fs: nodeFs, path: nodePath };
  return (name: string) => {
    if (name in modules) return modules[name];
    throw new Error(`Cannot find module '${name}'`);
  };
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
      Object.assign(win, { studio_api: studio, require });
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
