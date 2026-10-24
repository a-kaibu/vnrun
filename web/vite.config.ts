import { defineConfig } from "vite-plus";

// The player is served next to the game protocol (docs/protocol.md): relative URLs, built into
// the repository's dist/. `vp dev` passes /game/ to a running vnrun-devserver.
export default defineConfig({
  base: "./",
  build: {
    outDir: "../dist",
    emptyOutDir: true,
    target: "es2022",
  },
  server: {
    proxy: {
      "/game": process.env.VNRUN_DEVSERVER ?? "http://127.0.0.1:8090",
    },
  },
  lint: {
    ignorePatterns: ["public/**"],
    options: { typeAware: true, typeCheck: true },
  },
  fmt: {
    printWidth: 110,
    ignorePatterns: ["public/**"],
  },
});
