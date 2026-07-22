//! The tiling engine: recomputing window and layer-surface layout.

use crate::config::Direction;
use crate::ffi::*;
use crate::state::*;
use crate::wlr;

/// Recompute the whole picture: hide windows whose workspace isn't on any
/// output, then tile each output's workspace from its split tree. Called
/// after any change to windows, workspaces or outputs.
pub(crate) unsafe fn refresh(server: &mut Server) {
    // A window is visible iff its workspace is currently shown on some output.
    let mut shown = [false; WORKSPACE_COUNT];
    for o in &server.outputs {
        shown[o.workspace] = true;
    }
    for (wi, ws) in server.workspaces.iter().enumerate() {
        for &tl in &ws.windows {
            oxide_scene_tree_set_enabled((*tl).scene_tree, shown[wi]);
        }
    }

    let gap = server.config.gap;
    for o in &server.outputs {
        let ws = &server.workspaces[o.workspace];
        if ws.windows.is_empty() {
            continue;
        }

        let place = |tl: *mut Toplevel, x: i32, y: i32, w: i32, h: i32| {
            oxide_scene_tree_set_position((*tl).scene_tree, x, y);
            wlr::wlr_xdg_toplevel_set_size((*tl).xdg_toplevel, w, h);
            (*tl).x = x;
            (*tl).y = y;
            (*tl).w = w;
            (*tl).h = h;
        };

        // Three kinds of window: fullscreen ones cover the output's full box
        // (over bars — their scene trees live in tree_fullscreen, above
        // layer-shell top); floating ones keep whatever rect they already
        // have (we never place them here — their size is the client's own);
        // the rest tile in the usable area as usual.
        let tiled = tiled_windows(ws);
        for &tl in ws.windows.iter().filter(|&&tl| (*tl).fullscreen) {
            place(tl, o.x, o.y, o.w, o.h);
        }
        let rects = match &ws.tree {
            Some(t) => tree_rects(t, o.ux, o.uy, o.uw, o.uh, gap),
            None => Vec::new(),
        };
        for (&tl, &(x, y, w, h)) in tiled.iter().zip(&rects) {
            place(tl, x, y, w, h);
        }
    }
}

/// The windows of a workspace that are tiled — neither fullscreen nor
/// floating — in stacking order; the same order the split tree's leaves are
/// in. Shared by `refresh()`, `tiled_position`, and the initial-configure
/// tile prediction (`toplevel::handle_commit`), so nothing can drift apart.
pub(crate) unsafe fn tiled_windows(ws: &Workspace) -> Vec<*mut Toplevel> {
    ws.windows.iter().copied().filter(|&tl| !(*tl).fullscreen && !(*tl).floating).collect()
}

/// The original flat-list spiral (dwindle) layout: `n` rects filling the
/// `x,y,w,h` box with `gap` pixels around and between windows. Each window
/// except the last splits the remaining rect, alternating vertical
/// (left/right) then horizontal (top/bottom); the window takes the first
/// half, the rest recurse into the second. `refresh()` no longer calls this
/// (Stage 10 moved it to the split tree below) — it survives as the known-
/// good reference the tree's output is checked against.
#[cfg(test)]
pub(crate) fn spiral_rects(
    n: usize,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    gap: i32,
) -> Vec<(i32, i32, i32, i32)> {
    let (mut rx, mut ry) = (x + gap, y + gap);
    let (mut rw, mut rh) = ((w - gap * 2).max(1), (h - gap * 2).max(1));
    let mut split_vertical = true;
    let mut rects = Vec::with_capacity(n);
    for i in 0..n {
        let (x, y, w, h);
        if i == n - 1 {
            // Last window fills whatever rect is left.
            (x, y, w, h) = (rx, ry, rw, rh);
        } else if split_vertical {
            let half = ((rw - gap) / 2).max(1);
            (x, y, w, h) = (rx, ry, half, rh);
            rx += half + gap;
            rw = (rw - half - gap).max(1);
        } else {
            let half = ((rh - gap) / 2).max(1);
            (x, y, w, h) = (rx, ry, rw, half);
            ry += half + gap;
            rh = (rh - half - gap).max(1);
        }
        split_vertical = !split_vertical;
        rects.push((x, y, w, h));
    }
    rects
}

