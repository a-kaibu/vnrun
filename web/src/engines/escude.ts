// Escu:de games through our own engine (crates/escude), built by engines/escude/build.sh into
// public/engines/escude/. The engine compiles and runs the game's C programs and draws the
// screen itself; this side runs it 60 times a second, shows the screen it hands over, plays
// sounds on numbered channels and passes the mouse and the keyboard on.
import { importEngine, type EngineContext } from "../engine";
import type { GameFS } from "../gamefs";
import { Presenter } from "../present";

interface Game {
  width(): number;
  height(): number;
  frame(now: number): void;
  serial(): number;
  pixels(): Uint8Array;
  ended(): boolean;
  point(x: number, y: number): void;
  button_down(button: number): void;
  button_up(button: number): void;
  key(code: number, down: boolean): void;
  set_clock(secs: number): void;
  sounds(): Sound[];
}

interface Sound {
  channel: number;
  op: "play" | "stop" | "volume" | "pause" | "resume";
  data?: Uint8Array;
  volume: number;
  repeat: boolean;
}

interface EscudeModule {
  default(): Promise<unknown>;
  Game: new (font: Uint8Array) => Game;
}

// Saves get their own folder so they never shadow a game file of the same name.
const SAVES = "escude-save";

function bind(fs: GameFS): void {
  const save = (name: string) => `${SAVES}/${name}`;
  Object.assign(globalThis, {
    escudeFileSize: (path: string) => (fs.exists(path) ? fs.size(path) : -1),
    escudeReadRange: (path: string, offset: number, length: number) =>
      fs.exists(path) ? fs.readRange(path, offset, length) : undefined,
    escudeList: () => fs.list(""),
    escudeReadSave: (name: string) => (fs.exists(save(name)) ? fs.read(save(name)) : undefined),
    escudeWriteSave: (name: string, bytes: Uint8Array) => fs.write(save(name), bytes.slice()),
  });
}

// Windows virtual-key codes by KeyboardEvent.code, for getkey.
function virtualKey(code: string): number | undefined {
  const named: Record<string, number> = {
    Backspace: 8,
    Tab: 9,
    Enter: 13,
    NumpadEnter: 13,
    ShiftLeft: 16,
    ShiftRight: 16,
    ControlLeft: 17,
    ControlRight: 17,
    AltLeft: 18,
    AltRight: 18,
    Escape: 27,
    Space: 32,
    PageUp: 33,
    PageDown: 34,
    End: 35,
    Home: 36,
    ArrowLeft: 37,
    ArrowUp: 38,
    ArrowRight: 39,
    ArrowDown: 40,
    Delete: 46,
  };
  if (code in named) return named[code];
  if (/^Key[A-Z]$/.test(code)) return code.charCodeAt(3);
  if (/^Digit[0-9]$/.test(code)) return code.charCodeAt(5);
  if (/^F([1-9]|1[0-2])$/.test(code)) return 111 + Number(code.slice(1));
  return undefined;
}

class Channels {
  private readonly playing = new Map<
    number,
    {
      source?: AudioBufferSourceNode;
      gain: GainNode;
      buffer?: AudioBuffer;
      at: number;
      offset: number;
      repeat: boolean;
    }
  >();

  constructor(private readonly audio: AudioContext) {}

  handle(s: Sound): void {
    const c = this.playing.get(s.channel);
    switch (s.op) {
      case "play":
        this.stop(s.channel);
        if (s.data) this.play(s.channel, s.data, s.volume, s.repeat);
        break;
      case "stop":
        this.stop(s.channel);
        break;
      case "volume":
        if (c) c.gain.gain.setValueAtTime(s.volume, this.audio.currentTime);
        break;
      case "pause":
        if (c?.source && c.buffer) {
          c.offset = (c.offset + this.audio.currentTime - c.at) % c.buffer.duration;
          c.source.onended = null;
          c.source.stop();
          c.source = undefined;
        }
        break;
      case "resume":
        if (c && !c.source && c.buffer) this.start(c, c.offset);
        break;
    }
  }

