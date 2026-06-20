//! Build script: the FFI pipeline for snertwl.
//!
//! 1. Locate wlroots 0.19 + wayland-server via pkg-config (emits linker flags).
//! 2. Compile our C shim (shim/snertwl_shim.c) against those headers.
//! 3. Run bindgen over wrapper.h to produce Rust declarations for the wlroots
//!    functions we call directly.

use std::env;
use std::path::PathBuf;

fn main() {
    // 1. Find the libraries. `.probe()` returns include paths AND prints the
    //    cargo directives that link `-lwlroots-0.19` / `-lwayland-server`.
    let wlroots = pkg_config::Config::new()
        .probe("wlroots-0.19")
        .expect("wlroots-0.19 not found via pkg-config");
    let wayland = pkg_config::Config::new()
        .probe("wayland-server")
        .expect("wayland-server not found via pkg-config");

    // Include dirs shared by the shim build (cc) and bindgen (clang).
    let include_paths: Vec<_> = wlroots
        .include_paths
        .iter()
        .chain(wayland.include_paths.iter())
        .cloned()
        .collect();

    // 2. Compile the C shim. WLR_USE_UNSTABLE is set inside the .c itself.
    let mut shim = cc::Build::new();
    shim.file("shim/snertwl_shim.c");
    for path in &include_paths {
        shim.include(path);
    }
    shim.compile("snertwl_shim");

    // 3. Generate Rust bindings. We allowlist exactly the functions Rust calls
    //    directly; bindgen pulls in the types they reference automatically. The
    //    listener glue and render helpers stay in C (declared in snertwl_shim.h).
    let clang_args: Vec<String> = include_paths
        .iter()
        .map(|p| format!("-I{}", p.display()))
        .collect();

    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_args(&clang_args)
        .allowlist_function("wl_display_create")
        .allowlist_function("wl_display_get_event_loop")
        .allowlist_function("wl_display_run")
        .allowlist_function("wl_display_destroy")
        .allowlist_function("wlr_backend_autocreate")
        .allowlist_function("wlr_backend_start")
        .allowlist_function("wlr_backend_destroy")
        .allowlist_function("wlr_renderer_autocreate")
        .allowlist_function("wlr_allocator_autocreate")
        .allowlist_function("wlr_output_init_render")
        .allowlist_function("wlr_log.*")
        .layout_tests(false)
        .generate()
        .expect("bindgen failed to generate wlroots bindings");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_dir.join("wlr_bindings.rs"))
        .expect("failed to write wlr_bindings.rs");

    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-changed=shim/snertwl_shim.c");
    println!("cargo:rerun-if-changed=shim/snertwl_shim.h");
}
