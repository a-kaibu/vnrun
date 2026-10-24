// SPDX-License-Identifier: MPL-2.0 (adapted from ethornell-rs, https://github.com/xmoezzz/ethornell-rs)
// BGI/Ethornell through ethornell-rs (third_party/ethornell-rs), built by
// engines/ethornell/build.sh into public/engines/ethornell/. The engine runs in ethornell.worker.js on a
// transferred canvas (WebGL2); this side forwards input, plays audio, answers the game's
// dialogs and passes saves to the GameFS. Adapted from third_party/ethornell-rs/platform/wasm/main.js.

import { engineUrl, type EngineContext } from "../engine";
import type { GameFS } from "../gamefs";
import type { FromWorker } from "./ethornell.worker";

// Input kinds and buttons of the host ABI (ethornell-rs platform/android/include/ABI.md).
const MOVE = 1,
  DOWN = 2,
  UP = 3,
  KEY_DOWN = 4,
  KEY_UP = 5,
  WHEEL = 6,
  CANCEL = 7;
const LEFT = 1,
  RIGHT = 2;

type Send = (kind: number, x?: number, y?: number, code?: number, value?: number) => void;
interface Audio {
  context: AudioContext;
  port: MessagePort;
}

function backingSize(canvas: HTMLCanvasElement) {
  const scale = window.devicePixelRatio || 1;
  return {
    width: Math.max(1, Math.round(canvas.clientWidth * scale)),
    height: Math.max(1, Math.round(canvas.clientHeight * scale)),
  };
}

async function startAudio(): Promise<Audio> {
  const context = new AudioContext();
  await context.audioWorklet.addModule(engineUrl("ethornell", "audio-worklet.js"));
  const node = new AudioWorkletNode(context, "ethornell-output", { outputChannelCount: [2] });
  node.connect(context.destination);
  const channel = new MessageChannel();
  node.port.postMessage({ port: channel.port1 }, [channel.port1]);
  // Browsers start audio suspended until the first tap or key.
  const resume = () => void context.resume();
  window.addEventListener("pointerdown", resume, { once: true });
  window.addEventListener("keydown", resume, { once: true });
  return { context, port: channel.port2 };
}

// A font for game text: one served with the player, else a font file in the game folder.
function chooseFont(fs: GameFS, fonts: Uint8Array[]): Uint8Array {
  if (fonts.length > 0) return fonts[0];
  const own = fs.list("").find((name) => /\.(ttf|otf|ttc)$/i.test(name));
  if (own) return fs.read(own);
  throw new Error("文字を描くフォントがありません (fonts/fonts.json か、ゲームのフォルダの .ttf)");
}

export async function start({ fs, manifest, ui, fonts }: EngineContext): Promise<void> {
  if (!("transferControlToOffscreen" in HTMLCanvasElement.prototype)) {
    throw new Error("このブラウザは OffscreenCanvas に対応していません");
  }
  const font = chooseFont(fs, fonts);
  const paths = (manifest.files || []).map((f) => f.p);
  const saves = Array.from(fs.saves, ([path, bytes]) => [path, bytes]);
  const audio = await startAudio().catch((error: unknown) => {
    console.warn("audio unavailable", error);
    return null;
  });

  const { canvas } = ui;
  const size = backingSize(canvas);
  const offscreen = canvas.transferControlToOffscreen();
  const worker = new Worker(new URL("./ethornell.worker.ts", import.meta.url), { type: "module" });
  const send: Send = (kind, x = 0, y = 0, code = 0, value = 0) =>
    worker.postMessage({ type: "input", kind, x, y, code, value });

  const started = new Promise<void>((resolve, reject) => {
    worker.onerror = (event) => reject(new Error(event.message));
    worker.onmessage = (event: MessageEvent<FromWorker>) => {
      const message = event.data;
      switch (message.type) {
        case "started":
          resolve();
          break;
        case "save":
          fs.write(message.path, message.bytes);
          break;
        case "remove":
          try {
            fs.remove(message.path);
          } catch (error) {
            console.warn(error);
          }
          break;
        case "title":
          if (message.text) document.title = message.text;
          break;
        case "confirm":
          worker.postMessage({ type: "answer", id: message.id, result: answerConfirm(message) });
          break;
        case "dialog":
          worker.postMessage({
            type: "dialog",
            id: message.id,
            response: JSON.stringify(answerDialog(JSON.parse(message.payload) as DialogRequest)),
          });
          break;
        case "closed":
          void ui.exit();
          break;
        case "error":
          reject(new Error(message.message));
          // After start-up, a stop is shown like any other failure.
          window.dispatchEvent(new ErrorEvent("error", { error: new Error(message.message) }));
          break;
      }
    };
  });

  const transfer: Transferable[] = [offscreen];
  if (audio) transfer.push(audio.port);
  worker.postMessage(
    {
      type: "start",
      module: engineUrl("ethornell", "ethornell_web.js"),
      urls: paths.map((p) => fs.fileUrl(p)),
      sizes: (manifest.files || []).map((f) => f.s || 0),
      paths,
      saves,
      font: font.slice().buffer,
      silent: !audio,
      sampleRate: audio ? audio.context.sampleRate : 48000,
      canvas: offscreen,
      audioPort: audio ? audio.port : null,
      ...size,
    },
    transfer,
  );
  await started;
  ui.hide();
  attachInput(canvas, worker, send, audio);
  canvas.focus();
}

