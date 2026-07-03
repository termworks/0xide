//! Input device hotplug: hands new keyboards/pointers to the shim.

use crate::ffi::oxide_handle_new_input;
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
