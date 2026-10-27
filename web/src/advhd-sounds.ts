export type SoundEvent =
  | { kind: "play"; ch: string; data: Uint8Array; looped: boolean; volume: number; fade: number }
  | { kind: "stop"; ch: string; fade: number };

// Sounds by channel ("bgm01", "se01", a character's voice); "*" stops them all.
export class Sounds {
  private readonly context = new AudioContext();
  private readonly playing = new Map<
    string,
    { source: AudioBufferSourceNode; gain: GainNode; started: boolean }
  >();

  constructor() {
    // Browsers start audio suspended until the first tap or key.
    const resume = () => void this.context.resume();
    window.addEventListener("pointerdown", resume, { once: true });
    window.addEventListener("keydown", resume, { once: true });
  }

  private stop(ch: string, fade: number): void {
    const s = this.playing.get(ch);
    if (!s) return;
    this.playing.delete(ch);
    if (!s.started) return;
    const now = this.context.currentTime;
    s.gain.gain.cancelScheduledValues(now);
    s.gain.gain.setValueAtTime(s.gain.gain.value, now);
    s.gain.gain.linearRampToValueAtTime(0, now + fade / 1000);
    s.source.stop(now + fade / 1000);
  }

  handle(e: SoundEvent): void {
    if (e.kind === "stop") {
      for (const ch of e.ch === "*" ? [...this.playing.keys()] : [e.ch]) this.stop(ch, e.fade);
      return;
    }
    this.stop(e.ch, 0);
    // a later play on the channel replaces this one even before it is decoded
    const source = this.context.createBufferSource();
    const gain = this.context.createGain();
    const pending = { source, gain, started: false };
    this.playing.set(e.ch, pending);
    this.context.decodeAudioData(e.data.slice().buffer).then(
      (buffer) => {
        if (this.playing.get(e.ch)?.source !== source) return;
        source.buffer = buffer;
        source.loop = e.looped;
        const now = this.context.currentTime;
        gain.gain.setValueAtTime(e.fade > 0 ? 0 : e.volume, now);
        if (e.fade > 0) gain.gain.linearRampToValueAtTime(e.volume, now + e.fade / 1000);
        source.connect(gain).connect(this.context.destination);
        source.onended = () => {
          if (this.playing.get(e.ch)?.source === source) this.playing.delete(e.ch);
        };
        source.start();
        pending.started = true;
      },
      (error: unknown) => {
        if (this.playing.get(e.ch) === pending) this.playing.delete(e.ch);
        console.warn(`sound ${e.ch}: ${String(error)}`);
      },
    );
  }
}
