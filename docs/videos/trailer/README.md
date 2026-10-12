# The trailer

`site/tasq-trailer.mp4` is made here, from real takes of the app:

1. `rec.py` runs tasq in tmux on the demo list (`seed.sh`, which sources
   `docs/videos/setup.sh`) and records each take in `script.py` as terminal
   frames on a virtual timeline → `frames/*.ansi` and `scenes.json`.
   `python3 rec.py themes` re-records one take.
2. `cells.py` turns each frame into HTML (a cell grid, JetBrains Mono and
   Symbols Nerd Font), `render.js` into a 2× PNG.
3. `trailer.html` is the edit: captions, camera moves (zooms on a point of a
   take), crossfades, the title and end cards; `make.js` renders it frame by
   frame at 30 fps into the MP4 with ffmpeg (`--stills 6000,12500` for
   single frames to check).

The "connected" Apple Calendar in the settings take is a local CalDAV
server (Radicale on 127.0.0.1:5232, user `you@icloud.com`); `seed.sh`
points tasq at it. Paths to fonts and the Playwright install are the ones
of the box it was made on; adjust them in `cells.py` and the `.js` files.