  private play(channel: number, data: Uint8Array, volume: number, repeat: boolean): void {
    const gain = this.audio.createGain();
    gain.gain.setValueAtTime(volume, this.audio.currentTime);
    gain.connect(this.audio.destination);
    const entry = { gain, at: 0, offset: 0, repeat } as {
      source?: AudioBufferSourceNode;
      gain: GainNode;
      buffer?: AudioBuffer;
      at: number;
      offset: number;
      repeat: boolean;
    };
    this.playing.set(channel, entry);
    void this.audio
      .decodeAudioData(data.slice().buffer)
      .then((buffer) => {
        if (this.playing.get(channel) !== entry) return;
        entry.buffer = buffer;
        this.start(entry, 0);
      })
      .catch((error: unknown) => console.warn("Escu:de sound:", error));
  }

  private start(
    entry: {
      source?: AudioBufferSourceNode;
      gain: GainNode;
      buffer?: AudioBuffer;
      at: number;
      offset: number;
      repeat: boolean;
    },
    offset: number,
  ): void {
    const source = this.audio.createBufferSource();
    source.buffer = entry.buffer!;
    source.loop = entry.repeat;
    source.connect(entry.gain);
    source.start(0, offset);
    entry.source = source;
    entry.at = this.audio.currentTime;
    entry.offset = offset;
  }

  private stop(channel: number): void {
    const c = this.playing.get(channel);
    this.playing.delete(channel);
    if (!c) return;
    c.source?.stop();
    c.gain.disconnect();
  }
}

export async function start({ fs, ui, fonts }: EngineContext): Promise<void> {
  bind(fs);
  const presenter = await Presenter.create(ui.canvas);
  const escude = await importEngine<EscudeModule>("escude", "escude_web.js");
  await escude.default();
  const game = new escude.Game(fonts[0] ?? new Uint8Array());
  const audio = new AudioContext();
  const channels = new Channels(audio);
  const resumeAudio = () => void audio.resume();
  window.addEventListener("pointerdown", resumeAudio, { once: true });
  window.addEventListener("keydown", resumeAudio, { once: true });
  const { canvas } = ui;
  // Mouse buttons as the engine numbers them: 0 left, 1 right, 2 middle.
  const buttons: Record<number, number> = { 0: 0, 2: 1, 1: 2 };
  canvas.addEventListener("pointerdown", (event) => {
    event.preventDefault();
    canvas.setPointerCapture(event.pointerId);
    game.point(...presenter.toFrame(event));
    const b = buttons[event.button];
    if (b !== undefined) game.button_down(b);
  });
  canvas.addEventListener("pointerup", (event) => {
    game.point(...presenter.toFrame(event));
    const b = buttons[event.button];
    if (b !== undefined) game.button_up(b);
  });
  canvas.addEventListener("pointermove", (event) => game.point(...presenter.toFrame(event)));
  canvas.addEventListener("contextmenu", (event) => event.preventDefault());
  const onKey = (down: boolean) => (event: KeyboardEvent) => {
    const target = event.target;
    if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement) return;
    const code = virtualKey(event.code);
    if (code === undefined) return;
    event.preventDefault();
    game.key(code, down);
  };
  window.addEventListener("keydown", onKey(true));
  window.addEventListener("keyup", onKey(false));
  ui.hide();

  const step = 1000 / 60;
  const begin = performance.now();
  let frames = 0;
  let shown = -1;
  const tick = () => {
    try {
      const now = performance.now() - begin;
      // Catch up after a slow frame, but not after the tab slept.
      if (now - frames * step > 250) frames = Math.floor(now / step);
      const date = new Date();
      game.set_clock(date.getTime() / 1000 - date.getTimezoneOffset() * 60);
      // one run of the game a display frame: it keeps its own time
      if (frames * step <= now) {
        game.frame(now);
        for (const sound of game.sounds()) channels.handle(sound);
        frames = Math.floor(now / step) + 1;
      }
      if (game.ended()) {
        void ui.exit();
        return;
      }
      const serial = game.serial();
      if (serial !== shown) {
        shown = serial;
        presenter.show(game.pixels(), game.width(), game.height());
      }
    } catch (error) {
      window.dispatchEvent(new ErrorEvent("error", { error }));
      return;
    }
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
}
