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
        let n = ws.windows.len();
        if n == 0 {
            continue;
        }

        // Spiral layout: each window except the last splits the remaining rect,
        // alternating vertical (left/right) then horizontal (top/bottom). The
        // window takes the first half; the rest recurse into the second half.
        let (mut rx, mut ry) = (o.ux + gap, o.uy + gap);
        let (mut rw, mut rh) = ((o.uw - gap * 2).max(1), (o.uh - gap * 2).max(1));
        let mut split_vertical = true;
        for (i, &tl) in ws.windows.iter().enumerate() {
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
            oxide_scene_tree_set_position((*tl).scene_tree, x, y);
            wlr::wlr_xdg_toplevel_set_size((*tl).xdg_toplevel, w, h);
            (*tl).x = x;
            (*tl).y = y;
            (*tl).w = w;
            (*tl).h = h;
        }
    }
}

/// Find whichever window in workspace `ws_idx` is spatially adjacent to
/// `from_idx` in direction `dir` (by their rects as of the last `refresh()`),
/// or `None` if nothing qualifies (no wraparound). Filters to windows whose
/// center lies on the correct side, then picks the closest by a
/// primary-axis-weighted distance so a roughly-aligned neighbor wins over a
/// diagonal one.
pub(crate) unsafe fn spatial_neighbor(
    server: &Server,
    ws_idx: usize,
    from_idx: usize,
    dir: Direction,
) -> Option<usize> {
    let windows = &server.workspaces[ws_idx].windows;
    let center = |tl: *mut Toplevel| ((*tl).x + (*tl).w / 2, (*tl).y + (*tl).h / 2);
    let (fx, fy) = center(windows[from_idx]);

    windows
        .iter()
        .enumerate()
        .filter(|&(i, _)| i != from_idx)
        .filter_map(|(i, &tl)| {
            let (cx, cy) = center(tl);
            let (dx, dy) = (cx - fx, cy - fy);
            let on_side = match dir {
                Direction::Left => dx < 0,
                Direction::Right => dx > 0,
                Direction::Up => dy < 0,
                Direction::Down => dy > 0,
            };
            on_side.then_some((i, dx, dy))
        })
        .min_by_key(|&(_, dx, dy)| match dir {
            Direction::Left | Direction::Right => dx.abs() * 2 + dy.abs(),
            Direction::Up | Direction::Down => dy.abs() * 2 + dx.abs(),
        })
        .map(|(i, ..)| i)
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
            commit_listener: ptr::null_mut(),
            map_listener: ptr::null_mut(),
            unmap_listener: ptr::null_mut(),
            destroy_listener: ptr::null_mut(),
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
