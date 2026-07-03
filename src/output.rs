//! Output (monitor) lifecycle: creation, destroy, VT-resume repaint, framing.

use crate::ffi::*;
use crate::state::*;
use crate::tiling::refresh;
use crate::wlr;
use std::os::raw::c_void;

/// Called by the shim when the backend produces an output (one window, here).
pub(crate) unsafe extern "C" fn handle_new_output(userdata: *mut c_void, data: *mut c_void) {
    let server = &mut *(userdata as *mut Server);
    let output = data as *mut wlr::wlr_output;

    // Give the output our renderer + allocator so it can produce buffers, then
    // enable it (the shim owns the wlr_output_state dance).
    wlr::wlr_output_init_render(output, server.allocator, server.renderer);
    oxide_output_enable(output);

    // Place the output in the layout (auto = to the right of existing ones), and
    // tie that layout slot to a scene output so the scene knows where this
    // output sits and what to repaint.
    let layout_output = wlr::wlr_output_layout_add_auto(server.output_layout, output);
    let scene_output = wlr::wlr_scene_output_create(server.scene, output);
    wlr::wlr_scene_output_layout_add_output(server.scene_layout, layout_output, scene_output);

    // Read this output's box (position + size) in layout coords for tiling.
    let (mut x, mut y, mut w, mut h) = (0, 0, 0, 0);
    oxide_output_layout_get_box(server.output_layout, output, &mut x, &mut y, &mut w, &mut h);

    // Give the output the lowest-numbered workspace not already on a monitor.
    let mut workspace = 0;
    for cand in 0..WORKSPACE_COUNT {
        if !server.outputs.iter().any(|o| o.workspace == cand) {
            workspace = cand;
            break;
        }
    }
    // Background node for this output, placed at its layout origin.
    let (r, g, b) = server.config.background;
    let background =
        oxide_scene_add_output_background(server.tree_bg_fallback, output, x, y, r, g, b);

    // Render through the scene on every frame. The frame callback needs to find
    // this output (for repaint_frames), so hand it a heap FrameCtx. Track the
    // frame + destroy listeners so we can remove them when the output dies.
    let frame_ctx = Box::into_raw(Box::new(FrameCtx {
        server: userdata as *mut Server,
        scene_output,
        wlr_output: output,
    }));
    let frame_listener = oxide_output_add_frame(output, handle_frame, frame_ctx as *mut c_void);
    let destroy_listener = oxide_output_add_destroy(output, handle_output_destroy, userdata);

    server.outputs.push(Output {
        wlr_output: output,
        x,
        y,
        w,
        h,
        // No layer surfaces yet; usable area starts as the full box.
        ux: x,
        uy: y,
        uw: w,
        uh: h,
        workspace,
        frame_listener,
        destroy_listener,
        background,
        frame_ctx,
        repaint_frames: REPAINT_FRAMES,
    });

    refresh(server); // tile any windows already belonging to this workspace

    // Kick the first paint via a scheduled frame rather than rendering now: the
    // output may not be ready this instant (esp. on VT resume). The frame handler
    // forces a full repaint for the first few frames, so idle windows reappear.
    oxide_output_schedule_frame(output);

    eprintln!(
        "0xide: output online @ {x},{y} {w}x{h} — workspace {}",
        workspace + 1
    );
}

/// An output was removed (monitor unplugged, or logind disabled the seat on a VT
/// switch). Remove its listeners + background before wlroots finishes it (else it
/// asserts a non-empty frame listener list), then drop it from our list.
unsafe extern "C" fn handle_output_destroy(userdata: *mut c_void, data: *mut c_void) {
    let server = &mut *(userdata as *mut Server);
    let output = data as *mut wlr::wlr_output;
    let Some(pos) = server.outputs.iter().position(|o| o.wlr_output == output) else {
        return;
    };
    let o = &server.outputs[pos];
    // Remove the frame listener first (so no more frame callbacks), then it's
    // safe to free the FrameCtx it referenced.
    oxide_listener_remove(o.frame_listener);
    oxide_listener_remove(o.destroy_listener);
    oxide_scene_rect_destroy(o.background);
    let frame_ctx = o.frame_ctx;
    server.outputs.remove(pos);
    drop(Box::from_raw(frame_ctx));
    refresh(server);
    eprintln!("0xide: output removed — {} left", server.outputs.len());
}

/// Called by the shim on every session active change (VT switch away/back).
/// On a VT switch the outputs aren't destroyed — they're re-modeset to black —
/// and idle clients never redraw, so on regaining the VT we schedule a frame on
/// each output to force a full repaint (the post-modeset swapchain is fresh, so
/// the scene paints everything, not just damaged regions).
pub(crate) unsafe extern "C" fn handle_session_active(userdata: *mut c_void, _data: *mut c_void) {
    let server = &mut *(userdata as *mut Server);
    if !oxide_session_is_active(server.session) {
        eprintln!("0xide: session inactive (VT switched away)");
        return;
    }
    // Rebuild every window's scene node. After the outputs are torn down and
    // recreated on a VT switch, the original scene nodes stop presenting their
    // surfaces (the client still has a valid buffer — confirmed — but the node
    // never draws it). Recreating the node, exactly like a freshly-mapped
    // window, makes it present the surface's current buffer again.
    for ws in &mut server.workspaces {
        for &tl in &ws.windows {
            oxide_scene_tree_destroy((*tl).scene_tree);
            (*tl).scene_tree = oxide_scene_add_xdg_toplevel(server.tree_normal, (*tl).xdg_toplevel);
        }
    }

    // Re-tile (enable + position + size the fresh nodes) and arm a few forced
    // repaints per output so the rebuilt scene is presented once the output is
    // back (handle_frame only runs after the async resume modeset).
    refresh(server);
    for o in &mut server.outputs {
        o.repaint_frames = REPAINT_FRAMES;
        oxide_output_schedule_frame(o.wlr_output);
    }
    eprintln!(
        "0xide: session active — repainting {} output(s)",
        server.outputs.len()
    );
}

/// Called by the shim each time the output is ready for a new frame. For the
/// first few frames after the output comes up / the VT resumes, force a full
/// repaint (toggle the full-screen background to damage the whole output) so
/// idle windows — which generate no damage of their own — are re-presented.
unsafe extern "C" fn handle_frame(userdata: *mut c_void, _data: *mut c_void) {
    let ctx = &*(userdata as *mut FrameCtx);
    let server = &mut *ctx.server;
    if let Some(pos) = server.outputs.iter().position(|o| o.wlr_output == ctx.wlr_output) {
        if server.outputs[pos].repaint_frames > 0 {
            let bg = server.outputs[pos].background;
            oxide_scene_rect_set_enabled(bg, false);
            oxide_scene_rect_set_enabled(bg, true);
            server.outputs[pos].repaint_frames -= 1;
            eprintln!(
                "0xide: forced repaint (output {}, {} left)",
                pos, server.outputs[pos].repaint_frames
            );
            oxide_scene_output_render(ctx.scene_output);
            // Keep the loop alive until we've forced the full set of repaints.
            if server.outputs[pos].repaint_frames > 0 {
                oxide_output_schedule_frame(ctx.wlr_output);
            }
            return;
        }
    }
    oxide_scene_output_render(ctx.scene_output);
}
