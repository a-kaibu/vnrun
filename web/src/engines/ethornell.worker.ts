// SPDX-License-Identifier: MPL-2.0 (adapted from ethornell-rs, https://github.com/xmoezzz/ethornell-rs)
// Runs ethornell-rs (BGI/Ethornell) off the main thread. Adapted from the upstream worker
// (third_party/ethornell-rs/platform/wasm/worker.js): game files are read over HTTP with
// synchronous Range requests (allowed in workers) through a block cache instead of from picked
// File objects, and saves go back to the page, which sends them to the server.

import "../wgpu-compat";

// The part of the wasm-bindgen module (public/engines/ethornell/ethornell_web.js) used here.
interface EngineEvent {
  kind: number;
  requestId: number;
  code: number;
  text: string;
  free(): void;
}
interface WebEngine {
  step(now: number): void;
  pollEvent(): EngineEvent | undefined;
  requestPayload(id: number): string;
  input(kind: number, x: number, y: number, code: number, value: number): void;
  font(): unknown;
  attach(renderer: unknown): void;
  resize(width: number, height: number): void;
  pause(mask: number): void;
  completeRequest(id: number, result: number): void;
  completeDialog(id: number, response: string): void;
  requestClose(): void;
}
interface EthornellWeb {
  default(): Promise<unknown>;
  WebEngine: new (
    listing: [string, number][],
    saves: [string, Uint8Array][],
    font: Uint8Array,
    debug: boolean,
    silent: boolean,
  ) => WebEngine;
  createRenderer(canvas: OffscreenCanvas, width: number, height: number, font: unknown): Promise<unknown>;
  renderAudio(out: Float32Array): void;
  setAudioSampleRate(rate: number): void;
}

/** Messages the worker sends to the page. */
export type FromWorker =
  | { type: "started" }
  | { type: "save"; path: string; bytes: Uint8Array }
  | { type: "remove"; path: string }
  | { type: "title"; text: string }
  | { type: "confirm"; id: number; code: number; text: string }
  | { type: "dialog"; id: number; payload: string }
  | { type: "closed" }
  | { type: "error"; message: string };

interface Start {
  type: "start";
  module: string;
  urls: string[];
  sizes: number[];
  paths: string[];
  saves: [string, Uint8Array][];
  font: ArrayBuffer;
  silent: boolean;
  sampleRate: number;
  canvas: OffscreenCanvas;
  audioPort: MessagePort | null;
  width: number;
  height: number;
}
interface Input {
  type: "input";
  kind: number;
  x: number;
  y: number;
  code: number;
  value: number;
}
type ToWorker =
  | Start
  | Input
  | { type: "resize"; width: number; height: number }
  | { type: "pause"; mask: number }
  | { type: "answer"; id: number; result: number }
  | { type: "dialog"; id: number; response: string }
  | { type: "close" };

const post = (message: FromWorker, transfer: Transferable[] = []) => self.postMessage(message, transfer);

const BLOCK = 256 * 1024;
const CACHE_BLOCKS = 512; // 128MB

let urls: string[] = []; // file id -> URL
let sizes: number[] = [];
const blocks = new Map<string, Uint8Array>(); // `${id}:${index}`, least recently used first
let engine: WebEngine | null = null;
let canvas: OffscreenCanvas | null = null;
let stopped = false;
// A release that arrives in the same frame as its press is held to the next frame: the
// engine reads a press and release within one frame as the pointer leaving the button
// (a quick click on a slow frame would otherwise not activate it).
const DOWN = 2,
  UP = 3;
let pressedThisFrame = false;
let held: Input[] = [];

function fetchBlock(id: number, index: number): Uint8Array {
  const key = `${id}:${index}`;
  let block = blocks.get(key);
  if (block) {
    blocks.delete(key);
  } else {
    const start = index * BLOCK;
    const end = Math.min(sizes[id], start + BLOCK) - 1;
    const xhr = new XMLHttpRequest();
    xhr.open("GET", urls[id], false);
    xhr.responseType = "arraybuffer";
    xhr.setRequestHeader("Range", `bytes=${start}-${end}`);
    xhr.send(null);
    if (xhr.status !== 206 && xhr.status !== 200) throw new Error(`GET ${urls[id]}: HTTP ${xhr.status}`);
    block = new Uint8Array(xhr.response as ArrayBuffer);
    // A server that ignores Range sends the whole file.
    if (xhr.status === 200) block = block.subarray(start, end + 1);
    if (blocks.size >= CACHE_BLOCKS) blocks.delete(blocks.keys().next().value!);
  }
  blocks.set(key, block);
  return block;
}

