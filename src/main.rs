//! snertwl — Stage 0a: prove the FFI toolchain end to end.
//!
//! This binary does no compositing yet. Its only job is to confirm our
//! three-part FFI pipeline works:
//!   1. bindgen turns the wlroots C headers into Rust declarations,
//!   2. the C shim compiles and links against wlroots,
//!   3. the final binary links libwlroots-0.19 and runs.
//! If `cargo run` prints the wlroots version, all three are proven.

// The bindgen output is C-shaped; silence Rust's naming lints for that module.
mod wlr {
    #![allow(non_upper_case_globals, non_camel_case_types, non_snake_case, dead_code)]
    include!(concat!(env!("OUT_DIR"), "/wlr_bindings.rs"));
}

use std::ffi::CStr;
use std::os::raw::c_char;

// Implemented in shim/snertwl_shim.c.
extern "C" {
    fn snertwl_wlroots_version() -> *const c_char;
    fn snertwl_log_init();
}

fn main() {
    // Path 1 — shim → wlroots: bring up wlroots' logger (C side, native enum).
    unsafe { snertwl_log_init() };

    // Path 2 — direct Rust → wlroots via bindgen: read back the verbosity.
    let verbosity = unsafe { wlr::wlr_log_get_verbosity() };

    // Path 3 — shim → wlroots version macro, returned as a C string.
    let version = unsafe { CStr::from_ptr(snertwl_wlroots_version()) }.to_string_lossy();

    println!("snertwl: toolchain alive — linked wlroots {version} (log verbosity = {verbosity})");
}