/// One node of a workspace's split tree: a window (`Leaf`) or a divider
/// between two children. `ratio` is the fraction of the split's box the
/// `first` child gets — the piece of state the flat `Vec`-order spiral had
/// no room for, and the reason this tree exists (Stage 10: per-window
/// resize). Shape mirrors the old spiral's alternating dwindle exactly, so
/// `build_dwindle` + `tree_rects` reproduce it bit-for-bit at the default
/// ratio. Lives on `Workspace.tree`, one per workspace; `Clone` is for
/// `predict_tile_rect`, which simulates an insert without touching the real
/// tree.
#[derive(Clone)]
pub(crate) enum Node {
    Leaf,
    Split { vertical: bool, ratio: f32, first: Box<Node>, second: Box<Node> },
}

/// Build the tree for `n` windows in the same shape `spiral_rects` computes
/// iteratively: window 0 splits off the first half (alternating
/// vertical/horizontal), the rest recurse into the second half, and the
/// last window is a bare leaf that fills whatever's left. `None` for `n == 0`
/// (nothing to tile).
pub(crate) fn build_dwindle(n: usize) -> Option<Node> {
    fn go(n: usize, vertical: bool) -> Node {
        if n <= 1 {
            Node::Leaf
        } else {
            Node::Split { vertical, ratio: 0.5, first: Box::new(Node::Leaf), second: Box::new(go(n - 1, !vertical)) }
        }
    }
    (n > 0).then(|| go(n, true))
}

/// Render a split tree to rects, in the same left-to-right/top-to-bottom
/// in-order as `spiral_rects` — leaf `i` here is window `i` of the same
/// list. Gap semantics match exactly: one `gap` margin around the outer box,
/// one `gap` between each split's two children, none inside a leaf.
pub(crate) fn tree_rects(tree: &Node, x: i32, y: i32, w: i32, h: i32, gap: i32) -> Vec<(i32, i32, i32, i32)> {
    fn go(node: &Node, x: i32, y: i32, w: i32, h: i32, gap: i32, out: &mut Vec<(i32, i32, i32, i32)>) {
        match node {
            Node::Leaf => out.push((x, y, w.max(1), h.max(1))),
            Node::Split { vertical, ratio, first, second } => {
                if *vertical {
                    let fw = (((w - gap) as f32) * ratio) as i32;
                    let (fw, sw) = (fw.max(1), (w - gap - fw).max(1));
                    go(first, x, y, fw, h, gap, out);
                    go(second, x + fw + gap, y, sw, h, gap, out);
                } else {
                    let fh = (((h - gap) as f32) * ratio) as i32;
                    let (fh, sh) = (fh.max(1), (h - gap - fh).max(1));
                    go(first, x, y, w, fh, gap, out);
                    go(second, x, y + fh + gap, w, sh, gap, out);
                }
            }
        }
    }
    let mut out = Vec::with_capacity(tree_leaf_count(tree));
    go(tree, x + gap, y + gap, (w - gap * 2).max(1), (h - gap * 2).max(1), gap, &mut out);
    out
}

/// Number of leaves (windows) a tree holds — the capacity hint for `tree_rects`.
fn tree_leaf_count(tree: &Node) -> usize {
    match tree {
        Node::Leaf => 1,
        Node::Split { first, second, .. } => tree_leaf_count(first) + tree_leaf_count(second),
    }
}

