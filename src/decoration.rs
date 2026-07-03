//! xdg-decoration: force server-side window decoration (no client title bars).

use crate::ffi::oxide_xdg_toplevel_decoration_set_server_side;
use std::os::raw::c_void;

/// Called by the shim when a client creates an xdg-decoration object for one
/// of its toplevels. We always force server-side mode and draw nothing —
/// bare, borderless windows — so there's nothing else to track here.
pub(crate) unsafe extern "C" fn handle_new_decoration(_userdata: *mut c_void, data: *mut c_void) {
    oxide_xdg_toplevel_decoration_set_server_side(data);
}
