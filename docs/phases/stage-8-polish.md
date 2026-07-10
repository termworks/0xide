# Stage 8 — Polish & Compat

**What it is.** The catch-all final stage in `KICKOFF.md`'s roadmap: the
things that make a compositor pleasant and compatible to actually live in,
rather than strictly necessary to tile windows at all.

**Deliverable** (from `KICKOFF.md`): *XWayland, server-side decorations,
layer-shell (bar/wallpaper), animations, screenshot/screencopy.*

## How it's gone so far

Several of these landed early, driven by real need rather than roadmap
order — a bar and a wallpaper are hard to live without day-to-day, so
layer-shell support arrived well before this stage was "next":

- **`wlr-layer-shell-unstable-v1`** — bars, panels, and wallpaper (e.g.
  [quickshell](https://quickshell.org)) render in the correct z-order (into
  the layer trees set up back in [Stage 2](stage-2-outputs-render.md)) and
  reserve their exclusive screen space, so tiled windows never sit
  underneath them. One real bug here: layer surfaces that arrive before any
  output exists yet were being silently dropped; the fix tracks them as
  pending and attaches them to the next output that shows up, instead of
  requiring output-then-surface ordering.
- **Server-side decorations** (`xdg-decoration-unstable-v1`) — 0xide always
  claims decoration mode, so clients skip drawing their own title bar/CSD:
  bare, borderless windows by default.
- **Screenshots/screen recording** (`wlr-screencopy-unstable-v1` +
  `xdg-output`) — tools like `grim` and `wf-recorder` capture 0xide's real
  composited output directly. `xdg-output` specifically exists because
  screenshot tools need to learn each output's logical position/size, or
  `grim` fails with a 0×0 capture.
- **Fullscreen** — both client-requested (F11, `mpv --fs`, honored on map
  for apps launched fullscreen) and compositor-driven (`Mod+F` toggle). A
  fullscreen window covers its output's full box in a dedicated scene layer
  above the bars but below overlay surfaces, and other windows stay tiled
  beneath it. Per the xdg-shell protocol every state request must be
  answered with a configure even when denied — 0xide previously wasn't
  listening at all, which was a protocol violation, not just a missing
  feature. Closely related fix from the same work: windows are declared
  **tiled** in their very first configure, carrying their predicted tile
  size — without a tiled state the configure size is only a floating-style
  hint, and clients with a remembered window size (browsers especially)
  would map at their own size and overflow across outputs.

**Not yet started:** XWayland (X11 app compatibility) and animations.

## Status

**Partially done**, and likely to keep being partially done for a while —
this is explicitly the "as needed" stage rather than one with a single
clean finish line.
