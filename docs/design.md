# Design & ideas

0xide is a dynamic tiling window manager for Wayland, written in Rust on
wlroots. This chapter documents its design decisions — how the layout,
configuration, and workspace model work and why — and the ideas planned
next. It is updated as decisions are made.

## Layout

There is one tiling layout — the spiral/dwindle described in
[Stage 5](phases/stage-5-window-management.md) — and it is **recomputed from
the window list's order on every change**. There is no persisted layout
tree, no per-window split ratios, no manual layout mode.

The tradeoff: layout is a pure function of a `Vec` — reproducible, unit-
testable against exact computed rectangles, with no layout state to corrupt
or desynchronize. The cost is that some layout-shape questions have no clean
answer; the corner-touch ambiguity in directional navigation is a direct
consequence, and is documented with a test that pins the behavior. If
per-window split control becomes a requirement, the structural fix is an
explicit split-tree — listed under planned ideas below.

## Configuration

Three rules, applied throughout `src/config.rs`:

1. **Nothing is fatal.** A line that doesn't parse warns on stderr and is
   skipped. A missing config file means defaults. A config with zero `bind`
   lines still has every default binding. 0xide always starts.
2. **User config merges, never replaces.** A `bind` line overrides exactly
   that key combination; every unmentioned default stays active. A two-line
   config is a two-line diff, not a fork of the whole keymap.
3. **Explicit over implicit.** Monitor placement is literal pixel
   coordinates per named connector (`monitor = eDP-1, 0x0`) — no relative
   keywords, no DPI auto-scale heuristics. The config states what happens;
   nothing else does.

## Workspaces and outputs

Nine workspaces, with one invariant: **a workspace is never visible on two
outputs at once**. Switching to a workspace already shown on another monitor
swaps the two monitors' workspaces instead of duplicating it. New windows
open on the monitor the cursor is on (focus-follows-monitor). The model
stays predictable regardless of how many outputs are attached.

## Decorations

0xide always claims server-side decoration and draws nothing in its place:
every window is a bare, borderless rectangle. In a tiler the layout itself
conveys what title bars and borders would — window position and focus are
already visible from the arrangement.

## Planned ideas

Under consideration, not committed:

- **An explicit split-tree layout** — per-window split ratios, interactive
  resize, and fully reversible directional navigation; the structural answer
  to the corner-touch limitation above.
- **Floating exceptions** — per-window rules for clients that shouldn't
  tile (pickers, dialogs).
- **Runtime control** — a socket/IPC for querying and scripting the
  compositor without keybindings.

When one of these lands, it moves out of this list and into a stage chapter.
