# Stage 9 — Floating Windows

**What it is.** The first stage of the daily-driver era: windows that
shouldn't tile, don't. File pickers, "Save as…" dialogs, and fixed-size
utility windows get force-tiled today like any other toplevel — the single
most disruptive behavior in day-to-day use of a pure tiler.

**Deliverable** (from `KICKOFF.md`): *a file picker opens floating and
centered instead of being tiled.*

The intended shape: xdg-shell already marks dialog-like toplevels (a set
`parent`, or min/max size hints pinning the window to a fixed size) — those
float automatically, centered over their parent, at their preferred size.
Per-app config rules and a manual float toggle can follow once the
automatic detection covers the common cases.

## Status

**Not started** — next up. This chapter will be filled in once the work
lands, the same way every earlier stage was: after building it, not before.
