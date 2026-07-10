//! Input device hotplug and pointer-driven focus policy.

use crate::ffi::{oxide_handle_new_input, oxide_xdg_toplevel_surface};
use crate::keybindings::handle_keybinding;
use crate::state::Server;
use crate::wlr;
use std::os::raw::c_void;

/// Called by the shim when an input device (keyboard, pointer, …) appears.
pub(crate) unsafe extern "C" fn handle_new_input(userdata: *mut c_void, data: *mut c_void) {
    let server = &mut *(userdata as *mut Server);
    let device = data as *mut wlr::wlr_input_device;
    oxide_handle_new_input(server.seat, server.cursor, device, handle_keybinding, userdata);
}

/// Called by the shim on every click with the clicked root wlr_surface. The
/// shim already moved seat keyboard focus; this keeps `Workspace.focused` in
/// step so close/movefocus/movewindow act on the clicked window, not the last
/// keyboard-focused one. A click on a non-toplevel surface (bar, wallpaper)
/// matches nothing and changes nothing.
pub(crate) unsafe extern "C" fn handle_click_focus(userdata: *mut c_void, data: *mut c_void) {
    let server = &mut *(userdata as *mut Server);
    for ws in server.workspaces.iter_mut() {
        let hit = ws.windows.iter().position(|&tl| oxide_xdg_toplevel_surface((*tl).xdg_toplevel) == data);
        if let Some(idx) = hit {
            ws.focused = idx;
            return;
        }
    }
}
