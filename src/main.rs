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
use std::os::raw::{c_char, c_void};
use std::process::Command;
use std::ptr;

/// The color we paint every output. A saturated teal, easy to spot.
const COLOR: (f32, f32, f32) = (0.0, 0.6, 0.6);

/// Type of the callbacks our C shim invokes: (userdata, signal-data).
type ShimCallback = unsafe extern "C" fn(*mut c_void, *mut c_void);

/// Keybinding callback: (userdata, keysym, modifiers) -> was it consumed?
type KeyCallback = unsafe extern "C" fn(*mut c_void, u32, u32) -> bool;

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
    fn snertwl_xdg_shell_add_new_toplevel(
        shell: *mut wlr::wlr_xdg_shell,
        callback: ShimCallback,
        userdata: *mut c_void,
    ) -> *mut ShimListener;
    fn snertwl_scene_add_xdg_toplevel(
        scene: *mut wlr::wlr_scene,
        toplevel: *mut wlr::wlr_xdg_toplevel,
    ) -> *mut wlr::wlr_scene_tree;
    fn snertwl_xdg_add_map(
        toplevel: *mut wlr::wlr_xdg_toplevel,
        callback: ShimCallback,
        userdata: *mut c_void,
    ) -> *mut ShimListener;
    fn snertwl_xdg_add_unmap(
        toplevel: *mut wlr::wlr_xdg_toplevel,
        callback: ShimCallback,
        userdata: *mut c_void,
    ) -> *mut ShimListener;
    fn snertwl_xdg_add_destroy(
        toplevel: *mut wlr::wlr_xdg_toplevel,
        callback: ShimCallback,
        userdata: *mut c_void,
    ) -> *mut ShimListener;
    fn snertwl_scene_tree_set_position(tree: *mut wlr::wlr_scene_tree, x: i32, y: i32);
    fn snertwl_focus_toplevel(
        seat: *mut wlr::wlr_seat,
        toplevel: *mut wlr::wlr_xdg_toplevel,
    );
    fn snertwl_output_get_size(
        output: *mut wlr::wlr_output,
        width: *mut i32,
        height: *mut i32,
    );
    fn snertwl_seat_create(
        display: *mut wlr::wl_display,
        name: *const c_char,
    ) -> *mut wlr::wlr_seat;
    fn snertwl_backend_add_new_input(
        backend: *mut wlr::wlr_backend,
        callback: ShimCallback,
        userdata: *mut c_void,
    ) -> *mut ShimListener;
    fn snertwl_handle_new_input(
        seat: *mut wlr::wlr_seat,
        cursor: *mut wlr::wlr_cursor,
        device: *mut wlr::wlr_input_device,
        key_callback: KeyCallback,
        key_userdata: *mut c_void,
    );
    fn snertwl_cursor_setup(
        layout: *mut wlr::wlr_output_layout,
        scene: *mut wlr::wlr_scene,
        seat: *mut wlr::wlr_seat,
    ) -> *mut wlr::wlr_cursor;
}

/// Long-lived compositor state. We hand a pointer to this to the shim as the
/// `userdata` for the new_output callback, so the handler can reach the scene
/// and layout it needs to wire up each output.
struct Server {
    display: *mut wlr::wl_display,
    scene: *mut wlr::wlr_scene,
    output_layout: *mut wlr::wlr_output_layout,
    scene_layout: *mut wlr::wlr_scene_output_layout,
    seat: *mut wlr::wlr_seat,
    cursor: *mut wlr::wlr_cursor,
    renderer: *mut wlr::wlr_renderer,
    allocator: *mut wlr::wlr_allocator,
    /// Mapped windows, in stacking order. Pointers to heap-allocated Toplevels.
    windows: Vec<*mut Toplevel>,
    /// Index into `windows` of the focused window.
    focused: usize,
    /// Modifier that triggers keybindings (Super by default; Alt for nested dev).
    modkey: u32,
    /// Size of the (single, for now) output we tile within.
    output_width: i32,
    output_height: i32,
}

/// One application window we track. Heap-allocated; a raw pointer to it is the
/// `userdata` for that window's map/unmap/destroy listeners.
struct Toplevel {
    server: *mut Server,
    xdg_toplevel: *mut wlr::wlr_xdg_toplevel,
    scene_tree: *mut wlr::wlr_scene_tree,
}