/// Insert a new leaf so it becomes tiled-position `i` (0-indexed, in-order)
/// once inserted. Every split the new leaf doesn't pass through keeps its
/// `ratio` exactly as it was — only the split that gains the new leaf as a
/// child is freshly created, at the default 0.5, same as any leaf
/// `build_dwindle` creates. Passing the tree's current leaf count as `i`
/// appends at the end, which is what a newly mapped window always does; any
/// other `i` is a window rejoining the tiled set (after a float/fullscreen
/// toggle) at whichever position `Workspace.windows` order puts it among the
/// other tiled ones.
pub(crate) fn tree_insert_at(tree: Option<Node>, i: usize) -> Node {
    fn go(node: Node, i: usize, axis: bool) -> Node {
        match node {
            Node::Leaf => {
                Node::Split { vertical: axis, ratio: 0.5, first: Box::new(Node::Leaf), second: Box::new(Node::Leaf) }
            }
            Node::Split { vertical, ratio, first, second } => {
                let first_n = tree_leaf_count(&first);
                if i < first_n {
                    Node::Split { vertical, ratio, first: Box::new(go(*first, i, !vertical)), second }
                } else {
                    Node::Split { vertical, ratio, first, second: Box::new(go(*second, i - first_n, !vertical)) }
                }
            }
        }
    }
    match tree {
        None => Node::Leaf,
        Some(t) => go(t, i, true),
    }
}

/// Remove tiled-position `i` (0-indexed, in-order) from the tree. The
/// removed leaf's sibling — and everything under it, ratios untouched —
/// reclaims the freed space by collapsing the now-empty parent split away.
/// `None` once the last leaf is gone.
pub(crate) fn tree_remove(tree: Option<Node>, i: usize) -> Option<Node> {
    match tree? {
        Node::Leaf => None,
        Node::Split { vertical, ratio, first, second } => {
            let first_n = tree_leaf_count(&first);
            if i < first_n {
                match tree_remove(Some(*first), i) {
                    Some(f) => Some(Node::Split { vertical, ratio, first: Box::new(f), second }),
                    None => Some(*second),
                }
            } else {
                match tree_remove(Some(*second), i - first_n) {
                    Some(s) => Some(Node::Split { vertical, ratio, first, second: Box::new(s) }),
                    None => Some(*first),
                }
            }
        }
    }
}

/// `tl`'s index among its workspace's tiled windows right now — its leaf
/// position in the split tree — or `None` if it's floating or fullscreen.
pub(crate) unsafe fn tiled_position(ws: &Workspace, tl: *mut Toplevel) -> Option<usize> {
    tiled_windows(ws).iter().position(|&w| w == tl)
}

/// Remove `tl`'s leaf from `ws`'s tree. Call *before* flipping whatever flag
/// (`floating`/`fullscreen`) is about to take it out of the tiled set — the
/// lookup needs the old state to still find it.
pub(crate) unsafe fn tree_untrack(ws: &mut Workspace, tl: *mut Toplevel) {
    if let Some(p) = tiled_position(ws, tl) {
        ws.tree = tree_remove(ws.tree.take(), p);
    }
}

/// Insert a leaf for `tl` into `ws`'s tree, at the position its (already
/// updated) tiled state puts it among the workspace's other tiled windows.
/// Call *after* flipping the flag that just made it tiled again.
pub(crate) unsafe fn tree_track(ws: &mut Workspace, tl: *mut Toplevel) {
    if let Some(p) = tiled_position(ws, tl) {
        ws.tree = Some(tree_insert_at(ws.tree.take(), p));
    }
}

/// Which workspace currently holds `tl`, if any.
pub(crate) unsafe fn workspace_of(server: &Server, tl: *mut Toplevel) -> Option<usize> {
    server.workspaces.iter().position(|ws| ws.windows.contains(&tl))
}

/// The rect a new tiled window would get if it mapped onto `ws` right now:
/// simulates the append on a clone of the tree, leaving the real one
/// untouched, so the very first configure a client gets already matches the
/// size it will actually receive once it maps (Stage 8: avoids a resize jump
/// on the client's first frame — see `toplevel::handle_commit`).
pub(crate) fn predict_tile_rect(ws: &Workspace, x: i32, y: i32, w: i32, h: i32, gap: i32) -> (i32, i32, i32, i32) {
    let n = ws.tree.as_ref().map_or(0, tree_leaf_count);
    let candidate = tree_insert_at(ws.tree.clone(), n);
    *tree_rects(&candidate, x, y, w, h, gap).last().unwrap()
}

