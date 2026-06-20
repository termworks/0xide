//! snertwl — Stage 0b: open a backend and clear the screen to a solid color.
//!
//! This is the smallest real compositor: it brings up a wlroots backend (a
//! nested Wayland window, since we run inside a Wayland session), a renderer and
//! allocator, then paints every output a fixed color. No clients, no input yet.

// The bindgen output is C-shaped; silence Rust's naming lints for that module.
mod wlr {
    #![allow(non_upper_case_globals, non_camel_case_types, non_snake_case, dead_code)]
    include!(concat!(env!("OUT_DIR"), "/wlr_bindings.rs"));
}

use std::os::raw::c_void;
use std::ptr;

/// The color we paint every output. A saturated teal, easy to spot.
const COLOR: (f32, f32, f32) = (0.0, 0.6, 0.6);

/// Type of the callbacks our C shim invokes: (userdata, signal-data).
type ShimCallback = unsafe extern "C" fn(*mut c_void, *mut c_void);

/// Opaque handle to a `snertwl_listener` living on the C heap.
#[repr(C)]
struct ShimListener {
    _opaque: [u8; 0],
}

// Functions implemented in shim/snertwl_shim.c.
extern "C" {
    fn snertwl_log_init();
    fn snertwl_backend_add_new_output(
        backend: *mut wlr::wlr_backend,
        callback: ShimCallback,
        userdata: *mut c_void,
    ) -> *mut ShimListener;
    fn snertwl_output_add_frame(
        output: *mut wlr::wlr_output,
        callback: ShimCallback,
        userdata: *mut c_void,
    ) -> *mut ShimListener;
    fn snertwl_output_enable(output: *mut wlr::wlr_output);
    fn snertwl_output_render_clear(output: *mut wlr::wlr_output, r: f32, g: f32, b: f32);
}

/// Long-lived compositor state. We hand a pointer to this to the shim as the
/// `userdata` for the new_output callback, so the handler can reach the
/// renderer/allocator it needs to wire up each output.
struct Server {
    renderer: *mut wlr::wlr_renderer,
    allocator: *mut wlr::wlr_allocator,
}

/// Called by the shim when the backend produces an output (one window, here).
unsafe extern "C" fn handle_new_output(userdata: *mut c_void, data: *mut c_void) {
    let server = &mut *(userdata as *mut Server);
    let output = data as *mut wlr::wlr_output;

    // Give the output our renderer + allocator so it can produce buffers.
    wlr::wlr_output_init_render(output, server.allocator, server.renderer);

    // Enable it (the shim owns the wlr_output_state dance).
    snertwl_output_enable(output);

    // Re-paint on every frame request; pass the output itself as userdata.
    snertwl_output_add_frame(output, handle_frame, output as *mut c_void);

    // Paint once now so the window isn't blank before the first frame event.
    let (r, g, b) = COLOR;
    snertwl_output_render_clear(output, r, g, b);

    println!("snertwl: output online — painting teal");
}

/// Called by the shim each time the output is ready for a new frame.
unsafe extern "C" fn handle_frame(userdata: *mut c_void, _data: *mut c_void) {
    let output = userdata as *mut wlr::wlr_output;
    let (r, g, b) = COLOR;
    snertwl_output_render_clear(output, r, g, b);
}

fn main() {
    unsafe {
        snertwl_log_init();

        // The display owns the event loop and (later) the client socket.
        let display = wlr::wl_display_create();
        let event_loop = wlr::wl_display_get_event_loop(display);

        // Autocreate picks a backend from the environment: a nested Wayland
        // window here. NULL = we don't want the optional session handle.
        let backend = wlr::wlr_backend_autocreate(event_loop, ptr::null_mut());
        assert!(!backend.is_null(), "failed to create wlr_backend");

        let renderer = wlr::wlr_renderer_autocreate(backend);
        assert!(!renderer.is_null(), "failed to create wlr_renderer");

        let allocator = wlr::wlr_allocator_autocreate(backend, renderer);
        assert!(!allocator.is_null(), "failed to create wlr_allocator");

        // `server` lives for the whole of main(), which blocks in wl_display_run
        // below, so the pointer we hand the shim stays valid for the run.
        let mut server = Server {
            renderer,
            allocator,
        };
        snertwl_backend_add_new_output(
            backend,
            handle_new_output,
            &mut server as *mut Server as *mut c_void,
        );

        assert!(wlr::wlr_backend_start(backend), "failed to start backend");
        println!("snertwl: backend started — entering event loop (Ctrl-C to quit)");

        wlr::wl_display_run(display);
        wlr::wl_display_destroy(display);
    }
}
