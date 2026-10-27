import assert from "node:assert/strict";
import { test } from "node:test";
import { GameFS } from "../src/gamefs.ts";

test("flush reports failed saves and retries their latest state", async () => {
  globalThis.window = new EventTarget();
  const attempts = [];
  let available = false;
  globalThis.fetch = async (_url, options) => {
    attempts.push({ method: options.method, body: options.body });
    return new Response(null, { status: available ? 204 : 500 });
  };
  const fs = new GameFS("/game", { engine: "test" });
  fs.write("slot.dat", new Uint8Array([1]));
  await assert.rejects(fs.flush(), /slot.dat/);
  assert.equal(attempts.length, 2);
  available = true;
  await fs.flush();
  assert.deepEqual([...attempts.at(-1).body], [1]);
  fs.write("slot.dat", new Uint8Array([2]));
  await fs.flush();
  assert.deepEqual([...attempts.at(-1).body], [2]);
  assert.equal(attempts.length, 4);
});

test("deleting an absent remote save clears an earlier failed write", async () => {
  globalThis.window = new EventTarget();
  globalThis.fetch = async (_url, options) =>
    new Response(null, { status: options.method === "DELETE" ? 404 : 500 });
  const fs = new GameFS("/game", { engine: "test" });
  fs.write("slot.dat", new Uint8Array([1]));
  fs.remove("slot.dat");
  await fs.flush();
});
