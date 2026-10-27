import assert from "node:assert/strict";
import { test } from "node:test";
import { Sounds } from "../src/advhd-sounds.ts";

test("replacing or stopping a channel while decoding cannot play stale audio", async () => {
  globalThis.window = new EventTarget();
  const decodes = [];
  const sources = [];
  globalThis.AudioContext = class {
    currentTime = 0;
    destination = {};
    resume() {
      return Promise.resolve();
    }
    createBufferSource() {
      const source = {
        started: false,
        stopped: false,
        connect() {
          return this;
        },
        start() {
          this.started = true;
        },
        stop() {
          if (!this.started) throw new Error("not started");
          this.stopped = true;
        },
      };
      sources.push(source);
      return source;
    }
    createGain() {
      return {
        gain: { value: 1, cancelScheduledValues() {}, setValueAtTime() {}, linearRampToValueAtTime() {} },
        connect() {},
      };
    }
    decodeAudioData() {
      return new Promise((resolve) => decodes.push(resolve));
    }
  };
  const sounds = new Sounds();
  const play = { kind: "play", ch: "bgm", data: new Uint8Array([1]), looped: true, volume: 1, fade: 0 };
  sounds.handle(play);
  sounds.handle(play);
  decodes[0]({});
  decodes[1]({});
  await Promise.resolve();
  assert.equal(sources[0].started, false);
  assert.equal(sources[1].started, true);
  sounds.handle({ kind: "stop", ch: "*", fade: 0 });
  assert.equal(sources[1].stopped, true);
  sounds.handle(play);
  sounds.handle({ kind: "stop", ch: "bgm", fade: 0 });
  decodes[2]({});
  await Promise.resolve();
  assert.equal(sources[2].started, false);
});
