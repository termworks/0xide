//! Build script: the FFI pipeline for snertwl.
//!
//! 1. Locate wlroots 0.19 + wayland-server via pkg-config (emits linker flags).
//! 2. Generate the xdg-shell protocol server header with wayland-scanner —
//!    wlroots' xdg headers #include it, and it isn't shipped as a system header.
//! 3. Compile our C shim against those headers.
//! 4. Run bindgen over wrapper.h for the functions Rust calls directly.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // 1. Find the libraries. `.probe()` returns include paths AND prints the
    //    cargo directives that link `-lwlroots-0.19` / `-lwayland-server`.
    let wlroots = pkg_config::Config::new()
        .probe("wlroots-0.19")
        .expect("wlroots-0.19 not found via pkg-config");
    let wayland = pkg_config::Config::new()
        .probe("wayland-server")
        .expect("wayland-server not found via pkg-config");
    // Our shim calls xkbcommon directly (keymap compilation), so link it.
    let xkbcommon = pkg_config::Config::new()
        .probe("xkbcommon")
        .expect("xkbcommon not found via pkg-config");

    // 2. Generate xdg-shell-protocol.h into OUT_DIR. wlroots' xdg-shell header
    //    #includes it; wayland-scanner produces it from the protocol XML.
    let protocols_dir = pkg_config::get_variable("wayland-protocols", "pkgdatadir")
        .expect("wayland-protocols pkgdatadir not found");
    let xdg_xml = format!("{protocols_dir}/stable/xdg-shell/xdg-shell.xml");
    let xdg_header = out_dir.join("xdg-shell-protocol.h");
    let status = Command::new("wayland-scanner")
        .arg("server-header")
        .arg(&xdg_xml)
        .arg(&xdg_header)
        .status()
        .expect("failed to run wayland-scanner");
    assert!(status.success(), "wayland-scanner failed on {xdg_xml}");

    // Include dirs shared by the shim build (cc) and bindgen (clang): the
    // library headers plus OUT_DIR for our generated protocol header.
    let mut include_paths: Vec<PathBuf> = wlroots
        .include_paths
        .iter()
        .chain(wayland.include_paths.iter())
        .chain(xkbcommon.include_paths.iter())
        .cloned()
        .collect();
    include_paths.push(out_dir.clone());

    // 3. Compile the C shim. WLR_USE_UNSTABLE is set inside the .c itself.
    let mut shim = cc::Build::new();
    shim.file("shim/snertwl_shim.c");
    for path in &include_paths {
        shim.include(path);
    }
    shim.compile("snertwl_shim");

    // 4. Generate Rust bindings. We allowlist exactly the functions Rust calls
    //    directly; bindgen pulls in the types they reference automatically. The
    //    listener glue and helpers stay in C (declared in snertwl_shim.h).
    let clang_args: Vec<String> = include_paths
        .iter()
        .map(|p| format!("-I{}", p.display()))
        .collect();

    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_args(&clang_args)
        .allowlist_function("wl_display_create")
        .allowlist_function("wl_display_get_event_loop")
        .allowlist_function("wl_display_add_socket_auto")
        .allowlist_function("wl_display_run")
        .allowlist_function("wl_display_terminate")
        .allowlist_function("wl_display_destroy_clients")
        .allowlist_function("wlr_backend_autocreate")
        .allowlist_function("wlr_backend_start")
        .allowlist_function("wlr_backend_destroy")
        .allowlist_function("wlr_renderer_autocreate")
        .allowlist_function("wlr_renderer_init_wl_display")
        .allowlist_function("wlr_allocator_autocreate")
        .allowlist_function("wlr_output_init_render")
        .allowlist_function("wlr_output_layout_create")
        .allowlist_function("wlr_output_layout_add_auto")
        .allowlist_function("wlr_scene_create")
        .allowlist_function("wlr_scene_attach_output_layout")
        .allowlist_function("wlr_scene_output_create")
        .allowlist_function("wlr_scene_output_layout_add_output")
        .allowlist_function("wlr_compositor_create")
        .allowlist_function("wlr_subcompositor_create")
        .allowlist_function("wlr_data_device_manager_create")
        .allowlist_function("wlr_xdg_shell_create")
        .allowlist_function("wlr_xdg_toplevel_set_size")
        .allowlist_function("wlr_xdg_toplevel_send_close")
        .allowlist_type("wlr_xdg_toplevel")
        .allowlist_type("wlr_session")
        .allowlist_type("wlr_seat")
        .allowlist_type("wlr_input_device")
        .allowlist_type("wlr_cursor")
        .allowlist_function("wlr_log.*")
        .layout_tests(false)
        .generate()
        .expect("bindgen failed to generate wlroots bindings");

    bindings
        .write_to_file(out_dir.join("wlr_bindings.rs"))
        .expect("failed to write wlr_bindings.rs");

    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-changed=shim/snertwl_shim.c");
    println!("cargo:rerun-if-changed=shim/snertwl_shim.h");
}