// --- Functions the wasm module imports from the worker's global scope ----

Object.assign(globalThis, {
  ethornellReadAt: (id: number, offset: number, out: Uint8Array): number => {
    if (urls[id] === undefined) throw new Error(`no file ${id}`);
    const length = Math.max(0, Math.min(out.length, sizes[id] - offset));
    let done = 0;
    while (done < length) {
      const at = offset + done;
      const index = Math.floor(at / BLOCK);
      const block = fetchBlock(id, index);
      const from = at - index * BLOCK;
      const count = Math.min(length - done, block.length - from);
      if (count <= 0) break;
      out.set(block.subarray(from, from + count), done);
      done += count;
    }
    return done;
  },
  ethornellSaveUserFile: (path: string, bytes: Uint8Array) => {
    const copy = bytes.slice();
    post({ type: "save", path, bytes: copy }, [copy.buffer]);
  },
  ethornellRemoveUserFile: (path: string) => post({ type: "remove", path }),
});

// --- Engine loop ----------------------------------------------------------

const nextFrame: (callback: (now: number) => void) => void =
  "requestAnimationFrame" in globalThis
    ? (callback) => requestAnimationFrame(callback)
    : (callback) => setTimeout(() => callback(performance.now()), 16);

function report(error: unknown): void {
  stopped = true;
  post({ type: "error", message: String(error instanceof Error ? error.message : error) });
}

function drainEvents(): void {
  for (;;) {
    const event = engine!.pollEvent();
    if (!event) return;
    const { kind, requestId, code, text } = event;
    event.free();
    if (kind === 5) {
      post({ type: "dialog", id: requestId, payload: engine!.requestPayload(requestId) });
    } else if (kind === 2) {
      post({ type: "confirm", id: requestId, code, text });
    } else if (kind === 3) {
      stopped = true;
      post({ type: "closed" });
    } else if (kind === 4) {
      post({ type: "title", text });
    } else {
      console.info(`ethornell: ${text}`);
    }
  }
}

function frame(now: number): void {
  if (stopped) return;
  try {
    engine!.step(now);
    drainEvents();
  } catch (error) {
    report(error);
    return;
  }
  pressedThisFrame = false;
  const release = held;
  held = [];
  for (const message of release) input(message);
  nextFrame(frame);
}

function input(message: Input): void {
  if (held.length > 0 || (message.kind === UP && pressedThisFrame)) {
    held.push(message);
    return;
  }
  if (message.kind === DOWN) pressedThisFrame = true;
  guarded((e) => e.input(message.kind, message.x, message.y, message.code, message.value));
}

function guarded(action: (engine: WebEngine) => void): void {
  if (!engine || stopped) return;
  try {
    action(engine);
    drainEvents();
  } catch (error) {
    report(error);
  }
}

async function start(message: Start): Promise<void> {
  const web = (await import(/* @vite-ignore */ message.module)) as EthornellWeb;
  await web.default();
  web.setAudioSampleRate(message.sampleRate || 48000);
  urls = message.urls;
  sizes = message.sizes;
  const listing = message.paths.map((path, index): [string, number] => [path, sizes[index]]);
  engine = new web.WebEngine(listing, message.saves, new Uint8Array(message.font), false, message.silent);
  canvas = message.canvas;
  canvas.width = message.width;
  canvas.height = message.height;
  const renderer = await web.createRenderer(canvas, message.width, message.height, engine.font());
  engine.attach(renderer);
  const port = message.audioPort;
  if (port) {
    // The AudioWorklet asks for frames; render and transfer them.
    port.onmessage = (event: MessageEvent<number>) => {
      const chunk = new Float32Array(event.data * 2);
      web.renderAudio(chunk);
      port.postMessage(chunk, [chunk.buffer]);
    };
  }
  drainEvents();
  post({ type: "started" });
  nextFrame(frame);
}

self.onmessage = (event: MessageEvent<ToWorker>) => {
  const message = event.data;
  switch (message.type) {
    case "start":
      start(message).catch(report);
      break;
    case "input":
      input(message);
      break;
    case "resize":
      guarded((e) => {
        canvas!.width = message.width;
        canvas!.height = message.height;
        e.resize(message.width, message.height);
      });
      break;
    case "pause":
      guarded((e) => e.pause(message.mask));
      break;
    case "answer":
      guarded((e) => e.completeRequest(message.id, message.result));
      break;
    case "dialog":
      guarded((e) => e.completeDialog(message.id, message.response));
      break;
    case "close":
      guarded((e) => e.requestClose());
      break;
  }
};
