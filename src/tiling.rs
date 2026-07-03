//! The tiling engine: recomputing window and layer-surface layout.

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
        }
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