/// Gap between/around tiled windows, in pixels.
const GAP: i32 = 10;

/// Arrange all mapped windows as equal-width columns across the output.
unsafe fn relayout(server: &mut Server) {
    let n = server.windows.len() as i32;
    if n == 0 {
        return;
    }
    let col_w = ((server.output_width - GAP * (n + 1)) / n).max(1);
    let h = (server.output_height - GAP * 2).max(1);
    for (i, &tl) in server.windows.iter().enumerate() {
        let x = GAP + (col_w + GAP) * i as i32;
        snertwl_scene_tree_set_position((*tl).scene_tree, x, GAP);
        wlr::wlr_xdg_toplevel_set_size((*tl).xdg_toplevel, col_w, h);
    }
}

// --- keybindings -----------------------------------------------------------

/// Modifier bits (mirror the WLR_MODIFIER_* enum).
const MOD_LOGO: u32 = 1 << 6; // Super
const MOD_ALT: u32 = 1 << 3; // Alt

// xkb keysyms. Latin-1 letters equal their ASCII codes; named keys are 0xff..
const KEY_RETURN: u32 = 0xff0d;
const KEY_Q: u32 = b'q' as u32;
const KEY_Q_SHIFT: u32 = b'Q' as u32;
const KEY_J: u32 = b'j' as u32;
const KEY_K: u32 = b'k' as u32;

/// Give keyboard focus to window `idx` (wrapped into range).
unsafe fn focus_index(server: &mut Server, idx: usize) {
    if server.windows.is_empty() {
        return;
    }
    let i = idx % server.windows.len();
    server.focused = i;
    snertwl_focus_toplevel(server.seat, (*server.windows[i]).xdg_toplevel);
}

/// Ask the focused window to close.
unsafe fn close_focused(server: &Server) {
    if let Some(&tl) = server.windows.get(server.focused) {
        wlr::wlr_xdg_toplevel_send_close((*tl).xdg_toplevel);
    }
}

/// Launch a program as a client of snertwl (inherits our WAYLAND_DISPLAY).
fn spawn(program: &str) {
    if let Err(e) = Command::new(program).spawn() {
        eprintln!("snertwl: failed to spawn `{program}`: {e}");
    }
}

/// Called by the shim for each key press; returns true to consume the key.
/// All bindings here are Super-chords; everything else falls through to apps.
unsafe extern "C" fn handle_keybinding(userdata: *mut c_void, keysym: u32, modifiers: u32) -> bool {
    let server = &mut *(userdata as *mut Server);
    if modifiers & server.modkey == 0 {
        return false;
    }
    let n = server.windows.len();
    match keysym {
        KEY_RETURN => spawn("foot"),
        KEY_Q => close_focused(server),
        KEY_Q_SHIFT => wlr::wl_display_terminate(server.display),
        KEY_J if n > 0 => focus_index(server, server.focused + 1),
        KEY_K if n > 0 => focus_index(server, server.focused + n - 1),
        _ => return false,
    }
    true
}