/// Ratio bounds a resize can't push past — keeps either side of a split from
/// collapsing away under repeated presses.
const MIN_RATIO: f32 = 0.1;
const MAX_RATIO: f32 = 0.9;

/// Resize tiled-position `i`'s window by `delta` in direction `dir`: walk up
/// from its leaf to the *nearest* ancestor split whose axis matches `dir`
/// (vertical for Left/Right, horizontal for Up/Down) — deeper splits along
/// the other axis don't affect this edge at all, so the first matching one
/// found is the one actually bordering the window in that direction. Only
/// one of that split's two edges is adjustable from a leaf's side of it (the
/// other is the tree's own outer boundary); pressing toward the boundary
/// edge is a no-op, the same "no wraparound" rule `spatial_neighbor` uses at
/// the edge of the layout — deliberately local, no cascading to a farther
/// split even if one exists further up.
pub(crate) fn tree_resize(tree: &mut Node, i: usize, dir: Direction, delta: f32) {
    fn go(node: &mut Node, i: usize, dir: Direction, delta: f32) -> bool {
        let Node::Split { vertical, ratio, first, second } = node else { return false };
        let first_n = tree_leaf_count(first);
        let (child, in_first, child_i) =
            if i < first_n { (first.as_mut(), true, i) } else { (second.as_mut(), false, i - first_n) };
        if go(child, child_i, dir, delta) {
            return true; // a nearer matching-axis split already handled this
        }
        let axis_matches = matches!(
            (*vertical, dir),
            (true, Direction::Left | Direction::Right) | (false, Direction::Up | Direction::Down)
        );
        if !axis_matches {
            return false; // keep looking further up for the right axis
        }
        // first is left/top, second is right/bottom (tree_rects' layout) — so
        // growing "into" the shared edge means: first grows on Right/Down,
        // second grows on Left/Up. The opposite direction from either side
        // has no adjustable edge here — a genuine no-op, not a search miss.
        match (in_first, matches!(dir, Direction::Right | Direction::Down)) {
            (true, true) => *ratio = (*ratio + delta).clamp(MIN_RATIO, MAX_RATIO),
            (false, false) => *ratio = (*ratio - delta).clamp(MIN_RATIO, MAX_RATIO),
            _ => {}
        }
        true // this was the nearest matching-axis split either way — stop here
    }
    go(tree, i, dir, delta);
}

/// Find whichever window in workspace `ws_idx` is spatially adjacent to
/// `from_idx` in direction `dir` (by their rects as of the last `refresh()`),
/// or `None` if nothing qualifies (no wraparound).
///
/// Filters to windows whose center lies on the correct side, then — like
/// i3/sway's directional focus — prefers whichever candidate shares the most
/// overlapping border with the focused window on the axis perpendicular to
/// `dir` (most overlap wins; primary-axis gap breaks ties). That's a much
/// stronger signal for "the window actually next to me" than raw
/// center-to-center distance: the dwindle spiral often puts one window
/// spanning much more area than its neighbors, and center-distance alone can
/// pick a window that doesn't really border the focused one, in a way that
/// isn't even reversible (A's right neighbor being B doesn't imply B's left
/// neighbor is A). Falls back to raw center-distance only when no candidate
/// has any border overlap at all (e.g. windows that meet only at a corner).
pub(crate) unsafe fn spatial_neighbor(
    server: &Server,
    ws_idx: usize,
    from_idx: usize,
    dir: Direction,
) -> Option<usize> {
    let windows = &server.workspaces[ws_idx].windows;
    let rect = |tl: *mut Toplevel| ((*tl).x, (*tl).y, (*tl).w, (*tl).h);
    let (fx, fy, fw, fh) = rect(windows[from_idx]);
    let (fcx, fcy) = (fx + fw / 2, fy + fh / 2);

    let candidates: Vec<(usize, i32, i32)> = windows
        .iter()
        .enumerate()
        .filter(|&(i, _)| i != from_idx)
        .filter_map(|(i, &tl)| {
            let (cx, cy, cw, ch) = rect(tl);
            let (ccx, ccy) = (cx + cw / 2, cy + ch / 2);
            let (dx, dy) = (ccx - fcx, ccy - fcy);
            let on_side = match dir {
                Direction::Left => dx < 0,
                Direction::Right => dx > 0,
                Direction::Up => dy < 0,
                Direction::Down => dy > 0,
            };
            if !on_side {
                return None;
            }
            let overlap = match dir {
                Direction::Left | Direction::Right => (fy + fh).min(cy + ch) - fy.max(cy),
                Direction::Up | Direction::Down => (fx + fw).min(cx + cw) - fx.max(cx),
            }
            .max(0);
            let gap = match dir {
                Direction::Left | Direction::Right => dx.abs(),
                Direction::Up | Direction::Down => dy.abs(),
            };
            Some((i, overlap, gap))
        })
        .collect();

    if candidates.iter().any(|&(_, overlap, _)| overlap > 0) {
        candidates.into_iter().max_by_key(|&(_, overlap, gap)| (overlap, -gap)).map(|(i, ..)| i)
    } else {
        candidates.into_iter().min_by_key(|&(_, _, gap)| gap).map(|(i, ..)| i)
    }
}

