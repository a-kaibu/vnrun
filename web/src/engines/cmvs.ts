// CMVS (Purple software: CPZ archives, PS2 scripts, PB3 pictures) games through our own engine
// (crates/cmvs), built by engines/cmvs/build.sh into public/engines/cmvs/. The engine runs the
// scenario scripts and says which pictures to draw where; this side runs it 60 times a second,
// draws the pictures with WebGPU and plays sounds on the channels the engine names.
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
  key(button: number): void;
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

interface CmvsModule {
  default(): Promise<unknown>;
  Game: new (font: Uint8Array) => Game;
}

// Saves get their own folder so they never shadow a game file of the same name.
const SAVES = "cmvs-save";

function bind(fs: GameFS): void {
  const save = (name: string) => `${SAVES}/${name}`;
  Object.assign(globalThis, {
    cmvsFileSize: (path: string) => (fs.exists(path) ? fs.size(path) : -1),
    cmvsReadRange: (path: string, offset: number, length: number) =>
      fs.exists(path) ? fs.readRange(path, offset, length) : undefined,
    cmvsList: () => fs.fileList().map((f) => f.path),
    cmvsReadSave: (name: string) => (fs.exists(save(name)) ? fs.read(save(name)) : undefined),
    cmvsWriteSave: (name: string, bytes: Uint8Array) => fs.write(save(name), bytes.slice()),
  });
}

export async function start({ fs, ui, fonts }: EngineContext): Promise<void> {
  bind(fs);
  const presenter = await Presenter.create(ui.canvas);
  const engine = await importEngine<CmvsModule>("cmvs", "cmvs_web.js");
  await engine.default();
  const game = new engine.Game(fonts[0] ?? new Uint8Array());
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
      .catch((error: unknown) => console.warn("CMVS sound:", error));
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
  // arrows, then the buttons OK, cancel and skip
  const KEYS: Record<string, number> = {
    ArrowUp: 0,
    ArrowDown: 1,
    ArrowLeft: 2,
    ArrowRight: 3,
    Enter: 4,
    Space: 4,
    KeyZ: 4,
    Escape: 5,
    KeyX: 5,
    ControlLeft: 6,
    ControlRight: 6,
  };
  canvas.addEventListener("keydown", (event) => {
    const button = KEYS[event.code];
    if (button === undefined) return;
    event.preventDefault();
    game.key(button);
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
