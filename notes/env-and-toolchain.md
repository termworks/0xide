# Environment & toolchain (verified 2026-06-20)

- Host: Arch Linux, running inside a Wayland session (`WAYLAND_DISPLAY=wayland-1`)
  → the nested Wayland backend is our fast dev loop.
- Rust: pinned **1.96.0** via `rust-toolchain.toml`.
- wlroots: **0.19.3**, Arch package `wlroots0.19`, pkg-config name **`wlroots-0.19`**.
  - include dirs: `/usr/include/wlroots-0.19 /usr/include/pixman-1 /usr/include/libdrm`
  - link flag: `-lwlroots-0.19`
  - **All wlroots headers require `-DWLR_USE_UNSTABLE`** or they expand to `#error`.
- FFI strategy: **bindgen** (types/functions) + a **thin C shim** in `shim/` for the
  `wl_listener`/`wl_signal` glue that is unsafe/awkward over raw FFI.
- bindgen needs libclang: `/usr/lib/libclang.so` (clang 22).
- Test clients: `foot` (terminal, Stage 3); `wayland-info` from `wayland-utils` (Stage 1).
- Other dev libs already present: wayland 1.25, wayland-protocols 1.49, xkbcommon 1.13,
  libinput 1.31, libdrm 2.4.134, libseat 0.9.3, seatd, pixman 0.46, mesa EGL/GLESv2.