function attachInput(canvas: HTMLCanvasElement, worker: Worker, send: Send, audio: Audio | null): void {
  const point = (event: PointerEvent | WheelEvent): [number, number] => {
    const scale = window.devicePixelRatio || 1;
    return [event.offsetX * scale, event.offsetY * scale];
  };
  const button = (event: PointerEvent) => (event.button === 2 ? RIGHT : event.button === 0 ? LEFT : 0);
  canvas.addEventListener("pointermove", (event) => send(MOVE, ...point(event)));
  canvas.addEventListener("pointerdown", (event) => {
    const code = button(event);
    if (!code) return;
    canvas.focus();
    canvas.setPointerCapture(event.pointerId);
    send(MOVE, ...point(event));
    send(DOWN, ...point(event), code);
  });
  canvas.addEventListener("pointerup", (event) => {
    const code = button(event);
    if (code) send(UP, ...point(event), code);
  });
  canvas.addEventListener("pointercancel", (event) => send(CANCEL, ...point(event)));
  canvas.addEventListener("contextmenu", (event) => event.preventDefault());
  canvas.addEventListener(
    "wheel",
    (event) => {
      event.preventDefault();
      if (event.deltaY) send(WHEEL, ...point(event), 0, event.deltaY < 0 ? 1 : -1);
    },
    { passive: false },
  );
  // Keys use Windows virtual-key codes, which keyCode still reports.
  const key = (kind: number) => (event: KeyboardEvent) => {
    if (event.metaKey || !event.keyCode) return;
    if ([9, 13, 27, 32, 33, 34, 37, 38, 39, 40].includes(event.keyCode)) event.preventDefault();
    send(kind, 0, 0, event.keyCode);
  };
  window.addEventListener("keydown", key(KEY_DOWN));
  window.addEventListener("keyup", key(KEY_UP));
  let resizing: ReturnType<typeof setTimeout> | undefined;
  new ResizeObserver(() => {
    clearTimeout(resizing);
    resizing = setTimeout(() => worker.postMessage({ type: "resize", ...backingSize(canvas) }), 100);
  }).observe(canvas);
  document.addEventListener("visibilitychange", () => {
    worker.postMessage({ type: "pause", mask: document.hidden ? 1 : 0 });
    if (audio) void (document.hidden ? audio.context.suspend() : audio.context.resume());
  });
}

// --- The game's dialogs, with the browser's own prompts ---------------------

// A message (code 0) or a yes/no question (code 1).
function answerConfirm({ code, text }: { code: number; text: string }): number {
  console.info(`ethornell dialog: ${text}`);
  if (code === 0) {
    window.alert(text);
    return 1;
  }
  return window.confirm(text) ? 1 : 0;
}

interface DialogRequest {
  type: string;
  title?: string;
  prompt?: string;
  fields: { label?: string; initial?: string }[];
  segments: number;
  options: string[];
}

// Name entry, choices and dates (request.type: input / segmented / select / date).
function answerDialog(request: DialogRequest): object {
  const ask = (label: string | undefined, initial: string | undefined) =>
    window.prompt(label || request.title || "", initial || "");
  switch (request.type) {
    case "input": {
      const texts: string[] = [];
      for (const field of request.fields) {
        const text = ask(field.label || request.title, field.initial);
        if (text === null) return { accepted: false };
        texts.push(text);
      }
      return { accepted: true, texts };
    }
    case "segmented": {
      const texts: string[] = [];
      for (let i = 0; i < request.segments; i += 1) {
        const text = ask(`${request.prompt || request.title || ""} (${i + 1}/${request.segments})`, "");
        if (text === null) return { accepted: false };
        texts.push(text);
      }
      return { accepted: true, texts };
    }
    case "select": {
      const list = request.options.map((option, i) => `${i + 1}. ${option}`).join("\n");
      const text = ask(`${request.prompt || request.title || ""}\n${list}`, "1");
      const index = Number.parseInt(text ?? "", 10) - 1;
      return index >= 0 && index < request.options.length ? { accepted: true, index } : { accepted: false };
    }
    default:
      return { accepted: false };
  }
}
