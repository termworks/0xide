# Stage 10 — Split-Tree Layout

**What it is.** Replacing the flat, list-order-driven spiral layout with an
explicit split tree: every tiled window is a leaf in a tree of
vertical/horizontal splits, each split with its own adjustable ratio.

**Why it matters.** The current layout is a pure function of the window
list's order — simple, reproducible, unit-testable (see
[Design & ideas](../design.md)), but it can't express "make this window a
bit wider" and it's the structural cause of the corner-touch ambiguity in
directional navigation documented in
[Stage 5](stage-5-window-management.md). A real tree fixes both: per-window
resize, and fully reversible neighbor relationships.

**Deliverable** (from `KICKOFF.md`): *resize a window from the keyboard and
the layout keeps it.*

## How it went

The tree (`layout::Node`) is a binary `Leaf`/`Split { vertical, ratio, first,
second }`, one per workspace (`Workspace.tree`), shaped exactly like the old
spiral at the default 0.5 ratio — a parity test checks the two produce
bit-identical rects. The key design choice: leaves carry no payload. Which
window occupies tiled-position *i* is decided entirely by
`tiled_windows(ws)`'s order, the same as it always was; the tree only adds
the one thing that was missing, a persisted `ratio` per split. That
statelessness is what makes `MoveWindow`'s `.swap()` need zero tree code —
swapping two `Vec` entries already reproduces "swap tiling position" without
touching a single `Node`.

Keeping the tree's leaf count exactly matched to `tiled_windows(ws).len()`
turned out to be the real work of the stage: every place a window starts or
stops tiling — map, unmap/destroy, `set_floating`, `set_fullscreen`,
`move_to_workspace` — now calls `tree_track`/`tree_untrack` around the state
flip. `tree_insert_at`/`tree_remove` are pure and ratio-preserving: removing
a window collapses its parent split into whichever sibling remains (ratios
elsewhere untouched), and inserting elsewhere than the end (a window
rejoining the tiled set after a float/fullscreen toggle) walks to the exact
position its `Workspace.windows` order implies. The old spiral survives only
as `spiral_rects`, `#[cfg(test)]`-gated — the reference oracle the tree gets
checked against, no longer called at runtime. The initial-configure size
prediction (Stage 8) now simulates an append on a *clone* of the real tree
(`predict_tile_rect`) instead of recomputing from a count, so a client's
first frame still matches what it actually gets on map.

`tree_resize` is the deliverable itself: from the focused window's leaf,
walk up to the *nearest* ancestor split whose axis matches the pressed
direction (vertical for Left/Right, horizontal for Up/Down) and move that
split's one shared edge — deliberately local, no cascading to a farther
split even when one exists further up. The first attempt made the "wrong"
direction a no-op (reasoning: that edge is the window's own outer boundary,
nothing to push into) — wrong, because it's still the *same* shared edge,
just moved the other way. Fixed so every direction on the matching axis
does something: whichever key grows the focused window, the opposite always
undoes it, on either side of the split. Bound to `Mod+Ctrl+hjkl`.

Not pursued: the "fully reversible neighbor relationships" half of *why it
matters* above. `spatial_neighbor` still uses Stage 5's geometric heuristic,
corner-touch case and all — swapping it for exact tree adjacency is now
possible (the tree has the real structure `spatial_neighbor` was missing)
but wasn't necessary for the resize deliverable, so it's left as a possible
follow-up rather than done here.

Verified with `cargo test` (parity, insert/remove ratio-preservation,
resize's grow/shrink/clamp/locality) and manually on a real TTY session:
opened several windows, resized one with `Mod+Ctrl+L`, opened another window
and confirmed the resized one kept its size, and confirmed the opposite key
(`Mod+Ctrl+H`) shrinks it back.

## Status

**Done.** The gate — resize a window from the keyboard and the layout keeps
it — is verified both by tests and on real hardware.
