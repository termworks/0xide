//! Build script: the FFI pipeline for snertwl.
//!
//! Three jobs, in order:
//!   1. Locate wlroots 0.19 via pkg-config (this also emits the linker flags).
//!   2. Compile our C shim (shim/snertwl_shim.c) against the wlroots headers.
//!   3. Run bindgen over wrapper.h to produce Rust declarations for wlroots.

use std::env;
use std::path::PathBuf;

fn main() {
    // 1. Find wlroots 0.19. `.probe()` returns its include paths AND prints the
    //    cargo directives that tell rustc to link `-lwlroots-0.19` (and friends).
    let wlroots = pkg_config::Config::new()
        .probe("wlroots-0.19")
        .expect("wlroots-0.19 not found via pkg-config");

    // 2. Compile the C shim. It needs the wlroots include dirs; the
    //    WLR_USE_UNSTABLE gate the headers demand is set inside the .c itself.
    //    cc links the result into the final binary as a static lib.
    let mut shim = cc::Build::new();
    shim.file("shim/snertwl_shim.c");
    for path in &wlroots.include_paths {
        shim.include(path);
    }
    shim.compile("snertwl_shim");

    // 3. Generate Rust bindings from wrapper.h. We hand clang the wlroots include
    //    dirs (the WLR_USE_UNSTABLE define lives in wrapper.h), then allowlist only
    //    the logging/version surface so the output stays small while we learn.
    let clang_args: Vec<String> = wlroots
        .include_paths
        .iter()
        .map(|p| format!("-I{}", p.display()))
        .collect();

    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_args(&clang_args)
        .allowlist_function("wlr_log.*")
        .allowlist_type("wlr_log.*")
        .layout_tests(false)
        .generate()
        .expect("bindgen failed to generate wlroots bindings");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_dir.join("wlr_bindings.rs"))
        .expect("failed to write wlr_bindings.rs");

    // Re-run this script when any FFI surface changes.
    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-changed=shim/snertwl_shim.c");
    println!("cargo:rerun-if-changed=shim/snertwl_shim.h");
}
