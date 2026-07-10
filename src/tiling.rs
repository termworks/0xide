//! The tiling engine: recomputing window and layer-surface layout.

use crate::config::Direction;
use crate::ffi::*;
use crate::state::*;
use crate::wlr;

/// Recompute the whole picture: hide windows whose workspace isn't on any
/// output, then tile each output's workspace as a spiral (dwindle). Called after
/// any change to windows, workspaces or outputs.
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

        // Fullscreen windows cover the output's full box (over bars — their
        // scene trees live in tree_fullscreen, above layer-shell top); the
        // rest tile in the usable area beneath them as usual.
        let tiled: Vec<*mut Toplevel> =
            ws.windows.iter().copied().filter(|&tl| !(*tl).fullscreen).collect();
        for &tl in ws.windows.iter().filter(|&&tl| (*tl).fullscreen) {
            place(tl, o.x, o.y, o.w, o.h);
        }
        let rects = spiral_rects(tiled.len(), o.ux, o.uy, o.uw, o.uh, gap);
        for (&tl, &(x, y, w, h)) in tiled.iter().zip(&rects) {
            place(tl, x, y, w, h);
        }
    }
}

/// The spiral (dwindle) layout as a pure function: `n` rects filling the
/// `x,y,w,h` box with `gap` pixels around and between windows. Each window
/// except the last splits the remaining rect, alternating vertical
/// (left/right) then horizontal (top/bottom); the window takes the first
/// half, the rest recurse into the second. Pure so the unit tests exercise
/// the exact same geometry `refresh()` applies.
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
            tree_layer_top: ptr::null_mut(),
            tree_fullscreen: ptr::null_mut(),
            tree_layer_overlay: ptr::null_mut(),
            layers: Vec::new(),
            workspaces: vec![Workspace { windows, focused: 0 }],
            outputs: Vec::new(),
            config: Config::default(),
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
