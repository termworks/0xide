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

## Status

**Not started.** This is the largest planned rework of the tiling engine —
deliberately queued after floating windows ([Stage 9](stage-9-floating.md)),
which is smaller and independent of the layout representation.
