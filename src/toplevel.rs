//! xdg-shell application windows: creation, map/unmap/destroy, removal.

use crate::ffi::*;
use crate::keybindings::focus_index;
use crate::state::*;
use crate::tiling::{active_output, active_workspace, refresh, spiral_rects};
use crate::wlr;
use std::os::raw::c_void;
use std::ptr;

/// Called by the shim when a client creates an application window (toplevel).
pub(crate) unsafe extern "C" fn handle_new_toplevel(userdata: *mut c_void, data: *mut c_void) {
    let server = userdata as *mut Server;
    let toplevel = data as *mut wlr::wlr_xdg_toplevel;

    // Give it a scene node, then track it in Rust. We don't add it to the
    // layout yet — that happens on map, when it actually has content.
    let scene_tree = oxide_scene_add_xdg_toplevel((*server).tree_normal, toplevel);
    let tl = Box::into_raw(Box::new(Toplevel {
        server,
        xdg_toplevel: toplevel,
        scene_tree,
        x: 0,
        y: 0,
        w: 0,
        h: 0,
        fullscreen: false,
        commit_listener: ptr::null_mut(),
        map_listener: ptr::null_mut(),
        unmap_listener: ptr::null_mut(),
        destroy_listener: ptr::null_mut(),
        fullscreen_listener: ptr::null_mut(),
    }));

    // Listen for its lifecycle so Rust can keep the window list current. We keep
    // the listener handles to unregister them on destroy.
    let ud = tl as *mut c_void;
    (*tl).commit_listener = oxide_xdg_add_commit(toplevel, handle_commit, ud);
    (*tl).map_listener = oxide_xdg_add_map(toplevel, handle_map, ud);
    (*tl).unmap_listener = oxide_xdg_add_unmap(toplevel, handle_unmap, ud);
    (*tl).destroy_listener = oxide_xdg_add_destroy(toplevel, handle_destroy, ud);
    (*tl).fullscreen_listener =
        oxide_xdg_add_request_fullscreen(toplevel, handle_request_fullscreen, ud);
}

/// Put a window into or out of fullscreen: full output box, painted above
/// layer-shell bars (the `tree_fullscreen` scene layer). Also answers the
/// client — the protocol requires every state request to get a configure,
/// which `wlr_xdg_toplevel_set_fullscreen` schedules.
pub(crate) unsafe fn set_fullscreen(server: &mut Server, tl: *mut Toplevel, on: bool) {
    if (*tl).fullscreen == on {
        // Still answer the request (a configure is mandatory either way).
        wlr::wlr_xdg_toplevel_set_fullscreen((*tl).xdg_toplevel, on);
        return;
    }
    (*tl).fullscreen = on;
    wlr::wlr_xdg_toplevel_set_fullscreen((*tl).xdg_toplevel, on);
    let tree = if on { server.tree_fullscreen } else { server.tree_normal };
    oxide_scene_tree_reparent((*tl).scene_tree, tree);
    refresh(server);
    println!("0xide: fullscreen {}", if on { "on" } else { "off" });
}

/// The client asked to enter or leave fullscreen (e.g. F11). Apply whatever
/// it requested; the answer-configure happens inside set_fullscreen.
unsafe extern "C" fn handle_request_fullscreen(userdata: *mut c_void, _data: *mut c_void) {
    let tl = userdata as *mut Toplevel;
    let server = &mut *(*tl).server;
    let want = oxide_xdg_toplevel_requested_fullscreen((*tl).xdg_toplevel);
    set_fullscreen(server, tl, want);
}

