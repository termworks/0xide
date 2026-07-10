# Environment & toolchain

0xide is built and run on **Arch Linux**, as ordinary stable-Rust userspace
(no `no_std`, no custom target) — the toolchain is pinned in
`rust-toolchain.toml` so a fresh checkout always builds with the exact
version it was developed against.

## System dependencies

```
wlroots0.19 wayland wayland-protocols libxkbcommon libinput libdrm seatd mesa pixman pkgconf clang
```

The version that matters most is **wlroots**: 0xide targets wlroots **0.19**
specifically (Arch package `wlroots0.19`, pkg-config name `wlroots-0.19`).
wlroots' API moves between minor versions, so this pin isn't cosmetic —
`build.rs` resolves flags via `pkg-config wlroots-0.19` rather than a bare
`wlroots`, and every wlroots header requires `-DWLR_USE_UNSTABLE` defined or
it expands to `#error` (wlroots treats most of its own API as unstable by
design; the flag is an explicit "I know" acknowledgement, not a mistake to
work around).

`clang`/`libclang` is a build dependency, not a runtime one — `bindgen` needs
it to parse the wlroots C headers into Rust FFI declarations.

## The FFI pipeline

`build.rs` does four things, in order, every build:

1. Resolves wlroots/wayland/etc. include and link flags via `pkg-config`.
2. Generates the `xdg-shell` protocol header with `wayland-scanner` into
   `OUT_DIR` (wlroots' own xdg-shell header `#include`s this, and it isn't a
   system header — it has to be generated from the protocol XML on every
   machine that builds 0xide).
3. Compiles the C shim (`shim/*.c`) via the `cc` crate.
4. Runs `bindgen` over `wrapper.h`, allowlisting only the functions/types
   0xide actually calls (see [Architecture](architecture.md) for why the
   allowlist exists and what it means for opaque struct types).

See [`build.rs`](https://github.com/termworks/0xide/blob/main/build.rs) for the
exact allowlist and flag wiring.

## Running it

Two run modes, both via cargo aliases in `.cargo/config.toml`:

- **`cargo nested`** — the fast dev loop. Inside an existing Wayland session,
  `wlr_backend_autocreate` picks the nested Wayland backend automatically and
  0xide opens as an ordinary window on the host desktop. `OXIDE_MOD=alt cargo
  nested -- kitty` sets the modifier to Alt (since the host compositor
  usually grabs Super-chords before a nested client sees them) and launches a
  test client against 0xide's own socket.
- **Real TTY (DRM/KMS)** — from a free virtual terminal, logged in:
  `LIBSEAT_BACKEND=logind ~/Projects/0xide/target/debug/0xide kitty
  2>~/0xide-tty.log`. `wlr_backend_autocreate` detects there's no
  `WAYLAND_DISPLAY` and picks the DRM/KMS backend instead — this is 0xide as
  a real session, not a nested toy. `LIBSEAT_BACKEND=logind` lets logind hand
  the active VT its devices without needing the `seat` group.

Full recipes, verification commands, and known gotchas (multi-GPU device
selection, VT-switch repaint behavior, headless screenshot verification) live
in [Running & Verifying](running.md) and the in-repo
[`notes/`](https://github.com/termworks/0xide/tree/main/notes) directory, which
is the day-to-day working reference this chapter is distilled from.
