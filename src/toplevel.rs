//! xdg-shell application windows: creation, map/unmap/destroy, removal.

use crate::ffi::*;
use crate::keybindings::focus_index;
use crate::state::*;
use crate::tiling::{active_workspace, refresh};
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
        commit_listener: ptr::null_mut(),
        map_listener: ptr::null_mut(),
        unmap_listener: ptr::null_mut(),
        destroy_listener: ptr::null_mut(),
    }));

    // Listen for its lifecycle so Rust can keep the window list current. We keep
    // the listener handles to unregister them on destroy.
    let ud = tl as *mut c_void;
    (*tl).commit_listener = oxide_xdg_add_commit(toplevel);
    (*tl).map_listener = oxide_xdg_add_map(toplevel, handle_map, ud);
    (*tl).unmap_listener = oxide_xdg_add_unmap(toplevel, handle_unmap, ud);
    (*tl).destroy_listener = oxide_xdg_add_destroy(toplevel, handle_destroy, ud);
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
