// Yuka games (HOOK, SMEE, feng) through our own engine (crates/yuka), built by
// engines/yuka/build.sh into public/engines/yuka/. The engine runs the scripts and
// says which pictures to draw where; this side runs it 60 times a second, draws the pictures
// with WebGPU and passes clicks and saves.
import { importEngine, type EngineContext } from "../engine";
import type { GameFS } from "../gamefs";
import { Compositor } from "../compositor";
import { Presenter } from "../present";

interface Game {
  width(): number;
  height(): number;
  frame(): void;
  ended(): boolean;
  click(x: number, y: number): void;
  cancel(): void;
  wheel(up: boolean): void;
  point(x: number, y: number): void;
  /** a name entry's text box: [x, y, w, h] on the frame, empty when none is open */
  textBoxRect(): Int32Array;
  textBoxText(): string;
  setTextBox(text: string): void;
  draws(): Float32Array;
  texture(id: number): Uint8Array;
  sounds(): Sound[];
}

interface Sound {
  data: Uint8Array;
  volume: number;
  /** the game's sound channel (-1: all of them) */
  channel: number;
  stop: boolean;
  fadeMs: number;
  looped: boolean;
}

interface YukaModule {
  default(): Promise<unknown>;
  Game: new (font: Uint8Array) => Game;
}

// Saves get their own folder so they never shadow a game file of the same name.
const SAVES = "yuka-save";

function bind(fs: GameFS): void {
  const save = (name: string) => `${SAVES}/${name}`;
  Object.assign(globalThis, {
    yukaFileSize: (path: string) => (fs.exists(path) ? fs.size(path) : -1),
    yukaReadRange: (path: string, offset: number, length: number) =>
      fs.exists(path) ? fs.readRange(path, offset, length) : undefined,
    yukaList: () => fs.list(""),
    yukaReadSave: (name: string) => (fs.exists(save(name)) ? fs.read(save(name)) : undefined),
    yukaWriteSave: (name: string, bytes: Uint8Array) => fs.write(save(name), bytes.slice()),
  });
}

export async function start({ fs, ui, fonts }: EngineContext): Promise<void> {
  bind(fs);
  const presenter = await Presenter.create(ui.canvas);
  const engine = await importEngine<YukaModule>("yuka", "yuka_web.js");
  await engine.default();
  const game = new engine.Game(fonts[0] ?? new Uint8Array());
  const compositor = new Compositor(presenter.device, game.width(), game.height());
  const audio = new AudioContext();
  const channels = new Map<number, { source: AudioBufferSourceNode; gain: GainNode; started: boolean }>();
  const stopChannel = (channel: number, fadeMs: number) => {
    const previous = channels.get(channel);
    channels.delete(channel);
    if (!previous?.started) return;
    const now = audio.currentTime;
    previous.gain.gain.cancelScheduledValues(now);
    previous.gain.gain.setValueAtTime(previous.gain.gain.value, now);
    previous.gain.gain.linearRampToValueAtTime(0, now + fadeMs / 1000);
    previous.source.stop(now + fadeMs / 1000);
  };
  const playSound = (sound: Sound) => {
    const targets = sound.channel < 0 ? [...channels.keys()] : [sound.channel];
    for (const channel of targets) stopChannel(channel, sound.stop ? sound.fadeMs : 0);
    if (sound.stop) return;
    const source = audio.createBufferSource();
    const gain = audio.createGain();
    const pending = { source, gain, started: false };
    channels.set(sound.channel, pending);
    void audio
      .decodeAudioData(sound.data.slice().buffer)
      .then((buffer) => {
        if (channels.get(sound.channel) !== pending) return;
        source.buffer = buffer;
        source.loop = sound.looped;
        gain.gain.setValueAtTime(sound.volume, audio.currentTime);
        source.connect(gain).connect(audio.destination);
        source.start();
        pending.started = true;
        source.onended = () => {
          if (channels.get(sound.channel) === pending) channels.delete(sound.channel);
        };
      })
      .catch((error: unknown) => console.warn("Yuka sound:", error));
  };
  const resumeAudio = () => void audio.resume();
  window.addEventListener("pointerdown", resumeAudio, { once: true });
  window.addEventListener("keydown", resumeAudio, { once: true });
  const { canvas } = ui;
  canvas.addEventListener("pointerdown", (event) => {
    canvas.focus();
    if (event.button === 0) game.click(...presenter.toFrame(event));
    else if (event.button === 2) game.cancel();
  });
  canvas.addEventListener("pointermove", (event) => game.point(...presenter.toFrame(event)));
  canvas.addEventListener("contextmenu", (event) => event.preventDefault());
  canvas.addEventListener(
    "wheel",
    (event) => {
      event.preventDefault();
      if (event.deltaY !== 0) game.wheel(event.deltaY < 0);
    },
    { passive: false },
  );
  canvas.addEventListener("keydown", (event) => {
    if (event.code === "Escape") {
      event.preventDefault();
      game.cancel();
      return;
    }
    if (event.code === "ArrowUp" || event.code === "ArrowDown") {
      event.preventDefault();
      game.wheel(event.code === "ArrowUp");
      return;
    }
    if (event.code !== "Enter" && event.code !== "Space") return;
    event.preventDefault();
    game.click(-1, -1);
  });
  // A name entry's text box: the engine draws it; typing (with an IME) goes into an input
  // laid unseen over it, whose text the engine takes on each change.
  const box = document.createElement("input");
  box.type = "text";
  box.maxLength = 12;
  box.setAttribute("aria-label", "名前");
  Object.assign(box.style, {
    position: "fixed",
    opacity: "0",
    border: "0",
    padding: "0",
    background: "transparent",
  });
  box.addEventListener("input", () => game.setTextBox(box.value));
  box.addEventListener("keydown", (event) => {
    if (event.key !== "Enter" || event.isComposing) return;
    event.preventDefault();
    canvas.focus();
    game.click(-1, -1);
  });
  const placeBox = () => {
    const r = game.textBoxRect();
    if (r.length < 4) {
      if (box.isConnected) {
        box.remove();
        canvas.focus();
      }
      return;
    }
    const bounds = canvas.getBoundingClientRect();
    const k = Math.min(bounds.width / game.width(), bounds.height / game.height());
    const left = bounds.left + (bounds.width - game.width() * k) / 2;
    const top = bounds.top + (bounds.height - game.height() * k) / 2;
    Object.assign(box.style, {
      left: `${left + r[0] * k}px`,
      top: `${top + r[1] * k}px`,
      width: `${r[2] * k}px`,
      height: `${r[3] * k}px`,
      fontSize: `${Math.max(12, r[3] * k * 0.6)}px`,
    });
    if (!box.isConnected) {
      box.value = game.textBoxText();
      document.body.append(box);
      box.focus();
      // what was there is selected, so typing replaces it
      box.select();
    }
  };
  ui.hide();
  canvas.focus();

  const step = 1000 / 60;
  const begin = performance.now();
  let frames = 0;
  const tick = () => {
    try {
      const now = performance.now() - begin;
      // Catch up after a slow frame, but not after the tab slept.
      if (now - frames * step > 250) frames = Math.floor(now / step);
      let ran = false;
      while (frames * step <= now) {
        game.frame();
        for (const sound of game.sounds()) playSound(sound);
        frames += 1;
        ran = true;
      }
      if (game.ended()) {
        void ui.exit();
        return;
      }
      if (ran) {
        placeBox();
        presenter.showTexture(compositor.render(game));
      }
    } catch (error) {
      window.dispatchEvent(new ErrorEvent("error", { error }));
      return;
    }
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
}