/// Called by the shim when the backend produces an output (one window, here).
unsafe extern "C" fn handle_new_output(userdata: *mut c_void, data: *mut c_void) {
    let server = &mut *(userdata as *mut Server);
    let output = data as *mut wlr::wlr_output;

    // Give the output our renderer + allocator so it can produce buffers, then
    // enable it (the shim owns the wlr_output_state dance).
    wlr::wlr_output_init_render(output, server.allocator, server.renderer);
    snertwl_output_enable(output);

    // Remember the output size so the tiling layout knows its bounds.
    snertwl_output_get_size(output, &mut server.output_width, &mut server.output_height);

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

/// Called by the shim when a client creates an application window (toplevel).
unsafe extern "C" fn handle_new_toplevel(userdata: *mut c_void, data: *mut c_void) {
    let server = userdata as *mut Server;
    let toplevel = data as *mut wlr::wlr_xdg_toplevel;

    // Give it a scene node, then track it in Rust. We don't add it to the
    // layout yet — that happens on map, when it actually has content.
    let scene_tree = snertwl_scene_add_xdg_toplevel((*server).scene, toplevel);
    let tl = Box::into_raw(Box::new(Toplevel {
        server,
        xdg_toplevel: toplevel,
        scene_tree,
    }));

    // Listen for its lifecycle so Rust can keep the window list current.
    let ud = tl as *mut c_void;
    snertwl_xdg_add_map(toplevel, handle_map, ud);
    snertwl_xdg_add_unmap(toplevel, handle_unmap, ud);
    snertwl_xdg_add_destroy(toplevel, handle_destroy, ud);
}

/// A window's surface became mapped: add it to the layout and focus it.
unsafe extern "C" fn handle_map(userdata: *mut c_void, _data: *mut c_void) {
    let tl = userdata as *mut Toplevel;
    let server = &mut *(*tl).server;
    server.windows.push(tl);
    relayout(server);
    focus_index(server, server.windows.len() - 1);
    println!("snertwl: window mapped — {} tiled", server.windows.len());
}

/// A window's surface was unmapped (hidden): drop it from the layout.
unsafe extern "C" fn handle_unmap(userdata: *mut c_void, _data: *mut c_void) {
    let tl = userdata as *mut Toplevel;
    let server = &mut *(*tl).server;
    remove_window(server, tl);
}

/// A window was destroyed: drop it from the layout and free our tracking.
unsafe extern "C" fn handle_destroy(userdata: *mut c_void, _data: *mut c_void) {
    let tl = userdata as *mut Toplevel;
    let server = &mut *(*tl).server;
    remove_window(server, tl);
    drop(Box::from_raw(tl));
}

/// Remove a window from the layout and re-focus a remaining one.
unsafe fn remove_window(server: &mut Server, tl: *mut Toplevel) {
    server.windows.retain(|&w| w != tl);
    relayout(server);
    if !server.windows.is_empty() {
        focus_index(server, server.focused.min(server.windows.len() - 1));
    }
}

/// Called by the shim when an input device (keyboard, pointer, …) appears.
unsafe extern "C" fn handle_new_input(userdata: *mut c_void, data: *mut c_void) {
    let server = &mut *(userdata as *mut Server);
    let device = data as *mut wlr::wlr_input_device;
    snertwl_handle_new_input(server.seat, server.cursor, device, handle_keybinding, userdata);
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

        // Create the seat (wl_seat global). We wire input devices into it below.
        let seat = snertwl_seat_create(display, c"seat0".as_ptr());

        // The scene graph holds everything that gets drawn; the output layout
        // arranges outputs in space. Attaching them lets the scene keep each
        // scene-output positioned to match its layout slot.
        let scene = wlr::wlr_scene_create();
        let output_layout = wlr::wlr_output_layout_create(display);
        let scene_layout = wlr::wlr_scene_attach_output_layout(scene, output_layout);

        // Cursor over the layout; the shim routes its events through scene
        // hit-testing to the seat. Pointer devices get attached in new_input.
        let cursor = snertwl_cursor_setup(output_layout, scene, seat);

        // Keybinding modifier: Super by default; `SNERTWL_MOD=alt` for nested
        // dev (a nesting host like Hyprland grabs Super-chords before us).
        let modkey = match env::var("SNERTWL_MOD").as_deref() {
            Ok("alt") => MOD_ALT,
            _ => MOD_LOGO,
        };
        println!(
            "snertwl: keybinding modifier = {}",
            if modkey == MOD_ALT { "Alt" } else { "Super" }
        );

        // `server` lives for the whole of main(), which blocks in wl_display_run
        // below, so the pointer we hand the shim stays valid for the run.
        let mut server = Server {
            display,
            scene,
            output_layout,
            scene_layout,
            seat,
            cursor,
            renderer,
            allocator,
            windows: Vec::new(),
            focused: 0,
            modkey,
            output_width: 0,
            output_height: 0,
        };
        let server_ptr = &mut server as *mut Server as *mut c_void;
        snertwl_backend_add_new_output(backend, handle_new_output, server_ptr);
        snertwl_backend_add_new_input(backend, handle_new_input, server_ptr);

        // xdg-shell: the xdg_wm_base global apps bind to create windows. We hook
        // its new_toplevel signal so each app window enters our scene graph.
        let xdg_shell = wlr::wlr_xdg_shell_create(display, 6);
        snertwl_xdg_shell_add_new_toplevel(xdg_shell, handle_new_toplevel, server_ptr);

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
