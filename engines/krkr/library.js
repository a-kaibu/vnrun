// Functions the KiriKiri SDL2 build imports from the page (patches/krkrsdl2).
addToLibrary({
  // A frame of the window surface (SDL's BGR888: R, G, B, unused in memory) for the page to show.
  vnrun_present: (pixels, pitch, width, height) => {
    Module.vnrunPresent(HEAPU8.subarray(pixels, pixels + pitch * height), pitch, width, height);
  },
});
