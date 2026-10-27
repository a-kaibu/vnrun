// LiveMaker games through our own engine (crates/livemaker), built by engines/livemaker/build.sh
// into public/engines/livemaker/. The engine runs the charts and says which pictures to draw
// where; this side runs it 60 times a second, draws the pictures with WebGPU and plays sounds on
// channels named after the game's sound components.
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
  key(code: number): void;
  point(x: number, y: number): void;
  draws(): Float32Array;
  texture(id: number): Uint8Array;
  sounds(): Sound[];
}

interface Sound {
  channel: string;
  data?: Uint8Array;
  volume: number;
  repeat: boolean;
  fadeMs: number;
}

interface LivemakerModule {
  default(): Promise<unknown>;
  Game: new (font: Uint8Array) => Game;
}

// Saves get their own folder so they never shadow a game file of the same name.
const SAVES = "livemaker-save";

function bind(fs: GameFS): void {
  const save = (name: string) => `${SAVES}/${name}`;
  Object.assign(globalThis, {
    livemakerFileSize: (path: string) => (fs.exists(path) ? fs.size(path) : -1),
    livemakerReadRange: (path: string, offset: number, length: number) =>
      fs.exists(path) ? fs.readRange(path, offset, length) : undefined,
    livemakerList: () => fs.list(""),
    livemakerReadSave: (name: string) => (fs.exists(save(name)) ? fs.read(save(name)) : undefined),
    livemakerWriteSave: (name: string, bytes: Uint8Array) => fs.write(save(name), bytes.slice()),
  });
}

export async function start({ fs, ui, fonts }: EngineContext): Promise<void> {
  bind(fs);
  const presenter = await Presenter.create(ui.canvas);
  const livemaker = await importEngine<LivemakerModule>("livemaker", "livemaker_web.js");
  await livemaker.default();
  const game = new livemaker.Game(fonts[0] ?? new Uint8Array());
  const compositor = new Compositor(presenter.device, game.width(), game.height());
  const audio = new AudioContext();
  const channels = new Map<string, { source: AudioBufferSourceNode; gain: GainNode; started: boolean }>();
  const stop = (channel: string, fadeMs: number) => {
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
    stop(sound.channel, sound.data ? 0 : sound.fadeMs);
    if (!sound.data) return;
    const source = audio.createBufferSource();
    const gain = audio.createGain();
    const pending = { source, gain, started: false };
    channels.set(sound.channel, pending);
    void audio
      .decodeAudioData(sound.data.slice().buffer)
      .then((buffer) => {
        if (channels.get(sound.channel) !== pending) return;
        source.buffer = buffer;
        source.loop = sound.repeat;
        gain.gain.setValueAtTime(sound.volume, audio.currentTime);
        source.connect(gain).connect(audio.destination);
        source.start();
        pending.started = true;
        source.onended = () => {
          if (channels.get(sound.channel) === pending) channels.delete(sound.channel);
        };
      })
      .catch((error: unknown) => console.warn("LiveMaker sound:", error));
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
  // Windows virtual-key codes, which the charts read as @KeyClick[code].
  const keyCodes: Record<string, number> = {
    Enter: 13,
    NumpadEnter: 13,
    Escape: 27,
    Space: 32,
    ArrowLeft: 37,
    ArrowUp: 38,
    ArrowRight: 39,
    ArrowDown: 40,
  };
  canvas.addEventListener("keydown", (event) => {
    const code = keyCodes[event.code];
    if (code === undefined) return;
    event.preventDefault();
    game.key(code);
    if (code === 13) game.click(-1, -1);
  });
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
      if (ran) presenter.showTexture(compositor.render(game));
    } catch (error) {
      window.dispatchEvent(new ErrorEvent("error", { error }));
      return;
    }
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
}