/// Every surface commit; only the client's very first one matters here. That
/// initial commit must be answered with a configure (or the client never
/// maps) — and the size we put in it is the client's first real size hint.
/// Answering `0,0` ("pick your own size") lets clients map at their
/// remembered/preferred size — often larger than their tile, spilling across
/// outputs, and some (e.g. browsers) then mishandle the immediate resize that
/// follows on map. Instead, predict the tile this window will get — it joins
/// the end of the active output's workspace, so it takes the last rect of the
/// spiral with one extra window — and send that, so the first frame the
/// client ever draws already fits.
unsafe extern "C" fn handle_commit(userdata: *mut c_void, _data: *mut c_void) {
    let tl = userdata as *mut Toplevel;
    if !oxide_xdg_initial_commit((*tl).xdg_toplevel) {
        return;
    }
    let server = &*(*tl).server;
    let (mut w, mut h) = (0, 0); // 0,0 = client decides (no output to predict from)
    if !server.outputs.is_empty() {
        let o = &server.outputs[active_output(server)];
        let ws = &server.workspaces[o.workspace];
        let tiled = ws.windows.iter().filter(|&&t| !(*t).fullscreen).count();
        let rects = spiral_rects(tiled + 1, o.ux, o.uy, o.uw, o.uh, server.config.gap);
        (_, _, w, h) = *rects.last().unwrap();
    }
    // Tiled state makes the size binding: without it this configure has
    // floating semantics, and clients with a remembered size (Firefox) may
    // use that instead of what we send.
    oxide_xdg_toplevel_set_tiled_all((*tl).xdg_toplevel);
    wlr::wlr_xdg_toplevel_set_size((*tl).xdg_toplevel, w, h);
    println!("0xide: new window — initial configure {w}x{h}");
}

/// A window's surface became mapped: add it to the focused output's workspace,
/// re-tile and focus it.
unsafe extern "C" fn handle_map(userdata: *mut c_void, _data: *mut c_void) {
    let tl = userdata as *mut Toplevel;
    let server = &mut *(*tl).server;
    if server.outputs.is_empty() {
        return; // no monitor to place it on yet
    }
    let a = active_workspace(server);
    server.workspaces[a].windows.push(tl);
    refresh(server);
    focus_index(server, server.workspaces[a].windows.len() - 1);
    println!(
        "0xide: window mapped — ws {} now {} tiled",
        a + 1,
        server.workspaces[a].windows.len()
    );
    // A client may request fullscreen before it maps (e.g. launched with
    // --fullscreen); the request struct is meant to be checked on map.
    if oxide_xdg_toplevel_requested_fullscreen((*tl).xdg_toplevel) {
        set_fullscreen(server, tl, true);
    }
}

/// A window's surface was unmapped (hidden): drop it from the layout.
unsafe extern "C" fn handle_unmap(userdata: *mut c_void, _data: *mut c_void) {
    let tl = userdata as *mut Toplevel;
    let server = &mut *(*tl).server;
    remove_window(server, tl);
}

/// A window was destroyed: unregister its listeners, drop it from the layout,
/// and free our tracking.
unsafe extern "C" fn handle_destroy(userdata: *mut c_void, _data: *mut c_void) {
    let tl = userdata as *mut Toplevel;
    let server = &mut *(*tl).server;
    // Remove every listener we put on this window before wlroots frees it.
    oxide_listener_remove((*tl).commit_listener);
    oxide_listener_remove((*tl).map_listener);
    oxide_listener_remove((*tl).unmap_listener);
    oxide_listener_remove((*tl).destroy_listener);
    oxide_listener_remove((*tl).fullscreen_listener);
    remove_window(server, tl);
    drop(Box::from_raw(tl));
}

/// Remove a window from whichever workspace holds it, then re-tile and focus.
unsafe fn remove_window(server: &mut Server, tl: *mut Toplevel) {
    for ws in server.workspaces.iter_mut() {
        if let Some(pos) = ws.windows.iter().position(|&w| w == tl) {
            ws.windows.remove(pos);
            if ws.focused >= ws.windows.len() && !ws.windows.is_empty() {
                ws.focused = ws.windows.len() - 1;
            }
            break;
        }
    }
    refresh(server);
    if !server.outputs.is_empty() {
        let a = active_workspace(server);
        if !server.workspaces[a].windows.is_empty() {
            let f = server.workspaces[a].focused;
            focus_index(server, f);
        }
    }
}
