// Game files and saves for engines built with Emscripten: game files appear as lazy files read
// in blocks from the GameFS, and a saves folder is copied from the GameFS at start and back
// whenever a file in it changes.
import type { GameFS, Manifest } from "./gamefs";

// The parts of Emscripten's file system used here.
interface FSNode {
  contents: unknown;
  stream_ops: Record<string, unknown>;
}
export interface EmFS {
  mkdirTree(path: string): void;
  createFile(parent: string, name: string, properties: object, canRead: boolean, canWrite: boolean): FSNode;
  writeFile(path: string, data: Uint8Array): void;
  readFile(path: string): Uint8Array;
  readdir(path: string): string[];
  stat(path: string): { mode: number; mtime: Date };
  isDir(mode: number): boolean;
  analyzePath(path: string): { exists: boolean };
  ErrnoError: new (errno: number) => Error;
}
type ReadInto = (buffer: Uint8Array, offset: number, length: number, position: number) => number;

const BLOCK = 256 * 1024;
const CACHE_BLOCKS = 512; // 128MB

// Reads game files in blocks, keeping the most recently used ones.
function blockReader(fs: GameFS) {
  const blocks = new Map<string, Uint8Array>(); // `${path}\n${index}`
  return (path: string, size: number, position: number, out: Uint8Array, offset: number, length: number) => {
    const end = Math.min(size, position + length);
    let at = position;
    while (at < end) {
      const index = Math.floor(at / BLOCK);
      const key = `${path}\n${index}`;
      let block = blocks.get(key);
      if (block) {
        blocks.delete(key);
      } else {
        block = fs.readRange(path, index * BLOCK, BLOCK).slice();
        if (blocks.size >= CACHE_BLOCKS) blocks.delete(blocks.keys().next().value!);
      }
      blocks.set(key, block);
      const from = at - index * BLOCK;
      const count = Math.min(end - at, block.length - from);
      if (count <= 0) break;
      out.set(block.subarray(from, from + count), offset + (at - position));
      at += count;
    }
    return at - position;
  };
}

function parentDir(path: string): string {
  return path.slice(0, path.lastIndexOf("/"));
}

// A read-only file of `size` bytes whose reads go to `read` (after Emscripten's createLazyFile,
// which only works in workers).
function lazyFile(FS: EmFS, path: string, size: number, read: ReadInto): void {
  FS.mkdirTree(parentDir(path));
  const node = FS.createFile(parentDir(path), path.slice(path.lastIndexOf("/") + 1), {}, true, false);
  node.contents = { length: size }; // never read: the stream ops below replace MEMFS's
  Object.defineProperties(node, { usedBytes: { get: () => size } });
  const ops = { ...node.stream_ops };
  ops.read = (_stream: unknown, buffer: Uint8Array, offset: number, length: number, position: number) =>
    read(buffer, offset, length, position);
  ops.mmap = () => {
    throw new FS.ErrnoError(28); // EINVAL
  };
  node.stream_ops = ops;
}

/**
 * Puts the game's files under `/game`. With `lowercase`, each file is also reachable by its
 * lowercased path, for engines written against Windows' case-insensitive names.
 */
export function mountGame(FS: EmFS, fs: GameFS, manifest: Manifest, lowercase = false): void {
  const read = blockReader(fs);
  for (const f of manifest.files || []) {
    const size = f.s || 0;
    const reader: ReadInto = (buffer, offset, length, position) =>
      read(f.p, size, position, buffer, offset, length);
    lazyFile(FS, `/game/${f.p}`, size, reader);
    const lower = `/game/${f.p.toLowerCase()}`;
    if (lowercase && lower !== `/game/${f.p}` && !FS.analyzePath(lower).exists)
      lazyFile(FS, lower, size, reader);
  }
}

/** The `/saves` folder, kept in the GameFS under `prefix` (saves never shadow game files). */
export class Saves {
  private seen = new Map<string, number>(); // path under /saves -> mtime

  constructor(
    private readonly FS: EmFS,
    private readonly fs: GameFS,
    private readonly prefix: string,
  ) {
    FS.mkdirTree("/saves");
    for (const [path, bytes] of fs.saves) {
      if (!path.startsWith(`${prefix}/`)) continue;
      const local = `/saves/${path.slice(prefix.length + 1)}`;
      FS.mkdirTree(parentDir(local));
      FS.writeFile(local, bytes);
    }
    this.scan(false);
  }

  private files(dir = "/saves", out: [string, number][] = []): [string, number][] {
    for (const name of this.FS.readdir(dir)) {
      if (name === "." || name === "..") continue;
      const path = `${dir}/${name}`;
      const stat = this.FS.stat(path);
      if (this.FS.isDir(stat.mode)) this.files(path, out);
      else out.push([path, stat.mtime.getTime()]);
    }
    return out;
  }

  /** Sends files written or removed since the last scan. */
  scan(send = true): void {
    const now = new Map(this.files());
    const remote = (path: string) => `${this.prefix}/${path.slice("/saves/".length)}`;
    for (const [path, mtime] of now) {
      if (send && this.seen.get(path) !== mtime) this.fs.write(remote(path), this.FS.readFile(path));
    }
    for (const path of this.seen.keys()) {
      if (send && !now.has(path)) {
        try {
          this.fs.remove(remote(path));
        } catch (error) {
          console.warn(error);
        }
      }
    }
    this.seen = now;
  }
}