/// Recompute one output's layer-shell placement: walk its layer surfaces in
/// background -> overlay order, positioning each per its anchors/margins and
/// shrinking a running "usable" box by any exclusive zone. Stores the result
/// on the `Output` so `refresh()` can tile app windows within it. Called after
/// any layer-surface commit/map/unmap/destroy on that output.
pub(crate) unsafe fn arrange_layers(server: &mut Server, output_idx: usize) {
    let o = &server.outputs[output_idx];
    let (fx, fy, fw, fh) = (o.x, o.y, o.w, o.h);
    let wlr_output = o.wlr_output;
    let (mut ux, mut uy, mut uw, mut uh) = (fx, fy, fw, fh);

    for layer in 0u32..=3 {
        for &ls in &server.layers {
            let l = &*ls;
            if l.wlr_output != wlr_output || oxide_layer_surface_layer(l.wlr_layer_surface) != layer {
                continue;
            }
            oxide_scene_layer_surface_configure(
                l.scene_ls, fx, fy, fw, fh, &mut ux, &mut uy, &mut uw, &mut uh,
            );
        }
    }

    let o = &mut server.outputs[output_idx];
    (o.ux, o.uy, o.uw, o.uh) = (ux, uy, uw, uh);
}

/// The output the cursor is currently on (the target for new windows and
/// workspace switches). Falls back to output 0 if the cursor is off all outputs.
pub(crate) unsafe fn active_output(server: &Server) -> usize {
    let out = oxide_output_at_cursor(server.cursor, server.output_layout);
    server.outputs.iter().position(|o| o.wlr_output == out).unwrap_or(0)
}

