# Stage 9 — Floating Windows

**What it is.** The first stage of the daily-driver era: windows that
shouldn't tile, don't. File pickers, "Save as…" dialogs, and fixed-size
utility windows used to get force-tiled like any other toplevel — the single
most disruptive behavior in day-to-day use of a pure tiler.

**Deliverable** (from `KICKOFF.md`): *a file picker opens floating and
centered instead of being tiled.*

## How it went

The key realization: tiled and floating aren't two branches of the layout
code — they're two different **protocol postures**, and the difference shows
up in the very first configure a client ever receives. A tiled window gets
tiled states plus a binding size (Stage 8's fix for browsers overriding
their tile). A floating window gets the exact opposite: no tiled states and
a `0×0` configure ("pick your own size") — the client's natural size is
precisely what floating exists to preserve. That's why float detection runs
on the **initial commit**, not on map: by map time the first configure has
long been answered.

A window floats when any of three signals match, checked in order:

1. **It's a dialog** — the toplevel has a `parent` set (what
   `xdg_toplevel.set_parent` conveys; GTK file pickers, "Save as…" dialogs).
   This is the deliverable case, and it's re-checked on map as a backstop
   for clients that set the parent late.
2. **It declares a fixed size** — committed min and max sizes that are
   equal and nonzero on both axes. Tiling a window that cannot resize only
   stretches or letterboxes it.
3. **A config rule says so** — `float = <app_id>` lines, matched
   case-insensitively (e.g. `float = pavucontrol`).

The first two keep the client's natural size — that's the point of floating
them. Rule windows are different: they're ordinary apps *told* to float, and
an ordinary app's "natural" size is whatever it last remembered — so they
open at the configured default instead: `float_size = 60% x 60%`
(percentages of the usable area; that's also the built-in default).

On map the window is centered in the active output's usable area at the
natural size it just committed, in a new scene layer between the tiled
windows and the layer-shell top layer — floating windows paint above tiles
but never above bars (fullscreen keeps its own, higher layer). The spiral
skips them entirely: `refresh()` and the initial-configure tile prediction
now share one `tiled_windows()` filter, so the predicted tile count and the
placed tile count can't drift apart.

Two follow-on behaviors fell out of testing rather than planning:

- **Oversized clients.** A real GTK file picker remembered a size *taller
  than the output*, so naive centering pushed its header (and its Open
  button) off the top edge. The fix caps the size hint to the usable area
  and clamps the position into it — the hint is non-binding without tiled
  states, but the position clamp guarantees the top-left corner stays
  reachable no matter what size the client insists on.
- **Keyboard moves.** `movewindow` on a floating window has no tile to swap
  with, so it nudges the window 50 px in that direction instead, clamped to
  the usable area. Interactive mouse move/resize needs a pointer-grab state
  machine and is deliberately left for a later pass.

`Mod+V` (`togglefloating`) flips the focused window between the postures:
tiled → floating resizes to the `float_size` default, centered (keeping the
tile's size looked arbitrary in practice — whatever the spiral last
assigned); floating → tiled restores the tiled states so `refresh()`'s
sizes bind again.

Verified nested with logs and screenshots: a tiled kitty, a GTK app whose
file chooser (parent set) mapped floating and centered with its header
visible, and a `float =` rule floating an ordinary terminal by app id.

## Status

**Substantially done.** The gate — a file picker opens floating and
centered — is verified. Interactive mouse move/resize of floating windows is
the one piece deliberately deferred.
