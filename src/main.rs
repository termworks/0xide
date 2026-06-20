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

use std::env;
use std::ffi::CStr;
use std::os::raw::c_void;
use std::process::Command;
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
    fn snertwl_scene_add_output_background(
        scene: *mut wlr::wlr_scene,
        output: *mut wlr::wlr_output,
        r: f32,
        g: f32,
        b: f32,
    );
    fn snertwl_scene_output_render(scene_output: *mut wlr::wlr_scene_output);
}

/// Long-lived compositor state. We hand a pointer to this to the shim as the
/// `userdata` for the new_output callback, so the handler can reach the scene
/// and layout it needs to wire up each output.
struct Server {
    scene: *mut wlr::wlr_scene,
    output_layout: *mut wlr::wlr_output_layout,
    scene_layout: *mut wlr::wlr_scene_output_layout,
    renderer: *mut wlr::wlr_renderer,
    allocator: *mut wlr::wlr_allocator,
}

/// Called by the shim when the backend produces an output (one window, here).
unsafe extern "C" fn handle_new_output(userdata: *mut c_void, data: *mut c_void) {
    let server = &mut *(userdata as *mut Server);
    let output = data as *mut wlr::wlr_output;

    // Give the output our renderer + allocator so it can produce buffers, then
    // enable it (the shim owns the wlr_output_state dance).
    wlr::wlr_output_init_render(output, server.allocator, server.renderer);
    snertwl_output_enable(output);

    // Place the output in the layout, and tie that layout slot to a scene
    // output so the scene knows where this output sits and what to repaint.
    let layout_output = wlr::wlr_output_layout_add_auto(server.output_layout, output);
    let scene_output = wlr::wlr_scene_output_create(server.scene, output);
    wlr::wlr_scene_output_layout_add_output(server.scene_layout, layout_output, scene_output);

    // The teal background is now a node in the scene graph, sized to the output.
    let (r, g, b) = COLOR;
    snertwl_scene_add_output_background(server.scene, output, r, g, b);

    // Render through the scene on every frame; pass the scene output along.
    snertwl_output_add_frame(output, handle_frame, scene_output as *mut c_void);
    snertwl_scene_output_render(scene_output); // kick the first frame

    println!("snertwl: output online — scene attached");
}

/// Called by the shim each time the output is ready for a new frame.
unsafe extern "C" fn handle_frame(userdata: *mut c_void, _data: *mut c_void) {
    let scene_output = userdata as *mut wlr::wlr_scene_output;
    snertwl_scene_output_render(scene_output);
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

        // Buffer-factory globals: wl_shm + linux-dmabuf. Clients need these to
        // hand us pixel buffers; without them no app can show anything.
        wlr::wlr_renderer_init_wl_display(renderer, display);

        // Core client-facing globals: surfaces/regions, subsurfaces, clipboard.
        wlr::wlr_compositor_create(display, 6, renderer);
        wlr::wlr_subcompositor_create(display);
        wlr::wlr_data_device_manager_create(display);

        // The scene graph holds everything that gets drawn; the output layout
        // arranges outputs in space. Attaching them lets the scene keep each
        // scene-output positioned to match its layout slot.
        let scene = wlr::wlr_scene_create();
        let output_layout = wlr::wlr_output_layout_create(display);
        let scene_layout = wlr::wlr_scene_attach_output_layout(scene, output_layout);

        // `server` lives for the whole of main(), which blocks in wl_display_run
        // below, so the pointer we hand the shim stays valid for the run.
        let mut server = Server {
            scene,
            output_layout,
            scene_layout,
            renderer,
            allocator,
        };
        snertwl_backend_add_new_output(
            backend,
            handle_new_output,
            &mut server as *mut Server as *mut c_void,
        );

        // Open the Unix socket clients connect through (e.g. "wayland-2").
        let socket_ptr = wlr::wl_display_add_socket_auto(display);
        assert!(!socket_ptr.is_null(), "failed to open a Wayland socket");
        let socket = CStr::from_ptr(socket_ptr).to_str().unwrap().to_owned();

        assert!(wlr::wlr_backend_start(backend), "failed to start backend");
        println!("snertwl: socket ready — WAYLAND_DISPLAY={socket}");

        // Clients we spawn should talk to *us*, not the host compositor. (Our
        // own backend already connected to the host before this point.)
        env::set_var("WAYLAND_DISPLAY", &socket);

        // `cargo nested -- <cmd> [args…]` auto-spawns a test client against us.
        let mut args = env::args().skip(1);
        if let Some(program) = args.next() {
            match Command::new(&program).args(args).spawn() {
                Ok(_) => println!("snertwl: spawned client `{program}`"),
                Err(e) => eprintln!("snertwl: failed to spawn `{program}`: {e}"),
            }
        }

        println!("snertwl: entering event loop (Ctrl-C to quit)");
        wlr::wl_display_run(display);
        wlr::wl_display_destroy(display);
    }
}
