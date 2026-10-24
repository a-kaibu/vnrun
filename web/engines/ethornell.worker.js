// SPDX-License-Identifier: MPL-2.0 (adapted from ethornell-rs, https://github.com/xmoezzz/ethornell-rs)
// Runs ethornell-rs (BGI/Ethornell) off the main thread. Adapted from the upstream worker
// (third_party/ethornell-rs/platform/wasm/worker.js): game files are read over HTTP with
// synchronous Range requests (allowed in workers) through a block cache instead of from picked
// File objects, and saves go back to the page, which sends them to the server.
import init, { WebEngine, createRenderer, renderAudio, setAudioSampleRate } from "./ethornell/ethornell_web.js";

const BLOCK = 256 * 1024;
const CACHE_BLOCKS = 512; // 128MB

let urls = []; // file id -> URL
let sizes = [];
const blocks = new Map(); // `${id}:${index}` -> Uint8Array, least recently used first
let engine = null;
let canvas = null;
let stopped = false;
// A release that arrives in the same frame as its press is held to the next frame: the
// engine reads a press and release within one frame as the pointer leaving the button
// (a quick click on a slow frame would otherwise not activate it).
const DOWN = 2, UP = 3;
let pressedThisFrame = false;
let held = [];

function fetchBlock(id, index) {
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
    block = new Uint8Array(xhr.response);
    // A server that ignores Range sends the whole file.
    if (xhr.status === 200) block = block.subarray(start, end + 1);
    if (blocks.size >= CACHE_BLOCKS) blocks.delete(blocks.keys().next().value);
  }
  blocks.set(key, block);
  return block;
}

// --- Functions the wasm module imports from the worker's global scope ----

globalThis.ethornellReadAt = (id, offset, out) => {
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
};

globalThis.ethornellSaveUserFile = (path, bytes) => {
  const copy = bytes.slice();
  postMessage({ type: "save", path, bytes: copy }, [copy.buffer]);
};

globalThis.ethornellRemoveUserFile = (path) => {
  postMessage({ type: "remove", path });
};

// --- Engine loop ----------------------------------------------------------

const nextFrame = globalThis.requestAnimationFrame
  ? (callback) => requestAnimationFrame(callback)
  : (callback) => setTimeout(() => callback(performance.now()), 16);

function report(error) {
  stopped = true;
  postMessage({ type: "error", message: String(error && error.message ? error.message : error) });
}

function drainEvents() {
  for (;;) {
    const event = engine.pollEvent();
    if (!event) return;
    const { kind, requestId, code, text } = event;
    event.free();
    if (kind === 5) {
      postMessage({ type: "dialog", id: requestId, payload: engine.requestPayload(requestId) });
    } else if (kind === 2) {
      postMessage({ type: "confirm", id: requestId, code, text });
    } else if (kind === 3) {
      stopped = true;
      postMessage({ type: "closed" });
    } else if (kind === 4) {
      postMessage({ type: "title", text });
    } else {
      console.info(`ethornell: ${text}`);
    }
  }
}

function frame(now) {
  if (stopped) return;
  try {
    engine.step(now);
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

function input(message) {
  if (held.length > 0 || (message.kind === UP && pressedThisFrame)) {
    held.push(message);
    return;
  }
  if (message.kind === DOWN) pressedThisFrame = true;
  guarded(() => engine.input(message.kind, message.x, message.y, message.code, message.value));
}

function guarded(action) {
  if (!engine || stopped) return;
  try {
    action();
    drainEvents();
  } catch (error) {
    report(error);
  }
}

async function start(message) {
  await init();
  setAudioSampleRate(message.sampleRate || 48000);
  urls = message.urls;
  sizes = message.sizes;
  const listing = message.paths.map((path, index) => [path, sizes[index]]);
  engine = new WebEngine(listing, message.saves, new Uint8Array(message.font), false, message.silent);
  canvas = message.canvas;
  canvas.width = message.width;
  canvas.height = message.height;
  const renderer = await createRenderer(canvas, message.width, message.height, engine.font());
  engine.attach(renderer);
  if (message.audioPort) {
    // The AudioWorklet asks for frames; render and transfer them.
    message.audioPort.onmessage = (event) => {
      const chunk = new Float32Array(event.data * 2);
      renderAudio(chunk);
      message.audioPort.postMessage(chunk, [chunk.buffer]);
    };
  }
  drainEvents();
  postMessage({ type: "started" });
  nextFrame(frame);
}

onmessage = (event) => {
  const message = event.data;
  switch (message.type) {
    case "start":
      start(message).catch(report);
      break;
    case "input":
      input(message);
      break;
    case "resize":
      guarded(() => {
        canvas.width = message.width;
        canvas.height = message.height;
        engine.resize(message.width, message.height);
      });
      break;
    case "pause":
      guarded(() => engine.pause(message.mask));
      break;
    case "answer":
      guarded(() => engine.completeRequest(message.id, message.result));
      break;
    case "dialog":
      guarded(() => engine.completeDialog(message.id, message.response));
      break;
    case "close":
      guarded(() => engine.requestClose());
      break;
  }
};