/// The workspace currently displayed on the active (cursor's) output.
pub(crate) unsafe fn active_workspace(server: &Server) -> usize {
    server.outputs[active_output(server)].workspace
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::ptr;

    unsafe fn server_from_rects(rects: &[(i32, i32, i32, i32)]) -> Server {
        let windows = rects.iter().map(|&(x, y, w, h)| toplevel_at(x, y, w, h)).collect();
        server_with(windows)
    }

    // The bug this test guards against: at 4+ windows the spiral produces one
    // window (W2) whose only positive-overlap neighbor above it is W1, but the
    // old center-distance heuristic picked W0 instead (W0 spans the full
    // output height, so its center can be "closer" even with zero shared
    // border) — not reversible with W1's own Down pick. Confirmed via a
    // throwaway diagnostic dump of every window/direction pair at n=2..6
    // before writing this fix.
    #[test]
    fn spatial_neighbor_prefers_overlapping_border_at_4_windows() {
        unsafe {
            let rects = spiral_rects(4, 0, 0, 1280, 720, 0);
            let server = server_from_rects(&rects);
            assert_eq!(spatial_neighbor(&server, 0, 2, Direction::Up), Some(1));
            assert_eq!(spatial_neighbor(&server, 0, 1, Direction::Down), Some(3));
            for &tl in &server.workspaces[0].windows {
                drop(Box::from_raw(tl));
            }
        }
    }

    // Known, accepted limitation: the dwindle spiral can put two windows that
    // only touch at a single point (a "corner"), not a shared border — no
    // geometric heuristic makes that reversible, since neither window is
    // really "beside" the other. W1 and W3 meet only at (1280, 360) here, so
    // W1's Right neighbor (only candidate: W3) doesn't imply W3's Left
    // neighbor is W1 (it has real overlapping-border candidates, W0 and W2,
    // and correctly prefers one of those instead). Documented so a future
    // change to this heuristic doesn't have to silently re-discover this.
    #[test]
    fn spatial_neighbor_corner_touch_is_not_reversible() {
        unsafe {
            let rects = spiral_rects(4, 0, 0, 1280, 720, 0);
            let server = server_from_rects(&rects);
            assert_eq!(spatial_neighbor(&server, 0, 1, Direction::Right), Some(3));
            assert_ne!(spatial_neighbor(&server, 0, 3, Direction::Left), Some(1));
            for &tl in &server.workspaces[0].windows {
                drop(Box::from_raw(tl));
            }
        }
    }

    // spatial_neighbor only reads Toplevel.{x,y,w,h} and Server.workspaces, so
    // every other field can be a dangling/null placeholder for this test.
    unsafe fn toplevel_at(x: i32, y: i32, w: i32, h: i32) -> *mut Toplevel {
        Box::into_raw(Box::new(Toplevel {
            server: ptr::null_mut(),
            xdg_toplevel: ptr::null_mut(),
            scene_tree: ptr::null_mut(),
            x,
            y,
            w,
            h,
            fullscreen: false,
            floating: false,
            commit_listener: ptr::null_mut(),
            map_listener: ptr::null_mut(),
            unmap_listener: ptr::null_mut(),
            destroy_listener: ptr::null_mut(),
            fullscreen_listener: ptr::null_mut(),
        }))
    }

    unsafe fn server_with(windows: Vec<*mut Toplevel>) -> Server {
        Server {
            display: ptr::null_mut(),
            session: ptr::null_mut(),
            scene: ptr::null_mut(),
            output_layout: ptr::null_mut(),
            scene_layout: ptr::null_mut(),
            seat: ptr::null_mut(),
            cursor: ptr::null_mut(),
            renderer: ptr::null_mut(),
            allocator: ptr::null_mut(),
            tree_bg_fallback: ptr::null_mut(),
            tree_layer_bg: ptr::null_mut(),
            tree_layer_bottom: ptr::null_mut(),
            tree_normal: ptr::null_mut(),
            tree_floating: ptr::null_mut(),
            tree_layer_top: ptr::null_mut(),
            tree_fullscreen: ptr::null_mut(),
            tree_layer_overlay: ptr::null_mut(),
            layers: Vec::new(),
            workspaces: vec![Workspace { windows, focused: 0, tree: None }],
            outputs: Vec::new(),
            config: Config::default(),
            grab: GrabMode::None,
            grab_tl: ptr::null_mut(),
            grab_cx: 0.0,
            grab_cy: 0.0,
            grab_x: 0,
            grab_y: 0,
            grab_w: 0,
            grab_h: 0,
        }
    }

    // Floating and fullscreen windows must never join the spiral: both
    // refresh() and the initial-configure tile prediction count windows
    // through this one function, so this pins the partition rule itself.
    #[test]
    fn tiled_windows_excludes_floating_and_fullscreen() {
        unsafe {
            let windows = vec![
                toplevel_at(0, 0, 100, 100),
                toplevel_at(100, 0, 100, 100),
                toplevel_at(0, 100, 100, 100),
            ];
            let (floater, fuller) = (windows[1], windows[2]);
            (*floater).floating = true;
            (*fuller).fullscreen = true;
            let server = server_with(windows);

            let ws = &server.workspaces[0];
            assert_eq!(tiled_windows(ws), vec![ws.windows[0]]);

            for &tl in &server.workspaces[0].windows {
                drop(Box::from_raw(tl));
            }
        }
    }

    #[test]
    fn spatial_neighbor_2x2_grid() {
        unsafe {
            // top-left(0) top-right(1)
            // bot-left(2) bot-right(3)
            let windows = vec![
                toplevel_at(0, 0, 100, 100),
                toplevel_at(100, 0, 100, 100),
                toplevel_at(0, 100, 100, 100),
                toplevel_at(100, 100, 100, 100),
            ];
            let server = server_with(windows);

            assert_eq!(spatial_neighbor(&server, 0, 0, Direction::Right), Some(1));
            assert_eq!(spatial_neighbor(&server, 0, 0, Direction::Down), Some(2));
            assert_eq!(spatial_neighbor(&server, 0, 3, Direction::Left), Some(2));
            assert_eq!(spatial_neighbor(&server, 0, 3, Direction::Up), Some(1));
            // No neighbor further right/up from the top-right window.
            assert_eq!(spatial_neighbor(&server, 0, 1, Direction::Right), None);
            assert_eq!(spatial_neighbor(&server, 0, 1, Direction::Up), None);

            for &tl in &server.workspaces[0].windows {
                drop(Box::from_raw(tl));
            }
        }
    }

    // The tree must reproduce today's spiral exactly at the default ratio —
    // this is what lets Stage 10 land without changing anything a user can
    // see until resize (a later step) actually sets a non-0.5 ratio.
    #[test]
    fn dwindle_tree_matches_spiral_rects() {
        for n in 0..=8 {
            let spiral = spiral_rects(n, 10, 20, 1280, 720, 3);
            let tree = match build_dwindle(n) {
                Some(t) => tree_rects(&t, 10, 20, 1280, 720, 3),
                None => Vec::new(),
            };
            assert_eq!(tree, spiral, "mismatch at n={n}");
        }
    }

    // The real insert path (append one at a time, as every window map does)
    // must land on the exact same shape `build_dwindle` computes in one shot
    // — otherwise a workspace built window-by-window would tile differently
    // than the parity test above assumes.
    #[test]
    fn tree_insert_at_end_matches_build_dwindle() {
        let mut tree: Option<Node> = None;
        for n in 1..=8 {
            let count_before = tree.as_ref().map_or(0, tree_leaf_count);
            tree = Some(tree_insert_at(tree, count_before));
            let expected = build_dwindle(n).unwrap();
            assert_eq!(
                tree_rects(tree.as_ref().unwrap(), 0, 0, 1280, 720, 4),
                tree_rects(&expected, 0, 0, 1280, 720, 4),
                "mismatch at n={n}"
            );
        }
    }

    // The whole point of the tree over the old spiral: removing one window
    // must not disturb another split's ratio. A|[B|C] with B|C customized to
    // 0.7 — removing A must leave B|C exactly as it was.
    #[test]
    fn tree_remove_collapses_sibling_and_preserves_other_ratios() {
        let tree = Node::Split {
            vertical: true,
            ratio: 0.5,
            first: Box::new(Node::Leaf),
            second: Box::new(Node::Split {
                vertical: false,
                ratio: 0.7,
                first: Box::new(Node::Leaf),
                second: Box::new(Node::Leaf),
            }),
        };
        match tree_remove(Some(tree), 0).unwrap() {
            Node::Split { vertical, ratio, .. } => {
                assert!(!vertical);
                assert_eq!(ratio, 0.7);
            }
            Node::Leaf => panic!("expected the B|C split to survive removal of A"),
        }
    }

    #[test]
    fn tree_remove_last_leaf_empties_the_tree() {
        assert!(tree_remove(Some(Node::Leaf), 0).is_none());
    }

    // A|B, vertical split: A is first(left), B is second(right). Growing A
    // rightward (into the shared edge) and growing B leftward (same edge,
    // opposite side) both move the ratio the same direction it takes to
    // widen whichever one is asking; the outward edges (A-Left, B-Right) are
    // the layout's own boundary and must no-op.
    #[test]
    fn tree_resize_vertical_split_adjustable_edge_only() {
        let mut tree =
            Node::Split { vertical: true, ratio: 0.5, first: Box::new(Node::Leaf), second: Box::new(Node::Leaf) };

        // 0.25 (not the real RESIZE_STEP) because it's exactly representable
        // in binary floating point, so the assertions below can compare
        // against literals without worrying about rounding drift.
        tree_resize(&mut tree, 0, Direction::Right, 0.25); // A grows right: ratio up
        assert_ratio(&tree, 0.75);
        tree_resize(&mut tree, 0, Direction::Left, 0.25); // A's left edge is outer: no-op
        assert_ratio(&tree, 0.75);
        tree_resize(&mut tree, 1, Direction::Left, 0.25); // B grows left: ratio down
        assert_ratio(&tree, 0.5);
        tree_resize(&mut tree, 1, Direction::Right, 0.25); // B's right edge is outer: no-op
        assert_ratio(&tree, 0.5);
    }

    #[test]
    fn tree_resize_horizontal_split_adjustable_edge_only() {
        let mut tree =
            Node::Split { vertical: false, ratio: 0.5, first: Box::new(Node::Leaf), second: Box::new(Node::Leaf) };

        tree_resize(&mut tree, 0, Direction::Down, 0.25); // top grows down: ratio up
        assert_ratio(&tree, 0.75);
        tree_resize(&mut tree, 0, Direction::Up, 0.25); // top's own edge is outer: no-op
        assert_ratio(&tree, 0.75);
        tree_resize(&mut tree, 1, Direction::Up, 0.25); // bottom grows up: ratio down
        assert_ratio(&tree, 0.5);
    }

    #[test]
    fn tree_resize_clamps_at_bounds() {
        let mut tree =
            Node::Split { vertical: true, ratio: 0.5, first: Box::new(Node::Leaf), second: Box::new(Node::Leaf) };
        for _ in 0..20 {
            tree_resize(&mut tree, 0, Direction::Right, 0.1);
        }
        assert_ratio(&tree, MAX_RATIO);
    }

    // A|[B|C]: resizing B (the nearer, inner vertical split) must leave the
    // outer A|[B|C] split's ratio untouched — locality, not cascading.
    #[test]
    fn tree_resize_only_touches_the_nearest_matching_split() {
        let mut tree = Node::Split {
            vertical: true,
            ratio: 0.5,
            first: Box::new(Node::Leaf), // A, leaf 0
            second: Box::new(Node::Split {
                vertical: true,
                ratio: 0.5,
                first: Box::new(Node::Leaf),  // B, leaf 1
                second: Box::new(Node::Leaf), // C, leaf 2
            }),
        };
        tree_resize(&mut tree, 1, Direction::Right, 0.25); // grow B into C
        match &tree {
            Node::Split { ratio, second, .. } => {
                assert_eq!(*ratio, 0.5, "outer A|[B|C] ratio must be untouched");
                assert_ratio(second, 0.75);
            }
            Node::Leaf => panic!("expected a split"),
        }
    }

    fn assert_ratio(node: &Node, expected: f32) {
        match node {
            Node::Split { ratio, .. } => assert_eq!(*ratio, expected),
            Node::Leaf => panic!("expected a split, got a leaf"),
        }
    }

    #[test]
    fn spatial_neighbor_prefers_aligned_over_diagonal() {
        unsafe {
            // focused(0) at left; a slightly-offset-down neighbor(1) directly
            // right, and a far-diagonal neighbor(2) — same primary distance
            // but larger perpendicular offset. Right should pick (1).
            let windows = vec![
                toplevel_at(0, 0, 100, 100),
                toplevel_at(100, 10, 100, 100),
                toplevel_at(100, 500, 100, 100),
            ];
            let server = server_with(windows);
            assert_eq!(spatial_neighbor(&server, 0, 0, Direction::Right), Some(1));

            for &tl in &server.workspaces[0].windows {
                drop(Box::from_raw(tl));
            }
        }
    }
}
