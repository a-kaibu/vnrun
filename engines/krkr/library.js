// Functions the KiriKiri SDL2 build imports from the page (patches/krkrsdl2).
addToLibrary({
  // A frame of the window surface for the page to show. KiriKiri copies its own pixels (B, G, R, A
  // in memory) into it whatever the surface's format says (SDLBitmapCompletion.cpp).
  vnrun_present: (pixels, pitch, width, height) => {
    Module.vnrunPresent(HEAPU8.subarray(pixels, pixels + pitch * height), pitch, width, height);
  },
});
